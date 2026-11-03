import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, writeFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { createServer } from 'node:http';

const run = promisify(execFile);
const ps = value => `'${value.replaceAll("'", "''")}'`;
const script = resolve('scripts/mcp-session.ps1');

test('MCP setup rejects an external endpoint before reading credentials', { skip: process.platform !== 'win32' }, async () => {
  const dir = await mkdtemp(join(tmpdir(), 'timeschedule-mcp-'));
  try {
    await writeFile(join(dir, 'mcp-status.json'), JSON.stringify({ endpoint: 'https://example.com/mcp' }));
    await assert.rejects(run('powershell.exe', ['-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', script, '-DataDir', dir]), error => {
      assert.match(error.stderr, /only http:\/\/127\.0\.0\.1/);
      assert.doesNotMatch(error.stderr, /credential file/);
      return true;
    });
  } finally { await rm(dir, { recursive: true, force: true }); }
});

test('MCP setup decrypts only into process env and verifies status without printing the credential', { skip: process.platform !== 'win32' }, async () => {
  const dir = await mkdtemp(join(tmpdir(), 'timeschedule-mcp-'));
  let authenticated = 0;
  const server = createServer(async (request, response) => {
    if (/^Bearer [a-f0-9]{64}$/.test(request.headers.authorization ?? '')) authenticated++;
    let body = ''; for await (const chunk of request) body += chunk;
    const rpc = JSON.parse(body);
    response.setHeader('Content-Type', 'application/json');
    response.end(JSON.stringify({ jsonrpc: '2.0', id: rpc.id, result: rpc.method === 'initialize'
      ? { protocolVersion: '2025-11-25', serverInfo: { name: 'TimeSchedule', version: 'fixture' } }
      : { tools: ['calendar_list', 'calendar_get', 'calendar_create', 'calendar_update'].map(name => ({ name })) } }));
  });
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  try {
    await writeFile(join(dir, 'mcp-status.json'), JSON.stringify({ endpoint: `http://127.0.0.1:${server.address().port}/mcp` }));
    const command = `Add-Type -AssemblyName System.Security
$fixtureToken = [Guid]::NewGuid().ToString('N') + [Guid]::NewGuid().ToString('N')
$encrypted = [Security.Cryptography.ProtectedData]::Protect([Text.Encoding]::UTF8.GetBytes($fixtureToken), $null, [Security.Cryptography.DataProtectionScope]::CurrentUser)
[IO.File]::WriteAllBytes(${ps(join(dir, 'mcp-token.dpapi'))}, $encrypted)
$before = [Environment]::GetEnvironmentVariable('TIMESCHEDULE_MCP_TOKEN', 'User')
$result = & ${ps(script)} -DataDir ${ps(dir)} -Status
@{ status=$result.status; tools=$result.tools.Count; processMatches=($env:TIMESCHEDULE_MCP_TOKEN -ceq $fixtureToken); userUnchanged=($before -ceq [Environment]::GetEnvironmentVariable('TIMESCHEDULE_MCP_TOKEN', 'User')) } | ConvertTo-Json -Compress`;
    const { stdout, stderr } = await run('powershell.exe', ['-NoProfile', '-ExecutionPolicy', 'Bypass', '-Command', command]);
    assert.equal(stderr, '');
    assert.doesNotMatch(stdout, /[a-f0-9]{64}/);
    assert.deepEqual(JSON.parse(stdout), { status: 'running', tools: 4, processMatches: true, userUnchanged: true });
    assert.equal(authenticated, 2);
  } finally {
    await new Promise(resolve => server.close(resolve));
    await rm(dir, { recursive: true, force: true });
  }
});
