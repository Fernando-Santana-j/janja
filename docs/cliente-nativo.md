# Atualização r11 — transmissão, áudio e baixa latência

Versão: `2026.11.12-r11-transmissao`. Mantém GTK3/GStreamer e as melhorias anteriores.

- Áudio Opus em pacotes de 10 ms, preservando FEC e ocultação de perdas.
- Receptor recupera avisos de perda de áudio descartados pelo buffer cheio,
  acionando a ocultação de lacunas curtas sem repetir avisos já recebidos.
- Reserva do decodificador ajustada à duração real do pacote: remove a reserva
  fixa de 120 ms que atrasava áudio e vídeo sincronizados.
- Buffer de rede começa em 40 ms. A cada 2 s, aumenta conforme jitter/pacotes
  atrasados (limite 150 ms) e recua gradualmente após estabilidade. Isso independe
  da opção de ajuste automático de resolução/FPS, que continua desligada por padrão.
- Recuperação RTP com retorno de controle mais frequente e sem solicitações
  especulativas de retransmissão de cada próximo pacote.
- Pacote inclui Libav, com orçamento de threads e decodificação por slices.
- Durante o compartilhamento, indicador discreto “N assistindo”; clique para
  ver os nomes das conexões estabelecidas, atualizados em até 2 s.
- Removida a marca gráfica do cabeçalho. Tela cheia preenche o monitor sem
  barras do aplicativo; quando as proporções diferem, corta as bordas sem deformar.
  “Ver conteúdo inteiro” permite recuperar a imagem completa.

No teste UDP local de 1080p30, a reserva mínima informada pelo pipeline receptor
ficou em 88 ms, contra 290 ms no cenário anterior à redução dos buffers. Esse
valor **não mede o atraso entre a tela original e a tela do espectador**: captura,
rede, GPU, monitor e dispositivo de áudio precisam de medição em dois PCs.
Com 3 perdas RTP deliberadas, a maior pausa entre quadros decodificados caiu de
601 ms para 183 ms; o maior intervalo na saída de áudio foi 29 ms.
Também passaram três janelas de 60 s com padrão de vídeo mais pesado e 12 perdas:
áudio de 440 Hz confirmado na saída, maior intervalo de 28–29 ms; vídeo em cerca
de 25 FPS, recuperando perdas com pausas máximas de 218–378 ms. O encoder de
software também ficou perto de 25 FPS nesse cenário. Isso não equivale a 30 FPS
constantes nem a zero pausas sob perdas.
Não há promessa de atraso fixo nem ausência absoluta de cortes em qualquer rede.

## Interface e qualidade de vida


- Salas com busca por nome, ocupação, proteção e indicação de sala cheia.
- Participantes com ação Assistir visível e destaque de transmissões abertas.
- Palco com ação para compartilhar e orientação quando não há vídeos abertos.
- Barra inferior com qualidade selecionada, estado do compartilhamento e ações
  Qualidade/Compartilhar/Parar; atalho Ctrl+Shift+S.
- Qualidade com perfis Leve, Equilibrado e Mais detalhes. FPS, resolução,
  bitrate e medição local ficam em Ajustes avançados, com rolagem no painel.
- Ajuste automático **desligado por padrão**. Remover o áudio do Discord **ligado
  por padrão no Windows**. No Linux a exclusão continua indisponível e identificada.
  Perfis antigos recebem esses dois padrões uma vez, preservando os números e
  o áudio; escolhas posteriores ficam salvas, inclusive ao fechar o app.
- No Windows, Discord fechado não impede compartilhar: usa áudio normal. Abra
  o Discord antes de começar para excluí-lo; se abri-lo durante a transmissão,
  reinicie o compartilhamento. Suporte a exclusão por processo continua necessário
  quando há um Discord a excluir; não há fallback que exponha seu áudio silenciosamente.

### Reprodução sem descanso automático e tela cheia

Enquanto houver uma transmissão sendo assistida, o Janja solicita ao sistema
que mantenha a tela e a sessão ativas. O bloqueio é liberado ao fechar o último
vídeo, sair/desconectar da sala ou fechar o aplicativo. Vale também em tela cheia.
Chamadas de energia usam uma thread adormecida, fora da interface e da mídia;
D-Bus tem timeout. Se o desktop não oferecer suporte, há aviso na interface.

