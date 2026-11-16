//! The routes. JSON in, JSON out, one error shape.
//!
//! Everything a signed-in player does carries `Authorization: Bearer <token>`
//! and nothing else: there is no session table to keep, which is what makes a
//! restart invisible to everybody who is signed in.
//!
//! POST and GET, because Godot's `HTTPRequest` is happiest that way. The one
//! exception is `DELETE /me`, the GDPR-shaped name for `POST /me/delete`.

use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;

use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier};
use axum::extract::{ConnectInfo, DefaultBodyLimit, FromRequestParts, Path, State};
use axum::http::{HeaderMap, StatusCode, request::Parts};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::{Value, json};

use crate::accounts::{self, Ban, Refused};
use crate::avatar;
use crate::presence::{self, Where};
use crate::{Fireplace, limit, token};

/// Nothing we read is bigger than a handle and a password.
const MAX_BODY: usize = 8 * 1024;
/// A request that has not finished in this long is not going to.
const TIMEOUT_SECONDS: u64 = 10;
/// Hashing a password that does not exist, so that a wrong handle takes the
/// same time as a wrong password and the two cannot be told apart.
const NOBODY: &str =
    "$argon2id$v=19$m=19456,t=2,p=1$c2FsdHNhbHRzYWx0c2E$+Y1dD5m5Zq0hJjZk1xkX0wGjvV1S0kTqCk3K5bN5zPk";

pub fn router(state: Arc<Fireplace>) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/key", get(public_key))
        .route("/accounts", post(create_account))
        .route("/sessions", post(sign_in))
        .route("/sessions/refresh", post(refresh))
        .route("/features", get(features))
        .route("/me", get(me).delete(delete_account))
        .route("/me/email", post(set_email))
        .route("/me/recovery", post(new_recovery_codes))
        .route("/me/export", get(export))
        .route("/me/name", post(set_name))
        .route("/me/password", post(set_password))
        .route("/me/delete", post(delete_account))
        .route("/join", post(join))
        .route("/settings", post(settings))
        .route("/friends", get(friends))
        .route("/friends/add", post(add_friend))
        .route("/friends/accept", post(accept_friend))
        .route("/friends/remove", post(remove_friend))
        .route("/presence", post(report_presence))
        .route("/sessions/code", post(sign_in_with_code))
        .route("/me/sessions", get(my_sessions))
        .route("/me/sessions/end", post(end_session))
        .route("/me/email/confirm", post(confirm_email))
        .route("/me/email/resend", post(resend_confirmation))
        .route("/me/two-factor", post(set_two_factor))
        .route("/me/avatar", post(upload_avatar).layer(DefaultBodyLimit::max(avatar::MAX_UPLOAD)))
        .route("/me/avatar/delete", post(delete_avatar))
        .route("/avatars/{file}", get(avatar_file))
        .route("/admin/claim", post(admin_claim))
        .route("/admin/accounts", get(admin_accounts))
        .route("/admin/bans", get(admin_bans))
        .route("/admin/ban", post(admin_ban))
        .route("/admin/unban", post(admin_unban))
        .route("/admin/ban-ip", post(admin_ban_ip))
        .route("/admin/unban-ip", post(admin_unban_ip))
        .route("/admin/delete", post(admin_delete))
        .route("/admin/set-admin", post(admin_set_admin))
        .route("/admin/sign-out", post(admin_sign_out))
        .merge(crate::pages::routes())
        // Anything that is not a route is a file from the web folder: the
        // account page, its stylesheet and script, and whatever else a host
        // puts there. ServeDir refuses anything that climbs out of it. A file
        // the folder does not have comes from the copy built into the binary,
        // so the page works on a server nobody copied web/ to.
        .fallback_service(
            tower_http::services::ServeDir::new(&state.web_dir)
                .fallback(axum::handler::HandlerWithoutStateExt::into_service(built_in_web)),
        )
        .layer(tower_http::timeout::TimeoutLayer::with_status_code(
            StatusCode::REQUEST_TIMEOUT,
            std::time::Duration::from_secs(TIMEOUT_SECONDS),
        ))
        .layer(DefaultBodyLimit::max(MAX_BODY))
        // Outermost, so a banned address is turned away before its body is
        // read or anything else is done for it.
        .layer(axum::middleware::from_fn_with_state(state.clone(), refuse_banned))
        .with_state(state)
}

/// The repository's `web/`, as it was when this binary was built.
async fn built_in_web(uri: axum::http::Uri) -> Response {
    let (body, kind): (&[u8], &str) = match uri.path() {
        "/" | "/index.html" => (include_bytes!("../../../web/index.html"), "text/html; charset=utf-8"),
        "/style.css" => (include_bytes!("../../../web/style.css"), "text/css; charset=utf-8"),
        "/app.js" => (include_bytes!("../../../web/app.js"), "text/javascript; charset=utf-8"),
        "/favicon.svg" => (include_bytes!("../../../web/favicon.svg"), "image/svg+xml"),
        "/favicon.ico" => (include_bytes!("../../../web/favicon.ico"), "image/x-icon"),
        _ => return StatusCode::NOT_FOUND.into_response(),
    };
    ([(axum::http::header::CONTENT_TYPE, kind)], body).into_response()
}

/// What a refusal by `Fireplace::may_email` says.
const TOO_MANY_EMAILS: &str = "too many emails to that address or from here: try again later";

/// Sign-in codes an account can be sent in an hour.
const CODES_PER_HOUR: u32 = 6;

/// The confirmed address to send a code to, and whether it is because `ip`
/// is new, when this sign-in needs one: always from an IP address the account
/// has not used, and every time with two-step sign-in on. Otherwise the place
/// is noted and `None` -- so also for an account with no confirmed address to
/// send to, or a Fireplace with no email.
fn needs_code(state: &Fireplace, id: u64, ip: &str, now: u64) -> Option<(String, bool)> {
    let mut accounts = state.accounts.lock().unwrap();
    let account = accounts.get(id)?;
    let address = account.email.as_ref().filter(|e| e.verified).map(|e| e.address.clone());
    let two_step = account.two_factor;
    let new_place = !accounts.place_is_known(id, ip);
    let ask = state.mail.enabled() && address.is_some() && (new_place || two_step);
    accounts.remember_place(id, ip, now, !ask);
    if ask { address.map(|a| (a, new_place)) } else { None }
}

/// A code to type, and a link for when the page is gone. The caller has
/// already asked `may_email`.
fn send_confirmation(state: &Fireplace, id: u64, handle: &str, epoch: u64, address: &str, now: u64) {
    let link = format!("{}/verify/{}", state.mail.public_url, state.keys.mint_link("verify", id, epoch, address, now));
    let (_, code) = state.codes.lock().unwrap().issue("verify", id, address, now);
    state.mail.send(address, state.mail.verify_letter(handle, &code, &link));
}

fn wrong_code(wrong: crate::codes::Wrong) -> ApiError {
    use crate::codes::Wrong;
    match wrong {
        Wrong::NotIt => ApiError::new(StatusCode::UNAUTHORIZED, "that is not the code"),
        Wrong::UsedUp => ApiError::new(StatusCode::GONE, "that was the last try: ask for a new code"),
        Wrong::Gone => ApiError::new(StatusCode::GONE, "that code has run out: ask for a new one"),
    }
}

// --- what comes back when something will not be done ------------------------

/// One shape for every refusal: a status and a sentence. Never a stack trace,
/// never the internals -- this is the one service here that strangers talk to.
pub struct ApiError(StatusCode, String);

impl ApiError {
    fn new(status: StatusCode, say: impl Into<String>) -> Self {
        Self(status, say.into())
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(json!({ "error": self.1 }))).into_response()
    }
}

