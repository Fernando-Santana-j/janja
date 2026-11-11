# Correção da transmissão — cliente leve

Escopo: substituir o caminho de captura usado pelo cliente recomendado; preservar o servidor de sinalização e o cliente experimental como opção separada.

- Reusar WebRTC e interface web existentes, com sala pública automática e assets sob /janja/.
- Reusar o launcher Rust com endereço fixo, Edge/Chrome/Chromium instalado, sem Electron ou MSYS2.
- Permitir janela/tela no padrão 720p30; manter limites de FPS/upload, captura de aba ou dispositivo para áudio separado.
- Informar falhas ICE e ausência de frames; não prometer conectividade em NAT restritivo sem TURN.
- Atualizar testes, build Windows, CI e roteiro de implantação. Todos os participantes migram juntos.

Validação local: 10 testes Rust e 13 JavaScript passaram, Clippy e compilação release Linux passaram. Um teste Rust com sockets foi excluído porque sockets são bloqueados neste ambiente. Teste Playwright público foi adicionado, mas não executado: dependência ausente e sockets bloqueados. Windows, captura real, áudio do sistema, NAT entre provedores e desempenho precisam de validação nos PCs de destino.
