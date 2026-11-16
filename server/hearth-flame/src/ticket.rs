//! Who somebody is, as far as this Flame is allowed to know (docs/TRUST.md).
//!
//! A signed-in player's client asks the Fireplace for a ticket naming this
//! Flame's certificate, and hands it over in `Hello`. The Flame checks it
//! offline against the Fireplace's public key and learns two things: a
//! display name, and an id that is stable here and means nothing anywhere
//! else. Never the account, never a handle, never a password.
//!
//! The key is fetched from the Fireplace once and kept in
//! `data/fireplace.key`, so a Flame whose Fireplace is unreachable still lets
//! people in with the key it already has.

use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::Engine as _;
use ring::signature::{ED25519, UnparsedPublicKey};
use tracing::{info, warn};

const PREFIX: &str = "v1.";
const BASE64: base64::engine::general_purpose::GeneralPurpose =
    base64::engine::general_purpose::URL_SAFE_NO_PAD;
/// Clocks disagree. A ticket "from the future" by less than this is fine.
const SKEW_SECONDS: u64 = 60;
const RETRY: Duration = Duration::from_secs(30);

/// What a Flame learns from a good ticket.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Player {
    /// Stable on this Flame, hex, and only here.
    pub id: String,
    pub name: String,
}

/// Why a ticket was not good enough. Logged, never shown to anybody else.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refused {
    /// This Flame has no Fireplace key yet, or none configured.
    NoKey,
    Malformed,
    BadSignature,
    /// Signed, but for another Flame.
    NotForUs,
    Expired,
}

#[derive(serde::Deserialize)]
struct Claims {
    typ: String,
    aud: String,
    pid: String,
    name: String,
    iat: u64,
    exp: u64,
}

pub struct Tickets {
    /// The SHA-256 of our own certificate, hex: what a ticket must name.
    certificate: String,
    key: RwLock<Option<[u8; 32]>>,
}

impl Tickets {
    pub fn new(certificate: &[u8]) -> Self {
        let digest = ring::digest::digest(&ring::digest::SHA256, certificate);
        Self { certificate: hex(digest.as_ref()), key: RwLock::new(None) }
    }

    pub fn certificate(&self) -> &str {
        &self.certificate
    }

    pub fn has_key(&self) -> bool {
        self.key.read().unwrap().is_some()
    }

    pub fn set_key(&self, key: [u8; 32]) {
        *self.key.write().unwrap() = Some(key);
    }

    pub fn check(&self, ticket: &str) -> Result<Player, Refused> {
        self.check_at(ticket, now())
    }

    pub fn check_at(&self, ticket: &str, now: u64) -> Result<Player, Refused> {
        let key = self.key.read().unwrap().ok_or(Refused::NoKey)?;
        let rest = ticket.strip_prefix(PREFIX).ok_or(Refused::Malformed)?;
        let (payload, signature) = rest.split_once('.').ok_or(Refused::Malformed)?;
        let signature = BASE64.decode(signature).map_err(|_| Refused::Malformed)?;
        UnparsedPublicKey::new(&ED25519, key)
            .verify(format!("{PREFIX}{payload}").as_bytes(), &signature)
            .map_err(|_| Refused::BadSignature)?;
        let claims: Claims = BASE64
            .decode(payload)
            .ok()
            .and_then(|json| serde_json::from_slice(&json).ok())
            .ok_or(Refused::Malformed)?;
        if claims.typ != "join" || claims.aud != self.certificate {
            return Err(Refused::NotForUs);
        }
        if claims.exp <= now || claims.iat > now + SKEW_SECONDS {
            return Err(Refused::Expired);
        }
        // The id names a file and the name is shown to everybody, so both are
        // checked even though the Fireplace signed them.
        let id_ok = !claims.pid.is_empty()
            && claims.pid.len() <= 64
            && claims.pid.bytes().all(|b| b.is_ascii_hexdigit());
        if !id_ok || hearth_protocol::validate_name(&claims.name).is_err() {
            return Err(Refused::Malformed);
        }
        Ok(Player { id: claims.pid, name: claims.name })
    }
}

/// Loads the key we kept last time, then asks the Fireplace for the current
/// one on a thread of its own, retrying until it answers. Nobody waits on it:
/// a guest gets in either way, and somebody with an account gets in as soon
/// as there is a key to check them with.
pub fn fetch_key(tickets: Arc<Tickets>, fireplace: String, data_dir: PathBuf) {
    let cache = data_dir.join("fireplace.key");
    if let Some(key) = std::fs::read_to_string(&cache).ok().and_then(|text| parse_key(text.trim())) {
        tickets.set_key(key);
    }
    std::thread::spawn(move || {
        let url = format!("{}/key", fireplace.trim_end_matches('/'));
        loop {
            match ask(&url) {
                Ok(key) => {
                    tickets.set_key(key);
                    keep(&cache, &key);
                    info!(%fireplace, "accounts are checked against this Fireplace");
                    return;
                }
                Err(err) => {
                    let kept = if tickets.has_key() { " (using the key from last time)" } else { "" };
                    warn!(%fireplace, "could not reach the Fireplace{kept}: {err}");
                    std::thread::sleep(RETRY);
                }
            }
        }
    });
}

