use crate::{state, tray::TrayEvent};
use eframe::egui;
use egui_phosphor::regular as icon;
use std::{
    sync::mpsc::Receiver,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const REFRESH_INTERVAL: Duration = Duration::from_secs(2);

const ACCENT: egui::Color32 = egui::Color32::from_rgb(88, 101, 242);
const GREEN: egui::Color32 = egui::Color32::from_rgb(87, 242, 135);
const YELLOW: egui::Color32 = egui::Color32::from_rgb(250, 166, 26);
const RED: egui::Color32 = egui::Color32::from_rgb(237, 66, 69);
const MUTED: egui::Color32 = egui::Color32::from_rgb(148, 155, 164);

pub struct App {
    tray_receiver: Receiver<TrayEvent>,
    status: state::Status,
    connections: Vec<state::Connection>,
    last_refresh: Instant,
    message: Option<String>,
    styled: bool,
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
                styled: false,
            };
        }
        Self {
            tray_receiver,
            status: state::status(),
            connections: state::connections(),
            last_refresh: Instant::now(),
            message: None,
            styled: false,
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
        "operacional" => GREEN,
        "pausado" => YELLOW,
        "sem_proxies" => YELLOW,
        _ => RED,
    }
}

fn state_title(state: &str) -> &'static str {
    match state {
        "operacional" => "Operacional",
        "pausado" => "Pausado",
        "sem_proxies" => "Procurando saída",
        _ => "Parado",
    }
}

fn now_millis() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64
}

fn relative_time(epoch_ms: u64) -> String {
    let elapsed_secs = now_millis().saturating_sub(epoch_ms) / 1000;
    match elapsed_secs {
        0..=4 => "agora mesmo".to_string(),
        5..=59 => format!("há {elapsed_secs}s"),
        60..=3599 => format!("há {}min", elapsed_secs / 60),
        3600..=86399 => format!("há {}h", elapsed_secs / 3600),
        _ => format!("há {}d", elapsed_secs / 86400),
    }
}

fn apply_style(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();
    style.spacing.item_spacing = egui::vec2(8.0, 10.0);
    style.spacing.button_padding = egui::vec2(14.0, 7.0);
    style.visuals.window_corner_radius = egui::CornerRadius::same(10);
    style.visuals.menu_corner_radius = egui::CornerRadius::same(8);
    for widgets in [
        &mut style.visuals.widgets.noninteractive,
        &mut style.visuals.widgets.inactive,
        &mut style.visuals.widgets.hovered,
        &mut style.visuals.widgets.active,
        &mut style.visuals.widgets.open,
    ] {
        widgets.corner_radius = egui::CornerRadius::same(8);
    }
    style.visuals.selection.bg_fill = ACCENT;
    style.visuals.hyperlink_color = ACCENT.gamma_multiply(1.3);
    ctx.set_style(style);
}

fn status_pill(ui: &mut egui::Ui, color: egui::Color32, text: &str) {
    egui::Frame::new()
        .fill(color.gamma_multiply(0.16))
        .corner_radius(egui::CornerRadius::same(20))
        .inner_margin(egui::Margin::symmetric(10, 4))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.colored_label(color, "●");
                ui.colored_label(color, text);
            });
        });
}

fn card(ui: &mut egui::Ui, add_contents: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::new()
        .fill(ui.visuals().extreme_bg_color)
        .corner_radius(egui::CornerRadius::same(10))
        .inner_margin(egui::Margin::same(12))
        .show(ui, add_contents);
}

fn info_row(ui: &mut egui::Ui, glyph: &str, label: &str, value: impl Into<String>) {
    ui.horizontal(|ui| {
        ui.colored_label(MUTED, glyph);
        ui.weak(label);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(value.into());
        });
    });
}

fn primary_button(ui: &mut egui::Ui, glyph: &str, text: &str) -> egui::Response {
    ui.add(
        egui::Button::new(
            egui::RichText::new(format!("{glyph} {text}")).color(egui::Color32::from_gray(20)),
        )
        .fill(egui::Color32::from_gray(235)),
    )
}

