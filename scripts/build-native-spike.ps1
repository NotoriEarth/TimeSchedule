param(
    [switch]$Run,
    [ValidateSet('editor', 'desktop')][string]$Probe = 'editor'
)
$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
. (Join-Path $PSScriptRoot 'toolchain.ps1')
Push-Location $projectRoot
try {
    cargo test --manifest-path experiments/rust-ui-spike/Cargo.toml --release --locked
    if ($LASTEXITCODE -ne 0) { throw 'Rust prototype tests failed.' }
    cargo build --manifest-path experiments/rust-ui-spike/Cargo.toml --release --locked
    if ($LASTEXITCODE -ne 0) { throw 'Rust prototype build failed.' }
    $output = Join-Path $projectRoot 'artifacts\TimeSchedule-rust-prototype.exe'
    New-Item -ItemType Directory -Force (Split-Path -Parent $output) | Out-Null
    Copy-Item -LiteralPath 'experiments\rust-ui-spike\target\release\timeschedule-ui-spike.exe' -Destination $output
    Get-Item -LiteralPath $output | Select-Object FullName, Length
    Get-FileHash -LiteralPath $output -Algorithm SHA256
    if ($Run) { Start-Process -FilePath $output -ArgumentList "--probe=$Probe" -WindowStyle Hidden }
} finally { Pop-Location }
