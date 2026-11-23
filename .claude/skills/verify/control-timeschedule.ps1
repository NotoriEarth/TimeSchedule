# Drives an isolated TimeSchedule instance for verification.
# Keep this file ASCII-only: Windows PowerShell 5.1 reads a BOM-less script in the
# system code page, so literal Korean here would be mangled.
param(
    [Parameter(Mandatory = $true, Position = 0)]
    [ValidateSet('launch', 'doctor', 'mcp', 'cli', 'windows', 'screenshot', 'click', 'type', 'key', 'stop')]
    [string]$Command,
    [string]$RunDir,
    [string]$Exe,
    [switch]$ShowWidget,
    [string]$Tool,
    [string]$Arguments,
    [string]$ArgumentsFile,
    [string[]]$CliArguments,
    [ValidateSet('diary', 'quick', 'jump')][string]$Window = 'diary',
    [string]$Name,
    [int]$X = -1,
    [int]$Y = -1,
    [switch]$Double,
    [string]$Text,
    [string]$TextFile,
    [ValidateSet('Enter', 'Escape', 'Tab', 'Backspace', 'Delete', 'Left', 'Right', 'Up', 'Down', 'Home', 'End')][string]$Key,
    [switch]$KeepData
)
$ErrorActionPreference = 'Stop'
$previousEncoding = [Console]::OutputEncoding
$previousDpi = 0
try {
[Console]::OutputEncoding = New-Object Text.UTF8Encoding $false
$repoRoot = Split-Path -Parent (Split-Path -Parent (Split-Path -Parent $PSScriptRoot))
$runsRoot = Join-Path $repoRoot '.test-data\verify'
$utf8 = New-Object Text.UTF8Encoding $false
# PowerShell's location and the process working directory can differ. Resolve
# every path argument once, the PowerShell way, and use the absolute path after.
foreach ($pathArgument in 'RunDir', 'Exe', 'ArgumentsFile', 'TextFile') {
    $given = Get-Variable -Name $pathArgument -ValueOnly
    if ($given) { Set-Variable -Name $pathArgument -Value $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($given) }
}

Add-Type -AssemblyName System.Security
Add-Type -AssemblyName System.Drawing
# Bump the number in this name whenever the C# below changes: a PowerShell
# session that already loaded the old type would otherwise keep running it.
if (-not ('TsVerifyNative2' -as [type])) {
    Add-Type -ReferencedAssemblies System.Drawing -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.Drawing;
using System.Drawing.Imaging;
using System.Runtime.InteropServices;
using System.Text;
public class TsVerifyWindow2 {
    public long Handle; public string Title; public string ClassName;
    public bool Visible; public bool Minimized; public int ClientWidth; public int ClientHeight; public int Left; public int Top;
}
public static class TsVerifyNative2 {
    delegate bool EnumProc(IntPtr h, IntPtr l);
    [StructLayout(LayoutKind.Sequential)] struct RECT { public int L, T, R, B; }
    [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc cb, IntPtr l);
    [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
    [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll")] static extern bool IsIconic(IntPtr h);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int GetWindowTextW(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int GetClassNameW(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll")] static extern bool GetClientRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] static extern bool GetWindowRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] static extern bool PrintWindow(IntPtr h, IntPtr dc, uint flags);
    [DllImport("user32.dll")] static extern uint MapVirtualKeyW(uint code, uint type);
    [DllImport("user32.dll")] static extern IntPtr SetThreadDpiAwarenessContext(IntPtr ctx);

    // Per-monitor v2, so client sizes and coordinates are the app's physical pixels.
    // Returns the caller's context, to be put back with LeavePixels.
    public static long EnterPixels() { return SetThreadDpiAwarenessContext(new IntPtr(-4)).ToInt64(); }
    public static void LeavePixels(long previous) { if (previous != 0) SetThreadDpiAwarenessContext(new IntPtr(previous)); }

    public static List<TsVerifyWindow2> Windows(uint pid) {
        var found = new List<TsVerifyWindow2>();
        EnumWindows((h, l) => {
            uint owner; GetWindowThreadProcessId(h, out owner);
            if (owner != pid) return true;
            var title = new StringBuilder(512); GetWindowTextW(h, title, 512);
            var cls = new StringBuilder(256); GetClassNameW(h, cls, 256);
            RECT c, w; GetClientRect(h, out c); GetWindowRect(h, out w);
            found.Add(new TsVerifyWindow2 { Handle = h.ToInt64(), Title = title.ToString(), ClassName = cls.ToString(),
                Visible = IsWindowVisible(h), Minimized = IsIconic(h), ClientWidth = c.R - c.L, ClientHeight = c.B - c.T, Left = w.L, Top = w.T });
            return true;
        }, IntPtr.Zero);
        return found;
    }
    // Client area only (1) with full content (2): pixels match click coordinates.
    public static bool Capture(long handle, string path) {
        var h = new IntPtr(handle); RECT c; GetClientRect(h, out c);
        int width = c.R - c.L, height = c.B - c.T;
        if (width <= 0 || height <= 0) return false;
        using (var bitmap = new Bitmap(width, height, PixelFormat.Format24bppRgb)) {
            bool ok;
            using (var g = Graphics.FromImage(bitmap)) { var dc = g.GetHdc(); ok = PrintWindow(h, dc, 3); g.ReleaseHdc(dc); }
            if (ok) bitmap.Save(path, ImageFormat.Png);
            return ok;
        }
    }
    [StructLayout(LayoutKind.Sequential)] struct POINT { public int X, Y; }
    [StructLayout(LayoutKind.Sequential)] struct MOUSEINPUT { public int dx, dy; public uint mouseData, dwFlags, time; public IntPtr dwExtraInfo; }
    [StructLayout(LayoutKind.Sequential)] struct KEYBDINPUT { public ushort wVk, wScan; public uint dwFlags, time; public IntPtr dwExtraInfo; }
    [StructLayout(LayoutKind.Explicit)] struct InputUnion { [FieldOffset(0)] public MOUSEINPUT mi; [FieldOffset(0)] public KEYBDINPUT ki; }
    [StructLayout(LayoutKind.Sequential)] struct INPUT { public uint type; public InputUnion u; }
    [DllImport("user32.dll")] static extern uint SendInput(uint count, INPUT[] inputs, int size);
    [DllImport("user32.dll")] static extern bool ClientToScreen(IntPtr h, ref POINT p);
    [DllImport("user32.dll")] static extern IntPtr WindowFromPoint(POINT p);
    [DllImport("user32.dll")] static extern IntPtr GetAncestor(IntPtr h, uint flags);
    [DllImport("user32.dll")] static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] static extern bool SetForegroundWindow(IntPtr h);
    [DllImport("user32.dll")] static extern bool BringWindowToTop(IntPtr h);
    [DllImport("user32.dll")] static extern bool ShowWindow(IntPtr h, int command);
    [DllImport("user32.dll")] static extern bool AttachThreadInput(uint from, uint to, bool attach);
    [DllImport("user32.dll")] static extern int GetSystemMetrics(int index);
    [DllImport("kernel32.dll")] static extern uint GetCurrentThreadId();

    // One call queues its events together; no other input lands between them.
    static bool Send(params INPUT[] inputs) { return SendInput((uint)inputs.Length, inputs, Marshal.SizeOf(typeof(INPUT))) == (uint)inputs.Length; }
    static INPUT Mouse(uint flags) { var i = new INPUT { type = 0 }; i.u.mi.dwFlags = flags; return i; }
    // Absolute position on the virtual desktop, in the 0..65535 units SendInput expects.
    static INPUT MoveTo(POINT p) {
        int left = GetSystemMetrics(76), top = GetSystemMetrics(77), width = GetSystemMetrics(78), height = GetSystemMetrics(79);
        var i = Mouse(0x0001 | 0x8000 | 0x4000);
        i.u.mi.dx = (int)Math.Floor((p.X - left + 0.5) * 65536.0 / width);
        i.u.mi.dy = (int)Math.Floor((p.Y - top + 0.5) * 65536.0 / height);
        return i;
    }
    static INPUT Keyboard(ushort vk, ushort scan, uint flags) { var i = new INPUT { type = 1 }; i.u.ki.wVk = vk; i.u.ki.wScan = scan; i.u.ki.dwFlags = flags; return i; }

    // A minimized window has no client area to capture or click. Restores it without activating it.
    public static void Restore(long handle) {
        var h = new IntPtr(handle);
        if (IsIconic(h)) { ShowWindow(h, 4); System.Threading.Thread.Sleep(400); }
    }
    // Real input goes to the foreground window, so the target must be it. Nothing
    // is sent to whatever is in front to get there: when Windows refuses, so do we.
    public static bool Focus(long handle) {
        var h = new IntPtr(handle);
        if (GetForegroundWindow() == h) return true;
        if (IsIconic(h)) ShowWindow(h, 9);
        uint ignored; uint front = GetWindowThreadProcessId(GetForegroundWindow(), out ignored); uint self = GetCurrentThreadId();
        AttachThreadInput(self, front, true);
        BringWindowToTop(h); SetForegroundWindow(h);
        AttachThreadInput(self, front, false);
        System.Threading.Thread.Sleep(250);
        return GetForegroundWindow() == h;
    }
    const string NoFocus = "Windows did not give the window the foreground; click the TimeSchedule verification window once and retry";
    // Moves the real pointer and clicks, as one queued burst. Returns why it did not, or null.
    public static string Click(long handle, int x, int y, int times) {
        var h = new IntPtr(handle);
        if (!Focus(handle)) return NoFocus;
        var p = new POINT { X = x, Y = y }; ClientToScreen(h, ref p);
        if (GetForegroundWindow() != h || GetAncestor(WindowFromPoint(p), 2) != h) return "another window covers that point";
        var burst = new List<INPUT> { MoveTo(p) };
        for (int i = 0; i < times; i++) { burst.Add(Mouse(0x0002)); burst.Add(Mouse(0x0004)); }
        return Send(burst.ToArray()) ? null : "Windows blocked the input";
    }
    public static string Key(long handle, ushort vk, bool extended) {
        if (!Focus(handle)) return NoFocus;
        ushort scan = (ushort)MapVirtualKeyW(vk, 0); uint flags = extended ? 1u : 0u;
        if (GetForegroundWindow() != new IntPtr(handle)) return "the window lost the foreground";
        return Send(Keyboard(vk, scan, flags), Keyboard(vk, scan, flags | 2)) ? null : "Windows blocked the input";
    }
    // Unicode input: the characters arrive as typed text whatever the keyboard layout or IME state.
    public static string TypeText(long handle, string text) {
        if (!Focus(handle)) return NoFocus;
        foreach (char ch in text) {
            if (GetForegroundWindow() != new IntPtr(handle)) return "the window lost the foreground while typing";
            if (!Send(Keyboard(0, ch, 4), Keyboard(0, ch, 4 | 2))) return "Windows blocked the input";
            System.Threading.Thread.Sleep(15);
        }
        return null;
    }
}
'@
}
$previousDpi = [TsVerifyNative2]::EnterPixels()

