# Estabilidade de mídia — r7, 11/11/2026

Versão: `2026.11.11-r7-network`. Atualize os dois clientes para obter negociação de retransmissão nos dois lados. O protocolo do servidor permanece 2.

## Correções verificadas

| Caminho | Antes | Agora |
|---|---|---|
| Receptor com saída artificialmente lenta (80 ms/quadro) | Cerca de 782 ms de imagem atrasada após 15 quadros | 80–130 ms nos testes locais, com decoder automático e OpenH264 |
| Oferta/resposta WebRTC | Sem NACK genérico/RTX H.264 | Negocia pedidos de retransmissão e o codec RTX |
| Áudio com um pacote atrasado 90 ms e outro perdido | A janela de 60 ms descartava ambos | A janela de 150 ms recupera o atrasado; o decoder disfarça a perda restante |
| ICE recebido no `welcome` | Ignorado; somente STUN fixo | Aplicado a novos emissores/receptores, inclusive TURN e TURN/TLS |
| Oferta Opus antes do primeiro buffer | Não anunciava os dois canais RTP | Anuncia `/48000/2`, estéreo e suporte a FEC |

O player usa QoS e limite de 100 ms de atraso em relação ao horário esperado de apresentação. Assim o decoder pode reagir ao processamento atrasado, sem acrescentar uma fila de vídeo. Isso pode descartar quadros em PCs lentos; não torna a CPU/GPU capaz de sustentar qualquer resolução. A sincronização com áudio foi mantida.

Opus usa blocos de 20 ms em vez de 10 ms, reduzindo a frequência de pacotes. Encoder e decoder habilitam FEC, quando o modo Opus conseguir usá-lo, além da ocultação de perdas (PLC). A fila de envio de áudio tolera até 120 ms, com limite adicional de buffers. O jitterbuffer tem descarte por latência e notificação de perdas. Esses limites evitam armazenamento ilimitado; não eliminam o atraso de captura, codec, rede ou dispositivo.

Não foi alterada a resolução, FPS ou taxa escolhida pelo usuário. Não foi introduzido um instalador: o extrator portátil já encerra sua participação antes do uso normal.

## Validação local

```bash
cargo test --locked --features native,portable -- --skip real_websocket_auth_presence_capacity_and_cleanup
cargo clippy --locked --features native,portable --all-targets -- -D warnings
npm test
python3 scripts/package-native.py
```

Passaram 53 testes Rust (50 da biblioteca e 3 do portátil) e 3 testes Cloudflare. Clippy sem avisos. UDP e GUI ignorados conforme as restrições do ambiente; WebSocket real excluído após confirmar bloqueio de sockets. Formatação verificada nos arquivos alterados.

O empacotador verificou extração limpa, plugins e fluxo real encoder → RTP → decoder → appsink. Resultado: 1920×1080, **30,13 FPS entregues ao appsink**, 181 quadros e 303 buffers de áudio. A medição de FPS durou 3,02 s após aquecimento. O autoteste agora conta os quadros aceitos pela saída de vídeo; contabilizar apenas frames decodificados poderia esconder descartes. Isso ainda não mede pintura GTK nem captura real.

## Testar nos PCs

1. Feche todas as instâncias anteriores. No Linux, confira `./dist/janja-linux --version` antes de abrir o app.
2. No Windows, extraia `dist/janja-nativo-fontes-r7.zip` numa pasta nova e execute `powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\build-windows.ps1`. Esse script exige MSYS2 e pode baixar/atualizar dependências. O resultado será `dist/janja-windows.exe`. O `.exe` antigo desta pasta não foi atualizado no Linux.
3. Compartilhe o mesmo conteúdo em movimento com áudio por 30 s. Anote resolução/FPS/taxa escolhidos, espectadores e os valores de captura/encoder/Mbps do rodapé. Compare primeiro um espectador e depois o grupo habitual.
4. Para investigar a sirene, coloque o volume geral **do Janja** em zero no transmissor, mantendo a transmissão ativa. Se o problema desaparecer, investigue áudio recebido que está sendo recapturado pelo loopback. Esse teste não comprova sozinho a causa.

## Limites e próximos passos

A sirene não foi reproduzida aqui. Não houve teste de WASAPI, captura Windows, portal Wayland real, GPU, internet ou rota entre os PCs. O pacote Linux foi compilado com as bibliotecas desta máquina; para distribuição em sistemas mais antigos, use o job Ubuntu 24.04 existente.

TURN precisa existir e ser configurado no servidor; esta revisão faz o cliente usá-lo, mas não instala um relay. O runtime aceita um STUN e vários TURN. STUN/TLS não é suportado e é rejeitado explicitamente. As credenciais são escapadas e configurações malformadas são rejeitadas sem incluí-las nas mensagens de erro.

A taxa de vídeo continua fixa **por espectador**. Muitos espectadores podem saturar o upload com CPU baixa. Se as medições reais confirmarem congestionamento, o próximo trabalho é controle adaptativo de taxa com feedback RTCP e teste de rede controlado. Uma reestruturação geral não resolve esse limite por si só.

Referências: [QoS](https://gstreamer.freedesktop.org/documentation/additional/design/qos.html), [jitterbuffer](https://gstreamer.freedesktop.org/documentation/rtpmanager/rtpjitterbuffer.html), [Opus encoder](https://gstreamer.freedesktop.org/documentation/opus/opusenc.html), [WebRTC](https://gstreamer.freedesktop.org/documentation/webrtc/).
