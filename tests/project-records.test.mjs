import test, { after } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { randomUUID } from 'node:crypto';
import { spawn } from 'node:child_process';
import { pathToFileURL } from 'node:url';
import { prepare as prepareRecord, sync as syncRecords, status } from '../scripts/project-records.mjs';
import { gitFixture } from './helpers/record-git-fixture.mjs';
import { appHolding } from './helpers/space-era.mjs';

const fixtureDirectory = fs.mkdtempSync(path.join(os.tmpdir(), 'timeschedule-record-git-'));
const fixture = gitFixture(fixtureDirectory);
after(() => fs.rmSync(fixtureDirectory, { recursive: true, force: true }));
const prepare = (input, options) => prepareRecord(input, { ...options, projectRoot: fixture.repo });
const sync = options => syncRecords({ vaultRoot: path.join(options.stateDir, 'vault'), ...options, projectRoot: fixture.repo });

function source() {
  return {
    workId: randomUUID(), projectTitle: 'TimeSchedule', repoPath: fixture.repo, gitChanges: fixture.gitChanges, date: '2026-10-31',
    kind: 'troubleshooting', title: '검증 결과와 한계', status: 'verification_pending',
    detailedMarkdown: 'RAM은 감소했지만 유휴 CPU는 증가했다. 실제 입력 성능은 측정하지 않았다.\n\n# 원인\n\nCPU 증가 원인은 아직 확인하지 않았다.',
    diarySummary: 'RAM은 감소했지만 유휴 CPU는 증가했다. 실제 입력 성능은 측정하지 않았다.',
  };
}
function stateDir(t) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'timeschedule-record-delivery-'));
  t.after(() => {
    assert.ok(dir.startsWith(path.join(os.tmpdir(), 'timeschedule-record-delivery-')));
    fs.rmSync(dir, { recursive: true, force: true });
  });
  return dir;
}

const saved = r => ({ outcome: 'saved', workId: r.workId, revision: r.revision, entryId: 'entry-1' });
test('a no-PR record sends its full text to the diary once and an unchanged prepare sends nothing more', async t => {
  const dir = stateDir(t); const input = source();
  const a = await prepare(input, { stateDir: dir });
  assert.equal(a.revision, 1);
  const sent = [];
  const options = { stateDir: dir, importer: async record => { sent.push(record); return saved(record); } };
  await sync(options);
  assert.deepEqual(sent.map(r => [r.deliveryState, r.spaceUrl, r.detail]), [['local', '', input.detailedMarkdown]]);
  assert.equal(status({ stateDir: dir })[0].phase, 'local_complete');
  assert.equal((await prepare(input, { stateDir: dir })).revision, 1);
  await sync(options);
  assert.equal(sent.length, 1);
});

test('queued is not complete and user-edit conflict is retained', async t => {
  const dir = stateDir(t); await prepare(source(), { stateDir: dir });
  await sync({ stateDir: dir, importer: async r => ({ outcome: 'queued', workId: r.workId, revision: r.revision, entryId: null }) });
  assert.equal(status({ stateDir: dir })[0].phase, 'diary_queued');
  await sync({ stateDir: dir, retryNow: true, importer: async r => ({ outcome: 'conflict', workId: r.workId, revision: r.revision, message: 'user edited' }) });
  assert.equal(status({ stateDir: dir })[0].phase, 'conflict');
});

test('a queued record is sent again with the same content until the app confirms it', async t => {
  const dir = stateDir(t); await prepare(source(), { stateDir: dir });
  const sent = [];
  await sync({ stateDir: dir, importer: async r => { sent.push(r); return { outcome: 'queued', workId: r.workId, revision: r.revision, entryId: null }; } });
  await sync({ stateDir: dir, retryNow: true, importer: async r => { sent.push(r); return saved(r); } });
  assert.equal(sent.length, 2);
  assert.deepEqual(sent[1], sent[0]);
  assert.equal(status({ stateDir: dir })[0].phase, 'local_complete');
});

