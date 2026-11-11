# Atualização r7

Correções atuais, testes e instruções para validar Windows → Linux: [estabilidade r7](estabilidade-r7.md). A versão atual é `2026.11.11-r7-network`; as instruções r6 abaixo são históricas.

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
é a opção prática para desktops). Discord precisa estar aberto. Uma única
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
sudo apt-get install -y build-essential pkg-config python3 libgtk-3-dev libdbus-1-dev \
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

No terminal **MSYS2 UCRT64**, atualize com `pacman -Syu`. Se o atualizador pedir
para fechar o terminal, reabra UCRT64 e repita. Depois:
```bash
pacman -S --needed --noconfirm mingw-w64-ucrt-x86_64-rust \
  mingw-w64-ucrt-x86_64-gcc mingw-w64-ucrt-x86_64-pkgconf \
  mingw-w64-ucrt-x86_64-python mingw-w64-ucrt-x86_64-gtk3 \
  mingw-w64-ucrt-x86_64-gstreamer mingw-w64-ucrt-x86_64-gst-plugins-base \
  mingw-w64-ucrt-x86_64-gst-plugins-good mingw-w64-ucrt-x86_64-gst-plugins-bad \
  mingw-w64-ucrt-x86_64-gst-plugins-ugly mingw-w64-ucrt-x86_64-gst-libav \
  mingw-w64-ucrt-x86_64-libnice
cd /c/Users/Caixa/Downloads/janja-nativo
python scripts/package-native.py
```
Alternativa pelo PowerShell/SSH, após instalar MSYS2:
```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\build-windows.ps1
```
O script atualiza MSYS2, instala as dependências e compila.

Resultado: `dist/janja-windows.exe`.
Use o Cargo de /ucrt64/bin, não o Cargo MSVC usado para compilar o servidor.
A instalação MSYS2 é necessária somente na máquina que compila.

Os pacotes portáteis incluem bibliotecas nativas. Na primeira abertura, extraem
o conteúdo no cache do usuário; depois reutilizam esse cache. Portanto há um
único arquivo para distribuir, mas o runtime é extraído no disco.
Windows: %LOCALAPPDATA%\Janja\runtime. Linux: ~/.cache/janja/runtime.

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

Diagnóstico do pacote sem abrir a interface: `./dist/janja-linux --check`.
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
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\build-windows.ps1
if ($LASTEXITCODE -ne 0) { throw "Compilação falhou" }
$report = Join-Path $PWD 'teste-rede.json'
$p = Start-Process -FilePath '.\dist\janja-windows.exe' -ArgumentList @('--network-test', '--report', ('"' + $report + '"')) -Wait -PassThru
Get-Content $report
if ($p.ExitCode -ne 0) { throw "Autoteste WebRTC falhou; guarde o relatório" }
Start-Process .\dist\janja-windows.exe
```

No Linux:

```bash
./dist/janja-linux --network-test --report /tmp/janja-teste-rede.json
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
redes distintas. Guarde o codificador e FPS exibidos no app. Não há medição
comparativa de RAM ou latência real aprovada nesta revisão.

### Gerar Windows pelo SSH a partir do Linux

```bash
cd /home/fernando/Downloads/janja
scp -i /home/fernando/.ssh/windows_caixa dist/janja-nativo-fontes.zip scripts/build-remote-windows.ps1 caixa@192.168.1.11:
ssh -i /home/fernando/.ssh/windows_caixa caixa@192.168.1.11 'powershell.exe -NoProfile -ExecutionPolicy Bypass -File C:/Users/Caixa/build-remote-windows.ps1 -UpdateServer'
scp -i /home/fernando/.ssh/windows_caixa caixa@192.168.1.11:Downloads/janja-windows.exe ./dist/janja-windows.exe
```

O build usa MSYS2 em C:\msys64 e acesso à internet no Windows. O fonte é extraído em uma pasta r6 para não misturar arquivos dos antigos clientes. O executável fica em C:\Users\Caixa\Downloads\janja-windows.exe. Feche instâncias antigas antes de abrir o novo. A versão Linux pode ser confirmada com `./dist/janja-linux --version`: `2026.11.10-r6-player`.
