# Auditoria e estabilidade — 12/11/2026

Build atual: `2026.11.12-r10-interface` (base r9). A revisão preserva Rust, GTK3, GStreamer,
WebRTC e as alterações locais que já existiam ao iniciar o trabalho.

## Evolução r10: interface e player

Redesign GTK em `ui.css`, `ui.rs`, `rooms_ui.rs`, `stage.rs` e `source_picker.rs`:
camadas neutras, acento lilás, hierarquia de sala/palco/compartilhamento, busca
local, ações visíveis, estado vazio acionável, perfis rápidos, ajustes avançados,
resumo de qualidade e atalho. Sem dependências ou animações contínuas novas.
Direção e matriz de validação em `DESIGN.md` na raiz.

Preferências passam a ter versionamento. A migração troca somente os padrões
adaptive/exclude_discord solicitados; testes preservam qualidade e alterações
posteriores. Excluir Discord ausente no Windows não bloqueia compartilhar,
inclusive em runtime com apenas loopback legado. Discord presente sem suporte
continua produzindo erro explícito, evitando expor áudio que foi pedido excluir.

**Descanso durante reprodução:** não havia inibidor no player. `playback.rs`
solicita idle+suspend no Linux (SessionManager/portal) e display+system no Windows.
Worker estacionado e estado atômico coalescem mudanças sem fila crescente ou
chamadas bloqueantes no GTK. RAII libera ao fechar o último vídeo ou encerrar,
inclusive se o encerramento chega durante a aquisição. Falhas avisam sem derrubar
a mídia. `screensaver.rs` trata a mensagem Windows de protetor de tela apenas
quando assistindo; teclado, fechar e outros comandos seguem seu fluxo.
Referências: [portal Inhibit](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.Inhibit.html),
[execução no Windows](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-setthreadexecutionstate),
[protetores de tela e políticas](https://learn.microsoft.com/en-us/windows/win32/lwef/screen-saver-library).
Não altera políticas obrigatórias, suspensão manual ou configurações permanentes.

**Tela cheia em outro monitor:** a janela nova chamava fullscreen sem destino.
Agora busca monitor da janela principal no clique, posiciona a janela e usa
[fullscreen_on_monitor](https://docs.gtk.org/gtk3/method.Window.fullscreen_on_monitor.html).
Nenhum monitor é fixado em cache; mover a janela muda o alvo na próxima abertura.
API e parâmetros verificados; comportamento real com múltiplos monitores ainda
não foi validado neste ambiente.

Os quatro testes de playback passam, incluindo um barramento D-Bus privado sem
ativação de serviços do desktop: verificam flags, criação e fechamento em ambos
os caminhos Linux. Teste puro do filtro Windows preserva teclado/fechar. Parser
CSS nativo aceita o arquivo sem erros. Teste GUI ampliado para estado vazio,
busca, sala cheia e controles permanece ignorado conforme AGENTS.md. Não há
medição de custo de renderização da nova interface sem sessão gráfica.

## Cobertura da análise

| Caminho | Responsabilidade e conclusão |
|---|---|
| `media.rs`, `portal.rs`, `sources.rs` | X11, portal/PipeWire, DXGI/WGC e loopback; fallback de encoder já existia. Conversão/escala separadas faziam trabalho evitável. |
| `media.rs`, `rtc.rs` | Encoder H.264 compartilhado, Opus e WebRTC. Reproduzida colisão de payload entre RTX e áudio. Filas limitadas e proteção de referências H.264 já existiam. |
| `network.rs`, `server.rs` | Epochs, autenticação, salas, ICE, reconexão e filas. Reproduzidas perda permanente da reconexão e entrada em sala sem confirmação entregue. |
| `video.rs`, `stage.rs`, `windows_preview.rs` | Decoder automático, BGRx/xRGB e Cairo com lifetime do buffer preservado; sem cópia integral redundante. Grade ignorava redimensionamentos. |
| `ui.rs`, `rooms_ui.rs`, `source_picker.rs`, `icons.rs` | Fluxos, seleção, estados, acessibilidade, CSS GTK. Preferências não graváveis bloqueavam o uso desnecessariamente. |
| `profile.rs`, `preferences.rs`, `diagnostics.rs` | Validação, escrita atômica, logger limitado e isolamento de segredos. Coletor apontava para binário sem diagnóstico. |
| `src/bin`, `desktop.rs`, `build.rs`, Cargo, scripts e CI | Extração, runtime privado, integração desktop, instalador Windows, construção e testes. Extrator não permanece residente em uso normal. |

Não foi encontrada justificativa para reescrever o cliente ou trocar GTK.
Não foi demonstrada a causa de travamento completo do sistema nos PCs dos
usuários; é preciso coletar dados nesses PCs. A ausência de achado numa revisão
estática não prova ausência de vazamentos ou condições de corrida em drivers.

## Defeitos reproduzidos e correções

1. **Áudio desaparecia no WebRTC real.** A retransmissão de vídeo anunciava
   `rtx/90000` com payload 97, enquanto Opus também usava 97. O vídeo chegava,
   o áudio não: 741 frames de vídeo e zero buffers de áudio em uma reprodução.
   O teste falhou também no binário release anterior às alterações desta sessão.
   Opus agora usa 111; teste de SDP rejeita payloads com codecs incompatíveis.
   Os dois testes UDP passaram depois: transmissão normal e redução ao vivo
   de 1080p30 para 720p15 com áudio mantido. A restrição está no
   [RFC 8843, seção 9.1.1](https://www.rfc-editor.org/rfc/rfc8843.html#section-9.1.1).
2. **Reconexão encerrava com fila cheia.** `try_send(Server)` falhava e depois
   `try_send(Disconnected)` também; a tarefa saía permanentemente. Agora aguarda
   capacidade de forma assíncrona, preservando o evento; termina quando o
   receptor encerra. Teste de fila cheia falhou antes e passou após a correção.
3. **Servidor mudava participação sem entregar `entered`.** Agora só altera
   participação e estado após enfileirar a confirmação; falha encerra conexão
   e usa a limpeza existente. Regressão cobre falha e sucesso.
4. **Preferência auxiliar bloqueava nome/transmissão.** Valores continuam
   validados; falha de gravação avisa e permite usar a sessão em memória.
5. **Grade e feedback.** Colunas acompanham a largura do palco sem trocar
   tiles/decoders. Há estado de conexão até o primeiro frame, nomes acessíveis
   de vídeo/volume, label de nome e cor consistente do ícone desabilitado.
6. **Coleta de diagnóstico vazia.** O cliente comum deliberadamente não grava
   logs. O empacotador pode agora gerar um executável de diagnóstico separado,
   validado, e o coletor o exige; ausência de eventos não é mais tratada como
   relatório válido. Histórico e tamanho são limitados. Credenciais em URLs
   TURN também são ocultadas; uma regressão de redação falhou antes da correção.

## Adaptação conservadora

Na r10, “Ajustar automaticamente” inicia desligado e pode ser habilitado em
Qualidade. Perfis r9 são migrados uma vez; escolhas posteriores são preservadas. O perfil salvo é um teto e não é sobrescrito pelas reduções da sessão.

- Pressão local: FPS codificado abaixo de 80% do alvo, acompanhado de demanda
  da fonte ou estouro nas filas. Tela parada sozinha não é sobrecarga.
- Rede: pelo menos dois relatórios RTCP novos com perda de 10% ou mais, tratados
  separadamente por fluxo. Relatórios repetidos ou com mais de 10 s não contam.
  Áudio saudável não apaga perdas persistentes do vídeo.
- Exige 6 s de pressão e 10 s entre ajustes; uma pausa longa da engine não
  equivale a várias observações ruins.
- Rede reduz bitrate em 25%, até 1 Mbps, quando o encoder declara suporte a
  alteração em Playing. Sobrecarga local reduz 60→30 FPS, 1440→1080→720p e
  finalmente 30→15 FPS. Os controles disponíveis nunca são aumentados.
- Resolução ao vivo habilitada somente em software (`openh264enc`, `x264enc`);
  a execução local cobriu OpenH264. Em GPU, taxa/FPS dependem do suporte do
  backend; habilitar resolução exige testes específicos. O áudio é preservado.
- Uma falha no ajuste desativa a adaptação e é mostrada; erros assíncronos de
  pipeline continuam sujeitos ao encerramento seguro já existente.

Isso é proteção conservadora por perdas/backlog, **não** um estimador completo de
banda, carga total do SO ou custo de decodificação de cada espectador. Um encode
compartilhado tem uma qualidade comum; uma rede ruim pode reduzir todos os
espectadores. Não há aumento automático após redução nem recuperação garantida
de driver/dispositivo travado. Para voltar ao teto, reinicie o compartilhamento.

## Medições

Linux x86_64, GStreamer 1.28.7, build release, três execuções por cenário;
outras aplicações permaneceram abertas. CPU soma tempo de usuário e sistema,
não percentual de todo o computador. Dados: [medicoes-20261112.json](medicoes-20261112.json).

| Cenário | Antes | Depois | Leitura |
|---|---:|---:|---|
| 120 frames BGRx 3840×2160→I420 1280×720, CPU mediana | 6,052 s | 4,896 s | −19,1% **nessa transformação isolada** |
| Ponte sintética completa 1080p30/Opus, CPU mediana por execução | 3,418 s | 3,276 s | −4,2% observado; amostra curta, sem generalização para captura real |
| Frames entregues ao appsink nessa ponte, FPS mediano | 29,98 | 29,96 | Cadência média mantida dentro da variação local |
| Maior RSS entre execuções da ponte | 140412 KiB | 136896 KiB | Observação local; não é teste prolongado de vazamento |

A transformação combina `videoconvert ! videoscale` em `videoconvertscale`,
mantendo bordas e formato. Comando comparativo, trocando apenas a transformação:

```bash
gst-launch-1.0 -q videotestsrc num-buffers=120 pattern=ball \
  ! video/x-raw,format=BGRx,width=3840,height=2160,framerate=60/1 \
  ! videoconvertscale add-borders=true \
  ! video/x-raw,format=I420,width=1280,height=720 ! fakesink sync=false
```

O autoteste usa `videotestsrc`/`audiotestsrc`, não captura desktop, não pinta GTK,
não usa dispositivo de áudio real e não mede internet, GPU ou latência entre
monitores físicos. O teste UDP usa ICE/DTLS/SRTP de verdade em loopback.

## Persistência de perfil e senhas

Implementada por último, conforme solicitado. Nome e qualidade continuam nas
configurações locais. Senhas de salas ficam exclusivamente no cofre do SO:
Secret Service no Linux e Credential Manager no Windows. Não houve dependência
nova; foram reutilizados D-Bus e SHA-256 existentes e a API nativa do Windows.

Um worker dedicado, com filas limitadas, faz as chamadas bloqueantes. GTK e
engine de mídia não esperam pelo cofre. A chave inclui endpoint + ID + nome
exato da sala. O worker grava somente após o servidor confirmar a entrada;
criação usa o novo ID retornado. Cache da sessão cobre falhas no cofre.

Uma senha incorreta invalida cache/cofre e pede correção sem repetir tentativas
automaticamente. Epoch/request descartam consultas e diálogos antigos. Eventos
de interface não transportam a senha e não a expõem por Debug. Testes com cofre
simulado cobrem confirmação, reabertura, erro, saturação e respostas antigas;
os testes do backend verificam namespace, validação e tupla D-Bus sem acessar
cofres pessoais. ABI Windows recebeu revisão e checagem de tipos isolada,
não execução Windows.

No Linux é necessário um cofre padrão existente/desbloqueado. A integração
não abre prompts automáticos de Unlock nem instala um gerenciador de senhas.
Aguarde terminar “Salvando senha no cofre…”: fechar o processo antes da resposta
pode interromper a gravação pendente. Salas vazias continuam sendo removidas;
as senhas não tornam salas persistentes no servidor.

## Distribuição e diagnóstico

Mantido o instalador Windows que já estava em desenvolvimento: inclui runtime
privado e instala por usuário. Instalar GStreamer globalmente não corrige a
colisão RTP nem reduz o custo do encoder, e expõe a aplicação a versões externas.
O `.exe` Windows existente continha GStreamer 1.28.9, posterior à correção upstream
de WASAPI publicada na 1.28.4; não foi atribuída a ele essa regressão antiga.

No Linux, mantido o pacote portátil com bibliotecas. glibc, drivers, portal,
compositor e serviço de áudio continuam dependências do sistema. Um futuro
pacote nativo pode melhorar instalação/atualização, mas não dispensa validar
esses componentes. Não foram instalados drivers ou pacotes globais nesta sessão.

```bash
python3 scripts/package-native.py --diagnostics
./dist/janja-diagnostico-linux --check
./dist/janja-diagnostico-linux --self-test --report /tmp/janja-self-test.json
./dist/janja-diagnostico-linux --network-test
python3 scripts/diagnose-linux.py
```

O coletor informa CPU (100% = um núcleo), RSS, threads e estados da mídia.
Compartilhe somente `relatorio.json`, não o terminal nem logs brutos.

## Verificações executadas

- `cargo test --locked --features native,portable`: 75 testes da biblioteca e
  3 do portátil passaram. Três ignorados nessa execução: os dois UDP e o GTK.
- `cargo test --locked --features native,portable actual_webrtc_loopback -- --ignored`:
  os dois testes UDP foram executados separadamente e passaram, incluindo áudio
  e adaptação ao vivo. WebSocket TCP real também passou na suíte normal.
- `cargo clippy --locked --features native,portable --all-targets -- -D warnings`,
  também repetido com `--all-features`, `cargo fmt --check` e `git diff --check`:
  sem problemas.
- `npm test`: 3 testes Cloudflare passaram.
- `python3 scripts/diagnose-linux.py --self-test`: seleção do executável,
  CPU/RSS, limites de histórico e redação de credenciais passaram.
- Harness de empacotamento com subprocessos simulados: opt-in, separação dos
  artefatos e preservação de artefatos anteriores após falha de validação passaram.
- Revisão independente dos caminhos de mídia/adaptação, credenciais/FFI e
  identidade das tentativas. Corrigida uma questão encontrada na revisão:
  um relatório saudável de áudio podia zerar o contador de perdas do vídeo.
- `python3 scripts/package-native.py --diagnostics`: extração limpa, plugins,
  TLS e autoteste 1080p passaram (29,98 FPS, 302 buffers de áudio). Gerados
  `dist/janja-linux` (71,7 MiB) e `dist/janja-diagnostico-linux` (71,8 MiB).
- O **artefato empacotado** de diagnóstico também passou `--check` e
  `--network-test` com cache/logs temporários: 30,12 FPS entregues, 181 frames,
  308 buffers de áudio. O cliente comum confirmou a versão r9.

O servidor público não foi atualizado e nenhum CI remoto foi disparado. As
correções do servidor estão no código; aplicar ao serviço real exige implantação
separada. A validação local não acessou cofres pessoais nem a sala pública.
O instalador/cliente Windows existente em `dist` não foi substituído por um
build desta revisão no Windows. Os fontes atualizados para compilar com UCRT64
estão em `dist/janja-nativo-fontes-r9.zip`; o script gera portátil e instalador.

## Validação pendente nas máquinas de destino

| Ambiente | Procedimento e evidência a coletar |
|---|---|
| Windows 10/11, Intel/AMD/NVIDIA | Construir no UCRT64; instalar/reinstalar/desinstalar pelo script existente; DXGI e WGC, WASAPI com silêncio/som e troca de dispositivo. Confirmar encoder e se fallback funciona. |
| Linux X11 e Wayland | Compartilhar tela/janela com movimento, cancelar portal, fechar fonte, parar/recomeçar; registrar compositor, CPU/RSS e FPS. |
| Computador modesto | Começar com teto 1080p30; aplicar carga real concorrente; verificar redução gradual e responsividade, comparando com adaptação desligada. Não induzir travamento de driver. |
| Upload limitado/perda | Usar rede de teste com perda/largura controladas; comprovar relatórios RTCP novos e redução de taxa; testar recuperação/reinício e TURN configurado pelo operador. |
| Vários vídeos | 1 e depois 7 espectadores, scroll/tela cheia/minimização, áudio simultâneo; separar custo de encode, decode, conversão e pintura. |
| Uso prolongado | 30 min e 2 h, comparar RSS/threads após aquecimento e vários ciclos iniciar/parar/entrar/sair. |

Testes GTK ignorados exigem desktop conforme AGENTS.md e não devem ser
apresentados como executados aqui. Windows, drivers/GPU, portal real, rede entre
PCs e reprodução pelos dispositivos físicos ainda precisam desses ensaios.

## Próximas ideias de maior valor

- Suspender trabalho de encode quando não houver espectadores, com teste de
  retomada por backend; não pausar portal/dispositivo às cegas.
- Medir redução da conversão/pintura ao tamanho visível e custo de tiles fora
  da rolagem, mantendo áudio e decodificação referencial corretos.
- Após dados reais, orçamento de upload estimado e um controlador de banda mais
  completo; uma SFU/simulcast só se o uso em grupo justificar servidor de mídia.

## Verificação final r10

- `cargo test --locked --features native,portable`: 83 testes da biblioteca e
  3 do empacotador passaram; 3 ignorados (2 UDP e 1 GUI).
- Os 2 testes UDP foram executados separadamente e passaram: vídeo/áudio
  normais e redução de qualidade ao vivo preservando o áudio.
- Clippy com todos os alvos/features e com native,portable: sem avisos.
- `cargo fmt --check`, `git diff --check` e parser CSS GTK: passaram.
- Verificados tipos das funções GDK do filtro Windows e offsets do prefixo
  MSG x64 contra bindings locais. Isso não é uma compilação nem teste Windows.
- `package-native.py --diagnostics`: pacote reconstruído e validado em extração
  limpa; versão r10 confirmada. Cliente comum continua separado do diagnóstico.
- Comparação sintética sequencial entre os pacotes: r9 29,971 FPS/302 buffers
  de áudio; r10 29,963 FPS/301 buffers. Ambos receberam 180 quadros em 1080p30.
  É uma verificação curta do caminho de mídia, sem alegação de ganho na UI.
  Resultados completos em `medicoes-20261112.json`.
- Testes GUI não foram executados por instrução do repositório. Aparência,
  suspensão/apagamento reais, vários monitores, Windows e portal/compositor
  continuam requerendo validação de desktop. O executável Windows antigo em
  dist não foi recompilado nem deve ser confundido com esta revisão.


## Revisão r11 — áudio, recuperação e baixa latência

A versão antiga do transmissor sem áudio foi confirmada pelo usuário. Não foi
adicionada compatibilidade especial com ela: os testes e os pacotes desta revisão
usam H.264/Opus entre clientes novos, com payload de áudio 111 separado de RTX.
O servidor não mudou para estas melhorias.

### Causas e correções verificadas

- **Latência fixa do Opus com FEC:** o decoder reservava 120 ms (máximo Opus),
  embora o transmissor produzisse pacotes menores. A sincronização do pipeline
  fazia o vídeo acompanhar essa reserva. Um probe consulta a duração válida do
  pacote pela libopus e atualiza a API de latência do GstAudioDecoder. Mantém
  FEC e PLC, sem suprimir a proteção. Pacotes agora têm 10 ms em vez de 20 ms.
  O teste de regressão força essa mesma cadeia. Inverter a validação de pacotes
  fez o teste falhar: 120 ms em vez de 10 ms. O código foi restaurado e passou.
- **Recuperação RTP lenta:** três perdas reais, antes do jitterbuffer do receptor
  WebRTC local, reproduziram pausa de 601 ms. Havia feedback atrasado e pedidos
  especulativos do próximo pacote, inclusive áudio ainda em trânsito. Intervalo
  mínimo RTCP agora 50 ms; desativada previsão do próximo seqnum, preservando
  detecção de lacunas e NACK/RTX. Depayloader continua solicitando keyframes.
- **Buffer de rede fixo:** começa em 40 ms. Controlador separado do ajuste de
  qualidade observa jitter e pacotes atrasados a cada 2 s, aumenta até 150 ms
  quando necessário e reduz 10 ms após 10 s estáveis. Não altera resolução/FPS.
  Testes verificam limites, contadores reiniciados e retorno gradual.
- **Libav ausente no pacote:** a seleção de plugins excluía `libav`, mesmo com
  a dependência instalada. Agora inclui e exige `avdec_h264` no diagnóstico;
  orçamento de threads preservado e modo slice evita buffering por threading
  de quadros. OpenH264 continua disponível. Isso aumenta o pacote em disco
  de cerca de 72 para 115 MiB; não é uma medição do consumo de RAM em execução.
  CI/build Linux incluem libopus-dev e Windows inclui opus para a nova ligação.

Referências da implementação: [Opus decoder upstream](https://raw.githubusercontent.com/GStreamer/gstreamer/main/subprojects/gst-plugins-base/ext/opus/gstopusdec.c),
[RTPSession](https://gstreamer.freedesktop.org/documentation/rtpmanager/RTPSession.html)
e [WASAPI2](https://gstreamer.freedesktop.org/documentation/wasapi2/wasapi2src.html).
A API Windows já verifica as propriedades disponíveis; o próprio plugin só
expõe exclusão por processo quando o sistema a suporta. Não foi adicionado
fallback que capture Discord silenciosamente se a exclusão solicitada falhar.

### Medições e alcance

GStreamer 1.28.7, Linux x86_64. O teste UDP normal recebeu 180 quadros em 1080p30,
29,965 FPS e 605 blocos de áudio com pico não silencioso de 0,0506. A reserva
mínima reportada pelo pipeline receptor caiu de 290 ms para 88 ms. O pacote
final extraído em cache temporário também passou: 29,803 FPS, 605 blocos, reserva 89 ms.
Esses 88 ms **não são medição física entre telas nem alto-falantes**. O campo
`capture_min_latency_ms=0` não mede captura instantânea: appsinks de captura
não sincronizam pela latência do pipeline.

Com três perdas de vídeo, a execução final manteve 27,938 FPS durante a janela
com falhas; maior intervalo entre quadros decodificados de 183 ms e entre buffers
na saída de áudio sincronizada de 29 ms. Esses intervalos medem recuperação,
não latência de ponta a ponta. O buffer se ajustou e a reserva ficou em 108 ms.
Os dados completos e as execuções intermediárias estão em medicoes-20261112.json.

No benchmark do mesmo H.264 sintético 1080p60/180 quadros, três execuções alternadas
com aquecimento reduziram a mediana de CPU de 1,640 s (OpenH264) para 0,955 s
(Libav slice), cerca de 42%. Throughput mediano passou de 109,8 para 313,9 FPS.
Isso mede apenas decodificação/conversão; não permite afirmar redução de 42%
na CPU total do aplicativo. Para repetir com ferramentas GStreamer instaladas:

```bash
ffmpeg -f lavfi -i testsrc2=s=1920x1080:r=60 -frames:v 180 -c:v libx264 -preset veryfast -tune zerolatency -b:v 8M -threads 4 -g 60 -pix_fmt yuv420p /tmp/janja-1080p60.h264
/usr/bin/time -v gst-launch-1.0 -q filesrc location=/tmp/janja-1080p60.h264 ! h264parse ! openh264dec ! videoconvert ! video/x-raw,format=BGRx ! fakesink sync=false
/usr/bin/time -v gst-launch-1.0 -q filesrc location=/tmp/janja-1080p60.h264 ! h264parse ! avdec_h264 max-threads=4 thread-type=slice ! videoconvert ! video/x-raw,format=BGRx ! fakesink sync=false
```

### Interface e distribuição

Indicador nativo “N assistindo” na barra inferior, visível apenas ao transmitir;
clique abre nomes dos espectadores conectados. Usa conexões existentes, sem
novo protocolo, servidor, decoder ou tarefa contínua. Atualização em até 2 s.
Removido o símbolo gráfico do cabeçalho. Tela cheia usa fundo preto e escala
que preenche o monitor, cortando o excedente sem distorcer. “Ver conteúdo inteiro”
restaura a escala com toda a imagem, opcionalmente com margens pretas. O monitor
continua sendo identificado automaticamente a partir da janela principal.

`build-windows.ps1 -Diagnostics` gera portátil, instalador e diagnóstico separado;
o runtime e os codecs seguem embutidos, sem dependências baixadas no PC final.
Os executáveis Windows antigos em dist não foram substituídos nesta máquina.

### Verificação r11

- 88 testes de biblioteca + 3 do portátil passaram; 4 ignorados no comando geral.
- Os 3 testes UDP ignorados foram executados separadamente e passaram: vídeo e
  áudio, redução ao vivo e perdas deliberadas com continuidade de áudio.
- Clippy todos os alvos/features e native,portable, fmt e parser CSS passaram.
- Empacotamento real com diagnóstico passou em extração limpa; o diagnóstico
  também passou pelo WebRTC UDP com o próprio runtime distribuído.
- Mutação da correção Opus detectada pelo teste; arquivo restaurado byte a byte.
- Empacotador simulado preserva separação do diagnóstico e artefatos antigos
  quando a verificação falha. Bash incorporado no build Windows passa `bash -n`
  com/sem diagnóstico. Isso não é execução de PowerShell ou Windows.

Pendente: captura real de jogo e reprodução em dispositivos físicos no Windows,
Windows↔Linux, GPU/driver, Wayland/portal, dois monitores e validação visual GTK.
A suite GUI ignorada não foi executada, conforme instrução do projeto. Validar
30 min e 2 h em dois PCs com carga de jogo, som contínuo e momentos de silêncio,
NAT/TURN, perda/jitter, troca de saída de áudio e ciclos de iniciar/parar. Medir
atraso físico com vídeo/câmera ou relógio de referência antes de prometer SLA.


### Verificação adicional solicitada: transmissão contínua e som correto

Foram executadas duas janelas de 60 s (cerca de 64 s com negociação cada), com
SMPTE/ruído em 1080p30, teto de 8 Mbps e 12 pacotes de vídeo descartados em
intervalos de 3 s. Verificam a saída de áudio após decodificação e sincronização,
não apenas negociação ou presença de bytes. Contagem de cruzamentos por zero
confirmou o tom sintético de 440 Hz: 440,060 e 440,057 Hz. Ambas passaram.

- Áudio: 6.364 / 6.372 buffers, maior intervalo 29 / 28 ms, pico 0,0506.
- Vídeo: 24,927 / 24,942 FPS; o encoder de software também entregou cerca de
  25 FPS neste padrão complexo, apesar de receber 30 FPS. Há descarte interno
  do encoder sob esse perfil, portanto não se afirma 30 FPS constantes.
- Maior intervalo entre quadros durante as falhas: 378 / 218 ms. Há recuperação,
  mas perdas reais ainda podem causar pausas breves; não equivale a zero travadas.
- Na segunda execução, fase sem perdas após recuperação: 24,876 FPS e maior
  intervalo de 113 ms. Verifica também que o último quadro/áudio estejam frescos,
  evitando passar um teste que congele apenas no final da janela.
- Reserva mínima reportada pelo receptor: 89 ms em ambas; novamente não é uma
  medição de atraso físico entre PCs.

Amostras parciais de RSS do processo de integração variaram de cerca de 158 a
106 MiB, com 37 threads. Ele inclui captura sintética, emissor e receptor no
mesmo processo em debug; não é comparação de RAM do app nem prova contra
vazamento após horas de uso. Dados brutos estão no JSON de medições.

Esses testes aumentam a evidência de estabilidade entre clientes novos, sem
substituir jogo real, WASAPI/alto-falantes, drivers/GPU, desktop GTK ou internet.


### Correção adicional de perda de áudio encontrada na revisão final

A repetição do teste de áudio revelou uma falha intermitente: RTP registrava um
pacote perdido, mas o decoder não recebia o GAP e produzia zero amostras PLC.
O teste foi alinhado ao construtor real do receptor e seus buffers de replay
passaram a registrar tempo de chegada, sem reutilizar DTS antigo do encoder.
A falha permaneceu, portanto não foi atribuída apenas ao fixture.

Uma execução instrumentada mostrou o timer inserir GstRTPPacketLost e, no mesmo
instante, a fila cheia descartar sua entrada mais antiga antes de entregá-la.
O aviso não chegava ao Opus. O receptor agora verifica timestamps dos pacotes
Opus válidos e recria a indicação de lacunas curtas, de até 120 ms, quando o
aviso original não chegou. Eventos GAP normais atualizam o estado para evitar
PLC duplicado; caps, segmento, início de stream e flush limpam a referência.
Não tenta preencher grandes saltos de relógio ou transformar uma desconexão em
áudio contínuo. O buffer permanece limitado e não aumentou para esconder o bug.

Dois testes usam a cadeia real: reordenação com uma perda e perda acompanhada
pelo descarte forçado do próprio aviso GAP. Ambos verificam que cerca de um
pacote de 10 ms seja ocultado uma única vez. Desativar a nova recuperação fez
o segundo teste falhar de forma determinística (zero amostras PLC). Restaurar
o código passou; 30 repetições de cada cenário resultaram em 60 aprovações,
sem falhas. A suite completa final passou com 88 testes da biblioteca + 3 do
portátil, e Clippy com/sem diagnóstico passou sem avisos.

Os três testes UDP passaram novamente após a correção. A terceira janela de
60 s recebeu 6.359 buffers de áudio, confirmou 440,044 Hz e maior intervalo de
28 ms. O vídeo pesado recebeu 24,943 FPS; maior intervalo nas perdas de 310 ms,
e após recuperação 24,918 FPS / 114 ms. Pipeline normal: 29,951 FPS e reserva
88 ms. Esses resultados reforçam a recuperação automatizada; não comprovam
captura de jogo ou reprodução física no Windows, que continuam pendentes.


Pacotes finais reconstruídos após a correção do aviso de áudio e verificados em
extração limpa. O teste UDP do executável de diagnóstico, usando exclusivamente
o runtime distribuído, passou novamente. O cliente comum confirmou a versão
2026.11.12-r11-transmissao. Não houve execução de GUI nem build Windows local.
