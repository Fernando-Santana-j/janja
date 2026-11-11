# Player e desempenho — r6

## Interface

A transmissão passa a ocupar toda a área do player. Em tela cheia, a barra flutuante desaparece após três segundos sem atividade e reaparece com movimento ou teclado. Ela permanece acessível durante interação com os controles e navegação por teclado. Escape, duplo clique e o botão de restaurar continuam saindo da tela cheia; sair cancela o temporizador. A barra é uma sobreposição: ocultá-la não redimensiona nem reinicia o vídeo.

Assistir, fechar, volume, configurações, saída da sala e tela cheia usam ícones vetoriais desenhados pelo Cairo. Não dependem de fontes de símbolos, arquivos SVG, navegador ou carregadores extras. Botões preservam nomes de acessibilidade, dicas e foco visível. A ação principal mantém texto para ser reconhecível.

## Preferências e ajuste local

FPS, resolução, bitrate e opções de áudio ficam em `~/.config/janja/quality.json` (ou XDG_CONFIG_HOME) no Linux e `%APPDATA%/janja/quality.json` no Windows. A validação rejeita valores fora dos limites. A escrita usa arquivo temporário e substituição; alterações próximas são agrupadas e o fechamento normal também salva alterações pendentes. HWND/XID da captura, senhas de sala e senha mestra nunca são gravados nesse arquivo.

“Ajustar para este PC” mede o encoder com vídeo sintético em movimento, em uma thread separada. Testa primeiro o perfil escolhido e, se necessário, perfis inferiores até 720p/15 FPS. Não aumenta a qualidade solicitada nem altera bitrate ou opções de áudio. O resultado fica salvo; não existe benchmark pesado a cada abertura nem redução automática durante a transmissão. A medição não testa portal/DXGI reais, rede, CPU dos espectadores, jogos ou todos os padrões de imagem. Reavalie com a janela real em movimento.

## Custo de vídeo e áudio

O renderizador antigo recebia RGB e convertia novamente os pixels ao preparar a fonte Cairo. Agora recebe BGRx no Windows/Linux little endian e mantém uma superfície Cairo sobre o buffer mapeado. O mapa permanece vivo por meio dos dados da própria superfície; ela é privada e usada apenas para leitura. Stride, cores e duração do buffer têm teste de regressão. Não muda o H.264 enviado, resolução ou bitrate.

Microbenchmark local, 300 desenhos de 1920×1080: aproximadamente **2,959 ms/quadro → 0,708 ms/quadro**. É uma medição isolada de pintura em memória, não de desempenho completo nem de Windows. O número não implica eliminação de travamentos em qualquer PC.

Encoders de software têm limite explícito de 1–4 workers, reservando um processador lógico quando possível. O decoder FFmpeg também recebe esse limite, em vez de multiplicar workers automaticamente em cada transmissão. Filas continuam limitadas, e a recuperação de referências H.264 foi preservada.

Os peers usam SystemClock. As saídas GstAudioBaseSink passam a acompanhar esse relógio por resampling, em vez de deslocar o ponteiro de reprodução pelo método skew. Essa política é verificada nos testes sem abrir um dispositivo. O log recebido mostra underflows e correções de relógio, mas não prova a origem de toda falta de dados. Confirmar a melhora no PulseAudio/PipeWire real e em PCs fracos continua necessário; não foi alterado arbitrariamente o tamanho dos buffers ou a sinalização do Cloudflare.

## Produto sem diagnóstico

O empacotador primeiro testa uma compilação temporária com `native,diagnostics`. Depois recompila o cliente e o launcher sem essa feature e gera novamente o pacote final. A distribuição não inclui testes, scripts, README de desenvolvimento, comandos --check/--self-test/--network-test ou módulo de logs automáticos. O plugin audiotestsrc também é retirado. videotestsrc permanece porque é usado pelo ajuste de qualidade local. Bibliotecas, plugins utilizados, ícone, configurações de fontes e licenças são mantidos.

Somente para desenvolvimento: `cargo run --features native,diagnostics --bin janja -- --self-test`. Nessa compilação os logs usam fila limitada e escrita fora das threads de mídia. O produto normal não cria essa thread nem arquivos de log.

## Senha mestra

A senha solicitada foi preparada como hash PBKDF2-HMAC-SHA256, salt aleatório de 16 bytes, 100.000 iterações e hash de 32 bytes, em `dist/janja-master-password.json`. Ela inclui o ponto final fornecido pelo proprietário. Esse arquivo é privado e separado: não entra no cliente nem no ZIP de fontes. Não há senha em texto puro no código.

Envie-o para a pasta pessoal do usuário do servidor junto com o script remoto. `build-remote-windows.ps1 -UpdateServer` instala o hash em `C:/ProgramData/Janja/master-password.json`, sob as permissões do diretório do serviço. O servidor lê o arquivo ao iniciar; alternativamente, JANJA_MASTER_FILE define o caminho. Sem arquivo, a entrada por senha mestra fica desativada. JSON inválido falha na inicialização. `masterPasswordConfigured` no health confirma o carregamento sem publicar salt/hash.

Digite a senha mestra no campo normal da sala. Ela ignora apenas a senha específica; continuam valendo capacidade de oito pessoas, limites de mensagens/tentativas e isolamento de sinalização. O hash instalado é preservado nas atualizações seguintes. Reiniciar o servidor remove as salas atuais, que são mantidas em memória.

## Verificação e limites

46 testes executados passaram; teste gráfico e WebRTC UDP ficaram ignorados, e um teste TCP foi excluído pela restrição de sockets deste ambiente. Clippy foi verificado tanto em produção quanto nos testes. O teste gráfico existente também cobre ocultação/reaparecimento, restauração, layout e seletor, mas não foi executado aqui. Os testes locais cobrem senha mestra correta/incorreta/desativada, capacidade e isolamento, persistência, política de áudio, orçamento de threads, calibração e fidelidade dos pixels.

A GUI e o Windows ainda precisam de execução real. O seletor Windows continua sendo o seletor do app com miniaturas DWM; não foi trocado pelo GraphicsCapturePicker oficial. O tráfego entre cidades e o dispositivo de áudio do usuário não foram reproduzidos neste ambiente.

Referências: [sincronização de áudio GStreamer](https://gstreamer.freedesktop.org/documentation/audio/gstaudiobasesink.html), [threads x264](https://gstreamer.freedesktop.org/documentation/x264/index.html), [threads OpenH264](https://gstreamer.freedesktop.org/documentation/openh264/openh264enc.html).
