param(
    [string]$SourcePath = (Join-Path $PSScriptRoot '..\..\target\release\timeschedule-native.exe'),
    [switch]$CheckRunningApp
)
$ErrorActionPreference = 'Stop'
function Assert($condition, [string]$message) { if (-not $condition) { throw $message } }
function Throws([scriptblock]$operation, [string]$message) {
    $thrown = $false
    try { & $operation | Out-Null } catch { $thrown = $true }
    Assert $thrown $message
}
$source = (Resolve-Path -LiteralPath $SourcePath).Path
$version = (Get-Item -LiteralPath $source).VersionInfo.ProductVersion
$manifest = Get-Content -LiteralPath (Join-Path $PSScriptRoot '..\..\apps\timeschedule-native\Cargo.toml') -Raw
$expectedVersion = [regex]::Match($manifest, '(?m)^version = "([^"]+)"').Groups[1].Value
Assert ($expectedVersion -and $version -eq $expectedVersion) 'The native EXE must embed the current Cargo product version.'
Assert ((Get-Item -LiteralPath $source).VersionInfo.FileVersion -eq $expectedVersion) 'File and product versions must match.'
. (Join-Path $PSScriptRoot '..\publish-native.ps1')
$root = Join-Path ([IO.Path]::GetTempPath()) ('timeschedule-publish-' + [guid]::NewGuid().ToString('N'))
# Built from code points so the non-ASCII path survives any script encoding.
$artifacts = Join-Path $root ([string][char]0xD55C + [char]0xAE00 + ' artifacts')
New-Item -ItemType Directory -Path $artifacts | Out-Null
try {
    $first = Publish-NativeExecutable -SourcePath $source -ArtifactsDirectory $artifacts -ExpectedVersion $version
    Assert ($first.path -eq (Join-Path $artifacts 'TimeSchedule.exe')) 'Output filename changed.'
    Assert ((Get-FileHash -LiteralPath $first.path).Hash -eq (Get-FileHash -LiteralPath $source).Hash) 'Published bytes differ.'
    Assert ($null -eq $first.previousExecutable) 'First publication should not archive a nonexistent output.'
    [IO.File]::WriteAllText($first.path, 'previous executable bytes')
    $oldHash = (Get-FileHash -LiteralPath $first.path).Hash
    Throws { Publish-NativeExecutable -SourcePath $source -ArtifactsDirectory $artifacts -ExpectedVersion '99.0.0' } 'Version mismatch accepted.'
    Assert ((Get-FileHash -LiteralPath $first.path).Hash -eq $oldHash) 'Version mismatch changed the existing output.'
    $lock = [IO.File]::Open($first.path, 'Open', 'Read', 'None')
    try {
        Throws { Publish-NativeExecutable -SourcePath $source -ArtifactsDirectory $artifacts -ExpectedVersion $version } 'Locked output replaced.'
    } finally { $lock.Dispose() }
    Assert ((Get-FileHash -LiteralPath $first.path).Hash -eq $oldHash) 'Locked output was damaged.'
    $second = Publish-NativeExecutable -SourcePath $source -ArtifactsDirectory $artifacts -ExpectedVersion $version
    Assert ((Get-FileHash -LiteralPath $second.previousExecutable).Hash -eq $oldHash) 'Previous output was not preserved exactly.'
    Assert ((Get-FileHash -LiteralPath $second.path).Hash -eq (Get-FileHash -LiteralPath $source).Hash) 'Replacement bytes differ.'
    Throws { Publish-NativeExecutable -SourcePath (Join-Path $root 'missing.exe') -ArtifactsDirectory $artifacts -ExpectedVersion $version } 'Missing source accepted.'
    Assert (@(Get-ChildItem -LiteralPath $artifacts -Filter '*.staging' -File).Count -eq 0) 'Staged temporary files leaked.'
    Assert (@(Get-ChildItem -LiteralPath $artifacts -Filter '*.exe' -File).Count -eq 1) 'More than one current EXE.'
    if ($CheckRunningApp) {
        $previousDataDir = $env:TIMESCHEDULE_DATA_DIR
        $ownedProcess = $null
        try {
            $env:TIMESCHEDULE_DATA_DIR = Join-Path $root 'isolated-data'
            $ownedProcess = Start-Process -FilePath $second.path -WindowStyle Hidden -PassThru
            Start-Sleep -Seconds 3
            $ownedProcess.Refresh()
            Assert (-not $ownedProcess.HasExited) 'Isolated app exited before the running-output check.'
            $before = (Get-FileHash -LiteralPath $second.path).Hash
            Throws { Publish-NativeExecutable -SourcePath $source -ArtifactsDirectory $artifacts -ExpectedVersion $version } 'Running output replaced.'
            $ownedProcess.Refresh()
            Assert (-not $ownedProcess.HasExited) 'Publisher stopped the running app.'
            Assert ((Get-FileHash -LiteralPath $second.path).Hash -eq $before) 'Running EXE changed.'
        } finally {
            if ($ownedProcess -and -not $ownedProcess.HasExited) { $ownedProcess.Kill(); $ownedProcess.WaitForExit() }
            $env:TIMESCHEDULE_DATA_DIR = $previousDataDir
        }
    }
    Write-Output 'Publication checks passed: version, exact bytes, first install, archive, locked output, invalid source, staging cleanup.'
} finally {
    $resolved = [IO.Path]::GetFullPath($root)
    $tempRoot = [IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd('\') + '\'
    if (-not $resolved.StartsWith($tempRoot, [StringComparison]::OrdinalIgnoreCase) -or (Split-Path -Leaf $resolved) -notlike 'timeschedule-publish-*') { throw 'Unsafe test cleanup path.' }
    Remove-Item -LiteralPath $resolved -Recurse -Force
}
