mod app;
mod state;
mod tray;

fn main() -> eframe::Result<()> {
    let (sender, receiver) = std::sync::mpsc::channel();
    tray::start(sender);

    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([420.0, 520.0])
            .with_resizable(false),
        ..Default::default()
    };

    eframe::run_native(
        "Ropelato Discord",
        options,
        Box::new(|_cc| Ok(Box::new(app::App::new(receiver)))),
    )
}
