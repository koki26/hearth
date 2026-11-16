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
use lettre::message::{Attachment, Mailbox, MultiPart, SinglePart, header::ContentType};
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
    /// Plain text: what the log shows, and the fallback part of the email.
    pub body: String,
    pub html: String,
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
            .subject(letter.subject);
        if let Some(reply_to) = &self.reply_to {
            builder = builder.reply_to(reply_to.clone());
        }
        // Plain text first, then the designed one with its logo inside.
        let designed = MultiPart::related().singlepart(SinglePart::html(letter.html)).singlepart(
            Attachment::new_inline(String::from("logo")).body(LOGO.to_vec(), ContentType::parse("image/png")?),
        );
        Ok(builder.multipart(MultiPart::alternative().singlepart(SinglePart::plain(letter.body)).multipart(designed))?)
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
// Every one is about an account and nothing else: there is no marketing, and
// no list to be on. Each is written once, as parts, and rendered twice: as
// HTML with the Hearth banner, and as plain text for mail apps that want it.

/// The logo in the banner, sent inside the email (`cid:logo`) so it shows
/// without "load images".
const LOGO: &[u8] = include_bytes!("../assets/email-logo.png");

enum Part {
    Text(String),
    /// A code to type, shown large.
    Code(String),
    /// The one thing to click.
    Button(&'static str, String),
    /// Small print under the rest.
    Small(String),
}

struct Draft {
    kind: &'static str,
    subject: String,
    /// The line a mail app shows next to the subject.
    preview: String,
    heading: String,
    parts: Vec<Part>,
    /// Why this came, under the card.
    because: String,
}

const FONT: &str = "-apple-system,BlinkMacSystemFont,'Segoe UI',Roboto,Helvetica,Arial,sans-serif";
const SERIF: &str = "Georgia,'Palatino Linotype',Palatino,serif";

impl Draft {
    fn letter(self, url: &str) -> Letter {
        let esc = crate::pages::esc;
        let mut text = format!("{}\n\n", self.heading);
        let mut html = String::new();
        for part in &self.parts {
            match part {
                Part::Text(t) => {
                    text += &format!("{t}\n\n");
                    html += &format!(
                        "<p style=\"margin:0 0 16px;font:15px/1.6 {FONT};color:#3a4060\">{}</p>",
                        esc(t)
                    );
                }
                Part::Code(c) => {
                    text += &format!("    {c}\n\n");
                    html += &format!(
                        "<div style=\"margin:4px 0 20px;padding:18px 0;text-align:center;background:#f3f4f9;\
                         border:1px solid #dfe3ee;border-radius:12px;font:600 34px/1 'SFMono-Regular',Consolas,\
                         'Liberation Mono',Menlo,monospace;letter-spacing:10px;color:#1a1f3a\">{}</div>",
                        esc(c)
                    );
                }
                Part::Button(label, link) => {
                    text += &format!("{label}: {link}\n\n");
                    html += &format!(
                        "<table role=\"presentation\" cellpadding=\"0\" cellspacing=\"0\" style=\"margin:6px 0 22px\">\
                         <tr><td style=\"border-radius:10px;background:#ad500f\"><a href=\"{l}\" style=\"display:inline-block;\
                         padding:13px 24px;font:600 15px/1 {FONT};color:#ffffff;text-decoration:none;border-radius:10px\">{}</a>\
                         </td></tr></table><p style=\"margin:0 0 16px;font:12px/1.5 {FONT};color:#8a8fa8;word-break:break-all\">\
                         Or paste this into your browser: {l}</p>",
                        esc(label),
                        l = esc(link)
                    );
                }
                Part::Small(s) => {
                    text += &format!("{s}\n\n");
                    html += &format!(
                        "<p style=\"margin:0 0 12px;font:13px/1.55 {FONT};color:#6b7190\">{}</p>",
                        esc(s)
                    );
                }
            }
        }
        text += &format!("--\n{}\nHearth · {url}/privacy\n", self.because);
        let html = format!(
            "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\">\
             <meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">\
             <meta name=\"color-scheme\" content=\"light\"><title>{subject}</title></head>\
             <body style=\"margin:0;padding:0;background:#eceef5\">\
             <div style=\"display:none;max-height:0;overflow:hidden;opacity:0\">{preview}</div>\
             <table role=\"presentation\" width=\"100%\" cellpadding=\"0\" cellspacing=\"0\" style=\"background:#eceef5\">\
             <tr><td align=\"center\" style=\"padding:32px 12px\">\
             <table role=\"presentation\" width=\"100%\" cellpadding=\"0\" cellspacing=\"0\" style=\"max-width:560px\">\
             <tr><td style=\"background:#171c3a;border-radius:16px 16px 0 0;padding:22px 28px;border-bottom:3px solid #ad500f\">\
             <table role=\"presentation\" cellpadding=\"0\" cellspacing=\"0\"><tr>\
             <td style=\"vertical-align:middle\"><img src=\"cid:logo\" width=\"44\" height=\"44\" alt=\"\" \
             style=\"display:block;border:0;border-radius:11px\"></td>\
             <td style=\"vertical-align:middle;padding-left:12px;font:600 22px/1 {SERIF};color:#ffffff\">Hearth</td>\
             </tr></table></td></tr>\
             <tr><td style=\"background:#ffffff;border-radius:0 0 16px 16px;padding:32px 28px 18px\">\
             <h1 style=\"margin:0 0 18px;font:600 24px/1.25 {SERIF};color:#1a1f3a\">{heading}</h1>{html}</td></tr>\
             <tr><td style=\"padding:20px 8px 0;text-align:center;font:12px/1.6 {FONT};color:#8a8fa8\">{because}<br>\
             <a href=\"{url}/privacy\" style=\"color:#8a8fa8\">Privacy</a> · \
             <a href=\"{url}/terms\" style=\"color:#8a8fa8\">Terms</a></td></tr>\
             </table></td></tr></table></body></html>",
            subject = esc(&self.subject),
            preview = esc(&self.preview),
            heading = esc(&self.heading),
            because = esc(&self.because),
            url = esc(url),
        );
        Letter { kind: self.kind, subject: self.subject, body: text, html }
    }
}

fn because(handle: &str) -> String {
    format!("You got this because of the Hearth account @{handle}. Hearth sends nothing else.")
}

impl Mailer {
    /// The code is typed where the account was made; the link is for when
    /// that page is gone.
    pub fn verify_letter(&self, handle: &str, code: &str, link: &str) -> Letter {
        Draft {
            kind: "verify",
            subject: format!("{code} is your Hearth code"),
            preview: "Confirm your email address".into(),
            heading: "Confirm your email".into(),
            parts: vec![
                Part::Text(format!("This address was added to the Hearth account @{handle}. Enter this code to confirm it:")),
                Part::Code(code.into()),
                Part::Text("It works for 15 minutes. If you have closed that page, use the button instead (it works for 7 days).".into()),
                Part::Button("Confirm my email", link.into()),
                Part::Small("If you did not make a Hearth account, ignore this email: the address is deleted in a week.".into()),
            ],
            because: because(handle),
        }
        .letter(&self.public_url)
    }

