param(
    [Parameter(Mandatory)][ValidateSet('kotlin', 'swift')][string]$Language,
    [string]$ArtifactsDirectory = (Join-Path ([IO.Path]::GetTempPath()) 'sproyt-native-language-probe')
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$probeRoot = $PSScriptRoot
$artifactRoot = [IO.Path]::GetFullPath((Join-Path $ArtifactsDirectory $Language))
$targetDirectory = Join-Path $artifactRoot 'target'
$fixtureDirectory = Join-Path $artifactRoot ('fixtures/' + [guid]::NewGuid().ToString('N'))
$fixture = Join-Path $probeRoot 'scenarios/persistence-v1.json'
$database = Join-Path $fixtureDirectory 'fixture-ø-🙂.sqlite'
$priorTarget = $env:CARGO_TARGET_DIR
$priorLibrary = $env:SPROYT_NATIVE_PROBE_LIBRARY
$seed = $null

function Invoke-Checked([string]$Executable, [string[]]$Arguments) {
    & $Executable @Arguments
    if ($LASTEXITCODE -ne 0) { throw ('Command failed: ' + $Executable) }
}

function Stop-Seed($ChildProcess) {
    try {
        if (!$ChildProcess.HasExited) { $ChildProcess.Kill($true) }
        if (!$ChildProcess.WaitForExit(10000)) { throw 'Language probe child did not exit after termination' }
    } finally { $ChildProcess.Dispose() }
}

function Start-Seed {
    $start = [Diagnostics.ProcessStartInfo]::new()
    $start.FileName = $executable
    $start.UseShellExecute = $false
    $start.CreateNoWindow = $true
    $start.WindowStyle = [Diagnostics.ProcessWindowStyle]::Hidden
    $start.RedirectStandardOutput = $true
    $start.RedirectStandardError = $true
    foreach ($argument in @($prefix) + @('seed', $database, $fixture)) { $start.ArgumentList.Add($argument) }
    $process = [Diagnostics.Process]::Start($start)
    $clock = [Diagnostics.Stopwatch]::StartNew()
    $payloadHash = $null
    try {
        while ($clock.ElapsedMilliseconds -lt 30000) {
            $line = $process.StandardOutput.ReadLineAsync()
            if (!$line.Wait([int][Math]::Max(1, 30000 - $clock.ElapsedMilliseconds)) -or $null -eq $line.Result) {
                throw 'Language seed stopped or timed out before its expected marker'
            }
            $value = $line.Result
            if ($value.StartsWith('PROBE_HASH ')) {
                $payloadHash = $value.Substring(11)
                if ($payloadHash -notmatch '^[0-9a-f]{64}$') { throw 'Invalid pre-termination hash' }
            }
            if ($value.StartsWith('PASS seed:')) {
                if (!$payloadHash) { throw 'Missing parent-retained hash before process termination' }
                Write-Host $value
                return [pscustomobject]@{ Process = $process; Hash = $payloadHash }
            }
        }
        throw 'Language seed marker timeout'
    } catch {
        Stop-Seed $process
        throw
    }
}

try {
    # Tool discovery is explicit. This runner never installs an SDK locally.
    if ($Language -eq 'kotlin') {
        $executable = (Get-Command java -ErrorAction Stop).Source
        $maven = (Get-Command mvn -ErrorAction Stop).Source
    } else {
        if (!$IsMacOS) { throw 'The Swift probe currently requires the hosted macOS runner or a local Mac.' }
        $executable = (Get-Command swift -ErrorAction Stop).Source
    }
    New-Item -ItemType Directory -Path $fixtureDirectory -Force | Out-Null
    $env:CARGO_TARGET_DIR = $targetDirectory
    $buildArguments = @('build', '--manifest-path', (Join-Path $probeRoot 'client-core/Cargo.toml'), '--locked')
    if ($Language -eq 'kotlin') { $buildArguments += @('--features', 'jni') }
    Invoke-Checked 'cargo' $buildArguments
    $libraryName = if ($IsWindows) { 'sproyt_native_probe.dll' } elseif ($IsMacOS) { 'libsproyt_native_probe.dylib' } else { 'libsproyt_native_probe.so' }
    $library = Join-Path $targetDirectory ('debug/' + $libraryName)
    if (!(Test-Path -LiteralPath $library -PathType Leaf)) { throw 'Missing built native library' }
    $env:SPROYT_NATIVE_PROBE_LIBRARY = $library
    $versions = [ordered]@{}
    if ($Language -eq 'kotlin') {
        $managedBuild = Join-Path $artifactRoot 'managed'
        $classpathFile = Join-Path $artifactRoot 'classpath.txt'
        Invoke-Checked $maven @('-B', '-q', '-f', (Join-Path $probeRoot 'kotlin/pom.xml'),
            ('-Dprobe.build.directory=' + $managedBuild), 'compile', 'dependency:build-classpath',
            ('-Dmdep.outputFile=' + $classpathFile), '-DincludeScope=runtime')
        $classpath = (Join-Path $managedBuild 'classes') + [IO.Path]::PathSeparator + [IO.File]::ReadAllText($classpathFile).Trim()
        $prefix = @('-Xcheck:jni', '-cp', $classpath, 'org.sproyt.probe.MainKt')
        $versions.runtime = @(& $executable '-version' 2>&1 | ForEach-Object { "$_" })
        if ($LASTEXITCODE -ne 0) { throw 'Java version check failed' }
        $versions.compiler = 'Kotlin 2.3.21; JVM target 17'
        $versions.build = @(& $maven '--version' 2>&1 | ForEach-Object { "$_" })
        if ($LASTEXITCODE -ne 0) { throw 'Maven version check failed' }
    } else {
        $swift = $executable
        $swiftArguments = @('build', '--package-path', (Join-Path $probeRoot 'swift'), '--scratch-path', (Join-Path $artifactRoot 'swift-build'),
            '-Xcc', ('-I' + (Join-Path $probeRoot 'client-core/include')),
            '-Xlinker', ('-L' + (Split-Path $library)), '-Xlinker', '-rpath', '-Xlinker', (Split-Path $library))
        Invoke-Checked $swift $swiftArguments
        $binPath = (& $swift @swiftArguments '--show-bin-path' | Select-Object -Last 1).Trim()
        if ($LASTEXITCODE -ne 0) { throw 'Swift executable path lookup failed' }
        $executable = Join-Path $binPath 'swift-probe'
        if (!(Test-Path -LiteralPath $executable -PathType Leaf)) { throw 'Missing Swift executable' }
        $prefix = @()
        $versions.compiler = @(& $swift '--version' 2>&1 | ForEach-Object { "$_" })
        if ($LASTEXITCODE -ne 0) { throw 'Swift version check failed' }
    }

    Invoke-Checked $executable (@($prefix) + @('guard', (Join-Path $fixtureDirectory 'guard.sqlite'), $fixture))
    $seed = Start-Seed
    $preTerminationHash = $seed.Hash
    Invoke-Checked $executable (@($prefix) + @('busy', $database, $fixture))
    Stop-Seed $seed.Process
    $seed = $null
    Invoke-Checked $executable (@($prefix) + @('resume', $database, $fixture, $preTerminationHash))
    Invoke-Checked $executable (@($prefix) + @('verify', $database, $fixture))
    Invoke-Checked $executable (@($prefix) + @('worker', (Join-Path $fixtureDirectory 'worker.sqlite'), $fixture))
    $receipt = [ordered]@{
        schema = 1
        scope = 'desktop_language_binding_feasibility'
        language = $Language
        sourceRevision = (git -C $probeRoot rev-parse HEAD).Trim()
        sourceDirty = @(git -C $probeRoot status --porcelain -- ':(top)native/probes' ':(top)crates/sproyt-client-core').Count -gt 0
        platform = [Runtime.InteropServices.RuntimeInformation]::RuntimeIdentifier
        architecture = [Runtime.InteropServices.RuntimeInformation]::ProcessArchitecture.ToString()
        abi = 1
        diagnosticHooks = $false
        nativeLibrary = $library
        nativeLibrarySha256 = (Get-FileHash -LiteralPath $library -Algorithm SHA256).Hash.ToLowerInvariant()
        preTerminationPayloadSha256 = $preTerminationHash
        toolVersions = $versions
        outcomes = @('ABI contract', 'shared Unicode persistence', 'exclusive owner', 'abrupt process termination',
            'parent-retained payload hash', 'same-ID replay', 'duplicate acknowledgement', 'incoming edit',
            'account fence', 'logout generation fence', 'bounded worker lifecycle', 'input guards without mutation')
        notProven = @('Android packaging or ART', 'iOS packaging', 'mobile device lifecycle', 'SwiftUI or Compose',
            'native authentication', 'production sync', 'generated binding choice', 'performance or capacity')
    }
    $receiptPath = Join-Path $fixtureDirectory 'receipt.json'
    $receipt | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath $receiptPath -Encoding utf8
    Write-Output ('PASS ' + $Language + ' desktop binding probe; receipt: ' + $receiptPath)
} finally {
    if ($seed) { Stop-Seed $seed.Process }
    $env:CARGO_TARGET_DIR = $priorTarget
    $env:SPROYT_NATIVE_PROBE_LIBRARY = $priorLibrary
}
