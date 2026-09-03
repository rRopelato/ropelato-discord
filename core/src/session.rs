use std::{
    sync::Mutex,
    time::{Duration, Instant},
};
use tokio::sync::broadcast;

use crate::routing;

const QUIET_PERIOD: Duration = Duration::from_secs(30);

const CEILING: Duration = Duration::from_secs(120);

const READS_UNTIL_GONE: u32 = 3;

const GATEWAY_DEAD_THRESHOLD: Duration = Duration::from_secs(60);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Phase {
    Opening,
    Established,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Identity {
    pub pid: u32,
    pub created_at: u64,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Change {
    NewDiscord,
    DiscordClosed,
}

struct State {
    phase: Phase,
    in_flight: u32,
    armed_at: Instant,
    last_handshake: Option<Instant>,
    discord: Option<Identity>,
    empty_reads: u32,
    gateways: u32,
    gateway_down_at: Option<Instant>,
    gateway_counted: bool,
}

pub struct Session {
    state: Mutex<State>,
    cancel: broadcast::Sender<()>,
}

pub struct Handshake<'a> {
    session: &'a Session,
}

impl Handshake<'_> {
    #[cfg(test)]
    pub fn end_at(self, now: Instant) {
        self.session.end_handshake(now);
        std::mem::forget(self);
    }
}

impl Drop for Handshake<'_> {
    fn drop(&mut self) {
        self.session.end_handshake(Instant::now());
    }
}

