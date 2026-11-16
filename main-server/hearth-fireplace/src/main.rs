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
        // stderr, so `fireplace admin ...` prints only its answer on stdout.
        .with_writer(std::io::stderr)
        .init();

    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().is_some_and(|a| a == "--help" || a == "-h") {
        println!("fireplace [fireplace.toml]");
        println!("fireplace admin reset-password <handle> [fireplace.toml]");
        println!("fireplace admin test-mail <address> [fireplace.toml]");
        println!("fireplace admin policy-mail <letter.txt> [fireplace.toml]");
        println!("fireplace admin make-owner <handle> [fireplace.toml]");
        return Ok(());
    }
    if args.first().is_some_and(|a| a == "admin") {
        return admin(&args[1..]).await;
    }
    let config = read_config(args.first().map_or("fireplace.toml", String::as_str))?;

    let state = Arc::new(Fireplace::open(&config)?);
    let accounts = state.accounts.lock().unwrap().len();
    let addr = SocketAddr::from((config.bind, config.port));
    let listener =
        tokio::net::TcpListener::bind(addr).await.with_context(|| format!("could not listen on {addr}"))?;
    info!(%addr, accounts, email = state.mail.enabled(), key = %state.keys.public_hex(), "the Fireplace is lit");
    if !config.bind.is_loopback() {
        tracing::warn!(
            "listening beyond this machine: the Fireplace speaks plain HTTP and belongs behind a TLS terminator"
        );
    }

    // An address nobody confirmed is deleted after a week, whether or not
    // anything else happens to touch the file.
    let sweeping = state.clone();
    tokio::spawn(async move {
        let mut hourly = tokio::time::interval(std::time::Duration::from_secs(60 * 60));
        loop {
            hourly.tick().await;
            sweeping.accounts.lock().unwrap().forget_unverified(hearth_fireplace::token::now());
        }
    });

    // `ConnectInfo` so rate limiting knows who is asking -- see api::Caller,
    // which needs the peer to decide whether to believe X-Forwarded-For.
    axum::serve(listener, api::router(state).into_make_service_with_connect_info::<SocketAddr>())
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("the Fireplace stopped badly")?;
    info!("the Fireplace is out");
    Ok(())
}

fn read_config(path: &str) -> Result<Config> {
    Ok(match std::fs::read_to_string(path) {
        Ok(text) => Config::parse(&text).with_context(|| format!("in {path}"))?,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            info!("no {path}, using defaults");
            Config::default()
        }
        Err(err) => return Err(err).with_context(|| format!("could not read {path}")),
    })
}

