//! What a signed-in player carries: a short-lived token saying who they are.
//!
//! The shape is `v1.<payload>.<signature>`, both parts base64url without
//! padding, the signature over the ASCII bytes of `v1.<payload>`. It is
//! JWT-shaped but it is not a JWT, deliberately: no new dependency, and no
//! `alg` field for anybody to lie about. Anything whose prefix is not exactly
//! `v1.` is refused rather than guessed at, which is what leaves room for a
//! `v2.` that nothing old will accept by accident.
//!
//! Ed25519, because the signature is 64 bytes and verifying needs no state --
//! which is what lets a Flame check one offline later (`docs/TRUST.md`).

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use base64::Engine as _;
use ring::signature::{Ed25519KeyPair, KeyPair, UnparsedPublicKey};

/// How long a sign-in is good for before the client has to say its name
/// again. An hour, because a stolen one cannot be called back.
pub const ACCESS_SECONDS: u64 = 60 * 60;
/// How long the token kept on the player's own machine lasts. A month,
/// because typing a password on a keyboard floating in mid-air is miserable
/// and nobody should do it twice a day.
pub const REFRESH_SECONDS: u64 = 30 * 24 * 60 * 60;
/// A join ticket is spent the moment the client says `Hello`, so it only has
/// to outlive one round trip -- and anything longer is a longer window for a
/// copied one, even though it only works on the one Flame it names.
pub const JOIN_SECONDS: u64 = 120;

/// A password reset link lasts half an hour.
pub const RESET_SECONDS: u64 = 30 * 60;
/// A link that confirms an email address lasts as long as the unverified
/// address is kept (`accounts::UNVERIFIED_SECONDS`).
pub const VERIFY_SECONDS: u64 = 7 * 24 * 60 * 60;

const PREFIX: &str = "v1.";
const BASE64: base64::engine::general_purpose::GeneralPurpose =
    base64::engine::general_purpose::URL_SAFE_NO_PAD;

/// What a token says. Short field names: this travels on every request.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Claims {
    /// Which account.
    pub sub: u64,
    /// The handle, so a Flame can show a name without asking anybody.
    pub handle: String,
    /// `access` or `refresh`. A refresh token is not a key to anything but a
    /// new access token, and the two are never confused for each other.
    pub typ: String,
    /// The account's token epoch when this was minted. Bumping the account's
    /// epoch makes every token issued before it worthless, which is the whole
    /// of revocation and costs one number per account.
    pub epoch: u64,
    pub iat: u64,
    pub exp: u64,
}

impl Claims {
    pub fn is_access(&self) -> bool {
        self.typ == "access"
    }
    pub fn is_refresh(&self) -> bool {
        self.typ == "refresh"
    }
}

/// What a Flame is handed so it can let somebody in (`docs/TRUST.md`).
///
/// It says nothing about the account. `pid` is a stable id **for this one
/// Flame only** -- a keyed hash of the account and the Flame's certificate --
/// so a Flame can keep somebody's saves without ever learning who they are,
/// and two Flames comparing notes cannot tell they have the same player.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct JoinClaims {
    /// Always `join`, so a ticket is never mistaken for an access token.
    pub typ: String,
    /// The SHA-256 of the Flame's TLS certificate, hex. The Flame refuses a
    /// ticket naming any certificate but its own, which is what stops a Flame
    /// that was handed one from replaying it to another.
    pub aud: String,
    pub pid: String,
    /// The display name, which is all of the account a Flame ever sees.
    pub name: String,
    pub iat: u64,
    pub exp: u64,
}

/// What a link in an email carries: a reset (`typ` = `reset`) or the address
/// to confirm (`typ` = `verify`). Nothing is stored for one: it is signed,
/// it names the account's epoch, and a reset moves the epoch -- so a link
/// works once, and a leaked people file holds no link at all. `nonce` is 128
/// random bits, so no two links are alike even within one second.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct LinkClaims {
    pub typ: String,
    pub sub: u64,
    pub epoch: u64,
    /// The address being confirmed; empty on a reset.
    #[serde(default)]
    pub email: String,
    pub nonce: String,
    pub iat: u64,
    pub exp: u64,
}

/// The signing key, read from `data/key.pk8` or made on first run, and the
/// secret the per-Flame player ids are derived with, from `data/pid.key`.
pub struct Keys {
    pair: Ed25519KeyPair,
    pid: ring::hmac::Key,
}

