//! A window to start, stop and watch a Flame, manage the
//! worlds it hands out, and type the same commands the console server takes.
//! Run `flame` instead for a console-only one.

#![windows_subsystem = "windows"]

use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use eframe::egui;
use hearth_flame::log::Log;
use hearth_flame::worlds::Repository;
use hearth_flame::{Config, Limit, Roster, Server, command};
use tokio::sync::watch;
use tracing_subscriber::fmt::writer::MakeWriterExt;

const AMBER: egui::Color32 = egui::Color32::from_rgb(230, 180, 90);
const RED: egui::Color32 = egui::Color32::from_rgb(230, 120, 110);
const GREEN: egui::Color32 = egui::Color32::from_rgb(90, 200, 120);

fn main() -> eframe::Result {
    // Everything the server logs goes into the window, and to stdout as well
    // when there is one (`cargo run`), since this build has no console.
    let log = Log::new();
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_ansi(false)
        .with_writer(log.clone().and(std::io::stdout))
        .init();

    let path = PathBuf::from(std::env::args().nth(1).unwrap_or_else(|| "flame.toml".into()));
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([520.0, 920.0])
            .with_min_inner_size([420.0, 560.0]),
        ..Default::default()
    };
    eframe::run_native("Hearth - Flame", options, Box::new(|_| Ok(Box::new(App::new(path, log)))))
}

struct Running {
    endpoint: quinn::Endpoint,
    server: Arc<Server>,
    roster: watch::Receiver<Roster>,
    addr: String,
}

struct App {
    runtime: tokio::runtime::Runtime,
    config_path: PathBuf,
    port: u16,
    data_dir: String,
    worlds_dir: String,
    hub_world: String,
    client_instancing: bool,
    allow_guests: bool,
    needs_same_fireplace: bool,
    fireplace: String,
    listing: hearth_flame::public::Listing,
    max_players: Limit,
    max_players_per_instance: Limit,
    max_instances: Limit,
    max_instances_per_player: Limit,
    /// What is in the file now, so a change is saved once and only once.
    saved: String,
    /// When the settings last changed; they are written a moment later, so
    /// dragging a number does not write the file on every frame.
    changed_at: Option<Instant>,
    /// The repository as the window sees it; the running server has its own
    /// scan of the same directory.
    repo: Repository,
    running: Option<Running>,
    message: String,
    log: Log,
    command: String,
    follow_log: bool,
}

impl App {
    fn new(config_path: PathBuf, log: Log) -> Self {
        let config = std::fs::read_to_string(&config_path)
            .ok()
            .and_then(|text| Config::parse(&text).ok())
            .unwrap_or_default();
        let worlds_dir = config.worlds_dir();
        Self {
            runtime: tokio::runtime::Runtime::new().expect("tokio runtime"),
            config_path,
            port: config.port,
            data_dir: config.data_dir.display().to_string(),
            worlds_dir: worlds_dir.display().to_string(),
            saved: config.to_toml(),
            hub_world: config.hub_world,
            client_instancing: config.client_instancing,
            allow_guests: config.allow_guests,
            needs_same_fireplace: config.needs_same_fireplace,
            fireplace: config.fireplace,
            listing: config.listing,
            max_players: config.max_players,
            max_players_per_instance: config.max_players_per_instance,
            max_instances: config.max_instances,
            max_instances_per_player: config.max_instances_per_player,
            changed_at: None,
            repo: Repository::open(worlds_dir),
            running: None,
            message: String::new(),
            log,
            command: String::new(),
            follow_log: true,
        }
    }

    fn config(&self) -> Config {
        Config {
            port: self.port,
            data_dir: PathBuf::from(&self.data_dir),
            worlds_dir: Some(PathBuf::from(&self.worlds_dir)),
            hub_world: self.hub_world.clone(),
            client_instancing: self.client_instancing,
            allow_guests: self.allow_guests,
            needs_account: Some(!self.allow_guests),
            needs_same_fireplace: self.needs_same_fireplace,
            fireplace: self.fireplace.trim().to_owned(),
            max_players: self.max_players,
            max_players_per_instance: self.max_players_per_instance,
            max_instances: self.max_instances,
            max_instances_per_player: self.max_instances_per_player,
            listing: self.listing.clone(),
        }
    }

