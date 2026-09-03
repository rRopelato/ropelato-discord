use crate::session::Phase;

const REGION_DECIDING_HOSTS: &[&str] = &["discord.com", "gateway.discord.gg", "latency.discord.media"];

const STATUS_PAGE: &str = "status.discord.com";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Route {
    Foreign,
    Direct,
}

fn matches(host: &str, domain: &str) -> bool {
    host == domain || host.ends_with(&format!(".{domain}"))
}

fn normalize(host: &str) -> String {
    host.trim_end_matches('.').to_ascii_lowercase()
}

pub fn valid_hostname(host: &str) -> bool {
    !host.is_empty()
        && host.len() <= 253
        && host
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_'))
}

pub fn decide(host: &str, phase: Phase) -> Route {
    if phase == Phase::Established {
        return Route::Direct;
    }

    let host = normalize(host);
    if !valid_hostname(&host) {
        return Route::Direct;
    }
    if decides_region(&host) {
        Route::Foreign
    } else {
        Route::Direct
    }
}

pub fn is_gateway(host: &str) -> bool {
    let host = normalize(host);
    matches(&host, "discord.gg")
        && host
            .split('.')
            .next()
            .is_some_and(|label| label.starts_with("gateway"))
}

pub fn decides_region(host: &str) -> bool {
    let host = normalize(host);
    if matches(&host, STATUS_PAGE) {
        return false;
    }
    REGION_DECIDING_HOSTS.iter().any(|d| matches(&host, d)) || is_gateway(&host)
}

#[cfg(test)]
mod tests {
    use super::*;

    const FULL_DISCORD: &[&str] = &[
        "discord.com",
        "gateway.discord.gg",
        "gateway-us-east1-b.discord.gg",
        "latency.discord.media",
        "cdn.discordapp.com",
        "status.discord.com",
        "discord.gg",
        "DISCORD.COM.",
    ];

    #[test]
    fn na_abertura_so_quem_decide_regiao_sai_por_fora() {
        for h in ["discord.com", "gateway.discord.gg", "gateway-us-east1-b.discord.gg", "latency.discord.media", "DISCORD.COM."] {
            assert_eq!(decide(h, Phase::Opening), Route::Foreign, "{h}");
        }
    }

    #[test]
    fn cdn_e_status_vao_direto_mesmo_na_abertura() {
        for h in ["cdn.discordapp.com", "status.discord.com", "discord.gg", "discordapp.com"] {
            assert_eq!(decide(h, Phase::Opening), Route::Direct, "{h}");
        }
    }

    #[test]
    fn voz_vai_direto_mesmo_na_abertura() {
        for h in [
            "c-gru17-851904d3.discord.media",
            "c-gru18-6fa2a6cb.discord.media",
            "discord.media",
        ] {
            assert_eq!(decide(h, Phase::Opening), Route::Direct, "{h}");
        }
    }

    #[test]
    fn resto_da_internet_vai_direto() {
        for h in [
            "google.com",
            "api.spotify.com",
            "discord.com.evil.net",
            "media.discordapp.net",
        ] {
            assert_eq!(decide(h, Phase::Opening), Route::Direct, "{h}");
        }
    }

    #[test]
    fn nao_confunde_sufixo() {
        for h in ["naodiscord.com", "meudiscord.gg", "xdiscord.media"] {
            assert_eq!(decide(h, Phase::Opening), Route::Direct, "{h}");
        }
    }

    #[test]
    fn nome_mal_formado_nunca_sai_por_fora() {
        for h in [
            "evil.com\0.discord.com",
            "evil.com\n.discord.com",
            "a b.discord.com",
            "",
        ] {
            assert_eq!(decide(h, Phase::Opening), Route::Direct, "{h:?}");
        }
    }

    #[test]
    fn com_a_sessao_aberta_tudo_vai_direto() {
        for h in FULL_DISCORD {
            assert_eq!(decide(h, Phase::Established), Route::Direct, "{h}");
        }
    }

    #[test]
    fn so_quem_decide_regiao_alimenta_o_relogio() {
        for h in [
            "discord.com",
            "Discord.com",
            "gateway.discord.gg",
            "gateway-us-east1-b.discord.gg",
            "latency.discord.media",
        ] {
            assert!(decides_region(h), "{h} decide a região");
        }

        for h in [
            "cdn.discordapp.com",
            "c-gru17-851904d3.discord.media",
            "status.discord.com",
            "discord.gg",
            "discordapp.com",
            "remote-auth-gateway.discord.gg",
            "google.com",
        ] {
            assert!(!decides_region(h), "{h} não decide a região");
        }
    }

    #[test]
    fn o_que_decide_regiao_tambem_e_desviado() {
        for h in ["discord.com", "gateway-us-east1-b.discord.gg", "latency.discord.media"] {
            assert!(decides_region(h));
            assert_eq!(decide(h, Phase::Opening), Route::Foreign, "{h}");
        }
    }

    #[test]
    fn reconhece_o_gateway_em_todos_os_sabores() {
        assert!(is_gateway("gateway.discord.gg"));
        assert!(is_gateway("gateway-us-east1-b.discord.gg"));
        assert!(!is_gateway("remote-auth-gateway.discord.gg"));
        assert!(!is_gateway("discord.gg"));
        assert!(!is_gateway("gateway.discord.com"));
    }
}
