# Janja

Cliente nativo Rust para compartilhar tela e áudio em salas com senha opcional.
GTK3 + GStreamer + WebRTC, sem navegador.

- Revisão r11: [transmissão, áudio, baixa latência e melhorias de interface](docs/cliente-nativo.md).
- Revisão r9: [estabilidade, adaptação automática e medições](docs/auditoria-20261112.md).
- Senhas de sala lembradas entre execuções pelo [cofre do sistema](docs/cliente-nativo.md#senhas-lembradas-entre-execuções).
- Áudio r8: [captura Windows, verificação de sinal e baixa latência](docs/cliente-nativo.md#atualização-r8).
- Estabilidade r7: [correções de vídeo, áudio, retransmissão e STUN/TURN; testes e limites](docs/estabilidade-r7.md).
- Interface r6: [alterações, desempenho e validação](docs/player-r6.md).
- Cliente: [compilar, empacotar e usar](docs/cliente-nativo.md).
- Servidor: [instalação Windows e Cloudflare](docs/janja-servidor.md).
- Compilar para desenvolvimento: `cargo run --features native --bin janja`.
- Gerar arquivo único: `python3 scripts/package-native.py`.
- Windows: a compilação também gera `dist/janja-setup-windows.exe`, com bibliotecas, atalhos e desinstalador. [Compilar e distribuir](docs/cliente-nativo.md#gerar-windows).
- Servidor independente: `cargo build --release --bin janja-servidor`.

Esta versão exige atualizar também o servidor para permitir salas sem senha (r5, protocolo 2). [Atualizar servidor, salas e tela cheia](docs/salas-e-tela-cheia.md).
O cliente de navegador e seus lançadores foram removidos. O app nativo não inicia navegador; WebRTC é uma biblioteca de transporte, não uma interface web.

O cliente comum não contém logs automáticos nem comandos de diagnóstico. `python3 scripts/package-native.py --diagnostics` gera também um executável de diagnóstico separado, com logs e autotestes. Em desenvolvimento: `cargo run --features native,diagnostics --bin janja -- --self-test` e `--network-test`.

Erro `not-negotiated` ou FPS baixo: [diagnóstico atual e coleta automática de logs](docs/diagnostico-atual.md).
# Instalação e atualizações

Os instaladores Windows e Linux podem consultar Releases assinadas no GitHub ao
abrir o Janja. Os portáteis continuam disponíveis. Consulte
[atualizações e publicação](docs/atualizacoes.md) para configurar a chave, executar
os testes e publicar uma versão. O atualizador não fica residente na transmissão.
