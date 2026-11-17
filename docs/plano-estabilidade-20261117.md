# Estabilidade de sessão e recuperação de mídia

Objetivo: preservar as conexões P2P quando a sinalização cair e recuperar áudio ou
vídeo interrompidos sem exigir fechar e abrir a transmissão. Escopo autorizado
pelo relato de 17/11; preservar codecs, filas, latência, interface e preferências.

Evidências iniciais (testes reproduzem as falhas antes da correção):

- `signaling_outage_preserves_the_room_and_existing_media`: `Disconnected` apaga
  sala, captura e espectadores imediatamente.
- `a_departed_or_slow_recipient_does_not_disconnect_the_sender`: uma corrida de
  saída ou fila cheia do destinatário é tratada como violação do remetente.
- O watchdog verifica a primeira imagem, mas não a continuidade posterior.
  Erros da mídia removem a inscrição e não a recuperam automaticamente.

Implementação incremental:

- [x] Servidor: mensagens em trânsito não expulsam remetentes válidos; manter
  isolamento de salas e validação contra mensagens maliciosas.
- [x] Servidor: retomada por segredo aleatório, limite de sessões e prazo de
  reserva de 120 segundos após desconexão; reconexão substitui o socket antigo
  sem permitir que seu encerramento remova a sessão nova.
- [x] Servidor: cache atômico de salas e sessões (apenas hashes dos segredos e
  das senhas), restaurado após reinício com prazo de retomada; falhas de gravação
  são explícitas. Sem gravar mídia ou instalar dependências novas.
- [x] Cliente: separar geração de mídia e de sinalização; preservar pipelines,
  sala e estado enquanto reconecta; uma retomada confirmada não limpa a mídia.
- [x] Cliente: detectar ausência prolongada de quadros/blocos decodificados,
  distinguir silêncio de falta de dados e tolerar oscilações curtas de ICE.
- [x] Cliente: reinscrever automaticamente com espera limitada e isolamento por
  tentativa; preservar volume, cancelamento manual e transmissões saudáveis.
- [x] Regressões: socket em memória, expiração, reinício, segredo inválido,
  concorrência entre sockets, perdas breves, áudio sem som, recuperação/cancelamento
  e contadores da biblioteca comum sem diagnóstico.
- [x] Executar fmt, clippy e testes permitidos; preparar validação Windows pelo
  SSH existente. O proprietário concluiu a validação e trouxe os artefatos r14.
- [x] Executar WebRTC UDP, carga Windows, instalação e Defender fora deste
  ambiente, onde os sockets locais e o SSH estão bloqueados.
- [ ] Instalar o servidor, publicar os clientes e verificar sessão real prolongada.

Detalhes de recuperação:

- O segredo de retomada tem 192 bits aleatórios; somente seu SHA-256 é salvo.
  O cliente guarda o segredo na conexão em memória, sem criar um login novo.
- Até 32 sessões e 32 salas, oito participantes por sala e cache de até 64 KiB.
  O serviço Windows recebe permissão de escrita somente na pasta privada do
  cache, sem ganhar permissão para substituir seu executável.
- Depois de uma desconexão conhecida, a reserva vence em 120 segundos. Sessões
  ativas no último snapshot recebem 120 segundos a partir da volta do servidor;
  isso permite preservar P2P durante uma indisponibilidade mais longa. Não é um
  prazo contado desde a última gravação de uma sessão que continuava ativa.
- Uma visualização com pacotes chegando e decodificação parada por seis segundos
  é recriada com espera de 1, 2, 4, 8 e no máximo 15 segundos entre falhas. Volume,
  captura compartilhada e visualizações saudáveis são preservados. Não se
  interpreta ausência legítima de pacotes como falha de decodificação.
- Cancelamentos têm identificador e confirmação do transmissor; são reenviados
  quando falta confirmação, no máximo uma vez por segundo ou ao reconectar.
  Confirmações antigas não cancelam uma visualização nova. Saída explícita da
  sala continua encerrando os pares, inclusive com outros sockets reconectando.

Verificação local em 17/11:

```bash
cargo fmt --all -- --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked --features native,portable -- --test-threads=1 \
  --skip private_bus_checks_session_and_portal_acquire_and_release \
  --skip real_websocket_auth_presence_capacity_and_cleanup
python3 scripts/test-updates.py
python3 scripts/test-publication.py
```

