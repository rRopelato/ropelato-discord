use ropelato_discord_core::{data_dir, installed_path, log_path, ready_marker_path, last_check_path, discord, platform, pac_url};
use std::{
    fs,
    process::{Command, Stdio},
};

const APP_ICON: &[u8] = include_bytes!("../icones/icon.png");

pub const REPOSITORY_URL: &str = "https://github.com/rRopelato/ropelato-discord";
pub const APP_ID: &str = "ropelato-discord-gui";

fn local_bin_dir() -> Option<std::path::PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(std::path::PathBuf::from(home).join(".local/bin"))
}

fn applications_dir() -> Option<std::path::PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(std::path::PathBuf::from(home).join(".local/share/applications"))
}

fn desktop_dir() -> Option<std::path::PathBuf> {
    let home = std::env::var_os("HOME")?;
    let home = std::path::PathBuf::from(home);
    let config = fs::read_to_string(home.join(".config/user-dirs.dirs")).ok();
    let configured = config.as_deref().and_then(|content| {
        content.lines().find_map(|line| {
            let value = line.strip_prefix("XDG_DESKTOP_DIR=")?;
            let value = value.trim_matches('"').replace("$HOME", &home.to_string_lossy());
            Some(std::path::PathBuf::from(value))
        })
    });
    Some(configured.unwrap_or_else(|| home.join("Desktop")))
}

fn write_desktop_entry(path: &std::path::Path, content: &str) {
    let _ = fs::write(path, content);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(metadata) = fs::metadata(path) {
            let mut permissions = metadata.permissions();
            permissions.set_mode(0o755);
            let _ = fs::set_permissions(path, permissions);
        }
    }
    let _ = Command::new("gio").args(["set", &path.to_string_lossy(), "metadata::trusted", "true"]).output();
}

pub fn ensure_shortcut() {
    let Some(bin_dir) = local_bin_dir() else { return };
    let destination = bin_dir.join("ropelato-discord-gui");

    if let Ok(current) = std::env::current_exe() {
        if current != destination {
            let _ = fs::create_dir_all(&bin_dir);
            let _ = fs::remove_file(&destination);
            let _ = fs::copy(&current, &destination);
        }
    }
    if !destination.exists() {
        return;
    }

    let icon_path = data_dir().join("icone-app.png");
    if !icon_path.exists() {
        let _ = fs::create_dir_all(data_dir());
        let _ = fs::write(&icon_path, APP_ICON);
    }

    let entry = format!(
        "[Desktop Entry]\nType=Application\nName=Ropelato Discord\nComment=Corrige a região do Discord no Brasil\nExec=\"{}\"\nIcon={}\nStartupWMClass={}\nTerminal=false\nCategories=Network;Utility;\n",
        destination.display(),
        icon_path.display(),
        APP_ID
    );

    if let Some(apps_dir) = applications_dir() {
        let _ = fs::create_dir_all(&apps_dir);
        write_desktop_entry(&apps_dir.join(format!("{APP_ID}.desktop")), &entry);
    }

    if let Some(desktop) = desktop_dir() {
        if desktop.is_dir() {
            write_desktop_entry(&desktop.join("Ropelato Discord.desktop"), &entry);
        }
    }
}

#[derive(Clone)]
pub struct ProxyInUse {
    pub address: String,
    pub region: String,
    pub latency_ms: u64,
}

#[derive(Clone)]
pub struct Status {
    pub version: &'static str,
    pub state: &'static str,
    pub autostart: bool,
    pub fix_enabled: bool,
    pub healthy_proxies: u32,
    pub proxy_in_use: Option<ProxyInUse>,
    pub last_check_utc: Option<u64>,
}

#[derive(Clone)]
pub struct Connection {
    pub host: String,
    pub port: u16,
    pub route: &'static str,
}

const SERVICE_IMAGE_NAME: &str = "ropelato-discord";

fn service_running() -> bool {
    platform::is_running(SERVICE_IMAGE_NAME)
}

fn pool_data() -> (u32, Option<ProxyInUse>) {
    let Ok(log) = fs::read_to_string(log_path()) else {
        return (0, None);
    };

    let mut count = 0;
    let mut proxy = None;
    for line in log.lines().rev() {
        let text = line.trim();
        if count == 0 {
            if let Some((number, _)) = text.split_once(" proxies estrangeiros validados") {
                count = number.trim().parse().unwrap_or(0);
            }
        }
        if proxy.is_none() {
            let Some((address, rest)) = text.split_once(" (") else {
                continue;
            };
            let Some((region, latency)) = rest.split_once(") ") else {
                continue;
            };
            let Some(ms) = latency.strip_suffix("ms") else {
                continue;
            };
            let Ok(latency_ms) = ms.trim().parse() else {
                continue;
            };
            proxy = Some(ProxyInUse {
                address: address.to_string(),
                region: region.to_string(),
                latency_ms,
            });
        }
    }
    (count, proxy)
}

