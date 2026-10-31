$ErrorActionPreference='Stop'
. (Join-Path $PSScriptRoot '..\benchmark-support.ps1')
function Assert($condition,[string]$message) { if (-not $condition) { throw $message } }
function Throws([scriptblock]$operation,[string]$message) { $thrown=$false; try { & $operation | Out-Null } catch { $thrown=$true }; Assert $thrown $message }
$self=Get-Process -Id $PID
$root=Get-BenchmarkRoot -AppProcessId $PID -ExpectedExecutablePath $self.Path -ExpectedStartTime $self.StartTime
Assert ($root.id -eq $PID) 'Correct root rejected'
Throws { Get-BenchmarkRoot -AppProcessId $PID -ExpectedExecutablePath (Join-Path $PSScriptRoot 'not-this.exe') } 'Wrong path accepted'
Throws { Get-BenchmarkRoot -AppProcessId $PID -ExpectedExecutablePath $self.Path -ExpectedStartTime $self.StartTime.AddSeconds(-1) } 'Reused identity accepted'
Throws { Get-BenchmarkRoot -AppProcessId 2147483647 -ExpectedExecutablePath $self.Path } 'Dead root accepted'
$rows=@([pscustomobject]@{key='1/100';workingSetBytes=100;privateWorkingSetBytes=50;privateCommitBytes=200;cpuSeconds=2.0})
$first=Get-BenchmarkAggregate -Rows $rows -Missing @() -Elapsed 1 -Previous $null -LogicalProcessors 4
Assert ($first.workingSetSumBytes -eq 100 -and $null -eq $first.cpuCorePercent) 'First aggregate incorrect'
$previous=[pscustomobject]@{elapsedSeconds=1;memoryComplete=$true;processes=@([pscustomobject]@{key='1/100';cpuSeconds=1.0})}
$second=Get-BenchmarkAggregate -Rows $rows -Missing @() -Elapsed 3 -Previous $previous -LogicalProcessors 4
Assert ($second.cpuCorePercent -eq 50 -and $second.cpuMachinePercent -eq 12.5) 'CPU units incorrect'
$rows[0].privateWorkingSetBytes=$null
$missingCounter=Get-BenchmarkAggregate -Rows $rows -Missing @() -Elapsed 3 -Previous $previous -LogicalProcessors 4
Assert ($null -eq $missingCounter.privateWorkingSetSumBytes -and $missingCounter.workingSetSumBytes -eq 100) 'Missing private counter became zero'
$missingProcess=Get-BenchmarkAggregate -Rows $rows -Missing @('exited') -Elapsed 3 -Previous $previous -LogicalProcessors 4
Assert ($null -eq $missingProcess.workingSetSumBytes -and $null -eq $missingProcess.cpuCorePercent) 'Missing process silently undercounted'
$rows[0].key='1/200'
$churn=Get-BenchmarkAggregate -Rows $rows -Missing @() -Elapsed 3 -Previous $previous -LogicalProcessors 4
Assert ($churn.cpuMissingReason -eq 'process_churn') 'PID reuse hidden in CPU delta'
$when=[datetime]'2026-10-29T00:00:00Z'
$owned=@{a=[pscustomobject]@{id=1;startTicks=$when.ToUniversalTime().Ticks}}
$inventory=@([pscustomobject]@{ProcessId=1;ParentProcessId=0;CreationDate=$when},[pscustomobject]@{ProcessId=2;ParentProcessId=1;CreationDate=$when.AddSeconds(1)},[pscustomobject]@{ProcessId=3;ParentProcessId=2;CreationDate=$when.AddSeconds(2)},[pscustomobject]@{ProcessId=4;ParentProcessId=1;CreationDate=$when.AddSeconds(-1)})
$descendants=@(Get-BenchmarkDescendants -Inventory $inventory -Owned $owned)
Assert ($descendants.Count -eq 3 -and 4 -notin $descendants.ProcessId) 'Genealogy admits older reused-parent process'
$owned.a.startTicks=$when.AddSeconds(10).ToUniversalTime().Ticks
Assert (@(Get-BenchmarkDescendants -Inventory $inventory -Owned $owned).Count -eq 0) 'Dead/reused parent admitted children'
Assert ($null -eq (Get-BenchmarkPrivateWorkingSet -Counter $null -ProcessAgeSeconds 10)) 'Absent private counter accepted'
Assert ($null -eq (Get-BenchmarkPrivateWorkingSet -Counter ([pscustomobject]@{WorkingSetPrivate=500;ElapsedTime=5000}) -ProcessAgeSeconds 10)) 'Reused PID stale performance row accepted'
Assert ((Get-BenchmarkPrivateWorkingSet -Counter ([pscustomobject]@{WorkingSetPrivate=500;ElapsedTime=10}) -ProcessAgeSeconds 10.8) -eq 500) 'Current private counter rejected'
$helper=Start-Process -FilePath $self.Path -ArgumentList @('-NoProfile','-Command','Start-Sleep -Seconds 30') -WindowStyle Hidden -PassThru
$helperRoot=Get-BenchmarkRoot -AppProcessId $helper.Id -ExpectedExecutablePath $self.Path -ExpectedStartTime $helper.StartTime
try {
    $wrong=[pscustomobject]@{id=$helperRoot.id;path=$helperRoot.path;startTicks=$helperRoot.startTicks+1;startUtc=$helperRoot.startUtc;key='wrong';parentId=$null}
    $wrongState=New-BenchmarkState -Root $wrong
    $skipped=@(Stop-BenchmarkOwnedProcesses -State $wrongState)
    Assert ($skipped[0].result -eq 'identity_changed_not_stopped' -and -not $helper.HasExited) 'Cleanup stopped wrong identity'
    $correctState=New-BenchmarkState -Root $helperRoot
    $stopped=@(Stop-BenchmarkOwnedProcesses -State $correctState)
    Assert ($stopped[0].result -eq 'stopped') 'Owned helper cleanup failed'
} finally {
    $still=Get-Process -Id $helperRoot.id -ErrorAction SilentlyContinue
    if ($still -and $still.StartTime.ToUniversalTime().Ticks -eq $helperRoot.startTicks -and $still.Path -eq $helperRoot.path) { $still.Kill(); $null=$still.WaitForExit(5000) }
}
Write-Output 'PASS: root path/start/dead-root, CPU units, missing/stale counters, churn, genealogy and identity-checked helper cleanup.'
