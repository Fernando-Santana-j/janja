# Installed launcher: checks once before starting Janja; never runs inside the media client.
$ErrorActionPreference = 'Stop'

function Read-VerifiedManifest([byte[]]$Bytes, [byte[]]$Signature, [string]$PublicKey, [string]$Repository) {
    if ($Bytes.Length -gt 65536 -or $Signature.Length -ne 384) { throw 'Metadados de atualizacao invalidos.' }
    $rsa = [Security.Cryptography.RSACryptoServiceProvider]::new([Security.Cryptography.CspParameters]::new(24))
    $rsa.PersistKeyInCsp = $false
    $sha = [Security.Cryptography.SHA256]::Create()
    try {
        $rsa.FromXmlString($PublicKey)
        if (!$rsa.VerifyData($Bytes, $sha, $Signature)) { throw 'Assinatura de atualizacao invalida.' }
    } finally { $sha.Dispose(); $rsa.Dispose() }
    $metadata = [Text.Encoding]::UTF8.GetString($Bytes) | ConvertFrom-Json
    if ($metadata.schema -ne 1 -or $metadata.repository -cne $Repository -or
        $metadata.version -cnotmatch '^\d{4}\.\d{2}\.\d{2}-r([0-9]+)-[a-z0-9-]+$') { throw 'Versao ou repositorio invalido.' }
    if ([long]$metadata.revision -ne [long]$Matches[1]) { throw 'Revisao inconsistente.' }
    $asset = $metadata.files.'windows-x86_64'
    if ($asset.name -cne 'janja-setup-windows.exe' -or $asset.sha256 -cnotmatch '^[a-f0-9]{64}$' -or
        [long]$asset.size -le 0 -or [long]$asset.size -gt 536870912) { throw 'Artefato de atualizacao invalido.' }
    return $metadata
}

