# Read-only telemetry and identity-checked cleanup for processes this runner owns.
Set-StrictMode -Version Latest

function Get-BenchmarkRoot {
    param([int]$AppProcessId, [string]$ExpectedExecutablePath, [datetime]$ExpectedStartTime)
    $expected = [IO.Path]::GetFullPath($ExpectedExecutablePath)
    $process = Get-Process -Id $AppProcessId -ErrorAction Stop
    if (-not [string]::Equals($process.Path, $expected, [StringComparison]::OrdinalIgnoreCase)) { throw "Root executable mismatch for PID $AppProcessId" }
    $start = $process.StartTime.ToUniversalTime()
    if ($ExpectedStartTime -ne [datetime]::MinValue -and $start.Ticks -ne $ExpectedStartTime.ToUniversalTime().Ticks) { throw "Root start time mismatch for PID $AppProcessId" }
    [pscustomobject]@{ id=$AppProcessId; path=$expected; startTicks=$start.Ticks; startUtc=$start.ToString('o'); key="$AppProcessId/$($start.Ticks)"; parentId=$null }
}

function New-BenchmarkState {
    param($Root)
    $owned = @{}; $owned[$Root.key] = $Root
    @{ root=$Root; owned=$owned; previous=$null; clock=[Diagnostics.Stopwatch]::StartNew(); logicalProcessors=[Environment]::ProcessorCount }
}

function Get-BenchmarkDescendants {
    param([array]$Inventory, [hashtable]$Owned)
    # CIM CreationDate is rounded relative to Get-Process ticks. Match at microsecond
    # precision for parent genealogy, then obtain exact process identity below.
    $active = @{}
    foreach ($record in $Owned.Values) {
        foreach ($row in $Inventory) {
            if ([int]$row.ProcessId -eq $record.id -and [math]::Abs(([datetime]$row.CreationDate).ToUniversalTime().Ticks - $record.startTicks) -lt 10) { $active[$record.id]=$row }
        }
    }
    do {
        $added=$false
        foreach ($row in $Inventory) {
            $childId=[int]$row.ProcessId; $parentId=[int]$row.ParentProcessId
            if ($active.ContainsKey($childId) -or -not $active.ContainsKey($parentId)) { continue }
            if (([datetime]$row.CreationDate) -lt ([datetime]$active[$parentId].CreationDate)) { continue }
            $active[$childId]=$row; $added=$true
        }
    } while ($added)
    @($active.Values)
}

function Get-BenchmarkAggregate {
    param([array]$Rows, [array]$Missing, [double]$Elapsed, $Previous, [int]$LogicalProcessors)
    $complete=($Missing.Count -eq 0 -and $Rows.Count -gt 0)
    $privateComplete=$complete -and @($Rows | Where-Object { $null -eq $_.privateWorkingSetBytes }).Count -eq 0
    $memorySum={param($field) if ($complete) { [long](($Rows | Measure-Object -Property $field -Sum).Sum) } else { $null }}
    $cpuCore=$null; $cpuMachine=$null; $cpuReason='first_sample'; $interval=$null
    if ($null -ne $Previous) {
        $interval=$Elapsed-$Previous.elapsedSeconds
        $currentKeys=@($Rows.key | Sort-Object); $oldKeys=@($Previous.processes.key | Sort-Object)
        if (-not $complete -or -not $Previous.memoryComplete) { $cpuReason='missing_process' }
        elseif (($currentKeys -join '|') -ne ($oldKeys -join '|')) { $cpuReason='process_churn' }
        elseif ($interval -le 0) { $cpuReason='invalid_interval' }
        else {
            $delta=0.0
            foreach ($row in $Rows) { $before=$Previous.processes | Where-Object key -eq $row.key; $delta += $row.cpuSeconds-$before.cpuSeconds }
            if ($delta -lt 0) { $cpuReason='counter_regression' }
            else { $cpuCore=100.0*$delta/$interval; $cpuMachine=$cpuCore/$LogicalProcessors; $cpuReason=$null }
        }
    }
    [pscustomobject]@{
        memoryComplete=$complete; privateWorkingSetComplete=$privateComplete
        workingSetSumBytes=(& $memorySum 'workingSetBytes')
        privateWorkingSetSumBytes=$(if ($privateComplete) { [long](($Rows | Measure-Object privateWorkingSetBytes -Sum).Sum) } else { $null })
        privateCommitSumBytes=(& $memorySum 'privateCommitBytes')
        cpuCorePercent=$cpuCore; cpuMachinePercent=$cpuMachine; cpuMissingReason=$cpuReason; intervalSeconds=$interval
    }
}

