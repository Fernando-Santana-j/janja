$ErrorActionPreference = 'Stop'
$root = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$setup = Join-Path $root 'dist\janja-setup-windows.exe'
$key = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Janja'
if (!(Test-Path -LiteralPath $setup)) { throw "Instalador ausente: $setup" }
$directory = Join-Path ([IO.Path]::GetTempPath()) ('Janja teste ' + [char]0x00E1 + ' ' + [guid]::NewGuid())
if (Test-Path $key) {
    $previous = (Get-ItemProperty $key).InstallLocation
    $prefix = 'Janja teste ' + [char]0x00E1 + ' '
    $leaf = Split-Path -Leaf $previous
    $id = [guid]::Empty
    $temporary = [IO.Path]::GetTempPath().TrimEnd([IO.Path]::DirectorySeparatorChar)
    if ((Split-Path -Parent $previous) -eq $temporary -and $leaf.StartsWith($prefix) -and
        [guid]::TryParseExact($leaf.Substring($prefix.Length), 'D', [ref]$id) -and
        (Test-Path -LiteralPath (Join-Path $previous 'arquivo-do-usuario.txt')) -and
        (Get-Content -LiteralPath (Join-Path $previous 'arquivo-do-usuario.txt') -Raw).Trim() -ceq 'Preservar') {
        $directory = $previous
        Write-Host 'Retomando somente a instalacao temporaria deixada pelo teste anterior.'
    } else { throw 'Execute este teste numa conta sem Janja instalado (CI ou VM).' }
}
$version = [regex]::Match((Get-Content (Join-Path $root 'src\lib.rs') -Raw), 'pub const BUILD: &str = "([^"]+)";').Groups[1].Value
if (!$version) { throw 'Versao do codigo nao encontrada.' }
$exe = Join-Path $directory 'janja.exe'
$uninstall = Join-Path $directory 'uninstall.exe'
$sentinel = Join-Path $directory 'arquivo-do-usuario.txt'
$desktop = Join-Path ([Environment]::GetFolderPath('DesktopDirectory')) 'Janja.lnk'
$menu = Join-Path ([Environment]::GetFolderPath('Programs')) 'Janja\Janja.lnk'

# /D and _?= must be last, without quotes, even when the path contains spaces.
$process = Start-Process -FilePath $setup -ArgumentList @('/S', "/D=$directory") -Wait -PassThru
if ($process.ExitCode -ne 0) { throw "Instalacao falhou: $($process.ExitCode)" }
$process.Dispose()
foreach ($file in @($exe, $uninstall, $desktop, $menu, (Join-Path $directory 'update.ps1'), (Join-Path $directory 'update-config.json'), (Join-Path $directory 'update-public.xml'), (Join-Path $directory 'version.txt'))) {
    if (!(Test-Path -LiteralPath $file)) { throw "Arquivo nao instalado: $file" }
}
$entry = Get-ItemProperty $key
if ($entry.DisplayVersion -ne $version -or $entry.InstallLocation -ne $directory) { throw 'Registro de instalacao incorreto.' }
if ((Get-Content (Join-Path $directory 'version.txt') -Raw) -cne $version) { throw 'Versao do atualizador incorreta.' }
$shortcut = (New-Object -ComObject WScript.Shell).CreateShortcut($desktop)
if ($shortcut.TargetPath -notlike '*\powershell.exe' -or !$shortcut.Arguments.Contains('update.ps1')) { throw 'Atalho nao usa o atualizador.' }
Set-Content -LiteralPath $sentinel -Value 'Preservar'
$process = Start-Process -FilePath $setup -ArgumentList '/S' -Wait -PassThru
if ($process.ExitCode -ne 0 -or !(Test-Path -LiteralPath $sentinel)) { throw 'Reinstalacao falhou ou removeu arquivo do usuario.' }
$process.Dispose()

# Apply the genuine installer through the production update transaction, in this isolated installation.
. (Join-Path $directory 'update.ps1')
$stage = Join-Path ([IO.Path]::GetTempPath()) ('janja-update-stage-' + [guid]::NewGuid())
New-Item -ItemType Directory -Path $stage | Out-Null
try {
    $stagedSetup = Join-Path $stage 'setup.exe'
    Copy-Item -LiteralPath $setup -Destination $stagedSetup
    Install-JanjaUpdate $directory $stagedSetup $version
    if ((Get-Content -LiteralPath $sentinel -Raw).Trim() -cne 'Preservar') { throw 'A atualizacao alterou dados do usuario.' }
} finally {
    Remove-Item -LiteralPath $stage -Recurse -Force
}

$stdout = Join-Path $directory 'version-output.txt'
$stderr = Join-Path $directory 'version-error.txt'
$process = Start-Process -FilePath $exe -ArgumentList '--version' -RedirectStandardOutput $stdout -RedirectStandardError $stderr -Wait -PassThru
if ($process.ExitCode -ne 0 -or !(Get-Content $stdout -Raw).Contains($version)) { throw 'O aplicativo instalado nao executou a versao esperada.' }
$process.Dispose()

$process = Start-Process -FilePath $uninstall -ArgumentList @('/S', "_?=$directory") -Wait -PassThru
if ($process.ExitCode -ne 0) { throw "Desinstalacao falhou: $($process.ExitCode)" }
$process.Dispose()
foreach ($file in @($exe, $desktop, $menu, $key, (Join-Path $directory 'update.ps1'), (Join-Path $directory 'version.txt'), (Join-Path $directory 'update-public.xml'))) {
    if (Test-Path -LiteralPath $file) { throw "Nao foi removido: $file" }
}
if ((Get-Content -LiteralPath $sentinel -Raw).Trim() -ne 'Preservar') { throw 'A desinstalacao removeu dados do usuario.' }
# _?= keeps the uninstaller in place so the test can wait for its real exit code.
foreach ($name in @('arquivo-do-usuario.txt', 'version-output.txt', 'version-error.txt')) {
    Remove-Item -LiteralPath (Join-Path $directory $name) -Force
}
if (@(Get-ChildItem -LiteralPath $directory -Force).Count -eq 0) { Remove-Item -LiteralPath $directory }
Write-Host 'Instalacao, versao, reinstalacao, aplicacao real da atualizacao e desinstalacao verificadas.'