impl Session {
    pub fn new(now: Instant) -> Self {
        let (cancel, _) = broadcast::channel(1);
        Self {
            state: Mutex::new(State {
                phase: Phase::Opening,
                in_flight: 0,
                armed_at: now,
                last_handshake: None,
                discord: None,
                empty_reads: 0,
                gateways: 0,
                gateway_down_at: None,
                gateway_counted: false,
            }),
            cancel,
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn phase(&self) -> Phase {
        self.lock().phase
    }

    pub fn armed_for(&self, now: Instant) -> Duration {
        now.saturating_duration_since(self.lock().armed_at)
    }

    pub fn begin_handshake(&self, host: &str, now: Instant) -> Option<Handshake<'_>> {
        if !routing::decides_region(host) {
            return None;
        }
        let mut state = self.lock();
        if routing::is_gateway(host) {
            if state.gateway_counted {
                return None;
            }
            state.gateway_counted = true;
        }
        state.in_flight += 1;
        state.last_handshake = Some(now);
        Some(Handshake { session: self })
    }

    fn end_handshake(&self, now: Instant) {
        let mut state = self.lock();
        state.in_flight = state.in_flight.saturating_sub(1);
        state.last_handshake = Some(now);
    }

    pub fn evaluate(&self, now: Instant, has_foreign: bool) -> bool {
        let mut state = self.lock();
        if state.phase == Phase::Established {
            return false;
        }

        if !has_foreign || state.discord.is_none() {
            state.armed_at = now;
            state.last_handshake = None;
            return false;
        }

        let reference = state.last_handshake.unwrap_or(state.armed_at);
        let quiet =
            state.in_flight == 0 && now.saturating_duration_since(reference) >= QUIET_PERIOD;
        let hit_ceiling = now.saturating_duration_since(state.armed_at) >= CEILING;
        if !quiet && !hit_ceiling {
            return false;
        }

        state.phase = Phase::Established;
        drop(state);

        let _ = self.cancel.send(());
        true
    }

    pub fn observe_discord(&self, main: Option<Identity>, now: Instant) -> Option<Change> {
        let mut state = self.lock();
        match main {
            Some(current) => {
                state.empty_reads = 0;
                if state.discord == Some(current) {
                    return None;
                }
                state.discord = Some(current);
                Self::open(&mut state, now);
                Some(Change::NewDiscord)
            }
            None => {
                state.discord?;
                state.empty_reads += 1;
                if state.empty_reads == 1 {
                    Self::open(&mut state, now);
                }
                if state.empty_reads < READS_UNTIL_GONE {
                    return None;
                }
                state.discord = None;
                state.empty_reads = 0;
                Self::open(&mut state, now);
                Some(Change::DiscordClosed)
            }
        }
    }

    pub fn gateway_opened(&self, now: Instant) -> Option<Duration> {
        let mut state = self.lock();
        state.gateways += 1;
        if state.phase != Phase::Established || state.gateways != 1 {
            return None;
        }
        let gap = now.saturating_duration_since(state.gateway_down_at?);
        (gap >= GATEWAY_DEAD_THRESHOLD).then_some(gap)
    }

    pub fn gateway_closed(&self, now: Instant) {
        let mut state = self.lock();
        state.gateways = state.gateways.saturating_sub(1);
        if state.gateways == 0 {
            state.gateway_down_at = Some(now);
        }
    }

    pub fn subscribe_cancel(&self) -> broadcast::Receiver<()> {
        self.cancel.subscribe()
    }

    fn open(state: &mut State, now: Instant) {
        state.phase = Phase::Opening;
        state.armed_at = now;
        state.last_handshake = None;
        state.gateway_counted = false;
    }

    #[cfg(test)]
    fn in_flight(&self) -> u32 {
        self.lock().in_flight
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WITH_FOREIGN: bool = true;
    const WITHOUT_FOREIGN: bool = false;

    fn t0() -> Instant {
        Instant::now()
    }

    fn id(pid: u32) -> Option<Identity> {
        Some(Identity { pid, created_at: 1 })
    }

    fn with_discord(t: Instant) -> Session {
        let s = Session::new(t);
        s.observe_discord(id(100), t);
        s
    }

    fn decision(s: &Session, t: Instant) {
        s.begin_handshake("discord.com", t)
            .expect("discord.com decide a região")
            .end_at(t);
    }

    fn close_discord(s: &Session, t: Instant) -> Option<Change> {
        let mut last = None;
        for i in 0..READS_UNTIL_GONE {
            last = s.observe_discord(None, t + Duration::from_secs(u64::from(i)));
        }
        last
    }

    #[test]
    fn a_janela_nao_vence_com_o_discord_fechado() {
        let t = t0();
        let s = Session::new(t);
        s.observe_discord(None, t);

        assert!(!s.evaluate(t + Duration::from_secs(3600), WITH_FOREIGN));
        assert_eq!(s.phase(), Phase::Opening);
    }

    #[test]
    fn a_janela_reabre_quando_o_discord_fecha() {
        let t = t0();
        let s = with_discord(t);
        decision(&s, t);
        assert!(s.evaluate(t + QUIET_PERIOD, WITH_FOREIGN));
        assert_eq!(s.phase(), Phase::Established);

        assert_eq!(
            close_discord(&s, t + Duration::from_secs(60)),
            Some(Change::DiscordClosed)
        );
        assert_eq!(s.phase(), Phase::Opening);
    }

    #[test]
    fn a_janela_espera_a_conexao_em_voo() {
        let t = t0();
        let s = with_discord(t);

        let handshake = s.begin_handshake("discord.com", t).unwrap();
        assert!(!s.evaluate(t + QUIET_PERIOD + Duration::from_secs(5), WITH_FOREIGN));
        assert_eq!(s.phase(), Phase::Opening);

        let ended = t + QUIET_PERIOD + Duration::from_secs(5);
        handshake.end_at(ended);
        assert!(!s.evaluate(ended, WITH_FOREIGN), "o silêncio recomeça agora");
        assert!(s.evaluate(ended + QUIET_PERIOD, WITH_FOREIGN));
    }

    #[test]
    fn o_aperto_de_mao_solta_em_qualquer_saida() {
        let t = t0();
        let s = with_discord(t);

        fn opens_and_fails(s: &Session, t: Instant) -> Result<(), ()> {
            let _handshake = s
                .begin_handshake("discord.com", t)
                .expect("discord.com decide a região");
            assert_eq!(s.in_flight(), 1, "o guard existe enquanto o aperto corre");
            Err(())?;
            Ok(())
        }
        assert!(opens_and_fails(&s, t).is_err());
        assert_eq!(s.in_flight(), 0, "o guard soltou ao cair");

        assert!(s.evaluate(t + QUIET_PERIOD + Duration::from_secs(1), WITH_FOREIGN));
    }

    #[test]
    fn a_janela_espera_a_piscina_encher() {
        let t = t0();
        let s = with_discord(t);

        assert!(!s.evaluate(t + Duration::from_secs(600), WITHOUT_FOREIGN));
        assert_eq!(s.phase(), Phase::Opening);

        let filled = t + Duration::from_secs(600);
        assert!(!s.evaluate(filled + Duration::from_secs(9), WITH_FOREIGN));
        assert!(s.evaluate(filled + QUIET_PERIOD, WITH_FOREIGN));
    }

    #[test]
    fn comeca_em_abertura() {
        assert_eq!(Session::new(t0()).phase(), Phase::Opening);
    }

    #[test]
    fn silencio_fecha_a_janela() {
        let t = t0();
        let s = with_discord(t);
        decision(&s, t);

        assert!(s.evaluate(t + QUIET_PERIOD, WITH_FOREIGN), "o silêncio completo fecha");
        assert_eq!(s.phase(), Phase::Established);
    }

    #[test]
    fn trafego_continuo_nao_fecha_antes_do_silencio() {
        let t = t0();
        let s = with_discord(t);

        decision(&s, t);
        decision(&s, t + Duration::from_secs(5));

        assert!(!s.evaluate(t + Duration::from_secs(9), WITH_FOREIGN));
        assert_eq!(s.phase(), Phase::Opening, "ainda faltam 26s de silêncio");
        assert!(!s.evaluate(t + QUIET_PERIOD, WITH_FOREIGN), "a segunda conexão empurrou o relógio");
        assert!(s.evaluate(t + Duration::from_secs(5) + QUIET_PERIOD, WITH_FOREIGN));
    }

    #[test]
    fn so_quem_decide_regiao_alimenta_o_relogio() {
        let t = t0();
        let s = with_discord(t);
        decision(&s, t);

        let late = t + Duration::from_secs(25);
        for h in [
            "cdn.discordapp.com",
            "c-gru17-851904d3.discord.media",
            "status.discord.com",
        ] {
            assert!(
                s.begin_handshake(h, late).is_none(),
                "{h} não segura a janela"
            );
        }
        assert_eq!(s.in_flight(), 0);
        assert!(
            s.evaluate(t + QUIET_PERIOD, WITH_FOREIGN),
            "o relógio ficou parado em t: a janela fecha aos 30s"
        );

        let s = with_discord(t);
        decision(&s, t);
        decision(&s, late);
        assert!(!s.evaluate(t + QUIET_PERIOD, WITH_FOREIGN));
        assert!(s.evaluate(late + QUIET_PERIOD, WITH_FOREIGN));
    }

    #[test]
    fn o_teto_fecha_mesmo_com_trafego_continuo() {
        let t = t0();
        let s = with_discord(t);

        let mut step = Duration::ZERO;
        while step < CEILING {
            decision(&s, t + step);
            assert!(!s.evaluate(t + step, WITH_FOREIGN), "não pode fechar antes do teto");
            step += Duration::from_secs(5);
        }

        assert!(s.evaluate(t + CEILING, WITH_FOREIGN));
        assert_eq!(s.phase(), Phase::Established);
    }

    #[test]
    fn a_janela_so_fecha_uma_vez() {
        let t = t0();
        let s = with_discord(t);
        decision(&s, t);

        assert!(s.evaluate(t + QUIET_PERIOD, WITH_FOREIGN), "primeira passada fecha");
        assert!(
            !s.evaluate(t + QUIET_PERIOD + Duration::from_secs(1), WITH_FOREIGN),
            "já estava fechada; não fecha de novo"
        );
    }

    #[test]
    fn fechar_avisa_quem_esta_no_exterior() {
        let t = t0();
        let s = with_discord(t);
        let mut subscription = s.subscribe_cancel();
        decision(&s, t);

        assert!(
            subscription.try_recv().is_err(),
            "com a janela aberta ninguém é derrubado"
        );

        s.evaluate(t + QUIET_PERIOD, WITH_FOREIGN);

        assert!(subscription.try_recv().is_ok(), "fechou, então derruba");
        assert!(
            subscription.try_recv().is_err(),
            "um aviso só; a conexão não é derrubada duas vezes"
        );
    }

    #[test]
    fn armada_ha_mede_desde_a_ultima_reabertura() {
        let t = t0();
        let s = with_discord(t);

        decision(&s, t);
        s.evaluate(t + QUIET_PERIOD, WITH_FOREIGN);
        assert_eq!(s.armed_for(t + QUIET_PERIOD), QUIET_PERIOD);

        let later = t + Duration::from_secs(60);
        s.observe_discord(id(200), later);
        assert_eq!(s.armed_for(later + Duration::from_secs(5)), Duration::from_secs(5));
    }

    #[test]
    fn discord_reiniciado_rearma() {
        let t = t0();
        let s = with_discord(t);
        decision(&s, t);
        s.evaluate(t + QUIET_PERIOD, WITH_FOREIGN);
        assert_eq!(s.phase(), Phase::Established);

        assert_eq!(
            s.observe_discord(id(300), t + Duration::from_secs(60)),
            Some(Change::NewDiscord)
        );
        assert_eq!(s.phase(), Phase::Opening);
    }

    #[test]
    fn pid_reaproveitado_ainda_e_discord_novo() {
        let t = t0();
        let s = with_discord(t);
        decision(&s, t);
        s.evaluate(t + QUIET_PERIOD, WITH_FOREIGN);

        let reborn = Some(Identity { pid: 100, created_at: 2 });
        assert_eq!(
            s.observe_discord(reborn, t + Duration::from_secs(60)),
            Some(Change::NewDiscord)
        );
        assert_eq!(s.phase(), Phase::Opening);
    }

    #[test]
    fn o_mesmo_discord_nao_rearma() {
        let t = t0();
        let s = with_discord(t);
        decision(&s, t);
        s.evaluate(t + QUIET_PERIOD, WITH_FOREIGN);

        assert_eq!(s.observe_discord(id(100), t + Duration::from_secs(20)), None);
        assert_eq!(s.phase(), Phase::Established);
    }

    #[test]
    fn discord_abrindo_do_zero_rearma() {
        let t = t0();
        let s = Session::new(t);
        s.observe_discord(None, t);

        assert_eq!(
            s.observe_discord(id(500), t + Duration::from_secs(30)),
            Some(Change::NewDiscord),
            "Discord aparecendo do nada é um Discord novo"
        );
        assert_eq!(s.phase(), Phase::Opening);
    }

    #[test]
    fn leitura_vazia_ja_reabre_a_janela_sem_soltar_a_identidade() {
        let t = t0();
        let s = with_discord(t);
        decision(&s, t);
        s.evaluate(t + QUIET_PERIOD, WITH_FOREIGN);
        assert_eq!(s.phase(), Phase::Established);

        let later = t + Duration::from_secs(60);
        assert_eq!(s.observe_discord(None, later), None, "nenhuma mudança ainda");
        assert_eq!(s.phase(), Phase::Opening, "a janela já reabriu");

        assert_eq!(
            s.observe_discord(id(100), later + Duration::from_secs(1)),
            None,
            "o mesmo Discord voltou a aparecer: nada mudou"
        );

        assert_eq!(
            close_discord(&s, later + Duration::from_secs(10)),
            Some(Change::DiscordClosed)
        );
        assert_eq!(s.phase(), Phase::Opening);
    }

    #[test]
    fn discord_novo_no_meio_das_leituras_vazias_e_percebido_na_hora() {
        let t = t0();
        let s = with_discord(t);
        decision(&s, t);
        s.evaluate(t + QUIET_PERIOD, WITH_FOREIGN);

        let later = t + Duration::from_secs(60);
        assert_eq!(s.observe_discord(None, later), None);
        assert_eq!(
            s.observe_discord(id(300), later + Duration::from_secs(1)),
            Some(Change::NewDiscord)
        );
        assert_eq!(s.phase(), Phase::Opening);
    }

    #[test]
    fn rearmar_reabre_a_contagem_do_silencio() {
        let t = t0();
        let s = with_discord(t);
        decision(&s, t);
        s.evaluate(t + QUIET_PERIOD, WITH_FOREIGN);

        let later = t + Duration::from_secs(60);
        s.observe_discord(id(200), later);
        assert_eq!(s.phase(), Phase::Opening);

        assert!(!s.evaluate(later + Duration::from_secs(9), WITH_FOREIGN));
        assert!(s.evaluate(later + QUIET_PERIOD, WITH_FOREIGN));
    }

    #[test]
    fn reconexao_do_gateway_nao_empurra_o_relogio() {
        let t = t0();
        let s = with_discord(t);

        s.begin_handshake("gateway.discord.gg", t)
            .expect("o primeiro gateway decide a região")
            .end_at(t);

        assert!(
            s.begin_handshake("gateway.discord.gg", t + Duration::from_secs(25))
                .is_none(),
            "a reconexão do gateway não segura a janela nem empurra o relógio"
        );

        assert!(s.evaluate(t + Duration::from_secs(30), WITH_FOREIGN));
        assert_eq!(s.phase(), Phase::Established);
    }

    #[test]
    fn gateway_novo_depois_de_um_vao_longo_e_suspeito() {
        let t = t0();
        let s = with_discord(t);
        decision(&s, t);
        s.evaluate(t + QUIET_PERIOD, WITH_FOREIGN);
        assert_eq!(s.phase(), Phase::Established);

        let g1 = t + Duration::from_secs(31);
        assert_eq!(s.gateway_opened(g1), None);

        s.gateway_closed(g1 + Duration::from_secs(600));
        let g2 = g1 + Duration::from_secs(603);
        assert_eq!(s.gateway_opened(g2), None);

        s.gateway_closed(g2 + Duration::from_secs(600));
        let g3 = g2 + Duration::from_secs(600) + GATEWAY_DEAD_THRESHOLD;
        assert_eq!(s.gateway_opened(g3), Some(GATEWAY_DEAD_THRESHOLD));
    }

    #[test]
    fn gateway_novo_na_abertura_nao_e_suspeito() {
        let t = t0();
        let s = with_discord(t);

        s.gateway_closed(t);
        assert_eq!(s.gateway_opened(t + GATEWAY_DEAD_THRESHOLD * 10), None);
        assert_eq!(s.phase(), Phase::Opening);
    }

    #[test]
    fn segundo_gateway_com_o_primeiro_de_pe_nao_e_suspeito() {
        let t = t0();
        let s = with_discord(t);
        decision(&s, t);
        s.evaluate(t + QUIET_PERIOD, WITH_FOREIGN);

        let g1 = t + Duration::from_secs(31);
        s.gateway_opened(g1);
        s.gateway_closed(g1 + Duration::from_secs(10));
        let g2 = g1 + Duration::from_secs(10) + GATEWAY_DEAD_THRESHOLD;
        assert!(s.gateway_opened(g2).is_some(), "o primeiro depois do vão avisa");

        assert_eq!(s.gateway_opened(g2 + Duration::from_secs(1)), None);
    }
}
