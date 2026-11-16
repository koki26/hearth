use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Context, bail};
use hearth_protocol::{
    ALPN, Control, Datagram, DecodeError, HUB_INSTANCE, MAX_HEALTH, NOBODY, PROTOCOL_VERSION, Peer,
    RejectReason, World, dequantize_pos, ember, flags, welcome,
};
use quinn::crypto::rustls::QuicServerConfig;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use serde::Deserialize;
use tokio::sync::{mpsc, watch};
use tokio::time::timeout;
use tracing::{debug, info, warn};

pub mod command;
pub mod log;
pub mod store;
pub mod worlds;

use worlds::Repository;

pub const TICK_HZ: u8 = 20;
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);
/// Certificate subject name; clients use it as the TLS server name.
pub const CERT_NAME: &str = "hearth";

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub port: u16,
    pub max_players: u16,
    pub data_dir: PathBuf,
    /// Where the packed worlds live. Defaults to `data_dir/worlds`.
    pub worlds_dir: Option<PathBuf>,
    /// The world everybody spawns into. Instance 0, never stops.
    pub hub_world: String,
    /// Whether players may start their own instance of a world.
    pub client_instancing: bool,
    pub max_instances: u16,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            port: 7777,
            max_players: 16,
            data_dir: "data".into(),
            worlds_dir: None,
            hub_world: "hub".into(),
            client_instancing: false,
            max_instances: 8,
        }
    }
}

impl Config {
    pub fn parse(text: &str) -> anyhow::Result<Self> {
        let config: Self = toml::from_str(text)?;
        if config.max_players == 0 {
            bail!("max_players must be at least 1");
        }
        if config.max_instances == 0 {
            bail!("max_instances must be at least 1 (the hub)");
        }
        hearth_protocol::validate_slug(&config.hub_world)
            .context("hub_world must be a world name: a-z, 0-9, - and _")?;
        Ok(config)
    }

    pub fn worlds_dir(&self) -> PathBuf {
        self.worlds_dir.clone().unwrap_or_else(|| self.data_dir.join("worlds"))
    }
}

pub fn generate_cert() -> anyhow::Result<(CertificateDer<'static>, PrivateKeyDer<'static>)> {
    let generated = rcgen::generate_simple_self_signed(vec![CERT_NAME.to_owned()])?;
    let key = PrivatePkcs8KeyDer::from(generated.signing_key.serialize_der());
    Ok((generated.cert.der().clone(), key.into()))
}

/// Generates and saves a certificate on first run; it must stay stable across
/// restarts so clients can pin it (docs/decisions.md D3).
pub fn load_or_create_cert(dir: &Path) -> anyhow::Result<(CertificateDer<'static>, PrivateKeyDer<'static>)> {
    let (cert_path, key_path) = (dir.join("cert.der"), dir.join("key.der"));
    if cert_path.exists() && key_path.exists() {
        let cert = std::fs::read(&cert_path).with_context(|| format!("reading {}", cert_path.display()))?;
        let key = std::fs::read(&key_path).with_context(|| format!("reading {}", key_path.display()))?;
        return Ok((cert.into(), PrivatePkcs8KeyDer::from(key).into()));
    }
    let (cert, key) = generate_cert()?;
    std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    std::fs::write(&cert_path, &cert)?;
    write_private(&key_path, key.secret_der())?;
    info!(path = %cert_path.display(), "generated new TLS certificate");
    Ok((cert, key))
}

fn write_private(path: &Path, data: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    options.open(path)?.write_all(data)
}

pub fn endpoint(
    port: u16,
    cert: CertificateDer<'static>,
    key: PrivateKeyDer<'static>,
) -> anyhow::Result<quinn::Endpoint> {
    let mut crypto =
        rustls::ServerConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
            .with_protocol_versions(&[&rustls::version::TLS13])?
            .with_no_client_auth()
            .with_single_cert(vec![cert], key)?;
    crypto.alpn_protocols = vec![ALPN.to_vec()];

    let mut transport = quinn::TransportConfig::default();
    // One control stream, plus one at a time for a world download.
    transport.max_concurrent_bidi_streams(2u8.into());
    transport.max_concurrent_uni_streams(0u8.into());
    transport.max_idle_timeout(Some(Duration::from_secs(10).try_into()?));

    let mut config = quinn::ServerConfig::with_crypto(Arc::new(QuicServerConfig::try_from(crypto)?));
    config.transport_config(Arc::new(transport));
    Ok(quinn::Endpoint::server(config, ([0, 0, 0, 0], port).into())?)
}

pub async fn read_control(recv: &mut quinn::RecvStream) -> anyhow::Result<Control> {
    let mut prefix = [0; 2];
    recv.read_exact(&mut prefix).await?;
    let mut body = vec![0; Control::body_len(prefix)?];
    recv.read_exact(&mut body).await?;
    Ok(Control::decode(&body)?)
}

