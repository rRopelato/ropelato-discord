pub mod discord;
pub mod pac;
pub mod platform;
pub mod pool;
pub mod routing;
pub mod session;
pub mod socks;

use std::path::PathBuf;

pub const SOCKS_PORT: u16 = 9250;
pub const PAC_PORT: u16 = 9251;

pub fn pac_url() -> String {
    format!("http://127.0.0.1:{PAC_PORT}/proxy.pac")
}

pub fn data_dir() -> PathBuf {
    platform::data_dir()
}

pub fn log_path() -> PathBuf {
    data_dir().join("ropelato-discord.log")
}

pub fn ready_marker_path() -> PathBuf {
    data_dir().join("pronto")
}

pub fn last_check_path() -> PathBuf {
    data_dir().join("ultima-validacao-ms")
}

pub fn installed_path() -> PathBuf {
    data_dir().join("ropelato-discord")
}

pub fn pool_ready() -> bool {
    ready_marker_path().exists()
}

pub fn port_in_use(port: u16) -> bool {
    std::net::TcpStream::connect_timeout(
        &format!("127.0.0.1:{port}").parse().unwrap(),
        std::time::Duration::from_millis(300),
    )
    .is_ok()
}
