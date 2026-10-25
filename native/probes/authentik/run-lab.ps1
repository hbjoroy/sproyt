param(
    [Parameter(Mandatory)][string]$RunDirectory,
    [switch]$SystemBrowser,
    [switch]$HttpOnly
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$run = [IO.Path]::GetFullPath($RunDirectory)
$environment = Get-Content (Join-Path $run 'environment.json') -Raw | ConvertFrom-Json
if ($environment.environment -ne 'sproyt-n01c-local' -or $environment.status -ne 'ready' -or $environment.prefix -notmatch '^sproyt-n01c-[a-f0-9]{8}$') { throw 'Not a ready local lab.' }
if ($SystemBrowser -and (!$IsWindows -or $HttpOnly)) { throw 'The system-browser lab requires Windows native handoff.' }
if (!$IsWindows -and !$HttpOnly) { throw 'Linux CI must explicitly request the HTTP contract, not native Windows acceptance.' }
$containerCli = if ($IsWindows) { 'wslc' } else { 'docker' }
$worker = $environment.prefix + '-worker'
# Only the synthetic lab worker can mint these test sessions. None are logged
# or included in the public receipt. This is not password/passkey acceptance.
if (!$SystemBrowser) {
    $sessionOutput = @(& $containerCli exec $worker ak shell -c "exec(compile(open('/lab-code/session-fixture.py').read(), '/lab-code/session-fixture.py', 'exec'))" 2>&1)
    $sessionLine = @($sessionOutput | Where-Object { $_ -is [string] -and $_.StartsWith('N01C_SESSIONS ') })
    if ($LASTEXITCODE -ne 0 -or $sessionLine.Count -ne 1) { throw 'Isolated browser-session fixture failed; output suppressed.' }
    $sessionLine[0].Substring('N01C_SESSIONS '.Length) | Set-Content (Join-Path $run 'private/browser-sessions.json') -Encoding utf8
}
$check = Join-Path $run ('checks/' + [guid]::NewGuid().ToString('N'))
$root = Split-Path $run -Parent | Split-Path -Parent
$manifest = Join-Path $PSScriptRoot '../identity/Cargo.toml'
$project = Join-Path $PSScriptRoot '../identity-client/IdentityProbe.csproj'
$priorTarget = $env:CARGO_TARGET_DIR
function Checked([string]$Command,[string[]]$Arguments) { & $Command @Arguments; if ($LASTEXITCODE -ne 0) { throw ('N01c command failed: ' + $Command) } }
try {
    $env:CARGO_TARGET_DIR = Join-Path $root 'rust-target'
    Checked 'cargo' @('fmt','--manifest-path',$manifest,'--','--check')
    Checked 'cargo' @('clippy','--manifest-path',$manifest,'--locked','--all-targets','--','-D','warnings')
    Checked 'cargo' @('test','--manifest-path',$manifest,'--locked')
    Checked 'cargo' @('build','--manifest-path',$manifest,'--locked')
    Push-Location (Split-Path $project -Parent)
    try { Checked 'dotnet' @('publish',$project,'-c','Release','-p:UseSharedCompilation=false','--artifacts-path',(Join-Path $check 'managed'),'-o',(Join-Path $check 'client'),'--nologo') }
    finally { Pop-Location }
    $environment | Add-Member -NotePropertyName sourceRevision -NotePropertyValue ((git -C $PSScriptRoot rev-parse HEAD).Trim()) -Force
    $environment | ConvertTo-Json -Depth 4 | Set-Content (Join-Path $run 'environment.json') -Encoding utf8
    $serverName = if ($IsWindows) { 'sproyt-identity-probe.exe' } else { 'sproyt-identity-probe' }
    $mode = if ($SystemBrowser) { '--authentik-browser' } elseif ($HttpOnly) { '--authentik-http-test' } else { '--authentik-test' }
    Checked 'dotnet' @((Join-Path $check 'client/IdentityProbe.dll'),$mode,(Join-Path $env:CARGO_TARGET_DIR ('debug/' + $serverName)),$run,(Join-Path $check 'evidence'))
    $receiptPath = Join-Path $check 'evidence/receipt.json'
    $receipt = Get-Content $receiptPath -Raw | ConvertFrom-Json
    if (!$receipt.passed) { throw 'N01c evidence failed.' }
    $receipt | Add-Member -NotePropertyName sourceDirty -NotePropertyValue (@(git -C $PSScriptRoot status --porcelain -- ':(top)native/probes').Count -gt 0) -Force
    $receipt | ConvertTo-Json -Depth 8 | Set-Content $receiptPath -Encoding utf8
    Write-Output ('PASS N01c Authentik contract; receipt: ' + $receiptPath)
} finally { $env:CARGO_TARGET_DIR = $priorTarget }
