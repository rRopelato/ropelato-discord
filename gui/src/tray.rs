use std::{sync::mpsc::Sender, time::Duration};
use tray_icon::{
    menu::{Menu, MenuEvent, MenuItem},
    Icon, TrayIcon, TrayIconBuilder, TrayIconEvent,
};

const ICON_ACTIVE: &[u8] = include_bytes!("../icones/bandeja-operacional.png");
const ICON_PAUSED: &[u8] = include_bytes!("../icones/bandeja-pausado.png");
const ICON_NO_PROXIES: &[u8] = include_bytes!("../icones/bandeja-sem_proxies.png");
const ICON_STOPPED: &[u8] = include_bytes!("../icones/bandeja-parado.png");

const REFRESH_INTERVAL: Duration = Duration::from_millis(700);

pub enum TrayEvent {
    Open,
    TogglePause,
    Quit,
}

fn load_icon(bytes: &[u8]) -> Icon {
    let image = image::load_from_memory(bytes)
        .expect("ícone de bandeja embutido é inválido")
        .into_rgba8();
    let (width, height) = image.dimensions();
    Icon::from_rgba(image.into_raw(), width, height).expect("ícone de bandeja inválido")
}

fn icon_for(state: &str) -> &'static [u8] {
    match state {
        "operacional" => ICON_ACTIVE,
        "pausado" => ICON_PAUSED,
        "sem_proxies" | "inicializando" => ICON_NO_PROXIES,
        _ => ICON_STOPPED,
    }
}

fn tooltip_for(state: &str) -> &'static str {
    match state {
        "operacional" => "Ropelato Discord — funcionando",
        "pausado" => "Ropelato Discord — pausado",
        "sem_proxies" => "Ropelato Discord — procurando saída",
        "inicializando" => "Ropelato Discord — preparando",
        _ => "Ropelato Discord — parado",
    }
}

fn refresh_icon(tray: &TrayIcon, pause_item: &MenuItem, previous_state: &mut String) {
    let current = crate::state::status();
    if current.state == previous_state.as_str() {
        return;
    }
    let _ = tray.set_icon(Some(load_icon(icon_for(current.state))));
    let _ = tray.set_tooltip(Some(tooltip_for(current.state)));
    let _ = pause_item.set_text(if current.fix_enabled { "Pausar" } else { "Retomar" });
    *previous_state = current.state.to_string();
}

pub fn start(sender: Sender<TrayEvent>) {
    std::thread::spawn(move || {
        gtk::init().expect("não consegui iniciar o GTK da bandeja");

        let open_item = MenuItem::new("Abrir", true, None);
        let pause_item = MenuItem::new("Pausar", true, None);
        let quit_item = MenuItem::new("Sair (o serviço continua)", true, None);

        let menu = Menu::new();
        let _ = menu.append(&open_item);
        let _ = menu.append(&tray_icon::menu::PredefinedMenuItem::separator());
        let _ = menu.append(&pause_item);
        let _ = menu.append(&tray_icon::menu::PredefinedMenuItem::separator());
        let _ = menu.append(&quit_item);

        let open_id = open_item.id().clone();
        let pause_id = pause_item.id().clone();
        let quit_id = quit_item.id().clone();

        let tray = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_icon(load_icon(ICON_STOPPED))
            .with_tooltip(tooltip_for("inicializando"))
            .build()
            .expect("não consegui criar o ícone de bandeja");

        let mut previous_state = String::new();

        glib::timeout_add_local(REFRESH_INTERVAL, move || {
            if let Ok(event) = MenuEvent::receiver().try_recv() {
                if event.id == open_id {
                    let _ = sender.send(TrayEvent::Open);
                } else if event.id == pause_id {
                    let _ = sender.send(TrayEvent::TogglePause);
                } else if event.id == quit_id {
                    let _ = sender.send(TrayEvent::Quit);
                    gtk::main_quit();
                    return glib::ControlFlow::Break;
                }
            }
            if let Ok(TrayIconEvent::Click { .. }) = TrayIconEvent::receiver().try_recv() {
                let _ = sender.send(TrayEvent::Open);
            }

            refresh_icon(&tray, &pause_item, &mut previous_state);
            glib::ControlFlow::Continue
        });

        gtk::main();
    });
}