struct Player {
    name: String,
    conn: quinn::Connection,
    control: mpsc::Sender<Control>,
    instance: u16,
    /// Only the server writes this. A client never sends its own health.
    hp: u8,
    /// Where they last said they were. Not trusted as truth -- it is a pose,
    /// which is theirs to claim -- but it is enough to throw out a hit
    /// claimed from the other side of the world (docs/TRUST.md).
    at: [i16; 3],
}

struct Instance {
    slug: String,
    /// What its maker called it, and who it says it is open to (`ember::*`).
    /// The hub and the ones the host starts are named after their world.
    name: String,
    kind: u8,
    /// Player-made instances close when the last player leaves; the ones the
    /// host started from the window stay until the host stops them.
    temporary: bool,
    /// Who holds which item, by the item's index in the world's object list.
    /// The server never reads the world, so an item is just a number to it:
    /// its whole job is that exactly one player holds one at a time.
    held: HashMap<u16, u16>,
}

/// What the server window shows: the running instances, each with its players.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstanceView {
    pub id: u16,
    pub slug: String,
    pub temporary: bool,
    pub players: Vec<(u16, String)>,
}

/// Published on every membership change, while the lock is held (D15).
pub type Roster = Vec<InstanceView>;

#[derive(Default)]
struct State {
    /// Server-wide, so a player id is unique however many instances run.
    players: HashMap<u16, Player>,
    instances: BTreeMap<u16, Instance>,
    next_instance: u16,
}

/// How far apart two quantised positions are, in metres.
fn distance(a: [i16; 3], b: [i16; 3]) -> f32 {
    let delta = |i: usize| dequantize_pos(a[i]) - dequantize_pos(b[i]);
    (delta(0).powi(2) + delta(1).powi(2) + delta(2).powi(2)).sqrt()
}

/// How far a shot can reach, with enough slack on top for the half second of
/// lag between the shooter seeing somebody and the server hearing about it.
const WEAPON_RANGE: f32 = 50.0;
/// Four hits to put somebody down.
const DAMAGE: u8 = 25;

impl State {
    /// Somebody claims they hit somebody else. The claim is theirs; every
    /// consequence is the server's (docs/TRUST.md).
    ///
    /// What cannot be checked here is line of sight, because the server never
    /// reads a world and so has no walls to trace against. Everything that
    /// can be checked without one, is.
    fn hit(&mut self, shooter: u16, target: u16) {
        if shooter == target {
            return; // Not a way to take yourself out of a fight.
        }
        let Some(from) = self.players.get(&shooter) else {
            return;
        };
        let Some(victim) = self.players.get(&target) else {
            return;
        };
        if from.instance != victim.instance || from.hp == 0 || victim.hp == 0 {
            return;
        }
        if distance(from.at, victim.at) > WEAPON_RANGE {
            debug!(shooter, target, "dropped a hit claimed from out of range");
            return;
        }
        let instance = victim.instance;
        let left = victim.hp.saturating_sub(DAMAGE);
        self.players.get_mut(&target).expect("just looked it up").hp = left;
        self.broadcast(Some(instance), NOBODY, Control::Health { player: target, hp: left });
        if left == 0 {
            self.broadcast(Some(instance), NOBODY, Control::Died { player: target, by: shooter });
        }
    }

    /// A dead player asking to come back. Letting the client ask avoids a
    /// server tick loop for one timer (D28 keeps the server event-driven),
    /// and there is nothing to gain by asking early: it is refused while you
    /// are alive, and respawning is not an advantage.
    fn respawn(&mut self, id: u16) {
        let Some(player) = self.players.get_mut(&id) else {
            return;
        };
        if player.hp != 0 {
            return;
        }
        player.hp = MAX_HEALTH;
        let instance = player.instance;
        self.broadcast(Some(instance), NOBODY, Control::Health { player: id, hp: MAX_HEALTH });
        self.broadcast(Some(instance), NOBODY, Control::Spawned { player: id });
    }

    /// Remembers where somebody said they were, for the range check.
    fn note_position(&mut self, id: u16, at: [i16; 3]) {
        if let Some(player) = self.players.get_mut(&id) {
            player.at = at;
        }
    }

    fn count(&self, instance: u16) -> u16 {
        self.players.values().filter(|p| p.instance == instance).count().min(u16::MAX as usize) as u16
    }

    fn health_of(&self, id: u16) -> u8 {
        self.players.get(&id).map_or(0, |p| p.hp)
    }

    /// The name a player joined under and the world they are standing in.
    /// A store write is namespaced by both, and the client names neither.
    fn whereabouts(&self, id: u16) -> (String, String) {
        let Some(player) = self.players.get(&id) else {
            return (String::new(), String::new());
        };
        let world = self.instances.get(&player.instance).map_or_else(|| "hub".to_owned(), |i| i.slug.clone());
        (player.name.clone(), world)
    }