Linux: GNOME/Xfce SessionManager, com fallback para o portal Inhibit (necessário
no KDE/outros desktops sem esses gerenciadores). Windows: solicitação de energia
nativa e supressão de SC_SCREENSAVE nas janelas do Janja durante a reprodução.
Suspensão manual, fechar a tampa e políticas de bloqueio obrigatório do sistema
continuam sob controle do usuário/administrador. A proteção não muda o plano de
energia, o registro nem as configurações permanentes do desktop.

A tela cheia consulta o monitor da janela principal no instante do clique e
solicita explicitamente esse monitor, com posicionamento inicial no Windows/X11.
Isso evita depender da escolha padrão para uma janela nova. O compositor continua
responsável por aceitar o pedido, especialmente no Wayland.

Verificação: testes automatizados de preferências, ciclo de vida e protocolo
D-Bus isolado; suite de mídia/sinalização; parser CSS GTK sem display. Aparência,
timers reais de energia, dois monitores, Windows e Wayland exigem validação em
máquinas reais. O teste GUI existente foi ampliado e continua ignorado neste ambiente.

## Histórico r9 — adaptação e estabilidade

Versão: `2026.11.12-r9-adaptive`. [Auditoria, medições e limites de validação](auditoria-20261112.md).
Corrigida a colisão RTP entre retransmissão de vídeo e Opus, verificada com
WebRTC UDP real. Corrigidas reconexão com fila cheia, confirmação de sala e
preferências que impediam usar o aplicativo quando não podiam ser gravadas.

Qualidade oferece “Ajustar automaticamente”: reduz bitrate/FPS e, em software,
resolução após pressão persistente; o perfil salvo permanece como teto. Veja
o rodapé para o perfil aplicado. Não aumenta automaticamente; para voltar ao
teto, reinicie o compartilhamento. Resolução ao vivo em GPU não foi habilitada.

A grade acompanha o tamanho da janela sem recriar os vídeos; há feedback de
conexão e nomes acessíveis de vídeo/volume. Windows/GPU/portal e uso prolongado
continuam exigindo testes nos computadores de destino.

### Senhas lembradas entre execuções

Depois de uma entrada aceita, a senha é guardada no cofre do sistema:
Credential Manager no Windows; Secret Service no Linux (GNOME Keyring, KWallet
ou compatível, com cofre padrão desbloqueado). Ao clicar em Entrar novamente,
o Janja consulta a senha guardada antes de abrir o diálogo. O nome público e o
perfil de qualidade continuam salvos nas configurações usuais.

Nenhuma senha é gravada nos arquivos do perfil nem nos logs. Se o cofre falhar,
o app avisa e lembra somente durante aquela execução. Aguarde o fim do estado
“Salvando senha no cofre…” antes de fechar: uma gravação ainda pendente pode ser
interrompida pelo encerramento. Senha rejeitada é descartada e solicitada de novo.

Salas vazias continuam temporárias: quando o servidor remove uma sala, criar
outra não é reabrir a mesma. A senha lembrada vale para o mesmo endereço,
identificador e nome de sala; não altera autenticação nem dispensa o servidor.

## Atualização r8

Versão atual: `2026.11.11-r8-audio`. O rodapé distingue sinal, silêncio, ausência de dados e volume silenciado. Passe o mouse sobre ele para ler o texto completo. O sinal recebido é medido após o volume do Janja; ainda é preciso conferir a saída e o volume do sistema operacional.

O receptor solicita buffer de áudio de 40 ms, em vez dos 200 ms padrão. Em Windows com suporte, a captura por processo substitui a captura presa à saída padrão e exclui o próprio Janja para evitar retorno. A latência total depende também da captura, rede, codec e dispositivo. Se os aplicativos não renderizarem áudio, a captura entrega silêncio mesmo nesse modo.

## Atualização r7

Correções da revisão anterior e testes Windows → Linux: [estabilidade r7](estabilidade-r7.md), identificada como `2026.11.11-r7-network`. As instruções r6 abaixo são históricas.

## Atualização r6

O cliente final não grava logs nem inclui os comandos --check, --self-test e --network-test mencionados nas instruções históricas abaixo. Para diagnóstico, use `cargo run --features native,diagnostics --bin janja -- --self-test`. As mudanças atuais e as instruções de senha mestra estão em [player-r6.md](player-r6.md).

