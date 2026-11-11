# Interface, identidade e transmissão

Escopo: interface pública web, captura compartilhada, medição, testes, assets embutidos no servidor e pacote de atualização. O launcher e o protocolo de mídia permanecem compatíveis.

Interface inspirada no Discord: lateral de participantes (#2b2d31), perfil inferior (#232428), palco (#1e1f22), superfícies (#313338), ação azul (#5865f2), texto (#f2f3f5). Fonte de sistema Segoe UI/Noto Sans; hierarquia compacta, palco amplo, controles inferiores, sem logos, animações ou fontes remotas. Sem servidores/canais fictícios.

Nome: pedir antes da primeira conexão, validar 1–32 caracteres sem controles, salvar localmente, reutilizar nas próximas entradas. Alterar nome pelo perfil; reconectar encerra transmissões com aviso explícito. Servidor já aceita nomes públicos.

Qualidade: preservar ideal e max na aplicação de constraints; público usa 1080p30/12 Mbps por espectador, conservando configurações escolhidas anteriormente. Privado conserva orçamento total. Sem mínimos rígidos de bitrate ou FPS que imponham filas. Prévia local opcional no público para evitar composição duplicada. Não forçar codec ou aceleração por hipótese.

Diagnóstico: FPS real de fonte, encoder e decoder, dimensões enviadas/recebidas, bitrate, tempo de codificação, motivo de limitação e RTT via getStats. Mostrar dados por conexão; relatório local copiável sem SDP, IPs ou nomes. Coleta de baixa frequência apenas com peers ativos. Não confundir limite configurado com FPS efetivo; tela estática pode gerar poucos quadros.

Verificação: testes de regressão dos alvos de captura e orçamento por espectador; validação de nome e estatísticas com contadores reais simulados; teste navegador de nome persistido, duas transmissões, settings e dimensões/FPS de conteúdo em movimento. Aqui não há navegador/Playwright e sockets são restritos: testes unitários não comprovam os 4 FPS reais corrigidos. Pedida informação do computador/conteúdo ao usuário; causa final depende da medição no destino.

Validação concluída: testes JS de captura, identidade (incluindo a entrada real do app com DOM/WebSocket simulados) e estatísticas; testes Rust de assets e nomes via WebSocket em memória; Clippy e release Linux. Teste Playwright ampliado para identidade persistida, vídeo/áudio nos dois sentidos, resolução/FPS medidos, prévia opcional e viewport móvel. Não foi possível executá-lo aqui: Playwright/navegador ausentes; instalação offline indisponível. Nenhum benchmark real de Windows ↔ Linux foi realizado neste ambiente.
