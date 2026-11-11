# Interface e desempenho — 2026-11-10

Mudanças concretas:

- Removida a ramificação de prévia da captura, incluindo redução, conversão RGB e download D3D11 dessa ramificação.
- A imagem recebida mantém o buffer RGB mapeado vivo enquanto o GTK o usa. Não faz mais uma cópia completa em Vec por quadro. Corrigido também o stride: usa o alinhamento real do buffer, não o valor presumido pelas caps. Um teste com linhas de 16 bytes que eram interpretadas como 12 falhou antes da correção e agora passa.
- A apresentação segue o frame clock do GTK, substituindo o temporizador independente de 16 ms. Tiles ocultos deixam de consultar quadros.
- Atualizações da sala preservam os tiles, o volume e o último quadro. Não criam um novo decoder ao entrar/sair da tela cheia.
- Volume individual 0–100%, mute geral independente, fechar vídeo e tela cheia com Escape. Ganho escolhido antes da negociação do áudio é aplicado ao novo ramo de reprodução.
- Paleta e cores explícitas para botões, campos, menus, popovers, seleção, foco e estados desabilitados. Fontes Segoe UI/Noto Sans com fallback nativo.

Validação local: 34 testes da biblioteca passaram usando rustc com as dependências já compiladas, pois o cache de fontes Cargo está incompleto e o ambiente não resolve static.crates.io. Testes de UDP real e interface gráfica ficam ignorados neste ambiente; o teste WebSocket que exige sockets locais foi excluído. CSS validado pelo parser GTK. Clippy também passou com `-D warnings`, invocado diretamente sobre as mesmas dependências compiladas. Não foi uma execução de Cargo a partir de um cache limpo.

Teste visual disponível em desktop: `cargo test --features native tiles_keep_controls_and_restore_fullscreen_on_close -- --ignored --test-threads=1`.

Os testes reproduzem descarte de referências H.264, perda RTP e alinhamento de imagem; não reproduzem o travamento do mouse nos computadores dos amigos. FPS médio normal não garante cadência regular nem baixa carga de CPU/GPU. Não foi reduzida a qualidade configurada para esconder esse sintoma. Hardware, driver, resolução e número de espectadores continuam influenciando o custo.

Pacote Linux final: autoteste 1920×1080, 29,85 FPS recebidos, 180 quadros, 601 blocos de áudio, `preview: false`. A montagem reutilizou dependências release já compiladas e recompilou o código atual com rustc otimizado/LTO; o empacotador normal verificou o resultado. O ZIP mantém o fluxo Cargo normal para Windows/Linux.

Microbenchmark isolado (300 quadros RGB 1080p, mesmo buffer): preparação do Pixbuf antes 0,239 ms/quadro; agora 0,002 ms/quadro. Exclui captura, encoding, decoding e pintura Cairo; não representa a redução de CPU do aplicativo inteiro.

## Revisão r3

Interface reorganizada: cartões de participantes no topo, palco sem sidebar, dock com slider de volume geral e tela cheia sempre visíveis. Ganho geral multiplicativo, separado do volume individual; 25% individual × 40% geral = 10%, testado respeitando arredondamento do GStreamer. Feche a instância anterior antes de abrir a nova (GTK pode encaminhar a ativação à instância já aberta). `--version` identifica `2026.11.10-r3-native`.

Removidos assets web, lançadores browser, testes browser, Playwright e antigos executáveis de cliente. O binário `sala` é mantido apenas como entrada de servidor para os scripts antigos. HTTP/WSS continuam necessários à sinalização; não envolvem um navegador no cliente.

34 testes da biblioteca passaram, testes Cloudflare e Clippy também. Pacote r3: 1080p, 29,95 FPS, 601 buffers de áudio, preview desativado. CSS aceito pelo GTK. Teste gráfico tentado: GTK não inicializa neste ambiente; visual, tela cheia e controles ainda precisam de verificação no desktop. Build Windows por SSH não executado aqui.
