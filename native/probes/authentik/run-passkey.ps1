param(
    [Parameter(Mandatory)][string]$RunDirectory,
    [Parameter(Mandatory)][ValidateSet('Contract','PasswordBaseline','PrepareLogin','PasskeyLogin','Finalize')][string]$Phase,
    [string]$HandoffDirectory
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$run = [IO.Path]::GetFullPath($RunDirectory)
$environmentPath = Join-Path $run 'environment.json'
$environment = Get-Content -LiteralPath $environmentPath -Raw | ConvertFrom-Json
if ($environment.environment -ne 'sproyt-n01c-local' -or $environment.status -ne 'ready' -or
    !$environment.passkeyLab -or $environment.origin -ne 'http://localhost:19090' -or
    $environment.prefix -notmatch '^sproyt-n01c-[a-f0-9]{8}$') { throw 'Not a ready, separate N01e localhost lab.' }
if ($Phase -ne 'Contract' -and !$IsWindows) { throw 'Actual platform-passkey acceptance requires the Windows system-browser probe.' }
$cli = if ($IsWindows) { 'wslc' } else { 'docker' }
$worker = $environment.prefix + '-worker'
function Fixture([string]$Action, [string]$Attempt = '', [switch]$ExpectRejection) {
    $arguments = @('exec','--env',('N01E_ACTION=' + $Action))
    if ($Attempt) { $arguments += @('--env',('N01E_ATTEMPT_ID=' + $Attempt)) }
    $arguments += @($worker,'ak','shell','-c',"exec(compile(open('/lab-code/passkey-fixture.py').read(), '/lab-code/passkey-fixture.py', 'exec'))")
    $raw = @(& $cli @arguments 2>&1)
    $exitResult = $LASTEXITCODE
    $result = @($raw | Where-Object { $_ -is [string] -and $_.StartsWith('N01E_RESULT ') })
    $safe = @($raw | Where-Object { $_ -is [string] -and $_.StartsWith('N01E_FAILED ') })
    if ($ExpectRejection) {
        $rejected = @($raw | Where-Object { $_ -is [string] -and $_.Trim() -eq 'N01E_CONSUMED_PROOF_REJECTED' })
        if ($Action -ne 'verify-login' -or $exitResult -ne 2 -or $result.Count -ne 0 -or
            $rejected.Count -ne 1) { throw 'Consumed proof was not explicitly rejected.' }
        return [pscustomobject]@{rejected=$true}
    }
    if ($exitResult -ne 0 -or $result.Count -ne 1) {
        throw ('Passkey fixture failed; private output suppressed. ' + ($safe -join ' '))
    }
    return $result[0].Substring('N01E_RESULT '.Length) | ConvertFrom-Json
}
function Checked([string]$Command, [string[]]$Arguments) { & $Command @Arguments; if ($LASTEXITCODE -ne 0) { throw ('N01e command failed: ' + $Command) } }
function CompleteEvidence([string]$ReceiptPath, [string]$Attempt) {
    $receipt = Get-Content -LiteralPath $ReceiptPath -Raw | ConvertFrom-Json
    if (!$receipt.passed -or !$receipt.nativeWindowsHandoff -or !$receipt.baselineComparison -or $receipt.passkeyVerified -or
        $receipt.scope -ne 'n01e_passkey_handoff') { throw 'Unexpected passkey handoff receipt.' }
    $receipt | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath (Join-Path $run 'private/passkey-candidate.json') -Encoding utf8
    $device = Fixture 'verify-login' $Attempt
    if (!$device.sameSubject -or !$device.deviceUseAdvanced -or !$device.newAuthenticatedSession -or
        !$device.grantSessionDeviceBound -or !$device.singleUseProofConsumed) { throw 'Passkey use evidence incomplete.' }
    $replay = Fixture 'verify-login' $Attempt -ExpectRejection
    if (!$replay.rejected) { throw 'Consumed proof replay not rejected.' }
    $receipt | Add-Member -NotePropertyName deviceProof -NotePropertyValue $device -Force
    $receipt | Add-Member -NotePropertyName consumedProofReplayRejected -NotePropertyValue $true -Force
    $receipt | Add-Member -NotePropertyName verifiedAt -NotePropertyValue ([DateTimeOffset]::UtcNow.ToString('O')) -Force
    $receipt.passkeyVerified = $true
    $receipt.scope = 'n01e_windows_platform_passkey'
    $receipt.limitations = @('isolated localhost issuer and synthetic identity','system-browser platform passkey; not direct native Credential Manager','no Android/iOS or recovery acceptance','not a production API session')
    $receipt.PSObject.Properties.Remove('accessFingerprint')
    $receipt | Add-Member -NotePropertyName sourceDirty -NotePropertyValue (@(git -C $PSScriptRoot status --porcelain -- ':(top)native/probes').Count -gt 0) -Force
    $receipt | Add-Member -NotePropertyName authentikImage -NotePropertyValue $environment.authImage -Force
    $receipt | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath $ReceiptPath -Encoding utf8
    Write-Output ('PASS N01e platform passkey and single-use proof; receipt: ' + $ReceiptPath)
}
if ($Phase -eq 'Finalize') {
    if (!$HandoffDirectory) { throw 'Finalize requires an existing HandoffDirectory.' }
    $candidate = [IO.Path]::GetFullPath($HandoffDirectory)
    $allowed = [IO.Path]::GetFullPath((Join-Path $run 'passkey-checks')) + [IO.Path]::DirectorySeparatorChar
    if (!$candidate.StartsWith($allowed, [StringComparison]::OrdinalIgnoreCase)) { throw 'Handoff directory must belong to this lab.' }
    $before = Get-Content -LiteralPath (Join-Path $run 'private/passkey-before.json') -Raw | ConvertFrom-Json
    CompleteEvidence (Join-Path $candidate 'evidence/receipt.json') $before.attempt
    return
}
if ($Phase -eq 'PrepareLogin') {
    if (!(Test-Path -LiteralPath (Join-Path $run 'passkey-baseline.json'))) { throw 'Complete PasswordBaseline first.' }
    $before = Fixture 'prepare-login' ([guid]::NewGuid().ToString())
    $before | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath (Join-Path $run 'private/passkey-before.json') -Encoding utf8
    Write-Output 'READY N01e passkey-only login; enrollment browser sessions were revoked.'
    return
}
$check = Join-Path $run ('passkey-checks/' + [guid]::NewGuid().ToString('N'))
$attempt = [guid]::NewGuid().ToString()
$evidence = Join-Path $check 'evidence'
New-Item -ItemType Directory -Path $evidence -Force | Out-Null
$revision = (git -C $PSScriptRoot rev-parse HEAD).Trim()
$dirty = @(git -C $PSScriptRoot status --porcelain -- ':(top)native/probes').Count -gt 0
if ($Phase -eq 'Contract') {
    $policy = Fixture 'contract'
    [ordered]@{passed=$true;scope='n01e_configuration_contract';sourceRevision=$revision;sourceDirty=$dirty;
        hardwarePasskeyVerified=$false;nativeWindowsHandoff=$false;policy=$policy;authentikImage=$environment.authImage} |
        ConvertTo-Json -Depth 6 | Set-Content -LiteralPath (Join-Path $evidence 'receipt.json') -Encoding utf8
    Write-Output ('PASS N01e configuration/negative contract; hardware unverified; receipt: ' + (Join-Path $evidence 'receipt.json'))
    return
}
$root = Split-Path $run -Parent | Split-Path -Parent
$manifest = Join-Path $PSScriptRoot '../identity/Cargo.toml'
$project = Join-Path $PSScriptRoot '../identity-client/IdentityProbe.csproj'
$priorTarget = $env:CARGO_TARGET_DIR
try {
    # The generic identity probe cache is separate from real application builds.
    $env:CARGO_TARGET_DIR = if ($IsWindows) { 'S:/Build/sproyt-authentik/rust-target' } else { Join-Path $root 'rust-target' }
    Checked 'cargo' @('fmt','--manifest-path',$manifest,'--','--check')
    Checked 'cargo' @('clippy','--manifest-path',$manifest,'--locked','--all-targets','--','-D','warnings')
    Checked 'cargo' @('test','--manifest-path',$manifest,'--locked')
    Checked 'cargo' @('build','--manifest-path',$manifest,'--locked')
    Push-Location (Split-Path $project -Parent)
    try { Checked 'dotnet' @('publish',$project,'-c','Release','-p:UseSharedCompilation=false','--artifacts-path',(Join-Path $check 'managed'),'-o',(Join-Path $check 'client'),'--nologo') }
    finally { Pop-Location }
    $environment | Add-Member -NotePropertyName sourceRevision -NotePropertyValue $revision -Force
    $environment | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath $environmentPath -Encoding utf8
    if ($Phase -eq 'PasskeyLogin') {
        # Invalidate enrollment SSO immediately before launch, after all builds.
        # Exact grant/session/device evidence, rather than prompt=login, proves
        # freshness. The marker can authorize only this one hardware receipt.
        $before = Fixture 'prepare-login' $attempt
        $before | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath (Join-Path $run 'private/passkey-before.json') -Encoding utf8
    }
    $mode = if ($Phase -eq 'PasswordBaseline') { '--authentik-password-browser' } else { '--authentik-passkey-browser' }
    Checked 'dotnet' @((Join-Path $check 'client/IdentityProbe.dll'),$mode,(Join-Path $env:CARGO_TARGET_DIR 'debug/sproyt-identity-probe.exe'),$run,$evidence)
    $receiptPath = Join-Path $evidence 'receipt.json'
    $receipt = Get-Content -LiteralPath $receiptPath -Raw | ConvertFrom-Json
    if (!$receipt.passed -or !$receipt.nativeWindowsHandoff -or $receipt.passkeyVerified) { throw 'Unexpected native handoff receipt.' }
    if ($Phase -eq 'PasskeyLogin') {
        CompleteEvidence $receiptPath $attempt
        return
    }
    $receipt | Add-Member -NotePropertyName sourceDirty -NotePropertyValue $dirty -Force
    $receipt | Add-Member -NotePropertyName authentikImage -NotePropertyValue $environment.authImage -Force
    $receipt | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath $receiptPath -Encoding utf8
    Write-Output ('PASS N01e ' + $Phase + '; receipt: ' + $receiptPath)
} finally { $env:CARGO_TARGET_DIR = $priorTarget }
