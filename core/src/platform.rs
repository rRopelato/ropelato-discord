use anyhow::{bail, Result};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    thread,
    time::{Duration, Instant},
};

fn xdg_dir(env_var: &str, fallback: &str) -> PathBuf {
    match std::env::var(env_var) {
        Ok(dir) if !dir.is_empty() => PathBuf::from(dir),
        _ => {
            let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
            PathBuf::from(home).join(fallback)
        }
    }
}

pub fn data_dir() -> PathBuf {
    xdg_dir("XDG_DATA_HOME", ".local/share").join("ropelato-discord")
}

pub fn config_dir() -> PathBuf {
    xdg_dir("XDG_CONFIG_HOME", ".config").join("ropelato-discord")
}

fn autostart_dir() -> PathBuf {
    xdg_dir("XDG_CONFIG_HOME", ".config").join("autostart")
}

fn paused_marker_path() -> PathBuf {
    config_dir().join("pausado")
}

pub fn enable_pac() -> Result<()> {
    let _ = fs::remove_file(paused_marker_path());
    Ok(())
}

pub fn disable_pac() -> Result<()> {
    fs::create_dir_all(config_dir())?;
    fs::write(paused_marker_path(), b"")?;
    Ok(())
}

pub fn pac_active() -> bool {
    !paused_marker_path().exists()
}

const AUTOSTART_FILE_NAME: &str = "ropelato-discord.desktop";

fn autostart_path() -> PathBuf {
    autostart_dir().join(AUTOSTART_FILE_NAME)
}

pub fn enable_autostart(command: &str) -> Result<()> {
    fs::create_dir_all(autostart_dir())?;
    let content = format!(
        "[Desktop Entry]\nType=Application\nName=Ropelato Discord\nExec={command}\nX-GNOME-Autostart-enabled=true\nNoDisplay=true\nTerminal=false\n"
    );
    fs::write(autostart_path(), content)?;
    Ok(())
}

fn entry_belongs_to_service(content: &str, service: &Path) -> bool {
    let expected = service.display().to_string();
    content
        .lines()
        .any(|line| line.strip_prefix("Exec=").is_some_and(|exec| exec.contains(&expected)))
}

pub fn validate_autostart_belongs_to_service(service: &Path) -> Result<()> {
    let Ok(content) = fs::read_to_string(autostart_path()) else {
        return Ok(());
    };
    if !entry_belongs_to_service(&content, service) {
        bail!("a entrada de autostart não pertence ao serviço instalado pelo Ropelato Discord")
    }
    Ok(())
}

pub fn disable_autostart(service: &Path) -> Result<()> {
    validate_autostart_belongs_to_service(service)?;
    let _ = fs::remove_file(autostart_path());
    Ok(())
}

pub fn autostart_active() -> bool {
    autostart_path().is_file()
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Process {
    pub pid: u32,
    pub parent: u32,
}

fn read_stat(pid: u32) -> Option<(String, u32, u64)> {
    let text = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let open = text.find('(')?;
    let close = text.rfind(')')?;
    let comm = text.get(open + 1..close)?.to_string();
    let rest: Vec<&str> = text.get(close + 2..)?.split_whitespace().collect();
    let parent = rest.get(1)?.parse().ok()?;
    let start = rest.get(19)?.parse().ok()?;
    Some((comm, parent, start))
}

const KERNEL_COMM_MAX_LEN: usize = 15;

pub fn processes_by_name(name: &str) -> Vec<Process> {
    let truncated_name = &name[..name.len().min(KERNEL_COMM_MAX_LEN)];
    let Ok(entries) = fs::read_dir("/proc") else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter_map(|entry| entry.file_name().to_str()?.parse::<u32>().ok())
        .filter_map(|pid| {
            let (comm, parent, _) = read_stat(pid)?;
            comm.eq_ignore_ascii_case(truncated_name).then_some(Process { pid, parent })
        })
        .collect()
}

pub fn created_at(pid: u32) -> Option<u64> {
    read_stat(pid).map(|(_, _, start)| start)
}

const GRACEFUL_WAIT_TIMEOUT: Duration = Duration::from_secs(5);

fn process_exists(pid: u32) -> bool {
    Path::new(&format!("/proc/{pid}")).exists()
}

fn send_signal(pid: u32, signal: &str) {
    let _ = Command::new("kill")
        .args([signal, &pid.to_string()])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
}

pub fn terminate_all(pids: &[u32]) {
    for &pid in pids {
        send_signal(pid, "-TERM");
    }

    let deadline = Instant::now() + GRACEFUL_WAIT_TIMEOUT;
    let mut remaining: Vec<u32> = pids.to_vec();
    while Instant::now() < deadline {
        remaining.retain(|&pid| process_exists(pid));
        if remaining.is_empty() {
            return;
        }
        thread::sleep(Duration::from_millis(100));
    }

    for &pid in &remaining {
        if process_exists(pid) {
            send_signal(pid, "-KILL");
        }
    }
}

pub fn pids_by_name(name: &str) -> Vec<u32> {
    processes_by_name(name).into_iter().map(|p| p.pid).collect()
}

pub fn terminate_by_name(name: &str) {
    terminate_all(&pids_by_name(name));
}

pub fn is_running(name: &str) -> bool {
    !pids_by_name(name).is_empty()
}

const NAMES_IN_PATH: &[&str] = &["discord", "Discord"];
const FIXED_PATHS: &[&str] = &["/usr/bin/discord", "/opt/discord/Discord", "/usr/lib/discord/Discord"];

fn in_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path).find_map(|dir| {
        let candidate = dir.join(name);
        candidate.is_file().then_some(candidate)
    })
}

pub fn find_discord() -> Option<PathBuf> {
    NAMES_IN_PATH
        .iter()
        .find_map(|name| in_path(name))
        .or_else(|| FIXED_PATHS.iter().map(PathBuf::from).find(|p| p.is_file()))
}

pub fn discord_launcher(pac_url: &str) -> Option<(PathBuf, Vec<String>)> {
    find_discord().map(|path| (path, vec![format!("--proxy-pac-url={pac_url}")]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_ppid_and_starttime_from_a_real_stat() {
        let stat = "1234 (Discord Helper) S 999 1234 1234 0 -1 4194560 1234 0 0 0 10 2 0 0 20 0 4 0 555666 0 0 18446744073709551615 0 0 0 0 0 0 0 0 0 0 0 0 17 3 0 0 0 0 0";
        let comm = &stat[stat.find('(').unwrap() + 1..stat.rfind(')').unwrap()];
        assert_eq!(comm, "Discord Helper");
        let rest: Vec<&str> = stat[stat.rfind(')').unwrap() + 2..].split_whitespace().collect();
        assert_eq!(rest[1], "999");
        assert_eq!(rest[19], "555666");
    }

    #[test]
    fn autostart_entry_recognizes_its_own_path() {
        let service = Path::new("/home/ana/.local/share/ropelato-discord/ropelato-discord");
        let content = "[Desktop Entry]\nExec=\"/home/ana/.local/share/ropelato-discord/ropelato-discord\" rodar\n";
        assert!(entry_belongs_to_service(content, service));
        assert!(!entry_belongs_to_service("[Desktop Entry]\nExec=/usr/bin/outro\n", service));
    }
}
