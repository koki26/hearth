//! Starts the Fireplace. What it is and why is in `lib.rs` and
//! `docs/TRUST.md`; how to run one is `docs/RUNNING.md`.

use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::{Context, Result};
use hearth_fireplace::{Config, Fireplace, api};
use tracing::info;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env().add_directive("info".parse()?))
        .init();

    let path = std::env::args().nth(1).unwrap_or_else(|| "fireplace.toml".into());
    if path == "--help" || path == "-h" {
        println!("fireplace [fireplace.toml]");
        return Ok(());
    }
    let config = match std::fs::read_to_string(&path) {
        Ok(text) => Config::parse(&text).with_context(|| format!("in {path}"))?,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            info!("no {path}, using defaults");
            Config::default()
        }
        Err(err) => return Err(err).with_context(|| format!("could not read {path}")),
    };

    let state = Arc::new(Fireplace::open(&config)?);
    let accounts = state.accounts.lock().unwrap().len();
    let addr = SocketAddr::from((config.bind, config.port));
    let listener =
        tokio::net::TcpListener::bind(addr).await.with_context(|| format!("could not listen on {addr}"))?;
    info!(%addr, accounts, key = %state.keys.public_hex(), "the Fireplace is lit");
    if !config.bind.is_loopback() {
        tracing::warn!(
            "listening beyond this machine: the Fireplace speaks plain HTTP and belongs behind a TLS terminator"
        );
    }

    // `ConnectInfo` so rate limiting knows who is asking -- see api::Caller,
    // which needs the peer to decide whether to believe X-Forwarded-For.
    axum::serve(listener, api::router(state).into_make_service_with_connect_info::<SocketAddr>())
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("the Fireplace stopped badly")?;
    info!("the Fireplace is out");
    Ok(())
}

/// Ctrl-C at a console, or SIGTERM from a service manager. Both mean stop,
/// and a service stopped with the default SIGTERM handling never gets to
/// finish what it was writing.
async fn shutdown_signal() {
    #[cfg(unix)]
    {
        let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("SIGTERM handler");
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = term.recv() => {}
        }
    }
    #[cfg(not(unix))]
    tokio::signal::ctrl_c().await.expect("Ctrl-C handler");
}
