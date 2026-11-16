//! The commands, written once and used by both front ends: typed at the
//! console server's prompt, or into the box at the bottom of the window.
//!
//! Every one of them is a plain synchronous call on `Server`, so there is no
//! runtime, no channel and no second code path between the two.

use std::path::Path;
use std::sync::Arc;

use hearth_protocol::HUB_INSTANCE;

use crate::Server;

pub const HELP: &str = "\
help                 this list
status               port, players, worlds, instances
worlds               the worlds this server can hand out
players              who is connected, and where
start <world>        start an instance of a world
stop <instance>      stop an instance and send its players to the hub
restart <instance>   stop an instance and start its world again
kick <player>        disconnect one player
instancing [on|off]  whether players may start their own instances
rescan               look at the worlds folder again
add <file.hworld>    copy a packed world into the worlds folder
quit                 stop the server (console only; close the window instead)";

/// Runs one line and returns what to print. Unknown commands answer rather
/// than fail: this is a prompt a person is typing at.
pub fn run(server: &Arc<Server>, line: &str) -> String {
    let line = line.trim();
    let (word, rest) = line.split_once(char::is_whitespace).unwrap_or((line, ""));
    let rest = rest.trim();
    match word.to_ascii_lowercase().as_str() {
        "" => String::new(),
        "help" | "?" | "commands" => HELP.to_owned(),
        "status" => status(server),
        "worlds" | "world" => worlds(server),
        "players" | "who" => players(server),
        "start" => start(server, rest),
        "stop" => stop(server, rest),
        "restart" => restart(server, rest),
        "kick" => kick(server, rest),
        "instancing" => instancing(server, rest),
        "rescan" => {
            server.rescan_worlds();
            worlds(server)
        }
        "add" => match server.add_world(Path::new(rest.trim_matches('"'))) {
            Ok(slug) => format!("added the world {slug}"),
            Err(err) => format!("{err:#}"),
        },
        "quit" | "exit" => "close the window to stop the server".to_owned(),
        other => format!("no command {other:?} — try help"),
    }
}

/// True for the lines that mean "shut the server down". The console server
/// checks this before running the line; the window has a close button.
pub fn is_quit(line: &str) -> bool {
    matches!(line.trim().to_ascii_lowercase().as_str(), "quit" | "exit")
}

fn status(server: &Arc<Server>) -> String {
    let instances = server.instances();
    let here: usize = instances.iter().map(|instance| instance.players.len()).sum();
    format!(
        "{here}/{} players in {} instance(s), {} world(s) in {}, players may start their own: {}",
        server.max_players,
        instances.len(),
        server.worlds().len(),
        server.worlds_dir().display(),
        if server.client_instancing() { "yes" } else { "no" },
    )
}

fn worlds(server: &Arc<Server>) -> String {
    let worlds = server.worlds();
    if worlds.is_empty() {
        return format!("no .hworld files in {}", server.worlds_dir().display());
    }
    worlds
        .iter()
        .map(|world| format!("{:<24} {:>10} bytes", world.slug, world.size))
        .collect::<Vec<_>>()
        .join("\n")
}

fn players(server: &Arc<Server>) -> String {
    let mut lines = Vec::new();
    for instance in server.instances() {
        lines.push(format!(
            "instance {} — {} ({} player(s)){}",
            instance.id,
            instance.slug,
            instance.players.len(),
            if instance.temporary { ", player-made" } else { "" }
        ));
        for (id, name) in &instance.players {
            lines.push(format!("    {id}  {name}"));
        }
    }
    lines.join("\n")
}

fn start(server: &Arc<Server>, slug: &str) -> String {
    if slug.is_empty() {
        return "start <world> — see `worlds`".to_owned();
    }
    match server.start_instance(slug) {
        Ok(instance) => format!("started {slug} as instance {instance}"),
        Err(err) => format!("{err:#}"),
    }
}

fn stop(server: &Arc<Server>, rest: &str) -> String {
    match rest.parse::<u16>() {
        Ok(HUB_INSTANCE) => "the hub cannot be stopped".to_owned(),
        Ok(instance) if server.stop_instance(instance) => format!("stopped instance {instance}"),
        Ok(instance) => format!("no instance {instance} is running"),
        Err(_) => "stop <instance> — a number, see `players`".to_owned(),
    }
}