    fn start(&mut self) -> anyhow::Result<()> {
        hearth_protocol::validate_slug(&self.hub_world)
            .map_err(|_| anyhow::anyhow!("The hub world name may only use a-z, 0-9, - and _"))?;
        let config = self.config();
        config.check()?;
        // quinn needs a tokio context to bind its socket.
        let _guard = self.runtime.enter();
        let (cert, key) = hearth_flame::load_or_create_cert(Path::new(&self.data_dir))?;
        let endpoint = hearth_flame::endpoint(self.port, cert.clone(), key)?;
        let addr = endpoint.local_addr()?.to_string();
        let (tx, roster) = watch::channel(Roster::new());
        let server = Server::new(&config, &cert, tx);
        self.runtime.spawn(hearth_flame::serve_with(endpoint.clone(), server.clone()));
        tracing::info!(%addr, max_players = %self.max_players, guests = self.allow_guests, "the Flame is lit");
        self.running = Some(Running { endpoint, server, roster, addr });
        Ok(())
    }

    fn stop(&mut self) {
        if let Some(running) = self.running.take() {
            tracing::info!("server stopped from the window");
            running.endpoint.close(0u8.into(), b"server stopped");
        }
    }

    /// Writes the settings when they changed. Something the server would
    /// refuse to start with is not written, so the file always loads.
    fn save_config(&mut self) {
        let config = self.config();
        let text = config.to_toml();
        self.changed_at = None;
        if text == self.saved {
            return;
        }
        if let Err(err) = config.check() {
            self.message = format!("Not saved: {err:#}");
            return;
        }
        match std::fs::write(&self.config_path, &text) {
            Ok(()) => {
                self.saved = text;
                self.message.clear();
            }
            Err(err) => self.message = format!("Could not save {}: {err}", self.config_path.display()),
        }
    }

    /// Notices a change and saves it half a second after the last one.
    fn autosave(&mut self, ctx: &egui::Context) {
        if self.config().to_toml() != self.saved && self.changed_at.is_none() {
            self.changed_at = Some(Instant::now());
        }
        if let Some(at) = self.changed_at {
            let wait = Duration::from_millis(500).saturating_sub(at.elapsed());
            if wait.is_zero() {
                self.save_config();
            } else {
                ctx.request_repaint_after(wait);
            }
        }
    }

    /// Copies a packed world into the repository, and tells a running server.
    fn add_world(&mut self, path: &Path) {
        self.message = match self.repo.add(path) {
            Ok(slug) => {
                if let Some(running) = &self.running {
                    running.server.rescan_worlds();
                }
                format!("Added the world {slug}")
            }
            Err(err) => format!("{err:#}"),
        };
    }

    /// The window's command box and the console server's prompt take the same
    /// commands, and both answers end up in the log.
    fn run_command(&mut self) {
        let line = std::mem::take(&mut self.command);
        if line.trim().is_empty() {
            return;
        }
        self.log.push(format!("> {line}"));
        let Some(running) = &self.running else {
            self.log.push("the server is not running");
            return;
        };
        if command::is_quit(&line) {
            self.log.push("close the window to stop the server");
            return;
        }
        let reply = command::run(&running.server, &line);
        if !reply.is_empty() {
            self.log.push(reply);
        }
        // A command may have changed the repository or a setting.
        self.client_instancing = running.server.client_instancing();
        self.repo = Repository::open(PathBuf::from(&self.worlds_dir));
    }