function Assert-UpdateFile([string]$Path, $Asset) {
    if ((Get-Item -LiteralPath $Path).Length -ne [long]$Asset.size -or
        (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash -ine $Asset.sha256) {
        throw 'A atualizacao baixada esta incompleta ou foi alterada.'
    }
}

function Wait-UpdateTask($Task, [datetime]$Deadline) {
    if ($script:UpdateForm) { [Windows.Forms.Application]::DoEvents() }
    if ($script:UpdateCancelled -or (Get-Date) -gt $Deadline) { throw 'Atualizacao cancelada ou sem resposta.' }
    while (!$Task.IsCompleted) {
        if ($script:UpdateForm) { [Windows.Forms.Application]::DoEvents() }
        if ($script:UpdateCancelled -or (Get-Date) -gt $Deadline) { throw 'Atualizacao cancelada ou sem resposta.' }
        Start-Sleep -Milliseconds 40
    }
    return $Task.GetAwaiter().GetResult()
}

function Get-UpdateFile([string]$Url, [string]$Path, [long]$Limit, [int]$Seconds) {
    $request = [Net.HttpWebRequest]::CreateHttp($Url)
    $request.UserAgent = 'Janja-Updater/1'
    $request.MaximumAutomaticRedirections = 5
    $deadline = (Get-Date).AddSeconds($Seconds)
    $response = $null; $stream = $null; $destination = $null
    try {
        $response = Wait-UpdateTask ($request.GetResponseAsync()) $deadline
        if ($response.ResponseUri.Scheme -ne 'https' -or $response.ContentLength -gt $Limit) { throw 'Resposta de atualizacao invalida.' }
        $stream = $response.GetResponseStream()
        $destination = [IO.File]::Create($Path)
        $buffer = New-Object byte[] 65536
        [long]$total = 0
        while (($read = Wait-UpdateTask ($stream.ReadAsync($buffer, 0, $buffer.Length)) $deadline) -gt 0) {
            $total += $read
            if ($total -gt $Limit) { throw 'Atualizacao maior que o limite permitido.' }
            $destination.Write($buffer, 0, $read)
            if ($script:UpdateProgress) {
                $script:UpdateProgress.Value = [int][Math]::Min(99, 100 * $total / $Limit)
            }
        }
    } finally {
        $request.Abort()
        if ($destination) { $destination.Dispose() }
        if ($stream) { $stream.Dispose() }
        if ($response) { $response.Dispose() }
    }
}

function Install-JanjaUpdate([string]$Directory, [string]$Setup, [string]$Version) {
    $current = (Get-Content -LiteralPath (Join-Path $Directory 'version.txt') -Raw).Trim()
    $owned = @('janja.exe', 'update.ps1', 'update-config.json', 'update-public.xml', 'version.txt', 'uninstall.exe')
    $backup = Join-Path (Split-Path -Parent $Setup) 'backup'
    New-Item -ItemType Directory -Path $backup | Out-Null
    foreach ($name in $owned) { Copy-Item -LiteralPath (Join-Path $Directory $name) -Destination $backup }
    try {
        $process = Start-Process -FilePath $Setup -ArgumentList @('/S', "/D=$Directory") -Wait -PassThru
        try { $exitCode = $process.ExitCode } finally { $process.Dispose() }
        if ($exitCode -ne 0 -or (Get-Content -LiteralPath (Join-Path $Directory 'version.txt') -Raw).Trim() -cne $Version) {
            throw 'A instalacao da atualizacao falhou.'
        }
        $smoke = Start-Process -FilePath (Join-Path $Directory 'janja.exe') -ArgumentList '--version' -Wait -PassThru
        try { $exitCode = $smoke.ExitCode } finally { $smoke.Dispose() }
        if ($exitCode -ne 0) { throw 'O aplicativo atualizado nao iniciou.' }
    } catch {
        foreach ($name in $owned) { Copy-Item -LiteralPath (Join-Path $backup $name) -Destination (Join-Path $Directory $name) -Force }
        Set-ItemProperty 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Janja' -Name DisplayVersion -Value $current
        throw
    }
}

function Invoke-JanjaUpdate([string]$Directory) {
    $exe = Join-Path $Directory 'janja.exe'
    $mutex = [Threading.Mutex]::new($false, 'Local\Janja-Updater')
    $locked = $false; $temporary = $null; $script:UpdateForm = $null
    $script:UpdateProgress = $null; $script:UpdateCancelled = $false
    try {
        try { $locked = $mutex.WaitOne(0) } catch [Threading.AbandonedMutexException] { $locked = $true }
        if (!$locked) { return }
        if (Get-Process -Name janja,janja-windows -ErrorAction SilentlyContinue) { return }
        $config = Get-Content -LiteralPath (Join-Path $Directory 'update-config.json') -Raw | ConvertFrom-Json
        if ($config.repository -cnotmatch '^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$') { throw 'Repositorio invalido.' }
        $current = (Get-Content -LiteralPath (Join-Path $Directory 'version.txt') -Raw).Trim()
        if ($current -cnotmatch '^\d{4}\.\d{2}\.\d{2}-r([0-9]+)-[a-z0-9-]+$') { throw 'Versao instalada invalida.' }
        $revision = [long]$Matches[1]
        $temporary = Join-Path ([IO.Path]::GetTempPath()) ('janja-update-' + [guid]::NewGuid())
        New-Item -ItemType Directory -Path $temporary | Out-Null
        $base = 'https://github.com/' + $config.repository + '/releases/latest/download/'
        $payload = Join-Path $temporary 'manifest.json'
        $signature = Join-Path $temporary 'manifest.sig'
        [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
        Get-UpdateFile ($base + 'janja-update.json') $payload 65536 4
        Get-UpdateFile ($base + 'janja-update.json.sig') $signature 1024 4
        $metadata = Read-VerifiedManifest ([IO.File]::ReadAllBytes($payload)) ([IO.File]::ReadAllBytes($signature)) (Get-Content -LiteralPath (Join-Path $Directory 'update-public.xml') -Raw) $config.repository
        if ([long]$metadata.revision -le $revision) { return }
        Add-Type -AssemblyName System.Windows.Forms
        $script:UpdateForm = New-Object Windows.Forms.Form
        $script:UpdateForm.Text = 'Atualizando Janja'
        $script:UpdateForm.ClientSize = New-Object Drawing.Size(380, 120)
        $script:UpdateForm.StartPosition = 'CenterScreen'
        $script:UpdateForm.FormBorderStyle = 'FixedDialog'
        $script:UpdateForm.MaximizeBox = $false
        $label = New-Object Windows.Forms.Label
        $label.Text = 'Baixando uma nova versao. Seus perfis serao preservados.'
        $label.SetBounds(16, 16, 350, 30)
        $script:UpdateProgress = New-Object Windows.Forms.ProgressBar
        $script:UpdateProgress.SetBounds(16, 52, 348, 16)
        $cancel = New-Object Windows.Forms.Button
        $cancel.Text = 'Abrir versao atual'
        $cancel.SetBounds(214, 80, 150, 26)
        $cancel.Add_Click({ $script:UpdateCancelled = $true })
        $script:UpdateForm.Add_FormClosing({ $script:UpdateCancelled = $true })
        $script:UpdateForm.Controls.AddRange(@($label, $script:UpdateProgress, $cancel))
        $script:UpdateForm.Show()
        $asset = $metadata.files.'windows-x86_64'
        $setup = Join-Path $temporary 'setup.exe'
        $url = 'https://github.com/' + $config.repository + '/releases/download/' + $metadata.version + '/' + $asset.name
        Get-UpdateFile $url $setup ([long]$asset.size) 600
        Assert-UpdateFile $setup $asset
        if ($script:UpdateCancelled) { return }
        if (Get-Process -Name janja,janja-windows -ErrorAction SilentlyContinue) { return }
        $label.Text = 'Instalando a atualizacao...'
        $cancel.Enabled = $false
        $script:UpdateProgress.Value = 100
        [Windows.Forms.Application]::DoEvents()
        Install-JanjaUpdate $Directory $setup $metadata.version
    } catch {
        # Offline, cancelled or invalid updates leave the installed application usable.
        if ($script:UpdateForm -and !$script:UpdateCancelled) {
            [Windows.Forms.MessageBox]::Show('Nao foi possivel atualizar. A versao instalada sera aberta.', 'Janja') | Out-Null
        }
    } finally {
        if ($script:UpdateForm) { $script:UpdateForm.Dispose() }
        if ($temporary -and (Test-Path -LiteralPath $temporary)) { Remove-Item -LiteralPath $temporary -Recurse -Force -ErrorAction SilentlyContinue }
        if ($locked) { $mutex.ReleaseMutex() }
        $mutex.Dispose()
        if ($locked) { Start-Process -FilePath $exe -WorkingDirectory $Directory }
    }
}

if ($MyInvocation.InvocationName -ne '.') { Invoke-JanjaUpdate $PSScriptRoot }
