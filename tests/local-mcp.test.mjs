import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, writeFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { createServer } from 'node:http';
import { createHmac, randomBytes } from 'node:crypto';

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

// Same message the native server signs: the token never travels, and the host
// binds the answer to one port.
const serverProof = (token, host, challenge) => createHmac('sha256', token).update(`TimeSchedule MCP server proof v1\n${host}\n${challenge}`).digest('hex');
const protectToken = (dir, token) => `Add-Type -AssemblyName System.Security
$fixtureToken = '${token}'
$encrypted = [Security.Cryptography.ProtectedData]::Protect([Text.Encoding]::UTF8.GetBytes($fixtureToken), $null, [Security.Cryptography.DataProtectionScope]::CurrentUser)
[IO.File]::WriteAllBytes(${ps(join(dir, 'mcp-token.dpapi'))}, $encrypted)`;

test('server proof matches the vector pinned in the native app tests', () => {
  assert.equal(serverProof('0123456789abcdef'.repeat(4), '127.0.0.1:39281', 'ab'.repeat(32)), '9c488ddb357b469f4d2205932eb32a03afd257e7f518ac9ccf79326b91c6e978');
});

test('MCP status check never sends the credential to a listener that cannot prove it holds the token', { skip: process.platform !== 'win32' }, async () => {
  const dir = await mkdtemp(join(tmpdir(), 'timeschedule-mcp-'));
  let credentialsReceived = 0;
  // Stands in for another process occupying the port left in a stale status file.
  const server = createServer((request, response) => {
    if (request.headers.authorization) credentialsReceived++;
    request.resume();
    response.setHeader('Content-Type', request.url === '/proof' ? 'text/plain' : 'application/json');
    response.end(request.url === '/proof' ? '0'.repeat(64) : JSON.stringify({ jsonrpc: '2.0', id: 1, result: { protocolVersion: '2025-11-25', serverInfo: { name: 'TimeSchedule', version: 'impostor' } } }));
  });
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  try {
    await writeFile(join(dir, 'mcp-status.json'), JSON.stringify({ endpoint: `http://127.0.0.1:${server.address().port}/mcp` }));
    const command = `${protectToken(dir, randomBytes(32).toString('hex'))}
& ${ps(script)} -DataDir ${ps(dir)} -Status`;
    await assert.rejects(run('powershell.exe', ['-NoProfile', '-ExecutionPolicy', 'Bypass', '-Command', command]), error => {
      assert.match(error.stderr, /credential was not sent/);
      return true;
    });
    assert.equal(credentialsReceived, 0);
  } finally {
    await new Promise(resolve => server.close(resolve));
    await rm(dir, { recursive: true, force: true });
  }
});

test('MCP setup decrypts only into process env and verifies status without printing the credential', { skip: process.platform !== 'win32' }, async () => {
  const dir = await mkdtemp(join(tmpdir(), 'timeschedule-mcp-'));
  const token = randomBytes(32).toString('hex');
  let authenticated = 0;
  const server = createServer(async (request, response) => {
    if (request.url === '/proof') {
      response.setHeader('Content-Type', 'text/plain');
      return response.end(serverProof(token, request.headers.host, request.headers['timeschedule-challenge']));
    }
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
    const command = `${protectToken(dir, token)}
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
