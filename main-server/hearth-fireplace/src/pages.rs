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
@media (prefers-color-scheme:light){:root{--bg:#eceef5;--fg:#1a1f3a;--muted:#565d7c;--card:#fff;--line:#d3d8e6;--accent:#ad500f;--stone:#3d4f9c;--on:#fff}}
*{box-sizing:border-box}body{margin:0;background:var(--bg);color:var(--fg);font:15px/1.6 system-ui,-apple-system,'Segoe UI',sans-serif}
main{max-width:40rem;margin:0 auto;padding:3rem 16px;animation:rise .5s cubic-bezier(.2,.7,.2,1) both}
h1,h2{font-family:'Iowan Old Style','Palatino Linotype',Palatino,Georgia,serif;font-weight:600}
h1{font-size:1.9rem;margin:0 0 1.5rem;display:flex;align-items:center;gap:12px}h1 img{width:40px;height:40px;flex:none;border-radius:10px}
h2{font-size:1.15rem;margin:0 0 .5rem}section{background:var(--card);border:1px solid var(--line);border-radius:14px;padding:1.2rem 1.4rem;margin:1rem 0}
label{display:block;margin:.9rem 0 .3rem;font-size:.82rem;font-weight:500;color:var(--muted)}
input{width:100%;padding:.7rem .8rem;font:inherit;border:1px solid var(--line);border-radius:10px;background:var(--bg);color:var(--fg)}
input:focus{outline:none;border-color:var(--accent);box-shadow:0 0 0 3px color-mix(in srgb,var(--accent) 22%,transparent)}
button:focus-visible{outline:2px solid var(--accent);outline-offset:2px}
button{margin-top:1.1rem;padding:.65rem 1.2rem;font:inherit;font-weight:600;border:0;border-radius:10px;background:var(--accent);color:var(--on);cursor:pointer;transition:transform .12s}
button:active{transform:scale(.97)}p.muted{color:var(--muted)}a{color:var(--accent)}code{word-break:break-all}
table{width:100%;border-collapse:collapse;font-size:.88rem;min-width:36rem}th,td{text-align:left;vertical-align:top;padding:.55rem .6rem;border-top:1px solid var(--line)}th{color:var(--muted);font-weight:500;border-top:0}.scroll{overflow-x:auto;margin:0 -.4rem}ul{padding-left:1.2rem}li{margin:.3rem 0}
footer{margin-top:2.5rem;font-size:.85rem;display:flex;gap:18px;justify-content:center;flex-wrap:wrap}
footer a{color:var(--muted);text-decoration:none}footer a:hover{color:var(--fg)}
@keyframes rise{from{opacity:0;transform:translateY(10px)}}@media (prefers-reduced-motion:reduce){main{animation:none}}
";

/// The logo, which the account page serves (built in, see `api::built_in_web`).
const MARK: &str = "<img src=\"/favicon.svg\" alt=\"\">";

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
        if !state.limits.lock().unwrap().allow(&format!("reset@{id}"), RESETS_PER_ACCOUNT, limit::HOUR, now)
            || !state.may_email(&address, &from, now)
        {
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
    if let Some((address, handle)) = notify.filter(|(address, _)| state.may_email(address, "", token::now())) {
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

// --- banned -------------------------------------------------------------------

/// What a banned address, or a suspended account, is shown: for how long and why.
pub fn banned_page(title: &str, ban: &accounts::Ban) -> Response {
    let until = if ban.until == 0 {
        "<p>This is <strong>for good</strong>.</p>".to_owned()
    } else {
        format!("<p>Until <strong>{}</strong>.</p>", esc(&accounts::when(ban.until)))
    };
    page(
        StatusCode::FORBIDDEN,
        title,
        &format!(
            "<section>{until}<p class=\"muted\">Reason</p><p>{}</p></section>\
             <section><p class=\"muted\">If you think this is a mistake, write to whoever runs this server \
             (see <a href=\"/privacy\">privacy</a>), and say when it happened.</p></section>",
            esc(&ban.reason)
        ),
    )
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
    let mailing = !matches!(mail.backend, Backend::None);
    let sender = if mail.provider.trim().is_empty() {
        format!("through <code>{}</code>, run by the controller", esc(&mail.smtp.host))
    } else {
        format!(
            "through <code>{}</code>, run by {}, which handles them for the controller as a processor (GDPR Art. 28)",
            esc(&mail.smtp.host),
            esc(&mail.provider)
        )
    };
    // One row per thing kept: what, what for, on what basis, how long.
    let row = |what: &str, why: &str, basis: &str, how_long: &str| {
        format!("<tr><td>{what}</td><td>{why}</td><td>{basis}</td><td>{how_long}</td></tr>")
    };
    let contract = "Contract: running the account you asked for (Art. 6(1)(b))";
    let security = "Legitimate interest in keeping accounts safe (Art. 6(1)(f))";
    let until_deleted = "Until you delete your account";
    let mut rows = vec![
        row("Handle, display name, when the account was made", "Your account, and what other players see", contract, until_deleted),
        row("Password and recovery codes, only as one-way hashes", "Signing in, and getting back in", contract, until_deleted),
        row("Friends, friend requests, whether friends may see which server you are on", "Friends", contract, until_deleted),
        row(
            "Where you are signed in: each session's device type (like \"Firefox on Windows\"), IP address, and when it started and was last used",
            "So you can see your sessions and sign any of them out (Security)",
            security,
            "Until you sign it out, or 30 days after it was last used",
        ),
        row("Whether two-step sign-in is on", "Asking for a code from your email on every sign-in", contract, until_deleted),
        row("When you last changed your display name (the last 30 days)", "Display names change at most twice a month, so nobody can hide behind a new one every day", security, "30 days"),
        row("Your picture, if you upload one", "Shown to other players next to your name; anyone with its link can see it. A new one is first looked at by this server's moderators", contract, "Until you remove it or delete your account"),
        row("Pronouns and a bio, if you write them", "Your public profile, which anybody may look up", contract, "Until you change them or delete your account"),
        row("When you accepted the terms and this notice", "A record that you did", "Legal obligation to be able to show it (Art. 6(1)(c), 7(1))", until_deleted),
        row(
            "<strong>IP addresses you signed in from</strong>, with when each was first and last used",
            "Security: a sign-in from an address you have not used makes us email you to confirm it was you, \
             so a stolen password alone is not enough. Also to stop attacks on the server",
            security,
            "90 days after the address was last used",
        ),
    ];
    if mailing {
        rows.push(row(
            "<strong>Your email address</strong>, and whether you confirmed it (required)",
            "Confirming it is you on a new address, password reset links you ask for, and short notices when your \
             password or email changes. Never marketing, never shared",
            "Contract (Art. 6(1)(b)) and security (Art. 6(1)(f))",
            "Until you delete your account; an address you never confirm is deleted after 7 days",
        ));
        rows.push(row(
            "Whether you want an email when the terms or this notice change",
            "Telling you about those changes",
            "Your consent (Art. 6(1)(a)): optional, off by default, withdraw it on the account page any time",
            "Until you turn it off or delete your account",
        ));
    }
    let email_note = if mailing {
        format!("<p>Emails are sent {sender}.</p>")
    } else {
        "<p>This server sends no email and never asks for an address.</p>".to_owned()
    };
    format!(
        "<section><h2>Who</h2><p>{controller}</p><p>Where the data is kept: {hosting}.</p></section>\
         <section><h2>What is kept, what for, and why</h2>\
         <p>Everything this server keeps about you is in this table. Nothing is hidden, and you can download \
         all of it yourself (<em>Download my data</em> on the account page).</p>\
         <div class=\"scroll\"><table><thead><tr><th>What</th><th>What for</th><th>Legal basis (GDPR)</th>\
         <th>How long</th></tr></thead><tbody>{rows}</tbody></table></div>{email_note}</section>\
         <section><h2>Not kept</h2><ul>\
         <li>Your <strong>date of birth</strong>: asked once, when you make an account, only to check you are \
         at least {age} (Art. 8). Used for that check and thrown away: never stored, never logged.</li>\
         <li>Which server you are on: kept in memory only, if you share it, for 2 minutes after your game last \
         said so, and gone on restart.</li>\
         <li>Counts of how many emails went to an address, so nobody can use this server to flood an inbox: \
         memory only, keyed by a one-way hash of the address rather than the address, for at most a day.</li>\
         <li>The 6-digit codes we email you: kept in memory only, as a one-way hash, for 15 minutes or until \
         used, and gone on restart.</li>\
         <li>Counters of attempts per IP address, which stop people guessing passwords: memory only, at most \
         an hour.</li>\
         <li>IP addresses are never written to the server's logs.</li>\
         <li>No analytics, no advertising, no tracking cookies, nothing loaded from anywhere else. The account \
         page keeps your sign-in in the browser tab's session storage, which it needs to work.</li></ul>\
         <p>A game server you join learns only your display name and a number that means something on that \
         server alone: not your IP address as kept here, not your email.</p></section>\
         <section><h2>Who can see it</h2><ul>\
         <li><strong>You</strong>: all of it, including every IP address you signed in from (account page, \
         <em>Security</em>).</li>\
         <li><strong>This server's admins</strong>: the people the controller trusts to run it can see accounts, \
         emails and sign-in IP addresses, to stop abuse. They can suspend an account or block an address, \
         always with a reason and an end date or \"for good\", and the person is shown both.</li>\
         <li><strong>Other players</strong>: your public profile (handle, display name, picture, and pronouns \
         and bio if you wrote them), which anybody may look up; your friends also see \
         whether you are online and, if you allow it, on which server.</li>\
         <li>Nobody else. Nothing is sold or shared.</li></ul></section>\
         <section><h2>Decisions made automatically</h2><p>Only three, all simple rules, none of them \
         profiling: an account is refused below the minimum age; a sign-in from a new IP address waits until \
         you confirm it by email; and signing in to a <strong>suspended</strong> account with its correct \
         password blocks the IP address it came from until the suspension ends, with the same reason, so it \
         cannot be used to sign in to another account. That block is kept with the other blocked addresses, \
         admins can see and lift it, and it is lifted when the account is unbanned. If you share an address \
         with somebody suspended and are blocked by mistake, write to the contact above.</p></section>\
         <section><h2>How long, in general</h2><p>Deleting your account deletes all of it at once, picture \
         included. Backups roll it out within {days} days. Links in emails last 30 minutes (confirming an \
         address: 7 days).</p></section>\
         <section><h2>Your rights</h2><p>You can see everything kept about you and take it with you (access \
         and portability: <em>Download my data</em>, or <code>GET /me/export</code>), correct it \
         (rectification: change your display name or email yourself), delete it (erasure: <em>Delete \
         account</em>), and object to or ask to restrict its use, including the IP address check, by writing \
         to the contact above. You may also complain to {authority}.</p></section>\
         <section><h2>Age</h2><p>You must be at least {age} to make an account (<a href=\"/terms\">terms</a>).</p></section>",
        rows = rows.concat(),
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
             <li>You must be at least <strong>{age}</strong> years old to make an account. You are asked for \
             your date of birth to check this; it is not kept.</li>\
             <li>This account server is run by {who}, free and as it is, with no promise that it will always be \
             here. Keep your recovery codes somewhere safe.</li>\
             <li>Game servers (Flames) are run by other people. What happens on one is theirs, not this server's.</li>\
             <li>Where this server sends email, an account needs a confirmed email address, and a sign-in from \
             an IP address the account has not used waits until you confirm it by email.</li>\
             <li>A display name can change at most twice in 30 days.</li>\
             <li>Do not attack this server, make accounts in bulk, cheat, upload pictures that are illegal or \
             hurtful, or use it to harass anybody. Admins may suspend an account or block an IP address for \
             that, for a set time or for good; you are always shown how long and why, and can write to the \
             operator if you think it is a mistake. Signing in to a suspended account blocks the address it \
             was signed in from for as long as the suspension lasts.</li>\
             <li>You can delete your account whenever you like, from the account page.</li>\
             <li>When these terms or the privacy notice change in a way that matters, they are updated here, and \
             if you asked for it and have a confirmed email, you are told by email too.</li>\
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
