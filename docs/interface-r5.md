# Interface e salas — r5

- A lista de participantes usa ListBox com alinhamento no início. As linhas mantêm altura natural; avatar e botão são centralizados verticalmente. O FlowBox anterior esticava uma linha por toda a altura do viewport.
- Cabeçalho com nome da sala, perfil, estado e saída, sem marca. Controles inferiores compactos; mensagens vazias não reservam uma linha. Texto, foco e seleção possuem cores explícitas.
- Apenas o volume de cada transmissão permanece. Volume geral e mute geral foram removidos também da engine; 0% silencia aquele vídeo. Tela cheia conserva os mesmos controles e decoder.
- Senha vazia ou ausente cria sala aberta. Senha preenchida exige 8–128 bytes e mantém PBKDF2/salt e verificação no servidor. A lista informa locked corretamente; entrar em sala aberta não pede senha.
- Seletor por cartões e páginas, sem ComboBox de fontes. No Windows, as miniaturas de janelas são renderizadas pelo DWM e liberadas ao trocar página ou fechar. A tela tem um retrato estático ao abrir o seletor. Nenhuma prévia de captura permanece durante a transmissão.

## Limite do seletor Windows

Este é um seletor do aplicativo usando miniaturas nativas DWM, **não o GraphicsCapturePicker oficial da Microsoft**. Este retorna GraphicsCaptureItem; d3d11screencapturesrc aceita HWND/HMONITOR. Integrá-lo corretamente exige adaptar a captura para esse objeto. Não se faz associação aproximada por título, nem troca silenciosa para outra janela. Wayland continua usando o portal do compositor. No X11, os cartões de janelas apresentam o título, sem miniatura.

## Verificação

Os testes de servidor cobrem senha opcional, ausência do campo no protocolo, proteção de salas privadas, capacidade e isolamento. Os testes da engine verificam validação de senha e volumes independentes. O teste GTK ignorado `tiles_keep_controls_and_restore_fullscreen_on_close` também verifica altura de participantes/controles em 960×540 e 1920×1080, paginação/seleção/cancelamento e retorno de tela cheia. Execute via Xvfb no Linux ou em desktop Windows.

O empacotador valida o runtime e codificação/decodificação de vídeo e áudio sintéticos antes de substituir o Linux portátil. A validação visual e as miniaturas Windows exigem desktop; não foram executadas neste ambiente sem acesso a sockets/display. O pacote de fontes permite o build Windows por `scripts/build-remote-windows.ps1 -UpdateServer`.

Validação local r5: 40 testes passaram; Clippy sem avisos. Autoteste do portátil: 1920×1080, 29,86 FPS recebidos, 180 quadros e 606 buffers de áudio. O teste WebRTC UDP e o teste GTK foram ignorados; o teste TCP real foi excluído por restrição de sockets. Essa medição é sintética, não um teste entre Windows e Linux pela internet.

Referências do seletor: [GraphicsCapturePicker](https://learn.microsoft.com/en-us/windows/apps/develop/media-authoring-processing/screen-capture), [d3d11screencapturesrc](https://gstreamer.freedesktop.org/documentation/d3d11/d3d11screencapturesrc.html) e [miniaturas DWM](https://learn.microsoft.com/en-us/windows/win32/api/dwmapi/nf-dwmapi-dwmregisterthumbnail).
