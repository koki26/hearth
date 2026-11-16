//! The Fireplace: the one server that knows who people are.
//!
//! There is one of these, run centrally. Many Flames are run by anybody. The
//! split is the whole point and the reasoning is in `docs/TRUST.md`:
//!
//! * The Fireplace knows identity and nothing else. It never sees a world, a
//!   pose or a shot.
//! * A Flame verifies a signed token against a public key it already holds,
//!   so it never sees a password, never has to reach the Fireplace in the
//!   middle of a match, and cannot impersonate a player to another Flame.
//!
//! What it holds today: accounts, friends, and where a friend is -- which
//! Flame and which ember -- so you can go and stand next to them. Where
//! somebody is comes from **their own client**, never from a Flame: a Flame
//! that had to call the Fireplace mid-match would put this server in the path
//! of everybody's game, which is exactly what the design refuses.
//!
//! The tokens are Ed25519-signed already, so the step after this one -- a
//! `Hello` carrying a token and a Flame checking the signature offline --
//! needs no change to what is issued. Nothing but the Fireplace checks one
//! yet, and the client's "no account" path still works, because a Flame
//! anybody hosts has to keep working on its own (`docs/decisions.md` D52).

pub mod accounts;
pub mod api;
pub mod limit;
pub mod mail;
pub mod pages;
pub mod presence;
pub mod token;

use std::net::{IpAddr, Ipv4Addr};
use std::path::PathBuf;
use std::sync::Mutex;

use anyhow::{Context, Result};

/// Everything the Fireplace can be told, with its defaults. Same shape as a
/// Flame's config, so one is not a surprise after the other.
#[derive(Debug, Clone, serde::Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, default)]
pub struct Config {
    /// What to listen on. Loopback, because this belongs behind a TLS
    /// terminator on the same machine (`docs/ORACLE.md`) and a port that is
    /// never open is a better last line than a firewall rule that might be
    /// wrong. Set it only when the terminator is on another box.
    pub bind: IpAddr,
    /// TCP port for the account API. Plain HTTP on purpose: this sits behind
    /// a TLS terminator, and pretending otherwise would be the kind of
    /// half-security that reads as security.
    pub port: u16,
    /// Where keys and accounts live. Created on first run, kept across
    /// restarts.
    pub data_dir: PathBuf,
    /// How many accounts this Fireplace will hold. A number, so that running
    /// out is a refusal with a reason rather than a full disk.
    pub max_accounts: usize,
    /// New accounts allowed per hour from one address. Everybody in one
    /// house shares an address, so this cannot be as mean as it sounds like
    /// it should be; raise it while you are testing, lower it if somebody
    /// starts making accounts in bulk.
    pub accounts_per_hour: u32,
    /// The account web page: `index.html` and whatever it loads, served as
    /// they are from disk, so editing one is a reload rather than a rebuild.
    pub web_dir: PathBuf,
    /// Email: off unless set up (`docs/SELFHOST-MAIL.md`).
    pub mail: mail::MailConfig,
    /// Who runs this Fireplace, for `/privacy` and `/terms`.
    pub operator: Operator,
}

/// `[operator]`: whoever runs this Fireplace, so its privacy page describes
/// this server and not the official one.
#[derive(Debug, Clone, serde::Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, default)]
pub struct Operator {
    /// The data controller: a person or an organisation.
    pub name: String,
    /// Where privacy requests go.
    pub contact_email: String,
    /// Where the server and its data are, e.g. "Oracle Cloud, Frankfurt (EU)".
    pub hosting: String,
    /// The data protection authority people may complain to.
    pub authority: String,
    /// The youngest anybody may be to make an account. 16 is the GDPR
    /// default; member states may go lower (as low as 13).
    pub min_age: u8,
    /// How long a deleted account can live on in backups, stated on /privacy.
    pub backup_days: u32,
}

