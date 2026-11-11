Atualização seguinte: [bloqueio do logger, negociação e sala integrada](sala-integrada.md).

# Diagnóstico de negociação e FPS — 05/11/2026

## Correção reproduzida

O receptor ligava `decodebin` diretamente à saída de vídeo. A saída automática
pode exigir um formato diferente do produzido pelo decoder. Acrescentado
`videoconvert` na função de recepção compartilhada pelo app e pelos testes.

Teste `receiver_negotiates_rgb_display_from_h264_i420`: envia H.264 a partir de
I420 e exige RGB na saída. Antes: nenhum quadro decodificado chegava à saída.
Depois: quadros chegam e o fluxo termina corretamente. O teste exige quadros;
receber apenas EOS não significa reprodução bem-sucedida.

Isso corrige uma incompatibilidade comprovada no código. O erro `not-negotiated`
do PC ainda precisa ser correlacionado aos novos logs: `nicesrc` pode relatar a
falha de um elemento posterior. Referência oficial:
https://gstreamer.freedesktop.org/documentation/tutorials/basic/handy-elements.html

O autoteste agora usa a mesma função de recepção de vídeo do aplicativo e mede
três segundos após aquecimento. Falha se a recepção sintética 1080p30 ficar abaixo
de 24 FPS. Isso não mede captura real, rede externa ou saída gráfica.

## Coletar dados nos dois computadores

Atualize os dois clientes com os novos arquivos. O servidor não precisa mudar.
Use 1080p/30 FPS/8 Mbps, compartilhe conteúdo em movimento durante 20–30 segundos,
clique em assistir no outro PC e feche os aplicativos após reproduzir o problema.
Não altere os parâmetros durante essa medição. Repita no sentido oposto se possível.

Os logs são automáticos, JSON por linha, até 8 MiB por processo, com retenção dos
quatro últimos arquivos. Não gravam tela nem áudio; avisos/estatísticas de rede
podem conter endereços IP e caminhos locais. Compartilhe-os privadamente.

Windows, no PowerShell:

```powershell
Compress-Archive -Path "$env:LOCALAPPDATA\Janja\logs\*.jsonl" -DestinationPath "$HOME\janja-logs-windows.zip" -Force
```

Linux:

```bash
tar -czf /tmp/janja-logs-linux.tar.gz -C "${XDG_STATE_HOME:-$HOME/.local/state}/janja" logs
scp -i /home/fernando/.ssh/windows_caixa caixa@192.168.1.11:janja-logs-windows.zip /tmp/
```

Arquivos necessários: `/tmp/janja-logs-linux.tar.gz` e `/tmp/janja-logs-windows.zip`.
Se os diretórios não existirem, verifique se abriu a versão nova. O primeiro
registro deve conter `native-room-20261105`. Para escolher outro diretório,
defina `JANJA_LOG_DIR` antes de abrir o app.

## Como localizar a queda

- `capture-metrics.raw_fps`: quadros fornecidos pela fonte.
- `encoder_input_fps`: quadros após conversão/escala, entrando no encoder.
- `encoded_fps` e `encoded_mbps`: saída efetiva, não valores desejados.
- `peer-metrics`: totais de quadros/áudio decodificados; calcule diferenças entre
  amostras usando `at_ms`. `webrtc-stats` traz os contadores disponíveis no runtime.
- `caps`: formato, resolução, framerate negociado e tipo de memória por etapa.
- `encoder-attempt`/`capture-ready`: encoder escolhido e erros dos anteriores.
- `peer-element`: decoder e saída realmente escolhidos, com IDs da conexão.
- `gstreamer`/`pipeline`: avisos e formatos dos elementos quando há erro.

`raw_fps` baixo já aponta para captura/backpressure; fonte alta com entrada baixa
aponta para conversão/escala/encoder que não acompanha. Entrada alta e saída baixa
apontam para descarte ou atraso do encoder. Saída alta e recepção baixa exigem
examinar transporte/decoder. Essas distinções orientam a próxima medição; não
substituem perfil de CPU/GPU. Tela parada pode produzir poucos quadros naturalmente.

Não houve prova de que os 8 FPS relatados foram resolvidos: faltam medições de
captura real nos PCs. Não foi feito ajuste de bitrate ou troca de codec às cegas.

## Resultado local desta revisão

Clippy sem avisos; 30 testes aprovados (26 de biblioteca, 4 de executáveis).
O teste UDP permanece ignorado e o teste WebSocket real excluído neste ambiente
por bloqueio de sockets. Logs JSON e os eventos de formato nas três etapas foram
verificados em execução do executável empacotado. A primeira medição do pacote
foi 29,84 FPS em 1920×1080, por 3,02 segundos após aquecimento, usando OpenH264.
Não é benchmark de desktop real nem comprovação de desempenho em Windows.