# Cliente nativo Janja

Cliente Rust com GTK3, GStreamer e WebRTC nativo. Não usa navegador, Electron,
WebView, HTML nem JavaScript. Esta versão exige atualizar o servidor para protocolo 2; consulte [salas e atualização](salas-e-tela-cheia.md).

Para o erro de negociação ou FPS baixo, veja [coleta de logs e diagnóstico](diagnostico-atual.md).

## Uso

Ao clicar em “Transmitir”, escolha a tela ou janela no diálogo (no Wayland,
o seletor é do compositor). Cancelar não inicia a captura.

Na primeira abertura, escolha seu nome; ele fica salvo para as próximas sessões.
Abra o executável, aguarde “Conectado”, crie uma sala com senha opcional ou entre em uma disponível. Depois use “Transmitir” ou “Assistir”.
O endereço é fixo: wss://infocoretech.com.br/janja/ws. Cada sala aceita oito participantes; salas vazias são removidas.

Configurações: 15/30/60 FPS, 720p/1080p/1440p, 1–20 Mbps (padrão: 1080p, 30 FPS, 8 Mbps), áudio do computador.
Para mudar configurações, pare a transmissão. A reprodução aparece dentro do aplicativo. É possível haver vários transmissores; cada cliente assiste
a várias transmissões simultaneamente. A interface usa tema escuro inspirado no Discord,
com participantes à esquerda, palco amplo e controles fixos na barra inferior.
A captura não gera prévia local. Cada vídeo recebido tem volume independente (0–100%), botão Fechar e Tela cheia; Escape volta à sala. Não há controle de volume geral duplicado.

O aplicativo prefere codificadores por hardware, com fallback para software.
NVIDIA no Windows possui caminho D3D11 sem baixar cada quadro para a CPU.
H.264 é codificado uma vez e compartilhado com os espectadores; áudio em Opus.
Filas limitadas descartam quadros atrasados. A taxa configurada é por
espectador: 3 Mbps com três espectadores requer aproximadamente 9 Mbps de upload.
A taxa ainda não é ajustada automaticamente conforme a conexão.

## Áudio e captura

Windows: tela inteira por DXGI e seleção de janela por WGC; áudio WASAPI.
Captura de janela exige que o plugin instalado tenha suporte WGC; essa combinação
ainda precisa ser confirmada no Windows. Uma falha de janela não passa a transmitir
a tela inteira silenciosamente. “Excluir Discord” usa exclusão por árvore
de processos e exige suporte a process loopback (Windows build 20348+; Windows 11
é a opção prática para desktops). Abra o Discord antes de iniciar para excluí-lo. Uma única
árvore de processos é excluída; múltiplas instalações do Discord não são cobertas.

Linux X11: seleção de tela inteira ou janela. Wayland: seletor de tela/janela do portal do
desktop e PipeWire. Áudio exige PulseAudio ou pipewire-pulse. Exclusão automática
do Discord ainda indisponível no Linux.

O áudio recebido fica ligado por padrão. O botão “Silenciar recepção” controla
explicitamente o que você escuta; use-o se houver retorno ao transmitir também
o áudio do computador. Não há silenciamento automático.

## Gerar Linux

Ubuntu 24.04, em um terminal:
```bash
sudo apt-get update
sudo apt-get install -y build-essential pkg-config python3 libgtk-3-dev libdbus-1-dev libopus-dev \
  libgstreamer1.0-dev libgstreamer-plugins-base1.0-dev libgstreamer-plugins-bad1.0-dev \
  gstreamer1.0-tools gstreamer1.0-plugins-base gstreamer1.0-plugins-good \
  gstreamer1.0-plugins-bad gstreamer1.0-plugins-ugly gstreamer1.0-libav \
  gstreamer1.0-nice gstreamer1.0-pipewire
python3 scripts/package-native.py
chmod +x dist/janja-linux
./dist/janja-linux
```
É necessário Rust estável recente, com suporte à edição 2024.
Em Arch: pacotes gtk3, dbus, gstreamer, gst-plugins-base, gst-plugins-good,
gst-plugins-bad, gst-plugins-ugly, gst-libav, gst-plugin-pipewire, libnice,
rust, base-devel, python e pkgconf.

