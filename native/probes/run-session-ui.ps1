param(
    [Parameter(Mandatory)][string]$PublishDirectory,
    [Parameter(Mandatory)][string]$BrokerExecutable,
    [Parameter(Mandatory)][string]$ArtifactsDirectory,
    [switch]$Launch
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
if (!$IsWindows) { throw 'Native session UI requires Windows.' }
$root = [IO.Path]::GetFullPath($ArtifactsDirectory)
New-Item -ItemType Directory -Path $root -Force | Out-Null
function FreePort {
    $listener = [Net.Sockets.TcpListener]::new([Net.IPAddress]::Loopback, 0)
    try { $listener.Start(); return ([Net.IPEndPoint]$listener.LocalEndpoint).Port } finally { $listener.Stop() }
}
$marker = Join-Path $root 'laboratory.json'
$configuration = Join-Path $root 'desktop.config.json'
if (Test-Path -LiteralPath $marker) {
    $lab = Get-Content -LiteralPath $marker -Raw | ConvertFrom-Json
    if ($lab.scope -ne 'n03a-session-ui' -or $lab.port -lt 1024 -or $lab.callback -lt 1024 -or
        $lab.port -gt 65535 -or $lab.callback -gt 65535 -or $lab.port -eq $lab.callback) { throw 'Invalid owned session UI laboratory.' }
} else {
    $lab = [ordered]@{ scope = 'n03a-session-ui'; port = FreePort; callback = FreePort }
    while ($lab.callback -eq $lab.port) { $lab.callback = FreePort }
    $lab | ConvertTo-Json | Set-Content -LiteralPath $marker -Encoding utf8
}
$start = [Diagnostics.ProcessStartInfo]::new([IO.Path]::GetFullPath($BrokerExecutable))
$start.UseShellExecute = $false
$start.CreateNoWindow = $true
$start.WindowStyle = [Diagnostics.ProcessWindowStyle]::Hidden
$start.RedirectStandardOutput = $true
$start.RedirectStandardError = $true
foreach ($value in @('--database', (Join-Path $root 'broker.sqlite'), '--callback-uri', "http://127.0.0.1:$($lab.callback)/return",
    '--session-lab', '--listen-port', [string]$lab.port)) { $start.ArgumentList.Add($value) }
if ($Launch) { $start.ArgumentList.Add('--interactive') }
$broker = [Diagnostics.Process]::Start($start)
$errors = $broker.StandardError.ReadToEndAsync()
try {
    $ready = $broker.StandardOutput.ReadLineAsync().WaitAsync([TimeSpan]::FromSeconds(25)).GetAwaiter().GetResult()
    if (!$ready -or !$ready.StartsWith('N01_READY ')) { throw 'Session broker did not become ready.' }
    $output = $broker.StandardOutput.ReadToEndAsync()
    $info = $ready.Substring(10) | ConvertFrom-Json
    if ($info.address -ne "http://127.0.0.1:$($lab.port)" -or $info.issuer -ne "$($info.address)/provider" -or
        $info.instance_id -notmatch '^[A-Za-z0-9_-]{43}$') { throw 'Unexpected session broker scope.' }
    if (Test-Path -LiteralPath $configuration) {
        $old = Get-Content -LiteralPath $configuration -Raw | ConvertFrom-Json
        if ($old.instance -ne $info.instance_id -or $old.broker -ne $info.address) { throw 'Session broker identity changed.' }
    }
    [ordered]@{ broker = $info.address; instance = $info.instance_id; callback = "http://127.0.0.1:$($lab.callback)/return";
        accounts = @('alice'); systemBrowser = [bool]$Launch; sessionLab = $true } |
        ConvertTo-Json -Depth 3 | Set-Content -LiteralPath $configuration -Encoding utf8
    $modes = if ($Launch) { @('--session-config') } else { @('--session-ui-cancel', '--session-ui-smoke', '--session-ui-resume') }
    foreach ($mode in $modes) {
        $ui = [Diagnostics.ProcessStartInfo]::new((Join-Path ([IO.Path]::GetFullPath($PublishDirectory)) 'Sproyt.ReactorProbe.exe'))
        $ui.WorkingDirectory = [IO.Path]::GetFullPath($PublishDirectory)
        $ui.UseShellExecute = $false
        $ui.WindowStyle = if ($Launch) { [Diagnostics.ProcessWindowStyle]::Normal } else { [Diagnostics.ProcessWindowStyle]::Hidden }
        $ui.ArgumentList.Add($mode); $ui.ArgumentList.Add($configuration)
        $process = [Diagnostics.Process]::Start($ui)
        try {
            if ($Launch) { $process.WaitForExit() }
            elseif (!$process.WaitForExit(75000)) { $process.Kill($true); throw 'Native session UI timed out.' }
            if ($process.ExitCode -ne 0) { throw 'Native session UI failed; inspect bounded diagnostics in the laboratory.' }
        } finally { if (!$process.HasExited) { $process.Kill($true) }; $process.Dispose() }
    }
    if (!$Launch) {
        foreach ($name in @('session-ui-cancel', 'session-ui', 'session-ui-resume')) {
            $receipt = Get-Content -LiteralPath (Join-Path $root ($name + '-result.json')) -Raw | ConvertFrom-Json
            if (!$receipt.passed -or !$receipt.syntheticSession -or $receipt.productionAuthentication -or
                (Get-Content -LiteralPath (Join-Path $root ($name + '-closed.txt')) -Raw).Trim() -ne 'closed') { throw 'Invalid native session UI receipt.' }
        }
        Write-Output ('PASS native session UI, login/logout and process restart: ' + $root)
    }
} finally {
    if (!$broker.HasExited) { $broker.Kill($true) }
    $broker.WaitForExit(); $broker.Dispose()
    $null = $errors.GetAwaiter().GetResult()
}
