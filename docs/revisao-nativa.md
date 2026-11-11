# Revisão nativa — 05/11/2026

## Resultado

Cliente Rust/GTK/GStreamer sem navegador. O servidor de sinalização existente
continua compatível. Todos os participantes precisam atualizar o cliente nativo.
A presença pública sem chave não é uma sala privada: quem conhece o endereço
pode entrar. O nome salvo não autentica identidade.

## Problemas corrigidos e evidência

- Timestamps: o encoder pode entregar PTS com um deslocamento grande no segmento.
  O repasse agora converte PTS em running-time antes de ajustar os relógios das
  conexões. Teste reproduziu 3.600.001 segundos onde deveria haver 1 segundo e
  passou após a correção. Isso corrige o caminho nativo; não prova a causa da
  qualidade baixa no antigo cliente Chrome.
- Falha em um sentido encerrava também o outro quando duas pessoas transmitiam
  entre si. Teste reproduziu a perda da assinatura e passou após separar a limpeza
  pelo identificador da transmissão. A mesma regra vale para saída da sala e erros.
- Respostas de negociação antigas podiam atingir uma conexão nova. Geração local
  e sessão de mídia agora descartam callbacks e sinais antigos; testes cobrem isso.
- Captura/portal bloqueavam o processamento de comandos. A inicialização roda fora
  da thread de controle, com descarte do resultado se houver cancelamento/reconexão.
- Captura só é anunciada depois de produzir vídeo e áudio quando solicitado.
  Falhas mostram o codificador e erro real. Removida propriedade inválida de QSV.
- Portal respeita os modos de cursor e fontes oferecidos pelo desktop.
- Enumerar uma janela X11 que acaba de fechar não deve abortar o app: as consultas
  usam a proteção de erros do GDK.
- O extrator do pacote deixa de permanecer na memória durante o uso normal.

## Comportamento entregue

Nome persistente, sala fixa, seleção de tela/janela conforme plataforma,
720/1080/1440p, 15/30/60 FPS, 1–20 Mbps, H.264 e Opus, codificador compartilhado
entre espectadores, filas limitadas e múltiplas transmissões recebidas.
Padrão 1080p/30 FPS/8 Mbps. Hardware é tentado antes do software, sem garantia de
aceleração em drivers não testados. Áudio recebido é silenciado durante captura
do áudio do sistema para evitar retorno. Exclusão de Discord é específica do
Windows com suporte a process loopback.

## Verificações

- `cargo clippy --offline --features native,portable --all-targets -- -D warnings`: passou.
- `cargo test --offline --features native,portable -- --skip real_websocket_auth_presence_capacity_and_cleanup`:
  28 testes aprovados; 1 teste UDP ignorado; 1 teste WebSocket real excluído porque
  este ambiente bloqueia sockets. Os testes de protocolo sem sockets passaram.
- O teste sintético de mídia valida 1920×1080 e pelo menos 90 quadros de vídeo e
  90 buffers de áudio decodificados usando a ponte de codificação compartilhada.
- `--network-test` exige sockets UDP e testa dois pares WebRTC no mesmo computador.
  Não foi aprovado neste ambiente: criação de UDP retorna operação não permitida.
- O workflow de build exige o teste WebRTC real em Linux e Windows, mas não foi
  executado aqui. O empacotador exige plugins e autoteste de mídia em extração limpa.

O executável Linux empacotado passou em extração limpa: `--check` e
`--self-test` com OpenH264, 1920×1080, 90 quadros e 302 buffers de áudio
decodificados. Arquivo final: `dist/janja-linux`, aproximadamente 71,2 MiB.
Isso é tamanho em disco, não consumo de RAM.

## Limites não homologados

Não foi possível testar uma sessão gráfica, GPU, captura real de tela/áudio,
Windows, Windows↔Linux, redes externas, consumo sustentado de RAM ou latência.
O seletor de janela no Windows exige WGC no plugin distribuído; confirmar na
máquina de destino. Linux/Wayland depende do portal e compositor. Vídeos abrem
em janelas nativas separadas, não dentro de um painel idêntico ao Discord.

P2P sem TURN não funciona em todas as combinações de NAT/firewall. O Cloudflare
Tunnel atende apenas sinalização. Bitrate é por espectador, sem adaptação
congestionamento equivalente à do navegador; configure conforme upload disponível.
Não há base para prometer 1440p60 ou ausência de travamentos em qualquer computador.

Use o roteiro de instalação e teste em [cliente-nativo.md](cliente-nativo.md).
