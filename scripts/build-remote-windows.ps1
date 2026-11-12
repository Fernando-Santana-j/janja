param([switch]$UpdateServer)
$ErrorActionPreference = 'Stop'
$archive = Join-Path $HOME 'janja-nativo-fontes.zip'
$source = Join-Path $HOME 'Downloads\janja-nativo-r11'
if (!(Test-Path -LiteralPath $archive)) { throw "Arquivo ausente: $archive" }
Expand-Archive -LiteralPath $archive -DestinationPath $source -Force
& powershell.exe -NoProfile -ExecutionPolicy Bypass -File (Join-Path $source 'scripts\build-windows.ps1')
if ($LASTEXITCODE -ne 0) { throw 'Compilação falhou. O executável anterior não foi substituído.' }
foreach ($name in @('janja-windows.exe', 'janja-setup-windows.exe')) {
    $built = Join-Path $source "dist\$name"
    if (!(Test-Path -LiteralPath $built)) { throw "Executável ausente: $built" }
    $destination = Join-Path $HOME "Downloads\$name"
    Copy-Item -LiteralPath $built -Destination $destination -Force
    Write-Host "Pronto: $destination"
}

if ($UpdateServer) {
    $master = Join-Path $HOME 'janja-master-password.json'
    $install = @('-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', (Join-Path $source 'scripts\install-janja-server.ps1'))
    if (Test-Path -LiteralPath $master) { $install += @('-MasterPasswordFile', $master) }
    & powershell.exe @install
    if ($LASTEXITCODE -ne 0) { throw 'Cliente gerado, mas a atualizacao do servidor falhou.' }
    $health = Invoke-RestMethod 'http://127.0.0.1:8787/janja/health' -TimeoutSec 5
    if ($health.protocol -ne 2) { throw 'O servidor ainda esta na versao antiga.' }
    if ($health.passwordOptional -ne $true) { throw 'O servidor ainda nao suporta salas sem senha.' }
    Write-Host "Servidor atualizado: $($health.build)"
}
