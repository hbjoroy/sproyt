param(
    [string]$HeartSource = (Resolve-Path "$PSScriptRoot\..\..\heart"),
    [switch]$KeepImage
)

$ErrorActionPreference = 'Stop'
$network = 'sproyt-heart-contract'
$postgres = 'sproyt-heart-postgres'
$api = 'sproyt-heart-api'
$image = 'local/heart-api:contract'

try {
    wslc build -f "$PSScriptRoot\heart-contract.Containerfile" -t $image $HeartSource
    wslc network create $network
    wslc run --detach --name $postgres --network $network --network-alias postgres `
        -e POSTGRES_DB=heart -e POSTGRES_USER=heart -e POSTGRES_PASSWORD=heart-contract `
        -v "${HeartSource}\migrations:/docker-entrypoint-initdb.d:ro" postgres:17 | Out-Null
    Start-Sleep -Seconds 4
    wslc run --detach --name $api --network $network -p 13000:3000 `
        -e DATABASE_URL=postgres://heart:heart-contract@postgres:5432/heart `
        -e BIND_ADDR=0.0.0.0:3000 $image | Out-Null
    Start-Sleep -Seconds 2
    Invoke-WebRequest -UseBasicParsing http://127.0.0.1:13000/health | Out-Null
    $yaml = Get-Content "$PSScriptRoot\..\processes\event-planning.yaml" -Raw
    Invoke-RestMethod -Method Post -Uri http://127.0.0.1:13000/api/v1/definitions `
        -ContentType text/plain -Body $yaml | Out-Null
    $link = [guid]::NewGuid().ToString()
    $startKey = [guid]::NewGuid().ToString()
    $startHeaders = @{ 'X-Heart-Client'='sproyt'; 'Idempotency-Key'=$startKey }
    $start = @{ namespace='sproyt'; definition_name='sproyt-event-planning'; version='1.0.0'; metadata=@{ process_link_id=$link; title='Contract test' } } | ConvertTo-Json -Depth 5
    $instance = Invoke-RestMethod -Method Post -Uri http://127.0.0.1:13000/api/v1/instances -Headers $startHeaders -ContentType application/json -Body $start
    $replay = Invoke-RestMethod -Method Post -Uri http://127.0.0.1:13000/api/v1/instances -Headers $startHeaders -ContentType application/json -Body $start
    $resolved = Invoke-RestMethod -Uri "http://127.0.0.1:13000/api/v1/instance-starts/${startKey}?namespace=sproyt" -Headers @{ 'X-Heart-Client'='sproyt' }
    if (-not $replay.replayed -or $replay.instance_id -ne $instance.instance_id -or $resolved.instance_id -ne $instance.instance_id) { throw "Heart idempotent start/reconciliation contract failed" }
    $before = Invoke-RestMethod -Uri "http://127.0.0.1:13000/api/v1/instances/$($instance.instance_id)"
    if ($before.status -ne 'waiting' -or $before.current_node -ne 'wait-for-decision') { throw "Heart did not enter receive wait state" }
    $message = @{ namespace='sproyt'; correlation_key='process_link_id'; correlation_value=$link; payload=@{ decision='yes' } } | ConvertTo-Json -Depth 5
    $result = Invoke-RestMethod -Method Post -Uri http://127.0.0.1:13000/api/v1/messages -ContentType application/json -Body $message
    $after = Invoke-RestMethod -Uri "http://127.0.0.1:13000/api/v1/instances/$($instance.instance_id)"
    if ($result.matched_instances -ne 1 -or $after.status -ne 'completed' -or $after.metadata.decision -ne 'yes') { throw "Heart receive contract failed" }
    Write-Output "Heart contract passed for instance $($instance.instance_id)"
}
finally {
    wslc stop $api $postgres 2>$null | Out-Null
    wslc remove $api $postgres 2>$null | Out-Null
    wslc network remove $network 2>$null | Out-Null
    if (-not $KeepImage) { wslc rmi $image 2>$null | Out-Null }
}
