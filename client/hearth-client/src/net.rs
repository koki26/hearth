//! QUIC client, independent of Godot: one background thread owns the
//! connection, the caller polls events and pushes poses.

use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};

use anyhow::Context;
use hearth_protocol::{ALPN, Control, Datagram, PROTOCOL_VERSION, Pose, RejectReason};
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
}

#[derive(Debug)]
pub enum Event {
    Connected { your_id: u16, tick_hz: u8 },
    Rejected(RejectReason),
    PeerJoined { id: u16, name: String },
    PeerLeft { id: u16 },
    PeerPose { from: u16, pose: Pose },
    PeerVoice { from: u16, frame: Vec<u8> },
    Disconnected(String),
}

pub struct Client {
    events: Receiver<Event>,
    outgoing: UnboundedSender<Outgoing>,
}

impl Client {
    /// Starts connecting in the background; failures arrive as `Disconnected`.
    pub fn connect(host: String, port: u16, name: String) -> Self {
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
            let result = runtime.block_on(run(host, port, name, &event_tx, outgoing_rx));
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
    events: &Sender<Event>,
    mut outgoing: tokio::sync::mpsc::UnboundedReceiver<Outgoing>,
) -> anyhow::Result<()> {
    hearth_protocol::validate_name(&name)?;
    let (endpoint, conn) = connect(&host, port).await?;

    let (mut send, mut recv) = conn.open_bi().await?;
    let mut buf = Vec::new();
    Control::Hello { version: PROTOCOL_VERSION, name }.encode(&mut buf);
    send.write_all(&buf).await?;

    let mut voice_seq = 0u16;
    loop {
        tokio::select! {
            msg = read_control(&mut recv) => match msg? {
                Control::Welcome { your_id, tick_hz } => send_event(events, Event::Connected { your_id, tick_hz })?,
                Control::Reject { reason } => {
                    send_event(events, Event::Rejected(reason))?;
                    return Ok(());
                }
                Control::PeerJoined(peer) => send_event(events, Event::PeerJoined { id: peer.id, name: peer.name })?,
                Control::PeerLeft { id } => send_event(events, Event::PeerLeft { id })?,
                other => anyhow::bail!("unexpected control message {other:?}"),
            },
            datagram = conn.read_datagram() => match Datagram::decode(&datagram?) {
                Ok(Datagram::PoseRelay { from, pose }) => send_event(events, Event::PeerPose { from, pose })?,
                Ok(Datagram::VoiceRelay { from, frame, .. }) => {
                    send_event(events, Event::PeerVoice { from, frame })?
                }
                _ => {}
            },
            message = outgoing.recv() => match message {
                Some(Outgoing::Pose(pose)) => { let _ = conn.send_datagram(Datagram::Pose(pose).encode().into()); }
                Some(Outgoing::Voice(frame)) => {
                    voice_seq = voice_seq.wrapping_add(1);
                    let voice = Datagram::Voice { seq: voice_seq, frame };
                    let _ = conn.send_datagram(voice.encode().into());
                }
                // The Client handle was dropped.
                None => {
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