function Get-BenchmarkPrivateWorkingSet {
    param($Counter, [double]$ProcessAgeSeconds)
    # Performance-provider rows can lag PID reuse. A stale process-age counter
    # must not be accepted as the replacement process's resident private pages.
    if ($null -eq $Counter -or $null -eq $Counter.WorkingSetPrivate -or $null -eq $Counter.ElapsedTime) { return $null }
    if ([math]::Abs([double]$Counter.ElapsedTime-$ProcessAgeSeconds) -gt 2.0) { return $null }
    [long]$Counter.WorkingSetPrivate
}

function Get-BenchmarkSample {
    param([hashtable]$State)
    $sampleClock=[Diagnostics.Stopwatch]::StartNew()
    $null=Get-BenchmarkRoot -AppProcessId $State.root.id -ExpectedExecutablePath $State.root.path -ExpectedStartTime ([datetime]$State.root.startUtc)
    $inventory=@(Get-CimInstance Win32_Process -Property ProcessId,ParentProcessId,CreationDate,ExecutablePath -ErrorAction Stop)
    $candidates=@(Get-BenchmarkDescendants -Inventory $inventory -Owned $State.owned)
    $counterError=$null
    try { $counters=@(Get-CimInstance Win32_PerfFormattedData_PerfProc_Process -Property IDProcess,WorkingSetPrivate,ElapsedTime -ErrorAction Stop) }
    catch { $counters=@(); $counterError=$_.Exception.Message }
    $counterMap=@{}; foreach ($counter in $counters) { $counterMap[[int]$counter.IDProcess]=$counter }
    $rows=[Collections.Generic.List[object]]::new(); $missing=[Collections.Generic.List[object]]::new()
    foreach ($candidate in $candidates) {
        $candidateId=[int]$candidate.ProcessId
        try {
            $process=Get-Process -Id $candidateId -ErrorAction Stop
            $start=$process.StartTime.ToUniversalTime(); $path=$process.Path
            if ([math]::Abs($start.Ticks-([datetime]$candidate.CreationDate).ToUniversalTime().Ticks) -ge 10) { throw 'PID changed after inventory' }
            if ([string]::IsNullOrWhiteSpace($path)) { throw 'Executable path unavailable' }
            $key="$candidateId/$($start.Ticks)"
            $record=[pscustomobject]@{id=$candidateId;path=$path;startTicks=$start.Ticks;startUtc=$start.ToString('o');key=$key;parentId=[int]$candidate.ParentProcessId}
            $State.owned[$key]=$record
            $privateWs=$null
            if ($counterMap.ContainsKey($candidateId)) { $privateWs=Get-BenchmarkPrivateWorkingSet -Counter $counterMap[$candidateId] -ProcessAgeSeconds (([datetime]::UtcNow-$start).TotalSeconds) }
            $process.Refresh()
            if ($process.HasExited -or $process.StartTime.ToUniversalTime().Ticks -ne $start.Ticks) { throw 'Process exited during sample' }
            $rows.Add([pscustomobject]@{id=$candidateId;key=$key;name=$process.ProcessName;path=$path;startUtc=$start.ToString('o');parentId=[int]$candidate.ParentProcessId;workingSetBytes=$process.WorkingSet64;privateWorkingSetBytes=$privateWs;privateCommitBytes=$process.PrivateMemorySize64;cpuSeconds=$process.TotalProcessorTime.TotalSeconds})
        } catch { $missing.Add([pscustomobject]@{id=$candidateId;reason=$_.Exception.Message}) }
    }
    if (@($rows | Where-Object key -eq $State.root.key).Count -ne 1) { throw 'Root absent from sample inventory' }
    $elapsed=$State.clock.Elapsed.TotalSeconds
    $aggregate=Get-BenchmarkAggregate -Rows $rows.ToArray() -Missing $missing.ToArray() -Elapsed $elapsed -Previous $State.previous -LogicalProcessors $State.logicalProcessors
    $sample=[pscustomobject]@{ measuredAt=[datetime]::UtcNow.ToString('o');elapsedSeconds=$elapsed;collectionSeconds=$sampleClock.Elapsed.TotalSeconds;rootProcessId=$State.root.id;processCount=$rows.Count;memoryComplete=$aggregate.memoryComplete;privateWorkingSetComplete=$aggregate.privateWorkingSetComplete;workingSetSumBytes=$aggregate.workingSetSumBytes;privateWorkingSetSumBytes=$aggregate.privateWorkingSetSumBytes;privateCommitSumBytes=$aggregate.privateCommitSumBytes;cpuCorePercent=$aggregate.cpuCorePercent;cpuMachinePercent=$aggregate.cpuMachinePercent;cpuMissingReason=$aggregate.cpuMissingReason;intervalSeconds=$aggregate.intervalSeconds;counterError=$counterError;missing=$missing.ToArray();processes=$rows.ToArray() }
    $State.previous=$sample
    $sample
}

