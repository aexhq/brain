param([string]$TestName = '')
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
$root = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
Set-Location -LiteralPath $root
$suffix = [guid]::NewGuid().ToString('N')
$directory = Join-Path $root ".test-output\d1-linux-$suffix"
$image = "brain-control-test:$suffix"
$container = "brain-d1-test-$suffix"
$volume = "brain-d1-work-$suffix"
$createdImage = $false
$createdVolume = $false
try {
    New-Item -ItemType Directory -Force -Path (Join-Path $directory '.build\scratch') | Out-Null
    $files = git ls-files --cached --others --exclude-standard
    foreach ($file in $files) {
        $source = Join-Path $root $file
        if (-not (Test-Path -LiteralPath $source -PathType Leaf)) { continue }
        $target = Join-Path $directory $file
        New-Item -ItemType Directory -Force -Path (Split-Path -Parent $target) | Out-Null
        Copy-Item -LiteralPath $source -Destination $target
    }
    Copy-Item -LiteralPath (Join-Path $root '.build\brain-linux') -Destination (Join-Path $directory '.build\brain')
    docker build --quiet --file tests\greenfield\Dockerfile.control --tag $image .
    $createdImage = $true
    docker volume create --label copilot.session=211666ca-c6e9-4741-aef7-9b195ab235c0 $volume | Out-Null
    $createdVolume = $true
    $selector = if ($TestName) { "--test-name-pattern=$TestName" } else { '--test-concurrency=1' }
    docker run --name $container --cpus 2 --memory 2g --pids-limit 256 --read-only --cap-drop ALL `
        --security-opt no-new-privileges --mount "type=bind,source=$directory,target=/source,readonly" `
        --mount "type=volume,source=$volume,target=/work" `
        --env BRAIN_TEST_CONTROL=d1 --env BRAIN_TEST_PYTHON=python --env GOMAXPROCS=2 --env "BRAIN_TEST_SELECTOR=$selector" `
        $image sh -c 'set -eu; cp -a /source/. /work/; node --version; python --version; npm ci --ignore-scripts --no-audit --no-fund; npm run build -w packages/brain-sdk; npm test -w packages/brain-d1; chmod +x .build/brain; node --test "$BRAIN_TEST_SELECTOR" tests/greenfield/journeys.test.mjs'
} finally {
    $PSNativeCommandUseErrorActionPreference = $false
    docker container rm --force $container 2>$null | Out-Null
    if ($createdVolume) { docker volume rm $volume 2>$null | Out-Null }
    if ($createdImage) { docker image rm $image 2>$null | Out-Null }
    if (Test-Path -LiteralPath $directory) { Remove-Item -LiteralPath $directory -Recurse -Force }
}
