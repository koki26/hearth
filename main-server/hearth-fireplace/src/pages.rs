//! The few pages a browser needs: resetting a password, confirming an email,
//! and what this server does with your data.
//!
//! Embedded in the binary and filled from `fireplace.toml`, so every
//! self-hosted Fireplace describes **itself** on `/privacy`, not the official
//! one. Plain forms, no script, no cookies, nothing from anywhere else -- so
//! there is nothing to ask consent for and no banner.
//!
//! A reset link carries its token in the path. The page says
//! `no-referrer`, and everything it links to is on this server anyway.

use std::sync::Arc;

use axum::Router;
use axum::extract::{Form, Path, State};
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Response};
use axum::routing::get;

use crate::accounts;
use crate::api::{Caller, hash_password};
use crate::mail::Backend;
use crate::{Fireplace, limit, token};

/// Asking for reset links: per address, and per account. Over the account's
/// limit the answer is the same and nothing is sent, so the limit does not
/// say whether the address is known either.
pub const RESETS_PER_HOUR: u32 = 5;
pub const RESETS_PER_ACCOUNT: u32 = 3;

pub fn routes() -> Router<Arc<Fireplace>> {
    Router::new()
        .route("/reset", get(reset_page).post(ask_for_reset))
        .route("/reset/{token}", get(new_password_page).post(set_new_password))
        .route("/recover", axum::routing::post(recover))
        .route("/verify/{token}", get(verify))
        .route("/privacy", get(privacy))
        .route("/terms", get(terms))
}

// --- the frame ----------------------------------------------------------------

