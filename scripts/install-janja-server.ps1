param([string]$Binary = (Join-Path $PSScriptRoot '..\target\release\janja-servidor.exe'), [string]$MasterPasswordFile)
$ErrorActionPreference = 'Stop'
$source = (Resolve-Path -LiteralPath $Binary).Path
$identity = [Security.Principal.WindowsIdentity]::GetCurrent()
$principal = New-Object Security.Principal.WindowsPrincipal($identity)
if (!$principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
    throw 'Execute o PowerShell/SSH como administrador.'
}
$directory = Join-Path $env:ProgramData 'Janja'
New-Item -ItemType Directory -Path $directory -Force | Out-Null
& icacls.exe $directory /inheritance:r /grant:r "$($identity.Name):(OI)(CI)F" '*S-1-5-32-544:(OI)(CI)F' '*S-1-5-18:(OI)(CI)F' '*S-1-5-19:(OI)(CI)RX' | Out-Null
if ($LASTEXITCODE -ne 0) { throw 'Falha ao configurar permissoes.' }
$destination = Join-Path $directory 'janja-servidor.exe'
if ($MasterPasswordFile) {
    $master = Get-Content -LiteralPath $MasterPasswordFile -Raw | ConvertFrom-Json
    if ($master.salt.Count -ne 16 -or $master.hash.Count -ne 32 -or @($master.salt + $master.hash | Where-Object { $_ -isnot [int] -and $_ -isnot [long] -or $_ -lt 0 -or $_ -gt 255 }).Count) {
        throw 'Configuracao de senha mestra invalida.'
    }
}
foreach ($name in @('Sala-Servidor', 'Janja-Servidor')) {
    if (Get-ScheduledTask -TaskName $name -ErrorAction SilentlyContinue) {
        Disable-ScheduledTask -TaskName $name | Out-Null
        Stop-ScheduledTask -TaskName $name
    }
}
# Stop-ScheduledTask can return while the executable is still locked.
for ($attempt = 0; $attempt -lt 50; $attempt++) {
    $running = @(Get-CimInstance Win32_Process -Filter "Name='janja-servidor.exe'" |
        Where-Object { $_.ExecutablePath -eq $destination })
    if ($running.Count -eq 0) { break }
    foreach ($serverProcess in $running) {
        Stop-Process -Id $serverProcess.ProcessId -Force -ErrorAction SilentlyContinue
    }
    Start-Sleep -Milliseconds 200
}
if ($running.Count -ne 0) { throw "O servidor antigo nao encerrou: $destination. A copia foi cancelada." }
if ($source -ne $destination) { Copy-Item -LiteralPath $source -Destination $destination -Force }
if ($MasterPasswordFile) { Copy-Item -LiteralPath $MasterPasswordFile -Destination (Join-Path $directory 'master-password.json') -Force }
$action = New-ScheduledTaskAction -Execute $destination -WorkingDirectory $directory
$trigger = New-ScheduledTaskTrigger -AtStartup
$account = New-ScheduledTaskPrincipal -UserId 'S-1-5-19' -LogonType ServiceAccount
$settings = New-ScheduledTaskSettingsSet -ExecutionTimeLimit ([TimeSpan]::Zero) -RestartCount 3 -RestartInterval (New-TimeSpan -Minutes 1) -StartWhenAvailable -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries
Register-ScheduledTask -TaskName 'Janja-Servidor' -Action $action -Trigger $trigger -Principal $account -Settings $settings -Force | Out-Null
Start-ScheduledTask -TaskName 'Janja-Servidor'
for ($attempt = 0; $attempt -lt 15; $attempt++) {
    try {
        $health = Invoke-RestMethod 'http://127.0.0.1:8787/janja/health' -TimeoutSec 2
        if ($health.status -eq 'ok' -and $health.service -eq 'janja-signaling' -and $health.protocol -eq 2 -and $health.passwordOptional -eq $true) {
            if ($MasterPasswordFile -and $health.masterPasswordConfigured -ne $true) { throw 'Senha mestra nao foi carregada pelo servidor.' }
            Write-Host 'Janja pronto (protocolo 2). Inicia com o Windows e permanece ativo ao sair do SSH.'
            exit 0
        }
    } catch {}
    Start-Sleep -Seconds 1
}
throw 'Servidor nao respondeu. Consulte Get-ScheduledTaskInfo Janja-Servidor e confira se a porta 8787 esta ocupada.'