    fn send(&self, id: u16, msg: Control) {
        if let Some(player) = self.players.get(&id)
            && player.control.try_send(msg).is_err()
        {
            // A full queue means the client stopped reading.
            warn!(id, "closing: the client stopped reading its control stream");
            player.conn.close(0u8.into(), b"control backlog");
        }
    }

    /// Takes an item for `id`, off whoever had it. Everyone in the instance is
    /// told, the previous holder included, so they stop carrying it.
    fn grab(&mut self, id: u16, item: u16) {
        let Some(instance) = self.players.get(&id).map(|p| p.instance) else {
            return;
        };
        let Some(running) = self.instances.get_mut(&instance) else {
            return;
        };
        if running.held.insert(item, id) == Some(id) {
            return; // Already theirs; no need to tell anyone again.
        }
        self.broadcast(Some(instance), NOBODY, Control::Held { item, by: id });
    }

    /// Puts an item down. Ignored unless `id` is really the holder, so a late
    /// `Release` from a player who has already been robbed cannot drop it.
    fn release(&mut self, id: u16, item: u16) {
        let Some(instance) = self.players.get(&id).map(|p| p.instance) else {
            return;
        };
        let Some(running) = self.instances.get_mut(&instance) else {
            return;
        };
        if running.held.get(&item) != Some(&id) {
            return;
        }
        running.held.remove(&item);
        self.broadcast(Some(instance), NOBODY, Control::Held { item, by: NOBODY });
    }

    /// Everything `id` was carrying falls where it is: called when they walk
    /// into another instance and when they disconnect. Without this an item
    /// stays owned by a player who is gone, and nobody can ever pick it up.
    fn drop_all(&mut self, id: u16, instance: u16) {
        let Some(running) = self.instances.get_mut(&instance) else {
            return;
        };
        let dropped: Vec<u16> =
            running.held.iter().filter(|&(_, &holder)| holder == id).map(|(&item, _)| item).collect();
        for item in &dropped {
            running.held.remove(item);
        }
        for item in dropped {
            self.broadcast(Some(instance), id, Control::Held { item, by: NOBODY });
        }
    }

    /// Hands a new arrival the items already in somebody's hands there.
    fn send_held(&self, id: u16, instance: u16) {
        let Some(running) = self.instances.get(&instance) else {
            return;
        };
        let held: Vec<(u16, u16)> = running.held.iter().map(|(&item, &by)| (item, by)).collect();
        for (item, by) in held {
            self.send(id, Control::Held { item, by });
        }
    }

    /// An `instance` of `None` means every player on the server.
    fn broadcast(&self, instance: Option<u16>, except: u16, msg: Control) {
        for (&id, player) in &self.players {
            if id == except || instance.is_some_and(|i| i != player.instance) {
                continue;
            }
            if player.control.try_send(msg.clone()).is_err() {
                warn!(id, "closing: the client stopped reading its control stream");
                player.conn.close(0u8.into(), b"control backlog");
            }
        }
    }
}

pub struct Server {
    pub max_players: u16,
    max_instances: u16,
    client_instancing: AtomicBool,
    repo: Mutex<Repository>,
    state: Mutex<State>,
    roster: watch::Sender<Roster>,
    /// What players have asked the server to remember (docs/TRUST.md).
    store: Mutex<store::Store>,
}

impl Server {
    pub fn new(config: &Config, roster: watch::Sender<Roster>) -> Arc<Self> {
        let repo = Repository::open(config.worlds_dir());
        let mut state = State::default();
        // The hub exists even when its world file does not: clients then keep
        // their built-in backdrop and can still see each other.
        state.instances.insert(
            HUB_INSTANCE,
            Instance {
                slug: config.hub_world.clone(),
                name: config.hub_world.clone(),
                kind: ember::PUBLIC,
                temporary: false,
                held: HashMap::new(),
            },
        );
        state.next_instance = HUB_INSTANCE.wrapping_add(1);
        let server = Arc::new(Self {
            max_players: config.max_players,
            max_instances: config.max_instances.max(1),
            client_instancing: AtomicBool::new(config.client_instancing),
            repo: Mutex::new(repo),
            state: Mutex::new(state),
            roster,
            store: Mutex::new(store::Store::new(config.data_dir.join("players"))),
        });
        server.publish(&server.state.lock().unwrap());
        server
    }

    pub fn worlds(&self) -> Vec<World> {
        self.repo.lock().unwrap().worlds().map(|entry| entry.world.clone()).collect()
    }

    pub fn worlds_dir(&self) -> PathBuf {
        self.repo.lock().unwrap().dir().to_owned()
    }

    pub fn rescan_worlds(&self) {
        self.repo.lock().unwrap().scan();
    }

    pub fn add_world(&self, path: &Path) -> anyhow::Result<String> {
        self.repo.lock().unwrap().add(path)
    }

    pub fn client_instancing(&self) -> bool {
        self.client_instancing.load(Ordering::Relaxed)
    }

