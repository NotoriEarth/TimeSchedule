param([switch]$Run, [switch]$Offline)
$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
. (Join-Path $PSScriptRoot 'toolchain.ps1')
. (Join-Path $PSScriptRoot 'publish-native.ps1')
Push-Location $projectRoot
try {
    $cargoOptions = @('--locked')
    if ($Offline) { $cargoOptions += '--offline' }
    cargo test -p timeschedule-native @cargoOptions
    if ($LASTEXITCODE -ne 0) { throw 'Native application tests failed.' }
    cargo build -p timeschedule-native --release @cargoOptions
    if ($LASTEXITCODE -ne 0) { throw 'Native release build failed.' }
    $metadataJson = cargo metadata --no-deps --format-version 1 @cargoOptions
    if ($LASTEXITCODE -ne 0) { throw 'Could not read native package version.' }
    $metadata = ($metadataJson -join "`n") | ConvertFrom-Json
    $package = $metadata.packages | Where-Object name -eq 'timeschedule-native'
    $release = Publish-NativeExecutable -SourcePath (Join-Path $metadata.target_directory 'release\timeschedule-native.exe') -ArtifactsDirectory (Join-Path $projectRoot 'artifacts') -ExpectedVersion $package.version
    $release | ConvertTo-Json
    if ($Run) { Start-Process -FilePath $release.path -WindowStyle Hidden }
} finally { Pop-Location }