impl From<Refused> for ApiError {
    fn from(refused: Refused) -> Self {
        let status = match refused {
            Refused::BadHandle | Refused::Yourself | Refused::BadDisplayName => StatusCode::BAD_REQUEST,
            Refused::HandleTaken | Refused::AlreadyFriends => StatusCode::CONFLICT,
            Refused::NoSuchAccount => StatusCode::NOT_FOUND,
            Refused::TooManyFriends | Refused::TooManyRequests => StatusCode::CONFLICT,
            Refused::Full => StatusCode::SERVICE_UNAVAILABLE,
        };
        Self::new(status, refused.to_string())
    }
}

fn bad(say: &str) -> ApiError {
    ApiError::new(StatusCode::BAD_REQUEST, say)
}

fn unauthorised() -> ApiError {
    ApiError::new(StatusCode::UNAUTHORIZED, "sign in again")
}

fn too_many(what: &str) -> ApiError {
    ApiError::new(StatusCode::TOO_MANY_REQUESTS, what)
}

// --- who is asking ----------------------------------------------------------

/// The address a request came from, for rate limiting.
///
/// Behind Caddy every peer is loopback, so the real one is the **last** entry
/// of `X-Forwarded-For` -- last, because a proxy appends the peer it saw, so
/// anything a client puts in the header is pushed to the left of the truth.
/// And only when the peer really is loopback: anywhere else the header is
/// just something a stranger typed.
pub struct Caller(pub String);

impl<S: Send + Sync> FromRequestParts<S> for Caller {
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        Ok(Self(ip_of(&parts.extensions, &parts.headers)))
    }
}

fn ip_of(extensions: &axum::http::Extensions, headers: &HeaderMap) -> String {
    let peer = extensions.get::<ConnectInfo<SocketAddr>>().map(|info| info.0.ip());
    let behind_proxy = peer.is_none_or(|ip| ip.is_loopback());
    let forwarded = headers
        .get("x-forwarded-for")
        .and_then(|value| value.to_str().ok())
        .and_then(|list| list.rsplit(',').next())
        .map(str::trim)
        .and_then(|one| one.parse::<IpAddr>().ok());
    match (behind_proxy, forwarded) {
        (true, Some(real)) => real.to_string(),
        _ => peer.map(|ip| ip.to_string()).unwrap_or_else(|| "unknown".into()),
    }
}

/// A banned address gets a page saying so and nothing else: one map lookup,
/// before the body is read or any route runs.
async fn refuse_banned(
    State(state): State<Arc<Fireplace>>,
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> Response {
    let ip = ip_of(request.extensions(), request.headers());
    let Some(ban) = state.ip_bans.lock().unwrap().of(&ip, token::now()).cloned() else {
        return next.run(request).await;
    };
    let wants_page = request
        .headers()
        .get(axum::http::header::ACCEPT)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|a| a.contains("text/html"));
    if wants_page {
        return crate::pages::banned_page("This address is blocked", &ban);
    }
    let error = format!("this address is blocked {}: {}", until_text(&ban), ban.reason);
    (StatusCode::FORBIDDEN, Json(json!({ "error": error, "banned": ban_json(&ban) }))).into_response()
}

pub fn until_text(ban: &Ban) -> String {
    if ban.until == 0 { "for good".into() } else { format!("until {}", accounts::when(ban.until)) }
}

fn ban_json(ban: &Ban) -> Value {
    json!({ "until": ban.until, "until_text": until_text(ban), "reason": ban.reason, "at": ban.at })
}

/// A suspended account, answered with when and why, so the page can say so.
fn suspended(ban: &Ban) -> Response {
    let error = format!("this account is suspended {}: {}", until_text(ban), ban.reason);
    (StatusCode::FORBIDDEN, Json(json!({ "error": error, "banned": ban_json(ban) }))).into_response()
}

/// Where email is on it is required, and confirmed before an account plays
/// or makes friends.
fn must_be_confirmed(state: &Fireplace, id: u64) -> Result<(), ApiError> {
    let confirmed =
        state.accounts.lock().unwrap().get(id).is_some_and(|a| a.email.as_ref().is_some_and(|e| e.verified));
    if state.mail.enabled() && !confirmed {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "confirm your email first: enter the code we emailed you on your account page, or add an address there",
        ));
    }
    Ok(())
}

/// Which account this request is from, or 401.
///
/// The token's signature and expiry are checked by `token`; the epoch is
/// checked here, because only the account knows what its epoch is now.
fn signed_in(state: &Fireplace, headers: &HeaderMap) -> Result<u64, ApiError> {
    let raw = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .ok_or_else(unauthorised)?;
    let claims = state.keys.verify(raw.trim(), token::now()).map_err(|_| unauthorised())?;
    if !claims.is_access() {
        return Err(ApiError::new(StatusCode::UNAUTHORIZED, "that token is for refreshing, not for asking"));
    }
    let accounts = state.accounts.lock().unwrap();
    match accounts.get(claims.sub) {
        Some(account)
            if account.token_epoch == claims.epoch
                && (claims.sid.is_empty() || accounts.has_session(claims.sub, &claims.sid)) =>
        {
            Ok(claims.sub)
        }
        _ => Err(unauthorised()),
    }
}

// --- open routes ------------------------------------------------------------

/// Unchanged, and deliberately text: the deploy guide curls it, and so does
/// anything watching the box.
async fn health() -> &'static str {
    "ok"
}

/// What this Fireplace offers, so a client shows "Forgot password?" and email
/// fields only where they mean something.
async fn features(State(state): State<Arc<Fireplace>>) -> Json<Value> {
    Json(json!({
        "email": state.mail.enabled(),
        "recovery_codes": true,
        "reset": "/reset",
        "privacy": "/privacy",
        "terms": "/terms",
        "min_age": state.config.operator.min_age,
        "email_required": state.mail.enabled(),
        "renames": accounts::RENAMES,
    }))
}

/// The public half of the signing key, so a Flame can verify a token offline
/// at the next milestone without ever calling here.
async fn public_key(State(state): State<Arc<Fireplace>>) -> Json<Value> {
    Json(json!({ "algorithm": "ed25519", "key": state.keys.public_hex() }))
}

#[derive(serde::Deserialize)]
struct SignIn {
    handle: String,
    password: String,
}

#[derive(serde::Deserialize)]
struct NewAccount {
    handle: String,
    password: String,
    /// `YYYY-MM-DD`. Checked against `min_age` and dropped: never stored or
    /// logged (GDPR Art. 5(1)(c) and 8). Defaulted so that a client that
    /// does not send it hears why, not a parse error.
    #[serde(default)]
    birthdate: String,
    /// The terms and privacy notice: must be true.
    #[serde(default)]
    accept_terms: bool,
    /// Optional: an email when the terms or privacy notice change.
    #[serde(default)]
    policy_emails: bool,
    /// Required where this Fireplace sends email, and confirmed by a link.
    #[serde(default)]
    email: String,
}

