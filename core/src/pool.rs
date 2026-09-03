use anyhow::{anyhow, Result};
use std::{
    collections::HashSet,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::{sync::Notify, task::JoinSet};

const LISTS: &[&str] = &[
    "https://raw.githubusercontent.com/monosans/proxy-list/main/proxies/socks5.txt",
    "https://raw.githubusercontent.com/TheSpeedX/PROXY-List/master/socks5.txt",
    "https://api.proxyscrape.com/v4/free-proxy-list/get?request=display_proxies&protocol=socks5&proxy_format=ipport&format=text",
];

const PROBE: &str = "https://latency.discord.media/rtc";

const HEALTHY_TARGET: usize = 5;
const CONCURRENT_VALIDATIONS: usize = 60;
const VALIDATION_TIMEOUT: Duration = Duration::from_secs(8);

pub const MIN_HEALTHY: usize = 3;

const USER_AGENT: &str = concat!("Ropelato-Discord/", env!("CARGO_PKG_VERSION"));

#[derive(Clone, Debug)]
pub struct Upstream {
    pub address: String,
    pub latency: Duration,
    pub region: String,
    failures: u32,
}

#[derive(Clone)]
pub struct Pool {
    healthy: Arc<Mutex<Vec<Upstream>>>,
    drained: Arc<Notify>,
}

impl Pool {
    pub fn new() -> Self {
        Self {
            healthy: Arc::new(Mutex::new(Vec::new())),
            drained: Arc::new(Notify::new()),
        }
    }

    #[cfg(test)]
    pub fn best(&self) -> Option<String> {
        self.best_except(&[])
    }

    pub fn best_except(&self, except: &[String]) -> Option<String> {
        self.healthy
            .lock()
            .ok()?
            .iter()
            .find(|u| !except.contains(&u.address))
            .map(|u| u.address.clone())
    }

    pub fn count(&self) -> usize {
        self.healthy.lock().map(|v| v.len()).unwrap_or(0)
    }

    pub fn list(&self) -> Vec<Upstream> {
        self.healthy.lock().map(|v| v.clone()).unwrap_or_default()
    }

    pub fn mark_failure(&self, address: &str) {
        let low = match self.healthy.lock() {
            Ok(mut v) => {
                if let Some(u) = v.iter_mut().find(|u| u.address == address) {
                    u.failures += 1;
                }
                v.retain(|u| u.failures < 2);
                v.len() < MIN_HEALTHY
            }
            Err(_) => false,
        };
        if low {
            self.drained.notify_one();
        }
    }

    pub async fn wait_drained(&self) {
        loop {
            self.drained.notified().await;
            if self.count() < MIN_HEALTHY {
                return;
            }
        }
    }

    fn set(&self, mut new_list: Vec<Upstream>) {
        new_list.sort_by_key(|u| u.latency);
        if let Ok(mut v) = self.healthy.lock() {
            *v = new_list;
        }
    }

    pub async fn refill(&self) -> Result<usize> {
        let candidates = download_lists().await?;
        if candidates.is_empty() {
            return Err(anyhow!("nenhum candidato nas listas públicas"));
        }

        let found = Arc::new(Mutex::new(Vec::<Upstream>::new()));
        let mut queue = JoinSet::new();
        let mut iter = candidates.into_iter();

        for _ in 0..CONCURRENT_VALIDATIONS {
            if let Some(c) = iter.next() {
                queue.spawn(validate(c));
            }
        }

        while let Some(res) = queue.join_next().await {
            if let Ok(Some(u)) = res {
                let mut b = found.lock().unwrap();
                b.push(u);
                if b.len() >= HEALTHY_TARGET * 3 {
                    break;
                }
            }
            if let Some(c) = iter.next() {
                queue.spawn(validate(c));
            }
        }
        queue.abort_all();

        let results = Arc::try_unwrap(found)
            .map(|m| m.into_inner().unwrap())
            .unwrap_or_default();
        let n = results.len();
        self.set(results);
        Ok(n)
    }
}

async fn download_lists() -> Result<Vec<String>> {
    let client = reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .timeout(Duration::from_secs(30))
        .build()?;

    let mut seen = HashSet::new();
    let mut out = Vec::new();

    for url in LISTS {
        let Ok(resp) = client.get(*url).send().await else {
            continue;
        };
        let Ok(text) = resp.text().await else { continue };
        for line in text.lines() {
            let l = line.trim();
            if looks_like_ip_port(l) && seen.insert(l.to_string()) {
                out.push(l.to_string());
            }
        }
    }
    Ok(out)
}

fn looks_like_ip_port(s: &str) -> bool {
    let Some((ip, port)) = s.rsplit_once(':') else {
        return false;
    };
    port.parse::<u16>().is_ok()
        && ip.split('.').count() == 4
        && ip.split('.').all(|o| o.parse::<u8>().is_ok())
}

async fn validate(address: String) -> Option<Upstream> {
    let client = reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .proxy(reqwest::Proxy::all(format!("socks5h://{address}")).ok()?)
        .timeout(VALIDATION_TIMEOUT)
        .build()
        .ok()?;

    let t0 = Instant::now();
    let resp = client.get(PROBE).send().await.ok()?;
    let regions: Vec<serde_json::Value> = resp.json().await.ok()?;
    let latency = t0.elapsed();

    let first = regions.first()?.get("region")?.as_str()?.to_string();
    if first == "brazil" {
        return None;
    }

    Some(Upstream {
        address,
        latency,
        region: first,
        failures: 0,
    })
}

#[cfg(test)]
impl Pool {
    pub fn for_test(addresses: &[&str]) -> Self {
        let p = Pool::new();
        p.set(
            addresses
                .iter()
                .enumerate()
                .map(|(i, e)| Upstream {
                    address: e.to_string(),
                    latency: Duration::from_millis(100 * (i as u64 + 1)),
                    region: "rotterdam".into(),
                    failures: 0,
                })
                .collect(),
        );
        p
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn upstream(address: &str, latency_ms: u64) -> Upstream {
        Upstream {
            address: address.into(),
            latency: Duration::from_millis(latency_ms),
            region: "rotterdam".into(),
            failures: 0,
        }
    }

    #[test]
    fn a_segunda_escolha_pula_quem_acabou_de_falhar() {
        let p = Pool::for_test(&["a:1080", "b:1080"]);
        assert_eq!(p.best().as_deref(), Some("a:1080"));

        p.mark_failure("a:1080");
        assert_eq!(p.best().as_deref(), Some("a:1080"));
        assert_eq!(p.best_except(&["a:1080".into()]).as_deref(), Some("b:1080"));
        assert_eq!(p.best_except(&["a:1080".into(), "b:1080".into()]), None);
    }

    #[tokio::test]
    async fn aviso_velho_nao_acorda_a_manutencao_com_a_piscina_cheia() {
        let p = Pool::for_test(&["a:1080"]);

        p.mark_failure("a:1080");
        p.set(
            (0..MIN_HEALTHY + 1)
                .map(|i| upstream(&format!("10.0.0.{i}:1080"), 100))
                .collect(),
        );
        assert!(
            tokio::time::timeout(Duration::from_millis(100), p.wait_drained())
                .await
                .is_err(),
            "com a piscina cheia não há o que reabastecer"
        );
    }

    #[test]
    fn reconhece_ip_porta() {
        assert!(looks_like_ip_port("5.255.99.75:1080"));
        assert!(!looks_like_ip_port("exemplo.com:1080"));
        assert!(!looks_like_ip_port("5.255.99.75"));
        assert!(!looks_like_ip_port("5.255.99.999:1080"));
        assert!(!looks_like_ip_port("5.255.99.75:99999"));
    }

    #[test]
    fn duas_falhas_removem_da_fila() {
        let p = Pool::new();
        p.set(vec![upstream("1.2.3.4:1080", 100)]);
        assert_eq!(p.count(), 1);
        p.mark_failure("1.2.3.4:1080");
        assert_eq!(p.count(), 1, "uma falha ainda perdoa");
        p.mark_failure("1.2.3.4:1080");
        assert_eq!(p.count(), 0, "duas falhas eliminam");
    }

    #[tokio::test]
    async fn secar_acorda_a_manutencao() {
        let p = Pool::new();
        p.set(vec![upstream("1.2.3.4:1080", 100)]);

        p.mark_failure("1.2.3.4:1080");
        assert!(
            tokio::time::timeout(Duration::from_millis(200), p.wait_drained())
                .await
                .is_ok(),
            "abaixo do mínimo a manutenção tem que acordar"
        );
    }

    #[tokio::test]
    async fn piscina_cheia_nao_acorda_a_manutencao() {
        let p = Pool::new();
        p.set(
            (0..MIN_HEALTHY + 1)
                .map(|i| upstream(&format!("10.0.0.{i}:1080"), 100))
                .collect(),
        );

        p.mark_failure("10.0.0.0:1080");
        assert!(
            tokio::time::timeout(Duration::from_millis(100), p.wait_drained())
                .await
                .is_err(),
            "uma falha numa piscina cheia não é motivo para reabastecer"
        );
    }

    #[test]
    fn melhor_e_o_de_menor_latencia() {
        let p = Pool::new();
        p.set(vec![
            {
                let mut u = upstream("lento:1080", 900);
                u.region = "frankfurt".into();
                u
            },
            upstream("rapido:1080", 120),
        ]);
        assert_eq!(p.best().unwrap(), "rapido:1080");
    }
}
