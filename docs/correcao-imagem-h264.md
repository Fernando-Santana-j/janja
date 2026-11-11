# Recuperação de imagem H.264 — 2026-11-09

Relatório analisado: `/tmp/janja-diagnostico-7kvbnlmi/relatorio.json`.
A captura Wayland/OpenH264 produziu aproximadamente 45–48 FPS, chegando a 8 Mbps.
Os contadores de vídeo/áudio recebidos zerados eram de peers de envio, não prova de ausência de mídia. O log agora identifica a direção.
O relatório não registra os pacotes perdidos no Windows: não permite afirmar que toda corrupção observada teve uma única causa.

Dois defeitos reproduzidos, com testes falhando antes das correções:

- A fila limitada de `appsrc` descartava referências H.264 e continuava enviando quadros dependentes. O teste força a perda do quadro inicial e sobrecarga durante uma sessão já iniciada. A proteção na saída da fila observa DISCONT, solicita um quadro completo e descarta dependentes até recebê-lo. Mantém a fila limitada e não bloqueia outros espectadores.
- O receptor entregava quadros dependentes após a perda de um quadro RTP. O teste remove pacotes de um quadro real produzido por OpenH264 e verifica que o primeiro quadro liberado após a perda é independente. O depayloader agora usa `wait-for-keyframe=true`, `request-keyframe=true` e saída em access units.

O pedido de quadro completo é encaminhado ao encoder sem reutilizar o tempo do peer, pois os pipelines têm tempos-base diferentes. O appsink anterior à ponte deixa de descartar referências silenciosamente; o descarte antes do encoder e depois da decodificação continua permitido.

Referências: https://gstreamer.freedesktop.org/documentation/app/appsrc.html e https://gstreamer.freedesktop.org/documentation/rtp/rtph264depay.html.

Validação: testes de regressão com filas reais de GStreamer e perda de RTP; suíte Rust offline; Clippy; autoteste do executável empacotado. O teste sintético não substitui captura de desktop e recepção entre Windows/Linux em redes reais.

Atualizar ambos os clientes. O servidor de sinalização/Cloudflare não precisa mudar. Uma perda poderá causar uma breve pausa até o quadro completo, em vez de exibir quadros com referências ausentes. Não há garantia de 60 FPS nem de rede sem perdas.
