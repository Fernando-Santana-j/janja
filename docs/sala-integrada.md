# Sala integrada — 05/11/2026

Interface nativa GTK com participantes à esquerda, vídeos em grade no centro,
controles na parte inferior e configurações em um painel aberto pelo botão.
Referência visual: disposição de chamada do Discord, sem logos ou reprodução
pixel a pixel. A aparência ainda precisa ser conferida em sessão gráfica real.

O preview local é uma ramificação da captura: 480×270, 10 FPS, sem recodificar
H.264 e sem reproduzir áudio. Uma fila descarta quadros antigos para não bloquear
a transmissão. A recepção entrega RGB ao próprio app, com no máximo um quadro
pendente por vídeo. A apresentação usa desenho GTK, preservando proporção.
Há conversão/cópia para a memória da interface; não é renderização sem cópias.

Falha confirmada após o teste real do usuário: o ghost pad criado automaticamente
pelo parser apontava para uma entrada de vídeo bruto ainda não ligada ao decodebin,
em vez da fila RTP. Isso reproduziu `Pads do not have common format`. O receptor
agora cria explicitamente o ghost pad na fila RTP. O teste offline usa o mesmo bin
da recepção real; não liga mais uma cadeia contínua que contornava esse defeito.

Mudanças de negociação:

- SSRC de vídeo/áudio definido no payloader e nos caps antes da oferta SDP.
- Publicação do SDP somente após conclusão de set-local-description, com erro tratado.
- Autoteste cria WebRTC sem STUN desde o início, sem configurar endereço vazio.
- Diagnóstico não consulta nomes/caminhos de objetos dentro do callback de log
  do GStreamer, evitando readquirir mutex de objeto durante avisos.

Os logs anteriores do usuário mostram cerca de 48 FPS codificados em Linux e
interrupção dos registros ao criar o transporte WebRTC. Isso distingue o problema
atual da alegação anterior de 8 FPS. Não prova conexão entre sistemas ou redes.

O autoteste também exige um quadro RGB 1920×1080 entregue ao app e um quadro
480×270 do preview. Uma imagem capturada localmente não comprova que outro PC a recebeu.

Após atualizar todos os clientes, execute:

```bash
timeout 45s ./dist/janja-linux --network-test --report /tmp/janja-rede.json
cat /tmp/janja-rede.json
```

Confira `test: webrtc-loopback`, `received_fps`, `display` e `preview`. O teste usa
UDP real no mesmo computador, sem o servidor. Depois valide as duas direções entre
Windows/Linux. Para os caminhos e coleta dos logs, veja `diagnostico-atual.md`.

Validação local: 34 testes aprovados; Clippy sem avisos. O teste de bloqueio mantém
um mutex de objeto GStreamer ocupado: a consulta antiga ao caminho falha por
espera; a identificação nova não espera pelo mutex. O teste da oferta falhava
pela ausência de SSRC antes da correção e passou depois. O pacote também passou
com preview RGB e imagem RGB 1920×1080, aproximadamente 30 FPS sintéticos.
O teste de rede real depende do terminal do usuário porque este ambiente bloqueia UDP.