async fn create_account(
    State(state): State<Arc<Fireplace>>,
    Caller(from): Caller,
    headers: HeaderMap,
    Json(body): Json<NewAccount>,
) -> Result<Response, ApiError> {
    let now = token::now();
    let min_age = state.config.operator.min_age;
    match accounts::age_from_birthdate(&body.birthdate, now) {
        None => return Err(bad("enter your date of birth as YYYY-MM-DD")),
        Some(age) if age < u32::from(min_age) => {
            return Err(ApiError::new(
                StatusCode::FORBIDDEN,
                format!("you must be at least {min_age} to make an account here"),
            ));
        }
        Some(_) => {}
    }
    if !body.accept_terms {
        return Err(bad("accept the terms and the privacy notice to make an account"));
    }
    let address = body.email.trim().to_owned();
    if state.mail.enabled() && !crate::mail::address_is_usable(&address) {
        return Err(bad("an email address is needed to make an account here"));
    }
    let handle = body.handle.trim().to_owned();
    // Shape first, and the handle looked up before anything is hashed: a
    // typo must not cost an hour of the quota, and a name that is taken must
    // not cost 19 MiB of argon2.
    if !accounts::handle_is_usable(&handle) {
        return Err(Refused::BadHandle.into());
    }
    if !accounts::password_is_usable(&body.password) {
        return Err(bad("a password is 8 to 128 characters"));
    }
    if state.accounts.lock().unwrap().by_handle(&handle).is_some() {
        return Err(Refused::HandleTaken.into());
    }
    // Only a real attempt at a new account counts against the limit.
    if !state.limits.lock().unwrap().allow(&format!("new:{from}"), state.accounts_per_hour, limit::HOUR, now)
    {
        return Err(too_many("too many new accounts from here, try again later"));
    }
    if state.mail.enabled() && !state.may_email(&address, &from, now) {
        return Err(too_many(TOO_MANY_EMAILS));
    }
    let hashed = hash_password(body.password).await?;
    let (codes, hashes) = accounts::new_recovery_codes();
    let (id, epoch) = {
        let mut accounts = state.accounts.lock().unwrap();
        let id = accounts.create(&handle, hashed, now, state.max_accounts)?;
        accounts.set_recovery(id, hashes)?;
        accounts.set_policy_emails(id, body.policy_emails)?;
        // Where it was made from is its first known place.
        accounts.remember_place(id, &from, now, true);
        if state.mail.enabled() {
            accounts.set_email(id, Some(&address), now)?;
        }
        (id, accounts.get(id).expect("just made").token_epoch)
    };
    if state.mail.enabled() {
        send_confirmation(&state, id, &handle, epoch, &address, now);
    }
    tracing::info!(id, %handle, "a new account");
    // The codes are shown this once and never again: only their hashes stay.
    let mut answer = open_session(&state, id, &handle, epoch, &from, &headers, now);
    answer["recovery_codes"] = json!(codes);
    Ok((StatusCode::CREATED, Json(answer)).into_response())
}

async fn sign_in(
    State(state): State<Arc<Fireplace>>,
    Caller(from): Caller,
    headers: HeaderMap,
    Json(body): Json<SignIn>,
) -> Result<Response, ApiError> {
    let now = token::now();
    let handle = body.handle.trim().to_lowercase();
    {
        let mut limits = state.limits.lock().unwrap();
        let by_address = limits.allow(&format!("in:{from}"), limit::SIGN_INS, limit::SIGN_IN_WINDOW, now);
        // Counted per handle as well, so somebody spread across a hundred
        // addresses still cannot grind one account.
        let by_handle = limits.allow(&format!("in@{handle}"), limit::SIGN_INS, limit::SIGN_IN_WINDOW, now);
        if !by_address || !by_handle {
            return Err(too_many("too many sign-in tries, wait a few minutes"));
        }
    }
    // Copied out and the lock dropped: the hash below takes 19 MiB and a
    // good fraction of a millisecond, and holding the accounts lock across it
    // would stop every other request in the building.
    let found = {
        let accounts = state.accounts.lock().unwrap();
        accounts.by_handle(&handle).map(|a| (a.id, a.password.clone(), a.handle.clone(), a.token_epoch))
    };
    let (id, hashed, handle, epoch) = match found {
        Some(found) => found,
        // Still hash something, so a handle that does not exist takes as long
        // to refuse as a password that is wrong.
        None => (0, NOBODY.to_owned(), String::new(), 0),
    };
    if !check_password(hashed, body.password).await || id == 0 {
        // One message for both, so this is not a way to find out who has an
        // account here.
        return Err(ApiError::new(StatusCode::UNAUTHORIZED, "that handle and password do not go together"));
    }
    // Only told to whoever has the password: a ban is nobody else's business.
    let ban = state.accounts.lock().unwrap().get(id).and_then(|a| a.ban.clone()).filter(|b| b.active(now));
    if let Some(ban) = ban {
        // Whoever holds a suspended account's password is the person it was
        // suspended for: their address goes with it, until the same end, so
        // they cannot carry on under another account.
        let address = from.parse::<IpAddr>().ok().filter(|ip| !ip.is_loopback() && !ip.is_unspecified());
        if let Some(ip) = address {
            let mut bans = state.ip_bans.lock().unwrap();
            if bans.of(&ip.to_string(), now).is_none() {
                bans.set(
                    &ip.to_string(),
                    Ban {
                        until: ban.until,
                        reason: format!("signed in to the suspended account @{handle}: {}", ban.reason),
                        by: format!("automatic, for @{handle}"),
                        at: now,
                    },
                );
                tracing::info!(id, "blocked the address a suspended account signed in from");
            }
        }
        return Ok(suspended(&ban));
    }
    if let Some((address, new_place)) = needs_code(&state, id, &from, now) {
        if !state.limits.lock().unwrap().allow(&format!("code@{id}"), CODES_PER_HOUR, limit::HOUR, now) {
            return Err(too_many("that is a lot of codes: wait a while, then sign in again"));
        }
        if !state.may_email(&address, &from, now) {
            return Err(too_many(TOO_MANY_EMAILS));
        }
        let (ticket, code) = state.codes.lock().unwrap().issue("sign-in", id, &from, now);
        state.mail.send(&address, state.mail.sign_in_letter(&handle, &from, &code, new_place));
        tracing::info!(id, new_place, "sent a sign-in code");
        let error = if new_place {
            "you are signing in from somewhere new: enter the code we just emailed you"
        } else {
            "enter the code we just emailed you"
        };
        return Ok((
            StatusCode::FORBIDDEN,
            Json(json!({ "error": error, "code_needed": true, "ticket": ticket, "new_place": new_place })),
        )
            .into_response());
    }
    state.presence.lock().unwrap().sweep(now);
    tracing::info!(id, %handle, "signed in");
    Ok(Json(open_session(&state, id, &handle, epoch, &from, &headers, now)).into_response())
}

#[derive(serde::Deserialize)]
struct Refresh {
    refresh: String,
}

/// The token kept on the player's machine, traded for a fresh hour. This is
/// what means a password is typed on a floating keyboard once a month rather
/// than every evening.
async fn refresh(
    State(state): State<Arc<Fireplace>>,
    Caller(from): Caller,
    headers: HeaderMap,
    Json(body): Json<Refresh>,
) -> Result<Json<Value>, ApiError> {
    let now = token::now();
    let claims = state.keys.verify(body.refresh.trim(), now).map_err(|_| unauthorised())?;
    if !claims.is_refresh() {
        return Err(unauthorised());
    }
    let (handle, epoch) = {
        let mut accounts = state.accounts.lock().unwrap();
        let account = accounts.get(claims.sub).ok_or_else(unauthorised)?;
        if account.token_epoch != claims.epoch {
            return Err(unauthorised());
        }
        let found = (account.handle.clone(), account.token_epoch);
        // A session that was ended stays ended.
        if !claims.sid.is_empty() && !accounts.touch_session(claims.sub, &claims.sid, &from, now) {
            return Err(unauthorised());
        }
        found
    };
    if claims.sid.is_empty() {
        // From before sessions were kept: it becomes one now.
        return Ok(Json(open_session(&state, claims.sub, &handle, epoch, &from, &headers, now)));
    }
    Ok(Json(tokens_for(&state, claims.sub, &handle, epoch, &claims.sid, now)))
}

pub(crate) fn tokens_for(state: &Fireplace, id: u64, handle: &str, epoch: u64, sid: &str, now: u64) -> Value {
    json!({
        "id": id,
        "handle": handle,
        "access": state.keys.mint_in(id, handle, epoch, "access", sid, now),
        "refresh": state.keys.mint_in(id, handle, epoch, "refresh", sid, now),
        "expires_in": token::ACCESS_SECONDS,
    })
}

