use crate::{
    pool::Pool,
    routing::{self, Route},
    session::Session,
};
use anyhow::{bail, Result};
use std::{net::SocketAddr, sync::Arc, time::Instant};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::broadcast::{self, error::TryRecvError},
    time::{timeout, Duration},
};

const DIRECT_TIMEOUT: Duration = Duration::from_secs(15);

const FOREIGN_TIMEOUT: Duration = Duration::from_secs(5);

const FOREIGN_ATTEMPTS: usize = 2;

pub async fn serve(port: u16, pool: Pool, session: Arc<Session>) -> Result<()> {
    let listener = TcpListener::bind(("127.0.0.1", port)).await?;
    log::line(&format!("proxy local escutando em 127.0.0.1:{port}"));

    loop {
        let (client, _) = listener.accept().await?;
        let pool = pool.clone();
        let session = session.clone();
        tokio::spawn(async move {
            let _ = client.set_nodelay(true);
            if let Err(e) = handle(client, pool, session).await {
                log::line(&format!("conexão encerrada: {e}"));
            }
        });
    }
}

async fn handle(mut client: TcpStream, pool: Pool, session: Arc<Session>) -> Result<()> {
    let (host, port) = handshake(&mut client).await?;

    let mut notice = session.subscribe_cancel();
    let route = routing::decide(&host, session.phase());

    let (server, cancel) = match route {
        Route::Foreign => {
            let opened = {
                let _handshake = session.begin_handshake(&host, Instant::now());
                open_via_foreign(&pool, &host, port).await
            };

            match opened {
                Ok(_) if closed_midway(&mut notice) => {
                    log::line(&format!("a janela fechou durante o aperto de mão; {host} vai direto"));
                    log::line(&format!("direto    {host}:{port}"));
                    (open_direct(&host, port).await?, None)
                }
                Ok(s) => {
                    log::line(&format!("exterior  {host}:{port}"));
                    (s, Some(notice))
                }
                Err(e) => {
                    log::line(&format!("exterior indisponível ({e}); {host} vai direto"));
                    log::line(&format!("direto    {host}:{port}"));
                    (open_direct(&host, port).await?, None)
                }
            }
        }
        Route::Direct => {
            log::line(&format!("direto    {host}:{port}"));
            (open_direct(&host, port).await?, None)
        }
    };

    respond_ok(&mut client).await?;

    let _gateway = routing::is_gateway(&host).then(|| OpenGateway::new(&session));
    forward(client, server, cancel).await
}

fn closed_midway(notice: &mut broadcast::Receiver<()>) -> bool {
    !matches!(notice.try_recv(), Err(TryRecvError::Empty))
}

struct OpenGateway<'a> {
    session: &'a Session,
}

impl<'a> OpenGateway<'a> {
    fn new(session: &'a Session) -> Self {
        if let Some(gap) = session.gateway_opened(Instant::now()) {
            log::line(&format!(
                "suspeita: gateway novo depois de {} s sem gateway nenhum, com a sessão já aberta; \
                 ela pode ter renascido pelo IP brasileiro — se a tela parar de funcionar, reinicie o Discord",
                gap.as_secs()
            ));
        }
        Self { session }
    }
}

impl Drop for OpenGateway<'_> {
    fn drop(&mut self) {
        self.session.gateway_closed(Instant::now());
    }
}

async fn handshake(client: &mut TcpStream) -> Result<(String, u16)> {
    let mut header = [0u8; 2];
    client.read_exact(&mut header).await?;
    if header[0] != 0x05 {
        bail!("versão SOCKS não suportada: {}", header[0]);
    }
    let mut methods = vec![0u8; header[1] as usize];
    client.read_exact(&mut methods).await?;
    client.write_all(&[0x05, 0x00]).await?;

    let mut req = [0u8; 4];
    client.read_exact(&mut req).await?;
    if req[1] != 0x01 {
        refuse(client, 0x07).await?;
        bail!("só CONNECT é suportado");
    }

    let host = match req[3] {
        0x01 => {
            let mut o = [0u8; 4];
            client.read_exact(&mut o).await?;
            format!("{}.{}.{}.{}", o[0], o[1], o[2], o[3])
        }
        0x03 => {
            let mut n = [0u8; 1];
            client.read_exact(&mut n).await?;
            let mut d = vec![0u8; n[0] as usize];
            client.read_exact(&mut d).await?;
            let name = String::from_utf8(d)?;
            if !valid_hostname(&name) {
                refuse(client, 0x08).await?;
                bail!("nome de host inválido no pedido CONNECT");
            }
            name
        }
        0x04 => {
            let mut o = [0u8; 16];
            client.read_exact(&mut o).await?;
            std::net::Ipv6Addr::from(o).to_string()
        }
        other => {
            refuse(client, 0x08).await?;
            bail!("tipo de endereço desconhecido: {other}");
        }
    };

    let mut p = [0u8; 2];
    client.read_exact(&mut p).await?;
    Ok((host, u16::from_be_bytes(p)))
}

