param([switch]$DebugBuild)
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'toolchain.ps1')
Push-Location (Split-Path -Parent $PSScriptRoot)
try {
    & npm.cmd run check
    if ($LASTEXITCODE -ne 0) { throw 'Frontend type check failed.' }
    $buildArguments = @('run', 'tauri', 'build', '--', '--no-bundle')
    if ($DebugBuild) { $buildArguments += '--debug' }
    & npm.cmd @buildArguments
    if ($LASTEXITCODE -ne 0) { throw 'Native build failed.' }
} finally { Pop-Location }
