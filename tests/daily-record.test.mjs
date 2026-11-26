import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { existsSync, linkSync, readFileSync, readdirSync, writeFileSync } from 'node:fs';
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
  const entry = {
    file, tool: 'codex', session, cwd: 'C:\\work', client: 'Codex Desktop', started: '2026-11-24T00:00:00Z',
    source: 'x', sourceSize: 1, sourceModified: Date.parse('2026-11-24T03:00:00Z') * 1e6, archivedAt: '2026-11-25T00:00:00Z',
  };
  // A session of two days before whose kept file is gone.
  const lost = {...entry, file: 'codex/2026/11/22/rollout-b.jsonl.zst', session: 'b',
    started: '2026-11-22T00:00:00Z', sourceModified: Date.parse('2026-11-22T03:00:00Z') * 1e6};
  await writeFile(path.join(group, 'index.jsonl'), [entry, lost].map(e => JSON.stringify(e) + '\n').join(''));
  const env = {...process.env, USERPROFILE: user};
  delete env.TIMESCHEDULE_DATA_DIR;
  const on = date => (...args) => spawnSync(exe,
    ['record', 'daily', '--project', project, '--date', date, '--data-dir', data, ...args],
    {env, windowsHide: true, encoding: 'utf8', timeout: 15000});
  const daily = on('2026-11-24');

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

  // An output that is another name of a file kept elsewhere replaces that name and
  // leaves the file: each output is written beside its name and moved into place.
  const linked = path.join(root, 'linked'), keep = path.join(linked, 'keep.md');
  await mkdir(linked);
  await writeFile(keep, '다른 이름으로도 남겨 둔 글');
  const [asTranscript, asResult] = ['day.md', 'result.json'].map(name => path.join(linked, name));
  linkSync(keep, asTranscript);
  linkSync(keep, asResult);
  const links = daily('--dry-run', '--transcript', asTranscript, '--result', asResult);
  assert.equal(links.status, 0, links.stderr || links.stdout);
  assert.equal(readFileSync(keep, 'utf8'), '다른 이름으로도 남겨 둔 글');
  assert.equal(readFileSync(asTranscript, 'utf8'), before);
  assert.equal(JSON.parse(readFileSync(asResult, 'utf8')).outcome, 'dry_run');
  assert.deepEqual(readdirSync(linked).sort(), ['day.md', 'keep.md', 'result.json']);

  // An output named among the daily records or the kept originals is refused, and
  // the file it names is left as it was. No run here reaches the model.
  const records = path.join(vault, 'daily', 'c--work-0000000000000000');
  await mkdir(records, {recursive: true});
  for (const name of ['2026-11-24.json', '2026-11-24.r1.json']) {
    const record = path.join(records, name);
    await writeFile(record, '앞서 만든 기록');
    for (const args of [['--dry-run', '--result', record], ['--dry-run', '--result', record.toUpperCase()],
      ['--dry-run', '--transcript', record, '--result', result]]) {
      const kept = daily(...args);
      assert.equal(kept.status, 1, kept.stderr || kept.stdout);
      assert.match(JSON.parse(kept.stdout).message, /daily, originals, project 폴더 밖/);
      assert.equal(readFileSync(record, 'utf8'), '앞서 만든 기록');
    }
  }
  assert.deepEqual(JSON.parse(readFileSync(result, 'utf8')), printed);
  // While the store's marker cannot be read, nothing says where the records are, and
  // no output is written: not over a record, and not anywhere else.
  const marker = path.join(vault, 'vault.json'), record = path.join(records, '2026-11-24.json');
  const elsewhere = path.join(root, 'out', 'unfound.json');
  writeFileSync(marker, '{');
  for (const args of [['--result', record], ['--dry-run', '--result', record],
    ['--dry-run', '--transcript', record], ['--dry-run', '--result', elsewhere]]) {
    const unfound = daily(...args);
    assert.equal(unfound.status, 1, unfound.stderr || unfound.stdout);
    assert.equal(JSON.parse(unfound.stdout).outcome, 'error');
    assert.equal(readFileSync(record, 'utf8'), '앞서 만든 기록');
  }
  assert.equal(existsSync(elsewhere), false);
  writeFileSync(marker, JSON.stringify({schemaVersion: 1}));
  const among = path.join(group, 'codex', 'day.md');
  assert.equal(daily('--dry-run', '--transcript', among).status, 1);
  assert.equal(existsSync(among), false);
  await rm(path.join(vault, 'daily'), {recursive: true});

  // A day with no dialogue left and a warning is reported as the dry run reports it,
  // without --dry-run too: there is nothing to send to a model.
  const unread = on('2026-11-22')('--result', result);
  assert.equal(unread.status, 2, unread.stderr || unread.stdout);
  const told = JSON.parse(unread.stdout);
  assert.equal(told.outcome, 'attention');
  assert.equal(told.warnings.length, 1);
  assert.match(told.warnings[0], /^codex\/2026\/11\/22\/rollout-b\.jsonl\.zst: 읽지 못했습니다/);
  assert.deepEqual(told.summary.blocks, {});
  assert.equal(told.record, undefined);
  assert.deepEqual(JSON.parse(readFileSync(result, 'utf8')), told);
  assert.equal(existsSync(path.join(vault, 'daily')), false);
});
