param(
    [Parameter(Mandatory = $true)][string]$Origin,
    [string]$Binary = (Join-Path $PSScriptRoot '..\sala.exe')
)
$ErrorActionPreference = 'Stop'
$uri = [Uri]$Origin
if (!$uri.IsAbsoluteUri -or $uri.Scheme -ne 'https' -or $uri.UserInfo -or $uri.AbsolutePath -ne '/' -or $uri.Query -or $uri.Fragment) {
    throw 'Informe apenas a origem HTTPS da sala, sem caminho, credenciais ou parametros.'
}
$binaryPath = (Resolve-Path -LiteralPath $Binary).Path
$identity = [Security.Principal.WindowsIdentity]::GetCurrent()
$principal = New-Object Security.Principal.WindowsPrincipal($identity)
if (!$principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
    throw 'Execute em uma sessao SSH com permissao de administrador para registrar a tarefa.'
}
$directory = Join-Path $env:ProgramData 'Sala'
New-Item -ItemType Directory -Path $directory -Force | Out-Null
& icacls.exe $directory /inheritance:r /grant:r "$($identity.Name):(OI)(CI)F" '*S-1-5-32-544:(OI)(CI)F' '*S-1-5-18:(OI)(CI)F' '*S-1-5-19:(OI)(CI)RX' | Out-Null
if ($LASTEXITCODE -ne 0) { throw 'Nao foi possivel proteger a pasta de configuracao.' }
$file = Join-Path $directory 'servidor.json'
if (Test-Path -LiteralPath $file) {
    $config = Get-Content -LiteralPath $file -Raw | ConvertFrom-Json
    if (!$config.key -or $config.key.Length -lt 16) { throw 'Chave existente invalida; arquivo preservado.' }
    $config.origin = $uri.GetLeftPart([UriPartial]::Authority)
} else {
    $bytes = New-Object byte[] 32
    $rng = [Security.Cryptography.RandomNumberGenerator]::Create()
    try { $rng.GetBytes($bytes) } finally { $rng.Dispose() }
    $key = -join ($bytes | ForEach-Object { $_.ToString('x2') })
    $config = @{ key = $key; origin = $uri.GetLeftPart([UriPartial]::Authority); port = 8787; iceServers = @(@{ urls = 'stun:stun.cloudflare.com:3478' }) }
}
$config | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath $file -Encoding UTF8
$action = New-ScheduledTaskAction -Execute $binaryPath -Argument "serve --config `"$file`"" -WorkingDirectory (Split-Path $binaryPath)
$trigger = New-ScheduledTaskTrigger -AtStartup
$account = New-ScheduledTaskPrincipal -UserId 'S-1-5-19' -LogonType ServiceAccount
$settings = New-ScheduledTaskSettingsSet -ExecutionTimeLimit ([TimeSpan]::Zero) -RestartCount 3 -RestartInterval (New-TimeSpan -Minutes 1) -StartWhenAvailable -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries
$existing = Get-ScheduledTask -TaskName 'Sala-Servidor' -ErrorAction SilentlyContinue
if ($existing) { Stop-ScheduledTask -TaskName 'Sala-Servidor' }
Register-ScheduledTask -TaskName 'Sala-Servidor' -Action $action -Trigger $trigger -Principal $account -Settings $settings -Force | Out-Null
Start-ScheduledTask -TaskName 'Sala-Servidor'
$ready = $false
for ($attempt = 0; $attempt -lt 10; $attempt++) {
    try {
        $response = Invoke-WebRequest -UseBasicParsing 'http://127.0.0.1:8787/' -TimeoutSec 2
        if ($response.StatusCode -eq 200 -and $response.Content.Contains('id="join-form"')) { $ready = $true; break }
    } catch { Start-Sleep -Seconds 1 }
}
if (!$ready) { throw "Servidor nao respondeu. Verifique Get-ScheduledTaskInfo -TaskName Sala-Servidor; configuracao em $file" }
Write-Host "Servidor local respondendo. Origem: $($config.origin)"
Write-Host "Configuracao: $file (a chave nao foi exibida)."
Write-Host 'A tarefa continua rodando apos sair do SSH e inicia com o Windows.'
