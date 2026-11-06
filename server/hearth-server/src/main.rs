use std::time::Duration;

use anyhow::Context;
use hearth_server::Config;
use tracing::{info, warn};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .with_ansi(std::io::IsTerminal::is_terminal(&std::io::stdout()))
        .init();

    let path = std::env::args().nth(1).unwrap_or_else(|| "hearth.toml".into());
    let config = match std::fs::read_to_string(&path) {
        Ok(text) => Config::parse(&text).with_context(|| format!("invalid config {path}"))?,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            warn!(%path, "config file not found, using defaults");
            Config::default()
        }
        Err(err) => return Err(err).with_context(|| format!("reading {path}")),
    };

    let (cert, key) = hearth_server::load_or_create_cert(&config.data_dir)?;
    let endpoint = hearth_server::endpoint(config.port, cert, key)?;
    info!(addr = %endpoint.local_addr()?, max_players = config.max_players, "hearth server listening");

    let server = tokio::spawn(hearth_server::serve(endpoint.clone(), config.max_players));
    shutdown_signal().await;

    info!("shutting down");
    endpoint.close(0u8.into(), b"server shutting down");
    let _ = tokio::time::timeout(Duration::from_secs(3), endpoint.wait_idle()).await;
    let _ = server.await;
    info!("stopped");
    Ok(())
}

async fn shutdown_signal() {
    #[cfg(unix)]
    {
        let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()).expect("SIGTERM handler");
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = term.recv() => {}
        }
    }
    #[cfg(not(unix))]
    tokio::signal::ctrl_c().await.expect("Ctrl-C handler");
}
