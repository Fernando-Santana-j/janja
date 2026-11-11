# Executáveis e servidor Windows por SSH

## Arquivos necessários

- Amigos no Windows: **somente `sala.exe`**.
- Amigos no Linux: **somente `sala`** (`chmod +x sala`).
- Ambos precisam de Chrome/Edge/Chromium instalado; a interface atual usa esse
  navegador em modo app. Não é um pacote com navegador embutido.
- Servidor: o mesmo `sala.exe` + `servidor.json` criado na instalação. `cloudflared`
  é um processo separado para publicar o serviço. Não precisa de navegador nem
  Node no servidor. Rust só é necessário onde você compilar.

Na primeira abertura, o app pede a URL HTTPS no terminal e a salva em
`%APPDATA%\sala\room.txt` ou `~/.config/sala/room.txt`. Não é necessário enviar esse
arquivo aos amigos. Depois, o app abre a URL salva. Nome/chave são informados na
interface; marque Lembrar acesso. Para mudar a URL: `sala configure https://...`.

O executável Linux está em `dist/sala-linux-x64`. O pacote
`dist/sala-fontes.zip` contém os arquivos para compilar Windows. O `.exe` **não foi
gerado neste ambiente**, que só tem toolchain Linux e não permite downloads.

## 1. Enviar os fontes ao Windows (no seu Linux)

Substitua `USUARIO` e `SERVIDOR` pelo acesso SSH que você já usa:

```sh
cd /home/fernando/Downloads/janja
scp dist/sala-fontes.zip USUARIO@SERVIDOR:sala-fontes.zip
ssh USUARIO@SERVIDOR
```

No terminal SSH, entre no PowerShell caso esteja no `cmd`:

```powershell
powershell -NoProfile
```

Use uma conta SSH com permissão de administrador para instalar ferramentas e
registrar as tarefas. Os comandos seguintes são **no Windows remoto**.

## 2. Compilar o `.exe` uma vez

Se `cargo --version` já funcionar e houver ferramentas C++, pule as instalações:

```powershell
winget install --id Microsoft.VisualStudio.2022.BuildTools --exact --accept-source-agreements --accept-package-agreements --override "--quiet --wait --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"
winget install --id Rustlang.Rustup --exact --accept-source-agreements --accept-package-agreements
```