fn valid_hostname(name: &str) -> bool {
    routing::valid_hostname(name)
}

async fn open_direct(host: &str, port: u16) -> Result<TcpStream> {
    let s = timeout(DIRECT_TIMEOUT, TcpStream::connect((host, port))).await??;
    let _ = s.set_nodelay(true);
    Ok(s)
}

async fn open_via_foreign(pool: &Pool, host: &str, port: u16) -> Result<TcpStream> {
    open_via_foreign_with_deadline(pool, host, port, FOREIGN_TIMEOUT).await
}

async fn open_via_foreign_with_deadline(
    pool: &Pool,
    host: &str,
    port: u16,
    deadline: Duration,
) -> Result<TcpStream> {
    let mut tried: Vec<String> = Vec::new();
    for _ in 0..FOREIGN_ATTEMPTS {
        let Some(upstream) = pool.best_except(&tried) else {
            if tried.is_empty() {
                bail!("piscina vazia");
            }
            bail!("nenhum outro upstream na piscina");
        };
        match chain(&upstream, host, port, deadline).await {
            Ok(s) => return Ok(s),
            Err(e) => {
                log::line(&format!("upstream {upstream} falhou: {e}"));
                pool.mark_failure(&upstream);
                tried.push(upstream);
            }
        }
    }
    bail!("nenhum upstream respondeu")
}

async fn chain(upstream: &str, host: &str, port: u16, deadline: Duration) -> Result<TcpStream> {
    match timeout(deadline, chain_without_deadline(upstream, host, port)).await {
        Ok(result) => result,
        Err(_) => bail!("não respondeu em {:.1} s", deadline.as_secs_f32()),
    }
}

async fn chain_without_deadline(upstream: &str, host: &str, port: u16) -> Result<TcpStream> {
    let address: SocketAddr = upstream.parse()?;
    let mut s = TcpStream::connect(address).await?;
    let _ = s.set_nodelay(true);

    s.write_all(&[0x05, 0x01, 0x00]).await?;
    let mut resp = [0u8; 2];
    s.read_exact(&mut resp).await?;
    if resp != [0x05, 0x00] {
        bail!("upstream recusou o método sem autenticação");
    }

    let h = host.as_bytes();
    if h.len() > 255 {
        bail!("host longo demais");
    }
    let mut request = vec![0x05, 0x01, 0x00, 0x03, h.len() as u8];
    request.extend_from_slice(h);
    request.extend_from_slice(&port.to_be_bytes());
    s.write_all(&request).await?;

    let mut header = [0u8; 4];
    s.read_exact(&mut header).await?;
    if header[1] != 0x00 {
        bail!("upstream recusou a conexão (código {})", header[1]);
    }
    match header[3] {
        0x01 => {
            let mut d = [0u8; 4];
            s.read_exact(&mut d).await?;
        }
        0x03 => {
            let mut n = [0u8; 1];
            s.read_exact(&mut n).await?;
            let mut d = vec![0u8; n[0] as usize];
            s.read_exact(&mut d).await?;
        }
        0x04 => {
            let mut d = [0u8; 16];
            s.read_exact(&mut d).await?;
        }
        other => bail!("resposta do upstream ilegível: atyp {other}"),
    }
    let mut p = [0u8; 2];
    s.read_exact(&mut p).await?;
    Ok(s)
}

async fn respond_ok(client: &mut TcpStream) -> Result<()> {
    client
        .write_all(&[0x05, 0x00, 0x00, 0x01, 0, 0, 0, 0, 0, 0])
        .await?;
    Ok(())
}

async fn refuse(client: &mut TcpStream, code: u8) -> Result<()> {
    client
        .write_all(&[0x05, code, 0x00, 0x01, 0, 0, 0, 0, 0, 0])
        .await?;
    Ok(())
}

