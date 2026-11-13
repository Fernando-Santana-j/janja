param([switch]$DistributionOnly)
$ErrorActionPreference = 'Stop'
$env:MSYSTEM = 'UCRT64'
$env:JANJA_SOURCE_ROOT = Join-Path $HOME 'Downloads\janja-nativo-r12'
$checks = @'
set -e
export PATH="/ucrt64/bin:/usr/bin:$PATH"
cd "$(cygpath -u "$JANJA_SOURCE_ROOT")"
cargo test --release --locked --features native,portable -- --test-threads=1
cargo test --release --locked --features native,portable actual_webrtc_loopback -- --ignored --nocapture --test-threads=1
export JANJA_STRESS_REPORT="$(cygpath -w "$JANJA_SOURCE_ROOT/dist/windows-stress-test.json")"
cargo test --release --locked --features native,portable bounded_resource_pressure_keeps_webrtc_video_and_audio -- --ignored --nocapture --test-threads=1
'@
if (!$DistributionOnly) {
    $checks | & C:\msys64\usr\bin\bash.exe -l
    if ($LASTEXITCODE -ne 0) { throw 'Testes Rust ou WebRTC falharam.' }
}
& powershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass -File (Join-Path $env:JANJA_SOURCE_ROOT 'scripts\test-windows-updater.ps1')
if ($LASTEXITCODE -ne 0) { throw 'Atualizador nao passou nos testes.' }
$directory = Join-Path $env:JANJA_SOURCE_ROOT 'dist'
Copy-Item -LiteralPath (Join-Path $directory 'windows-stress-test.json') -Destination (Join-Path $HOME 'Downloads\windows-stress-test.json') -Force
$env:JANJA_LOG_DIR = Join-Path $directory 'validation-logs'
foreach ($mode in @('self-test', 'network-test')) {
    $report = Join-Path $directory "windows-$mode.json"
    $exe = Join-Path $directory 'janja-diagnostico-windows.exe'
    $process = Start-Process -FilePath $exe -ArgumentList "--$mode", '--report', $report -Wait -PassThru
    if ($process.ExitCode -ne 0) { throw "Falha em $mode. Consulte $report." }
    Get-Content -LiteralPath $report
    Copy-Item -LiteralPath $report -Destination (Join-Path $HOME "Downloads\windows-$mode.json") -Force
}
$existing = Get-ItemProperty 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Janja' -ErrorAction SilentlyContinue
$testPrefix = Join-Path ([IO.Path]::GetTempPath()) 'Janja teste *'
if ($existing -and $existing.InstallLocation -notlike $testPrefix) {
    Write-Host 'Teste de instalacao isolada preservado para o CI: Janja ja esta instalado nesta conta.'
} else {
    & powershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass -File (Join-Path $env:JANJA_SOURCE_ROOT 'scripts\test-windows-installer.ps1')
    if ($LASTEXITCODE -ne 0) { throw 'Instalador nao passou nos testes.' }
}
& powershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass -File (Join-Path $env:JANJA_SOURCE_ROOT 'scripts\test-windows-security.ps1')
Copy-Item -LiteralPath (Join-Path $directory 'windows-security.json') -Destination (Join-Path $HOME 'Downloads\windows-security.json') -Force
if ($LASTEXITCODE -ne 0) { throw 'A verificacao de seguranca exige revisao; consulte windows-security.json.' }
Write-Host 'Validacao Windows concluida, incluindo a verificacao do Defender.'