fn secondary_button(ui: &mut egui::Ui, glyph: &str, text: &str) -> egui::Response {
    ui.button(format!("{glyph} {text}"))
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if !self.styled {
            apply_style(ctx);
            self.styled = true;
        }

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

        if self.last_refresh.elapsed() >= REFRESH_INTERVAL {
            self.refresh_status();
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                ui.heading("Ropelato Discord");
                egui::Frame::new()
                    .fill(ui.visuals().extreme_bg_color)
                    .corner_radius(egui::CornerRadius::same(6))
                    .inner_margin(egui::Margin::symmetric(6, 2))
                    .show(ui, |ui| ui.weak(format!("v{}", self.status.version)));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    status_pill(ui, state_color(self.status.state), state_title(self.status.state));
                });
            });
            ui.add_space(10.0);

            if let Some(message) = &self.message {
                egui::Frame::new()
                    .fill(RED.gamma_multiply(0.15))
                    .corner_radius(egui::CornerRadius::same(8))
                    .inner_margin(egui::Margin::symmetric(10, 8))
                    .show(ui, |ui| {
                        ui.colored_label(RED, message);
                    });
                ui.add_space(8.0);
            }

            ui.columns(2, |columns| {
                card(&mut columns[0], |ui| {
                    ui.horizontal(|ui| {
                        ui.colored_label(GREEN, icon::WIFI_HIGH);
                        if let Some(proxy) = &self.status.proxy_in_use {
                            ui.label(format!("Saindo por {}", proxy.region));
                        } else {
                            ui.label("Sem saída ativa");
                        }
                    });
                    ui.separator();
                    info_row(ui, icon::SHIELD_CHECK, "Proxies saudáveis", self.status.healthy_proxies.to_string());
                    if let Some(proxy) = &self.status.proxy_in_use {
                        info_row(ui, icon::GLOBE, "IP em uso", proxy.address.clone());
                    }
                });

                card(&mut columns[1], |ui| {
                    ui.horizontal(|ui| {
                        ui.colored_label(YELLOW, icon::ACTIVITY);
                        ui.weak("Latência");
                    });
                    if let Some(proxy) = &self.status.proxy_in_use {
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new(proxy.latency_ms.to_string()).size(22.0).strong());
                            ui.weak("ms");
                        });
                        ui.weak(&proxy.region);
                    } else {
                        ui.weak("—");
                    }
                });
            });
            ui.add_space(10.0);

            ui.columns(3, |columns| {
                if primary_button(&mut columns[0], icon::PAPER_PLANE_TILT, "Verificar agora").clicked() {
                    let result = state::check().map(|_| ());
                    self.handle_result(result);
                }
                if secondary_button(&mut columns[1], icon::ARROW_CLOCKWISE, "Reiniciar Discord").clicked() {
                    let result = state::restart_discord().map(|_| ());
                    self.handle_result(result);
                }
                let label = if self.status.fix_enabled { "Pausar" } else { "Retomar" };
                if secondary_button(&mut columns[2], icon::PAUSE, label).clicked() {
                    let result = if self.status.fix_enabled { state::pause() } else { state::resume() };
                    self.handle_result(result);
                }
            });
            ui.add_space(8.0);

            let mut autostart = self.status.autostart;
            if ui.checkbox(&mut autostart, "Iniciar com a sessão").changed() {
                let result = state::set_autostart(autostart);
                self.handle_result(result);
            }

            ui.add_space(6.0);
            ui.separator();
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.colored_label(MUTED, icon::ACTIVITY);
                ui.strong("Atividade do Discord");
            });
            ui.add_space(4.0);

            egui::ScrollArea::vertical().max_height(220.0).show(ui, |ui| {
                for connection in &self.connections {
                    let (label, dot) = if connection.route == "exterior" {
                        ("exterior", YELLOW)
                    } else {
                        ("direto", GREEN)
                    };
                    ui.horizontal(|ui| {
                        ui.colored_label(dot, "●");
                        ui.add_sized([54.0, 16.0], egui::Label::new(egui::RichText::new(label).small()));
                        ui.monospace(&connection.host);
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.weak(format!(":{}", connection.port));
                        });
                    });
                }
                if self.connections.is_empty() {
                    ui.weak("Nada ainda.");
                }
            });

            ui.add_space(6.0);
            ui.separator();
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                if let Some(ms) = self.status.last_check_utc {
                    ui.weak(format!("Lista de proxies verificada {}", relative_time(ms)));
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.link("Desinstalar").clicked() {
                        let _ = state::start_uninstall();
                        std::process::exit(0);
                    }
                });
            });

            ui.add_space(6.0);
            ui.separator();
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.weak("Ropelato Discord");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .add(egui::Label::new(egui::RichText::new(icon::GITHUB_LOGO).size(16.0)).sense(egui::Sense::click()))
                        .on_hover_text("Abrir repositório no GitHub")
                        .clicked()
                    {
                        let _ = std::process::Command::new("xdg-open").arg(state::REPOSITORY_URL).spawn();
                    }
                });
            });
            });
        });

        ctx.request_repaint_after(Duration::from_millis(500));
    }
}
