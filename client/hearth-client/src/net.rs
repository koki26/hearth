//! QUIC client, independent of Godot: one background thread owns the
//! connection, the caller polls events and pushes poses.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};

use anyhow::{Context, bail};
use hearth_protocol::{
    ALPN, Control, Datagram, MAX_WORLD_SIZE, PROTOCOL_VERSION, Pose, RejectReason, Transform, World,
};
use quinn::crypto::rustls::QuicClientConfig;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{DigitallySignedStruct, SignatureScheme};
use tokio::sync::mpsc::{UnboundedSender, unbounded_channel};

const CERT_NAME: &str = "hearth";
const CONNECT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// What the Godot side pushes towards the server.
#[derive(Debug)]
pub enum Outgoing {
    Pose(Pose),
    /// One encoded Opus frame.
    Voice(Vec<u8>),
    JoinInstance(u16),
    /// World slug, the ember's name, and who it is open to (`ember::*`).
    CreateInstance(String, String, u8),
    /// Download a world into the cache directory, unless it is already there.
    /// Size and hash come from the `WorldAdded` that advertised it.
    RequestWorld(World),
    Grab(u16),
    Release(u16),
    /// Where an item we hold is now.
    Item {
        item: u16,
        at: Transform,
    },
    /// A shot left the muzzle here, pointing this way. A tracer for everyone
    /// else; whether it hit anybody goes separately as `Hit`.
    Shot {
        at: Transform,
    },
    /// "I hit them." A claim: the server decides what comes of it, and a
    /// client never sets anybody's health, its own included.
    Hit {
        target: u16,
    },
    /// Ask to come back. Refused unless we are actually dead.
    Respawn,
    /// Keep this against our name, in this world. Empty value deletes.
    Store {
        key: String,
        value: Vec<u8>,
    },
    Fetch {
        key: String,
    },
    /// A line of chat for everyone in this ember. Already validated.
    Say(String),
}

#[derive(Debug)]
pub enum Event {
    /// A chat line, ours included (the server echoes it back).
    Said {
        from: u16,
        text: String,
    },
    Connected {
        your_id: u16,
        tick_hz: u8,
        instance: u16,
        flags: u8,
    },
    Rejected(RejectReason),
    PeerJoined {
        id: u16,
        name: String,
    },
    PeerLeft {
        id: u16,
    },
    PeerPose {
        from: u16,
        pose: Pose,
    },
    PeerVoice {
        from: u16,
        frame: Vec<u8>,
    },
    WorldAdded(World),
    InstanceAdded {
        instance: u16,
        slug: String,
        players: u16,
        name: String,
        kind: u8,
    },
    InstanceClosed {
        instance: u16,
    },
    InstanceCount {
        instance: u16,
        players: u16,
    },
    Moved {
        instance: u16,
    },
    /// Who holds an item now; `by` is `hearth_protocol::NOBODY` for nobody.
    /// The server decides, so this is also the answer to our own `Grab`.
    Held {
        item: u16,
        by: u16,
    },
    /// Where an item somebody else is carrying has got to.
    ItemMoved {
        item: u16,
        at: Transform,
    },
    /// Somebody fired. Draw a tracer; it has already been rate limited.
    ShotFired {
        from: u16,
        at: Transform,
    },
    /// The only thing that sets anybody's health, our own included.
    Health {
        player: u16,
        hp: u8,
    },
    /// `by` is `hearth_protocol::NOBODY` when nobody killed them.
    Died {
        player: u16,
        by: u16,
    },
    Spawned {
        player: u16,
    },
    /// What the server has stored for us under `key`. Empty means nothing.
    Value {
        key: String,
        value: Vec<u8>,
    },
    /// A world is unpacked-ready on disk: a verified `.hworld` file.
    WorldReady {
        slug: String,
        path: PathBuf,
    },
    WorldFailed {
        slug: String,
        reason: String,
    },
    Disconnected(String),
}

pub struct Client {
    events: Receiver<Event>,
    outgoing: UnboundedSender<Outgoing>,
}

