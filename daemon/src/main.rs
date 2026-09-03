use anyhow::{Context, Result};
use ropelato_discord_core::{
    data_dir, discord, installed_path, last_check_path, log_path, pac, pool, port_in_use,
    pool_ready, platform, ready_marker_path, session, socks, pac_url, PAC_PORT, SOCKS_PORT,
};
use std::{process::Stdio, time::Duration};

const MAINTENANCE_INTERVAL: Duration = Duration::from_secs(300);

const WATCHER_INTERVAL: Duration = Duration::from_secs(1);

#[derive(Debug, PartialEq, Eq)]
struct InstallOptions {
    restart_discord: bool,
    create_autostart: bool,
}

fn install_options(args: &[String]) -> InstallOptions {
    InstallOptions {
        restart_discord: !args.iter().any(|arg| arg == "--sem-reiniciar"),
        create_autostart: !args.iter().any(|arg| arg == "--sem-autostart"),
    }
}

fn keep_files(args: &[String]) -> bool {
    args.iter().any(|arg| arg == "--manter-arquivos")
}

fn now_millis() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

fn record_check_at(path: &std::path::Path, instant: u128) {
    let _ = std::fs::create_dir_all(path.parent().unwrap_or(path));
    let _ = std::fs::write(path, format!("{instant}\n"));
}

fn local_bin_dir() -> Option<std::path::PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(std::path::PathBuf::from(home).join(".local/bin"))
}

fn path_contains(dir: &std::path::Path) -> bool {
    std::env::var_os("PATH")
        .map(|path| std::env::split_paths(&path).any(|p| p == dir))
        .unwrap_or(false)
}

fn link_into_local_bin(destination: &std::path::Path) -> Result<std::path::PathBuf> {
    let dir = local_bin_dir().context("HOME não definido")?;
    std::fs::create_dir_all(&dir).context("criando ~/.local/bin")?;
    let link = dir.join("ropelato-discord");
    let _ = std::fs::remove_file(&link);
    std::os::unix::fs::symlink(destination, &link).context("criando link em ~/.local/bin")?;
    Ok(link)
}

fn unlink_from_local_bin() {
    if let Some(dir) = local_bin_dir() {
        let _ = std::fs::remove_file(dir.join("ropelato-discord"));
    }
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let command = args
        .iter()
        .find(|a| !a.starts_with("--"))
        .map(|s| s.as_str())
        .unwrap_or("ajuda");

    match command {
        "instalar" => install(install_options(&args)),
        "desinstalar" => uninstall(keep_files(&args)),
        "status" => status(),
        "reiniciar-discord" => restart_discord(),
        "rodar" => run(),
        _ => {
            help();
            Ok(())
        }
    }
}

fn help() {
    println!(
        "\nropelato-discord {}\n\n\
         Uso:\n  \
         ropelato-discord instalar      liga a correção, reinicia o Discord e sobe com a sessão\n  \
         ropelato-discord desinstalar   remove tudo, sem deixar rastro\n  \
         ropelato-discord status        mostra o estado atual\n  \
         ropelato-discord reiniciar-discord fecha e abre só o Discord\n  \
         ropelato-discord rodar         roda em primeiro plano (para depurar)\n\n\
         Opções:\n  \
         --sem-reiniciar           não mexe no Discord aberto; a correção vale na\n                            \
         próxima vez que você abrir\n  \
         --sem-autostart           não cria a entrada de autostart (uso da GUI)\n  \
         --manter-arquivos         limpa a configuração sem apagar a pasta instalada\n",
        env!("CARGO_PKG_VERSION")
    );
}

