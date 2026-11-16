//! Email: optional, and only ever for resets and security notices (D63).
//!
//! Every operator brings their own SMTP server, so all of it is in
//! `[mail]` and the default is `backend = "none"` -- a fresh Fireplace sends
//! nothing and needs nothing set up. How to set one up is
//! `docs/SELFHOST-MAIL.md`.
//!
//! Sending happens on a task of its own, with a few retries, so a request
//! never waits on SMTP and answers the same whether a letter went out or not.
//! A failure is logged with the kind of letter and the SMTP code, never the
//! address: an SMTP error line often quotes it.

use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use lettre::message::{Mailbox, header::ContentType};
use lettre::transport::smtp::authentication::Credentials;
use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};

use crate::Config;

/// Set to `1` to allow `backend = "log"` in a release build. It prints whole
/// letters, links included, so it is for a developer's own machine only.
pub const DEV_FLAG: &str = "FIREPLACE_DEV_MAIL";
/// Overrides `password_file`, for hosts that hand secrets in as variables.
pub const PASSWORD_ENV: &str = "FIREPLACE_SMTP_PASSWORD";
/// Waits before each try. A request never sees any of this.
const RETRIES: [u64; 3] = [0, 10, 60];

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Backend {
    /// No email at all: reset by recovery code or the admin command.
    None,
    Smtp,
    /// Letters printed to the log. Development only.
    Log,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Security {
    /// Port 587: plain, then upgraded. Refused if the upgrade is not offered.
    Starttls,
    /// Port 465: TLS from the first byte.
    Tls,
    /// No TLS at all. Only to a server on this machine.
    None,
}

/// `[mail]` in `fireplace.toml`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct MailConfig {
    pub backend: Backend,
    /// What links in letters start with, e.g. `https://fireplace.example.org`.
    pub public_url: String,
    /// `Hearth <hearth@example.org>`.
    pub from: String,
    /// Optional.
    pub reply_to: String,
    /// Who runs the SMTP server, if it is not you: a processor under GDPR,
    /// and named on `/privacy`. Empty means you run it yourself.
    pub provider: String,
    pub smtp: SmtpConfig,
}

impl Default for MailConfig {
    fn default() -> Self {
        Self {
            backend: Backend::None,
            public_url: String::new(),
            from: String::new(),
            reply_to: String::new(),
            provider: String::new(),
            smtp: SmtpConfig::default(),
        }
    }
}

/// `[mail.smtp]`. There is no password here on purpose: it is read from
/// `password_file`, or `FIREPLACE_SMTP_PASSWORD`, and never printed.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct SmtpConfig {
    pub host: String,
    pub port: u16,
    pub security: Security,
    pub username: String,
    pub password_file: PathBuf,
    pub timeout_secs: u64,
}

impl Default for SmtpConfig {
    fn default() -> Self {
        Self {
            host: String::new(),
            port: 587,
            security: Security::Starttls,
            username: String::new(),
            password_file: PathBuf::new(),
            timeout_secs: 10,
        }
    }
}

/// One letter, already written.
#[derive(Debug, Clone)]
pub struct Letter {
    /// For the log line, in place of who it was to.
    pub kind: &'static str,
    pub subject: String,
    pub body: String,
}

/// What the memory backend kept, for the tests.
#[derive(Debug, Clone)]
pub struct Sent {
    pub to: String,
    pub letter: Letter,
}

enum Transport {
    None,
    Log,
    Smtp(AsyncSmtpTransport<Tokio1Executor>),
    /// Tests only: letters are kept in a list instead of sent.
    Memory(Mutex<Vec<Sent>>),
}

pub struct Mailer {
    transport: Transport,
    from: Option<Mailbox>,
    reply_to: Option<Mailbox>,
    /// Without a trailing slash.
    pub public_url: String,
    /// Where a worried recipient writes, from `[operator]`.
    pub contact: String,
}

fn is_local(host: &str) -> bool {
    matches!(host, "localhost" | "127.0.0.1" | "::1" | "[::1]")
}

