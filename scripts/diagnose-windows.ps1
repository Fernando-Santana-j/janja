param(
    [ValidateRange(10,1800)][int]$Seconds = 120,
    [string]$GameProcess = '',
    [string]$PresentMonPath = '',
    [string]$Report = (Join-Path (Get-Location) 'janja-desempenho.json')
)
$ErrorActionPreference = 'Stop'
$root = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$binary = Join-Path $root 'dist\janja-diagnostico-windows.exe'
if (!(Test-Path -LiteralPath $binary)) { throw 'Gere o pacote com scripts\build-windows.ps1 -Diagnostics.' }
$logs = Join-Path ([IO.Path]::GetTempPath()) ('janja-performance-' + [guid]::NewGuid())
New-Item -ItemType Directory -Path $logs | Out-Null
$oldLogs = $env:JANJA_LOG_DIR
$env:JANJA_LOG_DIR = $logs
try { Start-Process -FilePath $binary | Out-Null } finally { $env:JANJA_LOG_DIR = $oldLogs }
$game = [IO.Path]::GetFileNameWithoutExtension($GameProcess)
$present = $null
$csv = Join-Path $logs 'game-frames.csv'
if ($PresentMonPath) {
    if (!$game -or !(Test-Path -LiteralPath $PresentMonPath)) { throw 'Informe o executavel do jogo e o caminho do PresentMon.' }
    $present = Start-Process -FilePath $PresentMonPath -ArgumentList @('--process_name', ('"' + $game + '.exe"'), '--v1_metrics', '--timed', $Seconds, '--terminate_after_timed', '--output_file', ('"' + $csv + '"'), '--no_console_stats') -PassThru
}
Write-Host "Inicie o jogo e a transmissao. Coletando $Seconds segundos; o aplicativo continuara aberto ao terminar."
$cores = [Environment]::ProcessorCount
$samples = @()
$previous = @{}
$started = Get-Date
$last = $started
while (((Get-Date) - $started).TotalSeconds -lt $Seconds) {
    Start-Sleep -Seconds 2
    $now = Get-Date
    $elapsed = ($now - $last).TotalSeconds
    $clients = @(Get-Process -Name janja,janja-windows,janja-diagnostico-windows -ErrorAction SilentlyContinue)
    $games = @()
    if ($game) { $games = @(Get-Process -Name $game -ErrorAction SilentlyContinue) }
    $metrics = @{}
    foreach ($group in @(@{name='client'; processes=$clients}, @{name='game'; processes=$games})) {
        $cpu = 0.0; [long]$rss = 0; [long]$private = 0
        foreach ($process in $group.processes) {
            try {
                $total = $process.TotalProcessorTime.TotalSeconds
                if ($previous.ContainsKey($process.Id)) { $cpu += [Math]::Max(0, $total - $previous[$process.Id]) }
                $previous[$process.Id] = $total
                $rss += $process.WorkingSet64
                $private += $process.PrivateMemorySize64
            } catch { }
        }
        $metrics[$group.name] = @{cpu_percent_one_core=100 * $cpu / $elapsed; rss_bytes=$rss; private_bytes=$private}
    }
    $gpu = @(Get-CimInstance Win32_PerfFormattedData_GPUPerformanceCounters_GPUEngine -ErrorAction SilentlyContinue)
    foreach ($group in @(@{name='client'; processes=$clients}, @{name='game'; processes=$games})) {
        $peak = $null
        foreach ($engine in $gpu) {
            if ($engine.Name -match 'pid_(\d+)_') {
                if ([int]$Matches[1] -in @($group.processes.Id)) {
                    if ($null -eq $peak -or $engine.UtilizationPercentage -gt $peak) { $peak = $engine.UtilizationPercentage }
                }
            }
        }
        $metrics[$group.name].gpu_max_engine_percent = $peak
    }
    $samples += @{seconds=($now - $started).TotalSeconds; client=$metrics.client; game=$metrics.game}
    $last = $now
}
$events = @()
foreach ($file in Get-ChildItem -LiteralPath $logs -Filter '*.jsonl') {
    foreach ($line in Get-Content -LiteralPath $file.FullName) {
        try {
            $record = $line | ConvertFrom-Json
            if ($record.event -eq 'capture-metrics') {
                $events += @{at_ms=$record.at_ms; raw_fps=$record.data.raw_fps; encoded_fps=$record.data.encoded_fps; encoded_mbps=$record.data.encoded_mbps; encoder=($record.data.encoder -split ' ')[0]}
            }
        } catch { }
    }
}
$gameFps = $null
if ($present) {
    if (!$present.WaitForExit(10000)) { Write-Warning 'PresentMon ainda esta terminando; os FPS nao foram incluidos.' }
    elseif ($present.ExitCode -eq 0 -and (Test-Path -LiteralPath $csv)) {
        $chain = Import-Csv -LiteralPath $csv | Group-Object SwapChainAddress | Sort-Object Count -Descending | Select-Object -First 1
        $intervals = @($chain.Group | ForEach-Object {
            $value = 0.0
            if ([double]::TryParse($_.MsBetweenPresents, [Globalization.NumberStyles]::Float, [Globalization.CultureInfo]::InvariantCulture, [ref]$value) -and $value -gt 0) { $value }
        })
        if ($intervals.Count -gt 0) { $gameFps = 1000 / ($intervals | Measure-Object -Average).Average }
    }
}
@{logical_cores=$cores; cpu_unit='100 percent equals one core'; seconds=$Seconds; samples=$samples; capture=$events; game_present_fps=$gameFps;
  note='Compare the same scene without transmission and during transmission. A missing GPU/FPS value means not measured.'} |
    ConvertTo-Json -Depth 7 | Set-Content -LiteralPath $Report -Encoding UTF8
Write-Host "Relatorio: $Report. Compartilhe somente esse JSON, sem os logs brutos."