fn install(options: InstallOptions) -> Result<()> {
    let destination = installed_path();
    std::fs::create_dir_all(data_dir()).context("criando a pasta de dados")?;

    let current = std::env::current_exe()?;
    if current != destination {
        terminate_other_instances();
        std::fs::copy(&current, &destination).context("copiando o executável")?;
    }

    if options.create_autostart {
        platform::enable_autostart(&format!("\"{}\" rodar", destination.display())).context("registrando o autostart")?;
    }
    platform::enable_pac().context("ligando a correção")?;

    let _ = std::fs::remove_file(ready_marker_path());
    let _ = std::fs::remove_file(last_check_path());

    std::process::Command::new(&destination)
        .arg("rodar")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .context("subindo o serviço")?;

    print!("Validando proxies");
    let _ = std::io::Write::flush(&mut std::io::stdout());
    let mut ready = false;
    for _ in 0..15 {
        std::thread::sleep(Duration::from_secs(4));
        print!(".");
        let _ = std::io::Write::flush(&mut std::io::stdout());
        if port_in_use(SOCKS_PORT) && pool_ready() {
            ready = true;
            break;
        }
    }
    println!();
    if !ready {
        println!("\nNenhum proxy respondeu a tempo. O serviço continua tentando");
        println!("a cada 5 minutos — confira depois com `ropelato-discord status`.");
    }

    println!("\nInstalado.\n");
    println!("  executável : {}", destination.display());
    println!("  log        : {}", log_path().display());
    println!(
        "  autostart  : {}",
        if options.create_autostart { "sim" } else { "gerenciado pela GUI" }
    );
    println!("  PAC        : {}", pac_url());

    match link_into_local_bin(&destination) {
        Ok(link) => println!("  link       : {}", link.display()),
        Err(e) => println!("  link       : não consegui criar ({e}); use o caminho completo acima"),
    }

    if options.restart_discord {
        match discord::restart(&pac_url()) {
            Ok(true) => println!("\nDiscord reiniciado. Já está valendo."),
            Ok(false) => println!("\nDiscord não encontrado — a correção vale na próxima vez que você abrir."),
            Err(e) => println!("\nNão consegui reiniciar o Discord ({e}). Feche e abra ele uma vez."),
        }
    } else {
        println!("\nFeche e abra o Discord uma vez.");
    }

    match local_bin_dir() {
        Some(dir) if path_contains(&dir) => {
            println!("\nEm um terminal novo, o comando `ropelato-discord` já funciona sozinho.");
        }
        Some(dir) => {
            println!(
                "\n{} não está no seu PATH — adicione (ex.: `fish_add_path {}` no fish, ou \
                 `export PATH=\"{}:$PATH\"` no bash/zsh) para usar `ropelato-discord` direto. \
                 Até lá, use o caminho completo do executável acima.",
                dir.display(),
                dir.display(),
                dir.display()
            );
        }
        None => {}
    }
    Ok(())
}

fn uninstall(keep_files: bool) -> Result<()> {
    platform::validate_autostart_belongs_to_service(&installed_path())?;
    platform::disable_pac().context("desligando a correção")?;
    platform::disable_autostart(&installed_path()).context("removendo o autostart")?;
    terminate_other_instances();
    unlink_from_local_bin();

    let was_open = discord::terminate_if_open();
    if !keep_files {
        let _ = std::fs::remove_dir_all(data_dir());
    }

    println!(
        "{} A correção voltou ao que era antes.",
        if keep_files {
            "Configuração removida; os arquivos foram preservados para a GUI."
        } else {
            "Removido."
        }
    );
    if was_open {
        println!("O Discord foi fechado. Abra de novo e ele já sai pelo seu IP normal.");
    } else {
        println!("Na próxima abertura, o Discord já sai pelo seu IP normal.");
    }
    Ok(())
}

fn status() -> Result<()> {
    println!("\nropelato-discord {}\n", env!("CARGO_PKG_VERSION"));
    println!("  instalado  : {}", yes_no(installed_path().exists()));
    println!("  autostart  : {}", yes_no(platform::autostart_active()));
    println!("  correção   : {}", yes_no(platform::pac_active()));
    println!("  rodando    : {}", yes_no(port_in_use(SOCKS_PORT)));
    println!("  proxies    : {}", yes_no(pool_ready()));
    println!("  log        : {}", log_path().display());
    Ok(())
}

fn restart_discord() -> Result<()> {
    match discord::restart(&pac_url())? {
        true => println!("Discord reiniciado."),
        false => println!("Discord não encontrado."),
    }
    Ok(())
}

