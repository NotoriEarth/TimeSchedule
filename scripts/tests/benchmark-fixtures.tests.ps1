param([string]$OutputRoot = (Join-Path $PSScriptRoot '..\..\.test-data\benchmark\fixtures'))
$ErrorActionPreference = 'Stop'
$projectRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..\..'))
. (Join-Path $projectRoot 'scripts\toolchain.ps1')
Push-Location $projectRoot
try {
    cargo build -p timeschedule-core --example seed_benchmark --locked --offline
    if ($LASTEXITCODE -ne 0) { throw 'Fixture generator build failed.' }
    $generator = Join-Path $projectRoot 'target\debug\examples\seed_benchmark.exe'
    $root = [IO.Path]::GetFullPath($OutputRoot)
    $safeRoot = [IO.Path]::GetFullPath((Join-Path $projectRoot '.test-data\benchmark\fixtures'))
    if ($root -ne $safeRoot -and -not $root.StartsWith($safeRoot + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) { throw 'Fixture tests must write inside .test-data/benchmark/fixtures.' }
    foreach ($size in @('small','large')) {
        $first = Join-Path $root $size
        $second = Join-Path $root ($size + '-determinism')
        & $generator --size $size --output $first
        if ($LASTEXITCODE -ne 0) { throw "$size fixture generation failed" }
        & $generator --size $size --output $second
        if ($LASTEXITCODE -ne 0) { throw "$size deterministic regeneration failed" }
        $logicalHash = (Get-FileHash -LiteralPath (Join-Path $first 'logical.jsonl') -Algorithm SHA256).Hash
        $repeatHash = (Get-FileHash -LiteralPath (Join-Path $second 'logical.jsonl') -Algorithm SHA256).Hash
        if ($logicalHash -ne $repeatHash) { throw "$size logical hashes differ" }
        foreach ($directory in @($first,$second)) {
            $metadataPath = Join-Path $directory 'metadata.json'
            $metadata = Get-Content -LiteralPath $metadataPath -Raw | ConvertFrom-Json
            $expected = if ($size -eq 'small') { @(1000,2000,10,100,20) } else { @(10000,20000,50,1000,100) }
            $actual = @($metadata.events,$metadata.entries,$metadata.projects,$metadata.tasks,$metadata.notes)
            for ($i=0; $i -lt $expected.Count; $i++) { if ($actual[$i] -ne $expected[$i]) { throw "$size counts mismatch" } }
            if ($metadata.longestEntryCharacters -ne 100000 -or $metadata.schemaVersion -ne 2 -or -not $metadata.koreanSearchVerified -or -not $metadata.walCheckpointed) { throw "$size validation metadata mismatch" }
            $dbPath = Join-Path $directory 'schedule.db'
            $dbHash = (Get-FileHash -LiteralPath $dbPath -Algorithm SHA256).Hash
            $metadata | Add-Member -NotePropertyName logicalSha256 -NotePropertyValue $logicalHash
            $metadata | Add-Member -NotePropertyName databaseSha256 -NotePropertyValue $dbHash
            $metadata | Add-Member -NotePropertyName databaseBytes -NotePropertyValue (Get-Item -LiteralPath $dbPath).Length
            $metadata | Add-Member -NotePropertyName deterministicRegenerationVerified -NotePropertyValue $true
            [IO.File]::WriteAllText($metadataPath, ($metadata | ConvertTo-Json -Depth 5), [Text.UTF8Encoding]::new($false))
        }
        $originalDbHash = (Get-FileHash -LiteralPath (Join-Path $first 'schedule.db')).Hash
        & $generator --size $size --output $first 2> (Join-Path $root "$size-overwrite-rejection.log")
        if ($LASTEXITCODE -eq 0) { throw 'Generator unexpectedly overwrote existing fixture.' }
        if ((Get-FileHash -LiteralPath (Join-Path $first 'schedule.db')).Hash -ne $originalDbHash) { throw 'Refused overwrite changed fixture.' }
        Write-Output "PASS ${size}: counts, integrity, schema reopen, Korean FTS, 100000-character body, deterministic logical SHA256 $logicalHash, overwrite refusal"
    }
    $global:LASTEXITCODE = 0
} finally { Pop-Location }
