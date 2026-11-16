//! Public Flames: the list on the game's server page.
//!
//! A Flame asks to be listed (`POST /flames/announce`, every minute). The
//! Fireplace believes nothing it says about where it is: it connects to the
//! address itself over QUIC, and lists it only if the handshake works and
//! shows the certificate the Flame named. A Flame it has not reached in
//! [`STALE`] drops off the list.

use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use quinn::crypto::rustls::QuicClientConfig;
use rustls::DigitallySignedStruct;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};

/// Off the list this long after the last time it was reached.
pub const STALE: u64 = 3 * 60;
/// Listed at most at once, so the list cannot be flooded into uselessness.
pub const MAX_LISTED: usize = 500;
const PROBE_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, serde::Serialize)]
pub struct Listed {
    pub name: String,
    /// `host:port`, as a player types it.
    pub address: String,
    /// SHA-256 of its certificate, hex, as the Fireplace saw it.
    pub certificate: String,
    pub players: u32,
    /// None for no limit.
    pub max_players: Option<u32>,
    pub guests: bool,
    /// The last time the Fireplace reached it.
    pub seen: u64,
}

#[derive(Default)]
pub struct Flames {
    /// By certificate.
    listed: HashMap<String, Listed>,
    /// Certificates taken off the list by an admin.
    /// shortcut: in memory only, so a restart lists them again; persist it if hiding becomes routine.
    hidden: HashSet<String>,
}

impl Flames {
    pub fn list(&mut self, now: u64) -> Vec<Listed> {
        self.listed.retain(|_, f| now.saturating_sub(f.seen) < STALE);
        let mut all: Vec<Listed> = self.listed.values().cloned().collect();
        all.sort_by(|a, b| b.players.cmp(&a.players).then_with(|| a.name.cmp(&b.name)));
        all
    }

    pub fn is_hidden(&self, certificate: &str) -> bool {
        self.hidden.contains(certificate)
    }

    /// Lists or refreshes one the Fireplace has just reached.
    pub fn reached(&mut self, flame: Listed) {
        if self.hidden.contains(&flame.certificate) {
            return;
        }
        if self.listed.len() >= MAX_LISTED && !self.listed.contains_key(&flame.certificate) {
            return;
        }
        self.listed.insert(flame.certificate.clone(), flame);
    }

    pub fn hide(&mut self, certificate: &str) -> bool {
        self.hidden.insert(certificate.to_owned());
        self.listed.remove(certificate).is_some()
    }

    pub fn unhide(&mut self, certificate: &str) -> bool {
        self.hidden.remove(certificate)
    }
}

/// Connects to `host:port` as a player would and returns the SHA-256 of the
/// certificate it shows, hex.
pub async fn probe(host: &str, port: u16) -> Result<String> {
    let addrs: Vec<SocketAddr> =
        tokio::net::lookup_host((host, port)).await.with_context(|| format!("resolving {host}"))?.collect();
    let Some(addr) = addrs.first() else {
        bail!("{host} resolves to nothing");
    };
    let bind: SocketAddr = if addr.is_ipv4() { ([0, 0, 0, 0], 0).into() } else { (std::net::Ipv6Addr::UNSPECIFIED, 0).into() };
    let mut endpoint = quinn::Endpoint::client(bind)?;
    endpoint.set_default_client_config(client_config()?);
    let connection = tokio::time::timeout(PROBE_TIMEOUT, endpoint.connect(*addr, "hearth")?)
        .await
        .context("timed out")??;
    let certificate = connection
        .peer_identity()
        .and_then(|id| id.downcast::<Vec<CertificateDer<'static>>>().ok())
        .and_then(|chain| chain.first().map(|c| ring::digest::digest(&ring::digest::SHA256, c.as_ref())))
        .map(|digest| digest.as_ref().iter().map(|b| format!("{b:02x}")).collect::<String>())
        .context("it showed no certificate")?;
    connection.close(0u8.into(), b"listed");
    endpoint.wait_idle().await;
    Ok(certificate)
}

fn client_config() -> Result<quinn::ClientConfig> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let mut crypto = rustls::ClientConfig::builder_with_provider(provider.clone())
        .with_protocol_versions(&[&rustls::version::TLS13])?
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(AnyCertificate(provider)))
        .with_no_client_auth();
    crypto.alpn_protocols = vec![hearth_protocol::ALPN.to_vec()];
    Ok(quinn::ClientConfig::new(Arc::new(QuicClientConfig::try_from(crypto)?)))
}

/// Any certificate passes the handshake: the probe is not trusting the Flame,
/// it is reading which certificate it shows, and compares that afterwards.
#[derive(Debug)]
struct AnyCertificate(Arc<rustls::crypto::CryptoProvider>);

impl ServerCertVerifier for AnyCertificate {
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

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        self.0.signature_verification_algorithms.supported_schemes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flame(cert: &str, players: u32, seen: u64) -> Listed {
        Listed {
            name: cert.into(),
            address: "example.org:7777".into(),
            certificate: cert.into(),
            players,
            max_players: None,
            guests: true,
            seen,
        }
    }

    #[test]
    fn the_busiest_come_first_the_silent_drop_off_and_hidden_stay_off() {
        let mut flames = Flames::default();
        flames.reached(flame("a", 1, 100));
        flames.reached(flame("b", 5, 100));
        assert_eq!(flames.list(100).iter().map(|f| f.name.as_str()).collect::<Vec<_>>(), ["b", "a"]);
        assert!(flames.list(100 + STALE).is_empty(), "not reached for too long");
        flames.hide("a");
        flames.reached(flame("a", 1, 500));
        assert!(flames.list(500).is_empty(), "hidden by an admin");
        flames.unhide("a");
        flames.reached(flame("a", 1, 500));
        assert_eq!(flames.list(500).len(), 1);
    }
}
