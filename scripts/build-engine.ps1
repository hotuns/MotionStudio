param([switch]$Release)
$ErrorActionPreference = 'Stop'
$motionScript = Join-Path $PSScriptRoot 'build-engine.mjs'
if ($Release) { & node $motionScript --release } else { & node $motionScript }
exit $LASTEXITCODE
