# Instalação e atualizações

O cliente portátil continua independente. Os instaladores acrescentam um lançador
que procura uma atualização no GitHub **antes** de abrir o aplicativo. Não há
timer de atualização, coletor de desempenho ou processo auxiliar residente
durante a transmissão. No Windows, se já houver Janja aberto, a consulta é pulada.

Os downloads são públicos em `Fernando-Santana-j/janja`. O cliente não recebe token
do GitHub. Cada Release contém instaladores, portáteis, `janja-update.json` e sua
assinatura RSA-3072/SHA-256. O lançador verifica assinatura, repositório, revisão,
tamanho e SHA-256 antes de executar o instalador. Revisões antigas são recusadas.

A consulta de metadados tem limite de tempo. Sem rede, sem Release, com assinatura
inválida ou após cancelar o download, a versão instalada é aberta. O download
tem progresso e pode ser cancelado. A instalação preserva perfis e senhas de
salas. Os arquivos da instalação anterior são copiados antes de aplicar a nova
versão e restaurados se a instalação falhar. Isso não substitui testes reais da
nova versão: problemas de captura ou driver não são detectados por `--version`.

## Instalar

- Windows: executar `janja-setup-windows.exe`. Os atalhos usam o lançador de
  atualização; o `janja.exe` da pasta instalada continua executável diretamente.
- Linux x86_64: executar `sh janja-setup-linux.sh`. A instalação fica em
  `~/.local/share/janja`, com entrada no menu. Python 3, OpenSSL e Zenity são
  instalados pelo gerenciador da distribuição quando faltarem. Nenhum deles
  permanece rodando com o cliente. Atualizações posteriores ocorrem na pasta do
  usuário, sem administrador. O binário deve ser compilado numa distribuição
  compatível com a glibc de destino; o CI usa Ubuntu 24.04. **Os pacotes locais r12/r13 foram gerados no Arch Linux e incluem
  bibliotecas que exigem glibc 2.44**;
  ele não deve ser apresentado como compatível com Ubuntu 24.04.

Quem usa o portátil r11 ou anterior precisa instalar a versão nova uma vez. Copiar apenas o
portátil não instala o atualizador. Os portáteis continuam disponíveis para quem
prefere controlar manualmente as atualizações.

## Publicar com um comando, sem Actions

O Actions desta conta bloqueou a compilação por uma questão de cobrança antes
mesmo de iniciar os jobs. Use o fluxo local, que não executa compilação no GitHub:

```bash
bash scripts/publish-release.sh --local
```

O comando autentica no GitHub, executa os testes Linux e WebRTC, gera o pacote
Linux e usa o SSH já configurado para compilar e validar o Windows. A validação
Windows inclui mídia, carga sintética, atualização, rollback, instalador real e
Defender. Só depois prepara o commit, envia `main` e a tag da versão.

A publicação verifica os hashes dos quatro pacotes e exige que os executáveis
Windows correspondam exatamente à análise aprovada do Defender. Assina o
manifesto com a chave original, envia os seis arquivos para um rascunho, baixa e
confere os arquivos enviados e só então torna a Release pública. Não envia os
executáveis de diagnóstico nem a chave privada.

Se ocorrer uma falha depois de enviar a tag, execute o mesmo comando novamente:
ele pode retomar o rascunho usando os pacotes validados. Uma Release pública não
é sobrescrita. O andamento fica em `dist/publicacao.log`; o endereço final fica
em `dist/publicacao.json`. O workflow disparado pela tag pode aparecer como
bloqueado no Actions; ele não é utilizado pelo fluxo local.

Para a próxima atualização, incremente o número `r` em `src/lib.rs`, por exemplo
para `2026.11.17-r14-estavel`, e execute o mesmo comando. Preserve a chave de assinatura
e nunca mova uma tag publicada. Fazer upload apenas do código não atualiza os
usuários: a atualização depende dos instaladores e do manifesto na Release.

Se o Actions voltar a estar disponível, `bash scripts/publish-release.sh` usa a
compilação e os testes do GitHub em Ubuntu 24.04 e Windows. O fluxo local mantém
a compatibilidade da distribuição Linux em que os pacotes foram compilados;
não cria automaticamente um pacote compatível com glibc mais antiga.

## Mudanças desde a r11 de 12/11

- Tela cheia mostra a imagem inteira por padrão, sem cortar as bordas.
- Opção de preenchimento deixa claro quando haverá corte.
- Filtros de renderização mais leves, com medições antes e depois.
- A r13 reduz o custo de ampliação bilinear, incluindo escala de tela alta,
  preservando as bordas e as faixas pretas.
