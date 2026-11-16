//! The routes. JSON in, JSON out, one error shape.
//!
//! Everything a signed-in player does carries `Authorization: Bearer <token>`
//! and nothing else: there is no session table to keep, which is what makes a
//! restart invisible to everybody who is signed in.
//!
//! Only POST and GET. Godot's `HTTPRequest` is happiest that way, and REST
//! purity is not worth a body on a DELETE.

use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;

use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier};
use axum::extract::{ConnectInfo, DefaultBodyLimit, FromRequestParts, State};
use axum::http::{HeaderMap, StatusCode, request::Parts};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::{Value, json};

use crate::accounts::{self, Refused};
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
        .route("/", get(page))
        .route("/health", get(health))
        .route("/key", get(public_key))
        .route("/accounts", post(create_account))
        .route("/sessions", post(sign_in))
        .route("/sessions/refresh", post(refresh))
        .route("/me", get(me))
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
        .layer(tower_http::timeout::TimeoutLayer::with_status_code(
            StatusCode::REQUEST_TIMEOUT,
            std::time::Duration::from_secs(TIMEOUT_SECONDS),
        ))
        .layer(DefaultBodyLimit::max(MAX_BODY))
        .with_state(state)
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
        let peer = parts.extensions.get::<ConnectInfo<SocketAddr>>().map(|info| info.0.ip());
        let behind_proxy = peer.is_none_or(|ip| ip.is_loopback());
        let forwarded = parts
            .headers
            .get("x-forwarded-for")
            .and_then(|value| value.to_str().ok())
            .and_then(|list| list.rsplit(',').next())
            .map(str::trim)
            .filter(|one| !one.is_empty() && one.parse::<IpAddr>().is_ok());
        Ok(Self(match (behind_proxy, forwarded) {
            (true, Some(real)) => real.to_owned(),
            _ => peer.map(|ip| ip.to_string()).unwrap_or_else(|| "unknown".into()),
        }))
    }
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
        Some(account) if account.token_epoch == claims.epoch => Ok(claims.sub),
        _ => Err(unauthorised()),
    }
}

// --- open routes ------------------------------------------------------------

/// The account page: sign in, friends, display name, password, delete. One
/// file with no build step, calling the same JSON routes the game does.
async fn page() -> axum::response::Html<&'static str> {
    axum::response::Html(include_str!("page.html"))
}

/// Unchanged, and deliberately text: the deploy guide curls it, and so does
/// anything watching the box.
async fn health() -> &'static str {
    "ok"
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

async fn create_account(
    State(state): State<Arc<Fireplace>>,
    Caller(from): Caller,
    Json(body): Json<SignIn>,
) -> Result<Response, ApiError> {
    let now = token::now();
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
    let hashed = hash_password(body.password).await?;
    let (id, epoch) = {
        let mut accounts = state.accounts.lock().unwrap();
        let id = accounts.create(&handle, hashed, now, state.max_accounts)?;
        (id, accounts.get(id).expect("just made").token_epoch)
    };
    tracing::info!(id, %handle, "a new account");
    Ok((StatusCode::CREATED, Json(tokens_for(&state, id, &handle, epoch, now))).into_response())
}

async fn sign_in(
    State(state): State<Arc<Fireplace>>,
    Caller(from): Caller,
    Json(body): Json<SignIn>,
) -> Result<Json<Value>, ApiError> {
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
    state.presence.lock().unwrap().sweep(now);
    tracing::info!(id, %handle, "signed in");
    Ok(Json(tokens_for(&state, id, &handle, epoch, now)))
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
    Json(body): Json<Refresh>,
) -> Result<Json<Value>, ApiError> {
    let now = token::now();
    let claims = state.keys.verify(body.refresh.trim(), now).map_err(|_| unauthorised())?;
    if !claims.is_refresh() {
        return Err(unauthorised());
    }
    let (handle, epoch) = {
        let accounts = state.accounts.lock().unwrap();
        let account = accounts.get(claims.sub).ok_or_else(unauthorised)?;
        if account.token_epoch != claims.epoch {
            return Err(unauthorised());
        }
        (account.handle.clone(), account.token_epoch)
    };
    Ok(Json(tokens_for(&state, claims.sub, &handle, epoch, now)))
}

fn tokens_for(state: &Fireplace, id: u64, handle: &str, epoch: u64, now: u64) -> Value {
    json!({
        "id": id,
        "handle": handle,
        "access": state.keys.mint(id, handle, epoch, "access", now),
        "refresh": state.keys.mint(id, handle, epoch, "refresh", now),
        "expires_in": token::ACCESS_SECONDS,
    })
}

// --- signed in --------------------------------------------------------------

/// Also how a client finds out whether the token it had saved is still good.
async fn me(State(state): State<Arc<Fireplace>>, headers: HeaderMap) -> Result<Json<Value>, ApiError> {
    let id = signed_in(&state, &headers)?;
    let accounts = state.accounts.lock().unwrap();
    let account = accounts.get(id).ok_or_else(unauthorised)?;
    Ok(Json(json!({
        "id": account.id,
        "handle": account.handle,
        "display_name": account.display(),
        "share_location": account.share_location,
        "friends": account.friends.len(),
    })))
}

#[derive(serde::Deserialize)]
struct Sharing {
    share_location: bool,
}

async fn settings(
    State(state): State<Arc<Fireplace>>,
    headers: HeaderMap,
    Json(body): Json<Sharing>,
) -> Result<Json<Value>, ApiError> {
    let id = signed_in(&state, &headers)?;
    state.accounts.lock().unwrap().set_sharing(id, body.share_location)?;
    // Off means off: what was already reported stops being visible at once,
    // rather than lingering until its heartbeat runs out.
    if !body.share_location {
        state.presence.lock().unwrap().clear(id);
    }
    Ok(Json(json!({ "share_location": body.share_location })))
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
        let Some(account) = accounts.get(*friend) else { continue };
        let seen = account.share_location.then(|| presence.of(account.id, now)).flatten();
        friends.push(json!({
            "id": account.id,
            "handle": account.handle,
            "display_name": account.display(),
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
            .map(|a| json!({ "id": a.id, "handle": a.handle, "display_name": a.display() }))
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
    let mut accounts = state.accounts.lock().unwrap();
    accounts.set_display_name(id, &body.display_name)?;
    Ok(Json(json!({ "display_name": accounts.get(id).map(|a| a.display()) })))
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
    let epoch = state.accounts.lock().unwrap().set_password(id, new)?;
    tracing::info!(id, "changed their password");
    // Every other sign-in just ended; this one carries on with fresh tokens.
    Ok(Json(tokens_for(&state, id, &handle, epoch, token::now())))
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
async fn hash_password(password: String) -> Result<String, ApiError> {
    tokio::task::spawn_blocking(move || {
        // The salt comes from the operating system; argon2 generates it.
        let hashed: PasswordHash = Argon2::default().hash_password(password.as_bytes()).map_err(|_| ())?;
        Ok::<String, ()>(hashed.to_string())
    })
    .await
    .map_err(|_| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, "could not make that account"))?
    .map_err(|_| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, "could not make that account"))
}

async fn check_password(hashed: String, password: String) -> bool {
    tokio::task::spawn_blocking(move || match PasswordHash::new(&hashed) {
        Ok(parsed) => Argon2::default().verify_password(password.as_bytes(), &parsed).is_ok(),
        Err(_) => false,
    })
    .await
    .unwrap_or(false)
}