impl Keys {
    pub fn open(dir: &Path) -> Result<Self> {
        let path = key_path(dir);
        let pkcs8 = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                let made = Ed25519KeyPair::generate_pkcs8(&ring::rand::SystemRandom::new())
                    .map_err(|_| anyhow::anyhow!("could not generate a signing key"))?;
                write_private(&path, made.as_ref())
                    .with_context(|| format!("could not write {}", path.display()))?;
                tracing::info!(path = %path.display(), "made a signing key");
                made.as_ref().to_vec()
            }
            Err(err) => {
                return Err(err).with_context(|| format!("could not read {}", path.display()));
            }
        };
        let pair = Ed25519KeyPair::from_pkcs8(&pkcs8)
            .map_err(|_| anyhow::anyhow!("{} is not a usable signing key", path.display()))?;
        // Losing this changes every player id on every Flame, which orphans
        // everybody's saves, so it is made once and never overwritten.
        let pid_path = dir.join("pid.key");
        let secret = match std::fs::read(&pid_path) {
            Ok(bytes) => bytes,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                let mut made = [0u8; 32];
                ring::rand::SecureRandom::fill(&ring::rand::SystemRandom::new(), &mut made)
                    .map_err(|_| anyhow::anyhow!("could not generate the player id secret"))?;
                write_private(&pid_path, &made)
                    .with_context(|| format!("could not write {}", pid_path.display()))?;
                made.to_vec()
            }
            Err(err) => {
                return Err(err).with_context(|| format!("could not read {}", pid_path.display()));
            }
        };
        Ok(Self { pair, pid: ring::hmac::Key::new(ring::hmac::HMAC_SHA256, &secret) })
    }

    /// A ticket for one Flame, named by its certificate hash.
    pub fn mint_join(&self, account: u64, name: &str, flame: &str, now: u64) -> String {
        let tag = ring::hmac::sign(&self.pid, format!("{account}:{flame}").as_bytes());
        let claims = JoinClaims {
            typ: "join".into(),
            aud: flame.to_owned(),
            pid: tag.as_ref()[..16].iter().map(|b| format!("{b:02x}")).collect(),
            name: name.to_owned(),
            iat: now,
            exp: now + JOIN_SECONDS,
        };
        self.sign(&serde_json::to_vec(&claims).expect("claims are plain data"))
    }

    fn sign(&self, json: &[u8]) -> String {
        let signed = format!("{PREFIX}{}", BASE64.encode(json));
        let signature = BASE64.encode(self.pair.sign(signed.as_bytes()).as_ref());
        format!("{signed}.{signature}")
    }

    /// The public half, hex, for a Flame to hold and verify against.
    pub fn public_hex(&self) -> String {
        self.pair.public_key().as_ref().iter().map(|b| format!("{b:02x}")).collect()
    }

    /// Mints a token. `kind` is `"access"` or `"refresh"`.
    pub fn mint(&self, account: u64, handle: &str, epoch: u64, kind: &str, now: u64) -> String {
        let claims = Claims {
            sub: account,
            handle: handle.to_owned(),
            typ: kind.to_owned(),
            epoch,
            iat: now,
            exp: now + if kind == "refresh" { REFRESH_SECONDS } else { ACCESS_SECONDS },
        };
        self.sign(&serde_json::to_vec(&claims).expect("claims are plain data"))
    }

    /// A link for an email: `reset` or `verify`.
    pub fn mint_link(&self, typ: &str, account: u64, epoch: u64, email: &str, now: u64) -> String {
        let mut nonce = [0u8; 16];
        ring::rand::SecureRandom::fill(&ring::rand::SystemRandom::new(), &mut nonce)
            .expect("the system has randomness");
        let claims = LinkClaims {
            typ: typ.to_owned(),
            sub: account,
            epoch,
            email: email.to_owned(),
            nonce: BASE64.encode(nonce),
            iat: now,
            exp: now + if typ == "verify" { VERIFY_SECONDS } else { RESET_SECONDS },
        };
        self.sign(&serde_json::to_vec(&claims).expect("claims are plain data"))
    }

    /// A link of this kind, signed by us and not expired. The caller checks
    /// the epoch, as with a token.
    pub fn verify_link(&self, token: &str, typ: &str, now: u64) -> Result<LinkClaims> {
        let claims: LinkClaims = serde_json::from_slice(&self.open_signed(token)?).context("not a link")?;
        if claims.typ != typ {
            bail!("not that kind of link");
        }
        if claims.exp <= now {
            bail!("this link has expired");
        }
        Ok(claims)
    }

    /// The signed payload, if the signature is ours.
    fn open_signed(&self, token: &str) -> Result<Vec<u8>> {
        let Some(rest) = token.strip_prefix(PREFIX) else {
            bail!("not a Hearth token");
        };
        let Some((payload, signature)) = rest.split_once('.') else {
            bail!("a token is two parts");
        };
        let signature = BASE64.decode(signature).context("the signature is not base64url")?;
        let signed = format!("{PREFIX}{payload}");
        UnparsedPublicKey::new(&ring::signature::ED25519, self.pair.public_key().as_ref())
            .verify(signed.as_bytes(), &signature)
            .map_err(|_| anyhow::anyhow!("this token was not signed by us"))?;
        BASE64.decode(payload).context("the payload is not base64url")
    }

    /// Signature, shape and expiry. The caller still has to check the epoch
    /// against the account, because only the account knows what it is now.
    pub fn verify(&self, token: &str, now: u64) -> Result<Claims> {
        let claims: Claims =
            serde_json::from_slice(&self.open_signed(token)?).context("the payload is not a token")?;
        if claims.exp <= now {
            bail!("this token has expired");
        }
        Ok(claims)
    }
}