impl Client {
    /// Starts connecting in the background; failures arrive as `Disconnected`.
    /// Downloaded worlds are cached in `cache_dir`, named by their hash.
    pub fn connect(host: String, port: u16, name: String, cache_dir: PathBuf) -> Self {
        let (event_tx, events) = channel();
        let (outgoing, outgoing_rx) = unbounded_channel();
        std::thread::spawn(move || {
            let runtime = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
                Ok(runtime) => runtime,
                Err(err) => {
                    let _ = event_tx.send(Event::Disconnected(err.to_string()));
                    return;
                }
            };
            let result = runtime.block_on(run(host, port, name, cache_dir, &event_tx, outgoing_rx));
            let _ = event_tx.send(Event::Disconnected(match result {
                Ok(()) => "connection closed".into(),
                Err(err) => format!("{err:#}"),
            }));
        });
        Self { events, outgoing }
    }

    pub fn poll(&self) -> Option<Event> {
        self.events.try_recv().ok()
    }

    pub fn send(&self, message: Outgoing) {
        let _ = self.outgoing.send(message);
    }
}

async fn run(
    host: String,
    port: u16,
    name: String,
    cache_dir: PathBuf,
    events: &Sender<Event>,
    mut outgoing: tokio::sync::mpsc::UnboundedReceiver<Outgoing>,
) -> anyhow::Result<()> {
    hearth_protocol::validate_name(&name)?;
    let (endpoint, conn) = connect(&host, port).await?;

    let (mut send, mut recv) = conn.open_bi().await?;
    let mut buf = Vec::new();
    Control::Hello { version: PROTOCOL_VERSION, name }.encode(&mut buf);
    send.write_all(&buf).await?;

    // The control stream is read in a task of its own rather than in the
    // `select!` below, because `read_exact` is not cancel-safe: a message
    // half-read when another branch wins would lose those bytes and desync
    // the stream for good.
    let mut control = tokio::spawn({
        let events = events.clone();
        async move {
            loop {
                if !handle_control(read_control(&mut recv).await?, &events)? {
                    return Ok(()); // A `Reject` ends the conversation.
                }
            }
        }
    });

    let mut voice_seq = 0u16;
    loop {
        tokio::select! {
            finished = &mut control => return match finished {
                Ok(Ok(())) => Ok(()),
                // The connection going away may be a refused join whose
                // message we never got to read.
                Ok(Err(err)) => rejected_or(&conn, events, err),
                Err(err) => Err(err.into()),
            },
            datagram = conn.read_datagram() => match datagram {
                Ok(bytes) => match Datagram::decode(&bytes) {
                    Ok(Datagram::PoseRelay { from, pose }) => send_event(events, Event::PeerPose { from, pose })?,
                    Ok(Datagram::VoiceRelay { from, frame, .. }) => {
                        send_event(events, Event::PeerVoice { from, frame })?
                    }
                    Ok(Datagram::ItemRelay { item, at, .. }) => {
                        send_event(events, Event::ItemMoved { item, at })?
                    }
                    Ok(Datagram::ShotRelay { from, at }) => {
                        send_event(events, Event::ShotFired { from, at })?
                    }
                    _ => {}
                },
                Err(err) => return rejected_or(&conn, events, err.into()),
            },
            message = outgoing.recv() => match message {
                Some(Outgoing::Pose(pose)) => { let _ = conn.send_datagram(Datagram::Pose(pose).encode().into()); }
                Some(Outgoing::Voice(frame)) => {
                    voice_seq = voice_seq.wrapping_add(1);
                    let voice = Datagram::Voice { seq: voice_seq, frame };
                    let _ = conn.send_datagram(voice.encode().into());
                }
                Some(Outgoing::Item { item, at }) => {
                    let _ = conn.send_datagram(Datagram::Item { item, at }.encode().into());
                }
                Some(Outgoing::Shot { at }) => {
                    let _ = conn.send_datagram(Datagram::Shot { at }.encode().into());
                }
                Some(Outgoing::Grab(item)) => send_control(&mut send, Control::Grab { item }).await?,
                Some(Outgoing::Release(item)) => send_control(&mut send, Control::Release { item }).await?,
                Some(Outgoing::Hit { target }) => send_control(&mut send, Control::Hit { target }).await?,
                Some(Outgoing::Respawn) => {
                    // `player` is ignored on the way in: the server respawns
                    // whoever asked, and nobody else.
                    send_control(&mut send, Control::Spawned { player: 0 }).await?;
                }
                Some(Outgoing::Store { key, value }) => {
                    send_control(&mut send, Control::Store { key, value }).await?;
                }
                Some(Outgoing::Fetch { key }) => send_control(&mut send, Control::Fetch { key }).await?,
                Some(Outgoing::Say(text)) => send_control(&mut send, Control::Say { text }).await?,
                Some(Outgoing::JoinInstance(instance)) => {
                    send_control(&mut send, Control::JoinInstance { instance }).await?;
                }
                Some(Outgoing::CreateInstance(slug, name, kind)) => {
                    send_control(&mut send, Control::CreateInstance { slug, name, kind }).await?;
                }
                // Downloads run beside the game traffic: a big world must not
                // stop poses and voice from flowing.
                Some(Outgoing::RequestWorld(world)) => {
                    let (conn, events, dir) = (conn.clone(), events.clone(), cache_dir.clone());
                    tokio::spawn(async move {
                        let slug = world.slug.clone();
                        let event = match fetch_world(&conn, &world, &dir).await {
                            Ok(path) => Event::WorldReady { slug, path },
                            Err(err) => Event::WorldFailed { slug, reason: format!("{err:#}") },
                        };
                        let _ = events.send(event);
                    });
                }
                // The Client handle was dropped.
                None => {
                    control.abort();
                    conn.close(0u8.into(), b"bye");
                    endpoint.wait_idle().await;
                    return Ok(());
                }
            },
        }
    }
}

