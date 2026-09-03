use crate::{state, tray::TrayEvent};
use eframe::egui;
use std::{
    sync::mpsc::Receiver,
    time::{Duration, Instant},
};

const REFRESH_INTERVAL: Duration = Duration::from_secs(2);

pub struct App {
    tray_receiver: Receiver<TrayEvent>,
    status: state::Status,
    connections: Vec<state::Connection>,
    last_refresh: Instant,
    message: Option<String>,
}

impl App {
    pub fn new(tray_receiver: Receiver<TrayEvent>) -> Self {
        if let Err(e) = state::ensure_service() {
            return Self {
                tray_receiver,
                status: state::status(),
                connections: Vec::new(),
                last_refresh: Instant::now(),
                message: Some(e),
            };
        }
        Self {
            tray_receiver,
            status: state::status(),
            connections: state::connections(),
            last_refresh: Instant::now(),
            message: None,
        }
    }

    fn refresh_status(&mut self) {
        self.status = state::status();
        self.connections = state::connections();
        self.last_refresh = Instant::now();
    }

    fn handle_result(&mut self, result: Result<(), String>) {
        match result {
            Ok(()) => self.message = None,
            Err(e) => self.message = Some(e),
        }
        self.refresh_status();
    }
}

fn state_color(state: &str) -> egui::Color32 {
    match state {
        "operacional" => egui::Color32::from_rgb(87, 242, 135),
        "pausado" => egui::Color32::from_rgb(250, 166, 26),
        "sem_proxies" => egui::Color32::from_rgb(250, 166, 26),
        _ => egui::Color32::from_rgb(237, 66, 69),
    }
}

fn state_title(state: &str) -> &'static str {
    match state {
        "operacional" => "Funcionando",
        "pausado" => "Pausado",
        "sem_proxies" => "Procurando saída",
        _ => "Parado",
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        while let Ok(event) = self.tray_receiver.try_recv() {
            match event {
                TrayEvent::Open => {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                }
                TrayEvent::TogglePause => {
                    let result = if self.status.fix_enabled { state::pause() } else { state::resume() };
                    self.handle_result(result);
                }
                TrayEvent::Quit => std::process::exit(0),
            }
        }

        if ctx.input(|i| i.viewport().events.contains(&egui::ViewportEvent::Close)) {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
        }

        if self.last_refresh.elapsed() >= REFRESH_INTERVAL {
            self.refresh_status();
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.heading(format!("Ropelato Discord v{}", self.status.version));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.colored_label(state_color(self.status.state), "●");
                    ui.label(state_title(self.status.state));
                });
            });
            ui.separator();

            if let Some(message) = &self.message {
                ui.colored_label(egui::Color32::from_rgb(237, 66, 69), message);
                ui.add_space(6.0);
            }

            if let Some(proxy) = &self.status.proxy_in_use {
                ui.label(format!("Sua sessão está saindo por {}.", proxy.region));
                ui.add_space(4.0);
                egui::Grid::new("proxy_info").num_columns(2).show(ui, |ui| {
                    ui.label("Proxies saudáveis:");
                    ui.label(self.status.healthy_proxies.to_string());
                    ui.end_row();
                    ui.label("Em uso:");
                    ui.label(&proxy.address);
                    ui.end_row();
                    ui.label("Latência:");
                    ui.label(format!("{} ms", proxy.latency_ms));
                    ui.end_row();
                });
            } else {
                ui.label(format!("Proxies saudáveis: {}", self.status.healthy_proxies));
            }
            ui.add_space(10.0);

            ui.horizontal(|ui| {
                if ui.button("Verificar agora").clicked() {
                    let result = state::check().map(|_| ());
                    self.handle_result(result);
                }
                if ui.button("Reiniciar Discord").clicked() {
                    let result = state::restart_discord().map(|_| ());
                    self.handle_result(result);
                }
                let label = if self.status.fix_enabled { "Pausar" } else { "Retomar" };
                if ui.button(label).clicked() {
                    let result = if self.status.fix_enabled { state::pause() } else { state::resume() };
                    self.handle_result(result);
                }
            });
            ui.add_space(6.0);

            let mut autostart = self.status.autostart;
            if ui.checkbox(&mut autostart, "Iniciar com a sessão").changed() {
                let result = state::set_autostart(autostart);
                self.handle_result(result);
            }

            ui.separator();
            ui.label("Atividade do Discord");
            egui::ScrollArea::vertical().max_height(220.0).show(ui, |ui| {
                for connection in &self.connections {
                    let (symbol, color) = if connection.route == "exterior" {
                        ("↗ exterior", egui::Color32::from_rgb(250, 166, 26))
                    } else {
                        ("→ direto", egui::Color32::from_rgb(148, 155, 164))
                    };
                    ui.horizontal(|ui| {
                        ui.colored_label(color, symbol);
                        ui.label(format!("{}:{}", connection.host, connection.port));
                    });
                }
                if self.connections.is_empty() {
                    ui.weak("Nada ainda.");
                }
            });

            ui.separator();
            if let Some(ms) = self.status.last_check_utc {
                ui.weak(format!("Última checagem: {} ms desde a época", ms));
            }
            if ui.link("Desinstalar").clicked() {
                let _ = state::start_uninstall();
                std::process::exit(0);
            }
        });

        ctx.request_repaint_after(Duration::from_millis(500));
    }
}
