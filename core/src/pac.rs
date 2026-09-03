use anyhow::Result;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

pub fn text(socks_port: u16) -> String {
    format!(
        r#"function FindProxyForURL(url, host) {{
  if (dnsDomainIs(host, ".discord.com")      || host == "discord.com"      ||
      dnsDomainIs(host, ".discord.gg")       || host == "discord.gg"       ||
      dnsDomainIs(host, ".discord.media")    ||
      dnsDomainIs(host, ".discordapp.com")   || host == "discordapp.com")
    return "SOCKS5 127.0.0.1:{socks_port}";
  return "DIRECT";
}}
"#
    )
}

pub async fn serve(port: u16, socks_port: u16) -> Result<()> {
    let listener = TcpListener::bind(("127.0.0.1", port)).await?;
    crate::socks::log::line(&format!("PAC servido em http://127.0.0.1:{port}/proxy.pac"));
    let body = text(socks_port);

    loop {
        let (connection, _) = listener.accept().await?;
        let body = body.clone();
        tokio::spawn(async move {
            let _ = respond(connection, body).await;
        });
    }
}

async fn respond(mut c: TcpStream, body: String) -> Result<()> {
    let mut trash = [0u8; 1024];
    let _ = c.read(&mut trash).await;
    let resp = format!(
        "HTTP/1.1 200 OK\r\n\
         Content-Type: application/x-ns-proxy-autoconfig\r\n\
         Content-Length: {}\r\n\
         Cache-Control: no-store\r\n\
         Connection: close\r\n\r\n{}",
        body.len(),
        body
    );
    c.write_all(resp.as_bytes()).await?;
    c.shutdown().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pac_manda_discord_pro_proxy_e_o_resto_direto() {
        let p = text(9250);
        assert!(p.contains("SOCKS5 127.0.0.1:9250"));
        assert!(p.contains(".discord.com"));
        assert!(p.contains(".discord.media"));
        assert!(p.trim_end().ends_with('}'));
        assert!(p.contains("return \"DIRECT\""));
    }
}