O executável Linux funciona em sistemas com glibc igual ou mais recente que a
máquina de compilação. Para distribuir, prefira compilar no Ubuntu 24.04.
O pacote gerado neste ambiente Arch não é validado para Ubuntu.

## Gerar Windows

Instale MSYS2 uma vez, no PowerShell:
```powershell
winget install --id MSYS2.MSYS2 --exact --accept-package-agreements --accept-source-agreements
```

Com o ZIP `janja-nativo-fontes-r11.zip` em Downloads do Windows, a sequência
completa pelo PowerShell é:

```powershell
Expand-Archive "$HOME\Downloads\janja-nativo-fontes-r11.zip" "$HOME\Downloads\janja-r11" -Force
Set-Location "$HOME\Downloads\janja-r11"
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\build-windows.ps1 -Diagnostics
if ($LASTEXITCODE -ne 0) { throw 'Compilação falhou; confira o erro acima.' }
$p = Start-Process -FilePath '.\dist\janja-diagnostico-windows.exe' -WorkingDirectory $PWD.Path -ArgumentList '--check' -Wait -PassThru
if ($p.ExitCode -ne 0) { throw 'Verificação do runtime falhou.' }
$p = Start-Process -FilePath '.\dist\janja-diagnostico-windows.exe' -WorkingDirectory $PWD.Path -ArgumentList @('--network-test', '--report', 'teste-rede.json') -Wait -PassThru
Get-Content '.\teste-rede.json'
if ($p.ExitCode -ne 0) { throw 'Teste WebRTC falhou; guarde o relatório.' }
Start-Process '.\dist\janja-setup-windows.exe'
```

Se a atualização do MSYS2 encerrar a sessão, execute novamente o comando
`build-windows.ps1 -Diagnostics`. Feche o Janja antigo antes de instalar. Distribua
o instalador, atualize transmissor e espectadores e reabra o aplicativo.
O diagnóstico opcional é separado do cliente usado no dia a dia.

Para executar a suite no terminal **MSYS2 UCRT64**, na pasta do projeto:

```bash
cargo fmt --check
cargo clippy --locked --all-targets --features native,portable -- -D warnings
cargo test --locked --features native,portable
cargo test --locked --features native,portable actual_webrtc_loopback -- --ignored --nocapture --test-threads=1
```

O último comando inclui 60 s de transmissão sintética com perdas deliberadas e
checagem do tom de áudio de 440 Hz. Não exige captura do desktop nem servidor.
O teste do instalador é separado e deve ser feito numa VM/conta sem instalação
pessoal do Janja: `powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\test-windows-installer.ps1`.

### Compilação manual alternativa

No terminal **MSYS2 UCRT64**, atualize com `pacman -Syu`. Se o atualizador pedir
para fechar o terminal, reabra UCRT64 e repita. Depois:
```bash
pacman -S --needed --noconfirm mingw-w64-ucrt-x86_64-rust \
  mingw-w64-ucrt-x86_64-gcc mingw-w64-ucrt-x86_64-pkgconf \
  mingw-w64-ucrt-x86_64-python mingw-w64-ucrt-x86_64-gtk3 \
  mingw-w64-ucrt-x86_64-gstreamer mingw-w64-ucrt-x86_64-gst-plugins-base \
  mingw-w64-ucrt-x86_64-gst-plugins-good mingw-w64-ucrt-x86_64-gst-plugins-bad \
  mingw-w64-ucrt-x86_64-gst-plugins-ugly mingw-w64-ucrt-x86_64-gst-libav \
  mingw-w64-ucrt-x86_64-opus mingw-w64-ucrt-x86_64-libnice mingw-w64-ucrt-x86_64-nsis
cd /c/Users/Caixa/Downloads/janja-nativo
python scripts/package-native.py --diagnostics
```
Alternativa pelo PowerShell/SSH, após instalar MSYS2:
```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\build-windows.ps1 -Diagnostics
```
O script atualiza MSYS2, instala as dependências e compila. `-Diagnostics`
preserva um terceiro executável separado, `dist/janja-diagnostico-windows.exe`,
para os autotestes. Omitir essa opção gera somente o cliente comum e o instalador.

Resultados: `dist/janja-windows.exe` (portátil) e `dist/janja-setup-windows.exe` (instalador).
Use o Cargo de /ucrt64/bin, não o Cargo MSVC usado para compilar o servidor.
A instalação MSYS2 é necessária somente na máquina que compila.

