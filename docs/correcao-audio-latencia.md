# Áudio, latência e captura Linux — 05/11/2026

Evidência do log `janja-52098.jsonl`:

- Captura PipeWire de 1920×1200 negociou pixel-aspect-ratio 1/2147483647.
  A escala avisou que não conseguia calcular bordas. Agora a negociação da fonte
  exige pixels quadrados (1/1) antes da conversão/escala. O autoteste verifica isso.
- Configuração efetiva: 1440p, 60 FPS e 3 Mbps, com OpenH264 por software.
  Não havia encoder por hardware disponível no runtime usado naquela captura.
- O FPS bruto também caiu em diversas amostras. Não é possível atribuir toda a
  queda ao transporte: uma fonte Wayland parada pode enviar poucos quadros.
  Aumentar a resolução de 1920×1200 para 2560×1440 não adiciona detalhe à fonte.
- O receptor registrou milhares de buffers de áudio decodificados. O aplicativo
  silenciava a reprodução automaticamente durante captura do áudio do computador.
  Esse comportamento foi removido: o controle “Silenciar recepção” é explícito,
  desligado por padrão, e vale também para novas assinaturas. Ao transmitir e
  assistir simultaneamente, use-o se o som recebido voltar à transmissão.

A função que examinava o bus descartava mensagens LATENCY. Agora processa essas
mensagens e recalcula/distribui a latência, preservando os erros e EOS. O teste
`latency_bus_message_is_distributed_to_sources` falhou antes e passou depois.
Appsrc informa limites de latência em modo live. Referência oficial:
[latência no GStreamer](https://gstreamer.freedesktop.org/documentation/additional/design/latency.html).
Os avisos PulseAudio de leitura durante interrupção/reinício da captura não foram
reproduzidos aqui; não houve tentativa de escondê-los ou mudar parâmetros sem prova.

“Transmitir” abre a escolha de fonte: portal do compositor no Wayland, diálogo
com tela/janelas no Windows e X11. Cancelar não inicia transmissão.

Validação local: Clippy sem avisos; 35 testes passaram, incluindo distribuição de
latência, controles de mute, vídeo RGB/preview e mídia sintética. UDP real e a
sessão gráfica não estão disponíveis neste ambiente. Para conferir RTCP no PC:

```bash
cd /home/fernando/Downloads/janja
timeout 45s ./dist/janja-linux --network-test --report /tmp/janja-latencia.json
cat /tmp/janja-latencia.json
```

Depois teste captura real com conteúdo em movimento, primeiro em 1080p/30 FPS/
8 Mbps. Confira áudio com “Silenciar recepção” desmarcado. Repita os dois sentidos
Windows↔Linux, usando a mesma versão. Preserve os logs se houver erros RTCP
persistentes ou captura com baixa taxa durante movimento; a correção de software
não garante 1440p60 por CPU nem fidelidade alta em qualquer bitrate.
