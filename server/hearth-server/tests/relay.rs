//! Hand-rolled QUIC clients against an in-process server.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use hearth_protocol::{ALPN, Control, Datagram, PROTOCOL_VERSION, Peer, Pose, RejectReason, Transform};
use hearth_server::{CERT_NAME, Roster, read_control};
use quinn::crypto::rustls::QuicClientConfig;
use rustls::pki_types::CertificateDer;
use tokio::sync::watch;
use tokio::time::timeout;

const T: Duration = Duration::from_secs(5);

struct Client {
    conn: quinn::Connection,
    send: quinn::SendStream,
    recv: quinn::RecvStream,
}

impl Client {
    async fn connect(addr: SocketAddr, cert: &CertificateDer<'static>, version: u16, name: &str) -> Self {
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
        let body_len = (buf.len() - 2) as u16;
        buf[..2].copy_from_slice(&body_len.to_le_bytes());
        send.write_all(&buf).await.unwrap();
        Self { conn, send, recv }
    }

    async fn next(&mut self) -> Control {
        timeout(T, read_control(&mut self.recv)).await.unwrap().unwrap()
    }

    async fn datagram(&self) -> Datagram {
        Datagram::decode(&timeout(T, self.conn.read_datagram()).await.unwrap().unwrap()).unwrap()
    }
}

async fn start(max_players: u16) -> (SocketAddr, CertificateDer<'static>, watch::Receiver<Roster>) {
    let (cert, key) = hearth_server::generate_cert().unwrap();
    let endpoint = hearth_server::endpoint(0, cert.clone(), key).unwrap();
    let port = endpoint.local_addr().unwrap().port();
    let (tx, roster) = watch::channel(Roster::new());
    tokio::spawn(hearth_server::serve_with_roster(endpoint, max_players, tx));
    (([127, 0, 0, 1], port).into(), cert, roster)
}

/// The GUI's player list, which updates on every join and leave.
async fn roster(rx: &mut watch::Receiver<Roster>) -> Roster {
    timeout(T, rx.changed()).await.unwrap().unwrap();
    rx.borrow_and_update().clone()
}

#[tokio::test]
async fn two_players_join_relay_and_leave() {
    let (addr, cert, mut players) = start(2).await;

    let mut ada = Client::connect(addr, &cert, PROTOCOL_VERSION, "Ada").await;
    assert_eq!(ada.next().await, Control::Welcome { your_id: 1, tick_hz: 20 });

    assert_eq!(roster(&mut players).await, vec![(1, "Ada".to_owned())]);

    let mut bob = Client::connect(addr, &cert, PROTOCOL_VERSION, "Bob").await;
    assert_eq!(bob.next().await, Control::Welcome { your_id: 2, tick_hz: 20 });
    assert_eq!(bob.next().await, Control::PeerJoined(Peer { id: 1, name: "Ada".into() }));
    assert_eq!(ada.next().await, Control::PeerJoined(Peer { id: 2, name: "Bob".into() }));

    assert_eq!(roster(&mut players).await, vec![(1, "Ada".to_owned()), (2, "Bob".to_owned())]);

    // Room is full.
    let mut cy = Client::connect(addr, &cert, PROTOCOL_VERSION, "Cy").await;
    assert_eq!(cy.next().await, Control::Reject { reason: RejectReason::ServerFull });

    let t = Transform::from_f32([0.0, 1.7, 0.0], [0.0, 0.0, 0.0, 1.0]);
    let pose = Pose { seq: 5, flags: 0xFF, head: t, left: t, right: t };
    ada.conn.send_datagram(Datagram::Pose(pose).encode().into()).unwrap();
    // Unknown flag bits are stripped before relaying.
    assert_eq!(bob.datagram().await, Datagram::PoseRelay { from: 1, pose: Pose { flags: 0b111, ..pose } });

    bob.conn.send_datagram(Datagram::Voice { seq: 9, frame: vec![1, 2, 3] }.encode().into()).unwrap();
    assert_eq!(ada.datagram().await, Datagram::VoiceRelay { from: 2, seq: 9, frame: vec![1, 2, 3] });

    bob.conn.close(0u8.into(), b"bye");
    assert_eq!(ada.next().await, Control::PeerLeft { id: 2 });
    assert_eq!(roster(&mut players).await, vec![(1, "Ada".to_owned())]);

    // Bob's slot (and id) is free again.
    let mut dee = Client::connect(addr, &cert, PROTOCOL_VERSION, "Dee").await;
    assert_eq!(dee.next().await, Control::Welcome { your_id: 2, tick_hz: 20 });
    assert_eq!(ada.next().await, Control::PeerJoined(Peer { id: 2, name: "Dee".into() }));
    let _ = ada.send.finish();
}

#[tokio::test]
async fn rejects_bad_version_and_bad_name() {
    let (addr, cert, _) = start(4).await;
    let mut old = Client::connect(addr, &cert, PROTOCOL_VERSION + 1, "Ada").await;
    assert_eq!(old.next().await, Control::Reject { reason: RejectReason::BadVersion });
    let mut bad = Client::connect(addr, &cert, PROTOCOL_VERSION, "a\u{7}b").await;
    assert_eq!(bad.next().await, Control::Reject { reason: RejectReason::BadName });
}

#[test]
fn certificate_persists_across_restarts() {
    let dir = std::env::temp_dir().join(format!("hearth-cert-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let (first, _) = hearth_server::load_or_create_cert(&dir).unwrap();
    let (second, _) = hearth_server::load_or_create_cert(&dir).unwrap();
    assert_eq!(first, second);
    std::fs::remove_dir_all(&dir).unwrap();
}
