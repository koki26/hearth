use std::io::{BufRead, IsTerminal, Write};
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use hearth_flame::{Config, Server, command};
use tracing::{info, warn};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .with_ansi(std::io::stdout().is_terminal())
        .init();

    let path = std::env::args().nth(1).unwrap_or_else(|| "flame.toml".into());
    if path == "--help" || path == "-h" {
        println!("flame [flame.toml]\n\nCommands once it is running:\n{}", command::HELP);
        return Ok(());
    }
    let config = match std::fs::read_to_string(&path) {
        Ok(text) => Config::parse(&text).with_context(|| format!("invalid config {path}"))?,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            warn!(%path, "config file not found, using defaults");
            Config::default()
        }
        Err(err) => return Err(err).with_context(|| format!("reading {path}")),
    };

    let (cert, key) = hearth_flame::load_or_create_cert(&config.data_dir)?;
    let endpoint = hearth_flame::endpoint(config.port, cert, key)?;
    info!(
        addr = %endpoint.local_addr()?,
        max_players = config.max_players,
        worlds = %config.worlds_dir().display(),
        hub = %config.hub_world,
        "the Flame is lit"
    );

    let state = Server::new(&config, tokio::sync::watch::channel(Vec::new()).0);
    let server = tokio::spawn(hearth_flame::serve_with(endpoint.clone(), state.clone()));
    let quit = prompt(state);
    tokio::select! {
        _ = shutdown_signal() => {}
        _ = quit.notified() => {} // somebody typed `quit`
    }

    info!("shutting down");
    endpoint.close(0u8.into(), b"server shutting down");
    let _ = tokio::time::timeout(Duration::from_secs(3), endpoint.wait_idle()).await;
    server.abort();
    info!("stopped");
    Ok(())
}

/// The console server is operated by typing at it: the same commands the
/// window offers. Stdin is read on its own thread, because every command is a
/// plain call and blocking reads have no business in the runtime.
fn prompt(server: Arc<Server>) -> Arc<tokio::sync::Notify> {
    let quit = Arc::new(tokio::sync::Notify::new());
    let typed_quit = quit.clone();
    std::thread::spawn(move || {
        let interactive = std::io::stdin().is_terminal();
        if interactive {
            println!("Type `help` for the commands.");
        }
        for line in std::io::stdin().lock().lines() {
            let Ok(line) = line else { break };
            if command::is_quit(&line) {
                typed_quit.notify_one();
                return;
            }
            let reply = command::run(&server, &line);
            if !reply.is_empty() {
                println!("{reply}");
            }
            if interactive {
                print!("> ");
                let _ = std::io::stdout().flush();
            }
        }
        // Stdin ending is not a reason to stop: a server started as a service
        // has none, and one fed a few commands from a pipe keeps running.
    });
    quit
}

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
