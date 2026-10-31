function Publish-NativeExecutable {
    param(
        [Parameter(Mandatory)][string]$SourcePath,
        [Parameter(Mandatory)][string]$ArtifactsDirectory,
        [Parameter(Mandatory)][string]$ExpectedVersion
    )
    $ErrorActionPreference = 'Stop'
    $source = Get-Item -LiteralPath $SourcePath
    if ($source.PSIsContainer -or $source.VersionInfo.ProductVersion -ne $ExpectedVersion) {
        throw "Native EXE product version does not match $ExpectedVersion. Existing output was not changed."
    }
    $directory = [IO.Path]::GetFullPath($ArtifactsDirectory)
    New-Item -ItemType Directory -Force -Path $directory | Out-Null
    $output = Join-Path $directory 'TimeSchedule.exe'
    $running = Get-Process -Name 'TimeSchedule' -ErrorAction SilentlyContinue | Where-Object {
        [string]::Equals($_.Path, $output, [StringComparison]::OrdinalIgnoreCase)
    }
    if ($running) { throw 'TimeSchedule is running. Save your work and exit from the tray before replacing the EXE.' }
    $staging = Join-Path $directory ('.TimeSchedule-' + [guid]::NewGuid().ToString('N') + '.staging')
    $previous = $null
    try {
        $hash = (Get-FileHash -LiteralPath $source.FullName -Algorithm SHA256).Hash
        Copy-Item -LiteralPath $source.FullName -Destination $staging
        if ((Get-FileHash -LiteralPath $staging -Algorithm SHA256).Hash -ne $hash) {
            throw 'Staged EXE differs from the build output. Existing output was not changed.'
        }
        if (Test-Path -LiteralPath $output) {
            $archive = Join-Path $directory 'archive'
            New-Item -ItemType Directory -Force -Path $archive | Out-Null
            $previous = Join-Path $archive ('TimeSchedule-' + (Get-Date -Format 'yyyyMMdd-HHmmss') + '-' + [guid]::NewGuid().ToString('N') + '.exe')
            # Same-volume atomic replacement; a locked output fails without truncating it.
            [IO.File]::Replace($staging, $output, $previous)
        } else {
            [IO.File]::Move($staging, $output)
        }
        [pscustomobject]@{
            path = $output
            version = $ExpectedVersion
            bytes = (Get-Item -LiteralPath $output).Length
            sha256 = $hash
            previousExecutable = $previous
        }
    } finally {
        if (Test-Path -LiteralPath $staging) { Remove-Item -LiteralPath $staging -Force }
    }
}
