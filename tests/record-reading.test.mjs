import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { existsSync, readdirSync } from 'node:fs';
import { mkdtemp, mkdir, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import path from 'node:path';
import test from 'node:test';
import { zstdCompressSync } from 'node:zlib';

const exe = process.env.TIMESCHEDULE_RECORD_TEST_EXE;

test('the reading commands print the dialogue and the original line, and make no data folder', {skip: !exe}, async t => {
  const root = await mkdtemp(path.join(tmpdir(), 'timeschedule-reading-'));
  t.after(() => rm(root, {recursive: true, force: true}));
  // A home of our own keeps the command out of the real document store.
  const user = path.join(root, 'user'), data = path.join(root, 'no-data-folder-yet');
  const vault = path.join(user, 'Documents', 'TimeScheduleData');
  const project = '11111111-2222-4333-8444-555555555555', session = 'a';
  const group = path.join(vault, 'originals', 'c--work-0000000000000000');
  const file = 'codex/2026/11/24/rollout-a.jsonl.zst';
  await mkdir(path.dirname(path.join(group, file)), {recursive: true});
  await writeFile(path.join(vault, 'vault.json'), JSON.stringify({schemaVersion: 1}));
  await writeFile(path.join(group, 'project.json'),
    JSON.stringify({schemaVersion: 1, projectId: project, title: '작업', repoKey: 'c:/work'}));
  const typed = (at, id, text) => ({timestamp: at, type: 'event_msg', payload: {type: 'item_completed', thread_id: session,
    item: {type: 'UserMessage', id, client_id: '6f1c2d3e-0000-4000-8000-000000000000', content: [{type: 'text', text}]}}});
  const rows = [
    {timestamp: '2026-11-24T00:00:00Z', type: 'session_meta',
      payload: {id: session, cwd: 'C:\\work', originator: 'Codex Desktop', source: 'vscode', thread_source: 'user'}},
    typed('2026-11-24T01:00:00Z', 'u1', 'UNIQUE constraint 오류가 났어'),
    typed('2026-11-24T01:05:00Z', 'u2', '고쳤으면 커밋해'),
  ];
  await writeFile(path.join(group, file), zstdCompressSync(rows.map(r => JSON.stringify(r) + '\n').join('')));
  await writeFile(path.join(group, 'index.jsonl'), JSON.stringify({
    file, tool: 'codex', session, cwd: 'C:\\work', client: 'Codex Desktop', started: '2026-11-24T00:00:00Z',
    source: 'x', sourceSize: 1, sourceModified: Date.parse('2026-11-24T03:00:00Z') * 1e6, archivedAt: '2026-11-25T00:00:00Z',
  }) + '\n');
  const env = {...process.env, USERPROFILE: user};
  delete env.TIMESCHEDULE_DATA_DIR;
  const record = (...args) => spawnSync(exe, ['record', ...args, '--data-dir', data],
    {env, windowsHide: true, encoding: 'utf8', timeout: 15000});
  const storeBefore = readdirSync(group).sort();

  const found = record('transcript', '--project', project, '--date', '2026-11-24', '--query', 'unique');
  assert.equal(found.status, 0, found.stderr || found.stdout);
  const day = JSON.parse(found.stdout);
  assert.equal(day.outcome, 'ok');
  assert.deepEqual(day.blocks.map(b => [b.number, b.speaker, b.file, b.line, b.text]),
    [[1, '사람', file, 2, 'UNIQUE constraint 오류가 났어']]);

  const original = record('original', '--project', project, '--file', file, '--line', '3');
  assert.equal(original.status, 0, original.stderr || original.stdout);
  assert.deepEqual(JSON.parse(JSON.parse(original.stdout).lines[0].text), rows[2]);

  const past = record('original', '--project', project, '--file', file, '--line', '9');
  assert.equal(past.status, 1);
  assert.match(JSON.parse(past.stdout).message, /3줄입니다/);

  // A reading command takes no result file, wherever it is named.
  for (const named of [path.join(vault, 'out.json'), path.join(root, 'out.json')]) {
    const refused = record('transcript', '--project', project, '--date', '2026-11-24', '--result', named);
    assert.equal(refused.status, 1);
    assert.match(JSON.parse(refused.stdout).message, /--result를 받지 않습니다/);
    assert.equal(existsSync(named), false);
  }

  // The records are in the app's database, which is not there: the command says so.
  const list = record('list');
  assert.equal(list.status, 1);
  assert.match(JSON.parse(list.stdout).message, /DB 파일이 없습니다/);
  assert.equal(existsSync(data), false);
  assert.deepEqual(readdirSync(group).sort(), storeBefore);
});