/// Tries every address the host resolves to, so a name with both A and AAAA
/// records still reaches a server listening on only one of them.
async fn connect(host: &str, port: u16) -> anyhow::Result<(quinn::Endpoint, quinn::Connection)> {
    let addrs: Vec<_> = tokio::net::lookup_host((host, port))
        .await
        .with_context(|| format!("resolving {host}:{port}"))?
        .collect();
    let mut last = None;
    for addr in &addrs {
        let bind = if addr.is_ipv4() {
            SocketAddr::from(([0, 0, 0, 0], 0))
        } else {
            SocketAddr::from((std::net::Ipv6Addr::UNSPECIFIED, 0))
        };
        let mut endpoint = quinn::Endpoint::client(bind)?;
        endpoint.set_default_client_config(client_config()?);
        match tokio::time::timeout(CONNECT_TIMEOUT, endpoint.connect(*addr, CERT_NAME)?).await {
            Ok(Ok(conn)) => return Ok((endpoint, conn)),
            Ok(Err(err)) => last = Some(anyhow::Error::new(err)),
            Err(_) => last = Some(anyhow::anyhow!("timed out")),
        }
    }
    Err(last.unwrap_or_else(|| anyhow::anyhow!("host resolved to no address")))
        .with_context(|| format!("connecting to {host}:{port}"))
}

fn send_event(events: &Sender<Event>, event: Event) -> anyhow::Result<()> {
    events.send(event).map_err(|_| anyhow::anyhow!("client dropped"))
}

/// Turns one control message into events. False means the server is done with
/// us: a `Reject` is the end of the conversation.
fn handle_control(msg: Control, events: &Sender<Event>) -> anyhow::Result<bool> {
    match msg {
        Control::Welcome { your_id, tick_hz, instance, flags } => {
            send_event(events, Event::Connected { your_id, tick_hz, instance, flags })?
        }
        Control::Reject { reason } => {
            send_event(events, Event::Rejected(reason))?;
            return Ok(false);
        }
        Control::PeerJoined(peer) => send_event(events, Event::PeerJoined { id: peer.id, name: peer.name })?,
        Control::PeerLeft { id } => send_event(events, Event::PeerLeft { id })?,
        Control::WorldAdded(world) => send_event(events, Event::WorldAdded(world))?,
        Control::InstanceAdded { instance, slug, players, name, kind } => {
            send_event(events, Event::InstanceAdded { instance, slug, players, name, kind })?
        }
        Control::InstanceClosed { instance } => send_event(events, Event::InstanceClosed { instance })?,
        Control::InstanceCount { instance, players } => {
            send_event(events, Event::InstanceCount { instance, players })?
        }
        Control::Moved { instance } => send_event(events, Event::Moved { instance })?,
        Control::Held { item, by } => send_event(events, Event::Held { item, by })?,
        Control::Health { player, hp } => send_event(events, Event::Health { player, hp })?,
        Control::Died { player, by } => send_event(events, Event::Died { player, by })?,
        Control::Spawned { player } => send_event(events, Event::Spawned { player })?,
        Control::Value { key, value } => send_event(events, Event::Value { key, value })?,
        Control::Said { from, text } => send_event(events, Event::Said { from, text })?,
        other => anyhow::bail!("unexpected control message {other:?}"),
    }
    Ok(true)
}