impl Default for Operator {
    fn default() -> Self {
        Self {
            name: String::new(),
            contact_email: String::new(),
            hosting: String::new(),
            authority: String::new(),
            min_age: 16,
            backup_days: 14,
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            bind: IpAddr::V4(Ipv4Addr::LOCALHOST),
            port: 7800,
            data_dir: "data".into(),
            max_accounts: 10_000,
            accounts_per_hour: limit::ACCOUNTS_PER_HOUR,
            web_dir: "web".into(),
            mail: mail::MailConfig::default(),
            operator: Operator::default(),
        }
    }
}

impl Config {
    pub fn parse(text: &str) -> Result<Self> {
        toml::from_str(text).context("could not read the configuration")
    }
}

/// Everything the handlers share. The locks are `std::sync::Mutex` and no
/// guard is ever held across an `.await` (`docs/decisions.md` D9) -- the one
/// place that takes care is signing in, where the password check is handed to
/// a blocking thread with nothing locked.
pub struct Fireplace {
    pub accounts: Mutex<accounts::Accounts>,
    pub presence: Mutex<presence::Presence>,
    pub limits: Mutex<limit::Limiter>,
    pub keys: token::Keys,
    pub max_accounts: usize,
    pub accounts_per_hour: u32,
    pub web_dir: PathBuf,
    pub mail: mail::Mailer,
    /// What the pages are filled from.
    pub config: Config,
}

impl Fireplace {
    /// Reads what is on disk and makes what is missing. Called once, before
    /// anything is served.
    pub fn open(config: &Config) -> Result<Self> {
        // Checked first: a bad [mail] stops the server before it touches disk.
        let mail = mail::Mailer::new(config)?;
        std::fs::create_dir_all(&config.data_dir)
            .with_context(|| format!("could not make {}", config.data_dir.display()))?;
        Ok(Self {
            accounts: Mutex::new({
                let mut accounts = accounts::Accounts::open(&config.data_dir)?;
                accounts.forget_unverified(token::now());
                accounts
            }),
            presence: Mutex::new(presence::Presence::default()),
            limits: Mutex::new(limit::Limiter::default()),
            keys: token::Keys::open(&config.data_dir)?,
            max_accounts: config.max_accounts,
            accounts_per_hour: config.accounts_per_hour,
            web_dir: {
                // Not fatal: the API is the part the game needs.
                if !config.web_dir.join("index.html").exists() {
                    tracing::warn!(dir = %config.web_dir.display(), "no index.html, so no account page");
                }
                config.web_dir.clone()
            },
            mail,
            config: config.clone(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_defaults_and_parsing() {
        assert_eq!(Config::parse("").unwrap(), Config::default());
        let config = Config::parse("port = 9000\ndata_dir = \"accounts\"").unwrap();
        assert_eq!(config.port, 9000);
        assert_eq!(config.data_dir, PathBuf::from("accounts"));
        // A typo in a config is a stopped server, not a silently ignored key.
        assert!(Config::parse("prot = 9000").is_err());
    }

    #[test]
    fn the_shipped_config_parses_and_says_what_the_official_server_does() {
        let config = Config::parse(include_str!("../../../deploy/fireplace.toml")).unwrap();
        assert_eq!(config.mail.backend, mail::Backend::Smtp);
        assert_eq!(config.mail.smtp.port, 587);
        assert_eq!(config.operator.min_age, 15);
        // A fresh self-host sends nothing until somebody sets it up.
        assert_eq!(Config::default().mail.backend, mail::Backend::None);
    }

    #[test]
    fn the_default_bind_is_loopback_so_the_port_is_never_public() {
        assert_eq!(Config::default().bind, IpAddr::V4(Ipv4Addr::LOCALHOST));
        let open = Config::parse("bind = \"0.0.0.0\"").unwrap();
        assert_eq!(open.bind, IpAddr::V4(Ipv4Addr::UNSPECIFIED));
    }
}