- Menos instrumentação e cálculo de medição no cliente comum; diagnóstico separado.
- Correção das opções do encoder AMD AMF para evitar rejeição e fallback indevido.
- Instaladores Windows/Linux com atualização assinada ao abrir pelos atalhos.
- Recuperação da instalação anterior quando uma atualização falha.
- Testes de mídia, áudio, perdas, carga sintética, instalação e segurança ampliados.
- Publicação pelo GitHub com alternativa local sem Actions.

## r14: recuperação de conexão e mídia

A versão `2026.11.17-r14-estavel` acrescenta recuperação automática quando a
decodificação deixa de avançar, mesmo que só o áudio pare. A tentativa preserva
o volume e as outras transmissões. Silêncio sem pacotes e tela estática não
disparam reinícios; erros de relógio da pipeline também recebem tratamento.

A queda da sinalização mantém a captura e as conexões P2P já estabelecidas.
O servidor atualizado permite retomar a mesma identidade e sala, com reserva de
120 segundos depois que detecta uma desconexão. Um reinício do servidor carrega
o cache e dá 120 segundos para retomada a partir da sua volta; o período em que
ficou desligado não encerra por si só as sessões que estavam ativas no snapshot.
Sair explicitamente de uma sala continua encerrando a participação.

O cache padrão é `state/session-cache.json`, relativo à pasta de trabalho do
servidor; `JANJA_SESSION_FILE` permite mudar o caminho. No serviço Windows fica
em `C:\ProgramData\Janja\state\session-cache.json`. Ele guarda somente hashes dos
segredos de retomada e das senhas das salas, além dos dados de presença. Não contém
mídia, nem senhas em texto. Preserve essa pasta; não envie o cache para o GitHub.
Uma falha de gravação aparece no health check e no log do servidor.

**Clientes e servidor precisam ser atualizados** para retomar as salas. O cache
não reconstrói sessões que a versão anterior nunca salvou. Na troca inicial da
r13 para r14, faça a atualização do servidor fora de uma transmissão importante.
As atualizações dos instaladores continuam sendo apenas dos clientes.

Para validar no Windows sem publicar:

```bash
cd /home/fernando/Downloads/janja
bash scripts/test-windows-from-linux.sh
```

Esse teste foi concluído pelo proprietário em 17/11: executáveis e instalador
Windows r14 conferidos, sem falhas nos testes. A pasta remota ainda se chama
`janja-nativo-r12` para reutilizar o cache de compilação. A versão vem de `BUILD`
em `src/lib.rs`, é incorporada ao instalador e conferida em `windows.build.json`;
o nome da pasta não seleciona código antigo. Não é necessário renomeá-la.

Depois que esse comando concluir sem erros, atualize o servidor já configurado
no Windows. O comando preserva a configuração e prepara a permissão privada do
cache para a conta do serviço. Exige o mesmo SSH administrativo da instalação:

```bash
ssh -F /dev/null -i /home/fernando/.ssh/windows_caixa caixa@192.168.1.11 'powershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass -File C:/Users/Caixa/Downloads/janja-nativo-r12/scripts/install-janja-server.ps1'
```

Para gerar, validar e publicar os instaladores dos clientes, use
`bash scripts/publish-release.sh --local`. Esse fluxo já executa a validação
Windows completa, sem precisar rodar o primeiro comando separadamente. O
comando SSH de instalação do servidor continua necessário nesta atualização.

Sequência para colocar a r14 em uso, no terminal Linux do proprietário:

1. Fechar as transmissões e executar o comando SSH de instalação do servidor
   acima. Ele preserva a configuração existente, reinicia o serviço e verifica
   a retomada de sessões e a disponibilidade do cache privado. Se a conta SSH
   não estiver elevada, executar esse mesmo script em PowerShell administrador
   no Windows, sem mudar permissões do antivírus:

   ```powershell
   powershell.exe -NoProfile -ExecutionPolicy Bypass -File C:\Users\Caixa\Downloads\janja-nativo-r12\scripts\install-janja-server.ps1
   ```

2. Conferir o servidor através do endereço público:

   ```bash
   curl --fail --silent --show-error --max-time 20 https://infocoretech.com.br/janja/health |
     python3 -c 'import json,sys; h=json.load(sys.stdin); print(json.dumps(h,indent=2)); assert h.get("build")=="2026.11.17-r14-estavel" and h.get("sessionRecovery") is True and h.get("sessionCacheHealthy") is True, "Servidor incorreto ou cache indisponivel"'
   ```

3. Publicar os instaladores com `bash scripts/publish-release.sh --local`.
   O fluxo recompila os pacotes e repete a validação Windows como parte da
   publicação. Não é necessário fazer commit, tag ou uploads manualmente.
   Só continuar quando terminar com `Publicacao local concluida`.

