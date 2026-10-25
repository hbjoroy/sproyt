param([Parameter(Mandatory)][string]$RunDirectory)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$run = [IO.Path]::GetFullPath($RunDirectory)
$path = Join-Path $run 'environment.json'
$environment = Get-Content $path -Raw | ConvertFrom-Json
if ($environment.environment -ne 'sproyt-n01c-local' -or $environment.prefix -notmatch '^sproyt-n01c-[a-f0-9]{8}$') { throw 'Not an owned local lab.' }
$cli = if ($IsWindows) { 'wslc' } else { 'docker' }
& $cli stop --help | Out-Null
foreach ($suffix in @('-server','-worker','-db')) {
    $name = $environment.prefix + $suffix
    $format = if ($IsWindows) { 'json' } else { '{{json .}}' }
    $inspected = @(& $cli inspect --format $format $name 2>$null)
    if ($LASTEXITCODE -ne 0) { throw 'Could not verify owned lab container.' }
    # Docker uses a Go template for JSON; WSLC has a literal json format.
    $metadata = $inspected -join "`n" | ConvertFrom-Json
    if (@($metadata).Count -ne 1 -or $metadata.Config.Labels.app -ne 'sproyt-n01c' -or $metadata.Name.TrimStart('/') -ne $name) { throw 'Container ownership mismatch.' }
    & $cli stop --time 10 $name | Out-Null
    if ($LASTEXITCODE -ne 0) { throw 'Stopping owned lab container failed.' }
}
$environment.status='stopped'
$environment | ConvertTo-Json -Depth 4 | Set-Content $path -Encoding utf8
Write-Output 'Stopped only the owned N01c lab containers; evidence and synthetic data retained.'