- Rust: 108 testes da biblioteca, três do portátil e dois do runtime comum
  aprovados (113 no total). Cinco testes permanecem ignorados: WebRTC UDP,
  carga sintética e GUI. Os dois filtros acima correspondem a tentativas de
  abrir sockets D-Bus/TCP que falham com `Operation not permitted` no ambiente.
  O fluxo Windows completo executa a suíte sem esses filtros.
- Atualizador: 11 testes aprovados. Publicação: dez cenários aprovados.
- Cliente e servidor compilados em release. O `--check` confirmou bibliotecas e
  plugins; o autoteste da compilação final entregou 29,96 FPS em 1080p, 180 quadros
  e 607 blocos de áudio, com pico de áudio 0,0506. É uma ponte de encoder local,
  sem transporte UDP, e não mede a latência ponta a ponta.
- O teste `engine_recovers_real_decoded_audio_after_signaling_outage` usa H.264 e
  Opus reais, interrompe a saída decodificada do áudio enquanto o vídeo avança e
  verifica recuperação da imagem e de áudio com som. A injeção RTP ocorre na
  entrada do receptor: não substitui um teste de rede UDP entre computadores.
- O teste de reinício usa WebSockets em memória e cache real em disco; mantém
  identidade, sala privada e estado de transmissão. Também verifica segredo
  inválido, expiração e encerramento do socket substituído sem afetar o novo.
- O receptor da biblioteca comum conta também blocos de silêncio decodificados,
  sem ativar os handoffs de diagnóstico. Os novos contadores são atômicos e não
  criam filas de frames, logs ou processos auxiliares.
- Revisão independente de concorrência, cache e confirmações de cancelamento
  concluída sem bloqueadores remanescentes.

Comparação sintética exploratória com a r13: três pares alternados do encoder
compartilhado em 1080p/30, com as mesmas bibliotecas GStreamer e sem custo de
extração do portátil. CPU total de processo na r13: 4,165 / 3,761 / 3,491 s;
na candidata r14: 3,632 / 3,477 / 3,523 s. Pico RSS: 113.936–117.636 KiB na r13
e 116.252–118.080 KiB na r14. Ambas entregaram aproximadamente 30 FPS e áudio
decodificado com som. A amostra é curta, possui variação entre execuções e não
mede o fluxo completo de recuperação nem a confirmação de cancelamento adicionada
depois; **não comprova redução de CPU ou memória, nem latência ponta a ponta**.

Próximos passos externos e comandos: ver `docs/atualizacoes.md`, seção r14.
O script `publish-release.sh --local` inclui compilação e validação Windows,
mas a instalação do servidor é uma etapa separada e explícita.

Validação Windows trazida pelo proprietário em 17/11:

- 108 testes da biblioteca, dois do portátil e dois do runtime aprovados.
  Os três testes WebRTC ignorados na suíte padrão e o teste de carga foram
  executados separadamente e aprovados: 116 testes no total; GUI não executada.
- Perda deliberada de 12 pacotes em 60 segundos: aproximadamente 30 FPS,
  maior intervalo entre quadros de 115 ms e áudio decodificado com som. Esses
  intervalos não representam uma medição de latência ponta a ponta.
- Carga sintética: vídeo entre 29,80 e 29,96 FPS; a carga D3D11 manteve cerca
  de 60 FPS. Isso não reproduz todo jogo ou combinação de hardware.
- Instalação, reinstalação, atualização real, rollback e desinstalação aprovados.
  Defender sem detecções nos dois arquivos; Authenticode continua `NotSigned`.
- `dist/windows.build.json` confirma `2026.11.17-r14-estavel`. Os SHA-256 dos
  executáveis locais foram conferidos contra esse arquivo e o relatório do
  Defender. `janja-nativo-r12` continua apenas como nome da pasta de compilação
  remota; não determina a versão compilada.

Riscos e limites: não é possível sustentar P2P sem a rede entre os participantes.
Novas entradas/negociações dependem da volta do servidor. O cache precisa permanecer
no servidor e não pode ser publicado. Não há comprovação do evento original sem
logs; os testes distinguem falhas reproduzidas de causas ainda não confirmadas.
