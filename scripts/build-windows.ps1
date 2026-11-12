param(
    [string]$MsysRoot = 'C:\msys64',
    [switch]$Diagnostics
)
$ErrorActionPreference = 'Stop'
$bash = Join-Path $MsysRoot 'usr\bin\bash.exe'
if (!(Test-Path -LiteralPath $bash)) {
    throw 'Instale MSYS2: winget install --id MSYS2.MSYS2 --exact --accept-package-agreements --accept-source-agreements'
}
$oldSystem = $env:MSYSTEM
$oldRoot = $env:JANJA_SOURCE_ROOT
try {
    $env:MSYSTEM = 'UCRT64'
    $env:JANJA_SOURCE_ROOT = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
    & $bash -lc 'pacman -Syu --noconfirm'
    if ($LASTEXITCODE -ne 0) { throw 'A atualizacao MSYS2 interrompeu a sessao. Execute este script novamente.' }
    & $bash -lc 'pacman -Syu --noconfirm'
    if ($LASTEXITCODE -ne 0) { throw 'Falha ao atualizar MSYS2.' }
    $build = @'
set -e
pacman -S --needed --noconfirm mingw-w64-ucrt-x86_64-rust mingw-w64-ucrt-x86_64-gcc mingw-w64-ucrt-x86_64-binutils mingw-w64-ucrt-x86_64-pkgconf mingw-w64-ucrt-x86_64-python mingw-w64-ucrt-x86_64-gtk3 mingw-w64-ucrt-x86_64-gstreamer mingw-w64-ucrt-x86_64-gst-plugins-base mingw-w64-ucrt-x86_64-gst-plugins-good mingw-w64-ucrt-x86_64-gst-plugins-bad mingw-w64-ucrt-x86_64-gst-plugins-ugly mingw-w64-ucrt-x86_64-gst-libav mingw-w64-ucrt-x86_64-opus mingw-w64-ucrt-x86_64-libnice mingw-w64-ucrt-x86_64-nsis
export PATH="/ucrt64/bin:/usr/bin:$PATH"
cd "$(cygpath -u "$JANJA_SOURCE_ROOT")"
cargo build --release --locked --bin janja-servidor
python scripts/package-native.py
'@
    if ($Diagnostics) {
        $build = $build.Replace('python scripts/package-native.py', 'python scripts/package-native.py --diagnostics')
    }
    $build | & $bash -l
    if ($LASTEXITCODE -ne 0) { throw 'Compilacao falhou. Envie o erro exibido acima.' }
    Write-Host "Executavel: $env:JANJA_SOURCE_ROOT\dist\janja-windows.exe"
    Write-Host "Instalador: $env:JANJA_SOURCE_ROOT\dist\janja-setup-windows.exe"
    if ($Diagnostics) { Write-Host "Diagnostico: $env:JANJA_SOURCE_ROOT\dist\janja-diagnostico-windows.exe" }
} finally {
    $env:MSYSTEM = $oldSystem
    $env:JANJA_SOURCE_ROOT = $oldRoot
}