fn read_last_check() -> Option<u64> {
    fs::read_to_string(last_check_path()).ok()?.trim().parse().ok()
}

pub fn status() -> Status {
    let installed = installed_path().exists();
    let running = installed && service_running();
    let autostart = platform::autostart_active();
    let fix_enabled = platform::pac_active();
    let (mut count, proxy) = pool_data();
    let proxies_ready = ready_marker_path().exists();
    if proxies_ready && count == 0 {
        count = 1;
    }

    let state = if !installed || !running {
        "parado"
    } else if !fix_enabled {
        "pausado"
    } else if !proxies_ready {
        "sem_proxies"
    } else {
        "operacional"
    };

    Status {
        version: env!("CARGO_PKG_VERSION"),
        state,
        autostart,
        fix_enabled,
        healthy_proxies: count,
        proxy_in_use: proxy,
        last_check_utc: read_last_check(),
    }
}

pub fn connections() -> Vec<Connection> {
    let Ok(log) = fs::read_to_string(log_path()) else {
        return Vec::new();
    };
    log.lines()
        .rev()
        .filter_map(|line| {
            let text = line.trim();
            let (route, destination) = if let Some(destination) = text.strip_prefix("exterior  ") {
                ("exterior", destination)
            } else if let Some(destination) = text.strip_prefix("direto    ") {
                ("direto", destination)
            } else {
                return None;
            };
            let (host, port) = destination.rsplit_once(':')?;
            let port = port.parse().ok()?;
            (!host.is_empty()).then(|| Connection {
                host: host.to_string(),
                port,
                route,
            })
        })
        .take(20)
        .collect()
}

pub fn ensure_service() -> Result<(), String> {
    let destination = installed_path();
    if !destination.exists() {
        return Err(format!(
            "o serviço ainda não está instalado — rode `ropelato-discord instalar` num terminal (procurado em {})",
            destination.display()
        ));
    }
    if service_running() {
        return Ok(());
    }
    Command::new(&destination)
        .arg("rodar")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("não consegui subir o serviço: {e}"))
}

pub fn pause() -> Result<(), String> {
    if !installed_path().exists() {
        return Err("o serviço ainda não está instalado".into());
    }
    platform::disable_pac().map_err(|e| format!("não consegui pausar a correção: {e}"))
}

pub fn resume() -> Result<(), String> {
    if !installed_path().exists() {
        return Err("o serviço ainda não está instalado".into());
    }
    platform::enable_pac().map_err(|e| format!("não consegui retomar a correção: {e}"))
}

pub fn set_autostart(enabled: bool) -> Result<(), String> {
    let service = installed_path();
    if !service.exists() {
        return Err("o serviço ainda não está instalado".into());
    }
    if enabled {
        platform::enable_autostart(&format!("\"{}\" rodar", service.display()))
    } else {
        platform::disable_autostart(&service)
    }
    .map_err(|e| format!("não consegui mudar o autostart: {e}"))
}

pub fn restart_discord() -> Result<bool, String> {
    if !service_running() {
        return Err("o serviço precisa estar rodando antes de reiniciar o Discord".into());
    }
    discord::restart(&pac_url()).map_err(|e| format!("não consegui reiniciar o Discord: {e}"))
}

pub fn check() -> Result<Status, String> {
    ensure_service()?;
    for _ in 0..10 {
        let current = status();
        if current.state != "parado" {
            return Ok(current);
        }
        std::thread::sleep(std::time::Duration::from_millis(300));
    }
    Ok(status())
}

pub fn start_uninstall() -> Result<(), String> {
    let destination = installed_path();
    if !destination.exists() {
        return Err("o serviço ainda não está instalado".into());
    }
    Command::new(&destination)
        .arg("desinstalar")
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("não consegui abrir o desinstalador: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extrai_somente_as_rotas_reais_do_log() {
        let log = "reabastecendo a piscina\n\
exterior  gateway.discord.gg:443\n\
direto    cdn.discordapp.com:443\n\
conexão encerrada: early eof\n";
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("XDG_DATA_HOME", dir.path());
        fs::create_dir_all(log_path().parent().unwrap()).unwrap();
        fs::write(log_path(), log).unwrap();

        let connections = connections();
        assert_eq!(connections.len(), 2);
        assert_eq!(connections[0].host, "cdn.discordapp.com");
        assert_eq!(connections[0].route, "direto");
        assert_eq!(connections[1].host, "gateway.discord.gg");
        assert_eq!(connections[1].route, "exterior");
    }
}
