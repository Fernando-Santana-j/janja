# Instalação e atualizações — r12

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
  compatível com a glibc de destino; o CI usa Ubuntu 24.04.

Quem usa uma versão anterior precisa instalar a r12 uma vez. Copiar apenas o
portátil não instala o atualizador. Os portáteis continuam disponíveis para quem
prefere controlar manualmente as atualizações.

## Publicar com um comando

A publicação está autorizada pelo responsável. No terminal Linux do projeto:

```bash
bash scripts/publish-release.sh
```

O comando autentica no GitHub quando necessário, verifica o repositório público,
valida a chave original e executa os testes Windows pelo SSH configurado. Depois
configura o secret, prepara o commit dos arquivos do projeto, envia `main` e a tag
da versão e acompanha o workflow. Uma falha anterior ao envio interrompe a
publicação. O código é recompilado e testado pelo GitHub antes da Release.

Ao terminar, o comando baixa os instaladores publicados e confere assinatura,
versão, tamanho e SHA-256. O endereço e a lista de arquivos ficam em
`dist/publicacao.json`; o andamento fica em `dist/publicacao.log`. Não é necessário
comprar certificado para esse fluxo. Alertas de reputação do Windows ainda podem
ocorrer.

Para a próxima versão, altere `BUILD` em `src/lib.rs` para uma revisão maior e
execute o mesmo comando. Ele não substitui tags já existentes. Se uma execução do
GitHub falhar depois de enviar a tag, corrija a falha antes de decidir entre
reexecutar o workflow ou criar uma revisão nova; não mova uma tag publicada.

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

## Primeira publicação da r12

Estes comandos enviam código e publicam a versão para os usuários. Execute-os
depois de revisar os arquivos e concluir a validação Windows. A branch deste
projeto é `main`.

```bash
git add Cargo.toml Cargo.lock build.rs src scripts assets .github README.md .gitignore docs/atualizacoes.md docs/plano-r12.md docs/medicoes-20261113.json
git diff --cached --stat
git commit -m "Prepara Janja r12 com instaladores e atualizacoes assinadas"
git push origin main
git tag 2026.11.13-r12-leve
git push origin 2026.11.13-r12-leve
gh run list --workflow release.yml --limit 5
gh run watch
```

Selecione a execução da tag r12 ao acompanhar o workflow. Somente após ele
terminar com sucesso, confira a Release:

```bash
gh release view 2026.11.13-r12-leve --repo Fernando-Santana-j/janja
```

Ela deve conter `janja-setup-windows.exe`, `janja-setup-linux.sh`, os dois
portáteis, `janja-update.json` e `janja-update.json.sig`. A instalação inicial é
uma vez por computador; os atalhos instalados verificam as próximas versões.

## Publicar as próximas atualizações

1. Faça a alteração e incremente `src/lib.rs`, por exemplo para
   `2026.11.14-r13-leve`. O número depois de `r` deve sempre aumentar.
2. Revise, teste, faça commit e envie para `main`. Preserve a chave original.
3. Crie e envie a tag correspondente ao mesmo commit:

```bash
git push origin main
git tag 2026.11.14-r13-leve
git push origin 2026.11.14-r13-leve
gh run list --workflow release.yml --limit 5
gh run watch
```

O workflow `Publicar versao assinada` compila Windows/Linux, executa os testes,
gera os instaladores e assina os metadados antes de criar a Release. Ele verifica
se os artefatos pertencem à versão esperada e se a tag corresponde ao commit
compilado. Não publica executáveis de diagnóstico nem a chave privada.

Fazer upload do código ou criar uma tag sem os artefatos não atualiza os usuários.
Para reverter um problema já publicado, publique o código corrigido com uma
revisão maior; não tente reutilizar uma revisão antiga.

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