    pub fn set_client_instancing(&self, on: bool) {
        self.client_instancing.store(on, Ordering::Relaxed);
        info!(on, "players may start their own instances");
    }

    /// Disconnects one player. Everything else follows from the connection
    /// closing, the same as somebody quitting.
    pub fn kick(&self, id: u16) -> bool {
        let state = self.state.lock().unwrap();
        let Some(player) = state.players.get(&id) else {
            return false;
        };
        info!(id, name = %player.name, "kicked");
        player.conn.close(0u8.into(), b"kicked by the host");
        true
    }

    /// Starts an instance that stays until the host stops it.
    pub fn start_instance(&self, slug: &str) -> anyhow::Result<u16> {
        if self.repo.lock().unwrap().get(slug).is_none() {
            bail!("no world called {slug:?} in the repository");
        }
        let mut state = self.state.lock().unwrap();
        self.open_instance(&mut state, slug, slug, ember::PUBLIC, false)
            .context("too many instances are running")
    }

    /// Moves everyone in the instance back to the hub, then closes it.
    pub fn stop_instance(&self, instance: u16) -> bool {
        if instance == HUB_INSTANCE {
            return false; // The hub is where people land; it cannot close.
        }
        let mut state = self.state.lock().unwrap();
        if !state.instances.contains_key(&instance) {
            return false;
        }
        let stranded: Vec<u16> =
            state.players.iter().filter(|(_, p)| p.instance == instance).map(|(&id, _)| id).collect();
        for id in stranded {
            self.move_player(&mut state, id, HUB_INSTANCE);
        }
        self.close_instance(&mut state, instance);
        true
    }

    pub fn instances(&self) -> Roster {
        view(&self.state.lock().unwrap())
    }

    fn publish(&self, state: &State) {
        let _ = self.roster.send(view(state));
    }
}

/// The instance bookkeeping, all of it under the state lock so every client
/// sees membership changes in the same order (D9).
impl Server {
    fn open_instance(
        &self,
        state: &mut State,
        slug: &str,
        name: &str,
        kind: u8,
        temporary: bool,
    ) -> Option<u16> {
        if state.instances.len() >= self.max_instances as usize {
            return None;
        }
        // Instance ids are never reused while running, and wrap at u16.
        let id = (0..=u16::MAX)
            .map(|step| state.next_instance.wrapping_add(step))
            .find(|id| *id != HUB_INSTANCE && !state.instances.contains_key(id))?;
        state.next_instance = id.wrapping_add(1);
        state.instances.insert(
            id,
            Instance { slug: slug.to_owned(), name: name.to_owned(), kind, temporary, held: HashMap::new() },
        );
        state.broadcast(
            None,
            0,
            Control::InstanceAdded {
                instance: id,
                slug: slug.to_owned(),
                players: 0,
                name: name.to_owned(),
                kind,
            },
        );
        self.publish(state);
        info!(instance = id, %slug, temporary, "instance started");
        Some(id)
    }

    fn close_instance(&self, state: &mut State, instance: u16) {
        if state.instances.remove(&instance).is_some() {
            state.broadcast(None, 0, Control::InstanceClosed { instance });
            self.publish(state);
            info!(instance, "instance closed");
        }
    }

    /// The one path that changes which instance a player is in: it tells the
    /// mover where they ended up, hands them the peers there, and tells both
    /// instances what happened.
    fn move_player(&self, state: &mut State, id: u16, to: u16) {
        let Some(player) = state.players.get_mut(&id) else {
            return;
        };
        let from = player.instance;
        if from == to {
            state.send(id, Control::Moved { instance: to });
            return;
        }
        player.instance = to;
        let name = player.name.clone();
        state.drop_all(id, from);
        state.broadcast(Some(from), id, Control::PeerLeft { id });
        state.send(id, Control::Moved { instance: to });
        let peers: Vec<Peer> = state
            .players
            .iter()
            .filter(|(peer_id, p)| **peer_id != id && p.instance == to)
            .map(|(&peer_id, p)| Peer { id: peer_id, name: p.name.clone() })
            .collect();
        for peer in peers {
            state.send(id, Control::PeerJoined(peer));
        }
        state.broadcast(Some(to), id, Control::PeerJoined(Peer { id, name: name.clone() }));
        state.send_held(id, to);
        info!(id, %name, from, to, "player moved");
        self.announce_counts(state, 0, &[from, to]);
        self.publish(state);
        // A player-made instance exists only while somebody is in it.
        if state.instances.get(&from).is_some_and(|i| i.temporary) && state.count(from) == 0 {
            self.close_instance(state, from);
        }
    }

    /// `except` is the player who already knows, because they were just sent
    /// the whole instance list.
    fn announce_counts(&self, state: &State, except: u16, instances: &[u16]) {
        for &instance in instances {
            if state.instances.contains_key(&instance) {
                state.broadcast(
                    None,
                    except,
                    Control::InstanceCount { instance, players: state.count(instance) },
                );
            }
        }
    }

