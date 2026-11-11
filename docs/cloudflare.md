# Cloudflare pelo terminal

## O que é grátis

O perfil deste projeto usa **Cloudflare Tunnel para a sala** e **STUN para tentar
vídeo/áudio diretamente entre os amigos**. Não ativa TURN, assinatura ou cobrança.
Não precisa de Tailscale nos computadores dos amigos.

Cloudflare Tunnel suporta WebSocket:
[documentação oficial](https://developers.cloudflare.com/cloudflare-one/faq/cloudflare-tunnels-faq/).
O STUN `stun.cloudflare.com:3478` é gratuito e ilimitado segundo a
[FAQ do Realtime](https://developers.cloudflare.com/realtime/turn/faq/).

**Limite importante:** o túnel HTTPS não carrega a mídia WebRTC deste aplicativo.
STUN não resolve todo CGNAT/firewall. Se a sala abrir mas o vídeo não conectar,
será necessário TURN ou uma configuração de rede diferente. Não há promessa de
conectividade universal usando apenas Tunnel + STUN.

Cloudflare Realtime TURN é outro produto. Em 03/11/2026, há **1.000 GB/mês grátis,
compartilhados entre TURN/SFU**, depois **US$ 0,05/GB**. Portanto, não é gratuito
ilimitado nem um bloqueio automático de gastos. Este script NÃO habilita esse
produto. [Preço oficial](https://developers.cloudflare.com/realtime/sfu/platform/pricing/).

## Windows: iniciar sem domínio ou conta (para testar)

No PowerShell do **PC que ficará ligado**, instale apenas o que faltar:

```powershell
winget install --id Cloudflare.cloudflared --exact
winget install --id OpenJS.NodeJS.LTS --exact
```

Abra outro PowerShell para atualizar o PATH. Entre na pasta do projeto. Use Node
22 ou superior. Tenha `target\release\sala.exe`; se ainda não tiver, compile com
Rust e Visual Studio Build Tools instalados:

```powershell
cargo build --release --locked
node scripts/cloudflare.mjs --check
node scripts/cloudflare.mjs
```

Não é necessário `npm install`. O script usa somente módulos padrão do Node.
No Linux, o mesmo comando funciona com `cloudflared`, Node e o binário Linux.

O comando:

1. Confere o executável e se a porta local 8787 pode ser usada.
2. Cria uma chave aleatória persistente em `.sala-cloudflare/room-key.txt`.
3. Inicia um Quick Tunnel isolado, sem alterar os túneis/configurações existentes.
4. Descobre a URL HTTPS e configura a origem exata aceita pelo servidor Rust.
5. Inicia a sala com STUN gratuito; ignora configurações TURN herdadas do ambiente.
6. Salva a URL em `.sala-cloudflare/room.txt` e encerra seus processos ao usar Ctrl+C.

Para ler a chave no seu terminal privado:

```powershell
Get-Content .sala-cloudflare\room-key.txt
```

Compartilhe URL e chave somente com os amigos. Eles podem abrir a URL diretamente
no Chrome/Edge/Chromium, ou copiar `room.txt` para junto do executável do app.
A chave não aparece nos logs do script. Não publique a pasta `.sala-cloudflare`.

**Quick Tunnel é para testes:** URL muda a cada inicialização e não tem garantia
de disponibilidade. Quem conhecer a URL pode abrir a página, mas precisa da chave
para entrar na sala. [Limitações oficiais](https://developers.cloudflare.com/tunnel/get-started/quick-tunnels/).

## Reaproveitar um túnel existente com endereço fixo

Use um **novo subdomínio livre**, por exemplo `sala.seudominio.com`. No túnel que
já roda no Windows, esse hostname precisa apontar para `http://127.0.0.1:8787`.
Com a rota já configurada, execute:

```powershell
node scripts/cloudflare.mjs --origin https://sala.seudominio.com
```

Esse modo inicia somente o app. Não altera DNS, não inicia outro túnel e não
sobrescreve a configuração dos outros aplicativos. Não depende de `cloudflared`
no PATH do terminal se o túnel já estiver rodando como serviço nesse PC.

Se o túnel for gerenciado pelo painel, a rota pode ser adicionada pelo painel/API
Cloudflare. Para fazê-lo pela API, primeiro são necessários o hostname desejado,
a identificação do túnel e autenticação da conta; não coloque tokens no chat.
Não substitua todas as regras de ingress para adicionar uma única aplicação.

Se preferir criar **um túnel separado gerenciado localmente**, estes são os comandos
oficiais (exigem sua conta e um domínio que você já controla):

```powershell
cloudflared tunnel login
cloudflared tunnel create sala
cloudflared tunnel route dns sala sala.seudominio.com
```

O login abre o navegador uma vez. Anote o UUID e o caminho JSON informados. Crie
um arquivo dedicado `sala-tunnel.yml`, sem editar `config.yml` dos outros apps:

```yaml
tunnel: UUID-DO-TUNEL
credentials-file: C:/Users/SEU-USUARIO/.cloudflared/UUID-DO-TUNEL.json
ingress:
  - hostname: sala.seudominio.com
    service: http://127.0.0.1:8787
  - service: http_status:404
```

Execute em um terminal:

```powershell
cloudflared tunnel --config .\sala-tunnel.yml run sala
```

Em outro, inicie o app com `--origin` como acima. Os comandos criam um recurso e
um registro DNS novos; use um nome livre. O domínio, se comprado, tem custo
separado. [Tutorial oficial](https://developers.cloudflare.com/cloudflare-one/networks/connectors/cloudflare-tunnel/do-more-with-tunnels/local-management/create-local-tunnel/).

## Testar com os amigos

Entre em duas redes diferentes, transmita e clique em **Assistir**. Confira imagem
**e áudio** em ambos os sentidos. Mantenha **Usar somente TURN desmarcado** neste
perfil gratuito: ele não possui relay.

Se falhar apenas a mídia, trocar o hostname/Cloudflare Tunnel não resolve o NAT.
O próximo passo é avaliar TURN com franquia, sabendo que excedentes podem custar,
ou outra solução de rede. Não envie vídeo codificado por WebSocket só para
contornar isso: seria outro transporte, com outras características de latência.

O servidor Windows precisa continuar ligado e sem suspensão. Só os scripts
necessitam de Node; os amigos continuam precisando apenas do app/navegador.
