param([switch]$Run)
$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
. (Join-Path $PSScriptRoot 'toolchain.ps1')
Push-Location $projectRoot
try {
    cargo test -p timeschedule-native --locked
    if ($LASTEXITCODE -ne 0) { throw 'Native application tests failed.' }
    cargo build -p timeschedule-native --release --locked
    if ($LASTEXITCODE -ne 0) { throw 'Native release build failed.' }
    $output = Join-Path $projectRoot 'artifacts\TimeSchedule-native.exe'
    New-Item -ItemType Directory -Force (Split-Path -Parent $output) | Out-Null
    # If the exact artifact is already running, Copy-Item fails without touching it.
    Copy-Item -LiteralPath 'target\release\timeschedule-native.exe' -Destination $output
    [pscustomobject]@{path=$output;bytes=(Get-Item -LiteralPath $output).Length;sha256=(Get-FileHash -LiteralPath $output -Algorithm SHA256).Hash} | ConvertTo-Json
    if ($Run) { Start-Process -FilePath $output -WindowStyle Hidden }
} finally { Pop-Location }
