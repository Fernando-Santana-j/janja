# Salas e tela cheia — r5

O cliente abre a lista de salas. Criar exige nome (até 32 caracteres). A senha é opcional; quando preenchida, exige 8–128 bytes. Salas abertas entram diretamente, sem diálogo de senha. Nas salas protegidas, o servidor verifica a senha antes de liberar participantes e sinalização. Cada sala aceita oito pessoas; o servidor admite até 32 conexões e 32 salas. Salas vazias são removidas, e a lista é reiniciada com o servidor.

Senhas usam PBKDF2-HMAC-SHA256 com salt aleatório e 100.000 iterações em memória. A listagem não contém hash nem senha. Tentativas de entrada/criação são limitadas por conexão. O isolamento de subscribe/signal é verificado no servidor. Mídia continua P2P; conexões já autorizadas não podem ser revogadas à força em um cliente adulterado.

Apenas o botão do vídeo amplia a transmissão. Escape, o botão “Sair da tela cheia (Esc)” e duplo clique restauram o mesmo widget, sem abrir outro decoder. A saída não altera o estado de ToggleButton: no GTK3, set_active também emite clicked, que fazia a rotina reentrar em tela cheia. Referência: https://docs.gtk.org/gtk3/class.ToggleButton.html.

O PNG fornecido foi incluído como ícone da janela, ICO multirresolução nos executáveis Windows via windres e ícone do launcher Linux. No Linux, o executável portátil registra uma entrada de menu em ~/.local/share/applications (ou XDG_DATA_HOME), necessária para associação em Wayland. Um ELF solto pode continuar com ícone genérico no gerenciador de arquivos; use a entrada Janja do menu. Se mover o arquivo, execute-o novamente para atualizar o atalho.

É necessário atualizar o servidor e os clientes para r5 (protocolo 2). O health inclui `passwordOptional: true` e o build instalado. Feche a versão antiga antes de abrir a nova. A instalação Windows continua com Cloudflare/porta 8787 existentes:

```bash
cd /home/fernando/Downloads/janja
scp -i /home/fernando/.ssh/windows_caixa dist/janja-nativo-fontes.zip scripts/build-remote-windows.ps1 caixa@192.168.1.11:
ssh -i /home/fernando/.ssh/windows_caixa caixa@192.168.1.11 'powershell.exe -NoProfile -ExecutionPolicy Bypass -File C:/Users/Caixa/build-remote-windows.ps1 -UpdateServer'
scp -i /home/fernando/.ssh/windows_caixa caixa@192.168.1.11:Downloads/janja-windows.exe ./dist/janja-windows.exe
```

O comando remoto precisa das permissões de administrador já usadas na instalação do servidor. O novo executável fica em C:\Users\Caixa\Downloads\janja-windows.exe.

Validação disponível: testes unitários de senha/isolamento/capacidade e fluxo WebSocket em memória (criar, senha errada, entrar, sair); testes de mídia sintética, testes de integração do ícone Linux. A sessão gráfica e o ambiente Windows não estão acessíveis neste sandbox; o teste GTK fica ignorado aqui por exigir desktop. Não apresentar o ZIP de fontes como um .exe já compilado.

Resultado desta revisão: 38 testes de biblioteca passaram; um teste adicional do atalho/ícone Linux passou; Clippy sem warnings. O executável portátil validou 1080p a 29,85 FPS, 180 quadros e 602 buffers de áudio. As dependências pré-compiladas foram reutilizadas porque o cache de fontes Cargo está incompleto. O teste gráfico foi acrescentado ao CI Linux com Xvfb, mas não foi executado neste ambiente. AGENTS.md já existia e foi preservado.