    /// A player asked for their own instance of a world.
    fn create_for_player(&self, state: &mut State, id: u16, slug: &str, name: &str, kind: u8) {
        let current = state.players.get(&id).map(|p| p.instance).unwrap_or(HUB_INSTANCE);
        let known = self.repo.lock().unwrap().get(slug).is_some();
        let opened = (self.client_instancing() && known)
            .then(|| self.open_instance(state, slug, name, kind, true))
            .flatten();
        match opened {
            Some(instance) => self.move_player(state, id, instance),
            // Being told the instance you are already in is how a refusal
            // reaches the client without closing the connection.
            None => state.send(id, Control::Moved { instance: current }),
        }
    }

    /// Where a held item is now. Dropped unless `from` is the holder: nobody
    /// gets to move something out of somebody else's hands.
    fn relay_item(&self, from: u16, item: u16, at: hearth_protocol::Transform) {
        let state = self.state.lock().unwrap();
        let Some(instance) = state.players.get(&from).map(|p| p.instance) else {
            return;
        };
        if state.instances.get(&instance).and_then(|i| i.held.get(&item)) != Some(&from) {
            return;
        }
        let bytes = bytes::Bytes::from(Datagram::ItemRelay { from, item, at }.encode());
        for (&id, player) in &state.players {
            if id != from && player.instance == instance {
                let _ = player.conn.send_datagram(bytes.clone());
            }
        }
    }

    fn relay(&self, from: u16, msg: &Datagram) {
        let bytes = bytes::Bytes::from(msg.encode());
        let state = self.state.lock().unwrap();
        let Some(instance) = state.players.get(&from).map(|p| p.instance) else {
            return;
        };
        for (&id, player) in &state.players {
            if id != from && player.instance == instance {
                let _ = player.conn.send_datagram(bytes.clone());
            }
        }
    }
}

fn view(state: &State) -> Roster {
    state
        .instances
        .iter()
        .map(|(&id, instance)| {
            let mut players: Vec<(u16, String)> = state
                .players
                .iter()
                .filter(|(_, p)| p.instance == id)
                .map(|(&pid, p)| (pid, p.name.clone()))
                .collect();
            players.sort();
            InstanceView { id, slug: instance.slug.clone(), temporary: instance.temporary, players }
        })
        .collect()
}

pub async fn serve(endpoint: quinn::Endpoint, config: &Config) {
    let server = Server::new(config, watch::channel(Roster::new()).0);
    serve_with(endpoint, server).await
}

pub async fn serve_with(endpoint: quinn::Endpoint, server: Arc<Server>) {
    while let Some(incoming) = endpoint.accept().await {
        let server = server.clone();
        tokio::spawn(async move {
            let addr = incoming.remote_address();
            info!(%addr, "connection attempt");
            if let Err(err) = handle_connection(incoming, server).await {
                // Logged at info: a connection that never became a player is
                // exactly what a host wants to see when somebody says "it
                // will not let me in".
                info!(%addr, "connection ended: {err:#}");
            }
        });
    }
}

