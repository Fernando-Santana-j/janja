$ErrorActionPreference = 'Stop'
$root = Join-Path $HOME 'Downloads\janja-nativo-r12'
$env:PATH = 'C:\msys64\ucrt64\bin;' + $env:PATH
$case = 'client::media::tests::audio_jitter_recovers_when_loss_notice_is_evicted'
$test = $null
foreach ($candidate in (Get-ChildItem -LiteralPath (Join-Path $root 'target\release\deps') -Filter 'sala-*.exe' | Sort-Object LastWriteTime -Descending)) {
    $available = & $candidate.FullName --list
    if ($available -match [regex]::Escape($case)) { $test = $candidate; break }
}
if (!$test) { throw 'Executavel com o teste de audio nao encontrado; preserve a compilacao anterior.' }
$directory = Join-Path $root 'dist\audio-jitter-debug'
New-Item -ItemType Directory -Path $directory -Force | Out-Null
$env:GST_DEBUG = 'opusdec:6,audiodecoder:5,rtpjitterbuffer:5'
$env:GST_DEBUG_NO_COLOR = '1'
$env:JANJA_LOG_DIR = Join-Path $directory 'janja-logs'
$report = @{test_binary=$test.Name; failed=$false; attempts=0; exit_code=$null}
try {
    for ($attempt = 1; $attempt -le 40; $attempt++) {
        $trace = Join-Path $directory 'gstreamer.log'
        Remove-Item -LiteralPath $trace -Force -ErrorAction SilentlyContinue
        $env:GST_DEBUG_FILE = $trace
        $output = Join-Path $directory 'test-output.txt'
        $ErrorActionPreference = 'Continue'
        & $test.FullName $case --exact --nocapture --test-threads=1 *> $output
        $code = $LASTEXITCODE
        $ErrorActionPreference = 'Stop'
        $report.attempts = $attempt
        $report.exit_code = $code
        Write-Host "Audio: tentativa $attempt, codigo $code"
        if ($code -ne 0) {
            $report.failed = $true
            Get-Content -LiteralPath $output
            break
        }
    }
    & gst-inspect-1.0.exe opusdec *> (Join-Path $directory 'opusdec.txt')
} catch {
    $report.error = $_.Exception.Message
    Write-Host "Falha na coleta: $($report.error)"
} finally {
    $report | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $directory 'result.json') -Encoding UTF8
    $zip = Join-Path $HOME 'Downloads\windows-audio-debug.zip'
    Compress-Archive -Path (Join-Path $directory '*') -DestinationPath $zip -Force
    $report | ConvertTo-Json | Write-Host
    Write-Host "Diagnostico: $zip"
}
