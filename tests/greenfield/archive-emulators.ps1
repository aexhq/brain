param([ValidateSet('moto', 'minio')][string]$S3Engine = 'moto')
$ErrorActionPreference = 'Stop'
Set-Location -LiteralPath (Join-Path $PSScriptRoot '..\..')
$suffix = [guid]::NewGuid().ToString('N')
$s3Name = "brain-greenfield-s3-$suffix"
$dynamoName = "brain-greenfield-dynamo-$suffix"
$scratch = Join-Path $PWD ".test-output\archive-$suffix"
$created = @()
$result = 1
$s3Image = if ($S3Engine -eq 'moto') {
    'ghcr.io/getmoto/motoserver@sha256:66672d842bf19618966890c574d85198abe92800f4b3b2b5f791015fa1b58831'
} else {
    'minio/minio@sha256:14cea493d9a34af32f524e538b8346cf79f3321eff8e708c1e2960462bd8936e'
}
$s3Port = if ($S3Engine -eq 'moto') { '5000' } else { '9000' }
$s3Health = if ($S3Engine -eq 'moto') { '/moto-api/' } else { '/minio/health/ready' }
$s3Arguments = if ($S3Engine -eq 'moto') { @() } else { @('server', '/work/s3/data') }
$dynamoImage = 'amazon/dynamodb-local@sha256:ff89bd48ff32cd8d9be5fee8873b65b8854dc408f1afe881be6eb00247bc0dab'
$identity = @()
if (-not $IsWindows) {
    $uid = (& id -u).Trim()
    $gid = (& id -g).Trim()
    $identity = @('--user', "${uid}:${gid}")
}
function Get-EmulatorOrigin([string]$Container, [string]$Port) {
    $deadline = [datetime]::UtcNow.AddSeconds(30)
    while ([datetime]::UtcNow -lt $deadline) {
        $state = & docker inspect $Container --format '{{.State.Status}}'
        if ($LASTEXITCODE -ne 0 -or $state -eq 'exited' -or $state -eq 'dead') {
            & docker logs --tail 30 $Container
            throw "Emulator exited before publishing its port: $Container"
        }
        $published = & docker port $Container "$Port/tcp" 2>$null
        if ($LASTEXITCODE -eq 0 -and $published) { return 'http://' + $published.Trim() }
        Start-Sleep -Milliseconds 200
    }
    throw "Emulator did not publish its port: $Container"
}
try {
    New-Item -ItemType Directory -Force -Path (Join-Path $scratch 's3'), (Join-Path $scratch 'dynamo') | Out-Null
    foreach ($image in @($s3Image, $dynamoImage)) {
        & docker image inspect $image --format '{{.Id}}' 2>$null | Out-Null
        if ($LASTEXITCODE -ne 0) {
            & docker pull --quiet $image | Out-Null
            if ($LASTEXITCODE -ne 0) { throw "Pinned emulator image could not be pulled: $image" }
        }
    }
    $s3 = & docker run --detach --pull never @identity --name $s3Name --label "copilot.session=211666ca-c6e9-4741-aef7-9b195ab235c0" --cpus 1 --memory 512m --pids-limit 128 --publish "127.0.0.1::$s3Port" --mount "type=bind,source=$scratch,target=/work" --env TMPDIR=/work/s3 --env TMP=/work/s3 --env TEMP=/work/s3 --env HOME=/work/s3 --env MINIO_ROOT_USER=brain-fixture --env MINIO_ROOT_PASSWORD=brain-fixture-not-live --env S3_IGNORE_SUBDOMAIN_BUCKETNAME=true $s3Image @s3Arguments
    if ($LASTEXITCODE -ne 0) { throw 'S3 emulator failed to start' }
    $created += $s3.Trim()
    $dynamo = & docker create --pull never @identity --name $dynamoName --label "copilot.session=211666ca-c6e9-4741-aef7-9b195ab235c0" --cpus 1 --memory 512m --publish '127.0.0.1::8000' --mount "type=bind,source=$scratch,target=/work" --workdir /work/dynamo --env TMPDIR=/work/dynamo --env TMP=/work/dynamo --env TEMP=/work/dynamo --env HOME=/work/dynamo $dynamoImage '-Djava.io.tmpdir=/work/dynamo' '-Duser.home=/work/dynamo' -jar DynamoDBLocal.jar -inMemory -sharedDb
    if ($LASTEXITCODE -ne 0) { throw 'DynamoDB emulator could not be created' }
    $created += $dynamo.Trim()
    # The image's private home is inaccessible to the host UID used by Linux CI.
    # Copy only its public distribution into the owned, host-user fixture directory.
    & docker cp "${dynamo}:/home/dynamodblocal/." (Join-Path $scratch 'dynamo') | Out-Null
    if ($LASTEXITCODE -ne 0) { throw 'DynamoDB emulator distribution could not be copied' }
    & docker start $dynamo | Out-Null
    if ($LASTEXITCODE -ne 0) { throw 'DynamoDB emulator failed to start' }
    $env:BRAIN_TEST_S3_ENDPOINT = Get-EmulatorOrigin $s3Name $s3Port
    $env:BRAIN_TEST_DYNAMO_ENDPOINT = Get-EmulatorOrigin $dynamoName '8000'
    $deadline = [datetime]::UtcNow.AddSeconds(60)
    while ($true) {
        try {
            $health = Invoke-WebRequest -Uri ($env:BRAIN_TEST_S3_ENDPOINT + $s3Health) -TimeoutSec 2
            if ($health.StatusCode -eq 200) { break }
        } catch {}
        if ([datetime]::UtcNow -gt $deadline) { throw 'S3 emulator never became ready' }
        Start-Sleep -Milliseconds 300
    }
    while ($true) {
        try { Invoke-WebRequest -Uri $env:BRAIN_TEST_DYNAMO_ENDPOINT -TimeoutSec 2 | Out-Null; break }
        catch {
            if ($null -ne $_.Exception.Response -and [int]$_.Exception.Response.StatusCode -eq 400) { break }
        }
        if ([datetime]::UtcNow -gt $deadline) { throw 'DynamoDB emulator never became responsive' }
        Start-Sleep -Milliseconds 300
    }
    $env:GOTMPDIR = Join-Path $PWD '.build\go'
    $env:TMP = Join-Path $PWD '.build\scratch'
    $env:TEMP = $env:TMP
    & go test -tags=integration -count=1 -v ./adapters/archive
    $result = $LASTEXITCODE
} finally {
    foreach ($id in $created) { & docker container rm --force $id | Out-Null }
    if (Test-Path -LiteralPath $scratch) { Remove-Item -LiteralPath $scratch -Recurse -Force }
}
exit $result