function Write-Result($value, [int]$exitCode = 0) {
    ConvertTo-Json -InputObject $value -Depth 8
    exit $exitCode
}
function Test-Redirected([string]$path) {
    (Test-Path -LiteralPath $path) -and [bool]((Get-Item -LiteralPath $path -Force).Attributes -band [IO.FileAttributes]::ReparsePoint)
}
# A link anywhere between the repository and the run's files would point these
# commands, and stop's delete, at someone else's folder.
function Assert-NotRedirected([string[]]$paths) {
    foreach ($path in $paths) { if (Test-Redirected $path) { throw "$path is a link. Refusing to use this run." } }
}
# The run that launch created under .test-data\verify. Folders come from -RunDir,
# never from the file: stop deletes the data folder.
function Get-Instance([switch]$AllowStopped) {
    if (-not $RunDir) { throw 'Pass -RunDir with the runDir that launch printed.' }
    $root = [IO.Path]::GetFullPath($RunDir).TrimEnd('\')
    if (-not [string]::Equals((Split-Path -Parent $root), $runsRoot, [StringComparison]::OrdinalIgnoreCase)) { throw "A run directory is a folder directly under $runsRoot." }
    $dataDir = Join-Path $root 'data'
    $evidenceDir = Join-Path $root 'evidence'
    Assert-NotRedirected (Split-Path -Parent $runsRoot), $runsRoot, $root, $dataDir, $evidenceDir, (Join-Path $dataDir 'mcp-token.dpapi'), (Join-Path $dataDir 'mcp-status.json'), (Join-Path $root 'instance.json'), (Join-Path $root 'instance.stopped.json')
    $file = Join-Path $root 'instance.json'
    $stopped = $false
    if (-not (Test-Path -LiteralPath $file)) {
        $file = Join-Path $root 'instance.stopped.json'
        if (-not (Test-Path -LiteralPath $file)) { throw "No instance.json in $root. This run directory was not created by launch." }
        if (-not $AllowStopped) { throw 'This run was already stopped. Launch a new one.' }
        $stopped = $true
    }
    $instance = [IO.File]::ReadAllText($file, $utf8) | ConvertFrom-Json
    $instance.runDir = $root
    $instance.dataDir = $dataDir
    $instance.evidenceDir = $evidenceDir
    $instance | Add-Member NoteProperty stopped $stopped -Force
    $instance
}
# The process launch started, or $null when it is gone: same PID, same start time,
# same EXE. The handle is taken first and kept, so the identity that is checked
# and the process that stop ends are the same one even if the PID is reused.
function Get-OwnedProcess($instance) {
    $process = Get-Process -Id $instance.pid -ErrorAction SilentlyContinue
    if (-not $process) { return $null }
    try { $null = $process.Handle } catch { if ($process.HasExited) { return $null }; throw "PID $($instance.pid) exists but cannot be opened. Nothing was changed." }
    if ($process.HasExited) { return $null }
    if ($process.StartTime.ToUniversalTime().Ticks -ne [long]$instance.startTicks) { return $null }
    $running = $null
    try { $running = $process.MainModule.FileName } catch { }
    # A 32-bit PowerShell cannot read a 64-bit process. Unknown is not "gone".
    if (-not $running) { throw "PID $($instance.pid) started when the run did, but its EXE cannot be read from this PowerShell (use 64-bit Windows PowerShell). Nothing was changed." }
    if (-not [string]::Equals($running, $instance.exe, [StringComparison]::OrdinalIgnoreCase)) { return $null }
    $process
}
function Invoke-Http([string]$Method, [string]$Uri, [hashtable]$Headers, [byte[]]$Body) {
    # Whatever answers on the port gets a megabyte and twelve seconds in all.
    $clock = [Diagnostics.Stopwatch]::StartNew()
    $request = [Net.HttpWebRequest]::Create($Uri)
    $request.Method = $Method; $request.AllowAutoRedirect = $false; $request.Timeout = 12000; $request.ReadWriteTimeout = 12000; $request.Proxy = $null
    foreach ($name in $Headers.Keys) {
        if ($name -eq 'Accept') { $request.Accept = $Headers[$name] }
        elseif ($name -eq 'Content-Type') { $request.ContentType = $Headers[$name] }
        else { $request.Headers[$name] = $Headers[$name] }
    }
    if ($Body) { $request.ContentLength = $Body.Length; $stream = $request.GetRequestStream(); $stream.Write($Body, 0, $Body.Length); $stream.Close() }
    try { $response = $request.GetResponse() }
    catch {
        $web = $_.Exception
        while ($web -and -not ($web -is [Net.WebException])) { $web = $web.InnerException }
        if (-not $web -or -not $web.Response) { throw }
        $response = $web.Response
    }
    $buffer = New-Object IO.MemoryStream
    $chunk = New-Object byte[] 8192
    $reader = $response.GetResponseStream()
    try {
        while ($true) {
            $reader.ReadTimeout = [Math]::Max(1, 12000 - [int]$clock.ElapsedMilliseconds)
            $read = $reader.Read($chunk, 0, $chunk.Length)
            if ($clock.ElapsedMilliseconds -gt 12000) { throw 'The reply was too slow.' }
            if ($read -le 0) { break }
            $buffer.Write($chunk, 0, $read)
            if ($buffer.Length -gt 1MB) { throw 'The reply was too large.' }
        }
        $status = [int]$response.StatusCode
    } finally { $response.Close() }
    [pscustomobject]@{ status = $status; text = $utf8.GetString($buffer.ToArray()) }
}
# The token of the isolated instance. Never printed, never written to evidence.
function Read-Token([string]$dataDir) {
    $plain = [Security.Cryptography.ProtectedData]::Unprotect([IO.File]::ReadAllBytes((Join-Path $dataDir 'mcp-token.dpapi')), $null, [Security.Cryptography.DataProtectionScope]::CurrentUser)
    try { $token = $utf8.GetString($plain) } finally { [Array]::Clear($plain, 0, $plain.Length) }
    if ($token -cnotmatch '^[0-9a-f]{64}$') { throw 'The MCP credential file of the run is not a token.' }
    $token
}
# The run's token when the listener on the port proves it holds that same token
# (the token is not sent to find out), otherwise $null.
function Get-ProvenToken([string]$dataDir, [int]$port) {
    try {
        $token = Read-Token $dataDir
        $hostName = "127.0.0.1:$port"
        $challenge = [Guid]::NewGuid().ToString('N') + [Guid]::NewGuid().ToString('N')
        $hmac = New-Object Security.Cryptography.HMACSHA256 (, $utf8.GetBytes($token))
        try { $expected = -join ($hmac.ComputeHash($utf8.GetBytes("TimeSchedule MCP server proof v1`n$hostName`n$challenge")) | ForEach-Object { $_.ToString('x2') }) } finally { $hmac.Dispose() }
        $answer = Invoke-Http 'GET' "http://$hostName/proof" @{ 'TimeSchedule-Challenge' = $challenge } $null
        if ($answer.status -eq 200 -and $answer.text.Trim() -ceq $expected) { return $token }
    } catch { }
    $null
}
function Test-Proof([string]$dataDir, [int]$port) { $null -ne (Get-ProvenToken $dataDir $port) }
# Evidence goes to a plain file of the run, never through a link placed there.
function Get-EvidenceFile($instance, [string]$name, [switch]$New) {
    $file = Join-Path $instance.evidenceDir $name
    Assert-NotRedirected $instance.evidenceDir, $file
    if ($New -and (Test-Path -LiteralPath $file)) { throw "$name already exists in the evidence folder. Use a new name." }
    $file
}
# One line of strict JSON per call, whatever the tool or the caller supplied.
function Add-Transcript($instance, [string]$name, $entry) {
    [IO.File]::AppendAllText((Get-EvidenceFile $instance $name), (ConvertTo-Json -InputObject $entry -Compress -Depth 20) + "`n", $utf8)
}
function Read-Status([string]$dataDir) {
    $file = Join-Path $dataDir 'mcp-status.json'
    if (-not (Test-Path -LiteralPath $file)) { return $null }
    try { [IO.File]::ReadAllText($file, $utf8) | ConvertFrom-Json } catch { $null }
}
function Get-Port($status) {
    $uri = $null
    if (-not $status -or -not [Uri]::TryCreate([string]$status.endpoint, [UriKind]::Absolute, [ref]$uri)) { return 0 }
    if ($uri.Scheme -cne 'http' -or $uri.Host -cne '127.0.0.1' -or $uri.AbsolutePath -cne '/mcp') { return 0 }
    $uri.Port
}
function Get-TitlePart([string]$role) {
    # diary: "man-nyeon daieori", quick: "ppareun iljeong", jump: "yeon-wol idong" (Korean, as code points).
    $codes = switch ($role) {
        'diary' { 0xB9CC, 0xB144, 0x20, 0xB2E4, 0xC774, 0xC5B4, 0xB9AC }
        'quick' { 0xBE60, 0xB978, 0x20, 0xC77C, 0xC815 }
        'jump' { 0xC5F0, 0xB7, 0xC6D4, 0x20, 0xC774, 0xB3D9 }
    }
    -join ($codes | ForEach-Object { [char]$_ })
}
function Get-TargetWindow($instance) {
    if (-not (Get-OwnedProcess $instance)) { throw 'The instance this run started is not running.' }
    $part = Get-TitlePart $Window
    $match = @([TsVerifyNative2]::Windows([uint32]$instance.pid) | Where-Object { $_.Visible -and $_.Title.Contains($part) })
    if ($match.Count -ne 1) { throw "Expected one visible '$Window' window of the instance, found $($match.Count). Run the windows command." }
    if ($match[0].Minimized) {
        [TsVerifyNative2]::Restore($match[0].Handle)
        $match = @([TsVerifyNative2]::Windows([uint32]$instance.pid) | Where-Object { $_.Handle -eq $match[0].Handle })
    }
    $match[0]
}
# The newest file the EXE is built from: sources, manifests and embedded documents
# of the app, the shared crates and the vendored crates, without test sources.
function Get-NewestSource {
    $roots = 'apps\timeschedule-native', 'crates', 'vendor' | ForEach-Object { Join-Path $repoRoot $_ } | Where-Object { Test-Path -LiteralPath $_ }
    $files = @($roots | ForEach-Object { Get-ChildItem -LiteralPath $_ -Recurse -File } |
        Where-Object { ('.rs', '.toml' -contains $_.Extension -or $_.Name -eq 'README.md') -and $_.FullName -notmatch '\\(tests|target|examples|benches)\\' -and $_.Name -notmatch '(^|_)tests\.rs$' })
    foreach ($single in 'Cargo.lock', 'Cargo.toml', 'src-tauri\app.manifest') {
        $file = Join-Path $repoRoot $single
        if (Test-Path -LiteralPath $file) { $files += Get-Item -LiteralPath $file }
    }
    $files | Sort-Object LastWriteTimeUtc -Descending | Select-Object -First 1
}

switch ($Command) {
    'launch' {
        if (-not $Exe) { $Exe = Join-Path $repoRoot 'target\release\timeschedule-native.exe' }
        if (-not (Test-Path -LiteralPath $Exe)) { throw "Build first: $Exe is missing." }
        $Exe = (Resolve-Path -LiteralPath $Exe).Path
        if ($RunDir) { throw 'launch chooses the run directory itself. Pass -RunDir to the other commands.' }
        $RunDir = Join-Path $runsRoot ([datetime]::Now.ToString('yyyyMMdd-HHmmss') + '-' + [Guid]::NewGuid().ToString('N').Substring(0, 4))
        if (Test-Path -LiteralPath $RunDir) { throw 'Run directory already exists; every launch gets a new one.' }
        Assert-NotRedirected (Split-Path -Parent $runsRoot), $runsRoot
        New-Item -ItemType Directory -Path $runsRoot -Force | Out-Null
        Assert-NotRedirected (Split-Path -Parent $runsRoot), $runsRoot
        $exeSha256 = (Get-FileHash -LiteralPath $Exe).Hash
        $exeVersion = (Get-Item -LiteralPath $Exe).VersionInfo.FileVersion
        $dataDir = Join-Path $RunDir 'data'
        $evidenceDir = Join-Path $RunDir 'evidence'
        New-Item -ItemType Directory -Path $dataDir, $evidenceDir | Out-Null
        # A fresh data folder shows a second calendar on the user's desktop unless told otherwise.
        if (-not $ShowWidget) { [IO.File]::WriteAllText((Join-Path $dataDir 'preferences.json'), '{"widgetVisible":false}', $utf8) }
        $listener = New-Object Net.Sockets.TcpListener ([Net.IPAddress]::Loopback, 0)
        $listener.Start(); $port = $listener.LocalEndpoint.Port; $listener.Stop()
        $saved = @{}
        foreach ($name in 'TIMESCHEDULE_DATA_DIR', 'TIMESCHEDULE_MCP_PORT', 'TIMESCHEDULE_MCP_DISABLED') { $saved[$name] = [Environment]::GetEnvironmentVariable($name, 'Process') }
        try {
            [Environment]::SetEnvironmentVariable('TIMESCHEDULE_DATA_DIR', $dataDir, 'Process')
            [Environment]::SetEnvironmentVariable('TIMESCHEDULE_MCP_PORT', [string]$port, 'Process')
            [Environment]::SetEnvironmentVariable('TIMESCHEDULE_MCP_DISABLED', $null, 'Process')
            $process = Start-Process -FilePath $Exe -PassThru
        } finally { foreach ($name in $saved.Keys) { [Environment]::SetEnvironmentVariable($name, $saved[$name], 'Process') } }
        # Written before the wait, so stop can clean up a launch that never became ready.
        # Without the record nothing could stop the child later, so it does not outlive a failed write.
        try {
            $instance = [pscustomobject]@{
                pid = $process.Id; startTicks = $process.StartTime.ToUniversalTime().Ticks; exe = $Exe
                exeSha256 = $exeSha256; exeVersion = $exeVersion
                runDir = $RunDir; dataDir = $dataDir; evidenceDir = $evidenceDir; port = $port; widgetShown = [bool]$ShowWidget
            }
            [IO.File]::WriteAllText((Join-Path $RunDir 'instance.json'), ($instance | ConvertTo-Json), $utf8)
        } catch {
            try { $process.Kill() } catch { }
            throw "The run could not be recorded in $RunDir, so the instance was stopped again. $($_.Exception.Message)"
        }
        $ready = $false; $reason = 'the MCP status file did not appear'
        $deadline = [datetime]::UtcNow.AddSeconds(40)
        while ([datetime]::UtcNow -lt $deadline) {
            if ($process.HasExited) { $reason = "the app exited with code $($process.ExitCode)"; break }
            Assert-NotRedirected $RunDir, $dataDir, (Join-Path $dataDir 'mcp-status.json'), (Join-Path $dataDir 'mcp-token.dpapi')
            $status = Read-Status $dataDir
            if ($status -and [int]$status.pid -eq $process.Id -and (Get-Port $status) -eq $port) {
                if (Test-Proof $dataDir $port) {
                    $shown = @([TsVerifyNative2]::Windows([uint32]$process.Id) | Where-Object { $_.Visible -and $_.Title.Contains((Get-TitlePart 'diary')) })
                    if ($shown.Count -eq 1) { $ready = $true; break }
                    $reason = 'the diary window is not visible'
                } else { $reason = 'the listener did not prove it holds the run token' }
            }
            Start-Sleep -Milliseconds 250
        }
        if (-not $ready) { Write-Result ([pscustomobject]@{ ready = $false; reason = $reason; runDir = $RunDir; next = 'run stop with this runDir' }) 1 }
        Write-Result ([pscustomobject]@{ ready = $true; runDir = $RunDir; pid = $process.Id; endpoint = "http://127.0.0.1:$port/mcp"; exeVersion = $instance.exeVersion; evidenceDir = $evidenceDir })
    }
    'doctor' {
        $instance = Get-Instance
        $checks = New-Object Collections.Generic.List[object]
        $add = { param($name, $ok, $detail) $checks.Add([pscustomobject]@{ check = $name; ok = [bool]$ok; detail = [string]$detail }) }
        $process = Get-OwnedProcess $instance
        & $add 'process started by this run is alive' ($null -ne $process) "pid $($instance.pid)"
        $status = Read-Status $instance.dataDir
        & $add 'MCP status file belongs to that process' ($status -and [int]$status.pid -eq [int]$instance.pid -and (Get-Port $status) -eq [int]$instance.port) "port $($instance.port)"
        # The run's credential is read only for a listener that is this run's process.
        $mine = $checks[0].ok -and $checks[1].ok
        & $add 'listener proves it holds the run token' ($mine -and (Test-Proof $instance.dataDir ([int]$instance.port))) $(if ($mine) { '/proof' } else { 'skipped: no status file from the run process' })
        $default = Join-Path $env:LOCALAPPDATA 'com.timeschedule.personal.native-experiment'
        $inside = $instance.dataDir.StartsWith($instance.runDir.TrimEnd('\') + '\', [StringComparison]::OrdinalIgnoreCase)
        & $add 'data folder is the run folder, not the user data folder' ($inside -and -not [string]::Equals($instance.dataDir, $default, [StringComparison]::OrdinalIgnoreCase)) $instance.dataDir
        $log = Join-Path $instance.dataDir 'native.log'
        $started = (Test-Path -LiteralPath $log) -and [bool](Select-String -LiteralPath $log -Pattern 'startup version=\S+ .* storage=true' -Quiet)
        & $add 'storage opened at startup' $started 'native.log'
        $sourceVersion = ([regex]::Match([IO.File]::ReadAllText((Join-Path $repoRoot 'apps\timeschedule-native\Cargo.toml')), '(?m)^version\s*=\s*"([^"]+)"')).Groups[1].Value
        $runningVersion = $(if ($process) { $process.MainModule.FileVersionInfo.FileVersion } else { 'not running' })
        & $add 'running version is the source version' ($runningVersion -ceq $sourceVersion) "running $runningVersion, source $sourceVersion"
        $exists = Test-Path -LiteralPath $instance.exe
        & $add 'EXE on disk is the one that was launched' ($exists -and (Get-FileHash -LiteralPath $instance.exe).Hash -eq $instance.exeSha256) $instance.exe
        $newest = Get-NewestSource
        & $add 'EXE is not older than the source' ($exists -and (Get-Item -LiteralPath $instance.exe).LastWriteTimeUtc -ge $newest.LastWriteTimeUtc) "newest source: $($newest.FullName)"
        $diary = @()
        if ($process) { $diary = @([TsVerifyNative2]::Windows([uint32]$instance.pid) | Where-Object { $_.Visible -and $_.Title.Contains((Get-TitlePart 'diary')) }) }
        & $add 'diary window is visible' ($diary.Count -eq 1) "$($diary.Count) window(s)"
        $ok = @($checks | Where-Object { -not $_.ok }).Count -eq 0
        Write-Result ([pscustomobject]@{ ok = $ok; runDir = $instance.runDir; checks = $checks }) $(if ($ok) { 0 } else { 1 })
    }
    'mcp' {
        $instance = Get-Instance
        if (-not $Tool) { throw 'Pass -Tool (calendar_list, calendar_get, calendar_create, calendar_update).' }
        if ($ArgumentsFile) { $Arguments = [IO.File]::ReadAllText($ArgumentsFile, $utf8) }
        if ([string]::IsNullOrWhiteSpace($Arguments)) { $Arguments = '{}' }
        # Windows PowerShell reads more than JSON (single quotes, for one). What is
        # sent and recorded is what it understood, written back as strict JSON.
        $given = $Arguments | ConvertFrom-Json
        if (-not ($given -is [Management.Automation.PSCustomObject])) { throw '-Arguments must be a JSON object.' }
        $Arguments = ConvertTo-Json -InputObject $given -Compress -Depth 20
        if (-not (Get-OwnedProcess $instance)) { throw 'The instance this run started is not running.' }
        $token = Get-ProvenToken $instance.dataDir ([int]$instance.port)
        if (-not $token) { throw 'The listener on the run port did not prove it is this run. The credential was not sent.' }
        $body = '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":' + (ConvertTo-Json -InputObject $Tool) + ',"arguments":' + $Arguments + '}}'
        $headers = @{ Authorization = "Bearer $token"; Accept = 'application/json, text/event-stream'; 'MCP-Protocol-Version' = '2025-11-25'; 'Content-Type' = 'application/json' }
        $entry = [ordered]@{ time = [datetime]::Now.ToString('o'); tool = $Tool; arguments = $given; httpStatus = 0; response = $null }
        try { $reply = Invoke-Http 'POST' "http://127.0.0.1:$($instance.port)/mcp" $headers $utf8.GetBytes($body) }
        catch {
            # No reply does not mean nothing happened: a write may have been stored.
            $entry.response = "no reply, outcome unknown: $($_.Exception.Message)"
            Add-Transcript $instance 'mcp-transcript.jsonl' $entry
            throw
        } finally { $token = $null; $headers = $null }
        $parsed = $null
        if (-not [string]::IsNullOrWhiteSpace($reply.text)) { try { $parsed = $reply.text | ConvertFrom-Json } catch { } }
        $entry.httpStatus = $reply.status
        # A reply that is not JSON is kept as the text it was.
        $entry.response = $(if ($parsed -is [Management.Automation.PSCustomObject]) { $parsed } else { [string]$reply.text })
        Add-Transcript $instance 'mcp-transcript.jsonl' $entry
        $reply.text
        # Success is this request's JSON-RPC result. A refused tool call still answers 200; its result says isError.
        $result = $(if ($parsed -is [Management.Automation.PSCustomObject]) { $parsed.result } else { $null })
        $succeeded = $reply.status -eq 200 -and $parsed.jsonrpc -ceq '2.0' -and $parsed.id -eq 1 -and -not $parsed.error -and
            ($result -is [Management.Automation.PSCustomObject]) -and $null -ne $result.content -and -not $result.isError
        exit $(if ($succeeded) { 0 } else { 1 })
    }
    'cli' {
        # The EXE's own command line, against the run's data folder and nothing else.
        $instance = Get-Instance
        if (-not $CliArguments) { throw "Pass -CliArguments, for example -CliArguments record,drain" }
        if ($CliArguments -contains '--data-dir') { throw 'The helper adds --data-dir itself.' }
        if (-not (Get-OwnedProcess $instance)) { throw 'The instance this run started is not running.' }
        $quote = { param($value) '"' + ($value -replace '(\\*)"', '$1$1\"' -replace '(\\+)$', '$1$1') + '"' }
        $start = New-Object Diagnostics.ProcessStartInfo $instance.exe
        $start.Arguments = (@($CliArguments) + '--data-dir', $instance.dataDir | ForEach-Object { & $quote $_ }) -join ' '
        $start.UseShellExecute = $false; $start.RedirectStandardOutput = $true; $start.RedirectStandardError = $true; $start.CreateNoWindow = $true
        $start.StandardOutputEncoding = $utf8; $start.StandardErrorEncoding = $utf8
        $child = [Diagnostics.Process]::Start($start)
        $stdout = $child.StandardOutput.ReadToEndAsync(); $stderr = $child.StandardError.ReadToEndAsync()
        if (-not $child.WaitForExit(60000)) { try { $child.Kill() } catch { }; throw 'The command did not finish in 60 seconds and was stopped.' }
        $child.WaitForExit()
        $entry = [ordered]@{ time = [datetime]::Now.ToString('o'); arguments = @($CliArguments); exitCode = $child.ExitCode; stdout = $stdout.Result; stderr = $stderr.Result }
        Add-Transcript $instance 'cli-transcript.jsonl' $entry
        Write-Result ([pscustomobject]$entry) $(if ($child.ExitCode -eq 0) { 0 } else { 1 })
    }
    'windows' {
        $instance = Get-Instance
        if (-not (Get-OwnedProcess $instance)) { throw 'The instance this run started is not running.' }
        Write-Result ([pscustomobject]@{ pid = $instance.pid; windows = @([TsVerifyNative2]::Windows([uint32]$instance.pid) | Where-Object { $_.Title }) })
    }
    'screenshot' {
        $instance = Get-Instance
        if (-not $Name -or $Name -notmatch '^[A-Za-z0-9._-]+$') { throw 'Pass -Name with letters, digits, dot, dash or underscore.' }
        $target = Get-TargetWindow $instance
        $file = Get-EvidenceFile $instance "$Name.png" -New
        if (-not [TsVerifyNative2]::Capture($target.Handle, $file)) { throw 'The window could not be captured.' }
        # The title is the same for every TimeSchedule of this version. The PID is what ties the picture to the run.
        Write-Result ([pscustomobject]@{ file = $file; pid = $instance.pid; windowHandle = $target.Handle; window = $target.Title; width = $target.ClientWidth; height = $target.ClientHeight })
    }
    'click' {
        $instance = Get-Instance
        $target = Get-TargetWindow $instance
        if ($X -lt 0 -or $Y -lt 0 -or $X -ge $target.ClientWidth -or $Y -ge $target.ClientHeight) { throw "Pass -X and -Y inside the window: $($target.ClientWidth) x $($target.ClientHeight) pixels." }
        $refused = [TsVerifyNative2]::Click($target.Handle, $X, $Y, $(if ($Double) { 2 } else { 1 }))
        if ($refused) { throw "No click was sent: $refused." }
        Start-Sleep -Milliseconds 400
        Write-Result ([pscustomobject]@{ clicked = $target.Title; x = $X; y = $Y; double = [bool]$Double })
    }
    'type' {
        $instance = Get-Instance
        if ($TextFile) { $Text = [IO.File]::ReadAllText($TextFile, $utf8) }
        if (-not $Text) { throw 'Pass -Text or -TextFile (UTF-8).' }
        $target = Get-TargetWindow $instance
        $refused = [TsVerifyNative2]::TypeText($target.Handle, $Text)
        if ($refused) { throw "Typing stopped: $refused." }
        Start-Sleep -Milliseconds 400
        Write-Result ([pscustomobject]@{ typedInto = $target.Title; characters = $Text.Length })
    }
    'key' {
        $instance = Get-Instance
        if (-not $Key) { throw 'Pass -Key.' }
        $target = Get-TargetWindow $instance
        $keys = @{ Enter = @(0x0D, 0); Escape = @(0x1B, 0); Tab = @(0x09, 0); Backspace = @(0x08, 0); Delete = @(0x2E, 1)
            Left = @(0x25, 1); Right = @(0x27, 1); Up = @(0x26, 1); Down = @(0x28, 1); Home = @(0x24, 1); End = @(0x23, 1) }
        $spec = $keys[$Key]
        $refused = [TsVerifyNative2]::Key($target.Handle, [uint16]$spec[0], [bool]$spec[1])
        if ($refused) { throw "No key was sent: $refused." }
        Start-Sleep -Milliseconds 400
        Write-Result ([pscustomobject]@{ pressed = $Key; window = $target.Title })
    }
    'stop' {
        $instance = Get-Instance -AllowStopped
        $evidence = { @(Get-ChildItem -LiteralPath $instance.evidenceDir -ErrorAction SilentlyContinue | ForEach-Object { $_.Name }) }
        # A second stop deletes nothing: whatever is in data now is not what launch made.
        if ($instance.stopped) { Write-Result ([pscustomobject]@{ process = 'already stopped'; pid = $instance.pid; dataRemoved = $false; dataLeft = $(if (Test-Path -LiteralPath $instance.dataDir) { $instance.dataDir } else { $null }); evidenceDir = $instance.evidenceDir; evidence = (& $evidence) }) $(if ((Test-Path -LiteralPath $instance.dataDir) -and -not $KeepData) { 1 } else { 0 }) }
        $process = Get-OwnedProcess $instance
        $stopped = 'was not running'
        if ($process) {
            # Through the handle that was just checked, not by looking the PID up again.
            $process.Kill()
            if (-not $process.WaitForExit(10000)) { throw "PID $($process.Id) did not exit." }
            $stopped = 'stopped'
        }
        # The record is retired before anything is deleted: an interrupted cleanup
        # is then never repeated against a folder that appeared later.
        Move-Item -LiteralPath (Join-Path $instance.runDir 'instance.json') -Destination (Join-Path $instance.runDir 'instance.stopped.json')
        $removed = $false; $left = $null
        # Only a folder the app itself used as its data folder is removed.
        if (-not $KeepData -and (Test-Path -LiteralPath (Join-Path $instance.dataDir 'native-instance.lock'))) {
            try { Remove-Item -LiteralPath $instance.dataDir -Recurse -Force; $removed = $true } catch { }
        }
        if (Test-Path -LiteralPath $instance.dataDir) { $left = $instance.dataDir }
        # dataLeft names a data folder that is still there: kept on request, or not fully deleted.
        Write-Result ([pscustomobject]@{ process = $stopped; pid = $instance.pid; dataRemoved = $removed; dataLeft = $left; evidenceDir = $instance.evidenceDir; evidence = (& $evidence) }) $(if ($left -and -not $KeepData) { 1 } else { 0 })
    }
}
} finally {
    if ($previousDpi -ne 0) { [TsVerifyNative2]::LeavePixels($previousDpi) }
    [Console]::OutputEncoding = $previousEncoding
}
