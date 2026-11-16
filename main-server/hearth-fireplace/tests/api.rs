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
use hearth_fireplace::{Config, Fireplace, api, limit};
use serde_json::{Value, json};
use tower::ServiceExt;

/// A Fireplace of its own per test: they run in parallel, and sharing one
/// directory means each deleting the others out from under them.
fn fireplace(who: &str) -> (Router, PathBuf) {
    let dir = std::env::temp_dir().join(format!("hearth-fireplace-api-{}-{who}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let config = Config { data_dir: dir.clone(), ..Config::default() };
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