async fn handle_connection(incoming: quinn::Incoming, server: Arc<Server>) -> anyhow::Result<()> {
    let conn = incoming.await?;
    let addr = conn.remote_address();
    let (mut send, mut recv) =
        timeout(HANDSHAKE_TIMEOUT, conn.accept_bi()).await.context("handshake timeout")??;
    let hello = timeout(HANDSHAKE_TIMEOUT, read_control(&mut recv)).await.context("handshake timeout")?;

    let name = match hello {
        Ok(Control::Hello { version: PROTOCOL_VERSION, name }) => name,
        Ok(Control::Hello { version, .. }) => {
            info!(%addr, version, "rejected: wrong protocol version");
            return reject(&conn, send, RejectReason::BadVersion).await;
        }
        Err(err) if err.downcast_ref::<DecodeError>() == Some(&DecodeError::BadName) => {
            info!(%addr, "rejected: invalid name");
            return reject(&conn, send, RejectReason::BadName).await;
        }
        Ok(other) => bail!("expected Hello, got {other:?}"),
        Err(err) => return Err(err),
    };

    // The queue holds the whole join burst: Welcome, every world, every
    // instance, and one PeerJoined per player already in the hub.
    let worlds = server.worlds();
    // Two hands per player, so the arrival burst can carry that many `Held`.
    let (control_tx, mut control_rx) =
        mpsc::channel(server.max_players as usize * 3 + worlds.len() + server.max_instances as usize + 16);
    let joined = {
        let mut state = server.state.lock().unwrap();
        let free_id = (state.players.len() < server.max_players as usize)
            .then(|| (1..=u16::MAX).find(|id| !state.players.contains_key(id)))
            .flatten();
        if let Some(id) = free_id {
            let flags = if server.client_instancing() { welcome::CLIENT_INSTANCING } else { 0 };
            control_tx.try_send(Control::Welcome {
                your_id: id,
                tick_hz: TICK_HZ,
                instance: HUB_INSTANCE,
                flags,
            })?;
            for world in worlds {
                control_tx.try_send(Control::WorldAdded(world))?;
            }
            for (&instance, running) in &state.instances {
                control_tx.try_send(Control::InstanceAdded {
                    instance,
                    slug: running.slug.clone(),
                    players: state.count(instance),
                    name: running.name.clone(),
                    kind: running.kind,
                })?;
            }
            for (&peer_id, peer) in state.players.iter().filter(|(_, p)| p.instance == HUB_INSTANCE) {
                control_tx.try_send(Control::PeerJoined(Peer { id: peer_id, name: peer.name.clone() }))?;
            }
            state.broadcast(Some(HUB_INSTANCE), id, Control::PeerJoined(Peer { id, name: name.clone() }));
            state.players.insert(
                id,
                Player {
                    name: name.clone(),
                    conn: conn.clone(),
                    control: control_tx,
                    instance: HUB_INSTANCE,
                    hp: MAX_HEALTH,
                    at: [0; 3],
                },
            );
            state.send_held(id, HUB_INSTANCE);
            server.announce_counts(&state, id, &[HUB_INSTANCE]);
            server.publish(&state);
        }
        free_id
    };
    let Some(id) = joined else {
        info!(%addr, %name, "rejected: server full");
        return reject(&conn, send, RejectReason::ServerFull).await;
    };
    info!(id, %name, %addr, "player joined");

    let writer = tokio::spawn(async move {
        let mut buf = Vec::new();
        while let Some(msg) = control_rx.recv().await {
            buf.clear();
            msg.encode(&mut buf);
            if send.write_all(&buf).await.is_err() {
                break;
            }
        }
    });

    let result = relay(&conn, &server, id, &mut recv).await;

    {
        let mut state = server.state.lock().unwrap();
        let left = state.players.get(&id).map(|p| p.instance).unwrap_or(HUB_INSTANCE);
        state.drop_all(id, left);
        state.players.remove(&id);
        state.broadcast(Some(left), id, Control::PeerLeft { id });
        server.announce_counts(&state, 0, &[left]);
        server.publish(&state);
        if state.instances.get(&left).is_some_and(|i| i.temporary) && state.count(left) == 0 {
            server.close_instance(&mut state, left);
        }
    }
    writer.abort();
    info!(id, %name, reason = %result.as_ref().err().map(|e| e.to_string()).unwrap_or_default(), "player left");
    Ok(())
}

async fn reject(
    conn: &quinn::Connection,
    mut send: quinn::SendStream,
    reason: RejectReason,
) -> anyhow::Result<()> {
    let mut buf = Vec::new();
    Control::Reject { reason }.encode(&mut buf);
    send.write_all(&buf).await?;
    send.finish()?;
    // Wait for the client to receive it before closing; closing discards unsent data.
    let _ = timeout(HANDSHAKE_TIMEOUT, send.stopped()).await;
    // The reason rides along in the close frame too: a client that has the
    // bytes but has not read them yet would otherwise see only a closed
    // connection, and tell the player nothing useful.
    conn.close(0u8.into(), &reason.close_frame());
    Ok(())
}

