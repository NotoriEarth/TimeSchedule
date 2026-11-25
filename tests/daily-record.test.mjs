import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { existsSync, readFileSync, readdirSync } from 'node:fs';
import { mkdtemp, mkdir, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import path from 'node:path';
import test from 'node:test';
import { zstdCompressSync } from 'node:zlib';

const exe = process.env.TIMESCHEDULE_RECORD_TEST_EXE;

test('record daily --dry-run writes the transcript and the result, and makes no data folder', {skip: !exe}, async t => {
  const root = await mkdtemp(path.join(tmpdir(), 'timeschedule-daily-'));
  t.after(() => rm(root, {recursive: true, force: true}));
  // The command finds the default document store under the user's home. A home of
  // our own keeps this test out of the real one.
  const user = path.join(root, 'user'), data = path.join(root, 'no-data-folder-yet');
  const vault = path.join(user, 'Documents', 'TimeScheduleData');
  const project = '11111111-2222-4333-8444-555555555555', session = 'a';
  const group = path.join(vault, 'originals', 'c--work-0000000000000000');
  const file = 'codex/2026/11/24/rollout-a.jsonl.zst';
  await mkdir(path.dirname(path.join(group, file)), {recursive: true});
  await writeFile(path.join(vault, 'vault.json'), JSON.stringify({schemaVersion: 1}));
  await writeFile(path.join(group, 'project.json'),
    JSON.stringify({schemaVersion: 1, projectId: project, title: '작업', repoKey: 'c:/work'}));
  const rows = [
    {timestamp: '2026-11-24T00:00:00Z', type: 'session_meta',
      payload: {id: session, cwd: 'C:\\work', originator: 'Codex Desktop', source: 'vscode', thread_source: 'user'}},
    {timestamp: '2026-11-24T01:00:00Z', type: 'event_msg', payload: {type: 'item_completed', thread_id: session,
      item: {type: 'UserMessage', id: 'u1', client_id: '6f1c2d3e-0000-4000-8000-000000000000', content: [{type: 'text', text: '진행해'}]}}},
  ];
  await writeFile(path.join(group, file), zstdCompressSync(rows.map(r => JSON.stringify(r) + '\n').join('')));
  await writeFile(path.join(group, 'index.jsonl'), JSON.stringify({
    file, tool: 'codex', session, cwd: 'C:\\work', client: 'Codex Desktop', started: '2026-11-24T00:00:00Z',
    source: 'x', sourceSize: 1, sourceModified: Date.parse('2026-11-24T03:00:00Z') * 1e6, archivedAt: '2026-11-25T00:00:00Z',
  }) + '\n');
  const env = {...process.env, USERPROFILE: user};
  delete env.TIMESCHEDULE_DATA_DIR;
  const daily = (...args) => spawnSync(exe,
    ['record', 'daily', '--project', project, '--date', '2026-11-24', '--data-dir', data, ...args],
    {env, windowsHide: true, encoding: 'utf8', timeout: 15000});

  const transcript = path.join(root, 'out', 'day.md'), result = path.join(root, 'out', 'result.json');
  const run = daily('--dry-run', '--transcript', transcript, '--result', result);
  assert.equal(run.status, 0, run.stderr || run.stdout);
  const printed = JSON.parse(run.stdout);
  assert.equal(printed.outcome, 'dry_run');
  assert.deepEqual(printed.summary.blocks, {'사람': 1});
  assert.deepEqual(printed.sources.map(s => s.files), [[file]]);
  assert.deepEqual(JSON.parse(readFileSync(result, 'utf8')), printed);
  assert.match(readFileSync(transcript, 'utf8'), /\n\[1\] 10:00 사람 · S1\n진행해\n$/);
  assert.equal(existsSync(data), false);

  // Named as the result too, the transcript is refused and left as it was.
  const before = readFileSync(transcript, 'utf8');
  const refused = daily('--dry-run', '--transcript', transcript, '--result', transcript.toUpperCase());
  assert.equal(refused.status, 1, refused.stderr || refused.stdout);
  assert.equal(JSON.parse(refused.stdout).outcome, 'error');
  assert.equal(readFileSync(transcript, 'utf8'), before);
  assert.equal(existsSync(data), false);
  assert.deepEqual(readdirSync(path.dirname(transcript)).sort(), ['day.md', 'result.json']);
});
