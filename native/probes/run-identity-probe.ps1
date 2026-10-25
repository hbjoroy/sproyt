param(
    [string]$ArtifactsDirectory = $(if ($env:RUNNER_TEMP) { Join-Path $env:RUNNER_TEMP 'sproyt-native-identity' } else { Join-Path ([IO.Path]::GetTempPath()) 'sproyt-native-identity' })
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$root = [IO.Path]::GetFullPath($ArtifactsDirectory)
$run = Join-Path $root ('runs/' + [guid]::NewGuid().ToString('N'))
$manifest = Join-Path $PSScriptRoot 'identity/Cargo.toml'
$projectDirectory = Join-Path $PSScriptRoot 'identity-client'
$project = Join-Path $projectDirectory 'IdentityProbe.csproj'
$publish = Join-Path $run 'client'
$priorTarget = $env:CARGO_TARGET_DIR
function Checked([string]$Executable, [string[]]$Arguments) {
    & $Executable @Arguments
    if ($LASTEXITCODE -ne 0) { throw ('Identity probe command failed: ' + $Executable) }
}
try {
    New-Item -ItemType Directory -Path $run -Force | Out-Null
    $env:CARGO_TARGET_DIR = Join-Path $root 'rust-target'
    Checked 'cargo' @('fmt', '--manifest-path', $manifest, '--', '--check')
    Checked 'cargo' @('clippy', '--manifest-path', $manifest, '--locked', '--all-targets', '--', '-D', 'warnings')
    Checked 'cargo' @('test', '--manifest-path', $manifest, '--locked')
    Checked 'cargo' @('build', '--manifest-path', $manifest, '--locked')
    Push-Location $projectDirectory
    try {
        Checked 'dotnet' @('publish', $project, '-c', 'Release', '--artifacts-path', (Join-Path $root 'managed'), '-o', $publish, '--nologo')
    } finally { Pop-Location }
    $serverName = if ($IsWindows) { 'sproyt-identity-probe.exe' } else { 'sproyt-identity-probe' }
    $server = Join-Path $env:CARGO_TARGET_DIR ('debug/' + $serverName)
    $checks = Join-Path $run 'checks.json'
    Checked 'dotnet' @((Join-Path $publish 'IdentityProbe.dll'), $server, (Join-Path $run 'fixture.sqlite'), $checks)
    $evidence = Get-Content -LiteralPath $checks -Raw | ConvertFrom-Json
    if (!$evidence.passed) { throw 'Identity probe evidence failed' }
    $receipt = [ordered]@{
        schema = 1
        scope = 'n01_isolated_identity_http_lab'
        sourceRevision = (git -C $PSScriptRoot rev-parse HEAD).Trim()
        sourceDirty = @(git -C $PSScriptRoot status --porcelain -- ':(top)native/probes' ':(top)crates/sproyt-protocol' ':(top)src/auth.rs' ':(top).github/workflows/native-probe.yml').Count -gt 0
        architecture = [Runtime.InteropServices.RuntimeInformation]::ProcessArchitecture.ToString().ToLowerInvariant()
        platform = [Runtime.InteropServices.RuntimeInformation]::RuntimeIdentifier
        sdk = '10.0.301'
        cargoFmt = $true
        cargoClippy = $true
        cargoTests = $true
        evidence = $evidence
    }
    $receipt | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath (Join-Path $run 'receipt.json') -Encoding utf8
    Write-Output ('PASS identity lab; receipt: ' + (Join-Path $run 'receipt.json'))
} finally { $env:CARGO_TARGET_DIR = $priorTarget }

