param([Parameter(Mandatory)][string]$ProcessIds, [Parameter(Mandatory)][string]$Binary, [int]$IntervalMs = 250)
$ErrorActionPreference = 'Stop'
$ids = @($ProcessIds.Split(',') | ForEach-Object { [int]$_ })
if ($ids.Count -lt 1 -or $ids.Count -gt 2 -or ($ids | Where-Object {$_ -le 0})) { throw 'Invalid fixture PIDs' }
$resolvedBinary = (Resolve-Path -LiteralPath $Binary).Path
$processes = @($ids | ForEach-Object { Get-Process -Id $_ })
foreach($process in $processes) { if ($process.Path -ne $resolvedBinary) { throw 'PID is not the requested fixture binary' } }
$available = [long]$processes[0].ProcessorAffinity
$mask = 0L
for($bit = 0; $bit -lt 62; $bit++) { if (($available -band (1L -shl $bit)) -ne 0) { $mask = 1L -shl $bit } }
if ($mask -eq 0) { throw 'No usable CPU affinity bit' }
foreach($process in $processes) { $process.ProcessorAffinity = [IntPtr]$mask }
@{type='ready';affinityMask=$mask;logicalCpuBudget=1;pids=$ids} | ConvertTo-Json -Compress
while ($true) {
  $values = @()
  foreach($id in $ids) {
    $process = Get-Process -Id $id -ErrorAction SilentlyContinue
    if (!$process -or $process.Path -ne $resolvedBinary) { exit 0 }
    $values += @{pid=$id;cpuMs=$process.TotalProcessorTime.TotalMilliseconds;rss=$process.WorkingSet64;privateBytes=$process.PrivateMemorySize64}
  }
  @{type='sample';at=[DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds();processes=$values} | ConvertTo-Json -Compress -Depth 4
  Start-Sleep -Milliseconds $IntervalMs
}