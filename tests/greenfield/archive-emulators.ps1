$ErrorActionPreference = 'Stop'
Set-Location -LiteralPath (Join-Path $PSScriptRoot '..\..')
$suffix = [guid]::NewGuid().ToString('N')
$s3Name = "brain-greenfield-s3-$suffix"
$dynamoName = "brain-greenfield-dynamo-$suffix"
$created = @()
$result = 1
try {
    $s3 = & docker run --detach --rm --name $s3Name --label "copilot.session=211666ca-c6e9-4741-aef7-9b195ab235c0" --cpus 1 --memory 512m --publish '127.0.0.1::9000' --env MINIO_ROOT_USER=brain-fixture --env MINIO_ROOT_PASSWORD=brain-fixture-not-live 'minio/minio@sha256:14cea493d9a34af32f524e538b8346cf79f3321eff8e708c1e2960462bd8936e' server /data
    if ($LASTEXITCODE -ne 0) { throw 'S3 emulator failed to start' }
    $created += $s3.Trim()
    $dynamo = & docker run --detach --rm --name $dynamoName --label "copilot.session=211666ca-c6e9-4741-aef7-9b195ab235c0" --cpus 1 --memory 512m --publish '127.0.0.1::8000' 'amazon/dynamodb-local@sha256:ff89bd48ff32cd8d9be5fee8873b65b8854dc408f1afe881be6eb00247bc0dab' -jar DynamoDBLocal.jar -inMemory -sharedDb
    if ($LASTEXITCODE -ne 0) { throw 'DynamoDB emulator failed to start' }
    $created += $dynamo.Trim()
    $env:BRAIN_TEST_S3_ENDPOINT = 'http://' + (& docker port $s3Name '9000/tcp').Trim()
    $env:BRAIN_TEST_DYNAMO_ENDPOINT = 'http://' + (& docker port $dynamoName '8000/tcp').Trim()
    $deadline = [datetime]::UtcNow.AddSeconds(60)
    while ($true) {
        try {
            $health = Invoke-WebRequest -Uri ($env:BRAIN_TEST_S3_ENDPOINT + '/minio/health/ready') -TimeoutSec 2
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
    foreach ($id in $created) { & docker stop --time 10 $id | Out-Null }
}
exit $result
