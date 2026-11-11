# Cliente leve

Versão recomendada: `janja-leve`. O cliente GTK/GStreamer anterior permanece experimental.
O executável Rust abre https://infocoretech.com.br/janja/ em uma janela do Edge,
Chrome ou Chromium instalado. Não embute Electron nem um navegador. Sem chave ou URL
para preencher; escolha um nome na primeira abertura. Ele fica salvo e é reutilizado nas próximas entradas na mesma sala pública. Não misture com clientes nativos antigos.

Capture tela/janela/aba pelo seletor do navegador. Padrão: 1080p, 30 FPS, até 12 Mbps por espectador; protocolos acrescentam tráfego.
Configurações salvas são preservadas: clique em Configurações → Usar recomendado → Salvar para trocar o padrão antigo.
Com três espectadores, o teto é aproximadamente 36 Mbps mais overhead, sujeito à rede.
A prévia local começa oculta; Mostrar prévia ativa apenas a exibição local, sem mudar a transmissão.
WebRTC faz captura, codecs, adaptação de rede e reprodução. Cada espectador exige
uma conexão/codificação; mais espectadores aumentam CPU e upload. Não há garantia
universal de aceleração por hardware nem de ausência de atraso.

O servidor fornece interface e sinalização. O vídeo não passa pelo Cloudflare.
Sem TURN, alguns pares de redes não conectarão. A interface informa falha de
conexão ou ausência de imagem. Uma página/saúde online não comprova acesso P2P.

Áudio depende da fonte, navegador e sistema. No Linux, áudio do sistema pode não
ser oferecido. Áudio da aba exclui Discord fora dela; não é um filtro universal.
Para programas externos, use entrada/cabo virtual com roteamento separado.
No Wayland, instale e mantenha ativo o xdg-desktop-portal e o backend do seu desktop.

## Atualizar o Windows pelo SSH

No Linux, a partir da pasta do projeto:

```bash
scp -i /home/fernando/.ssh/windows_caixa dist/janja-leve-fontes.zip caixa@192.168.1.11:janja-leve-fontes.zip
ssh -i /home/fernando/.ssh/windows_caixa caixa@192.168.1.11
```

No PowerShell remoto (administrador, como na instalação anterior):

```powershell
$ErrorActionPreference = 'Stop'
Expand-Archive "$HOME\janja-leve-fontes.zip" "$HOME\Downloads\janja-leve" -Force
cd "$HOME\Downloads\janja-leve"
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\build-leve-windows.ps1
if ($LASTEXITCODE -ne 0) { throw 'Compilacao falhou; nao atualize o servidor.' }
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\install-janja-server.ps1
if ($LASTEXITCODE -ne 0) { throw 'Instalacao falhou.' }
(Invoke-WebRequest -UseBasicParsing https://infocoretech.com.br/janja/).StatusCode
```

Requer o Rust/MSVC já usado para compilar o servidor. MSYS2 não é necessário.
Não altere o tunnel: mantenha `^/janja(/.*)?$` apontado para HTTP localhost:8787.
O cliente Windows está em `dist\janja-leve.exe`; distribua só esse arquivo.

## Linux

Use `dist/janja-leve-linux`, ou compile:

```bash
cargo build --release --locked --bin janja-leve
./target/release/janja-leve
```

Requer Chrome/Chromium/Edge instalado. No Arch: `sudo pacman -S chromium`.

## Verificar a transmissão real

Após atualizar o servidor, abra https://infocoretech.com.br/janja/ em dois PCs.
No primeiro, Transmitir tela, selecione uma janela em movimento. No segundo,
Assistir e Ativar áudio. Verifique movimento e som por um minuto, depois inverta
os transmissores. Repita entre provedores diferentes. Se a conexão expirar,
registre o aviso e os estados em chrome://webrtc-internals (Edge: edge://webrtc-internals).
Isso distingue falta de rota ICE de falha de captura/decodificação.

Testes: `npm test`, `cargo test`; teste WebRTC com captura simulada:
`npm install`, `npx playwright install chromium`, `cargo build`,
`JANJA_PUBLIC_TEST=1 npm run test:browser` (Linux). Use porta 8787 livre.

## Verificar Windows ↔ Linux e os 4 FPS

O mesmo cliente WebRTC é usado nos dois sistemas. Esta atualização exige recompilar e
reinstalar apenas o servidor e reabrir o aplicativo em todos os PCs; o launcher leve não mudou.

1. No Windows, Configurações → Usar recomendado → Salvar. Transmita uma janela com
   vídeo em movimento por 30 segundos para o Linux, com um espectador.
2. Durante a transmissão, abra Diagnóstico → Copiar relatório nos dois PCs. O relatório
   contém o último minuto de amostras, navegador, codec, resolução e FPS efetivos.
3. Pare, inverta os papéis e repita: Linux transmite, Windows assiste. Confirme também áudio,
   nome de cada participante e encerramento da captura. Áudio do sistema no Linux pode não
   estar disponível no navegador; teste áudio de aba para uma fonte suportada nos dois sistemas.
4. Compare captura (`sourceFps`), encoder/decoder (`fps`), largura/altura, `limitation`
   (`cpu`, `bandwidth`, `other`, `none` ou desconhecido), `processingMs` e `rttMs`.
   `null` significa métrica indisponível, não zero. Dados de codec/aceleração dependem do navegador.

Uma janela parada pode gerar poucos quadros; use movimento contínuo para avaliar FPS.
`getSettings().frameRate` não mede o FPS real; o diagnóstico usa diferenças dos contadores
WebRTC. O relatório não inclui SDP, IPs, nomes, chaves ou conteúdo capturado. Não é enviado ao servidor.

Os testes de política de captura/orçamento reproduziram preferências de FPS descartadas
e divisão de bitrate entre espectadores. Foram corrigidos sem forçar bitrate mínimo,
codec específico ou desativar adaptação de congestionamento. Isso não comprova que esses
fatores eram a única causa dos 4 FPS nesse Windows: a confirmação exige as medições acima.

Referências de implementação: [constraints W3C](https://www.w3.org/TR/mediacapture-streams/),
[estatísticas WebRTC](https://www.w3.org/TR/webrtc-stats/).