async fn forward(
    mut a: TcpStream,
    mut b: TcpStream,
    cancel: Option<broadcast::Receiver<()>>,
) -> Result<()> {
    let Some(mut cancel) = cancel else {
        tokio::io::copy_bidirectional(&mut a, &mut b).await?;
        return Ok(());
    };

    tokio::select! {
        r = tokio::io::copy_bidirectional(&mut a, &mut b) => { r?; }
        _ = cancel.recv() => {
            let _ = a.shutdown().await;
            let _ = b.shutdown().await;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn pair() -> (TcpStream, TcpStream) {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let address = listener.local_addr().unwrap();
        let client = TcpStream::connect(address).await.unwrap();
        let (server, _) = listener.accept().await.unwrap();
        (client, server)
    }

    #[tokio::test]
    async fn fechar_a_janela_derruba_quem_saiu_pelo_exterior() {
        let (a, _guard_a) = pair().await;
        let (b, _guard_b) = pair().await;

        let (notify, listen) = broadcast::channel(1);
        let pumping = tokio::spawn(forward(a, b, Some(listen)));

        notify.send(()).unwrap();

        let end = timeout(Duration::from_secs(2), pumping).await;
        assert!(
            end.is_ok(),
            "a conexão devia ter caído assim que a janela fechou"
        );
        assert!(end.unwrap().unwrap().is_ok());
    }

    #[tokio::test]
    async fn conexao_direta_nao_e_derrubada_pela_janela() {
        let (a, _guard_a) = pair().await;
        let (b, _guard_b) = pair().await;

        let pumping = tokio::spawn(forward(a, b, None));

        let end = timeout(Duration::from_millis(300), pumping).await;
        assert!(end.is_err(), "conexão direta continua de pé");
    }

    #[tokio::test]
    async fn upstream_mudo_nao_prende_a_conexao() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let address = listener.local_addr().unwrap();
        let silent = tokio::spawn(async move {
            let (connection, _) = listener.accept().await.unwrap();
            tokio::time::sleep(Duration::from_secs(30)).await;
            drop(connection);
        });

        let start = Instant::now();
        let deadline = Duration::from_millis(300);
        let result = chain(&address.to_string(), "discord.com", 443, deadline).await;
        let elapsed = start.elapsed();
        silent.abort();

        assert!(result.is_err(), "um upstream mudo é uma falha, não uma espera");
        assert!(
            elapsed < Duration::from_secs(3),
            "desistiu em {elapsed:?}, muito depois do prazo de {deadline:?}"
        );
    }

    async fn accepting_upstream() -> SocketAddr {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            loop {
                let Ok((mut s, _)) = listener.accept().await else { break };
                tokio::spawn(async move {
                    let mut greeting = [0u8; 3];
                    if s.read_exact(&mut greeting).await.is_err() {
                        return;
                    }
                    let _ = s.write_all(&[0x05, 0x00]).await;
                    let mut header = [0u8; 5];
                    if s.read_exact(&mut header).await.is_err() {
                        return;
                    }
                    let mut rest = vec![0u8; header[4] as usize + 2];
                    if s.read_exact(&mut rest).await.is_err() {
                        return;
                    }
                    let _ = s.write_all(&[0x05, 0x00, 0x00, 0x01, 0, 0, 0, 0, 0, 0]).await;
                    let mut trash = [0u8; 1];
                    let _ = s.read(&mut trash).await;
                });
            }
        });
        address
    }

    #[tokio::test]
    async fn a_segunda_tentativa_vai_para_outro_upstream() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let silent = listener.local_addr().unwrap();
        let holding = tokio::spawn(async move {
            let (connection, _) = listener.accept().await.unwrap();
            tokio::time::sleep(Duration::from_secs(30)).await;
            drop(connection);
        });
        let good = accepting_upstream().await;
        let pool = Pool::for_test(&[&silent.to_string(), &good.to_string()]);

        let start = Instant::now();
        let result =
            open_via_foreign_with_deadline(&pool, "discord.com", 443, Duration::from_millis(300))
                .await;
        holding.abort();

        assert!(result.is_ok(), "o segundo upstream devia ter atendido: {result:?}");
        assert!(start.elapsed() < Duration::from_secs(3));
        assert_eq!(pool.count(), 2, "uma falha só ainda perdoa o mudo");
        assert_eq!(
            pool.best().as_deref(),
            Some(silent.to_string().as_str()),
            "o mudo continua na frente da fila até a segunda falha"
        );
    }

    #[test]
    fn nome_de_host_com_byte_de_controle_e_recusado() {
        assert!(valid_hostname("discord.com"));
        assert!(valid_hostname("gateway-us-east1-b.discord.gg"));
        assert!(valid_hostname("_sip._tcp.exemplo.com"));
        for h in ["evil.com\0.discord.com", "x\nexterior  google.com", "a b.discord.com", ""] {
            assert!(!valid_hostname(h), "{h:?}");
        }
    }

    #[test]
    fn a_janela_que_fechou_durante_o_aperto_de_mao_e_percebida() {
        let (notify, mut listen) = broadcast::channel(1);
        assert!(!closed_midway(&mut listen), "nada aconteceu ainda");

        notify.send(()).unwrap();
        assert!(
            closed_midway(&mut listen),
            "a janela fechou entre a assinatura e o fim do aperto de mão"
        );
    }
}

pub mod log {
    use std::{
        io::Write,
        sync::{Mutex, OnceLock},
    };

    const MAX_SIZE: u64 = 512 * 1024;

    fn lock() -> &'static Mutex<()> {
        static L: OnceLock<Mutex<()>> = OnceLock::new();
        L.get_or_init(|| Mutex::new(()))
    }

    pub fn line(msg: &str) {
        let _guard = lock().lock();
        let path = crate::log_path();
        let now = chrono::Local::now();
        let timestamped = format!("[{}:{:03}] {msg}", now.format("%H:%M:%S"), now.timestamp_subsec_millis());

        if std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0) > MAX_SIZE {
            let _ = std::fs::write(&path, b"");
        }

        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
        {
            let _ = writeln!(f, "{timestamped}");
        }
        println!("{timestamped}");
    }

    pub fn clear() -> std::io::Result<()> {
        let _guard = lock().lock();
        std::fs::write(crate::log_path(), b"")
    }
}
