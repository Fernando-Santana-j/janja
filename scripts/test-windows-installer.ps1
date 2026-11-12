$ErrorActionPreference = 'Stop'
$root = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$setup = Join-Path $root 'dist\janja-setup-windows.exe'
$key = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Janja'
if (Test-Path $key) { throw 'Execute este teste numa conta sem Janja instalado (CI ou VM).' }
if (!(Test-Path -LiteralPath $setup)) { throw "Instalador ausente: $setup" }
$directory = Join-Path ([IO.Path]::GetTempPath()) ('Janja teste ' + [char]0x00E1 + ' ' + [guid]::NewGuid())
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
foreach ($file in @($exe, $uninstall, $desktop, $menu)) {
    if (!(Test-Path -LiteralPath $file)) { throw "Arquivo nao instalado: $file" }
}
$entry = Get-ItemProperty $key
if ($entry.DisplayVersion -ne $version -or $entry.InstallLocation -ne $directory) { throw 'Registro de instalacao incorreto.' }
Set-Content -LiteralPath $sentinel -Value 'Preservar'
$process = Start-Process -FilePath $setup -ArgumentList '/S' -Wait -PassThru
if ($process.ExitCode -ne 0 -or !(Test-Path -LiteralPath $sentinel)) { throw 'Reinstalacao falhou ou removeu arquivo do usuario.' }

$stdout = Join-Path $directory 'version.txt'
$stderr = Join-Path $directory 'version-error.txt'
$process = Start-Process -FilePath $exe -ArgumentList '--version' -RedirectStandardOutput $stdout -RedirectStandardError $stderr -Wait -PassThru
if ($process.ExitCode -ne 0 -or !(Get-Content $stdout -Raw).Contains($version)) { throw 'O aplicativo instalado nao executou a versao esperada.' }

$process = Start-Process -FilePath $uninstall -ArgumentList @('/S', "_?=$directory") -Wait -PassThru
if ($process.ExitCode -ne 0) { throw "Desinstalacao falhou: $($process.ExitCode)" }
foreach ($file in @($exe, $desktop, $menu, $key)) {
    if (Test-Path -LiteralPath $file) { throw "Nao foi removido: $file" }
}
if ((Get-Content -LiteralPath $sentinel -Raw).Trim() -ne 'Preservar') { throw 'A desinstalacao removeu dados do usuario.' }
# _?= keeps the uninstaller in place so the test can wait for its real exit code.
Remove-Item -LiteralPath $directory -Recurse -Force
Write-Host 'Instalacao, versao, reinstalacao e desinstalacao verificadas.'