4. Conferir a Release publicada:

   ```bash
   gh release view 2026.11.17-r14-estavel --repo Fernando-Santana-j/janja --json tagName,isDraft,isPrerelease,url,assets
   python3 scripts/verify-release.py --directory dist/publicado-2026.11.17-r14-estavel
   ```

5. Atualizar o cliente do Linux, sem `sudo` para o próprio Janja:

   ```bash
   sh dist/janja-setup-linux.sh
   "$HOME/.local/share/janja/janja-linux" --version
   "$HOME/.local/share/janja/launch"
   ```

6. No Windows, fechar o Janja e abrir o atalho instalado para receber a
   atualização. Quem ainda usa somente o portátil deve executar o instalador
   uma vez. No PC usado para compilar, o instalador já está em Downloads;
   para instalar silenciosamente e conferir a versão no PowerShell da conta
   que usa o aplicativo:

   ```powershell
   $setup = Join-Path $env:USERPROFILE 'Downloads\janja-setup-windows.exe'
   $p = Start-Process -FilePath $setup -ArgumentList '/S' -Wait -PassThru
   if ($p.ExitCode -ne 0) { throw 'Instalacao do Janja falhou.' }
   $dir = (Get-ItemProperty 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Janja').InstallLocation
   $v = (Get-Content -LiteralPath (Join-Path $dir 'version.txt') -Raw).Trim()
   if ($v -ne '2026.11.17-r14-estavel') { throw "Versao instalada incorreta: $v" }
   Write-Host "Janja instalado: $v"
   Start-Process -FilePath (Join-Path ([Environment]::GetFolderPath('Desktop')) 'Janja.lnk')
   ```

7. Distribuir o endereço da Release aos amigos, para baixarem o instalador do
   sistema deles. Não precisam de MSYS2, Rust, GTK ou GStreamer no Windows.
   Abrir pelo atalho instalado ativa a consulta de atualizações; o portátil
   isolado continua com atualização manual. No Linux, permanece a restrição
   de glibc da máquina onde o pacote foi compilado.

Nenhum desses comandos exige apagar perfis, senhas salvas, a chave original de
atualização ou o cache do servidor. Não seguir para a próxima etapa se houver erro.

Validação real recomendada, com dois clientes r14 e o servidor r14: assistir a
uma fonte com movimento e som contínuo; interromper somente a sinalização,
mantendo a rede P2P; confirmar que imagem e áudio continuam; reiniciar o servidor
e confirmar o retorno dos mesmos participantes e sala, sem nova senha. Repetir
cancelando uma visualização durante a queda e verificar que ela não reaparece.
Os testes automatizados locais e os limites estão em
[`plano-estabilidade-20261117.md`](plano-estabilidade-20261117.md).

## Chave de publicação

A chave privada inicial foi criada em `.release-keys/update-private.pem`, com
permissão restrita. Ela está ignorada pelo Git e excluída do ZIP de fontes e dos
instaladores. **Guarde um backup seguro. Não publique esse arquivo.** Somente as
chaves públicas em `assets/update-public.pem` e `assets/update-public.xml` devem
ser versionadas.

Configure uma vez o secret do repositório, no seu terminal autenticado no GitHub:

```bash
gh auth status
gh repo view Fernando-Santana-j/janja --json visibility --jq .visibility
gh secret set JANJA_UPDATE_PRIVATE_KEY --repo Fernando-Santana-j/janja < .release-keys/update-private.pem
```

Se precisar entrar na conta, use `gh auth login`. O resultado da visibilidade deve
ser `PUBLIC`: os aplicativos baixam sem token. Um repositório privado não permite
esse acesso anônimo; não coloque tokens pessoais dentro do aplicativo.

Não execute novamente `create-update-key.py` para cada versão: os aplicativos
instalados confiam na chave original. O script recusa sobrescrevê-la.

## Validar antes da primeira publicação

```bash
python3 scripts/test-updates.py
cargo test --locked --features native,portable
python3 scripts/package-native.py --diagnostics
```