/// For the operator, over SSH, as the user the service runs as:
///
/// ```sh
/// cd /opt/fireplace && sudo -u hearth ./fireplace admin reset-password ada
/// ```
///
/// Neither touches the running server: a reset link is signed with the key
/// in `data/`, and nothing about it is stored, so there is nothing to write.
async fn admin(args: &[String]) -> Result<()> {
    let (Some(command), Some(what)) = (args.first(), args.get(1)) else {
        anyhow::bail!(
            "fireplace admin reset-password <handle> | test-mail <address> | policy-mail <letter.txt> | make-owner <handle> [fireplace.toml]"
        );
    };
    let config = read_config(args.get(2).map_or("fireplace.toml", String::as_str))?;
    match command.as_str() {
        "reset-password" => {
            // Read only, and refused rather than made: an admin command run in
            // the wrong directory must not mint a new signing key there.
            if !config.data_dir.join("people.json").exists()
                || !hearth_fireplace::token::key_path(&config.data_dir).exists()
            {
                anyhow::bail!(
                    "no accounts in {}: run this where fireplace.toml is",
                    config.data_dir.display()
                );
            }
            let accounts = hearth_fireplace::accounts::Accounts::open(&config.data_dir)?;
            let account =
                accounts.by_handle(what).with_context(|| format!("nobody here goes by {what:?}"))?;
            let keys = hearth_fireplace::token::Keys::open(&config.data_dir)?;
            let link =
                keys.mint_link("reset", account.id, account.token_epoch, "", hearth_fireplace::token::now());
            let base = config.mail.public_url.trim().trim_end_matches('/');
            let base = if base.is_empty() { "https://<this server>" } else { base };
            println!("{base}/reset/{link}");
            println!("Works once, for 30 minutes. Send it to {} by a channel you trust.", account.handle);
            Ok(())
        }
        "test-mail" => {
            let mailer = hearth_fireplace::mail::Mailer::new(&config)?;
            let letter = mailer.test_letter();
            match mailer.send_now(what, letter).await {
                Ok(said) => {
                    println!("sent: {said}");
                    Ok(())
                }
                Err(err) => {
                    println!("not sent: {err:#}");
                    std::process::exit(1);
                }
            }
        }
        // The Fireplace owner. Works only while there is none: the owner
        // makes groups and gives them out on the account page, and if the
        // owner deletes their account, this works again.
        "make-owner" | "make-admin" => {
            if !config.data_dir.join("people.json").exists()
                || !hearth_fireplace::token::key_path(&config.data_dir).exists()
            {
                anyhow::bail!("no accounts in {}: run this where fireplace.toml is", config.data_dir.display());
            }
            let accounts = hearth_fireplace::accounts::Accounts::open(&config.data_dir)?;
            if accounts.any_owner() {
                anyhow::bail!("this Fireplace already has an owner: they give out groups on the account page");
            }
            let account = accounts.by_handle(what).with_context(|| format!("nobody here goes by {what:?}"))?;
            let (id, handle, epoch) = (account.id, account.handle.clone(), account.token_epoch);
            drop(accounts);
            // A link only this server's key can sign, handed to the running
            // server over loopback, so its copy of the accounts is the one
            // that changes. Not running: the file is ours to change.
            let keys = hearth_fireplace::token::Keys::open(&config.data_dir)?;
            let link = keys.mint_link("admin", id, epoch, "", hearth_fireplace::token::now());
            let port = config.port;
            match claim_over_loopback(port, &link).await {
                Ok((200, _)) => println!("{handle} is now the Fireplace owner."),
                Ok((status, said)) => anyhow::bail!("the Fireplace said no ({status}): {said}"),
                Err(err) if err.kind() == std::io::ErrorKind::ConnectionRefused => {
                    let mut accounts = hearth_fireplace::accounts::Accounts::open(&config.data_dir)?;
                    accounts.set_owner(id).map_err(|refused| anyhow::anyhow!("{refused}"))?;
                    println!("{handle} is now the Fireplace owner (the Fireplace was not running).");
                }
                Err(err) => return Err(err).context("could not reach the Fireplace on this machine"),
            }
            Ok(())
        }
        // Tells everybody who asked that the terms or privacy notice changed.
        // The file's first line is the subject, the rest the body.
        "policy-mail" => {
            let text = std::fs::read_to_string(what).with_context(|| format!("could not read {what}"))?;
            let (subject, body) = text.split_once('\n').unwrap_or((&text, ""));
            if subject.trim().is_empty() || body.trim().is_empty() {
                anyhow::bail!("{what}: the first line is the subject, then the body; one of them is empty");
            }
            if !config.data_dir.join("people.json").exists() {
                anyhow::bail!("no accounts in {}: run this where fireplace.toml is", config.data_dir.display());
            }
            let accounts = hearth_fireplace::accounts::Accounts::open(&config.data_dir)?;
            let mailer = hearth_fireplace::mail::Mailer::new(&config)?;
            let (mut sent, mut failed) = (0, 0);
            for (handle, address) in accounts.policy_recipients() {
                match mailer.send_now(&address, mailer.policy_letter(&handle, subject, body)).await {
                    Ok(_) => sent += 1,
                    Err(err) => {
                        failed += 1;
                        // The handle, not the address: addresses stay out of logs.
                        eprintln!("not sent to {handle}: {err:#}");
                    }
                }
            }
            println!("sent {sent}, failed {failed}");
            if failed > 0 {
                std::process::exit(1);
            }
            Ok(())
        }
        other => anyhow::bail!("no admin command called {other:?}"),
    }
}

/// `POST /admin/claim` to the Fireplace on this machine, by hand: one request
/// is not worth an HTTP client. Returns the status and the body.
async fn claim_over_loopback(port: u16, link: &str) -> std::io::Result<(u16, String)> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let body = serde_json::json!({ "link": link }).to_string();
    let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", port)).await?;
    stream
        .write_all(
            format!(
                "POST /admin/claim HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\n\
                 Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .as_bytes(),
        )
        .await?;
    let mut answer = String::new();
    stream.read_to_string(&mut answer).await?;
    let status = answer.split_whitespace().nth(1).and_then(|s| s.parse().ok()).unwrap_or(0);
    let said = answer.split("\r\n\r\n").nth(1).unwrap_or("").to_owned();
    Ok((status, said))
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
