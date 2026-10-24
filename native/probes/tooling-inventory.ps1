# Read-only local inventory. Does not install, build, contact services, or inspect credentials.
# Run with: pwsh -NoProfile -File native/probes/tooling-inventory.ps1
[CmdletBinding()]
param()

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Get-Tool {
    param([string]$Name)
    $command = Get-Command $Name -CommandType Application -ErrorAction SilentlyContinue |
        Select-Object -First 1
    if ($null -eq $command) {
        return [ordered]@{ present = $false; path = $null }
    }
    return [ordered]@{ present = $true; path = $command.Source }
}

function Get-VersionOutput {
    param($Tool, [string[]]$Arguments)
    if (-not $Tool.present) { return $null }
    $priorAutoInstall = $env:RUSTUP_AUTO_INSTALL
    try {
        # Rustup proxies must report an absent toolchain, not install it.
        $env:RUSTUP_AUTO_INSTALL = '0'
        $lines = @(& $Tool.path @Arguments 2>&1 | ForEach-Object { "$_" })
        $code = $LASTEXITCODE
        return [ordered]@{ exit_code = $code; lines = $lines }
    } catch {
        # Keep failure reporting bounded; do not dump exception/environment details.
        return [ordered]@{ exit_code = $null; lines = @(); status = 'invocation_failed' }
    } finally {
        $env:RUSTUP_AUTO_INSTALL = $priorAutoInstall
    }
}

$tools = [ordered]@{}
foreach ($name in @('rustc', 'cargo', 'dotnet', 'java', 'javac', 'adb', 'gradle')) {
    $tools[$name] = Get-Tool $name
}

$locations = [ordered]@{}
$candidates = [ordered]@{
    android_program_files = Join-Path ([Environment]::GetFolderPath('ProgramFiles')) 'Android'
    android_local_sdk = Join-Path ([Environment]::GetFolderPath('LocalApplicationData')) 'Android/Sdk'
    java_program_files = Join-Path ([Environment]::GetFolderPath('ProgramFiles')) 'Java'
}
foreach ($key in $candidates.Keys) {
    $locations[$key] = [ordered]@{
        path = $candidates[$key]
        exists = Test-Path -LiteralPath $candidates[$key] -PathType Container
    }
}

$storage = @(
    foreach ($name in @('C', 'S')) {
        $drive = Get-PSDrive -Name $name -PSProvider FileSystem -ErrorAction SilentlyContinue
        if ($null -ne $drive) {
            [ordered]@{ drive = $name; free_bytes = [long]$drive.Free }
        }
    }
)

[ordered]@{
    schema_version = 1
    observed_at_utc = [DateTime]::UtcNow.ToString('o')
    scope = 'local_tool_presence_and_version_only'
    host = [ordered]@{
        os = [Runtime.InteropServices.RuntimeInformation]::OSDescription
        os_architecture = [Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString()
        process_architecture = [Runtime.InteropServices.RuntimeInformation]::ProcessArchitecture.ToString()
        powershell_version = $PSVersionTable.PSVersion.ToString()
    }
    tools = $tools
    versions = [ordered]@{
        rustc = Get-VersionOutput $tools.rustc @('-vV')
        cargo = Get-VersionOutput $tools.cargo @('--version')
        dotnet_sdks = Get-VersionOutput $tools.dotnet @('--list-sdks')
        dotnet_runtimes = Get-VersionOutput $tools.dotnet @('--list-runtimes')
    }
    conventional_locations = $locations
    storage = $storage
    validation = [ordered]@{
        rust_cdylib_build = 'not_tested'
        csharp_pinvoke = 'not_tested'
        sqlite_restart = 'not_tested'
        android_build = 'not_tested'
        apple_build_and_signing = 'not_available_on_this_windows_host'
        winui_build = 'not_tested'
        signing_credentials = 'not_inspected'
        capacity_benchmark = 'not_performed'
    }
} | ConvertTo-Json -Depth 8
