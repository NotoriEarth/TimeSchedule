param(
    [string]$BaselineExecutable = (Join-Path $PSScriptRoot '..\artifacts\baseline\TimeSchedule.exe'),
    [string]$NativeExecutable = (Join-Path $PSScriptRoot '..\artifacts\TimeSchedule-native.exe'),
    [string]$FixtureRoot = (Join-Path $PSScriptRoot '..\.test-data\benchmark\fixtures'),
    [string]$OutputDirectory,
    [ValidateSet('small','large')][string[]]$Datasets = @('small','large'),
    [ValidateRange(1,20)][int]$Pairs = 5,
    [ValidateRange(0,3600)][int]$SettleSeconds = 60,
    [ValidateRange(2,10000)][int]$Samples = 120,
    [ValidateRange(100,60000)][int]$IntervalMilliseconds = 1000,
    [switch]$Preflight
)
$ErrorActionPreference='Stop'
. (Join-Path $PSScriptRoot 'benchmark-support.ps1')
if (-not $Preflight -and ($Pairs -ne 5 -or $SettleSeconds -ne 60 -or $Samples -ne 120 -or $IntervalMilliseconds -ne 1000)) { throw 'Nonstandard timing requires -Preflight and is not a full benchmark.' }
if (-not $OutputDirectory) { $OutputDirectory=Join-Path $PSScriptRoot ('..\.test-data\benchmark\runs\'+[datetime]::Now.ToString('yyyyMMdd-HHmmss')) }
$OutputDirectory=[IO.Path]::GetFullPath($OutputDirectory)
if (Test-Path -LiteralPath $OutputDirectory) { throw 'Output directory already exists; use a new directory.' }
$variants=@{
    tauri=@{path=(Resolve-Path -LiteralPath $BaselineExecutable).Path;sha256='2C109E6C8D09A904C480B7B111B763824B3C47C42F54D879925E1FF92DCB5047'}
    native=@{path=(Resolve-Path -LiteralPath $NativeExecutable).Path;sha256='30EE5BFB0BD7A9FFB4814DA18F31E4507FB74A46E7DDE2BE4339B303B17096BD'}
}
foreach ($variant in $variants.Values) { if ((Get-FileHash -LiteralPath $variant.path).Hash -ne $variant.sha256) { throw "Executable provenance mismatch: $($variant.path)" } }
foreach ($dataset in $Datasets) {
    $fixture=Join-Path $FixtureRoot "$dataset\schedule.db"
    if (-not (Test-Path -LiteralPath $fixture)) { throw "Fixture missing: $fixture" }
    if ((Test-Path -LiteralPath "$fixture-wal") -and (Get-Item -LiteralPath "$fixture-wal").Length -gt 0) { throw "Fixture has an uncheckpointed WAL: $fixture" }
    $metadata=Get-Content -LiteralPath (Join-Path $FixtureRoot "$dataset\metadata.json") -Raw | ConvertFrom-Json
    if ((Get-FileHash -LiteralPath $fixture).Hash -ne $metadata.databaseSha256) { throw "Fixture metadata hash mismatch: $dataset" }
}
New-Item -ItemType Directory -Path $OutputDirectory | Out-Null
Initialize-BenchmarkWindowTelemetry
$protocol=[pscustomobject]@{kind=$(if($Preflight){'preflight'}else{'full'});scenario='default monthly startup idle; native and Tauri default window layouts may differ';pairs=$Pairs;settleSeconds=$SettleSeconds;samples=$Samples;intervalMilliseconds=$IntervalMilliseconds;datasets=$Datasets;variants=$variants;environment=(Get-BenchmarkEnvironment);notes=@('All observed app descendants included. Working-set sum can double-count shared pages. Private commit is not resident RAM.','CPU 100% core-equivalent means one saturated logical processor; machine percentage divides by logical processors.','Startup metric is first observed MainWindowHandle, not content-ready or interactive latency.','No automation changes windows. Start-Process uses Hidden; top-level visibility/minimized/geometry are recorded.','Existing user apps remain running. System CPU snapshots disclose background load; no exclusive-machine claim.')}
$protocol | ConvertTo-Json -Depth 9 | Set-Content -LiteralPath (Join-Path $OutputDirectory 'protocol.json') -Encoding utf8
$runSummaries=[Collections.Generic.List[object]]::new()
$previousEnvironment=[Environment]::GetEnvironmentVariable('TIMESCHEDULE_DATA_DIR','Process')
$previousWebViewEnvironment=[Environment]::GetEnvironmentVariable('WEBVIEW2_USER_DATA_FOLDER','Process')
try {
 foreach ($dataset in $Datasets) {
  for ($pair=1; $pair -le $Pairs; $pair++) {
   $order=if ($pair % 2 -eq 1) { @('tauri','native') } else { @('native','tauri') }
   foreach ($name in $order) {
    $runName="$dataset-p$pair-$name"; $runDirectory=Join-Path $OutputDirectory $runName
    New-Item -ItemType Directory -Path $runDirectory | Out-Null
    $dataDirectory=Join-Path $runDirectory 'data'; New-Item -ItemType Directory -Path $dataDirectory | Out-Null
    $fixture=Join-Path $FixtureRoot "$dataset\schedule.db"
    Copy-Item -LiteralPath $fixture -Destination (Join-Path $dataDirectory 'schedule.db')
    Copy-Item -LiteralPath (Join-Path $FixtureRoot "$dataset\metadata.json") -Destination (Join-Path $runDirectory 'fixture-metadata.json')
    $fixtureHash=(Get-FileHash -LiteralPath $fixture).Hash
    if ((Get-FileHash -LiteralPath (Join-Path $dataDirectory 'schedule.db')).Hash -ne $fixtureHash) { throw 'Fixture copy mismatch' }
    [Environment]::SetEnvironmentVariable('TIMESCHEDULE_DATA_DIR',$dataDirectory,'Process')
    $webViewDirectory=Join-Path $runDirectory 'webview-profile'
    [Environment]::SetEnvironmentVariable('WEBVIEW2_USER_DATA_FOLDER',$webViewDirectory,'Process')
    $state=$null; $errorText=$null; $cleanup=@(); $measurements=[Collections.Generic.List[object]]::new(); $startup=$null; $windowInfo=@(); $systemBefore=$null; $systemAfter=$null; $webViewIsolation=$null
    $launchClock=[Diagnostics.Stopwatch]::StartNew()
    try {
      $process=Start-Process -FilePath $variants[$name].path -WindowStyle Hidden -PassThru
      $launchStart=$process.StartTime.ToUniversalTime()
      $state=New-BenchmarkState -Root ([pscustomobject]@{id=$process.Id;path=$variants[$name].path;startTicks=$launchStart.Ticks;startUtc=$launchStart.ToString('o');key="$($process.Id)/$($launchStart.Ticks)";parentId=$null})
      $root=Get-BenchmarkRoot -AppProcessId $process.Id -ExpectedExecutablePath $variants[$name].path -ExpectedStartTime $process.StartTime
      $state=New-BenchmarkState -Root $root
      [pscustomobject]@{run=$runName;phase='settling';pid=$root.id;root=$root;dataDirectory=$dataDirectory;startedAt=[datetime]::UtcNow.ToString('o')} | ConvertTo-Json -Depth 6 | Set-Content (Join-Path $OutputDirectory 'progress.json')
      Write-Output "START $runName PID $($root.id)"
      do {
        $null=Get-BenchmarkSample -State $state
        $process.Refresh()
        if ($null -eq $startup -and $process.MainWindowHandle -ne [IntPtr]::Zero) { $startup=$launchClock.Elapsed.TotalMilliseconds }
        if ($launchClock.Elapsed.TotalSeconds -lt $SettleSeconds) { Start-Sleep -Milliseconds ([int][math]::Min(1000,[math]::Max(1,($SettleSeconds-$launchClock.Elapsed.TotalSeconds)*1000))) }
      } while ($launchClock.Elapsed.TotalSeconds -lt $SettleSeconds)
      $windowInfo=@([BenchmarkWindowTelemetry]::Read([uint32]$root.id))
      if ($name -eq 'tauri') {
        $webViewRows=@(Get-CimInstance Win32_Process -Filter "Name='msedgewebview2.exe'" | Where-Object { [int]$_.ProcessId -in @($state.owned.Values.id) })
        $profileRows=@($webViewRows | Where-Object { $_.CommandLine -match '--user-data-dir=' })
        if ($profileRows.Count -eq 0) { throw 'No owned WebView user-data-dir observed; profile isolation unverified' }
        foreach ($profileRow in $profileRows) {
            $match=[regex]::Match($profileRow.CommandLine,'--user-data-dir=(?:"([^"]+)"|([^\s]+))')
            $observed=if($match.Groups[1].Success){$match.Groups[1].Value}else{$match.Groups[2].Value}
            $observedFull=if($match.Success){[IO.Path]::GetFullPath($observed)}else{''}
            # WebView2 commonly appends EBWebView inside its supplied user-data root.
            $withinProfile=[string]::Equals($observedFull,$webViewDirectory,[StringComparison]::OrdinalIgnoreCase) -or $observedFull.StartsWith($webViewDirectory.TrimEnd('\')+'\',[StringComparison]::OrdinalIgnoreCase)
            if (-not $match.Success -or -not $withinProfile) { throw 'WebView profile override not honored; refusing measurement' }
        }
        $webViewIsolation=[pscustomobject]@{verified=$true;ownedProcessesWithProfile=$profileRows.Count;profileDirectory=$webViewDirectory}
      }
      $systemBefore=@(Get-CimInstance Win32_PerfFormattedData_PerfOS_Processor -Filter "Name='_Total'" | Select-Object PercentProcessorTime)
      $null=Get-BenchmarkSample -State $state
      $sampleClock=[Diagnostics.Stopwatch]::StartNew()
      for ($sampleIndex=1; $sampleIndex -le $Samples; $sampleIndex++) {
        $wait=$sampleIndex*$IntervalMilliseconds-$sampleClock.Elapsed.TotalMilliseconds
        if ($wait -gt 0) { Start-Sleep -Milliseconds ([int]$wait) }
        $sample=Get-BenchmarkSample -State $state
        $sample | Add-Member NoteProperty sampleIndex $sampleIndex
        $sample | Add-Member NoteProperty targetElapsedSeconds ($sampleIndex*$IntervalMilliseconds/1000.0)
        $sample | Add-Member NoteProperty samplingElapsedSeconds $sampleClock.Elapsed.TotalSeconds
        $measurements.Add($sample)
        $sample | ConvertTo-Json -Depth 8 -Compress | Add-Content -LiteralPath (Join-Path $runDirectory 'samples.jsonl') -Encoding utf8
        if ($sampleIndex % 15 -eq 0 -or $sampleIndex -eq $Samples) {
          [pscustomobject]@{run=$runName;phase='sampling';sample=$sampleIndex;total=$Samples;pid=$root.id;elapsed=$launchClock.Elapsed.TotalSeconds} | ConvertTo-Json | Set-Content (Join-Path $OutputDirectory 'progress.json')
        }
      }
      $systemAfter=@(Get-CimInstance Win32_PerfFormattedData_PerfOS_Processor -Filter "Name='_Total'" | Select-Object PercentProcessorTime)
    } catch { $errorText=$_.Exception.Message }
    finally { if ($null -ne $state) { $cleanup=@(Stop-BenchmarkOwnedProcesses -State $state) } }
    $cleanupFailures=@($cleanup | Where-Object result -eq 'error')
    $complete=$null -eq $errorText -and $measurements.Count -eq $Samples -and $cleanupFailures.Count -eq 0 -and @($measurements | Where-Object privateWorkingSetComplete).Count -gt 0 -and @($measurements | Where-Object {$null -ne $_.cpuCorePercent}).Count -gt 0
    $summary=[pscustomobject]@{run=$runName;dataset=$dataset;variant=$name;pair=$pair;complete=$complete;error=$errorText;fixtureSha256=$fixtureHash;exeSha256=$variants[$name].sha256;firstMainWindowObservedMs=$startup;elapsedSeconds=$launchClock.Elapsed.TotalSeconds;root=$(if($state){$state.root}else{$null});windows=$windowInfo;webViewIsolation=$webViewIsolation;systemCpuBefore=$systemBefore;systemCpuAfter=$systemAfter;sampleCount=$measurements.Count;validMemorySamples=@($measurements | Where-Object memoryComplete).Count;validPrivateWorkingSetSamples=@($measurements | Where-Object privateWorkingSetComplete).Count;validCpuSamples=@($measurements | Where-Object {$null -ne $_.cpuCorePercent}).Count;cleanup=$cleanup}
    $summary | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath (Join-Path $runDirectory 'summary.json') -Encoding utf8
    $measurements | Select-Object sampleIndex,elapsedSeconds,samplingElapsedSeconds,collectionSeconds,processCount,memoryComplete,privateWorkingSetComplete,workingSetSumBytes,privateWorkingSetSumBytes,privateCommitSumBytes,cpuCorePercent,cpuMachinePercent,cpuMissingReason,intervalSeconds | Export-Csv -LiteralPath (Join-Path $runDirectory 'samples.csv') -NoTypeInformation -Encoding utf8
    $runSummaries.Add($summary)
    $runSummaries.ToArray() | ConvertTo-Json -Depth 8 | Set-Content (Join-Path $OutputDirectory 'runs.json') -Encoding utf8
    Write-Output "END $runName complete=$complete samples=$($measurements.Count)"
    if (-not $complete) { throw "Run invalid: $runName : $errorText; cleanup failures=$($cleanupFailures.Count)" }
   }
  }
 }
 [pscustomobject]@{phase='complete';runs=$runSummaries.Count;outputDirectory=$OutputDirectory} | ConvertTo-Json | Set-Content (Join-Path $OutputDirectory 'progress.json')
} finally { [Environment]::SetEnvironmentVariable('TIMESCHEDULE_DATA_DIR',$previousEnvironment,'Process'); [Environment]::SetEnvironmentVariable('WEBVIEW2_USER_DATA_FOLDER',$previousWebViewEnvironment,'Process') }