/// A new session for this sign-in, and its tokens.
fn open_session(state: &Fireplace, id: u64, handle: &str, epoch: u64, ip: &str, headers: &HeaderMap, now: u64) -> Value {
    let sid = state.accounts.lock().unwrap().start_session(id, &client_of(headers), ip, now);
    tokens_for(state, id, handle, epoch, &sid, now)
}

/// What a device calls itself, made short: "Hearth game", "Firefox on Windows".
fn client_of(headers: &HeaderMap) -> String {
    let agent = headers.get(axum::http::header::USER_AGENT).and_then(|v| v.to_str().ok()).unwrap_or("");
    if agent.contains("Godot") {
        return "Hearth game".into();
    }
    let browser = [("Edg/", "Edge"), ("OPR/", "Opera"), ("Firefox/", "Firefox"), ("Chrome/", "Chrome"), ("Safari/", "Safari")]
        .iter()
        .find(|(mark, _)| agent.contains(mark))
        .map(|(_, name)| *name);
    let system = [("Windows", "Windows"), ("Android", "Android"), ("iPhone", "iPhone"), ("iPad", "iPad"), ("Mac OS", "macOS"), ("Linux", "Linux")]
        .iter()
        .find(|(mark, _)| agent.contains(mark))
        .map(|(_, name)| *name);
    match (browser, system) {
        (Some(b), Some(s)) => format!("{b} on {s}"),
        (Some(b), None) => b.to_owned(),
        (None, Some(s)) => s.to_owned(),
        (None, None) => "Unknown device".into(),
    }
}

// --- signed in --------------------------------------------------------------

/// Also how a client finds out whether the token it had saved is still good.
async fn me(State(state): State<Arc<Fireplace>>, headers: HeaderMap) -> Result<Json<Value>, ApiError> {
    let id = signed_in(&state, &headers)?;
    let accounts = state.accounts.lock().unwrap();
    let account = accounts.get(id).ok_or_else(unauthorised)?;
    let renames = accounts.renames_left(id, token::now());
    Ok(Json(json!({
        "id": account.id,
        "handle": account.handle,
        "display_name": account.display(),
        "share_location": account.share_location,
        "friends": account.friends.len(),
        "email": account.email.as_ref().map(|e| &e.address),
        "email_verified": account.email.as_ref().is_some_and(|e| e.verified),
        "recovery_codes_left": account.recovery.len(),
        "policy_emails": account.policy_emails,
        "avatar": account.avatar,
        "admin": account.admin,
        "two_factor": account.two_factor,
        "renames_left": renames.0,
        "next_rename": renames.1,
        "places": account.seen,
    })))
}

/// Everything stored about the account, for GDPR access and portability.
/// The hashes are left out: they are not information about anybody.
async fn export(State(state): State<Arc<Fireplace>>, headers: HeaderMap) -> Result<Json<Value>, ApiError> {
    let id = signed_in(&state, &headers)?;
    let now = token::now();
    let accounts = state.accounts.lock().unwrap();
    let account = accounts.get(id).ok_or_else(unauthorised)?;
    let people = |ids: &Vec<u64>| -> Vec<Value> {
        ids.iter()
            .filter_map(|id| accounts.get(*id))
            .map(|a| json!({ "id": a.id, "handle": a.handle }))
            .collect()
    };
    let seen = state.presence.lock().unwrap().of(id, now).cloned();
    Ok(Json(json!({
        "id": account.id,
        "handle": account.handle,
        "display_name": account.display_name,
        "created": account.created,
        "token_epoch": account.token_epoch,
        "share_location": account.share_location,
        "friends": people(&account.friends),
        "incoming": people(&account.incoming),
        "outgoing": people(&account.outgoing),
        "email": account.email,
        "recovery_codes_left": account.recovery.len(),
        "terms_accepted": account.terms_accepted,
        "policy_emails": account.policy_emails,
        "places_signed_in_from": account.seen,
        "sessions": account.sessions,
        "display_name_changes": account.renamed,
        "picture": (account.avatar > 0).then(|| format!("/avatars/{}.jpg", account.id)),
        "admin": account.admin,
        "two_factor": account.two_factor,
        "ban": account.ban,
        "password": "stored only as an argon2id hash, which is not included",
        "presence_in_memory": seen.map(|w| json!({
            "flame": w.flame, "ember": w.ember, "ember_name": w.ember_name, "world": w.world, "at": w.at,
        })),
    })))
}

#[derive(serde::Deserialize)]
struct SetEmail {
    password: String,
    /// Empty removes it.
    email: String,
}

/// Add, change or remove the address. The password as well as a token, as
/// for changing the password: the address is how a password gets reset, so
/// a token left on somebody else's machine must not be enough to take it.
async fn set_email(
    State(state): State<Arc<Fireplace>>,
    Caller(from): Caller,
    headers: HeaderMap,
    Json(body): Json<SetEmail>,
) -> Result<Json<Value>, ApiError> {
    let id = signed_in(&state, &headers)?;
    if !state.mail.enabled() {
        return Err(ApiError::new(StatusCode::NOT_FOUND, "this Fireplace does not send email"));
    }
    let address = body.email.trim();
    if address.is_empty() {
        return Err(bad("an email is needed here: you can change it, but not remove it"));
    }
    if !crate::mail::address_is_usable(address) {
        return Err(bad("that is not an email address"));
    }
    let (hashed, handle) = password_of(&state, id)?;
    if !check_password(hashed, body.password).await {
        return Err(ApiError::new(StatusCode::UNAUTHORIZED, "that is not your password"));
    }
    let now = token::now();
    if !state.may_email(address, &from, now) {
        return Err(too_many(TOO_MANY_EMAILS));
    }
    let (old, epoch) = {
        let mut accounts = state.accounts.lock().unwrap();
        let old = accounts.set_email(id, (!address.is_empty()).then_some(address), now)?;
        (old, accounts.get(id).map_or(0, |a| a.token_epoch))
    };
    // The old address hears about it, not the new one: that is the one an
    // account thief would want kept quiet.
    if let Some(old) = old.filter(|old| !old.eq_ignore_ascii_case(address) && state.may_email(old, "", now)) {
        state.mail.send(&old, state.mail.email_changed_letter(&handle));
    }
    if !address.is_empty() {
        send_confirmation(&state, id, &handle, epoch, address, now);
    }
    tracing::info!(id, "changed their email");
    Ok(Json(json!({ "email": (!address.is_empty()).then_some(address), "email_verified": false })))
}

/// A fresh set of eight, and the old ones stop working. The password too:
/// these reset the password, so they are worth as much as it is.
async fn new_recovery_codes(
    State(state): State<Arc<Fireplace>>,
    headers: HeaderMap,
    Json(body): Json<Confirm>,
) -> Result<Json<Value>, ApiError> {
    let id = signed_in(&state, &headers)?;
    let (hashed, _) = password_of(&state, id)?;
    if !check_password(hashed, body.password).await {
        return Err(ApiError::new(StatusCode::UNAUTHORIZED, "that is not your password"));
    }
    let (codes, hashes) = accounts::new_recovery_codes();
    state.accounts.lock().unwrap().set_recovery(id, hashes)?;
    Ok(Json(json!({ "recovery_codes": codes })))
}

#[derive(serde::Deserialize)]
struct Sharing {
    share_location: Option<bool>,
    policy_emails: Option<bool>,
}