Os pacotes portáteis incluem bibliotecas nativas. Na primeira abertura, extraem
o conteúdo no cache do usuário; depois reutilizam esse cache. Portanto há um
único arquivo para distribuir, mas o runtime é extraído no disco.
Windows: %LOCALAPPDATA%\Janja\runtime. Linux: ~/.cache/janja/runtime.

### Instalar e distribuir no Windows

Envie somente `janja-setup-windows.exe`. Ele inclui o portátil completo com GTK,
GStreamer, codecs e plugins; não baixa runtimes nem exige MSYS2 na máquina de destino.
Requer Windows 10/11 de 64 bits e os drivers de áudio/vídeo do sistema.

O instalador usa `%LOCALAPPDATA%\Programs\Janja`, sem administrador, cria atalhos
na área de trabalho e no menu Iniciar, e registra Janja em **Aplicativos instalados**.
Feche o Janja e execute o novo instalador para atualizar. As preferências e o cache
de runtime são preservados ao desinstalar, pois também atendem às cópias portáteis.
Instalar não resolve perda de pacotes, problemas de driver ou ausência de áudio
renderizado pelos aplicativos.

Para compilar via SSH, use `scripts/build-remote-windows.ps1`; ele coloca os dois
executáveis em `$HOME\Downloads`. O ZIP de fontes precisa estar em
`$HOME\janja-nativo-fontes.zip`. O script não atualiza o servidor sem `-UpdateServer`.

O CI Windows testa instalação silenciosa, versão do app, reinstalação e
desinstalação com caminho contendo espaços e acento, preservando um arquivo alheio.
Para repetir numa VM/conta sem Janja instalado:
`powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\test-windows-installer.ps1`.

Na r11, o receptor Windows usa Libav com threads limitadas. No autoteste da máquina
Windows acessada por SSH, a seleção automática de DXVA entregou 15–21 FPS;
Libav entregou 29,9 FPS no mesmo teste 1080p30, com áudio não silencioso.
Os encoders por hardware continuam disponíveis. Essa medição usa fontes sintéticas;
não comprova captura de jogo, saída física de áudio ou latência entre monitores.

## Validação e limites

Testes locais verificam limites de configurações, protocolo de presença,
SDP, codificação/decodificação sintética e extração do pacote.
Compilação e testes não medem latência real nem substituem teste em duas redes.

Sem TURN, determinadas combinações de NAT/firewall não estabelecem mídia P2P.
O Cloudflare Tunnel transporta somente sinalização; o vídeo/áudio não passa
pelo servidor Windows. Não há cobrança de relay ativada pelo aplicativo.

Ainda requer validação: executável Windows, portal Wayland real, captura de áudio,
drivers/GPU, aparência em tela e transmissão entre regiões. Não há promessa de
“zero lag”. Este é um primeiro cliente nativo para validação, não uma versão
de produção homologada.

Diagnóstico sem abrir a interface: gere com `python3 scripts/package-native.py --diagnostics`
e execute `./dist/janja-diagnostico-linux --check`. O cliente comum não inclui autotestes nem logs.
O empacotador executa esse diagnóstico e `--self-test` em uma extração limpa,
e só substitui o executável final se ambos passarem.

