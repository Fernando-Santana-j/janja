# Diagnóstico de 4 de novembro de 2026

## Evidência recebida

Windows transmite, Linux recebe. Relatório v3: alvo 1440p60, 12 Mbps por
espectador; VP8/libvpx, powerEfficientEncoder=false. Limitação reportada como
bandwidth, RTT 1–3 ms, envio 320×200 a 640×400. Após os primeiros segundos,
FPS de fonte/envio sobem de aproximadamente 1 para 18–26. O relatório não
contém estimativa de banda disponível, perdas, parâmetros efetivos do sender
nem diagnóstico do receptor. Não comprova link de internet lento, saturação
de CPU ou que o problema é exclusivamente codec/captura. O teto de bitrate
não é uma taxa mínima e não é a taxa efetiva.

O usuário rejeitou o consumo de RAM do navegador. O launcher pequeno não
representa o consumo agregado do Chrome; esse caminho não atende ao requisito.
Nenhum novo pacote foi publicado como correção definitiva nesta etapa.

## Plataforma encontrada

Linux local: Radeon 680M (Rembrandt), driver amdgpu, sessão Wayland. O ambiente
de execução não expõe /dev/dri para testar aceleração. GPU/driver do Windows
foram solicitados ao usuário; não inferir qual encoder existe no computador.

## Verificações realizadas

`cargo test --offline --features native --lib client::media::tests::software_video_encodes_and_decodes_without_a_display -- --nocapture`
passou. É um teste de encode/decode sintético; não testa captura, rede ou GUI.

Pipeline de medição, sem exibição nem sockets:

```bash
gst-launch-1.0 videotestsrc num-buffers=300 pattern=ball \
  ! video/x-raw,format=I420,width=1920,height=1080,framerate=30/1 \
  ! openh264enc complexity=low bitrate=8000000 \
  ! h264parse ! openh264dec ! fakesink sync=false
```

Exit 0; execução após preroll 5,464 s; processo inteiro incluindo inicialização
22,77 s; pico RSS do filho 291616 KiB (aproximadamente 285 MiB), obtido por
resource.getrusage(RUSAGE_CHILDREN) em um processo Python novo. Como sync=false
e a fonte não é live, não é uma medição de latência de transmissão nem FPS de
captura. Pattern=ball é conteúdo simples. Não extrapolar RAM/FPS para o app
completo, Windows ou múltiplos espectadores. O processo inclui encoder e decoder.

## Caminho necessário

Reparar o cliente nativo existente, preservando sala automática, identidade,
seletor de fonte, áudio e interface solicitada. Captura Windows via APIs nativas;
Wayland via portal/PipeWire. Usar encoder H.264 acelerado disponível com fallback
explicitamente identificado, filas limitadas, um encode compartilhado entre
espectadores e decodificação/renderização aceleradas. WebRTC nativo continua
podendo cuidar de ICE/DTLS/SRTP sem navegador; a rede pode continuar exigindo TURN.

O protótipo nativo não está pronto: captura ainda devolve erro genérico em certos
caminhos, seleção Windows e UI/nome não acompanham o cliente atual, reprodução
real entre PCs ainda não foi validada. A compilação/presença de plugins não é
critério de aceite. Necessário medir captura + vídeo/áudio recebido + memória
do processo em Windows→Linux e Linux→Windows, com movimento contínuo, antes
de substituir a versão distribuída. Dependência imediata: GPU e driver Windows.

Referências: https://www.w3.org/TR/webrtc-stats/ e
https://learn.microsoft.com/en-us/windows/apps/develop/media-authoring-processing/screen-capture .