    fn worlds_ui(&mut self, ui: &mut egui::Ui) {
        ui.label("Worlds");
        ui.add_space(4.0);
        let worlds: Vec<_> = self.repo.worlds().map(|entry| entry.world.clone()).collect();
        if worlds.is_empty() {
            ui.weak(format!("No .hworld files in {}", self.worlds_dir));
        } else if !worlds.iter().any(|world| world.slug == self.hub_world) {
            let missing =
                format!("No {}.hworld here: players see their own room in the hub.", self.hub_world);
            ui.colored_label(AMBER, missing);
        }
        let mut start: Option<String> = None;
        for world in &worlds {
            ui.horizontal(|ui| {
                ui.add(egui::Label::new(egui::RichText::new(&world.slug).monospace()).truncate());
                ui.weak(size_text(world.size));
                if world.slug == self.hub_world {
                    ui.colored_label(GREEN, "hub");
                }
                let can_start = self.running.is_some();
                if ui.add_enabled(can_start, egui::Button::new("Start").small()).clicked() {
                    start = Some(world.slug.clone());
                }
            });
        }
        if let Some(slug) = start
            && let Some(running) = &self.running
        {
            self.message = match running.server.start_instance(&slug) {
                Ok(instance) => format!("Started {slug} as instance {instance}"),
                Err(err) => format!("{err:#}"),
            };
        }
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.weak("Drop a .hworld file here to add it");
            if ui.small_button("Rescan").clicked() {
                self.repo = Repository::open(PathBuf::from(&self.worlds_dir));
                if let Some(running) = &self.running {
                    running.server.rescan_worlds();
                }
            }
        });
    }

    fn instances_ui(&mut self, ui: &mut egui::Ui) {
        let Some(running) = &self.running else {
            ui.weak("Start the server to see who is where.");
            return;
        };
        let roster = running.roster.borrow().clone();
        let players: usize = roster.iter().map(|instance| instance.players.len()).sum();
        ui.label(format!("Instances ({players}/{} players, {} running)", self.max_players, roster.len()));
        ui.add_space(4.0);
        let mut stop = None;
        let mut kick = None;
        egui::ScrollArea::vertical().id_salt("instances").max_height(170.0).show(ui, |ui| {
            for instance in &roster {
                ui.horizontal(|ui| {
                    let title = format!("{}  {}", instance.id, instance.slug);
                    ui.add(egui::Label::new(egui::RichText::new(title).monospace().strong()).truncate());
                    ui.weak(format!("{} player(s)", instance.players.len()));
                    if instance.temporary {
                        ui.weak("player-made");
                    }
                    if instance.id != 0 && ui.small_button("Stop").clicked() {
                        stop = Some(instance.id);
                    }
                });
                for (id, name) in &instance.players {
                    ui.horizontal(|ui| {
                        ui.add_space(16.0);
                        let who = egui::RichText::new(format!("{id}  {name}")).monospace();
                        ui.add(egui::Label::new(who).truncate());
                        if ui.small_button("Kick").clicked() {
                            kick = Some(*id);
                        }
                    });
                }
            }
            if players == 0 {
                ui.weak("Nobody here yet.");
            }
        });
        if let Some(instance) = stop {
            running.server.stop_instance(instance);
        }
        if let Some(id) = kick {
            running.server.kick(id);
        }
    }

    fn command_ui(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let box_id = ui.scope_id().with("command");
            let entry = ui.add(
                egui::TextEdit::singleline(&mut self.command)
                    .id(box_id)
                    .hint_text("type a command, or help")
                    .desired_width(f32::INFINITY),
            );
            let entered = entry.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
            if entered {
                self.run_command();
                // Keep the cursor where somebody is typing a second command.
                ui.ctx().memory_mut(|memory| memory.request_focus(box_id));
            }
        });
    }

    /// Everything the server logged, newest at the bottom. With no console in
    /// this build, this is the only place it goes.
    fn console_ui(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label("Log");
            ui.weak(format!("{} line(s)", self.log.len()));
            if self.log.dropped() > 0 {
                ui.weak(format!("+{} older, gone", self.log.dropped()));
            }
            ui.checkbox(&mut self.follow_log, "Follow");
            if ui.small_button("Copy").clicked() {
                ui.ctx().copy_text(self.log.lines().join("\n"));
            }
            if ui.small_button("Clear").clicked() {
                self.log.clear();
            }
        });
        ui.add_space(4.0);
        egui::ScrollArea::both()
            .id_salt("log")
            .auto_shrink([false, false])
            .stick_to_bottom(self.follow_log)
            .show(ui, |ui| {
                ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
                let plain = ui.visuals().text_color();
                for line in self.log.lines() {
                    let colour = if line.contains("ERROR") {
                        RED
                    } else if line.contains("WARN") {
                        AMBER
                    } else if line.starts_with('>') {
                        GREEN
                    } else {
                        plain
                    };
                    ui.colored_label(colour, egui::RichText::new(line).monospace().size(11.0));
                }
            });
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.heading("Hearth - Flame");
            match &self.running {
                Some(running) => ui.colored_label(GREEN, format!("listening on {}", running.addr)),
                None => ui.label("stopped"),
            };
        });
        if let Some(running) = &self.running {
            if let Some(ip) = local_ip() {
                ui.horizontal(|ui| {
                    let address = format!("{ip}:{}", self.port);
                    ui.label("Friends connect to");
                    ui.add(egui::Label::new(egui::RichText::new(&address).monospace().strong()));
                    if ui.small_button("copy").clicked() {
                        ctx.copy_text(address);
                    }
                });
            }
            // The same summary the `status` command prints.
            ui.weak(command::run(&running.server, "status"));
        }

        ui.add_space(8.0);
        ui.add_enabled_ui(self.running.is_none(), |ui| {
            egui::Grid::new("config").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
                ui.label("Port");
                ui.add(egui::DragValue::new(&mut self.port).range(1..=65535));
                ui.end_row();
                ui.label("Data folder");
                ui.text_edit_singleline(&mut self.data_dir);
                ui.end_row();
                ui.label("Worlds folder");
                ui.text_edit_singleline(&mut self.worlds_dir);
                ui.end_row();
                ui.label("Hub world");
                ui.text_edit_singleline(&mut self.hub_world);
                ui.end_row();
                ui.label("Accounts Fireplace");
                ui.add(egui::TextEdit::singleline(&mut self.fireplace).hint_text("https://fireplace... (or blank for guests only)"));
                ui.end_row();
                ui.label("");
                ui.checkbox(&mut self.allow_guests, "Allow guests (no account required, no saves kept)");
                ui.end_row();
                ui.label("");
                ui.checkbox(&mut self.needs_same_fireplace, "Require account from this specific Fireplace (otherwise guests)");
                ui.end_row();
                ui.label("");
                ui.checkbox(&mut self.listing.public, "List this Flame publicly on the Fireplace");
                ui.end_row();
                if self.listing.public {
                    ui.label("Publish to");
                    ui.add(egui::TextEdit::singleline(&mut self.listing.fireplace).hint_text("leave blank to use Accounts Fireplace"));
                    ui.end_row();
                    ui.label("Public name");
                    ui.add(egui::TextEdit::singleline(&mut self.listing.name).hint_text("the hub world's name"));
                    ui.end_row();
                    ui.label("Description");
                    ui.add(egui::TextEdit::singleline(&mut self.listing.description).hint_text("short summary of this server"));
                    ui.end_row();
                    ui.label("Banner URL");
                    ui.add(egui::TextEdit::singleline(&mut self.listing.banner).hint_text("https://... or banner.png"));
                    ui.end_row();
                }
            });
            ui.add_space(6.0);
            ui.label("Limits");
            egui::Grid::new("limits").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
                limit_ui(ui, "Players on the server", &mut self.max_players, 1, 16);
                limit_ui(ui, "Players per instance", &mut self.max_players_per_instance, 1, 16);
                limit_ui(ui, "Instances on the server", &mut self.max_instances, 1, 8);
                limit_ui(ui, "Instances per player", &mut self.max_instances_per_player, 0, 1);
            });
        });

        // Live: a host can open or close this while people are connected.
        if ui.checkbox(&mut self.client_instancing, "Let players start their own instances").changed()
            && let Some(running) = &self.running
        {
            running.server.set_client_instancing(self.client_instancing);
        }

        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if self.running.is_none() {
                if ui.add(egui::Button::new("Start").min_size([80.0, 28.0].into())).clicked() {
                    self.message = match self.start() {
                        Ok(()) => String::new(),
                        Err(err) => format!("{err:#}"),
                    };
                }
            } else if ui.add(egui::Button::new("Stop").min_size([80.0, 28.0].into())).clicked() {
                self.stop();
            }
            ui.weak(format!("Settings save to {} as you change them", self.config_path.display()));
        });

        if !self.message.is_empty() {
            ui.add_space(4.0);
            ui.colored_label(RED, &self.message);
        }

        ui.add_space(8.0);
        ui.separator();
        self.worlds_ui(ui);
        ui.add_space(8.0);
        ui.separator();
        self.instances_ui(ui);
        ui.add_space(8.0);
        ui.separator();
        self.command_ui(ui);
        ui.add_space(4.0);
        self.console_ui(ui);

        // Dropping a .hworld on the window adds it, so no file dialog is needed.
        let dropped: Vec<PathBuf> =
            ctx.input(|input| input.raw.dropped_files.iter().map(|file| file.path().to_path_buf()).collect());
        for path in dropped {
            self.add_world(&path);
        }

        self.autosave(&ctx);
        if self.running.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(500));
        }
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.save_config();
        self.stop();
    }
}

