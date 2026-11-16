//! Hand-rolled QUIC clients against an in-process server.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use hearth_flame::ticket::testing::Issuer;
use hearth_flame::{CERT_NAME, Config, Limit, Roster, Server, read_control};
use hearth_protocol::{
    ALPN, Control, Datagram, HUB_INSTANCE, PROTOCOL_VERSION, Peer, Pose, RejectReason, Transform, World,
    ember, welcome,
};
use quinn::crypto::rustls::QuicClientConfig;
use rustls::pki_types::CertificateDer;
use tokio::sync::watch;
use tokio::time::timeout;

const T: Duration = Duration::from_secs(5);

/// Asking for an ember: a world, a name for it, and who it is open to.
fn create(slug: &str, name: &str, kind: u8) -> Control {
    Control::CreateInstance { slug: slug.into(), name: name.into(), kind }
}

struct Client {
    conn: quinn::Connection,
    send: quinn::SendStream,
    recv: quinn::RecvStream,
}

impl Client {
    async fn connect(addr: SocketAddr, cert: &CertificateDer<'static>, version: u16, name: &str) -> Self {
        Self::connect_with(addr, cert, version, name, "").await
    }

    async fn connect_with(
        addr: SocketAddr,
        cert: &CertificateDer<'static>,
        version: u16,
        name: &str,
        ticket: &str,
    ) -> Self {
        let mut roots = rustls::RootCertStore::empty();
        roots.add(cert.clone()).unwrap();
        let mut crypto =
            rustls::ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
                .with_protocol_versions(&[&rustls::version::TLS13])
                .unwrap()
                .with_root_certificates(roots)
                .with_no_client_auth();
        crypto.alpn_protocols = vec![ALPN.to_vec()];
        let mut endpoint = quinn::Endpoint::client(([127, 0, 0, 1], 0).into()).unwrap();
        endpoint.set_default_client_config(quinn::ClientConfig::new(Arc::new(
            QuicClientConfig::try_from(crypto).unwrap(),
        )));
        let conn = endpoint.connect(addr, CERT_NAME).unwrap().await.unwrap();
        let (mut send, recv) = conn.open_bi().await.unwrap();
        let mut buf = Vec::new();
        // Encode by hand so invalid names can be sent too.
        buf.extend_from_slice(&[0, 0, 0x01]);
        buf.extend_from_slice(&version.to_le_bytes());
        buf.push(name.len() as u8);
        buf.extend_from_slice(name.as_bytes());
        buf.extend_from_slice(&(ticket.len() as u16).to_le_bytes());
        buf.extend_from_slice(ticket.as_bytes());
        let body_len = (buf.len() - 2) as u16;
        buf[..2].copy_from_slice(&body_len.to_le_bytes());
        send.write_all(&buf).await.unwrap();
        Self { conn, send, recv }
    }

    async fn next_any(&mut self) -> Control {
        timeout(T, read_control(&mut self.recv)).await.unwrap().unwrap()
    }

    /// Player counts arrive whenever anybody anywhere moves, so tests that
    /// are not about counts skip them.
    async fn next(&mut self) -> Control {
        loop {
            match self.next_any().await {
                Control::InstanceCount { .. } => continue,
                other => return other,
            }
        }
    }

    async fn datagram(&self) -> Datagram {
        Datagram::decode(&timeout(T, self.conn.read_datagram()).await.unwrap().unwrap()).unwrap()
    }

    async fn say(&mut self, msg: Control) {
        let mut buf = Vec::new();
        msg.encode(&mut buf);
        self.send.write_all(&buf).await.unwrap();
    }

    /// Joins, then eats the Welcome, the worlds and the instance list.
    async fn join(addr: SocketAddr, cert: &CertificateDer<'static>, name: &str) -> (Self, u16, Vec<World>) {
        let (client, id, worlds, _) = Self::join_with(addr, cert, name, "").await;
        (client, id, worlds)
    }

    /// The same, with a ticket, and the Welcome's flags as well.
    async fn join_with(
        addr: SocketAddr,
        cert: &CertificateDer<'static>,
        name: &str,
        ticket: &str,
    ) -> (Self, u16, Vec<World>, u8) {
        let mut client = Self::connect_with(addr, cert, PROTOCOL_VERSION, name, ticket).await;
        let (id, instance, flags) = match client.next().await {
            Control::Welcome { your_id, instance, flags, .. } => (your_id, instance, flags),
            other => panic!("expected Welcome, got {other:?}"),
        };
        assert_eq!(instance, HUB_INSTANCE, "everyone starts in the hub");
        let mut worlds = Vec::new();
        loop {
            match client.next().await {
                Control::WorldAdded(world) => worlds.push(world),
                Control::InstanceAdded { instance, .. } => {
                    assert_eq!(instance, HUB_INSTANCE);
                    break;
                }
                other => panic!("expected the world list, got {other:?}"),
            }
        }
        (client, id, worlds, flags)
    }

