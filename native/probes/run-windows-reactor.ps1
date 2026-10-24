param(
    [ValidateSet('arm64', 'x64')][string]$Architecture = [Runtime.InteropServices.RuntimeInformation]::ProcessArchitecture.ToString().ToLowerInvariant(),
    [string]$ArtifactsDirectory = (Join-Path ([IO.Path]::GetPathRoot([Environment]::SystemDirectory)) 'sproyt-build/reactor'),
    [switch]$UiSmoke,
    [switch]$Launch
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
if (!$IsWindows) { throw 'This probe requires Windows.' }
if ($Architecture -ne [Runtime.InteropServices.RuntimeInformation]::ProcessArchitecture.ToString().ToLowerInvariant()) {
    throw 'Use a matching native build host; emulation is not counted as native platform evidence.'
}
$root = [IO.Path]::GetFullPath($ArtifactsDirectory)
$run = Join-Path $root ('runs/' + [guid]::NewGuid().ToString('N'))
$publish = Join-Path $run ('publish-win-' + $Architecture)
$project = Join-Path $PSScriptRoot 'windows-reactor/Sproyt.ReactorProbe.csproj'
$manifest = Join-Path $PSScriptRoot 'client-core/Cargo.toml'
$priorTarget = $env:CARGO_TARGET_DIR
$platform = if ($Architecture -eq 'arm64') { 'ARM64' } else { 'x64' }
$runtime = 'win-' + $Architecture

function Checked([string]$Executable, [string[]]$Arguments) {
    & $Executable @Arguments
    if ($LASTEXITCODE -ne 0) { throw ('Command failed: ' + $Executable) }
}

function ProbeProcess([string[]]$Arguments, [bool]$Visible) {
    $start = [Diagnostics.ProcessStartInfo]::new()
    $start.FileName = Join-Path $publish 'Sproyt.ReactorProbe.exe'
    $start.WorkingDirectory = $publish
    $start.UseShellExecute = $false
    $start.CreateNoWindow = !$Visible
    $start.WindowStyle = if ($Visible) { [Diagnostics.ProcessWindowStyle]::Normal } else { [Diagnostics.ProcessWindowStyle]::Hidden }
    foreach ($argument in $Arguments) { $start.ArgumentList.Add($argument) }
    $process = [Diagnostics.Process]::Start($start)
    try {
        if (!$process.WaitForExit(60000)) { $process.Kill($true); [void]$process.WaitForExit(10000); throw 'Windows probe timed out' }
        if ($process.ExitCode -ne 0) { throw ('Windows probe failed: ' + $process.ExitCode + '; inspect ' + $run) }
    } finally { $process.Dispose() }
}

try {
    New-Item -ItemType Directory -Path $run -Force | Out-Null
    $env:CARGO_TARGET_DIR = Join-Path $root 'rust-target'
    Checked 'cargo' @('build', '--manifest-path', $manifest, '--locked')
    # SDK resolution follows the working directory, not the project argument.
    Push-Location (Split-Path -Parent $project)
    try {
        Checked 'dotnet' @('publish', $project, '-c', 'Release', '-r', $runtime, ('-p:Platform=' + $platform),
            '-p:RestoreLockedMode=true', '--artifacts-path', (Join-Path $root 'managed'), '-o', $publish, '--nologo')
    } finally { Pop-Location }
    $library = Join-Path $publish 'sproyt_native_probe.dll'
    Copy-Item -LiteralPath (Join-Path $env:CARGO_TARGET_DIR 'debug/sproyt_native_probe.dll') -Destination $library
    if (!(Test-Path -LiteralPath (Join-Path $publish 'Sproyt.ReactorProbe.pri'))) { throw 'Published application PRI is missing' }

    $coreTests = Join-Path $run 'core-tests'
    ProbeProcess @('--self-test', $coreTests) $false
    if (!(Test-Path -LiteralPath (Join-Path $coreTests 'core-result.txt'))) { throw 'Core test evidence missing' }
    Get-Content -LiteralPath (Join-Path $coreTests 'core-result.txt')
    $ui = $null
    if ($UiSmoke) {
        $uiTests = Join-Path $run 'ui-tests'
        ProbeProcess @('--ui-smoke', $uiTests) $true
        $ui = Get-Content -LiteralPath (Join-Path $uiTests 'ui-result.json') -Raw | ConvertFrom-Json
        if (!$ui.passed -or $ui.architecture.ToLowerInvariant() -ne $Architecture) { throw 'Native UI evidence failed' }
        if ((Get-Content -LiteralPath (Join-Path $uiTests 'ui-closed.txt') -Raw).Trim() -ne 'PASS: native Closed event after shutdown') {
            throw 'The native window did not close after shutdown'
        }
    }
    $receipt = [ordered]@{
        schema = 1
        scope = 'windows_reactor_local_feasibility'
        sourceRevision = (git -C $PSScriptRoot rev-parse HEAD).Trim()
        sourceDirty = @(git -C $PSScriptRoot status --porcelain -- ':(top)native/probes' ':(top)crates/sproyt-client-core').Count -gt 0
        architecture = $Architecture
        platform = [Runtime.InteropServices.RuntimeInformation]::RuntimeIdentifier
        reactor = '0.1.0-preview.16'
        compiler = 'Microsoft.Net.Compilers.Toolset 5.9.0'
        runtime = $runtime
        nativeLibrarySha256 = (Get-FileHash -LiteralPath $library -Algorithm SHA256).Hash.ToLowerInvariant()
        executableSha256 = (Get-FileHash -LiteralPath (Join-Path $publish 'Sproyt.ReactorProbe.exe') -Algorithm SHA256).Hash.ToLowerInvariant()
        applicationPriSha256 = (Get-FileHash -LiteralPath (Join-Path $publish 'Sproyt.ReactorProbe.pri') -Algorithm SHA256).Hash.ToLowerInvariant()
        coreTests = $true
        uiSmoke = $ui
        publishDirectory = $publish
        notTested = @('physical keyboard and IME', 'touch', 'screen reader', 'visual screenshot approval', 'production sync', 'authentication',
            'hard process termination during equal-body newer-draft repair after an unknown enqueue')
    }
    $receipt | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath (Join-Path $run 'receipt.json') -Encoding utf8
    Write-Output ('PASS Windows Reactor probe; package: ' + $publish)
    Write-Output ('Receipt: ' + (Join-Path $run 'receipt.json'))
    if ($Launch) {
        # This is the interactive preview requested by the user, not a helper.
        Start-Process -FilePath (Join-Path $publish 'Sproyt.ReactorProbe.exe') -WorkingDirectory $publish -WindowStyle Normal
    }
} finally { $env:CARGO_TARGET_DIR = $priorTarget }