async fn settings(
    State(state): State<Arc<Fireplace>>,
    headers: HeaderMap,
    Json(body): Json<Sharing>,
) -> Result<Json<Value>, ApiError> {
    let id = signed_in(&state, &headers)?;
    let mut accounts = state.accounts.lock().unwrap();
    if let Some(on) = body.policy_emails {
        accounts.set_policy_emails(id, on)?;
    }
    if let Some(on) = body.share_location {
        accounts.set_sharing(id, on)?;
        // Off means off: what was already reported stops being visible at
        // once, rather than lingering until its heartbeat runs out.
        if !on {
            state.presence.lock().unwrap().clear(id);
        }
    }
    let account = accounts.get(id).ok_or_else(unauthorised)?;
    Ok(Json(json!({ "share_location": account.share_location, "policy_emails": account.policy_emails })))
}

/// Friends, with where they are, and the requests waiting on either side.
///
/// This is the only thing that ever reads presence, and it only ever walks
/// the asker's own friends list -- so "friends only" is the shape of the code
/// rather than a check somebody could forget.
async fn friends(State(state): State<Arc<Fireplace>>, headers: HeaderMap) -> Result<Json<Value>, ApiError> {
    let id = signed_in(&state, &headers)?;
    let now = token::now();
    let accounts = state.accounts.lock().unwrap();
    let presence = state.presence.lock().unwrap();
    let mine = accounts.get(id).ok_or_else(unauthorised)?;

    let mut friends = Vec::new();
    for friend in &mine.friends {
        let Some(account) = accounts.get(*friend) else {
            continue;
        };
        let seen = account.share_location.then(|| presence.of(account.id, now)).flatten();
        friends.push(json!({
            "id": account.id,
            "handle": account.handle,
            "display_name": account.display(),
            "avatar": account.avatar,
            "online": seen.is_some(),
            "flame": seen.map(|w| w.flame.clone()),
            "ember": seen.map(|w| w.ember),
            "ember_name": seen.map(|w| w.ember_name.clone()),
            "world": seen.map(|w| w.world.clone()),
        }));
    }
    let names = |ids: &Vec<u64>| -> Vec<Value> {
        ids.iter()
            .filter_map(|id| accounts.get(*id))
            .map(|a| json!({ "id": a.id, "handle": a.handle, "display_name": a.display(), "avatar": a.avatar }))
            .collect()
    };
    Ok(Json(json!({
        "friends": friends,
        "incoming": names(&mine.incoming),
        "outgoing": names(&mine.outgoing),
    })))
}

#[derive(serde::Deserialize)]
struct DisplayName {
    display_name: String,
}

async fn set_name(
    State(state): State<Arc<Fireplace>>,
    headers: HeaderMap,
    Json(body): Json<DisplayName>,
) -> Result<Json<Value>, ApiError> {
    let id = signed_in(&state, &headers)?;
    let now = token::now();
    let mut accounts = state.accounts.lock().unwrap();
    let same = accounts.get(id).is_some_and(|a| a.display_name == body.display_name.trim());
    if !same {
        if let (0, next) = accounts.renames_left(id, now) {
            return Err(too_many(&format!(
                "a display name can change {} times in 30 days; the next change is free from {}",
                accounts::RENAMES,
                accounts::when(next.unwrap_or(now))
            )));
        }
        accounts.set_display_name(id, &body.display_name)?;
        accounts.count_rename(id, now);
    }
    Ok(Json(json!({
        "display_name": accounts.get(id).map(|a| a.display()),
        "renames_left": accounts.renames_left(id, now).0,
    })))
}

#[derive(serde::Deserialize)]
struct NewPassword {
    password: String,
    new_password: String,
}

/// The old password as well as a token: a token left on somebody else's
/// machine should not be enough to lock the owner out.
async fn set_password(
    State(state): State<Arc<Fireplace>>,
    Caller(from): Caller,
    headers: HeaderMap,
    Json(body): Json<NewPassword>,
) -> Result<Json<Value>, ApiError> {
    let id = signed_in(&state, &headers)?;
    if !accounts::password_is_usable(&body.new_password) {
        return Err(bad("a password is 8 to 128 characters"));
    }
    let (hashed, handle) = password_of(&state, id)?;
    if !check_password(hashed, body.password).await {
        return Err(ApiError::new(StatusCode::UNAUTHORIZED, "that is not your current password"));
    }
    let new = hash_password(body.new_password).await?;
    if !crate::pages::change_password(&state, id, None, new) {
        return Err(unauthorised());
    }
    let epoch = state.accounts.lock().unwrap().get(id).ok_or_else(unauthorised)?.token_epoch;
    tracing::info!(id, "changed their password");
    // Every other sign-in just ended; this one carries on with fresh tokens.
    Ok(Json(open_session(&state, id, &handle, epoch, &from, &headers, token::now())))
}

#[derive(serde::Deserialize)]
struct Confirm {
    password: String,
}

async fn delete_account(
    State(state): State<Arc<Fireplace>>,
    headers: HeaderMap,
    Json(body): Json<Confirm>,
) -> Result<StatusCode, ApiError> {
    let id = signed_in(&state, &headers)?;
    let (hashed, handle) = password_of(&state, id)?;
    if !check_password(hashed, body.password).await {
        return Err(ApiError::new(StatusCode::UNAUTHORIZED, "that is not your password"));
    }
    state.accounts.lock().unwrap().delete(id)?;
    state.presence.lock().unwrap().clear(id);
    avatar::remove(&state.config.data_dir, id);
    tracing::info!(id, %handle, "deleted their account");
    Ok(StatusCode::NO_CONTENT)
}

/// Copied out so the lock is not held across the hash (D9).
fn password_of(state: &Fireplace, id: u64) -> Result<(String, String), ApiError> {
    let accounts = state.accounts.lock().unwrap();
    let account = accounts.get(id).ok_or_else(unauthorised)?;
    Ok((account.password.clone(), account.handle.clone()))
}

#[derive(serde::Deserialize)]
struct Join {
    /// The SHA-256 of the Flame's certificate, as the client saw it.
    flame: String,
}

/// A ticket into one Flame (`docs/TRUST.md`). The client asks after the TLS
/// handshake, naming the certificate it was shown, so the ticket is good on
/// that Flame and nowhere else, and tells it a display name and an id that
/// only means something there.
async fn join(
    State(state): State<Arc<Fireplace>>,
    headers: HeaderMap,
    Json(body): Json<Join>,
) -> Result<Json<Value>, ApiError> {
    let id = signed_in(&state, &headers)?;
    must_be_confirmed(&state, id)?;
    let flame = body.flame.trim();
    if flame.len() != 64 || !flame.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) {
        return Err(bad("that is not a certificate hash"));
    }
    let name = {
        let accounts = state.accounts.lock().unwrap();
        accounts.get(id).ok_or_else(unauthorised)?.display().to_owned()
    };
    Ok(Json(json!({
        "ticket": state.keys.mint_join(id, &name, flame, token::now()),
        "expires_in": token::JOIN_SECONDS,
    })))
}

#[derive(serde::Deserialize)]
struct ByHandle {
    handle: String,
}

async fn add_friend(
    State(state): State<Arc<Fireplace>>,
    headers: HeaderMap,
    Json(body): Json<ByHandle>,
) -> Result<Json<Value>, ApiError> {
    let id = signed_in(&state, &headers)?;
    must_be_confirmed(&state, id)?;
    if !state.limits.lock().unwrap().allow(
        &format!("ask:{id}"),
        limit::REQUESTS_PER_HOUR,
        limit::HOUR,
        token::now(),
    ) {
        return Err(too_many("that is a lot of friend requests, try again later"));
    }
    let mut accounts = state.accounts.lock().unwrap();
    let them = accounts.by_handle(body.handle.trim()).map(|a| a.id).ok_or(Refused::NoSuchAccount)?;
    // True when they had already asked us: adding each other at the same
    // time makes you friends, which is what anybody would expect.
    let now_friends = accounts.request(id, them)?;
    Ok(Json(json!({ "state": if now_friends { "friends" } else { "requested" } })))
}