    /// A code to finish signing in: always for a new IP address, and every
    /// time for an account that turned on two-step sign-in.
    pub fn sign_in_letter(&self, handle: &str, ip: &str, code: &str, new_place: bool) -> Letter {
        let url = &self.public_url;
        let why = if new_place {
            format!("Somebody is signing in to @{handle} with the right password, from an IP address this account has not used before ({ip}).")
        } else {
            format!("Somebody is signing in to @{handle} with the right password, from {ip}. Two-step sign-in is on, so a code is needed.")
        };
        Draft {
            kind: "sign-in",
            subject: format!("{code} is your Hearth sign-in code"),
            preview: "Your code to finish signing in".into(),
            heading: if new_place { "New sign-in. Was this you?".into() } else { "Your sign-in code".into() },
            parts: vec![
                Part::Text(why),
                Part::Text("If it was you, enter this code to finish signing in:".into()),
                Part::Code(code.into()),
                Part::Small("It works for 15 minutes, once.".into()),
                Part::Text("If it was NOT you, do not share this code with anyone. Whoever it was knows your password but cannot get in without it. Change your password now:".into()),
                Part::Button("Change my password", format!("{url}/reset")),
            ],
            because: because(handle),
        }
        .letter(url)
    }

    pub fn reset_letter(&self, handle: &str, link: &str) -> Letter {
        Draft {
            kind: "reset",
            subject: "Reset your Hearth password".into(),
            preview: "Choose a new password".into(),
            heading: "Reset your password".into(),
            parts: vec![
                Part::Text(format!("Somebody asked to reset the password of the Hearth account @{handle}.")),
                Part::Button("Choose a new password", link.into()),
                Part::Small("The link works once, for 30 minutes, and signs the account out everywhere. If you did not ask, ignore this email: your password has not changed.".into()),
            ],
            because: because(handle),
        }
        .letter(&self.public_url)
    }