pub fn key_path(dir: &Path) -> PathBuf {
    dir.join("key.pk8")
}

/// Seconds since the epoch. One place, so tests can hand a time in instead.
pub fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// The key is the one secret this server has. Written so that only its owner
/// can read it, and never overwritten: `create_new` fails rather than
/// replacing a key that every token in the wild was signed with.
fn write_private(path: &Path, data: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    options.open(path)?.write_all(data)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each test gets its own directory: they run in parallel, and sharing
    /// one means each deleting the others out from under them.
    fn temp(who: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("hearth-fireplace-{}-{who}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_token_round_trips_and_a_tampered_one_does_not() {
        let dir = temp("round-trip");
        let keys = Keys::open(&dir).unwrap();
        let token = keys.mint(7, "ada", 1, "access", 1_000);
        let claims = keys.verify(&token, 1_000).unwrap();
        assert_eq!((claims.sub, claims.handle.as_str(), claims.epoch), (7, "ada", 1));
        assert!(claims.is_access() && !claims.is_refresh());
        assert_eq!(claims.exp, 1_000 + ACCESS_SECONDS);

        // A flipped character anywhere in it, and it is not a token any more.
        let mut bytes = token.clone().into_bytes();
        let last = bytes.len() - 1;
        bytes[last] = if bytes[last] == b'A' { b'B' } else { b'A' };
        assert!(keys.verify(&String::from_utf8(bytes).unwrap(), 1_000).is_err());
        assert!(keys.verify(&token.replace("v1.", "v2."), 1_000).is_err(), "a version we do not know");
        assert!(keys.verify("rubbish", 1_000).is_err());
        assert!(keys.verify("v1.rubbish", 1_000).is_err(), "no signature at all");
    }

    #[test]
    fn an_expired_token_is_refused() {
        let dir = temp("expiry");
        let keys = Keys::open(&dir).unwrap();
        let token = keys.mint(1, "ada", 1, "access", 1_000);
        assert!(keys.verify(&token, 1_000 + ACCESS_SECONDS - 1).is_ok());
        assert!(keys.verify(&token, 1_000 + ACCESS_SECONDS).is_err(), "the second it runs out");
        // The one kept on a player's machine lasts a month, not an hour.
        let saved = keys.mint(1, "ada", 1, "refresh", 1_000);
        assert!(keys.verify(&saved, 1_000 + ACCESS_SECONDS).unwrap().is_refresh());
    }

    #[test]
    fn a_token_signed_by_another_key_is_refused() {
        let theirs = Keys::open(&temp("another-key-theirs")).unwrap();
        let ours = Keys::open(&temp("another-key-ours")).unwrap();
        let token = theirs.mint(1, "ada", 1, "access", 1_000);
        assert!(ours.verify(&token, 1_000).is_err());
        assert_ne!(ours.public_hex(), theirs.public_hex());
    }

    #[test]
    fn a_join_ticket_names_one_flame_and_hides_the_account() {
        let keys = Keys::open(&temp("join")).unwrap();
        let (a, b) = ("a".repeat(64), "b".repeat(64));
        let claims = |ticket: &str| -> JoinClaims {
            let payload = ticket.strip_prefix(PREFIX).unwrap().split_once('.').unwrap().0;
            serde_json::from_slice(&BASE64.decode(payload).unwrap()).unwrap()
        };
        let on_a = claims(&keys.mint_join(7, "Ada", &a, 1_000));
        assert_eq!((on_a.typ.as_str(), on_a.aud.as_str(), on_a.name.as_str()), ("join", a.as_str(), "Ada"));
        assert_eq!(on_a.exp, 1_000 + JOIN_SECONDS);
        // The same player is the same id on the same Flame, every time...
        assert_eq!(claims(&keys.mint_join(7, "Ada", &a, 2_000)).pid, on_a.pid);
        // ...a different one on another Flame, and never the account number.
        assert_ne!(claims(&keys.mint_join(7, "Ada", &b, 1_000)).pid, on_a.pid);
        assert_ne!(claims(&keys.mint_join(8, "Bob", &a, 1_000)).pid, on_a.pid);
        assert_eq!(on_a.pid.len(), 32);
        // A ticket is not an access token, so it opens nothing here.
        assert!(keys.verify(&keys.mint_join(7, "Ada", &a, 1_000), 1_000).is_err());
    }

    #[test]
    fn the_signing_key_is_generated_once_and_reused() {
        let dir = temp("same-key");
        let first = Keys::open(&dir).unwrap().public_hex();
        // Restarting must not log everybody out.
        assert_eq!(Keys::open(&dir).unwrap().public_hex(), first);
        assert_eq!(first.len(), 64, "32 bytes of public key, in hex");
    }
}