test('each capture of a work gets a higher delivery number, and delivery never changes it', async t => {
  const dir = stateDir(t); const input = source();
  const numbers = () => status({ stateDir: dir }).map(s => JSON.parse(fs.readFileSync(path.join(dir, input.workId, s.revision === 3 ? 'state.json' : `pending-${s.revision}.json`), 'utf8')).localRevision);
  await prepare(input, { stateDir: dir });
  await prepare({ ...input, detailedMarkdown: input.detailedMarkdown + '\n\n둘째 수정본.' }, { stateDir: dir });
  await prepare({ ...input, detailedMarkdown: input.detailedMarkdown + '\n\n셋째 수정본.' }, { stateDir: dir });
  const prepared = numbers();
  assert.ok(prepared[0] < prepared[1] && prepared[1] < prepared[2], String(prepared));
  const sent = [];
  await sync({ stateDir: dir, importer: async r => { sent.push(r.revision); return saved(r); } });
  assert.deepEqual(sent, prepared);
  assert.deepEqual(numbers(), prepared);
});

test('a work item whose delivery fails does not block a different work item', async t => {
  const dir = stateDir(t), a = source(), b = source();
  await prepare(a, { stateDir: dir }); await prepare(b, { stateDir: dir });
  await sync({ stateDir: dir, importer: async r => { if (r.workId === a.workId) throw new Error('EXE unavailable'); return saved(r); } });
  assert.equal(status({ stateDir: dir, workId: a.workId })[0].phase, 'prepared');
  assert.match(status({ stateDir: dir, workId: a.workId })[0].lastError, /EXE unavailable/);
  assert.equal(status({ stateDir: dir, workId: b.workId })[0].phase, 'local_complete');
});

test('a new capture is numbered above every earlier capture of its work, also one kept in a pending file', async t => {
  const dir = stateDir(t); const input = source();
  await prepare(input, { stateDir: dir });
  await prepare({ ...input, detailedMarkdown: input.detailedMarkdown + '\n\n둘째 수정본.' }, { stateDir: dir });
  // As a version before 0.32.0 leaves it after renumbering the first capture.
  const file = path.join(dir, input.workId, 'pending-1.json');
  fs.writeFileSync(file, JSON.stringify({ ...JSON.parse(fs.readFileSync(file, 'utf8')), localRevision: 9, finalRevision: 10 }));
  await prepare({ ...input, detailedMarkdown: input.detailedMarkdown + '\n\n셋째 수정본.' }, { stateDir: dir });
  assert.equal(JSON.parse(fs.readFileSync(path.join(dir, input.workId, 'state.json'), 'utf8')).localRevision, 11);
});

test('an earlier capture ends when the app answers stale and a later capture exists, even if that one\'s answer was lost', async t => {
  const dir = stateDir(t); const input = source(), app = appHolding();
  const second = { ...input, detailedMarkdown: input.detailedMarkdown + '\n\n둘째 수정본.' };
  await prepare(input, { stateDir: dir });
  await sync({ stateDir: dir, importer: async () => { throw new Error('EXE unavailable'); } });
  await prepare(second, { stateDir: dir });
  // The app saves the second capture; its answer does not reach the script.
  await sync({ stateDir: dir, importer: async r => { await app.importer(r); throw new Error('answer lost'); } });
  assert.equal(app.detail, second.detailedMarkdown);
  assert.deepEqual(status({ stateDir: dir }).map(s => s.phase), ['prepared', 'prepared']);
  // As the app runs it: one job per run. The earlier capture is tried first.
  for (let run = 0; run < 2; run++) await sync({ stateDir: dir, retryNow: true, maxJobs: 1, importer: app.importer });
  assert.deepEqual(status({ stateDir: dir }).map(s => [s.phase, s.lastError]), [['local_complete', null], ['local_complete', null]]);
  assert.equal(app.detail, second.detailedMarkdown);
  assert.deepEqual(app.rollbacks, []);
  assert.equal(JSON.parse(fs.readFileSync(path.join(dir, 'wake.json'), 'utf8')).pending, 0);
});

