param(
    [string]$ArtifactsDirectory = $(if ($IsWindows) { 'S:/Build/sproyt-authentik' } else { Join-Path ([IO.Path]::GetTempPath()) 'sproyt-authentik' })
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$containerCli = if ($IsWindows) { 'wslc' } else { 'docker' }
$authImage = 'ghcr.io/goauthentik/server:2026.8.2@sha256:ff8489a5af4f4fe415ffd180a8e3c10b120bc2592d13d79dac050d977f7b9ecd'
$pgImage = 'docker.io/library/postgres:16-alpine@sha256:f4610795587492cd4fb887c2dc3de301b813278bb6f616992061141b5f15ac36'
$runId = [guid]::NewGuid().ToString('N')
$run = Join-Path ([IO.Path]::GetFullPath($ArtifactsDirectory)) ('runs/' + $runId)
$private = Join-Path $run 'private'
$prefix = 'sproyt-n01c-' + $runId.Substring(0,8)
$network = $prefix
$edgeNetwork = $prefix + '-edge'
$database = $prefix + '-db'
$server = $prefix + '-server'
$worker = $prefix + '-worker'
function CheckedContainer([string[]]$Arguments) {
    & $containerCli @Arguments
    if ($LASTEXITCODE -ne 0) { throw ('Container operation failed: ' + $Arguments[0]) }
}
function RandomSecret { [Convert]::ToHexString([Security.Cryptography.RandomNumberGenerator]::GetBytes(32)).ToLowerInvariant() }
foreach ($port in @(19090,19191,19192)) {
    $reservation = [Net.Sockets.TcpListener]::new([Net.IPAddress]::Loopback,$port)
    try { $reservation.Start() } catch { throw ('Lab port occupied: ' + $port) } finally { $reservation.Stop() }
}
New-Item -ItemType Directory -Path $private -Force | Out-Null
if ($IsWindows) {
$directoryAcl = [Security.AccessControl.DirectorySecurity]::new()
$currentSid = [Security.Principal.WindowsIdentity]::GetCurrent().User
$directoryAcl.SetOwner($currentSid)
$directoryAcl.SetAccessRuleProtection($true,$false)
foreach ($sid in @($currentSid,[Security.Principal.SecurityIdentifier]::new('S-1-5-18'))) {
    $directoryAcl.AddAccessRule([Security.AccessControl.FileSystemAccessRule]::new($sid,'FullControl','ContainerInherit,ObjectInherit','None','Allow'))
}
Set-Acl -LiteralPath $private -AclObject $directoryAcl
} else { [IO.File]::SetUnixFileMode($private, [IO.UnixFileMode]::UserRead -bor [IO.UnixFileMode]::UserWrite -bor [IO.UnixFileMode]::UserExecute) }
$dbPassword = RandomSecret
$secretKey = RandomSecret
$adminPassword = RandomSecret
$clientSecret = RandomSecret
$testPassword = RandomSecret
@('POSTGRES_DB=n01c','POSTGRES_USER=n01c',('POSTGRES_PASSWORD=' + $dbPassword)) | Set-Content (Join-Path $private 'postgres.env') -Encoding utf8
@('AUTHENTIK_POSTGRESQL__HOST=postgresql','AUTHENTIK_POSTGRESQL__NAME=n01c','AUTHENTIK_POSTGRESQL__USER=n01c',
    ('AUTHENTIK_POSTGRESQL__PASSWORD=' + $dbPassword),('AUTHENTIK_SECRET_KEY=' + $secretKey),
    ('AUTHENTIK_BOOTSTRAP_PASSWORD=' + $adminPassword),'AUTHENTIK_BOOTSTRAP_EMAIL=admin@example.invalid',
    'AUTHENTIK_LOG_LEVEL=warning','AUTHENTIK_DISABLE_STARTUP_ANALYTICS=true','AUTHENTIK_DISABLE_UPDATE_CHECK=true',
    'AUTHENTIK_ERROR_REPORTING__ENABLED=false','AUTHENTIK_WEB__BASE_URL=http://127.0.0.1:19090') |
    Set-Content (Join-Path $private 'authentik.env') -Encoding utf8
[ordered]@{environment='sproyt-n01c-local';redirect='http://127.0.0.1:19191/probe/callback';client_secret=$clientSecret;test_password=$testPassword} |
    ConvertTo-Json | Set-Content (Join-Path $private 'bootstrap.json') -Encoding utf8
[IO.File]::WriteAllText((Join-Path $private 'client-secret'),$clientSecret,[Text.UTF8Encoding]::new($false))
[ordered]@{issuer='http://127.0.0.1:19090/application/o/n01-lab/';client_id='n01-lab';client_secret_file=(Join-Path $private 'client-secret');listen_port=19191} |
    ConvertTo-Json | Set-Content (Join-Path $private 'broker.json') -Encoding utf8
[ordered]@{schema=1;environment='sproyt-n01c-local';prefix=$prefix;containers=@($server,$worker,$database);network=$network;volume=$database;
    containerCli=$containerCli;edgeNetwork=$edgeNetwork;authImage=$authImage;postgresImage=$pgImage;origin='http://127.0.0.1:19090';runDirectory=$run;status='starting'} |
    ConvertTo-Json -Depth 4 | Set-Content (Join-Path $run 'environment.json') -Encoding utf8
# Read actual help before using WSLC; never assume all Docker syntax transfers.
CheckedContainer @('run','--help') | Out-Null
CheckedContainer @('network','create','--help') | Out-Null
CheckedContainer @('volume','create','--help') | Out-Null
CheckedContainer @('exec','--help') | Out-Null
CheckedContainer @('pull','--quiet',$authImage) | Out-Null
CheckedContainer @('pull','--quiet',$pgImage) | Out-Null
CheckedContainer @('network','create','--internal','--label','app=sproyt-n01c',$network) | Out-Null
CheckedContainer @('network','create','--label','app=sproyt-n01c',$edgeNetwork) | Out-Null
CheckedContainer @('network','connect','--help') | Out-Null
CheckedContainer @('volume','create','--label','app=sproyt-n01c',$database) | Out-Null
CheckedContainer @('run','--detach','--name',$database,'--network',$network,'--network-alias','postgresql','--label','app=sproyt-n01c',
    '--memory','512M','--cpus','1','--env-file',(Join-Path $private 'postgres.env'),'--volume',($database+':/var/lib/postgresql/data'),$pgImage) | Out-Null
$pgReady = $false
for ($attempt=0; $attempt -lt 30; $attempt++) {
    & $containerCli exec $database pg_isready -U n01c -d n01c *> $null
    if ($LASTEXITCODE -eq 0) { $pgReady=$true; break }
    Start-Sleep -Seconds 2
}
if (!$pgReady) { throw 'Isolated PostgreSQL did not become ready.' }
$codeMount = $PSScriptRoot.Replace('\','/') + ':/lab-code:ro'
$privateMount = $private.Replace('\','/') + ':/lab:ro'
foreach ($kind in @('server','worker')) {
    $containerName = if ($kind -eq 'server') { $server } else { $worker }
    $runArguments = @('run','--detach','--name',$containerName,'--network',$network,'--label','app=sproyt-n01c',
        '--memory','1536M','--cpus','2','--shm-size','512M','--env-file',(Join-Path $private 'authentik.env'))
    if ($kind -eq 'worker') { $runArguments += @('--user','root','--volume',$codeMount,'--volume',$privateMount) }
    if ($kind -eq 'server') { $runArguments += @('--publish','127.0.0.1:19090:9000') }
    $runArguments += @($authImage,$kind)
    CheckedContainer $runArguments | Out-Null
    # WSLC port forwarding needs a gateway for replies to the host. Only the
    # server gets this edge network; PostgreSQL/worker stay internal.
    if ($kind -eq 'server') { CheckedContainer @('network','connect',$edgeNetwork,$server) | Out-Null }
}
$ready=$false
for ($attempt=0; $attempt -lt 120; $attempt++) {
    try {
        $health = Invoke-WebRequest 'http://127.0.0.1:19090/-/health/ready/' -TimeoutSec 2 -SkipHttpErrorCheck
        if ($health.StatusCode -eq 204 -or $health.StatusCode -eq 200) { $ready=$true; break }
    } catch { }
    Start-Sleep -Seconds 2
}
if (!$ready) { throw ('Isolated Authentik is not ready; inspect only owned lab containers from ' + (Join-Path $run 'environment.json')) }
# The worker loads default flows/keys asynchronously. Wait for those fixtures.
$configured = $null
for ($attempt=0; $attempt -lt 20; $attempt++) {
    $bootstrapOutput = @(& $containerCli exec $worker ak shell -c "exec(compile(open('/lab-code/bootstrap.py').read(), '/lab-code/bootstrap.py', 'exec'))" 2>&1)
    if ($LASTEXITCODE -eq 0) {
        $configuredLine = @($bootstrapOutput | Where-Object { $_ -is [string] -and $_.StartsWith('N01C_CONFIGURED ') })
        if ($configuredLine.Count -eq 1) { $configured = $configuredLine[0].Substring(16) | ConvertFrom-Json; break }
    }
    if (@($bootstrapOutput | Where-Object { $_ -is [string] -and $_.Trim() -eq 'N01C_NOT_READY' }).Count -gt 0) {
        Start-Sleep -Seconds 3
        continue
    }
    # Retry only the explicit read-only readiness status. Other failures stop.
    break
}
if (!$configured) { throw ('Local fixture configuration failed; no secret-bearing output was printed. Run: ' + $run) }
$configured | ConvertTo-Json -Depth 4 | Set-Content (Join-Path $run 'provider-policy.json') -Encoding utf8
$environment = Get-Content (Join-Path $run 'environment.json') -Raw | ConvertFrom-Json
$environment.status='ready'
$environment | ConvertTo-Json -Depth 4 | Set-Content (Join-Path $run 'environment.json') -Encoding utf8
Write-Output ('PASS isolated Authentik lab ready: ' + (Join-Path $run 'environment.json'))
