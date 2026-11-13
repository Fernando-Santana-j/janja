$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'update-windows.ps1')
function Assert-Rejected([scriptblock]$Action) {
    $rejected = $false
    try { & $Action | Out-Null } catch { $rejected = $true }
    if (!$rejected) { throw 'Atualizacao insegura foi aceita.' }
}
$directory = Join-Path ([IO.Path]::GetTempPath()) ('janja-update-test-' + [guid]::NewGuid())
New-Item -ItemType Directory -Path $directory | Out-Null
$rsa = [Security.Cryptography.RSACryptoServiceProvider]::new(3072, [Security.Cryptography.CspParameters]::new(24))
$rsa.PersistKeyInCsp = $false
$sha = [Security.Cryptography.SHA256]::Create()
try {
    $path = Join-Path $directory 'setup.exe'
    [IO.File]::WriteAllBytes($path, [byte[]]@(1, 2, 3, 4))
    $asset = @{name='janja-setup-windows.exe'; size=4; sha256=(Get-FileHash $path -Algorithm SHA256).Hash.ToLowerInvariant()}
    $metadata = @{schema=1; repository='owner/app'; version='2026.11.13-r12-leve'; revision=12; files=@{'windows-x86_64'=$asset}}
    $bytes = [Text.Encoding]::UTF8.GetBytes(($metadata | ConvertTo-Json -Depth 5 -Compress))
    $signature = $rsa.SignData($bytes, $sha)
    $public = $rsa.ToXmlString($false)
    $verified = Read-VerifiedManifest $bytes $signature $public 'owner/app'
    if ($verified.version -cne $metadata.version) { throw 'Versao valida rejeitada.' }
    Assert-UpdateFile $path $verified.files.'windows-x86_64'
    Assert-Rejected { Read-VerifiedManifest $bytes $signature $public 'other/repo' }
    $bytes[0] = $bytes[0] -bxor 1
    Assert-Rejected { Read-VerifiedManifest $bytes $signature $public 'owner/app' }
    $bytes[0] = $bytes[0] -bxor 1
    $signature[0] = $signature[0] -bxor 1
    Assert-Rejected { Read-VerifiedManifest $bytes $signature $public 'owner/app' }
    [IO.File]::WriteAllBytes($path, [byte[]]@(1, 2, 3, 5))
    Assert-Rejected { Assert-UpdateFile $path $verified.files.'windows-x86_64' }
    [IO.File]::WriteAllBytes($path, [byte[]]@(1, 2, 3))
    Assert-Rejected { Assert-UpdateFile $path $verified.files.'windows-x86_64' }
    foreach ($bad in @(
        @{schema=2; name='janja-setup-windows.exe'; size=4; revision=12},
        @{schema=1; name='../other.exe'; size=4; revision=12},
        @{schema=1; name='janja-setup-windows.exe'; size=536870913; revision=12},
        @{schema=1; name='janja-setup-windows.exe'; size=4; revision=13}
    )) {
        $metadata.schema = $bad.schema
        $metadata.revision = $bad.revision
        $asset.name = $bad.name
        $asset.size = $bad.size
        $bytes = [Text.Encoding]::UTF8.GetBytes(($metadata | ConvertTo-Json -Depth 5 -Compress))
        $signature = $rsa.SignData($bytes, $sha)
        Assert-Rejected { Read-VerifiedManifest $bytes $signature $public 'owner/app' }
    }
    # Exercise real backup/restore on temporary files; never launch a client or touch user registration.
    function Process-Result([int]$ExitCode) {
        return [pscustomobject]@{ExitCode=$ExitCode} | Add-Member -MemberType ScriptMethod -Name Dispose -Value { $script:Disposed++ } -PassThru
    }
    function Start-Process {
        param([string]$FilePath, [string[]]$ArgumentList, [switch]$Wait, [switch]$PassThru)
        if ($FilePath -eq $script:InstallSetup) {
            if ($ArgumentList[-1] -cne "/D=$script:InstallDirectory") { throw 'Destino da atualizacao incorreto.' }
            foreach ($name in $script:OwnedFiles) {
                [IO.File]::WriteAllText((Join-Path $script:InstallDirectory $name), 'updated')
            }
            $installedVersion = if ($script:Failure -eq 'wrong_version') { 'wrong' } else { $script:NewVersion }
            [IO.File]::WriteAllText((Join-Path $script:InstallDirectory 'version.txt'), $installedVersion)
            return Process-Result $(if ($script:Failure -eq 'installer') { 1 } else { 0 })
        }
        if ($FilePath -eq (Join-Path $script:InstallDirectory 'janja.exe') -and $ArgumentList[0] -eq '--version') {
            return Process-Result $(if ($script:Failure -eq 'startup') { 1 } else { 0 })
        }
        throw 'Processo inesperado durante o teste.'
    }
    function Set-ItemProperty {
        param([string]$Path, [string]$Name, [string]$Value)
        if ($Path -cne 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Janja' -or $Name -cne 'DisplayVersion') {
            throw 'Registro inesperado durante o teste.'
        }
        $script:RestoredVersion = $Value
    }
    $script:OwnedFiles = @('janja.exe', 'update.ps1', 'update-config.json', 'update-public.xml', 'version.txt', 'uninstall.exe')
    $script:NewVersion = '2026.11.14-r13-leve'
    foreach ($failure in @('', 'installer', 'wrong_version', 'startup')) {
        $case = Join-Path $directory ([guid]::NewGuid().ToString())
        $script:InstallDirectory = Join-Path $case 'Usuario com espacos'
        New-Item -ItemType Directory -Path $script:InstallDirectory | Out-Null
        $script:InstallSetup = Join-Path $case 'setup.exe'
        [IO.File]::WriteAllText($script:InstallSetup, 'fixture')
        $original = @{}
        foreach ($name in $script:OwnedFiles) {
            $original[$name] = if ($name -eq 'version.txt') { '2026.11.13-r12-leve' } else { "original-$name" }
            [IO.File]::WriteAllText((Join-Path $script:InstallDirectory $name), $original[$name])
        }
        [IO.File]::WriteAllText((Join-Path $script:InstallDirectory 'perfil-do-usuario.txt'), 'preservar')
        $script:Failure = $failure
        $script:RestoredVersion = $null
        $script:Disposed = 0
        if ($failure) {
            Assert-Rejected { Install-JanjaUpdate $script:InstallDirectory $script:InstallSetup $script:NewVersion }
            foreach ($name in $script:OwnedFiles) {
                if ([IO.File]::ReadAllText((Join-Path $script:InstallDirectory $name)) -cne $original[$name]) { throw "Rollback perdeu $name." }
            }
            if ($script:RestoredVersion -cne $original['version.txt']) { throw 'Versao do registro nao foi restaurada.' }
        } else {
            Install-JanjaUpdate $script:InstallDirectory $script:InstallSetup $script:NewVersion
            if ([IO.File]::ReadAllText((Join-Path $script:InstallDirectory 'version.txt')) -cne $script:NewVersion) { throw 'Nova versao nao instalada.' }
        }
        if ([IO.File]::ReadAllText((Join-Path $script:InstallDirectory 'perfil-do-usuario.txt')) -cne 'preservar') { throw 'Atualizacao alterou dados do usuario.' }
        $expectedDisposals = if ($failure -in @('installer', 'wrong_version')) { 1 } else { 2 }
        if ($script:Disposed -ne $expectedDisposals) { throw 'Recursos de processo nao foram liberados.' }
    }
} finally {
    $sha.Dispose(); $rsa.Dispose()
    Remove-Item -LiteralPath $directory -Recurse -Force
}
Write-Host 'Assinatura, integridade, atualizacao e rollback por falha do instalador, versao ou inicializacao verificados.'
