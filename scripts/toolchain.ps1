$workspace = Split-Path -Parent $PSScriptRoot
$env:CARGO_HOME = Join-Path $workspace '.tooling\cargo'
$env:RUSTUP_HOME = Join-Path $workspace '.tooling\rustup'
$env:PATH = (Join-Path $env:CARGO_HOME 'bin') + ';' + $env:PATH
$vsCommand = Join-Path $workspace '.tooling\BuildTools\Common7\Tools\VsDevCmd.bat'
if (-not (Test-Path -LiteralPath $vsCommand)) {
    $vswhere = 'C:\Program Files (x86)\Microsoft Visual Studio\Installer\vswhere.exe'
    if (Test-Path -LiteralPath $vswhere) {
        $vsInstall = & $vswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
        if ($vsInstall) { $vsCommand = Join-Path $vsInstall 'Common7\Tools\VsDevCmd.bat' }
    }
}
if (-not (Test-Path -LiteralPath $vsCommand)) { throw 'Microsoft C++ Build Tools are not installed.' }
$command = 'call "' + $vsCommand + '" -no_logo -arch=x64 -host_arch=x64 >nul && set'
& $env:ComSpec /d /s /c $command | ForEach-Object {
    if ($_ -match '^([^=]+)=(.*)$') { [Environment]::SetEnvironmentVariable($matches[1], $matches[2], 'Process') }
}
if ($LASTEXITCODE -ne 0) { throw 'Could not load the Microsoft C++ build environment.' }
