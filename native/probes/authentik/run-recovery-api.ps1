param([Parameter(Mandatory)][string]$RunDirectory)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$run = [IO.Path]::GetFullPath($RunDirectory)
$environment = Get-Content -LiteralPath (Join-Path $run 'environment.json') -Raw | ConvertFrom-Json
if ($environment.environment -ne 'sproyt-n01c-local' -or $environment.status -ne 'ready' -or
    !$environment.recoveryLab -or !$environment.passkeyLab -or $environment.origin -ne 'http://localhost:19090' -or
    $environment.prefix -notmatch '^sproyt-n01c-[a-f0-9]{8}$') { throw 'Not a ready isolated recovery lab.' }
$cli = if ($IsWindows) { 'wslc' } else { 'docker' }
$check = Join-Path $run ('recovery-api-checks/' + [guid]::NewGuid().ToString('N'))
$evidence = Join-Path $check 'evidence'
New-Item -ItemType Directory -Path $evidence -Force | Out-Null
$raw = @(& $cli exec ($environment.prefix + '-worker') ak shell -c "exec(compile(open('/lab-code/recovery-api-contract.py').read(), '/lab-code/recovery-api-contract.py', 'exec'))" 2>&1)
$exitResult = $LASTEXITCODE
$line = @($raw | Where-Object { $_ -is [string] -and $_.StartsWith('N01F2_RESULT ') })
if ($exitResult -ne 0 -or $line.Count -ne 1) {
    $safe = @($raw | Where-Object { $_ -is [string] -and $_.StartsWith('N01F2_FAILED ') })
    throw ('Recovery API observation failed; credential/link output suppressed. ' + ($safe -join ' '))
}
$receipt = $line[0].Substring('N01F2_RESULT '.Length) | ConvertFrom-Json
if (!$receipt.passed -or $receipt.scope -ne 'n01f2_public_api_restore_observation' -or
    $receipt.publicRecoveryApiApproved -ne $false -or $receipt.fullRecoveryVerified -ne $false -or
    $receipt.hardwarePasskeyVerified -ne $false -or $receipt.concurrentRestoreAdmissions -ne 2 -or
    $receipt.temporaryStateRestored -ne $true) {
    throw 'Unexpected recovery API observation scope.'
}
$receipt | Add-Member -NotePropertyName sourceRevision -NotePropertyValue ((git -C $PSScriptRoot rev-parse HEAD).Trim()) -Force
$receipt | Add-Member -NotePropertyName sourceDirty -NotePropertyValue (@(git -C $PSScriptRoot status --porcelain -- ':(top)native/probes').Count -gt 0) -Force
$receipt | Add-Member -NotePropertyName authentikImage -NotePropertyValue $environment.authImage -Force
$receipt | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $evidence 'receipt.json') -Encoding utf8
Write-Output ('PASS N01f2 public API observations; recovery gate BLOCKED; receipt: ' + (Join-Path $evidence 'receipt.json'))
