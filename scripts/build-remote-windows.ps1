param([switch]$UpdateServer, [switch]$Diagnostics, [switch]$DistributionOnly)
$ErrorActionPreference = 'Stop'
$archive = Join-Path $HOME 'janja-nativo-fontes.zip'
$source = Join-Path $HOME 'Downloads\janja-nativo-r12'
if (!(Test-Path -LiteralPath $archive)) { throw "Arquivo ausente: $archive" }
Expand-Archive -LiteralPath $archive -DestinationPath $source -Force
if ($DistributionOnly) {
    $env:MSYSTEM = 'UCRT64'
    $env:JANJA_SOURCE_ROOT = $source
    $package = @'
set -e
export PATH="/ucrt64/bin:/usr/bin:$PATH"
cd "$(cygpath -u "$JANJA_SOURCE_ROOT")"
python scripts/package-native.py --diagnostics
'@
    $package | & C:\msys64\usr\bin\bash.exe -l
} else {
    $build = @('-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', (Join-Path $source 'scripts\build-windows.ps1'))
    if ($Diagnostics) { $build += '-Diagnostics' }
    & powershell.exe @build
}
if ($LASTEXITCODE -ne 0) { throw 'Compilação falhou. O executável anterior não foi substituído.' }
$artifacts = @('janja-windows.exe', 'janja-setup-windows.exe', 'windows.build.json')
if ($Diagnostics) { $artifacts += 'janja-diagnostico-windows.exe' }
foreach ($name in $artifacts) {
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
