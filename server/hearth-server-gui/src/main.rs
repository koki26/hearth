//! A window to start, stop and watch the Hearth relay server.
//! Run `hearth-server` instead for a console-only server.

#![windows_subsystem = "windows"]

use std::net::IpAddr;
use std::path::PathBuf;

use eframe::egui;
use hearth_server::{Config, Roster};
use tokio::sync::watch;

fn main() -> eframe::Result {
    tracing_subscriber::fmt().with_env_filter("info").init();
    let path = PathBuf::from(std::env::args().nth(1).unwrap_or_else(|| "hearth.toml".into()));
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([420.0, 520.0]).with_min_inner_size([360.0, 400.0]),
        ..Default::default()
    };
    eframe::run_native("Hearth Server", options, Box::new(|_| Ok(Box::new(App::new(path)))))
}

struct Running {
    endpoint: quinn::Endpoint,
    roster: watch::Receiver<Roster>,
    addr: String,
}

struct App {
    runtime: tokio::runtime::Runtime,
    config_path: PathBuf,
    port: u16,
    max_players: u16,
    data_dir: String,
    running: Option<Running>,
    message: String,
}

impl App {
    fn new(config_path: PathBuf) -> Self {
        let config = std::fs::read_to_string(&config_path)
            .ok()
            .and_then(|text| Config::parse(&text).ok())
            .unwrap_or_default();
        Self {
            runtime: tokio::runtime::Runtime::new().expect("tokio runtime"),
            config_path,
            port: config.port,
            max_players: config.max_players,
            data_dir: config.data_dir.display().to_string(),
            running: None,
            message: String::new(),
        }
    }

    fn start(&mut self) -> anyhow::Result<()> {
        // quinn needs a tokio context to bind its socket.
        let _guard = self.runtime.enter();
        let (cert, key) = hearth_server::load_or_create_cert(std::path::Path::new(&self.data_dir))?;
        let endpoint = hearth_server::endpoint(self.port, cert, key)?;
        let addr = endpoint.local_addr()?.to_string();
        let (tx, roster) = watch::channel(Roster::new());
        self.runtime.spawn(hearth_server::serve_with_roster(endpoint.clone(), self.max_players, tx));
        self.running = Some(Running { endpoint, roster, addr });
        Ok(())
    }

    fn stop(&mut self) {
        if let Some(running) = self.running.take() {
            running.endpoint.close(0u8.into(), b"server stopped");
        }
    }

    fn save_config(&mut self) {
        let text = format!(
            "port = {}\nmax_players = {}\ndata_dir = {:?}\n",
            self.port, self.max_players, self.data_dir
        );
        self.message = match std::fs::write(&self.config_path, text) {
            Ok(()) => format!("Saved {}", self.config_path.display()),
            Err(err) => format!("Could not save {}: {err}", self.config_path.display()),
        };
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        ui.add_space(4.0);
        ui.heading("Hearth Server");
        ui.add_space(8.0);

        match &self.running {
            Some(running) => {
                ui.colored_label(egui::Color32::from_rgb(90, 200, 120), format!("Listening on {}", running.addr));
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
            }
            None => {
                ui.label("Stopped");
            }
        }

        ui.add_space(12.0);
        ui.add_enabled_ui(self.running.is_none(), |ui| {
            egui::Grid::new("config").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
                ui.label("Port");
                ui.add(egui::DragValue::new(&mut self.port).range(1..=65535));
                ui.end_row();
                ui.label("Max players");
                ui.add(egui::DragValue::new(&mut self.max_players).range(1..=u16::MAX));
                ui.end_row();
                ui.label("Data folder");
                ui.text_edit_singleline(&mut self.data_dir);
                ui.end_row();
            });
        });

        ui.add_space(12.0);
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
            if ui.button("Save config").clicked() {
                self.save_config();
            }
        });

        if !self.message.is_empty() {
            ui.add_space(6.0);
            ui.colored_label(egui::Color32::from_rgb(230, 120, 110), &self.message);
        }

        ui.add_space(12.0);
        ui.separator();
        let roster = self.running.as_ref().map(|running| running.roster.borrow().clone()).unwrap_or_default();
        ui.label(format!("Players ({}/{})", roster.len(), self.max_players));
        ui.add_space(4.0);
        egui::ScrollArea::vertical().show(ui, |ui| {
            if roster.is_empty() {
                ui.weak("Nobody here yet.");
            }
            for (id, name) in roster {
                ui.label(format!("{id}  {name}"));
            }
        });

        if self.running.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(500));
        }
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.stop();
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

    #[test]
    fn start_stop_and_save_round_trip() {
        let dir = std::env::temp_dir().join(format!("hearth-gui-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut app = App::new(dir.join("hearth.toml"));
        app.port = 0; // any free port
        app.max_players = 3;
        app.data_dir = dir.join("data").display().to_string();

        app.start().unwrap();
        assert!(app.running.as_ref().is_some_and(|running| running.roster.borrow().is_empty()));
        app.stop();
        assert!(app.running.is_none());

        app.save_config();
        let saved = Config::parse(&std::fs::read_to_string(dir.join("hearth.toml")).unwrap()).unwrap();
        assert_eq!((saved.port, saved.max_players), (0, 3));
        assert_eq!(saved.data_dir.display().to_string(), app.data_dir);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