impl Mailer {
    /// Checks all of `[mail]` and `[operator]` and gets ready to send, or
    /// says exactly what is wrong. Called before anything is served: a
    /// Fireplace that cannot send the letters it promises should not start.
    pub fn new(config: &Config) -> Result<Self> {
        let mail = &config.mail;
        let public_url = mail.public_url.trim().trim_end_matches('/').to_owned();
        let contact = config.operator.contact_email.trim().to_owned();
        if mail.backend == Backend::None {
            return Ok(Self { transport: Transport::None, from: None, reply_to: None, public_url, contact });
        }
        if mail.backend == Backend::Log
            && !cfg!(debug_assertions)
            && std::env::var(DEV_FLAG).as_deref() != Ok("1")
        {
            bail!(
                "[mail] backend = \"log\" prints whole emails, links included; it is refused in a release build unless {DEV_FLAG}=1"
            );
        }
        let local_url = ["http://localhost", "http://127.0.0.1", "http://[::1]"].iter().any(|p| {
            public_url.strip_prefix(p).is_some_and(|rest| rest.is_empty() || rest.starts_with([':', '/']))
        });
        if !public_url.starts_with("https://") && !local_url {
            bail!(
                "[mail] public_url must start with https:// (links in emails carry reset tokens); got {public_url:?}"
            );
        }
        if config.operator.name.trim().is_empty() || contact.is_empty() {
            bail!(
                "[operator] name and contact_email must be filled in when email is on: /privacy has to say who runs this server"
            );
        }
        let from: Mailbox =
            mail.from.parse().with_context(|| format!("[mail] from is not an address: {:?}", mail.from))?;
        let reply_to = match mail.reply_to.trim() {
            "" => None,
            text => {
                Some(text.parse().with_context(|| format!("[mail] reply_to is not an address: {text:?}"))?)
            }
        };
        let transport = match mail.backend {
            Backend::Log => Transport::Log,
            _ => Transport::Smtp(smtp(&mail.smtp)?),
        };
        Ok(Self { transport, from: Some(from), reply_to, public_url, contact })
    }

    /// For tests: everything is checked by the caller, and letters are kept.
    pub fn memory(public_url: &str) -> Self {
        Self {
            transport: Transport::Memory(Mutex::new(Vec::new())),
            from: Some("Hearth <hearth@example.org>".parse().expect("an address")),
            reply_to: None,
            public_url: public_url.to_owned(),
            contact: "privacy@example.org".into(),
        }
    }

    /// Whether this Fireplace sends email at all.
    pub fn enabled(&self) -> bool {
        !matches!(self.transport, Transport::None)
    }

    /// What the memory backend has kept; empty for every other.
    pub fn sent(&self) -> Vec<Sent> {
        match &self.transport {
            Transport::Memory(kept) => kept.lock().unwrap().clone(),
            _ => Vec::new(),
        }
    }

    /// Sends in the background. Returns at once, and the caller never learns
    /// whether it worked -- on purpose, so nothing about the answer differs.
    pub fn send(&self, to: &str, letter: Letter) {
        match &self.transport {
            Transport::None => {}
            Transport::Memory(kept) => kept.lock().unwrap().push(Sent { to: to.to_owned(), letter }),
            Transport::Log => {
                tracing::info!("[mail backend = log] to {to}: {}\n{}", letter.subject, letter.body)
            }
            Transport::Smtp(smtp) => {
                let kind = letter.kind;
                let Ok(message) = self.message(to, letter) else {
                    tracing::warn!(kind, "could not write an email: the address will not parse");
                    return;
                };
                let smtp = smtp.clone();
                tokio::spawn(async move {
                    for (attempt, wait) in RETRIES.iter().enumerate() {
                        tokio::time::sleep(Duration::from_secs(*wait)).await;
                        match smtp.send(message.clone()).await {
                            Ok(_) => return,
                            Err(err) => {
                                // The code, not the text: the text often quotes the address.
                                let code = err.status().map(|c| c.to_string());
                                tracing::warn!(
                                    kind,
                                    attempt,
                                    code,
                                    permanent = err.is_permanent(),
                                    "could not send an email"
                                );
                                if err.is_permanent() {
                                    return;
                                }
                            }
                        }
                    }
                });
            }
        }
    }

