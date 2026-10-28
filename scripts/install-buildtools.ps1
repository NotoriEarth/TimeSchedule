$ErrorActionPreference = 'Stop'
$workspace = Split-Path -Parent $PSScriptRoot
$tooling = Join-Path $workspace '.tooling'
New-Item -ItemType Directory -Force -Path $tooling | Out-Null
$installer = Join-Path $tooling 'vs_BuildTools.exe'
Invoke-WebRequest -UseBasicParsing -Uri 'https://aka.ms/vs/17/release/vs_BuildTools.exe' -OutFile $installer
$signature = Get-AuthenticodeSignature -LiteralPath $installer
if ($signature.Status -ne 'Valid' -or $signature.SignerCertificate.Subject -notmatch 'Microsoft Corporation') {
    throw 'Microsoft Build Tools installer signature is not valid.'
}
$installPath = Join-Path $tooling 'BuildTools'
$installerArgs = @('--quiet', '--wait', '--norestart', '--nocache', '--installPath', $installPath, '--add', 'Microsoft.VisualStudio.Workload.VCTools', '--includeRecommended')
$process = Start-Process -FilePath $installer -ArgumentList $installerArgs -WindowStyle Hidden -Wait -PassThru
if ($process.ExitCode -notin @(0, 3010)) { throw "Build Tools installation failed: $($process.ExitCode)" }
Write-Output "Build Tools exit code: $($process.ExitCode)"
