$ErrorActionPreference = 'Stop'
$temporary = [IO.Path]::GetTempPath()
$key = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Janja'
if (Test-Path $key) {
    Get-ItemProperty $key | Select-Object DisplayVersion, InstallLocation | Format-List
}
Get-CimInstance Win32_Process | Where-Object {
    $_.ExecutablePath -like '*\janja-update-stage-*' -or $_.ExecutablePath -like '*\Janja teste *'
} | Select-Object ProcessId, ParentProcessId, Name, ExecutablePath | Format-List
Get-ChildItem -LiteralPath $temporary -Directory -Filter 'janja-update-stage-*' |
    Sort-Object LastWriteTime -Descending | Select-Object -First 3 | ForEach-Object {
        $file = Join-Path $_.FullName 'setup.exe'
        if (Test-Path -LiteralPath $file) {
            $message = 'livre agora'
            try {
                $stream = [IO.File]::Open($file, [IO.FileMode]::Open, [IO.FileAccess]::ReadWrite, [IO.FileShare]::None)
                $stream.Dispose()
            } catch { $message = $_.Exception.Message }
            [pscustomobject]@{file=$file; access=$message} | Format-List
        }
    }
try { Get-MpComputerStatus | Select-Object AntivirusEnabled, RealTimeProtectionEnabled, AntivirusSignatureVersion | Format-List }
catch { Write-Host $_.Exception.Message }