Referências técnicas: [GTK no Windows](https://www.gtk.org/docs/installations/windows),
[GStreamer WebRTC](https://gstreamer.freedesktop.org/documentation/webrtc/),
[captura de áudio WASAPI](https://gstreamer.freedesktop.org/documentation/wasapi2/wasapi2src.html).

## Atualizar os dois PCs por SSH

Todos precisam usar esta versão nativa: a negociação de mídia mudou. O servidor
continua igual. No Linux, na pasta do projeto:

```bash
scp -i /home/fernando/.ssh/windows_caixa dist/janja-nativo-fontes.zip caixa@192.168.1.11:janja-nativo-fontes.zip
ssh -i /home/fernando/.ssh/windows_caixa caixa@192.168.1.11
```

No PowerShell do Windows (feche o Janja antigo antes):

```powershell
Expand-Archive "$HOME\janja-nativo-fontes.zip" "$HOME\Downloads\janja-nativo" -Force
Set-Location "$HOME\Downloads\janja-nativo"
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\build-windows.ps1 -Diagnostics
if ($LASTEXITCODE -ne 0) { throw "Compilação falhou" }
$report = Join-Path $PWD 'teste-rede.json'
$p = Start-Process -FilePath '.\dist\janja-diagnostico-windows.exe' -ArgumentList @('--network-test', '--report', ('"' + $report + '"')) -Wait -PassThru
Get-Content $report
if ($p.ExitCode -ne 0) { throw "Autoteste WebRTC falhou; guarde o relatório" }
Start-Process .\dist\janja-windows.exe
```

No Linux:

```bash
./dist/janja-diagnostico-linux --network-test --report /tmp/janja-teste-rede.json
./dist/janja-linux
```

`--self-test` verifica vídeo H.264 1920×1080 e áudio Opus pelo caminho compartilhado
entre codificador e receptor, incluindo RTP e decodificação. `--network-test`
acrescenta dois pares WebRTC reais, ICE, DTLS/SRTP e UDP no mesmo PC.
Esses testes usam fontes sintéticas: não validam o portal, captura de tela real,
WASAPI, GPU ou internet. O campo `seconds_including_negotiation` inclui inicialização/negociação.
Os campos `received_fps`, `encoded_fps`, `raw_fps` e `encoder_input_fps` medem
três segundos após aquecimento; o teste reprova recepção abaixo de 24 FPS em 1080p30.

Depois teste Windows → Linux e Linux → Windows com uma imagem em movimento,
áudio e 1080p/30 FPS; confirme também transmissão simultânea nos dois sentidos,
parar/reiniciar, fechar uma janela capturada e reconectar. Em seguida repita entre
redes distintas. Guarde o codificador e FPS exibidos no app. A reserva medida do pipeline é registrada em `receiver_min_latency_ms`; não é
latência real entre monitores. Não há comparação de RAM aprovada nesta revisão.

### Gerar Windows pelo SSH a partir do Linux

```bash
cd /home/fernando/Downloads/janja
scp -i /home/fernando/.ssh/windows_caixa dist/janja-nativo-fontes.zip scripts/build-remote-windows.ps1 caixa@192.168.1.11:
ssh -i /home/fernando/.ssh/windows_caixa caixa@192.168.1.11 'powershell.exe -NoProfile -ExecutionPolicy Bypass -File C:/Users/Caixa/build-remote-windows.ps1'
scp -i /home/fernando/.ssh/windows_caixa caixa@192.168.1.11:Downloads/janja-windows.exe ./dist/janja-windows.exe
scp -i /home/fernando/.ssh/windows_caixa caixa@192.168.1.11:Downloads/janja-setup-windows.exe ./dist/janja-setup-windows.exe
```

O build usa MSYS2 em C:\msys64 e acesso à internet no Windows. O fonte é extraído em
`C:\Users\Caixa\Downloads\janja-nativo-r11`. O portátil e o instalador ficam em
`C:\Users\Caixa\Downloads`. Estes comandos compilam somente o cliente; não atualizam
o servidor de salas. Feche instâncias antigas antes de abrir o novo. Confirme a
versão com `janja-windows.exe --version`: `2026.11.12-r11-transmissao`.

Se a chave SSH pedir frase secreta, desbloqueie-a no seu terminal com
`ssh-add /home/fernando/.ssh/windows_caixa`, com um agente SSH ativo. Não inclua
senhas ou chaves privadas nos comandos, nos fontes ou nos arquivos distribuídos.

## Executável opcional para diagnóstico

No ambiente de compilação, acrescente `--diagnostics` ao comando de
`scripts/package-native.py`. Isso preserva o executável com logs e autotestes,
validado pelo empacotador, em `dist/janja-diagnostico-linux` ou
`dist/janja-diagnostico-windows.exe`. O CI disponibiliza esses arquivos em
artefatos separados; o cliente comum e o instalador continuam sem diagnóstico.

No Linux, execute `python3 scripts/diagnose-linux.py`: ele exige esse executável,
abre uma sessão e gera `relatorio.json` com eventos filtrados, áudio, CPU e RSS.
CPU de 100% corresponde a um núcleo ocupado; RSS está em bytes. O histórico
retém até 120 amostras e 20 mil eventos. Sem logs, o coletor informa erro e
não gera relatório vazio. Compartilhe somente o relatório, nunca o terminal
local nem os logs brutos.