Se o instalador pedir reinicialização, faça-a e reconecte. Instalar compiladores
pode demorar. Em Windows Server, `winget` pode não estar disponível; use os
instaladores oficiais de Rust e Visual Studio Build Tools. Não tente executar o
binário Linux no Windows. [Requisitos oficiais do Rust](https://rust-lang.org/tools/install/).

Depois:

```powershell
$env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"
New-Item -ItemType Directory -Force C:\Sala | Out-Null
Expand-Archive -LiteralPath "$env:USERPROFILE\sala-fontes.zip" -DestinationPath C:\Sala -Force
Set-Location C:\Sala
cargo build --release --locked
if ($LASTEXITCODE -ne 0) { throw 'Compilacao falhou; nao continue.' }
Copy-Item .\target\release\sala.exe .\sala.exe
```

`C:\Sala\sala.exe` é o arquivo que você distribui aos amigos Windows e executa
no servidor. Depois de compilar, o servidor não precisa dos fontes, `web`, Cargo
ou Node. HTML/CSS/JS já ficam embutidos no binário.

## 3. Iniciar o servidor e manter após fechar SSH

```powershell
Set-Location C:\Sala
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\install-server.ps1 -Origin https://sala.seudominio.com
```

Esse comando cria `C:\ProgramData\Sala\servidor.json` com chave aleatória e STUN
gratuito, registra a tarefa `Sala-Servidor`, inicia o exe e confere a resposta HTTP
local. Ao executar novamente, preserva a chave e atualiza a origem. Tarefa existente
com esse nome é substituída; outros programas não são alterados.

Verificar e ler a chave para compartilhar **em privado**:

```powershell
Get-ScheduledTask -TaskName Sala-Servidor
Get-ScheduledTaskInfo -TaskName Sala-Servidor
(Invoke-WebRequest -UseBasicParsing http://127.0.0.1:8787/).StatusCode
(Get-Content C:\ProgramData\Sala\servidor.json -Raw | ConvertFrom-Json).key
```

O retorno HTTP local 200 não prova que o DNS/túnel ou a mídia funcionam
remotamente; configure o túnel no próximo passo.

Para parar/reiniciar:

```powershell
Stop-ScheduledTask -TaskName Sala-Servidor
Start-ScheduledTask -TaskName Sala-Servidor
```

Rodar em primeiro plano para diagnosticar (pare a tarefa antes):

```powershell
C:\Sala\sala.exe serve --config C:\ProgramData\Sala\servidor.json
```

## 4. Usar seu túnel existente

Se já houver uma rota **livre** `sala.seudominio.com` para `http://127.0.0.1:8787`
no seu Cloudflare Tunnel, pule para o passo 5. O túnel deve rodar nesse Windows.
Adicionar apenas DNS não configura o destino HTTP: a regra de ingress também
precisa existir. Não substitua as regras dos seus outros aplicativos.

Se você não tiver essa rota e quiser configurar tudo por CLI sem mexer no túnel
existente, crie um **túnel separado**. É necessário um domínio seu na Cloudflare
(já pago, se você tiver comprado um). Em PowerShell:

```powershell
winget install --id Cloudflare.cloudflared --exact --accept-source-agreements --accept-package-agreements
```

Reconecte o SSH se `cloudflared --version` não for encontrado. Autorize uma vez:

```powershell
cloudflared tunnel login
```

O comando mostra uma URL. Abra-a no navegador do seu computador, entre na sua
conta e autorize o domínio. Não envie certificados ou tokens no chat.

Crie nomes novos; se `sala-amigos` ou o subdomínio já existirem, escolha outros:

```powershell
$hostnameSala = 'sala.seudominio.com'
cloudflared tunnel create sala-amigos
if ($LASTEXITCODE -ne 0) { throw 'Falha ao criar o tunel.' }
cloudflared tunnel route dns sala-amigos $hostnameSala
if ($LASTEXITCODE -ne 0) { throw 'Falha ao criar DNS; confira o dominio.' }
$tunnels = cloudflared tunnel list --output json | ConvertFrom-Json
$tunnelId = ($tunnels | Where-Object { $_.name -eq 'sala-amigos' }).id
if (!$tunnelId) { throw 'Tunel nao encontrado.' }
```

Com a pasta protegida criada no passo 3, configure o processo desse túnel
separado (ainda na mesma sessão PowerShell):

```powershell
Copy-Item "$env:USERPROFILE\.cloudflared\$tunnelId.json" C:\ProgramData\Sala\tunnel.json
@"
tunnel: $tunnelId
credentials-file: C:/ProgramData/Sala/tunnel.json
ingress:
  - hostname: $hostnameSala
    service: http://127.0.0.1:8787
  - service: http_status:404
"@ | Set-Content C:\ProgramData\Sala\tunnel.yml -Encoding ASCII

$cloudflaredPath = (Get-Command cloudflared).Source
$action = New-ScheduledTaskAction -Execute $cloudflaredPath -Argument 'tunnel --config C:\ProgramData\Sala\tunnel.yml run'
$trigger = New-ScheduledTaskTrigger -AtStartup
$account = New-ScheduledTaskPrincipal -UserId 'S-1-5-19' -LogonType ServiceAccount
$settings = New-ScheduledTaskSettingsSet -ExecutionTimeLimit ([TimeSpan]::Zero) -RestartCount 3 -RestartInterval (New-TimeSpan -Minutes 1) -StartWhenAvailable -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries
Register-ScheduledTask -TaskName Sala-Tunnel -Action $action -Trigger $trigger -Principal $account -Settings $settings
Start-ScheduledTask -TaskName Sala-Tunnel
```

Isso cria apenas a tarefa `Sala-Tunnel`; não reinstala o serviço global do
cloudflared nem interfere nos outros túneis. O `cloudflared.exe` precisa estar em
uma pasta que LOCAL SERVICE possa ler, como Program Files. Se veio de um diretório
pessoal restrito, copie o executável para `C:\Sala\cloudflared.exe` antes e use esse
caminho na ação da tarefa. A pasta ProgramData\Sala recebe permissão de LOCAL SERVICE
pelo instalador do passo 3. [CLI oficial](https://developers.cloudflare.com/cloudflare-one/networks/connectors/cloudflare-tunnel/do-more-with-tunnels/local-management/create-local-tunnel/).

Abra `https://sala.seudominio.com` em outro computador e teste transmitir com um
amigo em outra rede. Neste perfil sem TURN, deixe Usar somente TURN desmarcado.

## 5. Buscar o `.exe` e distribuir

No seu Linux, em outro terminal:

```sh
scp USUARIO@SERVIDOR:C:/Sala/sala.exe ./dist/sala.exe
```

Envie apenas `sala.exe` para Windows e `dist/sala-linux-x64` para Linux. Os amigos
não precisam instalar Rust, Node, cloudflared ou Tailscale. Precisam do navegador
compatível instalado e recebem de você a URL HTTPS e a chave.

O servidor e o túnel rodam pelo Agendador mesmo após desconectar o SSH e reiniciam
com o Windows. O computador precisa estar ligado e não suspenso.

## Gratuidade e validação

Este caminho usa Tunnel + STUN gratuito, **sem habilitar TURN cobrável**. Não há
custo de relay configurado, mas STUN não garante conexão em todo NAT/firewall.
O Cloudflare Tunnel não substitui TURN na mídia WebRTC. Se houver bloqueio,
será necessário avaliar outro caminho de rede; o TURN Cloudflare tem franquia,
não gratuidade ilimitada. Veja [custos e limites](cloudflare.md).

Compilação Windows, instalação de tarefas e conexão pública não foram executadas
neste ambiente. Não tenho acesso à sua sessão SSH; os comandos são para você
executar no servidor. A compilação Linux e os testes locais ficam verificados.
