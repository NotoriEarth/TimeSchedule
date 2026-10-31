$ErrorActionPreference = 'Stop'
# Exercise actual default EXE path/hash validation, then stop before any process launches.
$missing = Join-Path ([IO.Path]::GetTempPath()) ('timeschedule-missing-fixture-' + [guid]::NewGuid().ToString('N'))
$output = $missing + '-output'
try {
    & (Join-Path $PSScriptRoot '..\benchmark-ui.ps1') -Preflight -FixtureRoot $missing -OutputDirectory $output -Datasets small
    throw 'Benchmark unexpectedly accepted a missing fixture.'
} catch {
    if ($_.Exception.Message -notlike 'Fixture missing:*') { throw "Archived benchmark inputs failed validation: $($_.Exception.Message)" }
}
if (Test-Path -LiteralPath $output) { throw 'Benchmark created output before validating fixtures.' }
Write-Output 'Historical benchmark default paths and pinned hashes validated; no app launched.'