async fn accept_friend(
    State(state): State<Arc<Fireplace>>,
    headers: HeaderMap,
    Json(body): Json<ByHandle>,
) -> Result<Json<Value>, ApiError> {
    let id = signed_in(&state, &headers)?;
    must_be_confirmed(&state, id)?;
    let mut accounts = state.accounts.lock().unwrap();
    let them = accounts.by_handle(body.handle.trim()).map(|a| a.id).ok_or(Refused::NoSuchAccount)?;
    accounts.accept(id, them)?;
    Ok(Json(json!({ "state": "friends" })))
}

async fn remove_friend(
    State(state): State<Arc<Fireplace>>,
    headers: HeaderMap,
    Json(body): Json<ByHandle>,
) -> Result<StatusCode, ApiError> {
    let id = signed_in(&state, &headers)?;
    let mut accounts = state.accounts.lock().unwrap();
    let them = accounts.by_handle(body.handle.trim()).map(|a| a.id).ok_or(Refused::NoSuchAccount)?;
    accounts.remove(id, them)?;
    Ok(StatusCode::NO_CONTENT)
}

/// Where I am. An empty body, or no flame, means "nowhere" -- which is what
/// a client sends when it leaves a Flame.
#[derive(serde::Deserialize, Default)]
#[serde(default)]
struct Report {
    flame: String,
    ember: u32,
    ember_name: String,
    world: String,
}

async fn report_presence(
    State(state): State<Arc<Fireplace>>,
    headers: HeaderMap,
    Json(body): Json<Report>,
) -> Result<StatusCode, ApiError> {
    let id = signed_in(&state, &headers)?;
    if body.flame.trim().is_empty() {
        state.presence.lock().unwrap().clear(id);
        return Ok(StatusCode::NO_CONTENT);
    }
    // Checked, because this goes into a friend's client and from there into
    // `connect_to_server`. The worst case is small; it is still a boundary.
    let flame = body.flame.trim();
    if !presence::usable_flame(flame) {
        return Err(bad("that is not an address"));
    }
    if !body.world.is_empty() && hearth_protocol::validate_slug(&body.world).is_err() {
        return Err(bad("that is not a world"));
    }
    if !body.ember_name.is_empty() && hearth_protocol::validate_name(&body.ember_name).is_err() {
        return Err(bad("that is not an ember name"));
    }
    if !state.accounts.lock().unwrap().get(id).is_some_and(|a| a.share_location) {
        // Sharing is off: say yes and remember nothing. The client does not
        // need to know, and its heartbeat is harmless.
        return Ok(StatusCode::NO_CONTENT);
    }
    state.presence.lock().unwrap().report(
        id,
        Where {
            flame: flame.to_owned(),
            ember: body.ember,
            ember_name: body.ember_name,
            world: body.world,
            at: token::now(),
        },
    );
    Ok(StatusCode::NO_CONTENT)
}

// --- passwords --------------------------------------------------------------

/// argon2id costs 19 MiB and real time **on purpose**. That is exactly why it
/// goes to a blocking thread: a handful of people signing in at once would
/// otherwise stall every other request, the health check included.
pub(crate) async fn hash_password(password: String) -> Result<String, ApiError> {
    tokio::task::spawn_blocking(move || {
        // The salt comes from the operating system; argon2 generates it.
        let hashed: PasswordHash = Argon2::default().hash_password(password.as_bytes()).map_err(|_| ())?;
        Ok::<String, ()>(hashed.to_string())
    })
    .await
    .map_err(|_| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, "could not make that account"))?
    .map_err(|_| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, "could not make that account"))
}

pub(crate) async fn check_password(hashed: String, password: String) -> bool {
    tokio::task::spawn_blocking(move || match PasswordHash::new(&hashed) {
        Ok(parsed) => Argon2::default().verify_password(password.as_bytes(), &parsed).is_ok(),
        Err(_) => false,
    })
    .await
    .unwrap_or(false)
}

// --- pictures -----------------------------------------------------------------

/// The body is the picture itself. It is decoded and redrawn small on a
/// blocking thread (`avatar::shrink`); only that copy is kept.
async fn upload_avatar(
    State(state): State<Arc<Fireplace>>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Result<Json<Value>, ApiError> {
    let id = signed_in(&state, &headers)?;
    must_be_confirmed(&state, id)?;
    let now = token::now();
    if !state.limits.lock().unwrap().allow(&format!("avatar@{id}"), avatar::PER_HOUR, limit::HOUR, now) {
        return Err(too_many("that is a lot of pictures: try again in an hour"));
    }
    let jpeg = tokio::task::spawn_blocking(move || avatar::shrink(&body))
        .await
        .map_err(|_| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, "that did not work, try again"))?
        .map_err(|err| bad(&err.to_string()))?;
    avatar::save(&state.config.data_dir, id, &jpeg).map_err(|err| {
        tracing::error!(id, "could not save a picture: {err:#}");
        ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, "could not save that picture")
    })?;
    state.accounts.lock().unwrap().set_avatar(id, now)?;
    Ok(Json(json!({ "avatar": now })))
}

async fn delete_avatar(State(state): State<Arc<Fireplace>>, headers: HeaderMap) -> Result<Json<Value>, ApiError> {
    let id = signed_in(&state, &headers)?;
    avatar::remove(&state.config.data_dir, id);
    state.accounts.lock().unwrap().set_avatar(id, 0)?;
    Ok(Json(json!({ "avatar": 0 })))
}

