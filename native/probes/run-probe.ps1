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
$child = $null

function Invoke-Checked([string]$Executable, [string[]]$CommandArguments) {
    & $Executable @CommandArguments
    if ($LASTEXITCODE -ne 0) { throw ('Command failed: ' + $Executable) }
}

function Stop-ProbeChild($ChildProcess) {
    try {
        if (!$ChildProcess.HasExited) { $ChildProcess.Kill($true) }
        if (!$ChildProcess.WaitForExit(10000)) { throw 'Probe child did not exit after termination' }
    } finally { $ChildProcess.Dispose() }
}

function Start-MarkedChild([string]$Phase, [string]$DatabasePath, [string]$Site = '') {
    $start = [Diagnostics.ProcessStartInfo]::new()
    $start.FileName = (Get-Command dotnet -ErrorAction Stop).Source
    $start.UseShellExecute = $false
    $start.CreateNoWindow = $true
    $start.WindowStyle = [Diagnostics.ProcessWindowStyle]::Hidden
    $start.RedirectStandardOutput = $true
    $start.RedirectStandardError = $true
    foreach ($argument in @($assembly, $Phase, $DatabasePath)) { $start.ArgumentList.Add($argument) }
    if ($Site) { $start.ArgumentList.Add($Site) }
    $process = [Diagnostics.Process]::Start($start)
    $clock = [Diagnostics.Stopwatch]::StartNew()
    $payloadHash = $null
    try {
        while ($clock.ElapsedMilliseconds -lt 30000) {
            $line = $process.StandardOutput.ReadLineAsync()
            if (!$line.Wait([int][Math]::Max(1, 30000 - $clock.ElapsedMilliseconds)) -or $null -eq $line.Result) {
                throw 'Probe child stopped or timed out before its expected marker'
            }
            $value = $line.Result
            if ($value.StartsWith('PROBE_HASH ')) {
                $payloadHash = $value.Substring(11)
                if ($payloadHash -notmatch '^[0-9a-f]{64}$') { throw 'Invalid pre-termination hash marker' }
            }
            if ($Phase -eq 'seed' -and $value.StartsWith('PASS seed:')) {
                if (!$payloadHash) { throw 'Seed did not emit its pre-termination payload hash' }
                Write-Host $value
                return [pscustomobject]@{ Process = $process; Hash = $payloadHash }
            }
            if ($Phase -eq 'fault_seed' -and $value.StartsWith('PROBE_FAULT ')) {
                $marker = $value.Substring(12) | ConvertFrom-Json
                if ($marker.site -ne $Site -or !$marker.payload) { throw 'Fault marker does not match requested site' }
                $payloadHash = [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData([Text.Encoding]::UTF8.GetBytes($marker.payload))).ToLowerInvariant()
                Write-Host ('Reached diagnostic site: ' + $Site)
                return [pscustomobject]@{ Process = $process; Hash = $payloadHash }
            }
        }
        throw 'Probe marker timeout'
    } catch {
        Stop-ProbeChild $process
        throw
    }
}