/// One limit: a number, or a "no limit" box that turns it off -- the window's
/// way of writing the `-1` a headless server's config uses.
fn limit_ui(ui: &mut egui::Ui, label: &str, limit: &mut Limit, min: u32, default: u32) {
    ui.label(label);
    ui.horizontal(|ui| {
        let mut none = limit.0.is_none();
        if ui.checkbox(&mut none, "No limit").changed() {
            *limit = if none { Limit::NONE } else { Limit::of(default) };
        }
        if let Some(max) = &mut limit.0 {
            ui.add(egui::DragValue::new(max).range(min..=u16::MAX as u32));
        }
    });
    ui.end_row();
}

fn size_text(bytes: u32) -> String {
    match bytes {
        0..1024 => format!("{bytes} B"),
        n if n < 1024 * 1024 => format!("{:.0} kB", n as f32 / 1024.0),
        n => format!("{:.1} MB", n as f32 / (1024.0 * 1024.0)),
    }
}

/// Asks the OS which local address would reach the internet; sends nothing.
fn local_ip() -> Option<IpAddr> {
    let socket = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("8.8.8.8:80").ok()?;
    Some(socket.local_addr().ok()?.ip())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(tag: &str) -> (PathBuf, App) {
        let dir = std::env::temp_dir().join(format!("hearth-gui-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut app = App::new(dir.join("flame.toml"), Log::new());
        app.port = 0; // any free port
        app.max_players = Limit::of(3);
        app.fireplace.clear(); // no Fireplace to reach in a test
        app.data_dir = dir.join("data").display().to_string();
        app.worlds_dir = dir.join("worlds").display().to_string();
        app.repo = Repository::open(&app.worlds_dir);
        (dir, app)
    }

    #[test]
    fn start_stop_and_save_round_trip() {
        let (dir, mut app) = app("round-trip");
        app.client_instancing = true;

        app.start().unwrap();
        // The hub is running and empty.
        let roster = app.running.as_ref().unwrap().roster.borrow().clone();
        assert_eq!(roster.len(), 1);
        assert!(roster[0].players.is_empty());
        app.stop();
        assert!(app.running.is_none());

        app.max_players_per_instance = Limit::NONE;
        app.allow_guests = false;
        app.fireplace = "https://example.org".into();
        app.save_config();
        let saved = Config::parse(&std::fs::read_to_string(dir.join("flame.toml")).unwrap()).unwrap();
        assert_eq!((saved.port, saved.max_players), (0, Limit::of(3)));
        assert_eq!(saved.max_players_per_instance, Limit::NONE, "written as -1");
        assert!(!saved.allow_guests);
        assert_eq!(saved.fireplace, "https://example.org");
        assert_eq!(saved.data_dir.display().to_string(), app.data_dir);
        assert_eq!(saved.worlds_dir().display().to_string(), app.worlds_dir);
        assert!(saved.client_instancing);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn adding_a_world_copies_it_into_the_repository() {
        let (dir, mut app) = app("add");
        let loose = dir.join("meadow.hworld");
        std::fs::write(&loose, b"packed").unwrap();

        app.add_world(&loose);
        assert!(dir.join("worlds/meadow.hworld").exists(), "{}", app.message);
        assert_eq!(app.repo.worlds().count(), 1);

        // A file that is not a usable world is reported, not added.
        app.add_world(&dir.join("nope.hworld"));
        assert!(app.message.contains("not a usable world"), "{}", app.message);
        assert_eq!(app.repo.worlds().count(), 1);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// The box at the bottom of the window runs the console server's commands.
    #[test]
    fn the_command_box_drives_the_server() {
        let (dir, mut app) = app("commands");
        std::fs::create_dir_all(dir.join("worlds")).unwrap();
        std::fs::write(dir.join("worlds/forest.hworld"), b"packed").unwrap();
        app.repo = Repository::open(&app.worlds_dir);

        app.command = "status".into();
        app.run_command();
        assert!(app.log.lines().iter().any(|line| line.starts_with("> status")));
        assert!(app.log.lines().last().unwrap().contains("the server is not running"));

        app.start().unwrap();
        app.command = "start forest".into();
        app.run_command();
        assert!(app.log.lines().last().unwrap().starts_with("started forest"), "{:?}", app.log.lines());
        assert_eq!(app.running.as_ref().unwrap().server.instances().len(), 2);

        // A command that changes a setting shows up in the checkbox.
        app.command = "instancing on".into();
        app.run_command();
        assert!(app.client_instancing);

        app.command = "   ".into();
        let before = app.log.len();
        app.run_command();
        assert_eq!(app.log.len(), before, "an empty line does nothing");

        app.command = "quit".into();
        app.run_command();
        assert!(app.log.lines().last().unwrap().contains("close the window"));
        assert!(app.running.is_some(), "a typed quit does not close the window");
        app.stop();
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_change_is_saved_on_its_own_and_a_bad_one_is_not() {
        let (dir, mut app) = app("autosave");
        let ctx = egui::Context::default();
        app.save_config();
        let path = dir.join("flame.toml");
        let before = std::fs::read_to_string(&path).unwrap();

        app.max_instances_per_player = Limit::of(4);
        app.autosave(&ctx); // noticed, not yet written
        assert_eq!(std::fs::read_to_string(&path).unwrap(), before);
        app.changed_at = Some(Instant::now() - Duration::from_secs(1));
        app.autosave(&ctx);
        let saved = Config::parse(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(saved.max_instances_per_player, Limit::of(4));

        // Nobody could ever join this one, so it is not written.
        app.allow_guests = false;
        app.fireplace.clear();
        app.save_config();
        assert!(app.message.starts_with("Not saved"), "{}", app.message);
        assert!(Config::parse(&std::fs::read_to_string(&path).unwrap()).unwrap().allow_guests);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn sizes_read_as_people_expect() {
        assert_eq!(size_text(512), "512 B");
        assert_eq!(size_text(2048), "2 kB");
        assert_eq!(size_text(3 * 1024 * 1024), "3.0 MB");
    }
}