fn terminate_other_instances() {
    let me = std::process::id();
    let old: Vec<u32> = platform::pids_by_name("ropelato-discord").into_iter().filter(|pid| *pid != me).collect();
    platform::terminate_all(&old);
}

fn yes_no(b: bool) -> &'static str {
    if b {
        "sim"
    } else {
        "não"
    }
}

fn watch_session(session: std::sync::Arc<session::Session>, pool: pool::Pool) {
    std::thread::spawn(move || loop {
        let now = std::time::Instant::now();

        match session.observe_discord(discord::main_process(), now) {
            Some(session::Change::NewDiscord) => {
                socks::log::line("Discord novo no ar; a correção vale para esta sessão");
            }
            Some(session::Change::DiscordClosed) => {
                socks::log::line("Discord fechou; a janela reabre para a próxima abertura");
            }
            None => {}
        }

        if session.evaluate(now, pool.count() > 0) {
            let duration = session.armed_for(now).as_secs();
            socks::log::line(&format!("sessão aberta após {duration} s; o Discord volta a falar direto"));
        }

        std::thread::sleep(WATCHER_INTERVAL);
    });
}

fn run() -> Result<()> {
    let _ = std::fs::create_dir_all(data_dir());
    let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build()?;

    rt.block_on(async move {
        let pool = pool::Pool::new();
        let current_session = std::sync::Arc::new(session::Session::new(std::time::Instant::now()));
        watch_session(current_session.clone(), pool.clone());

        tokio::spawn({
            let p = pool.clone();
            async move {
                loop {
                    if p.count() < pool::MIN_HEALTHY {
                        socks::log::line("reabastecendo a piscina de proxies...");
                        match p.refill().await {
                            Ok(n) => {
                                socks::log::line(&format!("{n} proxies estrangeiros validados"));
                                for u in p.list().iter().take(3) {
                                    socks::log::line(&format!(
                                        "  {} ({}) {}ms",
                                        u.address,
                                        u.region,
                                        u.latency.as_millis()
                                    ));
                                }
                            }
                            Err(e) => socks::log::line(&format!("falha ao reabastecer: {e}")),
                        }
                    }

                    if p.count() > 0 {
                        let _ = std::fs::write(ready_marker_path(), b"");
                    } else {
                        let _ = std::fs::remove_file(ready_marker_path());
                    }

                    record_check_at(&last_check_path(), now_millis());

                    tokio::select! {
                        _ = tokio::time::sleep(MAINTENANCE_INTERVAL) => {}
                        _ = p.wait_drained() => {
                            socks::log::line("a piscina ficou magra; a manutenção acordou antes da hora");
                        }
                    }
                }
            }
        });

        tokio::spawn(async move {
            if let Err(e) = pac::serve(PAC_PORT, SOCKS_PORT).await {
                socks::log::line(&format!("servidor PAC caiu: {e}"));
            }
        });

        socks::serve(SOCKS_PORT, pool, current_session).await
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn setup_da_gui_instala_o_servico_sem_autostart_legado() {
        assert_eq!(
            install_options(&["instalar".into(), "--sem-reiniciar".into(), "--sem-autostart".into()]),
            InstallOptions {
                restart_discord: false,
                create_autostart: false,
            },
        );
    }

    #[test]
    fn cli_sem_novas_opcoes_mantem_o_autostart() {
        assert_eq!(
            install_options(&["instalar".into()]),
            InstallOptions {
                restart_discord: true,
                create_autostart: true,
            },
        );
    }

    #[test]
    fn desinstalar_com_manter_arquivos_nao_remove_a_pasta() {
        assert!(keep_files(&["desinstalar".into(), "--manter-arquivos".into()]));
        assert!(!keep_files(&["desinstalar".into()]));
    }

    #[test]
    fn a_manutencao_carimba_a_hora_que_a_gui_le() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("sub").join("ultima-validacao-ms");

        record_check_at(&path, 1_725_000_123_456);

        assert_eq!(std::fs::read_to_string(&path).unwrap(), "1725000123456\n");
        assert_eq!(
            std::fs::read_to_string(&path).unwrap().trim().parse::<u64>().unwrap(),
            1_725_000_123_456
        );
    }
}
