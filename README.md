# Ropelato Discord

Corrige o problema do Discord no Brasil: em vários provedores brasileiros o
Discord decide a região da sua sessão pelo IP que enxerga na abertura, e essa
decisão sai errada — o sintoma mais comum é a transmissão de tela parar de
funcionar.

Este projeto faz o mesmo que ligar uma VPN só para abrir o Discord e
desligá-la assim que ele entrou: enquanto a sessão está nascendo, o tráfego do
Discord que decide a região (API, gateway, CDN) sai por um proxy SOCKS5
estrangeiro gratuito; assim que a sessão fecha, tudo volta a sair direto pelo
seu IP normal, e a região já fica gravada. A voz, a câmera e a tela são UDP e
nunca passam por aqui — nem o TCP dos servidores de voz, que não decide região
nenhuma.

Exclusivo para **Linux**.

## Como funciona

- Um serviço local sobe um proxy SOCKS5 (`ropelato-discord`) e mantém uma
  piscina de proxies públicos, validados contra o próprio Discord e ordenados
  por latência.
- O Discord é lançado com `--proxy-pac-url` apontando para um arquivo PAC
  servido localmente, que só desvia o tráfego do Discord — o resto da
  internet nunca passa pelo proxy.
- Uma janela de abertura acompanha a sessão nascendo (por silêncio de
  tráfego ou por um teto de tempo) e, assim que fecha, todo o tráfego novo
  passa a sair direto — sem precisar reiniciar nada.

## Estrutura

- `core/` — biblioteca compartilhada (proxy SOCKS5, PAC, piscina de proxies,
  janela de sessão, integração com o sistema)
- `daemon/` — o serviço (`ropelato-discord`), linha de comando
- `gui/` — janela de gerenciamento opcional (`ropelato-discord-gui`), com
  ícone de bandeja

## Instalação

### Dependências (Linux)

- Rust estável (via [rustup](https://rustup.rs))
- GTK 3 (para o ícone de bandeja da GUI) — em distros baseadas em Debian/Ubuntu:
  ```bash
  sudo apt install libgtk-3-dev
  ```

### Compilar

```bash
git clone <url-do-repositorio>
cd ropelato-discord
cargo build --release
```

Gera dois binários em `target/release/`:

- `ropelato-discord` — o serviço
- `ropelato-discord-gui` — a janela opcional

### Instalar o serviço

```bash
./target/release/ropelato-discord instalar
```

Isso copia o executável para `~/.local/share/ropelato-discord/`, ativa o
autostart (XDG), liga a correção, sobe o serviço em segundo plano, espera a
piscina de proxies validar e reinicia o Discord já com a correção ativa.

Opções:

| Flag | Efeito |
|---|---|
| `--sem-reiniciar` | não mexe no Discord que já está aberto |
| `--sem-autostart` | não cria entrada de autostart (usado internamente pela GUI) |

## Uso

Depois de instalado, o serviço sobe sozinho com a sessão. Comandos
disponíveis:

```bash
ropelato-discord status                # mostra o estado atual
ropelato-discord reiniciar-discord     # fecha e abre só o Discord
ropelato-discord rodar                 # roda em primeiro plano (debug)
ropelato-discord desinstalar           # remove tudo, sem deixar rastro
```

`desinstalar` aceita `--manter-arquivos` para limpar a configuração sem
apagar a pasta instalada (usado pela GUI antes de reinstalar).

### GUI (opcional)

```bash
./target/release/ropelato-discord-gui
```

Abre minimizada na bandeja. Precisa que o serviço já esteja instalado — ela só
fala com o serviço e os arquivos dele, não instala nada sozinha. Permite
pausar/retomar a correção, reiniciar o Discord, ligar/desligar o autostart e
ver a atividade recente (quais hosts saíram pelo exterior ou direto).

## Licença

MIT
