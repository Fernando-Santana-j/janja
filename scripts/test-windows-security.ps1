param([string]$Directory = (Join-Path $PSScriptRoot '..\dist'))
$ErrorActionPreference = 'Stop'
$Directory = (Resolve-Path -LiteralPath $Directory).Path
$files = @('janja-windows.exe', 'janja-setup-windows.exe')
$signatures = @()
foreach ($name in $files) {
    $path = Join-Path $Directory $name
    if (!(Test-Path -LiteralPath $path)) { throw "Arquivo ausente: $name" }
    $signature = Get-AuthenticodeSignature -LiteralPath $path
    $signatures += @{file=$name; sha256=(Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant();
        status=$signature.Status.ToString(); publisher=if ($signature.SignerCertificate) { $signature.SignerCertificate.Subject } else { $null }}
}
$report = @{authenticode=$signatures; defender=@{status='not_scanned'; exit_code=$null};
    note='A scan is limited to these artifacts and the installed Defender definitions. It does not measure SmartScreen or browser reputation.'}
try {
    $status = Get-MpComputerStatus
    $report.defender.engine_version = $status.AMEngineVersion
    $report.defender.signature_version = $status.AntivirusSignatureVersion
    $report.defender.signature_updated_utc = $status.AntivirusSignatureLastUpdated.ToUniversalTime().ToString('o')
    if (!$status.AntivirusEnabled) { throw 'Microsoft Defender indisponivel para esta verificacao.' }
    $platform = Join-Path $env:ProgramData 'Microsoft\Windows Defender\Platform'
    $scanner = Get-ChildItem -LiteralPath $platform -Filter 'MpCmdRun.exe' -Recurse -ErrorAction SilentlyContinue |
        Sort-Object FullName -Descending | Select-Object -First 1 -ExpandProperty FullName
    if (!$scanner) { $scanner = Join-Path $env:ProgramFiles 'Windows Defender\MpCmdRun.exe' }
    if (!(Test-Path -LiteralPath $scanner)) { throw 'Ferramenta de verificacao do Defender ausente.' }
    foreach ($name in $files) {
        # Scan-only mode includes archives, ignores scan exclusions and preserves the files under review.
        & $scanner -Scan -ScanType 3 -File (Join-Path $Directory $name) -DisableRemediation
        $report.defender.exit_code = $LASTEXITCODE
        if ($LASTEXITCODE -ne 0) { throw "Defender encontrou uma deteccao ou erro de verificacao em $name (codigo $LASTEXITCODE)." }
    }
    $report.defender.status = 'no_detection_in_scan'
} catch {
    $report.defender.error = $_.Exception.Message
} finally {
    $report | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath (Join-Path $Directory 'windows-security.json') -Encoding UTF8
}
$report | ConvertTo-Json -Depth 6
if ($report.defender.status -ne 'no_detection_in_scan') { throw 'A verificacao antivirus nao foi aprovada; consulte windows-security.json.' }