    /// Downloads a world on a stream of its own, as the real client does.
    async fn fetch_world(&self, slug: &str) -> Result<Vec<u8>, String> {
        let (mut send, mut recv) = self.conn.open_bi().await.map_err(|e| e.to_string())?;
        let mut buf = Vec::new();
        Control::WorldRequest { slug: slug.to_owned() }.encode(&mut buf);
        send.write_all(&buf).await.map_err(|e| e.to_string())?;
        timeout(T, recv.read_to_end(16 * 1024 * 1024)).await.unwrap().map_err(|e| e.to_string())
    }
}

struct Harness {
    addr: SocketAddr,
    cert: CertificateDer<'static>,
    /// Stands in for the Fireplace: the server holds its public key.
    issuer: Issuer,
    server: Arc<Server>,
    roster: watch::Receiver<Roster>,
    dir: PathBuf,
}

impl Harness {
    /// Starts a server with `worlds` in its repository, each holding its own
    /// recognisable bytes.
    async fn start(tag: &str, max_players: u16, worlds: &[&str]) -> Self {
        Self::start_with(tag, worlds, |config| config.max_players = Limit::of(max_players.into())).await
    }

    async fn start_with(tag: &str, worlds: &[&str], tweak: impl FnOnce(&mut Config)) -> Self {
        let dir = std::env::temp_dir().join(format!("hearth-relay-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for slug in worlds {
            std::fs::write(dir.join(format!("{slug}.hworld")), format!("packed world {slug}")).unwrap();
        }
        let mut config = Config {
            worlds_dir: Some(dir.clone()),
            data_dir: dir.join("data"),
            max_instances: Limit::of(3),
            max_instances_per_player: Limit::NONE,
            // Nothing listens here: the key comes from the issuer below, and
            // the background fetch just keeps failing quietly.
            fireplace: "http://127.0.0.1:9".into(),
            ..Config::default()
        };
        tweak(&mut config);
        let (cert, key) = hearth_flame::generate_cert().unwrap();
        let endpoint = hearth_flame::endpoint(0, cert.clone(), key).unwrap();
        let port = endpoint.local_addr().unwrap().port();
        let (tx, roster) = watch::channel(Roster::new());
        let server = Server::new(&config, &cert, tx);
        let issuer = Issuer::default();
        server.tickets.set_key(issuer.public_key());
        tokio::spawn(hearth_flame::serve_with(endpoint, server.clone()));
        Self { addr: ([127, 0, 0, 1], port).into(), cert, issuer, server, roster, dir }
    }

    /// A ticket for this server, as the Fireplace would mint one.
    fn ticket(&self, pid: &str, name: &str) -> String {
        self.issuer.fresh(self.server.tickets.certificate(), pid, name)
    }

    async fn join(&self, name: &str) -> (Client, u16, Vec<World>) {
        Client::join(self.addr, &self.cert, name).await
    }

    /// The window's view, after the next change.
    async fn roster(&mut self) -> Roster {
        timeout(T, self.roster.changed()).await.unwrap().unwrap();
        self.roster.borrow_and_update().clone()
    }

    fn hub_players(&self) -> Vec<(u16, String)> {
        self.server.instances().into_iter().find(|i| i.id == HUB_INSTANCE).unwrap().players
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[tokio::test]
async fn two_players_join_relay_and_leave() {
    let mut h = Harness::start("relay", 2, &[]).await;

    let (mut ada, ada_id, worlds) = h.join("Ada").await;
    assert_eq!(ada_id, 1);
    assert!(worlds.is_empty(), "this server has no worlds");
    assert_eq!(h.roster().await[0].players, vec![(1, "Ada".to_owned())]);

    let (mut bob, bob_id, _) = h.join("Bob").await;
    assert_eq!(bob_id, 2);
    assert_eq!(bob.next().await, Control::PeerJoined(Peer { id: 1, name: "Ada".into(), account: String::new() }));
    assert_eq!(ada.next_any().await, Control::PeerJoined(Peer { id: 2, name: "Bob".into(), account: String::new() }));
    // Everyone already here is told how full the instance now is.
    assert_eq!(ada.next_any().await, Control::InstanceCount { instance: HUB_INSTANCE, players: 2 });
    assert_eq!(h.hub_players(), vec![(1, "Ada".to_owned()), (2, "Bob".to_owned())]);

    // Server is full.
    let mut cy = Client::connect(h.addr, &h.cert, PROTOCOL_VERSION, "Cy").await;
    assert_eq!(cy.next().await, Control::Reject { reason: RejectReason::ServerFull });

    let t = Transform::from_f32([0.0, 1.7, 0.0], [0.0, 0.0, 0.0, 1.0]);
    let pose = Pose { seq: 5, flags: 0xFF, head: t, left: t, right: t };
    ada.conn.send_datagram(Datagram::Pose(pose).encode().into()).unwrap();
    // Unknown flag bits are stripped before relaying.
    assert_eq!(bob.datagram().await, Datagram::PoseRelay { from: 1, pose: Pose { flags: 0b111, ..pose } });

    bob.conn.send_datagram(Datagram::Voice { seq: 9, frame: vec![1, 2, 3] }.encode().into()).unwrap();
    assert_eq!(ada.datagram().await, Datagram::VoiceRelay { from: 2, seq: 9, frame: vec![1, 2, 3] });

    bob.conn.close(0u8.into(), b"bye");
    assert_eq!(ada.next_any().await, Control::PeerLeft { id: 2 });
    assert_eq!(ada.next_any().await, Control::InstanceCount { instance: HUB_INSTANCE, players: 1 });

    // The slot, and the id, are free again.
    let (_dee, dee_id, _) = h.join("Dee").await;
    assert_eq!(dee_id, 2);
    assert_eq!(ada.next().await, Control::PeerJoined(Peer { id: 2, name: "Dee".into(), account: String::new() }));
    let _ = ada.send.finish();
}

#[tokio::test]
async fn rejects_bad_version_and_bad_name() {
    let h = Harness::start("reject", 4, &[]).await;
    let mut old = Client::connect(h.addr, &h.cert, PROTOCOL_VERSION + 1, "Ada").await;
    assert_eq!(old.next().await, Control::Reject { reason: RejectReason::BadVersion });
    let mut bad = Client::connect(h.addr, &h.cert, PROTOCOL_VERSION, "a\u{7}b").await;
    assert_eq!(bad.next().await, Control::Reject { reason: RejectReason::BadName });
}

#[tokio::test]
async fn instances_keep_players_apart() {
    let h = Harness::start("instances", 4, &["forest"]).await;
    let (mut ada, _, worlds) = h.join("Ada").await;
    assert_eq!(worlds.len(), 1);
    assert_eq!(worlds[0].slug, "forest");
    assert_eq!(worlds[0].size, "packed world forest".len() as u32);

    let (mut bob, bob_id, _) = h.join("Bob").await;
    assert_eq!(bob.next().await, Control::PeerJoined(Peer { id: 1, name: "Ada".into(), account: String::new() }));
    assert_eq!(ada.next().await, Control::PeerJoined(Peer { id: bob_id, name: "Bob".into(), account: String::new() }));

    // The host starts an instance; both players hear about it.
    let forest = h.server.start_instance("forest").unwrap();
    // An ember the host starts is named after its world and open to everyone.
    let added = Control::InstanceAdded {
        instance: forest,
        slug: "forest".into(),
        players: 0,
        name: "forest".into(),
        kind: ember::PUBLIC,
    };
    assert_eq!(ada.next().await, added.clone());
    assert_eq!(bob.next().await, added);

    // Ada walks through the portal.
    ada.say(Control::JoinInstance { instance: forest }).await;
    assert_eq!(ada.next().await, Control::Moved { instance: forest });
    assert_eq!(bob.next().await, Control::PeerLeft { id: 1 });

    // A pose from the forest never reaches the hub.
    let t = Transform::from_f32([1.0, 1.7, 0.0], [0.0, 0.0, 0.0, 1.0]);
    let pose = Pose { seq: 1, flags: 0, head: t, left: t, right: t };
    ada.conn.send_datagram(Datagram::Pose(pose).encode().into()).unwrap();
    assert!(
        timeout(Duration::from_millis(300), bob.conn.read_datagram()).await.is_err(),
        "a pose crossed an instance boundary"
    );

    // Bob follows, and now they see each other again.
    bob.say(Control::JoinInstance { instance: forest }).await;
    assert_eq!(bob.next().await, Control::Moved { instance: forest });
    assert_eq!(bob.next().await, Control::PeerJoined(Peer { id: 1, name: "Ada".into(), account: String::new() }));
    assert_eq!(ada.next().await, Control::PeerJoined(Peer { id: bob_id, name: "Bob".into(), account: String::new() }));
    ada.conn.send_datagram(Datagram::Pose(pose).encode().into()).unwrap();
    assert_eq!(bob.datagram().await, Datagram::PoseRelay { from: 1, pose });

    // The window shows them in the forest, and the hub empty.
    let roster = h.server.instances();
    assert_eq!(roster.iter().find(|i| i.id == HUB_INSTANCE).unwrap().players, vec![]);
    assert_eq!(roster.iter().find(|i| i.id == forest).unwrap().players.len(), 2);

    // Stopping it sends everyone home. Whose move happens first is not
    // defined, so Ada may see Bob leave the forest before her own move.
    assert!(h.server.stop_instance(forest));
    let mut home = None;
    for _ in 0..4 {
        match ada.next().await {
            Control::Moved { instance } => {
                home = Some(instance);
                break;
            }
            Control::PeerLeft { .. } | Control::PeerJoined(_) => continue,
            other => panic!("unexpected {other:?} while being sent home"),
        }
    }
    assert_eq!(home, Some(HUB_INSTANCE));
    assert_eq!(h.hub_players().len(), 2);
}

#[tokio::test]
async fn a_player_made_instance_needs_permission_and_closes_when_empty() {
    let mut h = Harness::start("creating", 4, &["forest"]).await;
    let (mut ada, _, _) = h.join("Ada").await;

    // Off by default: the answer names the instance Ada is already in.
    assert!(!h.server.client_instancing());
    ada.say(create("forest", "Ada's fire", ember::FRIENDS)).await;
    assert_eq!(ada.next().await, Control::Moved { instance: HUB_INSTANCE });
    assert_eq!(h.server.instances().len(), 1);

    h.server.set_client_instancing(true);
    // An unknown world is refused the same way.
    ada.say(create("nope", "nowhere", ember::PUBLIC)).await;
    assert_eq!(ada.next().await, Control::Moved { instance: HUB_INSTANCE });

    ada.say(create("forest", "Ada's fire", ember::FRIENDS)).await;
    let instance = match ada.next().await {
        // The name and the kind she asked for come back to everybody.
        Control::InstanceAdded { instance, slug, players, name, kind } => {
            assert_eq!(
                (slug.as_str(), players, name.as_str(), kind),
                ("forest", 0, "Ada's fire", ember::FRIENDS)
            );
            instance
        }
        other => panic!("expected InstanceAdded, got {other:?}"),
    };
    assert_eq!(ada.next().await, Control::Moved { instance });
    assert_eq!(h.server.instances().len(), 2);
    let _ = h.roster().await;

    // It is hers alone, so it closes behind her.
    ada.say(Control::JoinInstance { instance: HUB_INSTANCE }).await;
    assert_eq!(ada.next().await, Control::Moved { instance: HUB_INSTANCE });
    assert_eq!(ada.next().await, Control::InstanceClosed { instance });
    assert_eq!(h.server.instances().len(), 1);
}

#[tokio::test]
async fn worlds_download_on_their_own_stream() {
    let h = Harness::start("download", 4, &["forest"]).await;
    let (ada, _, worlds) = h.join("Ada").await;

    let bytes = match ada.fetch_world("forest").await {
        Ok(bytes) => bytes,
        Err(err) => panic!("download failed: {err}"),
    };
    assert_eq!(bytes, b"packed world forest");
    assert_eq!(bytes.len() as u32, worlds[0].size);
    // The advertised hash is the hash of what arrived.
    let digest = ring::digest::digest(&ring::digest::SHA256, &bytes);
    assert_eq!(digest.as_ref(), worlds[0].sha256);

    // A world the server does not have fails instead of hanging.
    assert!(ada.fetch_world("nope").await.is_err());
    // And the connection still works afterwards.
    assert_eq!(ada.fetch_world("forest").await.unwrap(), b"packed world forest");
}

#[test]
fn certificate_persists_across_restarts() {
    let dir = std::env::temp_dir().join(format!("hearth-cert-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let (first, _) = hearth_flame::load_or_create_cert(&dir).unwrap();
    let (second, _) = hearth_flame::load_or_create_cert(&dir).unwrap();
    assert_eq!(first, second);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// The commands the window and the console prompt share, against a real
/// player on a real connection.
#[tokio::test]
async fn commands_see_and_kick_real_players() {
    use hearth_flame::command;

    let h = Harness::start("commands", 4, &["forest"]).await;
    let (mut ada, ada_id, _) = h.join("Ada").await;

    let players = command::run(&h.server, "players");
    assert!(players.contains("instance 0 — hub (1 player(s))"), "{players}");
    assert!(players.contains(&format!("{ada_id}  Ada")), "{players}");
    assert!(command::run(&h.server, "status").contains("1/4 players"));
    assert!(command::run(&h.server, "worlds").contains("forest"));

    // Starting a world from the prompt reaches the player as a portal.
    let started = command::run(&h.server, "start forest");
    let instance = match ada.next().await {
        Control::InstanceAdded { instance, slug, .. } => {
            assert_eq!(slug, "forest");
            instance
        }
        other => panic!("expected InstanceAdded after {started:?}, got {other:?}"),
    };
    assert!(command::run(&h.server, &format!("stop {instance}")).starts_with("stopped"));
    assert_eq!(ada.next().await, Control::InstanceClosed { instance });

    // Kicking closes the connection, which the client sees as a disconnection.
    assert_eq!(command::run(&h.server, &format!("kick {ada_id}")), format!("kicked player {ada_id}"));
    assert!(
        timeout(T, read_control(&mut ada.recv)).await.unwrap().is_err(),
        "a kicked player stays connected"
    );
    // And the server forgets them.
    for _ in 0..50 {
        if h.hub_players().is_empty() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("the kicked player is still on the roster");
}

/// Items: the server's whole job is that exactly one player holds one, and
/// that everybody is told who. It never reads the world, so "item 7" is just
/// a number to it.
#[tokio::test]
async fn one_player_holds_an_item_and_everybody_is_told() {
    let harness = Harness::start("items", 4, &["hub"]).await;
    let (mut ada, ada_id, _) = harness.join("Ada").await;
    let (mut bob, bob_id, _) = harness.join("Bob").await;
    assert!(matches!(ada.next().await, Control::PeerJoined(Peer { id, .. }) if id == bob_id));
    assert!(matches!(bob.next().await, Control::PeerJoined(Peer { id, .. }) if id == ada_id));

    // Ada picks it up: both of them hear about it, Ada included, because the
    // grab is only real once the server says so.
    ada.say(Control::Grab { item: 7 }).await;
    assert_eq!(ada.next().await, Control::Held { item: 7, by: ada_id });
    assert_eq!(bob.next().await, Control::Held { item: 7, by: ada_id });

    // Bob takes it off her. The server cannot know whether the world allows
    // that, so it grants it and tells Ada she has lost it.
    bob.say(Control::Grab { item: 7 }).await;
    assert_eq!(ada.next().await, Control::Held { item: 7, by: bob_id });
    assert_eq!(bob.next().await, Control::Held { item: 7, by: bob_id });

    // Ada letting go now must not drop Bob's item.
    ada.say(Control::Release { item: 7 }).await;
    // Bob moving it still works, which proves he is still the holder.
    let at = Transform::from_f32([1.0, 2.0, 3.0], [0.0, 0.0, 0.0, 1.0]);
    bob.conn.send_datagram(Datagram::Item { item: 7, at }.encode().into()).unwrap();
    assert_eq!(ada.datagram().await, Datagram::ItemRelay { from: bob_id, item: 7, at });

    // Ada is not the holder, so her item datagram goes nowhere. Proven by
    // sending it and then a pose: the pose arrives, the item never does.
    let elsewhere = Transform::from_f32([9.0, 9.0, 9.0], [0.0, 0.0, 0.0, 1.0]);
    ada.conn.send_datagram(Datagram::Item { item: 7, at: elsewhere }.encode().into()).unwrap();
    let pose = Pose { seq: 1, flags: 0, head: at, left: at, right: at };
    ada.conn.send_datagram(Datagram::Pose(pose).encode().into()).unwrap();
    assert_eq!(bob.datagram().await, Datagram::PoseRelay { from: ada_id, pose });

    // Bob lets go properly.
    bob.say(Control::Release { item: 7 }).await;
    assert_eq!(ada.next().await, Control::Held { item: 7, by: hearth_protocol::NOBODY });
    assert_eq!(bob.next().await, Control::Held { item: 7, by: hearth_protocol::NOBODY });
}

#[tokio::test]
async fn what_a_leaver_carried_falls_and_an_arrival_is_told_what_is_held() {
    let harness = Harness::start("items-leave", 4, &["hub"]).await;
    let (mut ada, ada_id, _) = harness.join("Ada").await;
    ada.say(Control::Grab { item: 2 }).await;
    assert_eq!(ada.next().await, Control::Held { item: 2, by: ada_id });

    // Somebody arriving is handed what is already in hands.
    let (mut bob, _, _) = harness.join("Bob").await;
    assert!(matches!(bob.next().await, Control::PeerJoined(Peer { id, .. }) if id == ada_id));
    assert_eq!(bob.next().await, Control::Held { item: 2, by: ada_id });

    // Ada quits without releasing: the item must not stay owned by a ghost.
    ada.conn.close(0u8.into(), b"bye");
    drop(ada);
    assert_eq!(bob.next().await, Control::Held { item: 2, by: hearth_protocol::NOBODY });
    assert!(matches!(bob.next().await, Control::PeerLeft { id } if id == ada_id));

    // And it can be picked up again.
    bob.say(Control::Grab { item: 2 }).await;
    assert!(matches!(bob.next().await, Control::Held { item: 2, .. }));
}

/// Health is the server's and nobody else's: a client claims a hit, and the
/// server decides whether anything came of it (docs/TRUST.md).
#[tokio::test]
async fn the_server_owns_health_and_death() {
    let harness = Harness::start("combat", 4, &["hub"]).await;
    let (mut ada, ada_id, _) = harness.join("Ada").await;
    let (mut bob, bob_id, _) = harness.join("Bob").await;
    assert!(matches!(ada.next().await, Control::PeerJoined(Peer { id, .. }) if id == bob_id));
    assert!(matches!(bob.next().await, Control::PeerJoined(Peer { id, .. }) if id == ada_id));

    // Both say where they are, close enough together to be in a fight.
    let near = Transform::from_f32([0.0, 0.0, 0.0], [0.0, 0.0, 0.0, 1.0]);
    for who in [&ada, &bob] {
        let pose = Pose { seq: 1, flags: 0, head: near, left: near, right: near };
        who.conn.send_datagram(Datagram::Pose(pose).encode().into()).unwrap();
    }
    // Let the poses land before anything is claimed against them.
    tokio::time::sleep(std::time::Duration::from_millis(80)).await;

    // Four hits put somebody down, and everybody hears every step of it.
    for shot in 1..=4 {
        ada.say(Control::Hit { target: bob_id }).await;
        let expected = hearth_protocol::MAX_HEALTH.saturating_sub(25 * shot);
        assert_eq!(ada.next().await, Control::Health { player: bob_id, hp: expected });
        assert_eq!(bob.next().await, Control::Health { player: bob_id, hp: expected });
    }
    assert_eq!(ada.next().await, Control::Died { player: bob_id, by: ada_id });
    assert_eq!(bob.next().await, Control::Died { player: bob_id, by: ada_id });

    // Hitting a corpse does nothing, so it cannot be used to deny a respawn.
    // Proven by asking, then respawning: the respawn is the next thing heard.
    ada.say(Control::Hit { target: bob_id }).await;
    bob.say(Control::Spawned { player: bob_id }).await;
    assert_eq!(bob.next().await, Control::Health { player: bob_id, hp: hearth_protocol::MAX_HEALTH });
    assert_eq!(bob.next().await, Control::Spawned { player: bob_id });
}

/// The checks the server can make without ever reading a world.
#[tokio::test]
async fn a_hit_claimed_from_far_away_or_at_yourself_is_dropped() {
    let harness = Harness::start("range", 4, &["hub"]).await;
    let (mut ada, ada_id, _) = harness.join("Ada").await;
    let (mut bob, bob_id, _) = harness.join("Bob").await;
    assert!(matches!(ada.next().await, Control::PeerJoined(Peer { id, .. }) if id == bob_id));
    assert!(matches!(bob.next().await, Control::PeerJoined(Peer { id, .. }) if id == ada_id));

    // Bob says he is a long way off.
    let here = Transform::from_f32([0.0, 0.0, 0.0], [0.0, 0.0, 0.0, 1.0]);
    let far = Transform::from_f32([60.0, 0.0, 0.0], [0.0, 0.0, 0.0, 1.0]);
    ada.conn
        .send_datagram(
            Datagram::Pose(Pose { seq: 1, flags: 0, head: here, left: here, right: here }).encode().into(),
        )
        .unwrap();
    bob.conn
        .send_datagram(
            Datagram::Pose(Pose { seq: 1, flags: 0, head: far, left: far, right: far }).encode().into(),
        )
        .unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(80)).await;

    // Neither of these may do anything: out of range, and at herself.
    ada.say(Control::Hit { target: bob_id }).await;
    ada.say(Control::Hit { target: ada_id }).await;

    // Something that definitely does work, to prove the connection is alive
    // and the two dropped claims really did produce nothing.
    ada.say(Control::Grab { item: 3 }).await;
    assert_eq!(ada.next().await, Control::Held { item: 3, by: ada_id });
}

/// Stored data is the player's own, namespaced by world, and survives them
/// leaving (docs/TRUST.md).
#[tokio::test]
async fn stored_data_comes_back() {
    let harness = Harness::start("store", 4, &["hub"]).await;
    let (mut ada, _, _, flags) =
        Client::join_with(harness.addr, &harness.cert, "typed", &harness.ticket("a1", "Ada")).await;
    assert_ne!(flags & welcome::SAVES, 0, "an account gets saves");

    // Nothing stored yet.
    ada.say(Control::Fetch { key: "score".into() }).await;
    assert_eq!(ada.next().await, Control::Value { key: "score".into(), value: Vec::new() });

    ada.say(Control::Store { key: "score".into(), value: vec![4, 2] }).await;
    ada.say(Control::Fetch { key: "score".into() }).await;
    assert_eq!(ada.next().await, Control::Value { key: "score".into(), value: vec![4, 2] });

    // Somebody else, with an account of their own, sees their own nothing.
    let (mut bob, _, _, _) =
        Client::join_with(harness.addr, &harness.cert, "Bob", &harness.ticket("b2", "Bob")).await;
    // Bob hears about Ada before he hears his own answer, with the id her
    // ticket gave her here, so his game can look her profile up.
    assert!(matches!(bob.next().await, Control::PeerJoined(Peer { ref account, .. }) if account == "a1"));
    bob.say(Control::Fetch { key: "score".into() }).await;
    assert_eq!(bob.next().await, Control::Value { key: "score".into(), value: Vec::new() });

    // Ada hears about Bob arriving before anything else of hers.
    assert!(matches!(ada.next().await, Control::PeerJoined(Peer { .. })));

    // A value right on the limit goes through whole. Anything over it cannot
    // even be encoded, and the quotas have their own tests in store.rs.
    let full = vec![7u8; hearth_protocol::MAX_VALUE_LEN];
    ada.say(Control::Store { key: "score".into(), value: full.clone() }).await;
    ada.say(Control::Fetch { key: "score".into() }).await;
    assert_eq!(ada.next().await, Control::Value { key: "score".into(), value: full });

    // And an empty value deletes it.
    ada.say(Control::Store { key: "score".into(), value: Vec::new() }).await;
    ada.say(Control::Fetch { key: "score".into() }).await;
    assert_eq!(ada.next().await, Control::Value { key: "score".into(), value: Vec::new() });
}

#[tokio::test]
async fn chat_reaches_the_ember_and_a_flood_is_trimmed() {
    let harness = Harness::start("chat", 4, &["hub"]).await;
    let (mut ada, ada_id, _) = harness.join("Ada").await;
    let (mut bob, _, _) = harness.join("Bob").await;
    assert!(matches!(bob.next().await, Control::PeerJoined(Peer { .. })));
    assert!(matches!(ada.next().await, Control::PeerJoined(Peer { .. })));

    // Everybody hears it, the speaker included: the echo is the receipt.
    ada.say(Control::Say { text: "ahoj".into() }).await;
    let said = Control::Said { from: ada_id, text: "ahoj".into() };
    assert_eq!(bob.next().await, said);
    assert_eq!(ada.next().await, said);

    // Seven at once: the burst goes through and the rest are dropped, without
    // kicking anybody for typing quickly.
    for i in 0..7 {
        ada.say(Control::Say { text: format!("line {i}") }).await;
    }
    ada.say(Control::Fetch { key: "x".into() }).await;
    let mut heard = 0;
    loop {
        match ada.next().await {
            Control::Said { .. } => heard += 1,
            Control::Value { .. } => break,
            other => panic!("expected chat, got {other:?}"),
        }
    }
    assert_eq!(heard, 4, "one line of the burst went on the first message");
}

#[tokio::test]
async fn a_guest_plays_but_nothing_is_saved_and_typing_a_name_claims_nothing() {
    let harness = Harness::start("guest-saves", 4, &["hub"]).await;
    let (mut ada, _, _, _) =
        Client::join_with(harness.addr, &harness.cert, "x", &harness.ticket("a1", "Ada")).await;
    ada.say(Control::Store { key: "score".into(), value: vec![9] }).await;

    // A guest typing the same display name is somebody else entirely.
    let (mut guest, _, _, flags) = Client::join_with(harness.addr, &harness.cert, "Ada", "").await;
    assert_eq!(flags & welcome::SAVES, 0, "a guest is told there are no saves");
    assert!(matches!(guest.next().await, Control::PeerJoined(Peer { ref account, .. }) if account == "a1"));
    guest.say(Control::Fetch { key: "score".into() }).await;
    assert_eq!(guest.next().await, Control::Value { key: "score".into(), value: Vec::new() });
    // And a write is answered with what is kept for them: nothing.
    guest.say(Control::Store { key: "score".into(), value: vec![1] }).await;
    assert_eq!(guest.next().await, Control::Value { key: "score".into(), value: Vec::new() });

    // The account's name is the Fireplace's, not the one typed.
    assert!(harness.hub_players().iter().any(|(_, name)| name == "Ada"));
    assert!(!harness.hub_players().iter().any(|(_, name)| name == "x"));
}

#[tokio::test]
async fn saves_follow_the_account_across_a_reconnect() {
    let harness = Harness::start("saves-reconnect", 4, &["hub"]).await;
    let ticket = harness.ticket("a1", "Ada");
    let (mut ada, _, _, _) = Client::join_with(harness.addr, &harness.cert, "x", &ticket).await;
    ada.say(Control::Store { key: "score".into(), value: vec![7] }).await;
    ada.say(Control::Fetch { key: "score".into() }).await;
    assert_eq!(ada.next().await, Control::Value { key: "score".into(), value: vec![7] });

    // Signing in again from somewhere else ends the first session.
    let (mut again, _, _, _) =
        Client::join_with(harness.addr, &harness.cert, "x", &harness.ticket("a1", "Ada")).await;
    timeout(T, ada.conn.closed()).await.expect("the older session is closed");
    again.say(Control::Fetch { key: "score".into() }).await;
    loop {
        match again.next().await {
            Control::Value { value, .. } => break assert_eq!(value, vec![7], "the save came along"),
            _ => continue,
        }
    }
}

#[tokio::test]
async fn guests_can_be_turned_away_and_a_bad_ticket_always_is() {
    let harness = Harness::start_with("no-guests", &[], |config| config.allow_guests = false).await;
    let mut guest = Client::connect(harness.addr, &harness.cert, PROTOCOL_VERSION, "Ada").await;
    assert_eq!(guest.next().await, Control::Reject { reason: RejectReason::AccountRequired });

    // A ticket for another Flame -- the replay this design exists to stop.
    let elsewhere = harness.issuer.fresh(&"ab".repeat(32), "a1", "Ada");
    let mut replayed =
        Client::connect_with(harness.addr, &harness.cert, PROTOCOL_VERSION, "Ada", &elsewhere).await;
    assert_eq!(replayed.next().await, Control::Reject { reason: RejectReason::BadTicket });
    // One signed by somebody who is not our Fireplace.
    let forged = Issuer::default().fresh(harness.server.tickets.certificate(), "a1", "Ada");
    let mut forger = Client::connect_with(harness.addr, &harness.cert, PROTOCOL_VERSION, "Ada", &forged).await;
    assert_eq!(forger.next().await, Control::Reject { reason: RejectReason::BadTicket });

    // And the real thing gets in.
    let (_, _, _, flags) =
        Client::join_with(harness.addr, &harness.cert, "Ada", &harness.ticket("a1", "Ada")).await;
    assert_ne!(flags & welcome::SAVES, 0);
}

#[tokio::test]
async fn an_instance_can_be_full_and_a_player_can_start_only_so_many() {
    let harness = Harness::start_with("instance-limits", &["forest"], |config| {
        config.max_players_per_instance = Limit::of(2);
        config.max_instances_per_player = Limit::of(1);
        config.max_instances = Limit::NONE;
        config.client_instancing = true;
    })
    .await;
    let (mut ada, _, _) = harness.join("Ada").await;
    let (mut bob, _, _) = harness.join("Bob").await;
    // The hub holds two, so a third is turned away at the door.
    let mut cy = Client::connect(harness.addr, &harness.cert, PROTOCOL_VERSION, "Cy").await;
    assert_eq!(cy.next().await, Control::Reject { reason: RejectReason::ServerFull });

    assert!(matches!(ada.next().await, Control::PeerJoined(_)));
    ada.say(create("forest", "one", ember::PUBLIC)).await;
    let first = loop {
        match ada.next().await {
            Control::Moved { instance } => break instance,
            _ => continue,
        }
    };
    assert_ne!(first, HUB_INSTANCE);
    // Ada already has one running, so a second is refused where she stands.
    ada.say(create("forest", "two", ember::PUBLIC)).await;
    loop {
        match ada.next().await {
            Control::Moved { instance } => break assert_eq!(instance, first),
            _ => continue,
        }
    }

    // Bob may join Ada's; it then holds two, and is full.
    bob.say(Control::JoinInstance { instance: first }).await;
    loop {
        match bob.next().await {
            Control::Moved { instance } => break assert_eq!(instance, first),
            _ => continue,
        }
    }
    let (mut cy, _, _) = harness.join("Cy").await;
    cy.say(Control::JoinInstance { instance: first }).await;
    loop {
        match cy.next().await {
            Control::Moved { instance } => break assert_eq!(instance, HUB_INSTANCE, "full, so Cy stays"),
            _ => continue,
        }
    }
}