No Windows, os testes do atualizador usam arquivos temporários e não substituem
a instalação do usuário:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\test-windows-updater.ps1
```

O teste de instalar/reinstalar/desinstalar exige conta ou VM sem Janja instalado;
ele recusa mexer numa instalação existente. O CI executa ambos. Antes de liberar
a primeira atualização automática, teste também uma atualização real entre duas
Releases assinadas em uma conta isolada, incluindo cancelamento e falha de rede.

## Queda de FPS em jogo

O coletor é separado do cliente distribuído. No PowerShell do Windows:

```powershell
.\scripts\build-windows.ps1 -Diagnostics
.\scripts\diagnose-windows.ps1 -GameProcess 'jogo.exe' -Seconds 120
```

Ele registra CPU, memória, GPU e FPS codificados do Janja. Se um PresentMon oficial
já estiver disponível, acrescente `-PresentMonPath 'C:\ferramentas\PresentMon.exe'`
para registrar também a taxa de apresentações do jogo. Sem essa ferramenta,
`game_present_fps` fica nulo: o coletor não inventa FPS.

Compare a mesma cena e configurações: primeiro só o jogo, depois jogo + Janja
transmitindo, após a compilação terminar. GPU saturada, memória esgotada,
temperatura e outras aplicações também podem causar a queda. O relatório contém
somente os números selecionados; não compartilhe os logs brutos.

## Carga sintética sem jogo

A ponte de validação inclui um teste ignorado na suíte comum, executado somente
quando solicitado. No Windows ele compara uma transmissão sem carga com três
transmissões WebRTC 1080p30 sob carga de CPU (metade dos processadores lógicos,
no máximo seis trabalhadores, com pausas) e processamento D3D11 de imagem 4K60.
Ele mede FPS do vídeo e da carga gráfica, sinal de áudio decodificado e maiores
intervalos entre apresentações de vídeo e áudio. As tarefas terminam ao concluir
ou falhar; não são compiladas no cliente instalado.

```bash
cd /home/fernando/Downloads/janja
bash scripts/test-windows-from-linux.sh
```

Também é possível executar apenas o teste, em terminal UCRT64 com as dependências
já instaladas, sem reconstruir os instaladores:

```bash
cargo test --release --locked --features native,portable bounded_resource_pressure_keeps_webrtc_video_and_audio -- --ignored --nocapture --test-threads=1
```

É uma carga sintética limitada, não um jogo ou um teste térmico prolongado. Ela
não testa captura de um jogo real, WASAPI de um dispositivo físico, drivers de
outros computadores nem comunicação entre Windows e Linux. Uma aprovação não
comprova a causa da queda de FPS relatada.

## Distribuir gratuitamente entre amigos

As atualizações assinadas com a chave RSA deste projeto não têm custo de
certificado. Essa assinatura protege o download; ela não identifica o publicador
perante o Windows. Os executáveis atuais ainda não têm Authenticode.

"Aplicativo desconhecido" e "download incomum" podem ser avisos de reputação,
sem detecção de vírus. Sem certificado público, esses avisos podem continuar em
novas versões. Nem um certificado comprado garante que desapareçam imediatamente.
Consulte a [explicação oficial do SmartScreen](https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/smartscreen-reputation).

Use os instaladores das Releases do mesmo repositório por HTTPS. Não reempacote
os executáveis depois de publicar: tamanho e SHA-256 precisam corresponder ao
manifesto assinado. Quem recebe o instalador uma vez usa os atalhos para obter as
próximas versões; abrir o portátil diretamente não consulta atualizações.

Para verificar os arquivos finais no Windows, sem alterar exclusões nem desligar
a proteção, execute em PowerShell com permissão para usar o Defender:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\test-windows-security.ps1
```

O resultado fica em `dist/windows-security.json`, com hashes, estado das
assinaturas, versão do mecanismo e das definições do Defender. A ferramenta usa
o modo de análise sem remediação para inspecionar também arquivos compactados e
preservar os artefatos; esse modo não desliga o antivírus. Se houver erro ou se o
Defender não estiver disponível, o relatório não declara os arquivos aprovados.

Uma análise sem detecções vale para aqueles arquivos e definições, não para todo
antivírus ou navegador. Se aparecer uma ameaça com nome concreto, registre o
nome, o SHA-256 e a versão. Peça revisão gratuita no [portal de desenvolvedores da Microsoft](https://www.microsoft.com/en-us/wdsi/filesubmission/).
Para um bloqueio incorreto de página na Navegação Segura, use o [formulário oficial do Google](https://safebrowsing.google.com/safebrowsing/report_error/?hl=pt-BR).
Essas revisões não são um pedido de reputação automática para o SmartScreen.

Para repetir somente a validação de distribuição pelo SSH já configurado:

```bash
bash scripts/test-windows-from-linux.sh --distribution-only
```

Essa opção recompõe os instaladores, verifica atualização e rollback, testa a
aplicação do instalador real numa pasta isolada e analisa os arquivos no Defender.
Use a validação completa quando o código de mídia também mudar. Os testes ainda
não substituem uma atualização pública entre duas Releases em uma conta isolada.
