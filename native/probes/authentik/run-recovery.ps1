param([Parameter(Mandatory)][string]$RunDirectory)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$run = [IO.Path]::GetFullPath($RunDirectory)
$environment = Get-Content -LiteralPath (Join-Path $run 'environment.json') -Raw | ConvertFrom-Json
if ($environment.environment -ne 'sproyt-n01c-local' -or $environment.status -ne 'ready' -or
    !$environment.recoveryLab -or !$environment.passkeyLab -or $environment.origin -ne 'http://localhost:19090' -or
    $environment.mailpitOrigin -ne 'http://127.0.0.1:19093' -or
    $environment.prefix -notmatch '^sproyt-n01c-[a-f0-9]{8}$') { throw 'Not a ready isolated N01f recovery lab.' }
$cli = if ($IsWindows) { 'wslc' } else { 'docker' }
$check = Join-Path $run ('recovery-checks/' + [guid]::NewGuid().ToString('N'))
$evidence = Join-Path $check 'evidence'
New-Item -ItemType Directory -Path $evidence -Force | Out-Null
$raw = @(& $cli exec --env N01F_ACTION=contract ($environment.prefix + '-worker') ak shell -c "exec(compile(open('/lab-code/recovery-contract.py').read(), '/lab-code/recovery-contract.py', 'exec'))" 2>&1)
$exitResult = $LASTEXITCODE
$line = @($raw | Where-Object { $_ -is [string] -and $_.StartsWith('N01F_RESULT ') })
if ($exitResult -ne 0 -or $line.Count -ne 1) {
    $safe = @($raw | Where-Object { $_ -is [string] -and $_.StartsWith('N01F_FAILED ') })
    throw ('Recovery contract failed; mail/token output suppressed. ' + ($safe -join ' '))
}
$receipt = $line[0].Substring('N01F_RESULT '.Length) | ConvertFrom-Json
if (!$receipt.passed -or $receipt.hardwarePasskeyVerified -or $receipt.fullRecoveryVerified -or
    $receipt.recoveryGateApproved -ne $false -or !$receipt.outerPolicyFailureAllowsSetup) { throw 'Unexpected recovery observation scope.' }
$receipt | Add-Member -NotePropertyName sourceRevision -NotePropertyValue ((git -C $PSScriptRoot rev-parse HEAD).Trim()) -Force
$receipt | Add-Member -NotePropertyName sourceDirty -NotePropertyValue (@(git -C $PSScriptRoot status --porcelain -- ':(top)native/probes').Count -gt 0) -Force
$receipt | Add-Member -NotePropertyName authentikImage -NotePropertyValue $environment.authImage -Force
$receipt | Add-Member -NotePropertyName mailpitImage -NotePropertyValue $environment.mailpitImage -Force
$receipt | Add-Member -NotePropertyName policy -NotePropertyValue (Get-Content -LiteralPath (Join-Path $run 'recovery-policy.json') -Raw | ConvertFrom-Json) -Force
$receipt | ConvertTo-Json -Depth 7 | Set-Content -LiteralPath (Join-Path $evidence 'receipt.json') -Encoding utf8
Write-Output ('PASS N01f bounded observations; recovery gate BLOCKED; receipt: ' + (Join-Path $evidence 'receipt.json'))