/// Everything a joined player sends: poses and voice in datagrams, instance
/// requests on the control stream, and world downloads on streams of their own.
async fn relay(
    conn: &quinn::Connection,
    server: &Arc<Server>,
    id: u16,
    recv: &mut quinn::RecvStream,
) -> anyhow::Result<()> {
    // 1.5x the nominal rate plus a burst, so jitter isn't punished.
    let mut pose_limit = RateLimit::new(TICK_HZ as f32 * 1.5, 10.0);
    let mut voice_limit = RateLimit::new(75.0, 25.0); // 20 ms frames = 50/s
    // Two hands at the pose rate, and the same 1.5x headroom for jitter.
    let mut item_limit = RateLimit::new(TICK_HZ as f32 * 3.0, 20.0);
    // A tracer per shot, and a hit claim per shot at most. Ten a second is
    // faster than any weapon fires and slow enough that a patched client
    // cannot machine-gun claims at somebody.
    let mut shot_limit = RateLimit::new(10.0, 10.0);
    let mut hit_limit = RateLimit::new(10.0, 10.0);
    // Walking through portals is a human-speed action; a flood is abuse.
    let mut control_limit = RateLimit::new(5.0, 20.0);
    // Chat: a line a second, with room for a quick back-and-forth. Over it,
    // lines are dropped rather than the player kicked: typing fast is not abuse.
    let mut chat_limit = RateLimit::new(1.0, 5.0);
    let downloads = Arc::new(tokio::sync::Semaphore::new(1));
    loop {
        tokio::select! {
            data = conn.read_datagram() => {
                let data = data?;
                let now = Instant::now();
                match Datagram::decode(&data) {
                    Ok(Datagram::Pose(mut pose)) if pose_limit.allow(now) => {
                        pose.flags &= flags::VR | flags::LEFT_HAND | flags::RIGHT_HAND;
                        // Noted before relaying: a hit claim arriving right
                        // behind this one is checked against it.
                        server.state.lock().unwrap().note_position(id, pose.head.pos);
                        server.relay(id, &Datagram::PoseRelay { from: id, pose });
                    }
                    Ok(Datagram::Voice { seq, frame }) if voice_limit.allow(now) => {
                        server.relay(id, &Datagram::VoiceRelay { from: id, seq, frame });
                    }
                    Ok(Datagram::Item { item, at }) if item_limit.allow(now) => {
                        server.relay_item(id, item, at);
                    }
                    Ok(Datagram::Shot { at }) if shot_limit.allow(now) => {
                        // Only a tracer for everyone else to draw. Whether it
                        // hit anybody arrives separately, and reliably.
                        server.relay(id, &Datagram::ShotRelay { from: id, at });
                    }
                    Ok(
                        Datagram::Pose(_)
                        | Datagram::Voice { .. }
                        | Datagram::Item { .. }
                        | Datagram::Shot { .. },
                    ) => {} // rate limited
                    Ok(other) => warn!(id, "dropped server-only datagram {other:?}"),
                    Err(err) => debug!(id, "dropped bad datagram: {err}"),
                }
            }
            msg = read_control(recv) => {
                let msg = msg?;
                if !control_limit.allow(Instant::now()) {
                    bail!("control message flood");
                }
                let mut state = server.state.lock().unwrap();
                match msg {
                    Control::JoinInstance { instance } if state.instances.contains_key(&instance) => {
                        server.move_player(&mut state, id, instance);
                    }
                    Control::JoinInstance { .. } => {
                        // Gone already: tell them where they still are.
                        let current = state.players.get(&id).map(|p| p.instance).unwrap_or(HUB_INSTANCE);
                        state.send(id, Control::Moved { instance: current });
                    }
                    Control::CreateInstance { slug, name, kind } => {
                        server.create_for_player(&mut state, id, &slug, &name, kind)
                    }
                    Control::Grab { item } => state.grab(id, item),
                    Control::Release { item } => state.release(id, item),
                    Control::Hit { target } if hit_limit.allow(Instant::now()) => state.hit(id, target),
                    Control::Hit { .. } => debug!(id, "dropped a hit: firing too fast"),
                    Control::Spawned { .. } => state.respawn(id),
                    Control::Store { key, value } => {
                        let (name, world) = state.whereabouts(id);
                        let mut store = server.store.lock().unwrap();
                        if let Err(refused) = store.put(&name, &world, &key, value) {
                            debug!(id, ?refused, key, "refused a write");
                            // Hand back what is actually stored, so a world
                            // that was refused is not left believing itself.
                            let value = store.get(&name, &world, &key);
                            state.send(id, Control::Value { key, value });
                        }
                    }
                    Control::Say { text } if chat_limit.allow(Instant::now()) => {
                        let instance = state.players.get(&id).map(|p| p.instance).unwrap_or(HUB_INSTANCE);
                        let (name, _) = state.whereabouts(id);
                        info!(id, instance, "{name}: {text}");
                        // Sender included: the echo is their receipt.
                        state.broadcast(Some(instance), NOBODY, Control::Said { from: id, text });
                    }
                    Control::Say { .. } => debug!(id, "dropped a chat line: talking too fast"),
                    Control::Fetch { key } => {
                        let (name, world) = state.whereabouts(id);
                        let value = server.store.lock().unwrap().get(&name, &world, &key);
                        state.send(id, Control::Value { key, value });
                    }
                    other => bail!("unexpected control message {other:?}"),
                }
            }
            stream = conn.accept_bi() => {
                let (send, recv) = stream?;
                let server = server.clone();
                // One download at a time per connection, so a world transfer
                // cannot be used to make the server read many files at once.
                let permit = downloads.clone().try_acquire_owned();
                tokio::spawn(async move {
                    let Ok(_permit) = permit else {
                        info!(id, "refused a second world download at once");
                        return;
                    };
                    if let Err(err) = serve_world(&server, id, send, recv).await {
                        info!(id, "world download failed: {err:#}");
                    }
                });
            }
        }
    }
}

