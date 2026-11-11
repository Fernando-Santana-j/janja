# Janja

Cliente nativo Rust para compartilhar tela e áudio em salas com senha opcional.
GTK3 + GStreamer + WebRTC, sem navegador.

- Estabilidade r7: [correções de vídeo, áudio, retransmissão e STUN/TURN; testes e limites](docs/estabilidade-r7.md).
- Interface r6: [alterações, desempenho e validação](docs/player-r6.md).
- Cliente: [compilar, empacotar e usar](docs/cliente-nativo.md).
- Servidor: [instalação Windows e Cloudflare](docs/janja-servidor.md).
- Compilar para desenvolvimento: `cargo run --features native --bin janja`.
- Gerar arquivo único: `python3 scripts/package-native.py`.
- Servidor independente: `cargo build --release --bin janja-servidor`.

Esta versão exige atualizar também o servidor para permitir salas sem senha (r5, protocolo 2). [Atualizar servidor, salas e tela cheia](docs/salas-e-tela-cheia.md).
O cliente de navegador e seus lançadores foram removidos. O app nativo não inicia navegador; WebRTC é uma biblioteca de transporte, não uma interface web.

O executável distribuído não contém logs automáticos nem comandos de diagnóstico. Ferramentas de desenvolvimento: `cargo run --features native,diagnostics --bin janja -- --self-test` e `--network-test`. O empacotador verifica uma compilação temporária e depois gera novamente o cliente sem diagnóstico.

Erro `not-negotiated` ou FPS baixo: [diagnóstico atual e coleta automática de logs](docs/diagnostico-atual.md).