test('the newest capture does not end on a stale answer', async t => {
  const dir = stateDir(t); const input = source();
  await prepare(input, { stateDir: dir });
  await sync({ stateDir: dir, importer: async r => ({ outcome: 'stale', workId: r.workId, revision: r.revision, entryId: 'entry-1' }) });
  const [record] = status({ stateDir: dir });
  assert.equal(record.phase, 'prepared');
  assert.match(record.lastError, /stale/);
});

test('a document the user edited in the vault ends the next capture as a conflict, not as an endless retry', async t => {
  const dir = stateDir(t); const input = source(), app = appHolding();
  await prepare(input, { stateDir: dir });
  await sync({ stateDir: dir, importer: app.importer });
  const document = status({ stateDir: dir })[0].localFile;
  fs.writeFileSync(document, '사용자가 직접 고친 문서');
  await prepare({ ...input, detailedMarkdown: input.detailedMarkdown + '\n\n둘째 수정본.' }, { stateDir: dir });
  await sync({ stateDir: dir, importer: app.importer });
  assert.equal(status({ stateDir: dir })[0].phase, 'conflict');
  assert.equal(fs.readFileSync(document, 'utf8'), '사용자가 직접 고친 문서');
  assert.equal(app.detail, input.detailedMarkdown);
  assert.equal(JSON.parse(fs.readFileSync(path.join(dir, 'wake.json'), 'utf8')).pending, 0);
  await sync({ stateDir: dir, retryNow: true, importer: () => { throw new Error('must not retry a conflict'); } });
  assert.equal(status({ stateDir: dir })[0].phase, 'conflict');
});

test('summary must retain a whole source paragraph including limitations', async t => {
  const dir = stateDir(t); const input = source();
  await assert.rejects(prepare({ ...input, diarySummary: 'RAM은 감소했다. 모든 성능이 향상되었다.' }, { stateDir: dir }), /원문|문단/);
  const { diarySummary, ...withoutSummary } = input;
  await prepare(withoutSummary, { stateDir: dir });
  let summary;
  await sync({ stateDir: dir, importer: async r => { summary = r.summary; return saved(r); } });
  assert.equal(summary, diarySummary);
});

test('a stale lock observer cannot enter while another process owns delivery', async t => {
  const dir = stateDir(t); await prepare(source(), { stateDir: dir });
  const moduleUrl = pathToFileURL(path.resolve('scripts/project-records.mjs')).href;
  const child = spawn(process.execPath, ['--input-type=module', '-e', `
    import {sync} from ${JSON.stringify(moduleUrl)};
    try { await sync({stateDir:process.argv[1],vaultRoot:process.argv[1]+'/vault',projectRoot:process.argv[2],importer:async()=>{
      process.send('entered'); await new Promise(r=>process.once('message',r)); throw Error('test finished');
    }}); } catch {} finally { process.disconnect(); }
  `, dir, fixture.repo], { windowsHide: true, stdio: ['ignore', 'ignore', 'pipe', 'ipc'] });
  t.after(() => child.kill());
  await new Promise((resolve, reject) => { child.once('message', resolve); child.once('error', reject); child.once('exit', () => reject(Error('holder exited early'))); });
  const originalRead = fs.readFileSync;
  // Reproduce the TOCTOU interleaving: this caller observed a dead owner's
  // marker just before the child acquired the live lock.
  fs.readFileSync = function(file, ...args) {
    if (String(file) === path.join(dir, '.sync.lock')) return JSON.stringify({pid:2147483647});
    return originalRead.call(this, file, ...args);
  };
  try {
    await assert.rejects(sync({stateDir:dir, importer:async r=>({outcome:'saved',workId:r.workId,revision:r.revision,entryId:'unexpected'})}), /다른 기록 작업/);
  } finally { fs.readFileSync = originalRead; child.send('finish'); }
  await new Promise(resolve => child.once('exit', resolve));
});
