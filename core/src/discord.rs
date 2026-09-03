use anyhow::Result;
use std::{process::Command, time::Duration};

use crate::{platform, platform::Process, session::Identity};

const IMAGE_NAME: &str = "Discord";

pub fn is_running() -> bool {
    platform::is_running(IMAGE_NAME)
}

pub fn main_process() -> Option<Identity> {
    main_among(&platform::processes_by_name(IMAGE_NAME), platform::created_at)
}

fn main_among(processes: &[Process], created_at: impl Fn(u32) -> Option<u64>) -> Option<Identity> {
    let born: Vec<(Process, Option<u64>)> = processes.iter().map(|p| (*p, created_at(p.pid))).collect();

    let is_real_parent = |parent: u32, child_born: Option<u64>| {
        born.iter().any(|(q, q_born)| {
            q.pid == parent
                && match (q_born, child_born) {
                    (Some(parent_born), Some(child_born)) => *parent_born <= child_born,
                    _ => true,
                }
        })
    };

    let oldest = |(p, born): &&(Process, Option<u64>)| (born.is_none(), born.unwrap_or(0), p.pid);

    let root = born
        .iter()
        .filter(|(p, born)| !is_real_parent(p.parent, *born))
        .min_by_key(oldest)
        .or_else(|| born.iter().min_by_key(oldest))?;

    Some(Identity {
        pid: root.0.pid,
        created_at: root.1.unwrap_or(0),
    })
}

fn terminate() {
    platform::terminate_by_name(IMAGE_NAME);
    std::thread::sleep(Duration::from_millis(500));
}

pub fn restart(pac_url: &str) -> Result<bool> {
    let Some((executable, args)) = platform::discord_launcher(pac_url) else {
        return Ok(false);
    };
    let was_open = is_running();
    if was_open {
        terminate();
    }
    Command::new(executable)
        .args(args)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()?;
    Ok(true)
}

pub fn terminate_if_open() -> bool {
    if is_running() {
        terminate();
        true
    } else {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(pid: u32, parent: u32) -> Process {
        Process { pid, parent }
    }

    fn common_tree() -> Vec<Process> {
        vec![
            p(4000, 3990),
            p(4100, 4000),
            p(4200, 4000),
            p(4300, 4000),
            p(4400, 4000),
            p(4500, 4000),
            p(4600, 4000),
        ]
    }

    #[test]
    fn o_principal_e_quem_nao_tem_pai_discord() {
        let main = main_among(&common_tree(), |pid| Some(u64::from(pid) * 10));
        assert_eq!(
            main,
            Some(Identity {
                pid: 4000,
                created_at: 40_000
            })
        );
    }

    #[test]
    fn filho_novo_nao_muda_o_principal() {
        let mut tree = common_tree();
        let before = main_among(&tree, |_| Some(1));

        tree.retain(|p| p.pid != 4300);
        tree.push(p(4700, 4000));
        assert_eq!(main_among(&tree, |_| Some(1)), before);
    }

    #[test]
    fn sem_discord_nao_ha_principal() {
        assert_eq!(main_among(&[], |_| Some(1)), None);
    }

    #[test]
    fn sem_hora_de_criacao_o_pid_ainda_identifica() {
        let main = main_among(&common_tree(), |_| None);
        assert_eq!(main.map(|i| i.pid), Some(4000));
    }

    #[test]
    fn pid_do_lancador_reaproveitado_por_um_filho_nao_esconde_o_principal() {
        let tree = vec![p(4000, 3990), p(3990, 4000), p(4200, 4000), p(4300, 4000)];
        let hour = |pid: u32| Some(if pid == 4000 { 100 } else { 105 + u64::from(pid) });
        assert_eq!(
            main_among(&tree, hour),
            Some(Identity {
                pid: 4000,
                created_at: 100
            })
        );

        let without_hour = main_among(&tree, |_| None);
        assert!(without_hour.is_some(), "Discord de pé nunca vira None");
        assert_eq!(main_among(&tree, |_| None), without_hour);
    }

    #[test]
    fn raiz_sem_hora_nao_passa_na_frente_da_raiz_com_hora() {
        let tree = vec![p(4000, 1), p(9000, 2)];
        let hour = |pid: u32| (pid == 4000).then_some(500);
        assert_eq!(main_among(&tree, hour).map(|i| i.pid), Some(4000));
    }

    #[test]
    fn com_o_principal_morto_os_filhos_ainda_dao_uma_identidade_estavel() {
        let orphans = vec![p(4100, 4000), p(4200, 4000), p(4300, 4000)];
        let hour = |pid: u32| Some(u64::from(pid));
        assert_eq!(main_among(&orphans, hour).map(|i| i.pid), Some(4100));
        assert_eq!(main_among(&orphans, hour).map(|i| i.pid), Some(4100));
    }
}
