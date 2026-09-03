mod app;
mod state;
mod tray;

fn window_icon() -> eframe::egui::IconData {
    let bytes = include_bytes!("../icones/icon.png");
    let image = image::load_from_memory(bytes)
        .expect("ícone da janela inválido")
        .into_rgba8();
    let (width, height) = image.dimensions();
    eframe::egui::IconData { rgba: image.into_raw(), width, height }
}

fn main() -> eframe::Result<()> {
    let (sender, receiver) = std::sync::mpsc::channel();
    tray::start(sender);
    state::ensure_shortcut();

    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([460.0, 700.0])
            .with_resizable(false)
            .with_icon(std::sync::Arc::new(window_icon()))
            .with_app_id(state::APP_ID),
        ..Default::default()
    };

    eframe::run_native(
        "Ropelato Discord",
        options,
        Box::new(|cc| {
            let mut fonts = eframe::egui::FontDefinitions::default();
            egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);
            cc.egui_ctx.set_fonts(fonts);
            Ok(Box::new(app::App::new(receiver)))
        }),
    )
}