    /// Sends one letter and waits for the server's answer, for
    /// `fireplace admin test-mail`. The error is the whole SMTP story.
    pub async fn send_now(&self, to: &str, letter: Letter) -> Result<String> {
        match &self.transport {
            Transport::None => bail!("[mail] backend is \"none\": this Fireplace sends no email"),
            Transport::Smtp(smtp) => {
                let response =
                    smtp.send(self.message(to, letter)?).await.context("the SMTP server said no")?;
                Ok(format!("{} {}", response.code(), response.message().collect::<Vec<_>>().join(" ")))
            }
            _ => {
                self.send(to, letter);
                Ok("written to the log".into())
            }
        }
    }

    fn message(&self, to: &str, letter: Letter) -> Result<Message> {
        let mut builder = Message::builder()
            .from(self.from.clone().context("no from address")?)
            .to(to.parse().context("not an address")?)
            .subject(letter.subject)
            .header(ContentType::TEXT_PLAIN);
        if let Some(reply_to) = &self.reply_to {
            builder = builder.reply_to(reply_to.clone());
        }
        Ok(builder.body(letter.body)?)
    }
}

fn smtp(config: &SmtpConfig) -> Result<AsyncSmtpTransport<Tokio1Executor>> {
    let host = config.host.trim();
    if host.is_empty() {
        bail!("[mail.smtp] host is empty");
    }
    match (config.security, config.port) {
        (_, 0) => bail!("[mail.smtp] port 0 is not a port"),
        (Security::Tls, 587) => {
            bail!(
                "[mail.smtp] port 587 speaks STARTTLS: use security = \"starttls\", or port 465 for \"tls\""
            )
        }
        (Security::Starttls, 465) => {
            bail!(
                "[mail.smtp] port 465 is implicit TLS: use security = \"tls\", or port 587 for \"starttls\""
            )
        }
        (Security::None, _) if !is_local(host) => {
            bail!(
                "[mail.smtp] security = \"none\" would send your password in the clear; it is only allowed to localhost"
            )
        }
        _ => {}
    }
    let mut builder = match config.security {
        Security::Starttls => AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(host)?,
        Security::Tls => AsyncSmtpTransport::<Tokio1Executor>::relay(host)?,
        Security::None => AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(host),
    }
    .port(config.port)
    .timeout(Some(Duration::from_secs(config.timeout_secs.max(1))));
    if !config.username.trim().is_empty() {
        builder = builder.credentials(Credentials::new(config.username.trim().to_owned(), password(config)?));
    }
    Ok(builder.build())
}

/// The environment wins over the file. Neither is ever printed: the error
/// names the file, not what is in it.
fn password(config: &SmtpConfig) -> Result<String> {
    if let Ok(password) = std::env::var(PASSWORD_ENV) {
        return Ok(password);
    }
    if config.password_file.as_os_str().is_empty() {
        bail!("[mail.smtp] username is set, so password_file (or {PASSWORD_ENV}) is needed");
    }
    let text = std::fs::read_to_string(&config.password_file).with_context(|| {
        format!("[mail.smtp] could not read password_file {}", config.password_file.display())
    })?;
    let password = text.trim_end_matches(['\r', '\n']).to_owned();
    if password.is_empty() {
        bail!("[mail.smtp] password_file {} is empty", config.password_file.display());
    }
    Ok(password)
}

// --- the letters --------------------------------------------------------------
//
// Plain text, short, in English. Every one is about an account and
// nothing else: there is no marketing, and no list to be on.

fn footer(url: &str) -> String {
    format!(
        "\n-- \nHearth · {url}/privacy\n\
         You get this because of the account named above; Hearth sends nothing else.\n\
         Tento e-mail dostáváš kvůli účtu uvedenému výše; Hearth nic jiného neposílá.\n"
    )
}

impl Mailer {
    pub fn verify_letter(&self, handle: &str, link: &str) -> Letter {
        Letter {
            kind: "verify",
            subject: "Hearth: confirm your email / potvrzení e-mailu".into(),
            body: format!(
                "This address was added to the Hearth account \"{handle}\". To confirm it, open:\n\n{link}\n\n\
                 The link works for 7 days. If you know nothing about this, ignore this email and the address is deleted.\n\n\
                 ---\n\n\
                 Tato adresa byla přidána k účtu Hearth „{handle}“. Potvrdíš ji otevřením odkazu:\n\n{link}\n\n\
                 Odkaz platí 7 dní. Pokud o tom nevíš, e-mail ignoruj a adresa bude smazána.\n{}",
                footer(&self.public_url)
            ),
        }
    }