    pub fn password_changed_letter(&self, handle: &str) -> Letter {
        let (url, contact) = (&self.public_url, &self.contact);
        Draft {
            kind: "password-changed",
            subject: "Your Hearth password was changed".into(),
            preview: "Every sign-in was ended".into(),
            heading: "Your password was changed".into(),
            parts: vec![
                Part::Text(format!("The password of the Hearth account @{handle} was just changed, and every sign-in was ended.")),
                Part::Text("If that was you, there is nothing to do.".into()),
                Part::Text(format!("If it was not, reset it right away and write to {contact}.")),
                Part::Button("Reset my password", format!("{url}/reset")),
            ],
            because: because(handle),
        }
        .letter(url)
    }

    pub fn email_changed_letter(&self, handle: &str) -> Letter {
        let contact = &self.contact;
        Draft {
            kind: "email-changed",
            subject: "Your Hearth email was changed".into(),
            preview: "Nothing more will come to this address".into(),
            heading: "Your email was changed".into(),
            parts: vec![
                Part::Text(format!("The email address of the Hearth account @{handle} was changed, so nothing about that account will come to this address any more.")),
                Part::Text(format!("If that was you, there is nothing to do. If it was not, write to {contact} straight away.")),
            ],
            because: because(handle),
        }
        .letter(&self.public_url)
    }

    /// A change to the terms or privacy notice, in the operator's words, to
    /// somebody who asked to hear about it. Says how to stop them.
    pub fn policy_letter(&self, handle: &str, subject: &str, text: &str) -> Letter {
        let url = &self.public_url;
        let mut parts: Vec<Part> = text.trim().split("\n\n").map(|p| Part::Text(p.trim().to_owned())).collect();
        parts.push(Part::Button("Read the privacy notice", format!("{url}/privacy")));
        parts.push(Part::Small(format!("The terms are at {url}/terms. To stop these emails, turn them off on your account page: {url}/")));
        Draft {
            kind: "policy",
            subject: subject.trim().to_owned(),
            preview: "A change to how Hearth works".into(),
            heading: subject.trim().to_owned(),
            parts,
            because: format!("You got this because @{handle} asked to hear when the terms or the privacy notice change."),
        }
        .letter(url)
    }

    pub fn test_letter(&self) -> Letter {
        Draft {
            kind: "test",
            subject: "Hearth test email".into(),
            preview: "Sending works".into(),
            heading: "Sending works".into(),
            parts: vec![Part::Text("This is a test from your Hearth Fireplace. If you can read it, email is set up.".into())],
            because: "You got this because somebody ran `fireplace admin test-mail` on the server.".into(),
        }
        .letter(&self.public_url)
    }
}

/// Shape only: something@something, no spaces, not absurdly long. Whether
/// it is real is what the confirmation link is for.
pub fn address_is_usable(address: &str) -> bool {
    address.len() <= 254 && address.parse::<lettre::Address>().is_ok()
}
