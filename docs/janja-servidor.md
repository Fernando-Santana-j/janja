# Janja: servidor de presença

Este executável serve somente sinalização; o cliente nativo ainda não está pronto.
Não use o antigo sala.exe como cliente nativo.

Cloudflare Tunnel existente: hostname infocoretech.com.br, path
`^/janja(/.*)?$`, serviço HTTP localhost:8787, antes da regra geral do site.
Não precisa abrir portas no roteador.

No Windows, com Rust estável e ferramentas MSVC instalados, dentro das fontes:

```powershell
cargo build --release --locked --bin janja-servidor
if ($LASTEXITCODE -ne 0) { throw 'Compilacao falhou' }
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\install-janja-server.ps1
Invoke-RestMethod http://127.0.0.1:8787/janja/health
Invoke-RestMethod https://infocoretech.com.br/janja/health
```

A instalação exige administrador, copia o executável para ProgramData\Janja,
registra a tarefa Janja-Servidor como LocalService e desativa a antiga
Sala-Servidor, caso exista. Não altera o serviço Cloudflared.

Diagnóstico esperado: status=ok, service=janja-signaling, protocol=1.
502 externo: verifique o serviço local/porta; 404 externo com local funcionando:
verifique a regra e sua ordem no Tunnel.

Protocolo: WebSocket /janja/ws, primeira mensagem {"type":"join"}.
Retorna welcome (id, iceServers) e room (people). Cada pessoa possui
id, name e streaming. state atualiza streaming; subscribe/unsubscribe com to
solicitam/paralisam recepção; signal com to, stream e data encaminha SDP/ICE.
Ping/Pong mantém a conexão. Máximo de 8 participantes, mensagens de 64 KiB.
Reconexão exige novo join e nova negociação de mídia.

Sala pública sem senha. O endereço fixo não é autenticação.
Mídia deve circular diretamente entre clientes; STUN sozinho não garante
conexão em todas as redes. O servidor não fornece TURN nem retransmissão.
