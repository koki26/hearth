//! The routes, driven through the real router with no socket in the way.
//!
//! `tower::ServiceExt::oneshot` hands one request to the `Router` and gives
//! back the response, so these are end-to-end through the handlers, the
//! extractors and the error mapping, and still run in milliseconds.

use std::path::PathBuf;
use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use hearth_fireplace::{Config, Fireplace, api, limit, mail, pages, token};
use serde_json::{Value, json};
use tower::ServiceExt;

/// A Fireplace of its own per test: they run in parallel, and sharing one
/// directory means each deleting the others out from under them.
fn fireplace(who: &str) -> (Router, PathBuf) {
    let dir = std::env::temp_dir().join(format!("hearth-fireplace-api-{}-{who}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let web_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../web");
    let config = Config { data_dir: dir.clone(), web_dir, ..Config::default() };
    let state = Arc::new(Fireplace::open(&config).unwrap());
    (api::router(state), dir)
}

async fn call(
    app: &Router,
    method: &str,
    path: &str,
    token: Option<&str>,
    body: Value,
) -> (StatusCode, Value) {
    let mut request = Request::builder().method(method).uri(path).header("content-type", "application/json");
    if let Some(token) = token {
        request = request.header("authorization", format!("Bearer {token}"));
    }
    let body = if body.is_null() { Body::empty() } else { Body::from(serde_json::to_vec(&body).unwrap()) };
    let response = app.clone().oneshot(request.body(body).unwrap()).await.unwrap();
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
    (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
}

/// Makes an account and returns its access token.
async fn join(app: &Router, handle: &str) -> String {
    let (status, body) =
        call(app, "POST", "/accounts", None, json!({"handle": handle, "password": "a long one"})).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    body["access"].as_str().unwrap().to_owned()
}

#[tokio::test]
async fn the_health_check_still_answers_ok() {
    let (app, _dir) = fireplace("health");
    let response =
        app.clone().oneshot(Request::builder().uri("/health").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
    assert_eq!(&bytes[..], b"ok", "the deploy guide curls this, and so does anything watching the box");

    // And the public key is there for a Flame to hold.
    let (status, body) = call(&app, "GET", "/key", None, Value::Null).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["algorithm"], "ed25519");
    assert_eq!(body["key"].as_str().unwrap().len(), 64);
}

#[tokio::test]
async fn an_account_is_made_signed_in_to_and_refused_twice() {
    let (app, _dir) = fireplace("accounts");
    let (status, made) =
        call(&app, "POST", "/accounts", None, json!({"handle": "Ada", "password": "a long one"})).await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(made["handle"], "Ada");
    assert!(made["access"].as_str().unwrap().starts_with("v1."));
    assert!(made["refresh"].as_str().unwrap().starts_with("v1."));

    // The same handle, in any case, is taken.
    let (status, _) =
        call(&app, "POST", "/accounts", None, json!({"handle": "ada", "password": "another one"})).await;
    assert_eq!(status, StatusCode::CONFLICT);
    // And nonsense is refused before anything is hashed.
    for bad in
        [json!({"handle": "a", "password": "a long one"}), json!({"handle": "ada2", "password": "short"})]
    {
        let (status, _) = call(&app, "POST", "/accounts", None, bad).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    let (status, back) =
        call(&app, "POST", "/sessions", None, json!({"handle": "ADA", "password": "a long one"})).await;
    assert_eq!(status, StatusCode::OK, "signing in does not care about capitals");
    assert_eq!(back["id"], made["id"]);

    let (status, me) = call(&app, "GET", "/me", Some(back["access"].as_str().unwrap()), Value::Null).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!((me["handle"].as_str(), me["share_location"].as_bool()), (Some("Ada"), Some(true)));
}

#[tokio::test]
async fn signing_in_with_the_wrong_password_says_the_same_as_an_unknown_handle() {
    let (app, _dir) = fireplace("oracle");
    join(&app, "ada").await;
    let (wrong, said) =
        call(&app, "POST", "/sessions", None, json!({"handle": "ada", "password": "not it at all"})).await;
    let (missing, also) =
        call(&app, "POST", "/sessions", None, json!({"handle": "nobody", "password": "not it at all"})).await;
    assert_eq!(wrong, StatusCode::UNAUTHORIZED);
    assert_eq!(missing, StatusCode::UNAUTHORIZED);
    // Whether somebody has an account here is not ours to hand out.
    assert_eq!(said, also);
}

#[tokio::test]
async fn a_saved_token_buys_a_fresh_hour_and_an_access_token_does_not() {
    let (app, _dir) = fireplace("refresh");
    let (_, made) =
        call(&app, "POST", "/accounts", None, json!({"handle": "ada", "password": "a long one"})).await;
    let (status, fresh) =
        call(&app, "POST", "/sessions/refresh", None, json!({"refresh": made["refresh"]})).await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = call(&app, "GET", "/me", Some(fresh["access"].as_str().unwrap()), Value::Null).await;
    assert_eq!(status, StatusCode::OK, "the new access token works");

    // The two kinds are not each other: an access token cannot be refreshed
    // with, and a refresh token cannot be asked with.
    let (status, _) = call(&app, "POST", "/sessions/refresh", None, json!({"refresh": made["access"]})).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, _) = call(&app, "GET", "/me", Some(made["refresh"].as_str().unwrap()), Value::Null).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn everything_but_health_key_accounts_and_sessions_needs_a_token() {
    let (app, _dir) = fireplace("locked");
    let token = join(&app, "ada").await;
    for (method, path, body) in [
        ("GET", "/me", Value::Null),
        ("GET", "/friends", Value::Null),
        ("POST", "/friends/add", json!({"handle": "bob"})),
        ("POST", "/friends/accept", json!({"handle": "bob"})),
        ("POST", "/friends/remove", json!({"handle": "bob"})),
        ("POST", "/presence", json!({"flame": "1.2.3.4:7777"})),
        ("POST", "/settings", json!({"share_location": false})),
    ] {
        let (status, _) = call(&app, method, path, None, body.clone()).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{method} {path} with no token");
        // A token with a character changed is not a token either.
        let bent = format!("{}x", &token[..token.len() - 1]);
        let (status, _) = call(&app, method, path, Some(&bent), body).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{method} {path} with a bent token");
    }
}

#[tokio::test]
async fn a_friend_sees_where_you_are_and_a_stranger_does_not() {
    let (app, _dir) = fireplace("presence");
    let ada = join(&app, "ada").await;
    let bob = join(&app, "bob").await;
    let cat = join(&app, "cat").await;

    // Ada and Bob are friends; Cat is nobody to either of them.
    call(&app, "POST", "/friends/add", Some(&ada), json!({"handle": "bob"})).await;
    let (status, state) = call(&app, "POST", "/friends/accept", Some(&bob), json!({"handle": "ada"})).await;
    assert_eq!((status, state["state"].as_str()), (StatusCode::OK, Some("friends")));

    let (status, _) = call(
        &app,
        "POST",
        "/presence",
        Some(&ada),
        json!({"flame": "192.168.1.20:7777", "ember": 3, "ember_name": "the loft", "world": "forest"}),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (_, seen) = call(&app, "GET", "/friends", Some(&bob), Value::Null).await;
    let friend = &seen["friends"][0];
    assert_eq!(friend["handle"], "ada");
    assert_eq!(friend["online"], true);
    assert_eq!(friend["flame"], "192.168.1.20:7777");
    assert_eq!(friend["ember"], 3);
    assert_eq!(friend["ember_name"], "the loft");

    // Cat is not a friend, so Cat has no friends and learns nothing.
    let (_, nothing) = call(&app, "GET", "/friends", Some(&cat), Value::Null).await;
    assert_eq!(nothing["friends"].as_array().unwrap().len(), 0);

    // Leaving clears it.
    call(&app, "POST", "/presence", Some(&ada), json!({})).await;
    let (_, gone) = call(&app, "GET", "/friends", Some(&bob), Value::Null).await;
    assert_eq!(gone["friends"][0]["online"], false);
    assert_eq!(gone["friends"][0]["flame"], Value::Null);
}

#[tokio::test]
async fn turning_off_sharing_hides_where_you_are() {
    let (app, _dir) = fireplace("sharing");
    let ada = join(&app, "ada").await;
    let bob = join(&app, "bob").await;
    call(&app, "POST", "/friends/add", Some(&ada), json!({"handle": "bob"})).await;
    call(&app, "POST", "/friends/accept", Some(&bob), json!({"handle": "ada"})).await;
    call(&app, "POST", "/presence", Some(&ada), json!({"flame": "1.2.3.4:7777", "ember": 0})).await;

    let (status, _) = call(&app, "POST", "/settings", Some(&ada), json!({"share_location": false})).await;
    assert_eq!(status, StatusCode::OK);
    let (_, hidden) = call(&app, "GET", "/friends", Some(&bob), Value::Null).await;
    assert_eq!(hidden["friends"][0]["online"], false, "off at once, not when the heartbeat runs out");

    // And a heartbeat while it is off is accepted and remembered nowhere.
    call(&app, "POST", "/presence", Some(&ada), json!({"flame": "1.2.3.4:7777", "ember": 0})).await;
    let (_, still) = call(&app, "GET", "/friends", Some(&bob), Value::Null).await;
    assert_eq!(still["friends"][0]["online"], false);
}

#[tokio::test]
async fn a_presence_we_would_not_hand_to_a_friend_is_refused() {
    let (app, _dir) = fireplace("nasty-presence");
    let ada = join(&app, "ada").await;
    for bad in [
        json!({"flame": "has a space:7777"}),
        json!({"flame": "line\nbreak:7777"}),
        json!({"flame": "1.2.3.4:7777", "world": "../../etc"}),
        json!({"flame": "1.2.3.4:7777", "ember_name": "bell\u{7}"}),
    ] {
        let (status, _) = call(&app, "POST", "/presence", Some(&ada), bad.clone()).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{bad}");
    }
}

#[tokio::test]
async fn friends_come_and_go_from_either_side() {
    let (app, _dir) = fireplace("friends");
    let ada = join(&app, "ada").await;
    let bob = join(&app, "bob").await;

    let (status, state) = call(&app, "POST", "/friends/add", Some(&ada), json!({"handle": "bob"})).await;
    assert_eq!((status, state["state"].as_str()), (StatusCode::OK, Some("requested")));
    let (_, waiting) = call(&app, "GET", "/friends", Some(&bob), Value::Null).await;
    assert_eq!(waiting["incoming"][0]["handle"], "ada");
    let (_, mine) = call(&app, "GET", "/friends", Some(&ada), Value::Null).await;
    assert_eq!(mine["outgoing"][0]["handle"], "bob");

    // Bob adds Ada back rather than pressing accept: same thing.
    let (status, state) = call(&app, "POST", "/friends/add", Some(&bob), json!({"handle": "ada"})).await;
    assert_eq!((status, state["state"].as_str()), (StatusCode::OK, Some("friends")));

    // Either of them can undo it.
    let (status, _) = call(&app, "POST", "/friends/remove", Some(&bob), json!({"handle": "ada"})).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (_, after) = call(&app, "GET", "/friends", Some(&ada), Value::Null).await;
    assert_eq!(after["friends"].as_array().unwrap().len(), 0);

    // Somebody who is not here, and yourself.
    let (status, _) = call(&app, "POST", "/friends/add", Some(&ada), json!({"handle": "nobody"})).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = call(&app, "POST", "/friends/add", Some(&ada), json!({"handle": "ada"})).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn too_many_sign_in_tries_are_refused() {
    let (app, _dir) = fireplace("grinding");
    join(&app, "ada").await;
    let guess = json!({"handle": "ada", "password": "not it at all"});
    for _ in 0..limit::SIGN_INS {
        let (status, _) = call(&app, "POST", "/sessions", None, guess.clone()).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }
    let (status, _) = call(&app, "POST", "/sessions", None, guess).await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    // Even the right password waits, which is the point.
    let (status, _) =
        call(&app, "POST", "/sessions", None, json!({"handle": "ada", "password": "a long one"})).await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
}

#[tokio::test]
async fn everybody_is_still_here_after_a_restart() {
    let (app, dir) = fireplace("restart");
    let ada = join(&app, "ada").await;
    let bob = join(&app, "bob").await;
    call(&app, "POST", "/friends/add", Some(&ada), json!({"handle": "bob"})).await;
    call(&app, "POST", "/friends/accept", Some(&bob), json!({"handle": "ada"})).await;
    drop(app);

    // The same directory, a fresh server: the same accounts, the same
    // friendship, and the token Ada was holding still works -- because the
    // signing key was kept too.
    let config = Config { data_dir: dir, ..Config::default() };
    let app = api::router(Arc::new(Fireplace::open(&config).unwrap()));
    let (status, mine) = call(&app, "GET", "/friends", Some(&ada), Value::Null).await;
    assert_eq!(status, StatusCode::OK, "a restart does not sign anybody out");
    assert_eq!(mine["friends"][0]["handle"], "bob");
    assert_eq!(mine["friends"][0]["online"], false, "and presence does not survive it");
}

#[tokio::test]
async fn a_join_ticket_carries_the_display_name_and_needs_a_certificate_hash() {
    let (app, _dir) = fireplace("join");
    let ada = join(&app, "ada").await;
    let flame = "ab".repeat(32);

    let (status, _) = call(&app, "POST", "/join", None, json!({"flame": flame})).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "a ticket is for somebody signed in");
    for bad in ["", "not hex", &"AB".repeat(32), &"ab".repeat(31)] {
        let (status, _) = call(&app, "POST", "/join", Some(&ada), json!({"flame": bad})).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{bad:?}");
    }

    let (status, body) = call(&app, "POST", "/me/name", Some(&ada), json!({"display_name": "Ada L"})).await;
    assert_eq!((status, body["display_name"].as_str()), (StatusCode::OK, Some("Ada L")));
    let (status, body) = call(&app, "POST", "/join", Some(&ada), json!({"flame": flame})).await;
    assert_eq!(status, StatusCode::OK);
    let ticket = body["ticket"].as_str().unwrap();
    assert!(ticket.starts_with("v1.") && ticket.len() <= hearth_protocol::MAX_TICKET_LEN);
    // A ticket opens nothing here: it is for a Flame, not for the Fireplace.
    let (status, _) = call(&app, "GET", "/me", Some(ticket), Value::Null).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn a_new_password_signs_out_everywhere_and_deleting_needs_the_password() {
    let (app, _dir) = fireplace("password-delete");
    let ada = join(&app, "ada").await;
    let bob = join(&app, "bob").await;
    call(&app, "POST", "/friends/add", Some(&bob), json!({"handle": "ada"})).await;

    let (status, _) = call(
        &app,
        "POST",
        "/me/password",
        Some(&ada),
        json!({"password": "wrong one!", "new_password": "a new long one"}),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, fresh) = call(
        &app,
        "POST",
        "/me/password",
        Some(&ada),
        json!({"password": "a long one", "new_password": "a new long one"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = call(&app, "GET", "/me", Some(&ada), Value::Null).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "the old token died with the old password");
    let ada = fresh["access"].as_str().unwrap().to_owned();

    let (status, _) = call(&app, "POST", "/me/delete", Some(&ada), json!({"password": "a long one"})).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, _) =
        call(&app, "POST", "/me/delete", Some(&ada), json!({"password": "a new long one"})).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _) = call(&app, "GET", "/me", Some(&ada), Value::Null).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (_, friends) = call(&app, "GET", "/friends", Some(&bob), Value::Null).await;
    assert_eq!(friends["outgoing"], json!([]), "nobody is left waiting on an account that is gone");
}

#[tokio::test]
async fn the_account_page_is_served() {
    let (app, _dir) = fireplace("page");
    let response =
        app.clone().oneshot(Request::builder().uri("/").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
    assert!(String::from_utf8_lossy(&bytes).contains("<title>Hearth Account</title>"));

    // Its stylesheet and script come from the same folder...
    for (path, kind) in [("/style.css", "text/css"), ("/app.js", "javascript")] {
        let response =
            app.clone().oneshot(Request::builder().uri(path).body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK, "{path}");
        let served = response.headers()["content-type"].to_str().unwrap().to_owned();
        assert!(served.contains(kind), "{path} came back as {served}");
    }
    // ...and nothing outside it does.
    let response = app
        .clone()
        .oneshot(Request::builder().uri("/../Cargo.toml").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_ne!(response.status(), StatusCode::OK);
    // The API still wins over a file of the same name.
    let (status, _) = call(&app, "GET", "/me", None, Value::Null).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

// --- email, resets and recovery codes (D63) ----------------------------------

const SITE: &str = "https://fireplace.test";

/// A Fireplace that sends email -- into a list, never over SMTP.
fn fireplace_with_mail(who: &str) -> (Router, Arc<Fireplace>) {
    let dir = std::env::temp_dir().join(format!("hearth-fireplace-api-{}-{who}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let mut state = Fireplace::open(&Config { data_dir: dir, ..Config::default() }).unwrap();
    state.mail = mail::Mailer::memory(SITE);
    let state = Arc::new(state);
    (api::router(state.clone()), state)
}

/// A browser form: what the reset pages are.
async fn form(app: &Router, path: &str, from: &str, fields: &[(&str, &str)]) -> (StatusCode, String) {
    let body: Vec<String> = fields
        .iter()
        .map(|(k, v)| {
            format!("{k}={}", v.replace('%', "%25").replace('@', "%40").replace('+', "%2B").replace(' ', "+"))
        })
        .collect();
    let request = Request::builder()
        .method("POST")
        .uri(path)
        .header("content-type", "application/x-www-form-urlencoded")
        .header("x-forwarded-for", from)
        .body(Body::from(body.join("&")))
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
    (status, String::from_utf8_lossy(&bytes).into_owned())
}

async fn get_page(app: &Router, path: &str) -> (StatusCode, String) {
    let response =
        app.clone().oneshot(Request::builder().uri(path).body(Body::empty()).unwrap()).await.unwrap();
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
    (status, String::from_utf8_lossy(&bytes).into_owned())
}

/// The path of the link in the last letter of this kind.
fn link_in(state: &Fireplace, kind: &str) -> String {
    let sent = state.mail.sent();
    let letter = &sent.iter().rev().find(|s| s.letter.kind == kind).expect("a letter of that kind").letter;
    let at = letter.body.find(SITE).unwrap() + SITE.len();
    letter.body[at..].split_whitespace().next().unwrap().to_owned()
}

fn letters(state: &Fireplace, kind: &str) -> usize {
    state.mail.sent().iter().filter(|s| s.letter.kind == kind).count()
}

/// Makes an account with a confirmed address and returns what making it said.
async fn join_with_email(app: &Router, state: &Fireplace, handle: &str, address: &str) -> Value {
    let (_, made) =
        call(app, "POST", "/accounts", None, json!({"handle": handle, "password": "a long one"})).await;
    let access = made["access"].as_str().unwrap();
    let (status, _) =
        call(app, "POST", "/me/email", Some(access), json!({"password": "a long one", "email": address}))
            .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = get_page(app, &link_in(state, "verify")).await;
    assert_eq!(status, StatusCode::OK);
    made
}

#[tokio::test]
async fn email_is_optional_and_a_server_without_it_says_so_cleanly() {
    let (app, _dir) = fireplace("no-mail");
    let (status, made) =
        call(&app, "POST", "/accounts", None, json!({"handle": "ada", "password": "a long one"})).await;
    assert_eq!(status, StatusCode::CREATED, "a handle and a password are still all it takes");
    assert_eq!(made["recovery_codes"].as_array().unwrap().len(), 8, "and everybody gets recovery codes");
    let access = made["access"].as_str().unwrap();
    let (_, me) = call(&app, "GET", "/me", Some(access), Value::Null).await;
    assert_eq!((me["email"].clone(), me["recovery_codes_left"].clone()), (Value::Null, json!(8)));

    let (status, features) = call(&app, "GET", "/features", None, Value::Null).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        (features["email"].as_bool(), features["recovery_codes"].as_bool()),
        (Some(false), Some(true))
    );
    assert_eq!(
        (features["privacy"].as_str(), features["terms"].as_str()),
        (Some("/privacy"), Some("/terms"))
    );

    let (status, said) =
        call(&app, "POST", "/me/email", Some(access), json!({"password": "a long one", "email": "a@b.org"}))
            .await;
    assert_eq!(
        (status, said["error"].as_str()),
        (StatusCode::NOT_FOUND, Some("this Fireplace does not send email"))
    );
    let (status, _) = form(&app, "/reset", "1.1.1.1", &[("email", "a@b.org")]).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, page) = get_page(&app, "/reset").await;
    assert!(status == StatusCode::OK && page.contains("recovery code") && !page.contains("type=\"email\""));
    let (_, privacy) = get_page(&app, "/privacy").await;
    assert!(privacy.contains("sends no email"));
}

#[tokio::test]
async fn an_address_must_be_confirmed_before_it_can_reset_anything() {
    let (app, state) = fireplace_with_mail("verify-first");
    let (_, made) =
        call(&app, "POST", "/accounts", None, json!({"handle": "ada", "password": "a long one"})).await;
    let access = made["access"].as_str().unwrap();
    for (password, email, want) in [
        ("wrong one!", "ada@example.org", StatusCode::UNAUTHORIZED),
        ("a long one", "not an address", StatusCode::BAD_REQUEST),
        ("a long one", "ada@example.org", StatusCode::OK),
    ] {
        let (status, _) =
            call(&app, "POST", "/me/email", Some(access), json!({"password": password, "email": email}))
                .await;
        assert_eq!(status, want, "{email}");
    }
    assert_eq!(state.mail.sent()[0].to, "ada@example.org", "the link goes to the new address");

    form(&app, "/reset", "1.1.1.1", &[("email", "ada@example.org")]).await;
    assert_eq!(letters(&state, "reset"), 0, "an address nobody confirmed resets nothing");

    let (status, _) = get_page(&app, &link_in(&state, "verify")).await;
    assert_eq!(status, StatusCode::OK);
    let (_, me) = call(&app, "GET", "/me", Some(access), Value::Null).await;
    assert_eq!(me["email_verified"], true);
    form(&app, "/reset", "1.1.1.1", &[("email", "ADA@example.org")]).await;
    assert_eq!(letters(&state, "reset"), 1, "now it does, however it is capitalised");
}

#[tokio::test]
async fn asking_for_a_reset_says_the_same_whether_or_not_the_address_is_known() {
    let (app, state) = fireplace_with_mail("enumeration");
    join_with_email(&app, &state, "ada", "ada@example.org").await;
    let known = form(&app, "/reset", "1.1.1.1", &[("email", "ada@example.org")]).await;
    let unknown = form(&app, "/reset", "1.1.1.1", &[("email", "nobody@example.org")]).await;
    assert_eq!(known, unknown, "whether somebody has an account here is not ours to hand out");
    assert_eq!(letters(&state, "reset"), 1);
}

#[tokio::test]
async fn a_reset_link_works_once_and_signs_out_everywhere() {
    let (app, state) = fireplace_with_mail("reset-once");
    let made = join_with_email(&app, &state, "ada", "ada@example.org").await;
    form(&app, "/reset", "1.1.1.1", &[("email", "ada@example.org")]).await;
    let link = link_in(&state, "reset");
    assert!(link.starts_with("/reset/v1."));

    let (status, page) = get_page(&app, &link).await;
    assert!(status == StatusCode::OK && page.contains("New password"));
    let (status, _) = form(&app, &link, "1.1.1.1", &[("password", "short")]).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "a bad password does not spend the link");
    let (status, _) = form(&app, &link, "1.1.1.1", &[("password", "a brand new one")]).await;
    assert_eq!(status, StatusCode::OK);

    let (status, _) = call(&app, "GET", "/me", Some(made["access"].as_str().unwrap()), Value::Null).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "the old sign-in is over");
    let (status, _) =
        call(&app, "POST", "/sessions/refresh", None, json!({"refresh": made["refresh"]})).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "and so is the one saved on the player's machine");
    let (status, _) =
        call(&app, "POST", "/sessions", None, json!({"handle": "ada", "password": "a brand new one"})).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(letters(&state, "password-changed"), 1, "and the confirmed address is told");

    let (status, _) = form(&app, &link, "1.1.1.1", &[("password", "yet another one")]).await;
    assert_eq!(status, StatusCode::GONE, "a link works once");
    assert_eq!(get_page(&app, &link).await.0, StatusCode::GONE);
}

#[tokio::test]
async fn a_reset_link_runs_out_and_nothing_else_passes_for_one() {
    let (app, state) = fireplace_with_mail("reset-expiry");
    let made = join_with_email(&app, &state, "ada", "ada@example.org").await;
    let id = made["id"].as_u64().unwrap();
    let old = state.keys.mint_link("reset", id, 1, "", token::now() - token::RESET_SECONDS - 1);
    assert_eq!(
        get_page(&app, &format!("/reset/{old}")).await.0,
        StatusCode::GONE,
        "half an hour is half an hour"
    );
    let fresh = state.keys.mint_link("reset", id, 1, "", token::now());
    assert_eq!(get_page(&app, &format!("/reset/{fresh}")).await.0, StatusCode::OK);

    // A confirmation link, a sign-in token and nonsense are not reset links,
    // and a reset link is not a sign-in.
    let verify = state.keys.mint_link("verify", id, 1, "ada@example.org", token::now());
    for wrong in [verify.as_str(), made["access"].as_str().unwrap(), "v1.nonsense.here"] {
        assert_eq!(get_page(&app, &format!("/reset/{wrong}")).await.0, StatusCode::GONE, "{wrong}");
    }
    let (status, _) = call(&app, "GET", "/me", Some(&fresh), Value::Null).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn reset_requests_are_limited_per_address_and_per_account() {
    let (app, state) = fireplace_with_mail("reset-limits");
    join_with_email(&app, &state, "ada", "ada@example.org").await;
    for _ in 0..pages::RESETS_PER_HOUR {
        assert_eq!(
            form(&app, "/reset", "6.6.6.6", &[("email", "nobody@example.org")]).await.0,
            StatusCode::OK
        );
    }
    let (status, _) = form(&app, "/reset", "6.6.6.6", &[("email", "ada@example.org")]).await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS, "one address cannot ask for ever");

    // Spread over many addresses, one account still gets only a few letters,
    // and the answer does not change when it stops.
    let first = form(&app, "/reset", "10.0.0.1", &[("email", "ada@example.org")]).await;
    for i in 2..=6 {
        let again = form(&app, "/reset", &format!("10.0.0.{i}"), &[("email", "ada@example.org")]).await;
        assert_eq!(again, first);
    }
    assert_eq!(letters(&state, "reset") as u32, pages::RESETS_PER_ACCOUNT);
}

#[tokio::test]
async fn a_recovery_code_works_once_and_a_new_set_replaces_the_old() {
    let (app, _dir) = fireplace("recovery");
    let (_, made) =
        call(&app, "POST", "/accounts", None, json!({"handle": "ada", "password": "a long one"})).await;
    let codes: Vec<String> =
        made["recovery_codes"].as_array().unwrap().iter().map(|c| c.as_str().unwrap().to_owned()).collect();
    let recover = |code: &str, password: &str| {
        let fields =
            [("handle", "ADA"), ("code", code), ("password", password)].map(|(k, v)| (k, v.to_owned()));
        let app = app.clone();
        async move {
            let fields: Vec<(&str, &str)> = fields.iter().map(|(k, v)| (*k, v.as_str())).collect();
            form(&app, "/recover", "1.1.1.1", &fields).await
        }
    };

    let (status, page) = recover(&codes[0].to_uppercase(), "a brand new one").await;
    assert_eq!(status, StatusCode::OK, "capitals do not matter: {page}");
    assert!(page.contains("7 recovery codes left"));
    assert_eq!(recover(&codes[0], "another new one").await.0, StatusCode::UNAUTHORIZED, "once only");
    let (status, _) = call(&app, "GET", "/me", Some(made["access"].as_str().unwrap()), Value::Null).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "a recovered account is signed out everywhere");

    let (_, back) =
        call(&app, "POST", "/sessions", None, json!({"handle": "ada", "password": "a brand new one"})).await;
    let access = back["access"].as_str().unwrap();
    let (status, _) =
        call(&app, "POST", "/me/recovery", Some(access), json!({"password": "wrong one!"})).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, fresh) =
        call(&app, "POST", "/me/recovery", Some(access), json!({"password": "a brand new one"})).await;
    assert_eq!((status, fresh["recovery_codes"].as_array().unwrap().len()), (StatusCode::OK, 8));
    assert_eq!(
        recover(&codes[1], "another new one").await.0,
        StatusCode::UNAUTHORIZED,
        "the old set is gone"
    );
    let (status, _) = recover(fresh["recovery_codes"][0].as_str().unwrap(), "another new one").await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn changing_or_removing_the_address_tells_the_old_one() {
    let (app, state) = fireplace_with_mail("email-change");
    let made = join_with_email(&app, &state, "ada", "old@example.org").await;
    let access = made["access"].as_str().unwrap();
    let change = json!({"password": "a long one", "email": "new@example.org"});
    call(&app, "POST", "/me/email", Some(access), change).await;
    let told = state.mail.sent().into_iter().find(|s| s.letter.kind == "email-changed").unwrap();
    assert_eq!(told.to, "old@example.org");
    let (_, me) = call(&app, "GET", "/me", Some(access), Value::Null).await;
    assert_eq!(
        (me["email"].as_str(), me["email_verified"].as_bool()),
        (Some("new@example.org"), Some(false))
    );

    // Removing an unconfirmed address tells nobody, and deletes it at once.
    let remove = json!({"password": "a long one", "email": ""});
    assert_eq!(call(&app, "POST", "/me/email", Some(access), remove).await.0, StatusCode::OK);
    assert_eq!(letters(&state, "email-changed"), 1);
    let (_, me) = call(&app, "GET", "/me", Some(access), Value::Null).await;
    assert_eq!(me["email"], Value::Null);
    // The confirmation link sent to it is worth nothing now.
    assert_eq!(get_page(&app, &link_in(&state, "verify")).await.0, StatusCode::GONE);
}

#[tokio::test]
async fn everything_stored_can_be_taken_away_and_then_deleted() {
    let (app, state) = fireplace_with_mail("export-delete");
    let made = join_with_email(&app, &state, "ada", "ada@example.org").await;
    let access = made["access"].as_str().unwrap();
    let (status, all) = call(&app, "GET", "/me/export", Some(access), Value::Null).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        (all["handle"].as_str(), all["email"]["address"].as_str()),
        (Some("ada"), Some("ada@example.org"))
    );
    assert_eq!(all["email"]["verified"], true);
    assert!(!all.to_string().contains("argon2id$"), "no hashes in an export");
    assert_eq!(call(&app, "GET", "/me/export", None, Value::Null).await.0, StatusCode::UNAUTHORIZED);

    let wrong = call(&app, "DELETE", "/me", Some(access), json!({"password": "wrong one!"})).await;
    assert_eq!(wrong.0, StatusCode::UNAUTHORIZED);
    let right = call(&app, "DELETE", "/me", Some(access), json!({"password": "a long one"})).await;
    assert_eq!(right.0, StatusCode::NO_CONTENT);
    let people = std::fs::read_to_string(state.config.data_dir.join("people.json")).unwrap();
    assert!(!people.contains("ada@example.org"), "the address is gone from disk, not hidden");
    form(&app, "/reset", "1.1.1.1", &[("email", "ada@example.org")]).await;
    assert_eq!(letters(&state, "reset"), 0);
}

#[test]
fn bad_mail_settings_stop_the_server_with_a_reason() {
    let dir = std::env::temp_dir().join(format!("hearth-fireplace-api-{}-mailconfig", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let secret = dir.join("smtp-password");
    std::fs::write(&secret, "hunter2-the-real-one\n").unwrap();
    let good = format!(
        r#"data_dir = '{}'
[operator]
name = "Ada"
contact_email = "privacy@example.org"
[mail]
backend = "smtp"
public_url = "https://fireplace.example.org"
from = "Hearth <h@example.org>"
[mail.smtp]
host = "mail.example.org"
port = 587
security = "starttls"
username = "h@example.org"
password_file = '{}'
"#,
        dir.display(),
        secret.display()
    );
    let refused = |text: &str| Fireplace::open(&Config::parse(text).unwrap()).err().map(|e| format!("{e:#}"));
    assert_eq!(refused(&good), None, "the good one starts");

    for (from, to, says) in [
        ("password_file = '", "password_file = '/nowhere/at/all' #'", "password_file"),
        ("port = 587\nsecurity = \"starttls\"", "port = 587\nsecurity = \"tls\"", "587"),
        ("port = 587\nsecurity = \"starttls\"", "port = 465\nsecurity = \"starttls\"", "465"),
        ("security = \"starttls\"", "security = \"none\"", "localhost"),
        ("https://fireplace", "http://fireplace", "https://"),
        ("name = \"Ada\"", "name = \"\"", "[operator]"),
        ("from = \"Hearth <h@example.org>\"", "from = \"not an address\"", "from"),
    ] {
        let said = refused(&good.replace(from, to)).unwrap_or_else(|| panic!("should refuse: {to}"));
        assert!(said.contains(says), "{to}: {said}");
        assert!(!said.contains("hunter2"), "the password is never printed");
    }
    assert!(Config::parse("[mail]\nbackend = \"carrier pigeon\"").is_err());
    // "none" to a server on this machine is allowed, and so is a local URL.
    let local = good
        .replace("mail.example.org", "localhost")
        .replace("\"starttls\"", "\"none\"")
        .replace("https://fireplace.example.org", "http://localhost:7800");
    assert_eq!(refused(&local), None);
}

/// Collects log output, for the test below.
#[derive(Clone, Default)]
struct Captured(Arc<std::sync::Mutex<Vec<u8>>>);

impl std::io::Write for Captured {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[tokio::test]
async fn no_address_password_or_token_ever_reaches_the_log() {
    let captured = Captured::default();
    let writer = captured.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::TRACE)
        .with_writer(move || writer.clone())
        .finish();
    // Global, not scoped: tracing caches "nobody is listening" per call site,
    // and the tests running beside this one would teach it that. It also
    // means everything they log is checked too.
    tracing::subscriber::set_global_default(subscriber).expect("the only test that sets one");

    let (app, state) = fireplace_with_mail("logs");
    let made = join_with_email(&app, &state, "ada", "secret.address@example.org").await;
    let access = made["access"].as_str().unwrap();
    form(&app, "/reset", "1.1.1.1", &[("email", "secret.address@example.org")]).await;
    let link = link_in(&state, "reset");
    form(&app, &link, "1.1.1.1", &[("password", "the second secret")]).await;
    let code = made["recovery_codes"][0].as_str().unwrap();
    form(&app, "/recover", "1.1.1.1", &[("handle", "ada"), ("code", code), ("password", "the third secret")])
        .await;
    call(&app, "POST", "/sessions", None, json!({"handle": "ada", "password": "a wrong secret"})).await;
    call(&app, "GET", "/me/export", Some(access), Value::Null).await;

    let log = String::from_utf8(captured.0.lock().unwrap().clone()).unwrap();
    assert!(log.contains("a new account"), "the log was captured at all: {log}");
    for secret in [
        "secret.address",
        "a long one",
        "the second secret",
        "the third secret",
        "a wrong secret",
        code,
        access,
        made["refresh"].as_str().unwrap(),
        &link["/reset/".len()..],
    ] {
        assert!(!log.contains(secret), "{secret:?} is in the log:\n{log}");
    }
}