    pub fn reset_letter(&self, handle: &str, link: &str) -> Letter {
        Letter {
            kind: "reset",
            subject: "Hearth: reset your password / obnovení hesla".into(),
            body: format!(
                "Somebody asked to reset the password of the Hearth account \"{handle}\". To choose a new one, open:\n\n{link}\n\n\
                 The link works once, for 30 minutes, and signs the account out everywhere. \
                 If you did not ask, ignore this email: your password has not changed.\n\n\
                 ---\n\n\
                 Někdo požádal o obnovení hesla k účtu Hearth „{handle}“. Nové heslo nastavíš zde:\n\n{link}\n\n\
                 Odkaz funguje jednou, 30 minut, a odhlásí účet všude. \
                 Pokud o tom nevíš, e-mail ignoruj: heslo se nezměnilo.\n{}",
                footer(&self.public_url)
            ),
        }
    }

    pub fn password_changed_letter(&self, handle: &str) -> Letter {
        let (url, contact) = (&self.public_url, &self.contact);
        Letter {
            kind: "password-changed",
            subject: "Hearth: your password was changed / heslo bylo změněno".into(),
            body: format!(
                "The password of the Hearth account \"{handle}\" was just changed, and every sign-in was ended. \
                 If that was you, there is nothing to do. If not, reset it at {url}/reset and write to {contact}.\n\n\
                 ---\n\n\
                 Heslo k účtu Hearth „{handle}“ bylo právě změněno a všechna přihlášení byla ukončena. \
                 Pokud o změně víš, nic nedělej. Pokud ne, obnov heslo na {url}/reset a napiš na {contact}.\n{}",
                footer(url)
            ),
        }
    }

    pub fn email_changed_letter(&self, handle: &str) -> Letter {
        let contact = &self.contact;
        Letter {
            kind: "email-changed",
            subject: "Hearth: your email was changed / e-mail byl změněn".into(),
            body: format!(
                "The email address of the Hearth account \"{handle}\" was changed or removed, so nothing about \
                 that account will come to this address any more. If that was you, there is nothing to do. \
                 If not, write to {contact}.\n\n\
                 ---\n\n\
                 E-mailová adresa u účtu Hearth „{handle}“ byla změněna nebo odebrána, takže na tuto adresu \
                 už o něm nic nepřijde. Pokud o změně víš, nic nedělej. Pokud ne, napiš na {contact}.\n{}",
                footer(&self.public_url)
            ),
        }
    }

    /// A change to the terms or privacy notice, in the operator's words, to
    /// somebody who asked to hear about it. Says how to stop them.
    pub fn policy_letter(&self, handle: &str, subject: &str, text: &str) -> Letter {
        let url = &self.public_url;
        Letter {
            kind: "policy",
            subject: format!("Hearth: {}", subject.trim()),
            body: format!(
                "Hello {handle},\n\n{}\n\nThe terms: {url}/terms\nThe privacy notice: {url}/privacy\n\n\
                 -- \nYou get this because the Hearth account \"{handle}\" asked for an email when the terms or \
                 the privacy notice change. To stop them, turn that off on your account page: {url}/\n\
                 Tento e-mail dostáváš, protože si účet „{handle}“ vyžádal upozornění na změny podmínek nebo \
                 zásad ochrany soukromí. Vypnout to můžeš na stránce účtu: {url}/\n",
                text.trim()
            ),
        }
    }

    pub fn test_letter(&self) -> Letter {
        Letter {
            kind: "test",
            subject: "Hearth: test email / zkušební e-mail".into(),
            body: format!(
                "This is a test from your Hearth Fireplace. If you can read it, sending works.\n\n\
                 Toto je zkušební e-mail z tvého Hearth Fireplace. Pokud ho čteš, odesílání funguje.\n{}",
                footer(&self.public_url)
            ),
        }
    }
}

/// Shape only: something@something, no spaces, not absurdly long. Whether
/// it is real is what the confirmation link is for.
pub fn address_is_usable(address: &str) -> bool {
    address.len() <= 254 && address.parse::<lettre::Address>().is_ok()
}
