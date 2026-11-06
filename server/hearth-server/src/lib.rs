use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Context, bail};
use hearth_protocol::{ALPN, Control, Datagram, DecodeError, PROTOCOL_VERSION, Peer, RejectReason, flags};
use quinn::crypto::rustls::QuicServerConfig;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use serde::Deserialize;
use tokio::sync::mpsc;
use tokio::time::timeout;
use tracing::{debug, info, warn};

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
}

impl Default for Config {
    fn default() -> Self {
        Self { port: 7777, max_players: 16, data_dir: "data".into() }
    }
}

impl Config {
    pub fn parse(text: &str) -> anyhow::Result<Self> {
        let config: Self = toml::from_str(text)?;
        if config.max_players == 0 {
            bail!("max_players must be at least 1");
        }
        Ok(config)
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
    let mut crypto = rustls::ServerConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
        .with_protocol_versions(&[&rustls::version::TLS13])?
        .with_no_client_auth()
        .with_single_cert(vec![cert], key)?;
    crypto.alpn_protocols = vec![ALPN.to_vec()];

    let mut transport = quinn::TransportConfig::default();
    transport.max_concurrent_bidi_streams(1u8.into());
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
}

struct Room {
    max_players: u16,
    players: Mutex<HashMap<u16, Player>>,
}

fn broadcast_control(players: &HashMap<u16, Player>, except: u16, msg: Control) {
    for (&id, player) in players {
            // A full queue means the client stopped reading.
            if id != except && player.control.try_send(msg.clone()).is_err() {
            player.conn.close(0u8.into(), b"control backlog");
        }
    }
}

impl Room {
    fn relay(&self, from: u16, msg: &Datagram) {
        let bytes = bytes::Bytes::from(msg.encode());
        for (&id, player) in self.players.lock().unwrap().iter() {
            if id != from {
                let _ = player.conn.send_datagram(bytes.clone());
            }
        }
    }
}

pub async fn serve(endpoint: quinn::Endpoint, max_players: u16) {
    let room = Arc::new(Room { max_players, players: Mutex::default() });
    while let Some(incoming) = endpoint.accept().await {
        let room = room.clone();
        tokio::spawn(async move {
            let addr = incoming.remote_address();
            if let Err(err) = handle_connection(incoming, room).await {
                debug!(%addr, "connection ended: {err:#}");
            }
        });
    }
}

async fn handle_connection(incoming: quinn::Incoming, room: Arc<Room>) -> anyhow::Result<()> {
    let conn = incoming.await?;
    let addr = conn.remote_address();
    let (mut send, mut recv) = timeout(HANDSHAKE_TIMEOUT, conn.accept_bi()).await.context("handshake timeout")??;
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

    // Room size plus headroom: Welcome and the peer list are queued up front.
    let (control_tx, mut control_rx) = mpsc::channel(room.max_players as usize + 16);
    let joined = {
        let mut players = room.players.lock().unwrap();
        let free_id = (players.len() < room.max_players as usize)
            .then(|| (1..=u16::MAX).find(|id| !players.contains_key(id)))
            .flatten();
        if let Some(id) = free_id {
            control_tx.try_send(Control::Welcome { your_id: id, tick_hz: TICK_HZ })?;
            for (&peer_id, peer) in players.iter() {
                control_tx.try_send(Control::PeerJoined(Peer { id: peer_id, name: peer.name.clone() }))?;
            }
            broadcast_control(&players, id, Control::PeerJoined(Peer { id, name: name.clone() }));
            players.insert(id, Player { name: name.clone(), conn: conn.clone(), control: control_tx });
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

    let result = relay_datagrams(&conn, &room, id).await;

    {
        let mut players = room.players.lock().unwrap();
        players.remove(&id);
        broadcast_control(&players, id, Control::PeerLeft { id });
    }
    writer.abort();
    info!(id, %name, reason = %result.as_ref().err().map(|e| e.to_string()).unwrap_or_default(), "player left");
    Ok(())
}

async fn reject(conn: &quinn::Connection, mut send: quinn::SendStream, reason: RejectReason) -> anyhow::Result<()> {
    let mut buf = Vec::new();
    Control::Reject { reason }.encode(&mut buf);
    send.write_all(&buf).await?;
    send.finish()?;
    // Wait for the client to receive it before closing; closing discards unsent data.
    let _ = timeout(HANDSHAKE_TIMEOUT, send.stopped()).await;
    conn.close(0u8.into(), b"rejected");
    Ok(())
}

async fn relay_datagrams(conn: &quinn::Connection, room: &Room, id: u16) -> anyhow::Result<()> {
    // 1.5x the nominal rate plus a burst, so jitter isn't punished.
    let mut pose_limit = RateLimit::new(TICK_HZ as f32 * 1.5, 10.0);
    let mut voice_limit = RateLimit::new(75.0, 25.0); // 20 ms frames = 50/s
    loop {
        let data = conn.read_datagram().await?;
        let now = Instant::now();
        match Datagram::decode(&data) {
            Ok(Datagram::Pose(mut pose)) if pose_limit.allow(now) => {
                pose.flags &= flags::VR | flags::LEFT_HAND | flags::RIGHT_HAND;
                room.relay(id, &Datagram::PoseRelay { from: id, pose });
            }
            Ok(Datagram::Voice { seq, frame }) if voice_limit.allow(now) => {
                room.relay(id, &Datagram::VoiceRelay { from: id, seq, frame });
            }
            Ok(Datagram::Pose(_) | Datagram::Voice { .. }) => {} // rate limited
            Ok(other) => warn!(id, "dropped server-only datagram {other:?}"),
            Err(err) => debug!(id, "dropped bad datagram: {err}"),
        }
    }
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
    }
}
