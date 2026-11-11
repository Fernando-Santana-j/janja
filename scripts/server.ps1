param(
    [string]$Config = (Join-Path $PSScriptRoot '..\server.json'),
    [string]$Binary = (Join-Path $PSScriptRoot '..\target\release\sala.exe')
)
$ErrorActionPreference = 'Stop'
$settings = Get-Content -LiteralPath $Config -Raw | ConvertFrom-Json
if (!$settings.key -or $settings.key.StartsWith('SUBSTITUA-')) {
    throw 'Edite server.json e defina uma chave aleatoria antes de iniciar.'
}
$env:SALA_KEY = $settings.key
$env:SALA_ORIGIN = $settings.origin.TrimEnd('/')
$env:SALA_ICE_SERVERS = ConvertTo-Json -InputObject @($settings.iceServers) -Depth 5 -Compress
& $Binary serve
if ($LASTEXITCODE -ne 0) { throw "Servidor encerrou com codigo $LASTEXITCODE" }
