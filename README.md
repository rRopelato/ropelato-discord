# Ropelato Discord

Corrige o problema do Discord no Brasil
(Obrigado Janja por não deixar eu ver anime em call com meu amigo.)

Discord decide a região da sua sessão pelo IP que é verificado na abertura do programa.

Este projeto faz o mesmo que ligar uma VPN só para abrir o Discord e
fechar assim que ele entrou. Enquanto a sessão está sendo aberta, o tráfego do
Discord que decide a região (API, gateway, CDN) sai por um proxy SOCKS5
estrangeiro gratuito, assim que a sessão fecha, tudo volta a sair direto pelo
seu IP normal, e a região já fica gravada.

Exclusivo para **Linux**.

## Como funciona

- Um serviço local sobe um proxy SOCKS5 (`ropelato-discord`) e mantém uma
  lista de proxies públicos, validados contra o próprio Discord e ordenados
  por latência.
- O Discord é aberto com `--proxy-pac-url` apontando para um arquivo PAC
  servido localmente, que só desvia o tráfego do Discord, o resto da
  internet nunca passa pelo proxy.

## Estrutura

- `core/` — biblioteca compartilhada (proxy SOCKS5, PAC, lista de proxies,
  janela da sessão, integração com o sistema).
- `daemon/` — o serviço (`ropelato-discord`), linha de comando.
- `gui/` — janela de gerenciamento opcional (`ropelato-discord-gui`).

## Instalação

### Dependências (Linux)

- Rust estável (via [rustup](https://rustup.rs))
- GTK 3 (para o ícone da GUI), em distros baseadas em Debian/Ubuntu:
  ```bash
  sudo apt install libgtk-3-dev
  ```

### Compilar

```bash
git clone <repository-url>
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
lista de proxies validar e reinicia o Discord já com a correção ativa.

Opções:

| Flag                | Efeito                                                       |
| ------------------- | ------------------------------------------------------------ |
| `--sem-reiniciar` | não mexe no Discord que já está aberto                    |
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

Abre minimizada na barra de tarefas. Precisa que o serviço já esteja instalado, ela só
fala com o serviço e os arquivos dele, não instala nada sozinha. Permite
pausar/retomar a correção, reiniciar o Discord, ligar/desligar o autostart e
ver a atividade recente (quais hosts saíram pelo exterior ou direto).

## Licença

MIT
