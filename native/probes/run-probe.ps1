param(
    [string]$ArtifactsDirectory = (Join-Path ([IO.Path]::GetTempPath()) 'sproyt-native-probe')
)
$ErrorActionPreference = 'Stop'
$probeRoot = $PSScriptRoot
$manifest = Join-Path $probeRoot 'client-core/Cargo.toml'
$managedProject = Join-Path $probeRoot 'managed/ManagedProbe.csproj'
$artifactRoot = [IO.Path]::GetFullPath($ArtifactsDirectory)
$targetDirectory = Join-Path $artifactRoot 'target'
$managedArtifacts = Join-Path $artifactRoot 'managed'
$fixtureDirectory = Join-Path $artifactRoot ('fixtures/' + [guid]::NewGuid().ToString('N'))
# Never reuse or overwrite somebody's database. No recursive cleanup is needed.
New-Item -ItemType Directory -Path $fixtureDirectory -Force | Out-Null
$database = Join-Path $fixtureDirectory 'fixture-æøå.sqlite'
$priorTarget = $env:CARGO_TARGET_DIR
$priorLibrary = $env:SPROYT_NATIVE_PROBE_LIBRARY
$seed = $null

function Invoke-Checked([string]$Executable, [string[]]$CommandArguments) {
    & $Executable @CommandArguments
    if ($LASTEXITCODE -ne 0) { throw ('Command failed: ' + $Executable) }
}

try {
    $env:CARGO_TARGET_DIR = $targetDirectory
    Invoke-Checked 'cargo' @('fmt','--manifest-path',$manifest,'--check')
    Invoke-Checked 'cargo' @('clippy','--manifest-path',$manifest,'--all-targets','--locked','--','-D','warnings')
    Invoke-Checked 'cargo' @('test','--manifest-path',$manifest,'--locked')
    Invoke-Checked 'cargo' @('build','--manifest-path',$manifest,'--locked')
    Push-Location (Join-Path $probeRoot 'managed')
    try { Invoke-Checked 'dotnet' @('build',$managedProject,'--artifacts-path',$managedArtifacts,'--nologo') }
    finally { Pop-Location }
    $libraryName = if ($IsWindows) { 'sproyt_native_probe.dll' } elseif ($IsMacOS) { 'libsproyt_native_probe.dylib' } else { 'libsproyt_native_probe.so' }
    $env:SPROYT_NATIVE_PROBE_LIBRARY = Join-Path $targetDirectory ('debug/' + $libraryName)
    $assembly = Join-Path $managedArtifacts 'bin/ManagedProbe/debug/Sproyt.NativeProbe.dll'
    if (!(Test-Path -LiteralPath $assembly) -or !(Test-Path -LiteralPath $env:SPROYT_NATIVE_PROBE_LIBRARY)) { throw 'Missing native or managed build output' }

    $start = [Diagnostics.ProcessStartInfo]::new()
    $start.FileName = (Get-Command dotnet -ErrorAction Stop).Source
    $start.UseShellExecute = $false
    $start.CreateNoWindow = $true
    $start.WindowStyle = [Diagnostics.ProcessWindowStyle]::Hidden
    $start.RedirectStandardOutput = $true
    $start.RedirectStandardError = $true
    foreach ($argument in @($assembly, 'seed', $database)) { $start.ArgumentList.Add($argument) }
    $seed = [Diagnostics.Process]::Start($start)
    $line = $seed.StandardOutput.ReadLineAsync()
    if (!$line.Wait(30000) -or !$line.Result -or !$line.Result.StartsWith('PASS seed:')) {
        throw 'Seed process did not confirm its durable dispatch within 30 seconds'
    }
    Write-Output $line.Result
    Invoke-Checked 'dotnet' @($assembly, 'busy', $database)
    $seed.Kill($true)
    if (!$seed.WaitForExit(10000)) { throw 'Terminated seed process did not exit' }
    Invoke-Checked 'dotnet' @($assembly, 'resume', $database)
    Invoke-Checked 'dotnet' @($assembly, 'verify', $database)
    $receipt = [ordered]@{
        schema = 1
        sourceRevision = (git -C $probeRoot rev-parse HEAD).Trim()
        sourceDirty = @(git -C $probeRoot status --porcelain -- ':(top)native/probes' ':(top)crates/sproyt-client-core').Count -gt 0
        platform = [Runtime.InteropServices.RuntimeInformation]::RuntimeIdentifier
        architecture = [Runtime.InteropServices.RuntimeInformation]::ProcessArchitecture.ToString()
        abi = 1
        outcomes = @('sqlite persistence','exclusive owner','abrupt process termination','same-ID exact-payload replay','duplicate acknowledgement','incoming edit','account fence','logout generation fence')
        database = $database
        nativeLibrary = $env:SPROYT_NATIVE_PROBE_LIBRARY
        nativeLibrarySha256 = (Get-FileHash -LiteralPath $env:SPROYT_NATIVE_PROBE_LIBRARY -Algorithm SHA256).Hash.ToLowerInvariant()
        notProven = @('WinUI','Android','iOS','native authentication','production sync','power-loss durability','performance or capacity')
    }
    $receiptPath = Join-Path $fixtureDirectory 'receipt.json'
    $receipt | ConvertTo-Json -Depth 3 | Set-Content -LiteralPath $receiptPath -Encoding utf8
    Write-Output ('PASS native probe; receipt: ' + $receiptPath)
}
finally {
    if ($seed) {
        if (!$seed.HasExited) { $seed.Kill($true); $null = $seed.WaitForExit(10000) }
        $seed.Dispose()
    }
    $env:CARGO_TARGET_DIR = $priorTarget
    $env:SPROYT_NATIVE_PROBE_LIBRARY = $priorLibrary
}