/// A download stream: one `WorldRequest`, then the file, then the stream ends.
async fn serve_world(
    server: &Server,
    id: u16,
    mut send: quinn::SendStream,
    mut recv: quinn::RecvStream,
) -> anyhow::Result<()> {
    let slug =
        match timeout(HANDSHAKE_TIMEOUT, read_control(&mut recv)).await.context("request timeout")?? {
            Control::WorldRequest { slug } => slug,
            other => bail!("expected WorldRequest, got {other:?}"),
        };
    // The guard is dropped before the awaits below: no lock is ever held
    // across one (D9).
    let path = server.repo.lock().unwrap().get(&slug).map(|entry| entry.path.clone());
    let Some(path) = path else {
        // A reset stream is how "I do not have that" reaches the client.
        let _ = send.reset(1u8.into());
        bail!("no world called {slug:?}");
    };
    info!(id, %slug, "sending a world");
    let mut file =
        tokio::fs::File::open(&path).await.with_context(|| format!("opening {}", path.display()))?;
    let sent = tokio::io::copy(&mut file, &mut send).await?;
    send.finish()?;
    let _ = send.stopped().await;
    info!(id, %slug, sent, "world sent");
    Ok(())
}

struct RateLimit {
    tokens: f32,
    rate: f32,
    burst: f32,
    last: Instant,
}

impl RateLimit {
    fn new(rate: f32, burst: f32) -> Self {
        Self { tokens: burst, rate, burst, last: Instant::now() }
    }

    fn allow(&mut self, now: Instant) -> bool {
        self.tokens = (self.tokens + now.duration_since(self.last).as_secs_f32() * self.rate).min(self.burst);
        self.last = now;
        if self.tokens < 1.0 {
            return false;
        }
        self.tokens -= 1.0;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rate_limit_allows_burst_then_refills() {
        let mut limit = RateLimit::new(10.0, 3.0);
        let t0 = limit.last;
        assert!((0..3).all(|_| limit.allow(t0)));
        assert!(!limit.allow(t0));
        assert!(limit.allow(t0 + Duration::from_millis(100)));
        assert!(!limit.allow(t0 + Duration::from_millis(100)));
        // Long idle refills only up to the burst.
        let t1 = t0 + Duration::from_secs(60);
        assert!((0..3).all(|_| limit.allow(t1)));
        assert!(!limit.allow(t1));
    }

    #[test]
    fn config_parsing() {
        let config = Config::parse("port = 9000\nmax_players = 2").unwrap();
        assert_eq!((config.port, config.max_players), (9000, 2));
        assert_eq!(Config::parse("").unwrap().port, 7777);
        assert!(Config::parse("max_players = 0").is_err());
        assert!(Config::parse("prot = 1").is_err()); // typo is an error, not silently ignored

        // Worlds live under the data folder unless told otherwise.
        assert_eq!(Config::parse("data_dir = \"srv\"").unwrap().worlds_dir(), PathBuf::from("srv/worlds"));
        assert_eq!(Config::parse("worlds_dir = \"w\"").unwrap().worlds_dir(), PathBuf::from("w"));
        assert!(Config::parse("hub_world = \"Hub World\"").is_err());
        assert!(Config::parse("max_instances = 0").is_err());
        assert!(Config::parse("client_instancing = true").unwrap().client_instancing);
    }

    fn with_worlds(tag: &str, worlds: &[&str]) -> (PathBuf, Arc<Server>) {
        let dir = std::env::temp_dir().join(format!("hearth-srv-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for slug in worlds {
            std::fs::write(dir.join(format!("{slug}.hworld")), b"a world").unwrap();
        }
        let config = Config { worlds_dir: Some(dir.clone()), max_instances: 3, ..Config::default() };
        (dir, Server::new(&config, watch::channel(Roster::new()).0))
    }

    #[test]
    fn hub_runs_even_without_its_world_and_cannot_be_stopped() {
        let (dir, server) = with_worlds("hub", &[]);
        let instances = server.instances();
        assert_eq!(instances.len(), 1);
        assert_eq!((instances[0].id, instances[0].slug.as_str()), (HUB_INSTANCE, "hub"));
        assert!(server.worlds().is_empty());
        assert!(!server.stop_instance(HUB_INSTANCE));
        assert!(server.start_instance("nope").is_err()); // unknown world
        assert_eq!(server.instances().len(), 1);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn instances_start_and_stop_within_the_limit() {
        let (dir, server) = with_worlds("instances", &["forest"]);
        let first = server.start_instance("forest").unwrap();
        let second = server.start_instance("forest").unwrap();
        assert_ne!(first, second);
        assert_eq!(server.instances().len(), 3); // the hub plus two
        // max_instances counts the hub, so there is no room for a third.
        assert!(server.start_instance("forest").is_err());

        assert!(server.stop_instance(first));
        assert!(!server.stop_instance(first)); // already gone
        assert!(server.start_instance("forest").is_ok());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn adding_a_world_makes_it_startable() {
        let (dir, server) = with_worlds("add", &[]);
        let loose = dir.join("meadow.hworld");
        std::fs::write(&loose, b"packed").unwrap();
        assert!(server.start_instance("meadow").is_err()); // not scanned yet
        assert_eq!(server.add_world(&loose).unwrap(), "meadow");
        assert!(server.start_instance("meadow").is_ok());
        assert_eq!(server.worlds().len(), 1);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
