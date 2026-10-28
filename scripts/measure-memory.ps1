param([Parameter(Mandatory = $true)][int]$AppProcessId)
$ErrorActionPreference = 'Stop'
$appProcess = Get-Process -Id $AppProcessId
if ($appProcess.ProcessName -ne 'TimeSchedule') { throw 'Select a running TimeSchedule process.' }
$processInventory = Get-CimInstance Win32_Process
$ownedIds = [System.Collections.Generic.HashSet[int]]::new()
[void]$ownedIds.Add($AppProcessId)
do {
    $added = $false
    foreach ($item in $processInventory) {
        if ($ownedIds.Contains([int]$item.ParentProcessId) -and $ownedIds.Add([int]$item.ProcessId)) { $added = $true }
    }
} while ($added)
$privateCounters = Get-CimInstance Win32_PerfFormattedData_PerfProc_Process
$rows = @(Get-Process -Id @($ownedIds) -ErrorAction SilentlyContinue | ForEach-Object {
    $process = $_
    $counter = $privateCounters | Where-Object { $_.IDProcess -eq $process.Id } | Select-Object -First 1
    [pscustomobject]@{
        id = $process.Id
        name = $process.ProcessName
        workingSetMiB = [math]::Round($process.WorkingSet64 / 1MB, 2)
        privateWorkingSetMiB = if ($null -ne $counter) { [math]::Round($counter.WorkingSetPrivate / 1MB, 2) } else { $null }
        privateCommitMiB = [math]::Round($process.PrivateMemorySize64 / 1MB, 2)
    }
})
[pscustomobject]@{
    measuredAt = (Get-Date).ToString('o')
    rootProcessId = $AppProcessId
    note = 'Includes descendant WebView2 processes. Working set sum double-counts shared pages; private commit is not resident RAM.'
    workingSetSumMiB = [math]::Round(($rows | Measure-Object workingSetMiB -Sum).Sum, 2)
    privateWorkingSetSumMiB = if (@($rows | Where-Object { $null -eq $_.privateWorkingSetMiB }).Count -eq 0) { [math]::Round(($rows | Measure-Object privateWorkingSetMiB -Sum).Sum, 2) } else { $null }
    privateCommitSumMiB = [math]::Round(($rows | Measure-Object privateCommitMiB -Sum).Sum, 2)
    processes = $rows
} | ConvertTo-Json -Depth 4