fn ask(url: &str) -> anyhow::Result<[u8; 32]> {
    let mut response = ureq::get(url).config().timeout_global(Some(Duration::from_secs(10))).build().call()?;
    let body: serde_json::Value = serde_json::from_str(&response.body_mut().read_to_string()?)?;
    if body["algorithm"] != "ed25519" {
        anyhow::bail!("the Fireplace signs with something this Flame does not know");
    }
    body["key"].as_str().and_then(parse_key).ok_or_else(|| anyhow::anyhow!("the Fireplace sent no usable key"))
}

fn keep(path: &Path, key: &[u8; 32]) {
    if let Err(err) = std::fs::write(path, hex(key)) {
        warn!(path = %path.display(), "could not keep the Fireplace key: {err}");
    }
}

fn parse_key(text: &str) -> Option<[u8; 32]> {
    if text.len() != 64 {
        return None;
    }
    let mut key = [0u8; 32];
    for (i, byte) in key.iter_mut().enumerate() {
        *byte = u8::from_str_radix(text.get(i * 2..i * 2 + 2)?, 16).ok()?;
    }
    Some(key)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// Mints tickets the way the Fireplace does, for this crate's tests and the
/// relay tests. Not used by the server itself.
pub mod testing {
    use super::*;
    use ring::signature::{Ed25519KeyPair, KeyPair};

    pub struct Issuer(Ed25519KeyPair);

    impl Default for Issuer {
        fn default() -> Self {
            let pkcs8 = Ed25519KeyPair::generate_pkcs8(&ring::rand::SystemRandom::new()).unwrap();
            Self(Ed25519KeyPair::from_pkcs8(pkcs8.as_ref()).unwrap())
        }
    }

    impl Issuer {
        pub fn public_key(&self) -> [u8; 32] {
            self.0.public_key().as_ref().try_into().unwrap()
        }

        pub fn ticket(&self, certificate: &str, pid: &str, name: &str, iat: u64, exp: u64) -> String {
            let claims = serde_json::json!({
                "typ": "join", "aud": certificate, "pid": pid, "name": name, "iat": iat, "exp": exp,
            });
            let signed = format!("{PREFIX}{}", BASE64.encode(claims.to_string()));
            format!("{signed}.{}", BASE64.encode(self.0.sign(signed.as_bytes())))
        }

        /// A ticket good for the next two minutes.
        pub fn fresh(&self, certificate: &str, pid: &str, name: &str) -> String {
            self.ticket(certificate, pid, name, now(), now() + 120)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::testing::Issuer;
    use super::*;

    #[test]
    fn a_good_ticket_names_the_player_and_nothing_else() {
        let issuer = Issuer::default();
        let tickets = Tickets::new(b"our certificate");
        let ours = tickets.certificate().to_owned();
        let ticket = issuer.ticket(&ours, "0a1b", "Ada", 1_000, 1_120);
        assert_eq!(tickets.check_at(&ticket, 1_000), Err(Refused::NoKey), "nothing to check it with yet");

        tickets.set_key(issuer.public_key());
        assert_eq!(tickets.check_at(&ticket, 1_000), Ok(Player { id: "0a1b".into(), name: "Ada".into() }));
        assert_eq!(tickets.check_at(&ticket, 1_120), Err(Refused::Expired));
        assert_eq!(
            tickets.check_at(&issuer.ticket(&ours, "0a1b", "Ada", 2_000, 2_120), 1_000),
            Err(Refused::Expired),
            "issued well in the future"
        );
    }

    #[test]
    fn a_ticket_for_another_flame_or_from_another_key_is_refused() {
        let (issuer, stranger) = (Issuer::default(), Issuer::default());
        let tickets = Tickets::new(b"our certificate");
        tickets.set_key(issuer.public_key());
        let theirs = Tickets::new(b"their certificate").certificate().to_owned();
        let ours = tickets.certificate().to_owned();

        // The replay this exists to stop: a ticket handed to another Flame.
        assert_eq!(tickets.check_at(&issuer.ticket(&theirs, "aa", "Ada", 1_000, 1_120), 1_000), Err(Refused::NotForUs));
        assert_eq!(
            tickets.check_at(&stranger.ticket(&ours, "aa", "Ada", 1_000, 1_120), 1_000),
            Err(Refused::BadSignature)
        );
        // Signed, but carrying an id that could climb out of a directory, or a
        // name nobody should see.
        assert_eq!(tickets.check_at(&issuer.ticket(&ours, "../x", "Ada", 1_000, 1_120), 1_000), Err(Refused::Malformed));
        assert_eq!(tickets.check_at(&issuer.ticket(&ours, "aa", "A\nB", 1_000, 1_120), 1_000), Err(Refused::Malformed));
        for junk in ["", "v1.", "v1.abc", "v2.abc.def", "v1.!!!.???"] {
            assert!(tickets.check_at(junk, 1_000).is_err(), "{junk:?}");
        }
    }

    #[test]
    fn keys_round_trip_through_hex() {
        let key = Issuer::default().public_key();
        assert_eq!(parse_key(&hex(&key)), Some(key));
        assert_eq!(parse_key("abc"), None);
        assert_eq!(parse_key(&"zz".repeat(32)), None);
    }
}