/// `/avatars/<id>.jpg`. Only ever a JPEG this server drew, served so that a
/// browser cannot take it for anything else.
async fn avatar_file(State(state): State<Arc<Fireplace>>, Path(file): Path<String>) -> Response {
    let id = file.strip_suffix(".jpg").and_then(|id| id.parse::<u64>().ok());
    let has = id.is_some_and(|id| state.accounts.lock().unwrap().get(id).is_some_and(|a| a.avatar > 0));
    let (Some(id), true) = (id, has) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    match tokio::fs::read(avatar::path(&state.config.data_dir, id)).await {
        Ok(bytes) => (
            [
                (axum::http::header::CONTENT_TYPE, "image/jpeg"),
                (axum::http::header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
                (axum::http::header::CONTENT_SECURITY_POLICY, "default-src 'none'"),
                // The page asks for ?v=<when it was set>, so a new one is a new URL.
                (axum::http::header::CACHE_CONTROL, "public, max-age=604800"),
            ],
            bytes,
        )
            .into_response(),
        Err(_) => StatusCode::NOT_FOUND.into_response(),
    }
}

// --- admins -------------------------------------------------------------------
//
// The first admin is made with `fireplace admin make-admin <handle>` on the
// server itself; after that, admins make admins. Every action is logged with
// who did it to whom -- by account number and handle, never an address.

/// The admin asking, or 403.
fn admin_of(state: &Fireplace, headers: &HeaderMap) -> Result<(u64, String), ApiError> {
    let id = signed_in(state, headers)?;
    let accounts = state.accounts.lock().unwrap();
    match accounts.get(id) {
        Some(account) if account.admin => Ok((id, account.handle.clone())),
        _ => Err(ApiError::new(StatusCode::FORBIDDEN, "that is for admins")),
    }
}

/// Another account, by handle. An admin acts on others; their own account
/// goes through the ordinary pages, with their password.
fn target(state: &Fireplace, me: u64, handle: &str) -> Result<u64, ApiError> {
    let id = state.accounts.lock().unwrap().by_handle(handle.trim()).map(|a| a.id).ok_or(Refused::NoSuchAccount)?;
    if id == me {
        return Err(bad("not on your own account"));
    }
    Ok(id)
}

#[derive(serde::Deserialize)]
struct Claim {
    link: String,
}

/// The one-time door: a link only the server's own key can sign, from the
/// server's own machine, and only while there is no admin at all.
async fn admin_claim(
    State(state): State<Arc<Fireplace>>,
    Caller(from): Caller,
    Json(body): Json<Claim>,
) -> Result<Json<Value>, ApiError> {
    if !from.parse::<IpAddr>().is_ok_and(|ip| ip.is_loopback()) {
        return Err(ApiError::new(StatusCode::FORBIDDEN, "only from the server itself"));
    }
    let claims = state
        .keys
        .verify_link(body.link.trim(), "admin", token::now())
        .map_err(|_| ApiError::new(StatusCode::FORBIDDEN, "that link is not good"))?;
    let mut accounts = state.accounts.lock().unwrap();
    if accounts.any_admin() {
        return Err(ApiError::new(StatusCode::CONFLICT, "there is already an admin"));
    }
    let account = accounts.get(claims.sub).filter(|a| a.token_epoch == claims.epoch).ok_or(Refused::NoSuchAccount)?;
    let handle = account.handle.clone();
    accounts.set_admin(claims.sub, true)?;
    tracing::info!(id = claims.sub, %handle, "is the first admin");
    Ok(Json(json!({ "admin": handle })))
}

/// Every account, with everything kept about it, sign-in addresses included.
async fn admin_accounts(State(state): State<Arc<Fireplace>>, headers: HeaderMap) -> Result<Json<Value>, ApiError> {
    admin_of(&state, &headers)?;
    let now = token::now();
    let accounts = state.accounts.lock().unwrap();
    let mut list: Vec<&accounts::Account> = accounts.all().collect();
    list.sort_by_key(|a| a.id);
    Ok(Json(json!(list
        .into_iter()
        .map(|a| json!({
            "id": a.id,
            "handle": a.handle,
            "display_name": a.display(),
            "created": a.created,
            "email": a.email.as_ref().map(|e| &e.address),
            "email_verified": a.email.as_ref().is_some_and(|e| e.verified),
            "admin": a.admin,
            "avatar": a.avatar,
            "friends": a.friends.len(),
            "ban": a.ban.as_ref().filter(|b| b.active(now)).map(ban_json),
            "places": a.seen,
        }))
        .collect::<Vec<_>>())))
}

async fn admin_bans(State(state): State<Arc<Fireplace>>, headers: HeaderMap) -> Result<Json<Value>, ApiError> {
    admin_of(&state, &headers)?;
    let mut bans = state.ip_bans.lock().unwrap();
    Ok(Json(json!(bans
        .list(token::now())
        .iter()
        .map(|(ip, ban)| json!({ "ip": ip, "by": ban.by, "ban": ban_json(ban) }))
        .collect::<Vec<_>>())))
}

#[derive(serde::Deserialize)]
struct BanBody {
    #[serde(default)]
    handle: String,
    #[serde(default)]
    ip: String,
    /// 0 is for good.
    #[serde(default)]
    minutes: u64,
    #[serde(default)]
    reason: String,
}

fn make_ban(body: &BanBody, by: &str, now: u64) -> Result<Ban, ApiError> {
    let reason = body.reason.trim();
    if reason.is_empty() || reason.len() > 300 || reason.chars().any(char::is_control) {
        return Err(bad("give a reason, up to 300 characters: the person banned is shown it"));
    }
    let until = if body.minutes == 0 { 0 } else { now.saturating_add(body.minutes.saturating_mul(60)) };
    Ok(Ban { until, reason: reason.to_owned(), by: by.to_owned(), at: now })
}

async fn admin_ban(
    State(state): State<Arc<Fireplace>>,
    headers: HeaderMap,
    Json(body): Json<BanBody>,
) -> Result<Json<Value>, ApiError> {
    let (me, by) = admin_of(&state, &headers)?;
    let id = target(&state, me, &body.handle)?;
    let ban = make_ban(&body, &by, token::now())?;
    let handle = {
        let mut accounts = state.accounts.lock().unwrap();
        if accounts.get(id).is_some_and(|a| a.admin) {
            return Err(bad("take their admin away first"));
        }
        accounts.set_ban(id, Some(ban.clone()))?;
        accounts.get(id).map(|a| a.handle.clone()).unwrap_or_default()
    };
    // Banned again (longer, shorter, for good): the addresses it already
    // blocked end when the new ban does.
    state.ip_bans.lock().unwrap().retime_by(&format!("automatic, for @{handle}"), ban.until);
    state.presence.lock().unwrap().clear(id);
    tracing::info!(admin = me, target = id, until = ban.until, "banned an account");
    Ok(Json(json!({ "ban": ban_json(&ban) })))
}

async fn admin_unban(
    State(state): State<Arc<Fireplace>>,
    headers: HeaderMap,
    Json(body): Json<ByHandle>,
) -> Result<Json<Value>, ApiError> {
    let (me, _) = admin_of(&state, &headers)?;
    let id = target(&state, me, &body.handle)?;
    let handle = {
        let mut accounts = state.accounts.lock().unwrap();
        accounts.set_ban(id, None)?;
        accounts.get(id).map(|a| a.handle.clone()).unwrap_or_default()
    };
    // The addresses blocked because this account was signed in to go too.
    let lifted = state.ip_bans.lock().unwrap().remove_by(&format!("automatic, for @{handle}"));
    tracing::info!(admin = me, target = id, lifted, "unbanned an account");
    Ok(Json(json!({ "ban": null, "addresses_unblocked": lifted })))
}

async fn admin_ban_ip(
    State(state): State<Arc<Fireplace>>,
    headers: HeaderMap,
    Caller(from): Caller,
    Json(body): Json<BanBody>,
) -> Result<Json<Value>, ApiError> {
    let (me, by) = admin_of(&state, &headers)?;
    let ip: IpAddr = body.ip.trim().parse().map_err(|_| bad("that is not an IP address"))?;
    if ip.is_loopback() || ip.is_unspecified() {
        return Err(bad("not this server's own address"));
    }
    if ip.to_string() == from {
        return Err(bad("that is the address you are using: you would lock yourself out"));
    }
    let ban = make_ban(&body, &by, token::now())?;
    state.ip_bans.lock().unwrap().set(&ip.to_string(), ban.clone());
    tracing::info!(admin = me, until = ban.until, "banned an address");
    Ok(Json(json!({ "ip": ip.to_string(), "ban": ban_json(&ban) })))
}

#[derive(serde::Deserialize)]
struct ByIp {
    ip: String,
}

async fn admin_unban_ip(
    State(state): State<Arc<Fireplace>>,
    headers: HeaderMap,
    Json(body): Json<ByIp>,
) -> Result<Json<Value>, ApiError> {
    let (me, _) = admin_of(&state, &headers)?;
    let ip = body.ip.trim().parse::<IpAddr>().map(|ip| ip.to_string()).unwrap_or_else(|_| body.ip.trim().to_owned());
    if !state.ip_bans.lock().unwrap().remove(&ip) {
        return Err(ApiError::new(StatusCode::NOT_FOUND, "that address is not banned"));
    }
    tracing::info!(admin = me, "unbanned an address");
    Ok(Json(json!({ "ip": ip })))
}

async fn admin_delete(
    State(state): State<Arc<Fireplace>>,
    headers: HeaderMap,
    Json(body): Json<ByHandle>,
) -> Result<StatusCode, ApiError> {
    let (me, _) = admin_of(&state, &headers)?;
    let id = target(&state, me, &body.handle)?;
    state.accounts.lock().unwrap().delete(id)?;
    state.presence.lock().unwrap().clear(id);
    avatar::remove(&state.config.data_dir, id);
    tracing::info!(admin = me, target = id, "deleted an account");
    Ok(StatusCode::NO_CONTENT)
}

#[derive(serde::Deserialize)]
struct SetAdmin {
    handle: String,
    admin: bool,
}

async fn admin_set_admin(
    State(state): State<Arc<Fireplace>>,
    headers: HeaderMap,
    Json(body): Json<SetAdmin>,
) -> Result<Json<Value>, ApiError> {
    let (me, _) = admin_of(&state, &headers)?;
    let id = target(&state, me, &body.handle)?;
    let mut accounts = state.accounts.lock().unwrap();
    if body.admin && accounts.get(id).is_some_and(|a| a.ban.is_some()) {
        return Err(bad("unban them first"));
    }
    accounts.set_admin(id, body.admin)?;
    tracing::info!(admin = me, target = id, on = body.admin, "changed who is an admin");
    Ok(Json(json!({ "admin": body.admin })))
}

async fn admin_sign_out(
    State(state): State<Arc<Fireplace>>,
    headers: HeaderMap,
    Json(body): Json<ByHandle>,
) -> Result<StatusCode, ApiError> {
    let (me, _) = admin_of(&state, &headers)?;
    let id = target(&state, me, &body.handle)?;
    state.accounts.lock().unwrap().sign_out_everywhere(id)?;
    tracing::info!(admin = me, target = id, "signed an account out everywhere");
    Ok(StatusCode::NO_CONTENT)
}

// --- codes from email -----------------------------------------------------------

#[derive(serde::Deserialize)]
struct WithCode {
    #[serde(default)]
    ticket: String,
    code: String,
}

/// The second half of a sign-in that needed a code: from the same IP address
/// the code was sent for, and then that address is a known place.
async fn sign_in_with_code(
    State(state): State<Arc<Fireplace>>,
    Caller(from): Caller,
    headers: HeaderMap,
    Json(body): Json<WithCode>,
) -> Result<Json<Value>, ApiError> {
    let now = token::now();
    if !state.limits.lock().unwrap().allow(&format!("code:{from}"), limit::SIGN_INS, limit::SIGN_IN_WINDOW, now) {
        return Err(too_many("too many tries, wait a few minutes"));
    }
    let pending = state.codes.lock().unwrap().check(body.ticket.trim(), &body.code, now).map_err(wrong_code)?;
    if pending.kind != "sign-in" || pending.about != from {
        return Err(wrong_code(crate::codes::Wrong::Gone));
    }
    let (handle, epoch, banned) = {
        let mut accounts = state.accounts.lock().unwrap();
        accounts.remember_place(pending.account, &from, now, true);
        let account = accounts.get(pending.account).ok_or(Refused::NoSuchAccount)?;
        (account.handle.clone(), account.token_epoch, account.ban.as_ref().is_some_and(|b| b.active(now)))
    };
    if banned {
        return Err(ApiError::new(StatusCode::FORBIDDEN, "this account is suspended"));
    }
    tracing::info!(id = pending.account, "signed in with a code");
    Ok(Json(open_session(&state, pending.account, &handle, epoch, &from, &headers, now)))
}

/// The code from the confirmation email, typed on the account page.
async fn confirm_email(
    State(state): State<Arc<Fireplace>>,
    headers: HeaderMap,
    Json(body): Json<WithCode>,
) -> Result<Json<Value>, ApiError> {
    let id = signed_in(&state, &headers)?;
    let pending =
        state.codes.lock().unwrap().check_account("verify", id, &body.code, token::now()).map_err(wrong_code)?;
    if !state.accounts.lock().unwrap().verify_email(id, &pending.about) {
        return Err(wrong_code(crate::codes::Wrong::Gone));
    }
    tracing::info!(id, "confirmed their email with a code");
    Ok(Json(json!({ "email_verified": true })))
}

async fn resend_confirmation(
    State(state): State<Arc<Fireplace>>,
    Caller(from): Caller,
    headers: HeaderMap,
) -> Result<StatusCode, ApiError> {
    let id = signed_in(&state, &headers)?;
    let now = token::now();
    let (handle, epoch, email) = {
        let accounts = state.accounts.lock().unwrap();
        let account = accounts.get(id).ok_or_else(unauthorised)?;
        (account.handle.clone(), account.token_epoch, account.email.clone())
    };
    let Some(email) = email.filter(|e| !e.verified) else {
        return Err(bad("there is no address waiting to be confirmed"));
    };
    if !state.limits.lock().unwrap().allow(&format!("verify@{id}"), 3, limit::HOUR, now) {
        return Err(too_many("three a hour is plenty: check your spam folder"));
    }
    if !state.may_email(&email.address, &from, now) {
        return Err(too_many(TOO_MANY_EMAILS));
    }
    send_confirmation(&state, id, &handle, epoch, &email.address, now);
    Ok(StatusCode::NO_CONTENT)
}

#[derive(serde::Deserialize)]
struct TwoFactor {
    password: String,
    on: bool,
}

/// Two-step sign-in: a code by email every time, not just from new places.
async fn set_two_factor(
    State(state): State<Arc<Fireplace>>,
    headers: HeaderMap,
    Json(body): Json<TwoFactor>,
) -> Result<Json<Value>, ApiError> {
    let id = signed_in(&state, &headers)?;
    if !state.mail.enabled() {
        return Err(ApiError::new(StatusCode::NOT_FOUND, "this Fireplace does not send email"));
    }
    let (hashed, _) = password_of(&state, id)?;
    if !check_password(hashed, body.password).await {
        return Err(ApiError::new(StatusCode::UNAUTHORIZED, "that is not your password"));
    }
    let mut accounts = state.accounts.lock().unwrap();
    let confirmed = accounts.get(id).is_some_and(|a| a.email.as_ref().is_some_and(|e| e.verified));
    if body.on && !confirmed {
        return Err(bad("confirm your email first: the codes go there"));
    }
    accounts.set_two_factor(id, body.on)?;
    tracing::info!(id, on = body.on, "changed two-step sign-in");
    Ok(Json(json!({ "two_factor": body.on })))
}

// --- sessions -------------------------------------------------------------------

/// The session this request's token belongs to.
fn current_session(state: &Fireplace, headers: &HeaderMap) -> Result<(u64, String), ApiError> {
    let id = signed_in(state, headers)?;
    let raw = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .unwrap_or("");
    let sid = state.keys.verify(raw.trim(), token::now()).map(|c| c.sid).unwrap_or_default();
    Ok((id, sid))
}

/// Everywhere this account is signed in, this one marked.
async fn my_sessions(State(state): State<Arc<Fireplace>>, headers: HeaderMap) -> Result<Json<Value>, ApiError> {
    let (id, sid) = current_session(&state, &headers)?;
    let accounts = state.accounts.lock().unwrap();
    let mut sessions = accounts.get(id).ok_or_else(unauthorised)?.sessions.clone();
    sessions.sort_by_key(|s| std::cmp::Reverse(s.last));
    Ok(Json(json!(sessions
        .iter()
        .map(|s| json!({
            "id": s.id, "client": s.client, "ip": s.ip, "started": s.started, "last": s.last,
            "current": s.id == sid,
        }))
        .collect::<Vec<_>>())))
}

#[derive(serde::Deserialize)]
struct EndSession {
    /// One session; empty, with `others`, every session but this one.
    #[serde(default)]
    id: String,
    #[serde(default)]
    others: bool,
}

/// Signs one session out, or every other one. Its tokens stop working at once.
async fn end_session(
    State(state): State<Arc<Fireplace>>,
    headers: HeaderMap,
    Json(body): Json<EndSession>,
) -> Result<Json<Value>, ApiError> {
    let (id, sid) = current_session(&state, &headers)?;
    let ended = if body.others {
        state.accounts.lock().unwrap().end_sessions(id, None, Some(&sid))
    } else {
        state.accounts.lock().unwrap().end_sessions(id, Some(body.id.trim()), None)
    };
    tracing::info!(id, ended, "ended sessions");
    Ok(Json(json!({ "ended": ended })))
}