fn restart(server: &Arc<Server>, rest: &str) -> String {
    let Ok(instance) = rest.parse::<u16>() else {
        return "restart <instance> — a number, see `players`".to_owned();
    };
    let Some(slug) = server.instances().into_iter().find(|view| view.id == instance).map(|view| view.slug)
    else {
        return format!("no instance {instance} is running");
    };
    let stopped = stop(server, rest);
    if !stopped.starts_with("stopped") {
        return stopped;
    }
    format!("{stopped}\n{}", start(server, &slug))
}

fn kick(server: &Arc<Server>, rest: &str) -> String {
    match rest.parse::<u16>() {
        Ok(id) if server.kick(id) => format!("kicked player {id}"),
        Ok(id) => format!("no player {id} is connected"),
        Err(_) => "kick <player> — a number, see `players`".to_owned(),
    }
}

fn instancing(server: &Arc<Server>, rest: &str) -> String {
    match rest.to_ascii_lowercase().as_str() {
        "on" | "yes" | "true" => server.set_client_instancing(true),
        "off" | "no" | "false" => server.set_client_instancing(false),
        "" => {}
        other => return format!("instancing on|off, not {other:?}"),
    }
    format!(
        "players may start their own instances: {}",
        if server.client_instancing() { "yes" } else { "no" }
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Config, Roster};
    use std::path::PathBuf;

    fn server(tag: &str, worlds: &[&str]) -> (PathBuf, Arc<Server>) {
        let dir = std::env::temp_dir().join(format!("hearth-cmd-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for slug in worlds {
            std::fs::write(dir.join(format!("{slug}.hworld")), b"packed").unwrap();
        }
        let config = Config { worlds_dir: Some(dir.clone()), max_instances: 3, ..Config::default() };
        (dir, Server::new(&config, tokio::sync::watch::channel(Roster::new()).0))
    }

    #[test]
    fn worlds_and_instances_from_the_prompt() {
        let (dir, server) = server("instances", &["forest"]);
        assert!(run(&server, "worlds").contains("forest"));
        assert!(run(&server, "status").contains("1 world"));
        assert!(run(&server, "players").starts_with("instance 0 — hub"));

        let started = run(&server, "start forest");
        assert!(started.starts_with("started forest as instance"), "{started}");
        let instance = server.instances().iter().map(|view| view.id).max().unwrap();
        assert_eq!(server.instances().len(), 2);

        assert!(run(&server, &format!("restart {instance}")).contains("stopped instance"));
        assert_eq!(server.instances().len(), 2, "restart leaves one running");

        let now = server.instances().iter().map(|view| view.id).max().unwrap();
        assert_eq!(run(&server, &format!("stop {now}")), format!("stopped instance {now}"));
        assert_eq!(run(&server, &format!("stop {now}")), format!("no instance {now} is running"));
        assert_eq!(run(&server, "stop 0"), "the hub cannot be stopped");
        assert_eq!(run(&server, "stop"), "stop <instance> — a number, see `players`");
        assert!(run(&server, "start nope").contains("no world"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn instancing_and_adding_worlds() {
        let (dir, server) = server("instancing", &[]);
        assert!(run(&server, "instancing").ends_with("no"));
        assert!(run(&server, "instancing on").ends_with("yes"));
        assert!(server.client_instancing());
        assert!(run(&server, "instancing off").ends_with("no"));
        assert!(run(&server, "instancing maybe").contains("on|off"));

        let loose = dir.join("meadow.hworld");
        std::fs::write(&loose, b"packed").unwrap();
        assert_eq!(run(&server, "add nowhere.hworld"), run(&server, "add nowhere.hworld"));
        assert!(run(&server, &format!("add {}", loose.display())).contains("meadow"));
        assert!(run(&server, "worlds").contains("meadow"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn unknown_and_empty_lines_answer_instead_of_failing() {
        let (dir, server) = server("unknown", &[]);
        assert_eq!(run(&server, "   "), "");
        assert!(run(&server, "help").contains("start <world>"));
        assert!(run(&server, "HELP").contains("start <world>"), "commands are not case sensitive");
        assert!(run(&server, "frobnicate the thing").contains("no command"));
        assert_eq!(run(&server, "kick 7"), "no player 7 is connected");
        assert!(is_quit("quit") && is_quit(" EXIT ") && !is_quit("status"));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