try {
    $env:CARGO_TARGET_DIR = $targetDirectory
    Invoke-Checked 'cargo' @('fmt','--manifest-path',$manifest,'--check')
    Invoke-Checked 'cargo' @('clippy','--manifest-path',$manifest,'--all-targets','--all-features','--locked','--','-D','warnings')
    Invoke-Checked 'cargo' @('test','--manifest-path',$manifest,'--locked')
    Invoke-Checked 'cargo' @('test','--manifest-path',$manifest,'--features','fault-injection','--locked')
    Invoke-Checked 'cargo' @('build','--manifest-path',$manifest,'--locked')
    Push-Location (Join-Path $probeRoot 'managed')
    try { Invoke-Checked 'dotnet' @('build',$managedProject,'--artifacts-path',$managedArtifacts,'--nologo') }
    finally { Pop-Location }
    $libraryName = if ($IsWindows) { 'sproyt_native_probe.dll' } elseif ($IsMacOS) { 'libsproyt_native_probe.dylib' } else { 'libsproyt_native_probe.so' }
    $defaultLibrary = Join-Path $artifactRoot ('libraries/default/' + $libraryName)
    $diagnosticLibrary = Join-Path $artifactRoot ('libraries/diagnostic/' + $libraryName)
    New-Item -ItemType Directory -Path (Split-Path $defaultLibrary), (Split-Path $diagnosticLibrary) -Force | Out-Null
    Copy-Item -LiteralPath (Join-Path $targetDirectory ('debug/' + $libraryName)) -Destination $defaultLibrary
    $env:SPROYT_NATIVE_PROBE_LIBRARY = $defaultLibrary
    $assembly = Join-Path $managedArtifacts 'bin/ManagedProbe/debug/Sproyt.NativeProbe.dll'
    if (!(Test-Path -LiteralPath $assembly) -or !(Test-Path -LiteralPath $env:SPROYT_NATIVE_PROBE_LIBRARY)) { throw 'Missing native or managed build output' }

    Invoke-Checked 'dotnet' @($assembly, 'guard', $database)
    $child = Start-MarkedChild 'seed' $database
    $preTerminationHash = $child.Hash
    Invoke-Checked 'dotnet' @($assembly, 'busy', $database)
    Stop-ProbeChild $child.Process
    $child = $null
    Invoke-Checked 'dotnet' @($assembly, 'resume', $database, $preTerminationHash)
    Invoke-Checked 'dotnet' @($assembly, 'verify', $database)
    Invoke-Checked 'dotnet' @($assembly, 'worker', (Join-Path $fixtureDirectory 'worker.sqlite'))

    # Diagnostic hooks are opt-in and copied to a different library path. A
    # separate child loads it; no loaded library is overwritten or unloaded.
    Invoke-Checked 'cargo' @('build','--manifest-path',$manifest,'--features','fault-injection','--locked')
    Copy-Item -LiteralPath (Join-Path $targetDirectory ('debug/' + $libraryName)) -Destination $diagnosticLibrary
    $env:SPROYT_NATIVE_PROBE_LIBRARY = $diagnosticLibrary
    $crashMatrix = @()
    foreach ($site in @('after_insert','before_commit','after_commit')) {
        $faultDatabase = Join-Path $fixtureDirectory ($site + '.sqlite')
        $child = Start-MarkedChild 'fault_seed' $faultDatabase $site
        $hash = $child.Hash
        Stop-ProbeChild $child.Process
        $child = $null
        Invoke-Checked 'dotnet' @($assembly, 'fault_verify', $faultDatabase, $site, $hash)
        $crashMatrix += [ordered]@{ site = $site; payloadSha256 = $hash; recovered = $true }
    }
    $receipt = [ordered]@{
        schema = 2
        sourceRevision = (git -C $probeRoot rev-parse HEAD).Trim()
        sourceDirty = @(git -C $probeRoot status --porcelain -- ':(top)native/probes' ':(top)crates/sproyt-client-core').Count -gt 0
        platform = [Runtime.InteropServices.RuntimeInformation]::RuntimeIdentifier
        architecture = [Runtime.InteropServices.RuntimeInformation]::ProcessArchitecture.ToString()
        abi = 1
        outcomes = @('sqlite persistence','exclusive owner','abrupt process termination','same-ID exact-payload replay','duplicate acknowledgement','incoming edit','account fence','logout generation fence','parent-retained pre-termination hash','transaction crash matrix','bounded worker lifecycle')
        database = $database
        nativeLibrary = $defaultLibrary
        nativeLibrarySha256 = (Get-FileHash -LiteralPath $defaultLibrary -Algorithm SHA256).Hash.ToLowerInvariant()
        diagnosticLibrarySha256 = (Get-FileHash -LiteralPath $diagnosticLibrary -Algorithm SHA256).Hash.ToLowerInvariant()
        preTerminationPayloadSha256 = $preTerminationHash
        crashMatrix = $crashMatrix
        notProven = @('WinUI','Android','iOS','native authentication','production sync','power-loss durability','performance or capacity')
    }
    $receiptPath = Join-Path $fixtureDirectory 'receipt.json'
    $receipt | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath $receiptPath -Encoding utf8
    Write-Output ('PASS native probe; receipt: ' + $receiptPath)
}
finally {
    if ($child) { Stop-ProbeChild $child.Process }
    $env:CARGO_TARGET_DIR = $priorTarget
    $env:SPROYT_NATIVE_PROBE_LIBRARY = $priorLibrary
}