/// A `Reject` can be lost in the race between the message and the close that
/// follows it, so the reason is also in the close frame. Without this, "the
/// server is full" reaches the player as "connection closed".
fn rejected_or(conn: &quinn::Connection, events: &Sender<Event>, err: anyhow::Error) -> anyhow::Result<()> {
    if let Some(quinn::ConnectionError::ApplicationClosed(frame)) = conn.close_reason()
        && let Some(reason) = RejectReason::from_close_frame(&frame.reason)
    {
        send_event(events, Event::Rejected(reason))?;
        return Ok(());
    }
    Err(err)
}

async fn send_control(send: &mut quinn::SendStream, msg: Control) -> anyhow::Result<()> {
    let mut buf = Vec::new();
    msg.encode(&mut buf);
    send.write_all(&buf).await?;
    Ok(())
}

/// The cache is addressed by content hash, so the same world from two servers
/// is downloaded once and a changed world can never be served from the cache.
pub fn cached_path(dir: &Path, world: &World) -> PathBuf {
    let mut name = String::with_capacity(70);
    for byte in world.sha256 {
        name.push_str(&format!("{byte:02x}"));
    }
    name.push_str(".hworld");
    dir.join(name)
}

/// Downloads one world on a stream of its own and verifies it before it is
/// handed to the game: a server could send anything, but not something whose
/// hash it did not advertise.
async fn fetch_world(conn: &quinn::Connection, world: &World, dir: &Path) -> anyhow::Result<PathBuf> {
    let target = cached_path(dir, world);
    if std::fs::metadata(&target).is_ok_and(|meta| meta.len() == world.size as u64) {
        return Ok(target);
    }
    if world.size > MAX_WORLD_SIZE {
        bail!("{} bytes is over the size limit", world.size);
    }
    let (mut send, mut recv) = conn.open_bi().await.context("opening a download stream")?;
    send_control(&mut send, Control::WorldRequest { slug: world.slug.clone() }).await?;
    send.finish()?;
    // Reading at most the advertised size caps what a server can make us hold.
    let bytes = recv.read_to_end(world.size as usize).await.context("reading the world")?;
    if bytes.len() != world.size as usize {
        bail!("expected {} bytes, got {}", world.size, bytes.len());
    }
    let digest = ring::digest::digest(&ring::digest::SHA256, &bytes);
    if digest.as_ref() != world.sha256 {
        bail!("the world does not match the hash the server advertised");
    }
    std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    // Written under a temporary name, so a half-written file is never cached.
    let partial = target.with_extension("part");
    std::fs::write(&partial, &bytes).with_context(|| format!("writing {}", partial.display()))?;
    std::fs::rename(&partial, &target)?;
    Ok(target)
}

async fn read_control(recv: &mut quinn::RecvStream) -> anyhow::Result<Control> {
    let mut prefix = [0; 2];
    recv.read_exact(&mut prefix).await?;
    let mut body = vec![0; Control::body_len(prefix)?];
    recv.read_exact(&mut body).await?;
    Ok(Control::decode(&body)?)
}

fn client_config() -> anyhow::Result<quinn::ClientConfig> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let mut crypto = rustls::ClientConfig::builder_with_provider(provider.clone())
        .with_protocol_versions(&[&rustls::version::TLS13])?
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(AcceptAnyServer(provider)))
        .with_no_client_auth();
    crypto.alpn_protocols = vec![ALPN.to_vec()];
    Ok(quinn::ClientConfig::new(Arc::new(QuicClientConfig::try_from(crypto)?)))
}

/// Accepts any server certificate: see docs/decisions.md D3. This is open to
/// man-in-the-middle attacks and must be replaced by trust-on-first-use before
/// any server is exposed to the internet.
#[derive(Debug)]
struct AcceptAnyServer(Arc<rustls::crypto::CryptoProvider>);

impl ServerCertVerifier for AcceptAnyServer {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(message, cert, dss, &self.0.signature_verification_algorithms)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(message, cert, dss, &self.0.signature_verification_algorithms)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.0.signature_verification_algorithms.supported_schemes()
    }
}
