$ErrorActionPreference = 'Stop'
$workspace = Split-Path -Parent $PSScriptRoot
$tooling = Join-Path $workspace '.tooling'
New-Item -ItemType Directory -Force -Path $tooling | Out-Null
$installer = Join-Path $tooling 'rustup-init.exe'
$baseUrl = 'https://static.rust-lang.org/rustup/dist/x86_64-pc-windows-msvc/rustup-init.exe'
Invoke-WebRequest -UseBasicParsing -Uri $baseUrl -OutFile $installer
$checksumResponse = (Invoke-WebRequest -UseBasicParsing -Uri ($baseUrl + '.sha256')).Content
$checksum = if ($checksumResponse -is [byte[]]) { [System.Text.Encoding]::UTF8.GetString($checksumResponse) } else { [string]$checksumResponse }
$expected = ($checksum.Trim() -split '\s+')[0].ToLowerInvariant()
$actual = (Get-FileHash -LiteralPath $installer -Algorithm SHA256).Hash.ToLowerInvariant()
if ($expected -ne $actual) { throw 'Rust installer SHA256 mismatch.' }
$env:CARGO_HOME = Join-Path $tooling 'cargo'
$env:RUSTUP_HOME = Join-Path $tooling 'rustup'
& $installer -y --no-modify-path --profile minimal --default-host x86_64-pc-windows-msvc --default-toolchain stable
if ($LASTEXITCODE -ne 0) { throw "Rust installation failed: $LASTEXITCODE" }
& (Join-Path $env:CARGO_HOME 'bin\rustc.exe') --version
