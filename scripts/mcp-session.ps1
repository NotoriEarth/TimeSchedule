param(
    [string]$DataDir = $(if ($env:TIMESCHEDULE_DATA_DIR) { $env:TIMESCHEDULE_DATA_DIR } else { Join-Path $env:LOCALAPPDATA 'com.timeschedule.personal.native-experiment' }),
    [switch]$PersistToken,
    [switch]$Status
)
$ErrorActionPreference = 'Stop'
# Never print the token or put it in a command argument / generated config.
$mcpStatusPath = Join-Path $DataDir 'mcp-status.json'
if (-not (Test-Path -LiteralPath $mcpStatusPath)) { throw 'TimeSchedule MCP status is missing. Start TimeSchedule and check Settings > Local calendar MCP.' }
$mcpStatusFile = Get-Item -LiteralPath $mcpStatusPath
if ($mcpStatusFile.Length -gt 4096) { throw 'Invalid MCP status file.' }
$mcpStatus = Get-Content -LiteralPath $mcpStatusPath -Raw | ConvertFrom-Json
$mcpUri = $null
if (-not [Uri]::TryCreate([string]$mcpStatus.endpoint, [UriKind]::Absolute, [ref]$mcpUri) -or
    $mcpUri.Scheme -cne 'http' -or $mcpUri.Host -cne '127.0.0.1' -or
    $mcpUri.AbsolutePath -cne '/mcp' -or $mcpUri.Port -le 0 -or
    $mcpUri.UserInfo -or $mcpUri.Query -or $mcpUri.Fragment) { throw 'Invalid MCP endpoint: only http://127.0.0.1:PORT/mcp is allowed.' }
$mcpEndpoint = 'http://127.0.0.1:' + $mcpUri.Port + '/mcp'
$mcpTokenPath = Join-Path $DataDir 'mcp-token.dpapi'
if ((Get-Item -LiteralPath $mcpTokenPath).Length -gt 16384) { throw 'Invalid MCP credential file.' }
Add-Type -AssemblyName System.Security
try {
    $mcpPlain = [Security.Cryptography.ProtectedData]::Unprotect([IO.File]::ReadAllBytes($mcpTokenPath), $null, [Security.Cryptography.DataProtectionScope]::CurrentUser)
    $mcpToken = [Text.Encoding]::UTF8.GetString($mcpPlain)
    if ($mcpToken -cnotmatch '^[0-9a-f]{64}$') { throw 'Invalid credential.' }
} catch { throw 'Cannot read the MCP credential. Use the Windows account that runs TimeSchedule.' }
finally { if ($mcpPlain) { [Array]::Clear($mcpPlain, 0, $mcpPlain.Length) } }
$env:TIMESCHEDULE_MCP_TOKEN = $mcpToken
if ($PersistToken) {
    [Environment]::SetEnvironmentVariable('TIMESCHEDULE_MCP_TOKEN', $mcpToken, 'User')
    Write-Host 'Saved token to your Windows user environment (not encrypted there). Sign out/in, then restart Codex to inherit it.'
}
if ($Status) {
    # mcp-status.json can outlive a crashed app, so another process may own this port.
    # Send the token only after the listener proves it already holds it (HMAC over a
    # fresh challenge and this host; the token itself is not transmitted).
    $mcpProven = $false
    try {
        $mcpHost = '127.0.0.1:' + $mcpUri.Port
        $mcpChallenge = [Guid]::NewGuid().ToString('N') + [Guid]::NewGuid().ToString('N')
        $mcpHmac = [Security.Cryptography.HMACSHA256]::new([Text.Encoding]::UTF8.GetBytes($mcpToken))
        try {
            $mcpExpected = -join ($mcpHmac.ComputeHash([Text.Encoding]::UTF8.GetBytes("TimeSchedule MCP server proof v1`n$mcpHost`n$mcpChallenge")) | ForEach-Object { $_.ToString('x2') })
        } finally { $mcpHmac.Dispose() }
        $mcpAnswer = Invoke-RestMethod -Uri ('http://' + $mcpHost + '/proof') -Method Get -Headers @{ 'TimeSchedule-Challenge' = $mcpChallenge } -MaximumRedirection 0 -TimeoutSec 3
        $mcpProven = ([string]$mcpAnswer -ceq $mcpExpected)
    } catch { $mcpProven = $false }
    if (-not $mcpProven) { throw 'Could not confirm that TimeSchedule 0.13.1 or later is the app listening on this port. The credential was not sent. Start TimeSchedule and try again.' }
    $mcpHeaders =@{ Authorization = 'Bearer ' + $mcpToken; Accept = 'application/json, text/event-stream'; 'MCP-Protocol-Version' = '2025-11-25' }
    $mcpInit = @{ jsonrpc = '2.0'; id = 1; method = 'initialize'; params = @{ protocolVersion = '2025-11-25'; capabilities = @{}; clientInfo = @{ name = 'TimeSchedule setup'; version = '1' } } } | ConvertTo-Json -Depth 8 -Compress
    try {
        $mcpReply = Invoke-RestMethod -Uri $mcpEndpoint -Method Post -Headers $mcpHeaders -ContentType 'application/json' -Body $mcpInit -MaximumRedirection 0 -TimeoutSec 12
        if ($mcpReply.result.serverInfo.name -cne 'TimeSchedule') { throw 'Unexpected MCP server.' }
        $mcpList = '{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}'
        $mcpTools = Invoke-RestMethod -Uri $mcpEndpoint -Method Post -Headers $mcpHeaders -ContentType 'application/json' -Body $mcpList -MaximumRedirection 0 -TimeoutSec 12
        [pscustomobject]@{ endpoint = $mcpEndpoint; status = 'running'; version = $mcpReply.result.serverInfo.version; tools = @($mcpTools.result.tools.name) }
    } catch { throw 'MCP connection check failed. Check that TimeSchedule is running and the configured port is available.' }
} else { Write-Host ('MCP token loaded into this PowerShell process. Endpoint: ' + $mcpEndpoint) }
$mcpToken = $null
$mcpHeaders = $null