/// Everything typed by a stranger or an operator goes through this.
pub fn esc(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

// The account page's palette (web/style.css): night stone and firelight.
const STYLE: &str = "
:root{--bg:#0d1022;--fg:#e9ecf8;--muted:#8f97bd;--card:#171c3a;--line:#2a3160;--accent:#ffb366;--stone:#aec2ff;--on:#2a1404}
@media (prefers-color-scheme:light){:root{--bg:#f4f1ea;--fg:#1a1f3a;--muted:#626987;--card:#fffdf8;--line:#ddd5c6;--accent:#c4601a;--stone:#3d4f9c;--on:#fff}}
*{box-sizing:border-box}body{margin:0;background:var(--bg);color:var(--fg);font:15px/1.6 system-ui,-apple-system,'Segoe UI',sans-serif}
main{max-width:40rem;margin:0 auto;padding:3rem 16px;animation:rise .5s cubic-bezier(.2,.7,.2,1) both}
h1,h2{font-family:'Iowan Old Style','Palatino Linotype',Palatino,Georgia,serif;font-weight:600}
h1{font-size:1.9rem;margin:0 0 1.5rem;display:flex;align-items:center;gap:12px}h1 svg{width:34px;height:34px;flex:none;color:var(--stone)}
h2{font-size:1.15rem;margin:0 0 .5rem}section{background:var(--card);border:1px solid var(--line);border-radius:14px;padding:1.2rem 1.4rem;margin:1rem 0}
label{display:block;margin:.9rem 0 .3rem;font-size:.82rem;font-weight:500;color:var(--muted)}
input{width:100%;padding:.7rem .8rem;font:inherit;border:1px solid var(--line);border-radius:10px;background:var(--bg);color:var(--fg)}
input:focus{outline:none;border-color:var(--accent);box-shadow:0 0 0 3px color-mix(in srgb,var(--accent) 22%,transparent)}
button:focus-visible{outline:2px solid var(--accent);outline-offset:2px}
button{margin-top:1.1rem;padding:.65rem 1.2rem;font:inherit;font-weight:600;border:0;border-radius:10px;background:var(--accent);color:var(--on);cursor:pointer;transition:transform .12s}
button:active{transform:scale(.97)}p.muted{color:var(--muted)}a{color:var(--accent)}code{word-break:break-all}
footer{margin-top:2.5rem;font-size:.85rem;display:flex;gap:18px;justify-content:center;flex-wrap:wrap}
footer a{color:var(--muted);text-decoration:none}footer a:hover{color:var(--fg)}
@keyframes rise{from{opacity:0;transform:translateY(10px)}}@media (prefers-reduced-motion:reduce){main{animation:none}}
";

/// The logo's arch and flame, as on the account page.
const MARK: &str = r##"<svg viewBox="0 0 512 512" aria-hidden="true"><g fill="none" stroke="currentColor" stroke-width="40" stroke-linecap="round"><path d="M136 376V240a120 120 0 0 1 240 0v136"/><path d="M104 396h304"/></g><path fill="#ffb366" d="M258 172C266 218 300 238 312 272C326 312 300 358 254 358C210 358 188 324 194 292C199 264 222 250 231 224C240 246 250 251 256 243C263 232 256 202 258 172Z"/></svg>"##;

fn page(status: StatusCode, title: &str, body: &str) -> Response {
    let html = format!(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\">\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\
         <meta name=\"referrer\" content=\"no-referrer\">\
         <meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; img-src 'self'; style-src 'unsafe-inline'; form-action 'self'\">\
         <link rel=\"icon\" href=\"/favicon.svg\" type=\"image/svg+xml\"><link rel=\"icon\" href=\"/favicon.ico\" sizes=\"any\">\
         <title>{title} · Hearth</title><style>{STYLE}</style></head><body><main>\
         <h1>{MARK}<span>Hearth {title}</span></h1>{body}\
         <footer><a href=\"/\">Account</a><a href=\"/reset\">Forgot password</a>\
         <a href=\"/privacy\">Privacy</a><a href=\"/terms\">Terms</a></footer></main></body></html>",
        title = esc(title)
    );
    (status, Html(html)).into_response()
}

fn said(status: StatusCode, title: &str, text: &str) -> Response {
    page(status, title, &format!("<section><p>{}</p></section>", esc(text)))
}

// --- resetting a password -----------------------------------------------------

async fn reset_page(State(state): State<Arc<Fireplace>>) -> Response {
    let by_email = if state.mail.enabled() {
        "<section><h2>By email</h2><p class=\"muted\">If the address belongs to a confirmed account here, \
         a link is sent to it. It works once, for 30 minutes.</p>\
         <form method=\"post\" action=\"/reset\"><label for=\"email\">Email</label>\
         <input id=\"email\" name=\"email\" type=\"email\" autocomplete=\"email\" required maxlength=\"254\">\
         <button>Send me a link</button></form></section>"
    } else {
        "<p class=\"muted\">This server sends no email. Use a recovery code, or ask whoever runs it.</p>"
    };
    let by_code = "<section><h2>With a recovery code</h2><p class=\"muted\">One of the eight codes you were shown \
         when you made your account. Each works once.</p>\
         <form method=\"post\" action=\"/recover\"><label for=\"handle\">Handle</label>\
         <input id=\"handle\" name=\"handle\" autocomplete=\"username\" required maxlength=\"20\">\
         <label for=\"code\">Recovery code</label><input id=\"code\" name=\"code\" required maxlength=\"40\" autocomplete=\"off\">\
         <label for=\"password\">New password</label>\
         <input id=\"password\" name=\"password\" type=\"password\" autocomplete=\"new-password\" required minlength=\"8\" maxlength=\"128\">\
         <button>Set new password</button></form></section>";
    page(StatusCode::OK, "password reset", &format!("{by_email}{by_code}"))
}

#[derive(serde::Deserialize)]
pub struct AskForReset {
    email: String,
}

/// The same page, whatever happened: an address nobody has, one that is not
/// confirmed, one over its limit and one that got a letter all look alike.
async fn ask_for_reset(
    State(state): State<Arc<Fireplace>>,
    Caller(from): Caller,
    Form(body): Form<AskForReset>,
) -> Response {
    if !state.mail.enabled() {
        return said(
            StatusCode::NOT_FOUND,
            "password reset",
            "This server sends no email. Use a recovery code.",
        );
    }
    let now = token::now();
    if !state.limits.lock().unwrap().allow(&format!("reset:{from}"), RESETS_PER_HOUR, limit::HOUR, now) {
        return said(
            StatusCode::TOO_MANY_REQUESTS,
            "password reset",
            "That is a lot of resets from here. Try again in an hour.",
        );
    }
    let found: Vec<(u64, String, u64, String)> = {
        let accounts = state.accounts.lock().unwrap();
        accounts
            .by_verified_email(body.email.trim())
            .into_iter()
            .map(|a| {
                (
                    a.id,
                    a.handle.clone(),
                    a.token_epoch,
                    a.email.clone().map(|e| e.address).unwrap_or_default(),
                )
            })
            .collect()
    };
    for (id, handle, epoch, address) in found {
        if !state.limits.lock().unwrap().allow(&format!("reset@{id}"), RESETS_PER_ACCOUNT, limit::HOUR, now) {
            continue;
        }
        let link =
            format!("{}/reset/{}", state.mail.public_url, state.keys.mint_link("reset", id, epoch, "", now));
        state.mail.send(&address, state.mail.reset_letter(&handle, &link));
    }
    said(
        StatusCode::OK,
        "password reset",
        "If that address belongs to a confirmed account here, a link is on its way. It works once, for 30 minutes.",
    )
}

/// The account a reset link is for, if it is still good: signed, in time,
/// and not used -- using one moves the epoch, which is what makes it once.
fn reset_link(state: &Fireplace, raw: &str) -> Option<token::LinkClaims> {
    let claims = state.keys.verify_link(raw, "reset", token::now()).ok()?;
    let accounts = state.accounts.lock().unwrap();
    (accounts.get(claims.sub)?.token_epoch == claims.epoch).then_some(claims)
}

const LINK_GONE: &str = "This link has run out or has already been used. Ask for a new one.";

async fn new_password_page(State(state): State<Arc<Fireplace>>, Path(raw): Path<String>) -> Response {
    if reset_link(&state, &raw).is_none() {
        return said(StatusCode::GONE, "password reset", LINK_GONE);
    }
    page(
        StatusCode::OK,
        "new password",
        &format!(
            "<section><form method=\"post\" action=\"/reset/{}\"><label for=\"password\">New password</label>\
             <input id=\"password\" name=\"password\" type=\"password\" autocomplete=\"new-password\" required minlength=\"8\" maxlength=\"128\">\
             <p class=\"muted\">This signs your account out everywhere.</p><button>Set new password</button></form></section>",
            esc(&raw)
        ),
    )
}

#[derive(serde::Deserialize)]
pub struct NewPassword {
    password: String,
}

async fn set_new_password(
    State(state): State<Arc<Fireplace>>,
    Caller(from): Caller,
    Path(raw): Path<String>,
    Form(body): Form<NewPassword>,
) -> Response {
    let now = token::now();
    if !state.limits.lock().unwrap().allow(
        &format!("link:{from}"),
        limit::SIGN_INS,
        limit::SIGN_IN_WINDOW,
        now,
    ) {
        return said(StatusCode::TOO_MANY_REQUESTS, "password reset", "Too many tries. Wait a few minutes.");
    }
    let Some(claims) = reset_link(&state, &raw) else {
        return said(StatusCode::GONE, "password reset", LINK_GONE);
    };
    if !state.limits.lock().unwrap().allow(
        &format!("link@{}", claims.sub),
        limit::SIGN_INS,
        limit::SIGN_IN_WINDOW,
        now,
    ) {
        return said(StatusCode::TOO_MANY_REQUESTS, "password reset", "Too many tries. Wait a few minutes.");
    }
    if !accounts::password_is_usable(&body.password) {
        return said(
            StatusCode::BAD_REQUEST,
            "password reset",
            "A password is 8 to 128 characters. Go back and try again.",
        );
    }
    let Ok(hashed) = hash_password(body.password).await else {
        return said(StatusCode::INTERNAL_SERVER_ERROR, "password reset", "That did not work. Try again.");
    };
    // Checked again under the lock: two tabs submitting one link at once
    // must not both get through.
    if !change_password(&state, claims.sub, Some(claims.epoch), hashed) {
        return said(StatusCode::GONE, "password reset", LINK_GONE);
    }
    tracing::info!(id = claims.sub, "reset their password by email");
    said(StatusCode::OK, "password reset", "Done. Your password is changed and every old sign-in has ended.")
}

/// Sets the password (which moves the epoch, signing out everywhere) and
/// tells the confirmed address, if there is one. With `epoch`, only if the
/// account is still on it.
pub fn change_password(state: &Fireplace, id: u64, epoch: Option<u64>, hashed: String) -> bool {
    let notify = {
        let mut accounts = state.accounts.lock().unwrap();
        let Some(account) = accounts.get(id) else {
            return false;
        };
        if epoch.is_some_and(|e| e != account.token_epoch) {
            return false;
        }
        let notify =
            account.email.clone().filter(|e| e.verified).map(|e| (e.address, account.handle.clone()));
        if accounts.set_password(id, hashed).is_err() {
            return false;
        }
        notify
    };
    if let Some((address, handle)) = notify {
        state.mail.send(&address, state.mail.password_changed_letter(&handle));
    }
    true
}

#[derive(serde::Deserialize)]
pub struct Recover {
    handle: String,
    code: String,
    password: String,
}

/// A recovery code instead of an email: for everybody, including accounts
/// with no address and servers that send none.
async fn recover(
    State(state): State<Arc<Fireplace>>,
    Caller(from): Caller,
    Form(body): Form<Recover>,
) -> Response {
    let now = token::now();
    let handle = body.handle.trim().to_lowercase();
    {
        let mut limits = state.limits.lock().unwrap();
        let by_address =
            limits.allow(&format!("recover:{from}"), limit::SIGN_INS, limit::SIGN_IN_WINDOW, now);
        let by_handle =
            limits.allow(&format!("recover@{handle}"), limit::SIGN_INS, limit::SIGN_IN_WINDOW, now);
        if !by_address || !by_handle {
            return said(
                StatusCode::TOO_MANY_REQUESTS,
                "password reset",
                "Too many tries. Wait a few minutes.",
            );
        }
    }
    if !accounts::password_is_usable(&body.password) {
        return said(
            StatusCode::BAD_REQUEST,
            "password reset",
            "A password is 8 to 128 characters. Go back and try again.",
        );
    }
    let code = accounts::hash_recovery(&body.code);
    let wrong = || {
        said(StatusCode::UNAUTHORIZED, "password reset", "That handle and recovery code do not go together.")
    };
    let Some(id) = state
        .accounts
        .lock()
        .unwrap()
        .by_handle(&handle)
        .filter(|a| a.recovery.contains(&code))
        .map(|a| a.id)
    else {
        return wrong();
    };
    let Ok(hashed) = hash_password(body.password).await else {
        return said(StatusCode::INTERNAL_SERVER_ERROR, "password reset", "That did not work. Try again.");
    };
    // Spent under the lock, so one code cannot be used twice at once.
    if !state.accounts.lock().unwrap().spend_recovery(id, &code) {
        return wrong();
    }
    change_password(&state, id, None, hashed);
    let left = state.accounts.lock().unwrap().get(id).map_or(0, |a| a.recovery.len());
    tracing::info!(id, left, "reset their password with a recovery code");
    said(
        StatusCode::OK,
        "password reset",
        &format!(
            "Done. Your password is changed and every old sign-in has ended. You have {left} recovery codes left; \
             you can make a new set on your account page."
        ),
    )
}

// --- confirming an address ----------------------------------------------------

async fn verify(State(state): State<Arc<Fireplace>>, Path(raw): Path<String>) -> Response {
    let confirmed = state
        .keys
        .verify_link(&raw, "verify", token::now())
        .is_ok_and(|claims| state.accounts.lock().unwrap().verify_email(claims.sub, &claims.email));
    if confirmed {
        said(
            StatusCode::OK,
            "email",
            "Thank you: the address is confirmed. It is used for password resets and security notices, and nothing else.",
        )
    } else {
        said(
            StatusCode::GONE,
            "email",
            "This link has run out, or the address on the account has changed since. Add it again from your account.",
        )
    }
}

// --- what this server does with your data -------------------------------------

async fn privacy(State(state): State<Arc<Fireplace>>) -> Response {
    page(StatusCode::OK, "privacy", &privacy_html(&state.config))
}

/// GDPR Art. 13, for **this** server, from its own config.
pub fn privacy_html(config: &crate::Config) -> String {
    let op = &config.operator;
    let or = |text: &str, otherwise: &str| {
        if text.trim().is_empty() { otherwise.to_owned() } else { esc(text.trim()) }
    };
    let controller = if op.name.trim().is_empty() {
        "Whoever runs this server has not filled in who they are (<code>[operator]</code> in <code>fireplace.toml</code>).".to_owned()
    } else {
        format!(
            "The controller is <strong>{}</strong>. Privacy requests: <a href=\"mailto:{c}\">{c}</a>.",
            esc(&op.name),
            c = esc(&op.contact_email)
        )
    };
    let mail = &config.mail;
    let email_part = match mail.backend {
        Backend::None => "<p>This server sends no email and never asks for an address.</p>".to_owned(),
        _ => format!(
            "<p>Adding an email is <strong>optional</strong>. It is used only to send a password reset link you \
             ask for and a short notice when your password or email changes. Never marketing, never shared. \
             An address you do not confirm within 7 days is deleted; removing it deletes it at once.</p>\
             <p>Emails are sent through <code>{host}</code>, {who}</p>",
            host = esc(&mail.smtp.host),
            who = if mail.provider.trim().is_empty() {
                "run by the controller.".to_owned()
            } else {
                format!(
                    "run by {}, which handles them on the controller's behalf as a processor (GDPR Art. 28).",
                    esc(&mail.provider)
                )
            },
        ),
    };
    format!(
        "<section><h2>Who</h2><p>{controller}</p><p>Where the data is kept: {hosting}.</p></section>\
         <section><h2>What is stored</h2><ul>\
         <li>Your handle, display name, and when the account was made.</li>\
         <li>Your password and recovery codes, only as one-way hashes.</li>\
         <li>Your friends, friend requests, and whether friends may see which server you are on.</li>\
         <li>Your email and whether it is confirmed, only if you add one.</li></ul>\
         <p>Kept in memory only, and gone on restart: which server you are on (if you share it, for 2 minutes \
         after your game last said so), and counters of attempts per IP address (at most an hour) that stop \
         people guessing passwords. IP addresses are not written to disk or logs.</p>\
         <p>A game server you join learns only your display name and a number that means something on that \
         server alone. No analytics, no advertising, no tracking cookies, nothing loaded from anywhere else. \
         The account page keeps your sign-in in the browser tab's session storage, which is strictly needed \
         for it to work.</p></section>\
         <section><h2>Why, and on what basis</h2><p>To run your account and send the emails above: performance \
         of the service you asked for (GDPR Art. 6(1)(b)). The attempt counters: our legitimate interest in \
         keeping accounts safe (Art. 6(1)(f)). Nothing is based on consent, and nothing is decided about you \
         automatically.</p>{email_part}</section>\
         <section><h2>How long</h2><p>Until you delete your account, which deletes it at once. Backups roll it \
         out within {days} days. Reset links last 30 minutes.</p></section>\
         <section><h2>Your rights</h2><p>You can see everything stored about you and take it with you \
         (access and portability: <em>Download my data</em> on the account page, or <code>GET /me/export</code>), \
         correct it (rectification: change your display name or email yourself), delete it (erasure: \
         <em>Delete account</em>), and object to or ask to restrict its use, by writing to the contact above. \
         You may also complain to {authority}.</p></section>\
         <section><h2>Age</h2><p>You must be at least {age} to make an account (<a href=\"/terms\">terms</a>).</p></section>",
        hosting = or(&op.hosting, "not stated by the operator"),
        days = op.backup_days,
        authority =
            or(&op.authority, "the data protection authority where you live, work, or where the operator is"),
        age = op.min_age,
    )
}

async fn terms(State(state): State<Arc<Fireplace>>) -> Response {
    let op = &state.config.operator;
    let who = if op.name.trim().is_empty() { "whoever runs this server".to_owned() } else { esc(&op.name) };
    page(
        StatusCode::OK,
        "terms",
        &format!(
            "<section><ul>\
             <li>You must be at least <strong>{age}</strong> years old to make an account.</li>\
             <li>This account server is run by {who}, free and as it is, with no promise that it will always be \
             here. Keep your recovery codes somewhere safe.</li>\
             <li>Game servers (Flames) are run by other people. What happens on one is theirs, not this server's.</li>\
             <li>Do not attack this server, make accounts in bulk, or use it to harass anybody. Accounts that do \
             may be removed.</li>\
             <li>You can delete your account whenever you like, from the account page.</li>\
             <li>What is stored and why: <a href=\"/privacy\">privacy</a>.</li></ul></section>",
            age = op.min_age,
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_an_operator_or_a_stranger_typed_cannot_become_markup() {
        assert_eq!(esc("<script>\"x\"&'y'"), "&lt;script&gt;&quot;x&quot;&amp;&#39;y&#39;");
        let mut config = crate::Config::default();
        config.operator.name = "<b>Eve</b>".into();
        assert!(!privacy_html(&config).contains("<b>Eve"));
    }
}