function Stop-BenchmarkOwnedProcesses {
    param([hashtable]$State)
    $outcomes=[Collections.Generic.List[object]]::new()
    $records=@($State.root)+@($State.owned.Values | Where-Object key -ne $State.root.key)
    foreach ($record in $records) {
        try {
            $process=Get-Process -Id $record.id -ErrorAction SilentlyContinue
            if ($null -eq $process) { continue }
            if ($process.StartTime.ToUniversalTime().Ticks -ne $record.startTicks -or -not [string]::Equals($process.Path,$record.path,[StringComparison]::OrdinalIgnoreCase)) { $outcomes.Add([pscustomobject]@{id=$record.id;result='identity_changed_not_stopped'}); continue }
            $process.Kill()
            if (-not $process.WaitForExit(5000)) { throw 'Owned process did not exit within 5s' }
            $outcomes.Add([pscustomobject]@{id=$record.id;result='stopped'})
        } catch { $outcomes.Add([pscustomobject]@{id=$record.id;result='error';error=$_.Exception.Message}) }
    }
    @($outcomes.ToArray())
}

function Initialize-BenchmarkWindowTelemetry {
    if ('BenchmarkWindowTelemetry' -as [type]) { return }
    Add-Type -TypeDefinition @'
using System; using System.Collections.Generic; using System.Runtime.InteropServices;
public static class BenchmarkWindowTelemetry {
 public delegate bool EnumProc(IntPtr hwnd, IntPtr arg);
 [StructLayout(LayoutKind.Sequential)] public struct Rect { public int left,top,right,bottom; }
 public class Row { public long handle,parentHandle; public bool visible,minimized; public int left,top,width,height; public uint dpi; }
 [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc cb,IntPtr arg);
 [DllImport("user32.dll")] static extern bool EnumChildWindows(IntPtr parent,EnumProc cb,IntPtr arg);
 [DllImport("user32.dll")] static extern IntPtr GetParent(IntPtr hwnd);
 [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr hwnd,out uint pid);
 [DllImport("user32.dll")] static extern bool GetWindowRect(IntPtr hwnd,out Rect r);
 [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr hwnd);
 [DllImport("user32.dll")] static extern bool IsIconic(IntPtr hwnd);
 [DllImport("user32.dll")] static extern uint GetDpiForWindow(IntPtr hwnd);
 public static Row[] Read(uint pid) { var rows=new List<Row>(); var seen=new HashSet<IntPtr>(); EnumProc visit=(h,a)=>{ if(!seen.Add(h)) return true; uint p; GetWindowThreadProcessId(h,out p); if(p==pid) { Rect r; if(GetWindowRect(h,out r)) rows.Add(new Row {handle=h.ToInt64(),parentHandle=GetParent(h).ToInt64(),visible=IsWindowVisible(h),minimized=IsIconic(h),left=r.left,top=r.top,width=r.right-r.left,height=r.bottom-r.top,dpi=GetDpiForWindow(h)}); } return true;}; EnumWindows((h,a)=>{visit(h,a); EnumChildWindows(h,visit,IntPtr.Zero); return true;},IntPtr.Zero); return rows.ToArray(); }
}
'@
}

function Get-BenchmarkEnvironment {
    $os=Get-CimInstance Win32_OperatingSystem
    $cpu=Get-CimInstance Win32_Processor
    [pscustomobject]@{timestamp=[datetime]::UtcNow.ToString('o');os=$os.Caption;osVersion=$os.Version;osBuild=$os.BuildNumber;logicalProcessors=[Environment]::ProcessorCount;processorNames=@($cpu.Name);totalMemoryKiB=$os.TotalVisibleMemorySize;freeMemoryKiB=$os.FreePhysicalMemory;powerShell=$PSVersionTable.PSVersion.ToString();note='Other user workloads remain running. No cache flush, power-plan change or UI manipulation.'}
}
