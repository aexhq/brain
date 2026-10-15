param([switch]$Cluster, [switch]$ClusterOnly)
$ErrorActionPreference = 'Stop'
Set-Location -LiteralPath (Join-Path $PSScriptRoot '..\..')
$name = 'brain-greenfield-postgres-' + [guid]::NewGuid().ToString('N')
$id = $null
$result = 1
try {
    $id = (& docker run --detach --rm --name $name --label 'copilot.session=211666ca-c6e9-4741-aef7-9b195ab235c0' --cpus 2 --memory 768m --publish '127.0.0.1::5432' --env POSTGRES_USER=brain --env POSTGRES_DB=brain --env POSTGRES_PASSWORD=brain-fixture-not-live 'postgres@sha256:051f7b7b3abdd564d5d1bd1e8c4b9c1b6e77087d1dd22020ede611c096a272e0').Trim()
    if ($LASTEXITCODE -ne 0 -or $id -notmatch '^[a-f0-9]{64}$') { throw 'PostgreSQL fixture did not start' }
    $deadline = [datetime]::UtcNow.AddSeconds(60)
    while ($true) {
        & docker exec $id pg_isready --username brain --dbname brain --quiet
        if ($LASTEXITCODE -eq 0) { break }
        if ([datetime]::UtcNow -gt $deadline) { throw 'PostgreSQL fixture did not become ready' }
        Start-Sleep -Milliseconds 300
    }
    $address = (& docker port $id '5432/tcp').Trim()
    $env:BRAIN_TEST_POSTGRES_URL = "postgres://brain:brain-fixture-not-live@$address/brain?sslmode=disable"
    $env:BRAIN_TEST_POSTGRES_CONTAINER = $id
    $env:GOTMPDIR = Join-Path $PWD '.build\go'
    $env:TMP = Join-Path $PWD '.build\scratch'
    $env:TEMP = $env:TMP
    $env:GOMAXPROCS = '4'
    $env:BRAIN_TEST_PYTHON = (python -c 'import sys; print(sys.executable)').Trim()
    $journeys = Join-Path $PSScriptRoot 'journeys.test.mjs'
    $clusterJourneys = Join-Path $PSScriptRoot 'cluster.journeys.test.mjs'
    if ($ClusterOnly) {
        & node --test --test-concurrency=1 $clusterJourneys
        $result = $LASTEXITCODE
    } else {
        & go test -count=1 -v ./adapters/sqlcontrol
        if ($LASTEXITCODE -ne 0) { $result = $LASTEXITCODE }
        else {
            & node --test --test-concurrency=1 $journeys
            $result = $LASTEXITCODE
            if ($result -eq 0 -and $Cluster) {
                & node --test --test-concurrency=1 $clusterJourneys
                $result = $LASTEXITCODE
            }
        }
    }
} finally {
    if ($id -match '^[a-f0-9]{64}$') { & docker stop --time 10 $id | Out-Null }
}
exit $result
