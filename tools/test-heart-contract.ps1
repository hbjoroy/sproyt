param(
    [string]$HeartSource = (Resolve-Path "$PSScriptRoot\..\..\heart"),
    [switch]$KeepImage
)

$ErrorActionPreference = 'Stop'
$network = 'sproyt-heart-contract'
$postgres = 'sproyt-heart-postgres'
$api = 'sproyt-heart-api'
$image = 'local/heart-api:contract'

function Assert-Wslc([string]$Step) {
    if ($LASTEXITCODE -ne 0) { throw "$Step failed with exit code $LASTEXITCODE" }
}

# A killed or timed-out previous contract must never be reused as evidence.
wslc remove --force $api $postgres 2>$null | Out-Null
wslc network remove $network 2>$null | Out-Null

try {
    wslc build -f "$PSScriptRoot\heart-contract.Containerfile" -t $image $HeartSource
    Assert-Wslc 'Heart contract image build'
    wslc network create $network
    Assert-Wslc 'Heart contract network creation'
    wslc run --detach --name $postgres --network $network --network-alias postgres `
        -e POSTGRES_DB=heart -e POSTGRES_USER=heart -e POSTGRES_PASSWORD=heart-contract `
        -v "${HeartSource}\migrations:/docker-entrypoint-initdb.d:ro" postgres:17 | Out-Null
    Assert-Wslc 'Heart contract PostgreSQL creation'
    $postgresReady = $false
    foreach ($attempt in 1..30) {
        wslc exec $postgres pg_isready -U heart -d heart 2>$null | Out-Null
        if ($LASTEXITCODE -eq 0) { $postgresReady = $true; break }
        Start-Sleep -Seconds 1
    }
    if (-not $postgresReady) { throw 'Heart contract PostgreSQL did not become ready within 30 seconds' }
    wslc run --detach --name $api --network $network -p 13000:3000 `
        -e DATABASE_URL=postgres://heart:heart-contract@postgres:5432/heart `
        -e BIND_ADDR=0.0.0.0:3000 $image | Out-Null
    Assert-Wslc 'Heart contract API creation'
    $apiReady = $false
    foreach ($attempt in 1..30) {
        try {
            Invoke-WebRequest -UseBasicParsing -TimeoutSec 2 http://127.0.0.1:13000/health | Out-Null
            $apiReady = $true
            break
        } catch {
            Start-Sleep -Seconds 1
        }
    }
    if (-not $apiReady) { throw 'Heart contract API did not become ready within 30 seconds' }
    $yaml = Get-Content "$PSScriptRoot\..\processes\event-planning.yaml" -Raw
    Invoke-RestMethod -Method Post -Uri http://127.0.0.1:13000/api/v1/definitions `
        -TimeoutSec 10 -ContentType text/plain -Body $yaml | Out-Null
    $link = [guid]::NewGuid().ToString()
    $startKey = [guid]::NewGuid().ToString()
    $startHeaders = @{ 'X-Heart-Client'='sproyt'; 'Idempotency-Key'=$startKey }
    $start = @{ namespace='sproyt'; definition_name='sproyt-event-planning'; version='1.0.0'; metadata=@{ process_link_id=$link; title='Contract test' } } | ConvertTo-Json -Depth 5
    $instance = Invoke-RestMethod -TimeoutSec 10 -Method Post -Uri http://127.0.0.1:13000/api/v1/instances -Headers $startHeaders -ContentType application/json -Body $start
    $replay = Invoke-RestMethod -TimeoutSec 10 -Method Post -Uri http://127.0.0.1:13000/api/v1/instances -Headers $startHeaders -ContentType application/json -Body $start
    $resolved = Invoke-RestMethod -TimeoutSec 10 -Uri "http://127.0.0.1:13000/api/v1/instance-starts/${startKey}?namespace=sproyt" -Headers @{ 'X-Heart-Client'='sproyt' }
    if (-not $replay.replayed -or $replay.instance_id -ne $instance.instance_id -or $resolved.instance_id -ne $instance.instance_id) { throw "Heart idempotent start/reconciliation contract failed" }
    $before = Invoke-RestMethod -TimeoutSec 10 -Uri "http://127.0.0.1:13000/api/v1/instances/$($instance.instance_id)"
    if ($before.status -ne 'waiting' -or $before.current_node -ne 'wait-for-decision') { throw "Heart did not enter receive wait state" }
    $message = @{ namespace='sproyt'; correlation_key='process_link_id'; correlation_value=$link; payload=@{ decision='yes' } } | ConvertTo-Json -Depth 5
    $result = Invoke-RestMethod -TimeoutSec 10 -Method Post -Uri http://127.0.0.1:13000/api/v1/messages -ContentType application/json -Body $message
    $after = Invoke-RestMethod -TimeoutSec 10 -Uri "http://127.0.0.1:13000/api/v1/instances/$($instance.instance_id)"
    if ($result.matched_instances -ne 1 -or $after.status -ne 'completed' -or $after.metadata.decision -ne 'yes') { throw "Heart receive contract failed" }
    Write-Output "Heart contract passed for instance $($instance.instance_id)"
}
finally {
    wslc stop --time 2 $api $postgres 2>$null | Out-Null
    wslc remove $api $postgres 2>$null | Out-Null
    wslc network remove $network 2>$null | Out-Null
    if (-not $KeepImage) { wslc rmi $image 2>$null | Out-Null }
}
