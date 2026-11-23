import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { randomUUID } from 'node:crypto';
import { prepare, sync, status } from '../scripts/project-records.mjs';
import { gitFixture } from './helpers/record-git-fixture.mjs';
import { configureVault } from '../scripts/document-vault.mjs';
function setup(t) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'timeschedule-local-'));
  t.after(() => fs.rmSync(dir, { recursive: true, force: true }));
  const f = gitFixture(dir);
  return { ...f, stateDir: path.join(dir, 'state'), projectRoot: f.repo, vaultRoot: path.join(dir, 'vault') };
}
const source = f => ({ workId: randomUUID(), projectTitle: 'TimeSchedule', repoPath: f.repo, gitChanges: f.gitChanges,
  date: '2026-11-16', kind: 'development', title: '로컬 저장', status: 'verification_pending', detailedMarkdown: '테스트 통과. 사용자 확인 전.' });
const saved = r => ({ outcome: 'saved', workId: r.workId, revision: r.revision, entryId: r.workId });
test('a capture saves Markdown and the local diary state', async t => {
  const f = setup(t), input = source(f); await prepare(input, f);
  await sync({ ...f, importer: async r => {
    assert.equal(r.deliveryState, 'local'); assert.equal(r.spaceUrl, ''); return saved(r);
  } });
  const record = status(f)[0];
  assert.equal(record.phase, 'local_complete'); assert.equal(record.vaultSaved, true);
  assert.equal(fs.readFileSync(record.localFile, 'utf8'), input.detailedMarkdown);
  assert.equal(JSON.parse(fs.readFileSync(path.join(f.stateDir, 'wake.json'))).pending, 0);
});
test('vault persists before unavailable diary executable and explicit configuration path survives retries', async t => {
  const f = setup(t); await configureVault(f.stateDir, { root: f.vaultRoot });
  const input = source(f); await prepare(input, f);
  const { vaultRoot, ...persisted } = f;
  await sync({ ...persisted, exe: path.join(f.stateDir, 'missing.exe') });
  const record = status(f)[0]; assert.equal(record.vaultSaved, true);
  assert.ok(record.localFile.startsWith(vaultRoot)); assert.ok(record.lastError);
});
test('a queued diary receipt is distinct from durable vault save and user edits remain conflicts', async t => {
  const f = setup(t); await prepare(source(f), f);
  await sync({ ...f, importer: async r => ({ ...saved(r), outcome: 'queued' }) });
  const queued = status(f)[0]; assert.equal(queued.phase, 'diary_queued'); assert.equal(queued.vaultSaved, true);
  await sync({ ...f, retryNow: true, importer: async r => ({ ...saved(r), outcome: 'conflict' }) });
  assert.equal(status(f)[0].phase, 'conflict'); assert.ok(fs.existsSync(queued.localFile));
});

test('a settings file that still has Space turned on is read, and nothing is published', async t => {
  const f = setup(t), input = source(f); await configureVault(f.stateDir, { root: f.vaultRoot });
  const settings = path.join(f.stateDir, 'vault-settings.json');
  fs.writeFileSync(settings, JSON.stringify({ ...JSON.parse(fs.readFileSync(settings, 'utf8')), spaceEnabled: true }));
  await prepare(input, f);
  const { vaultRoot, ...persisted } = f;
  await sync({ ...persisted, importer: async r => { assert.equal(r.deliveryState, 'local'); return saved(r); } });
  const record = status(f)[0];
  assert.equal(record.phase, 'local_complete'); assert.equal(record.pageUrl, null);
  assert.ok(record.localFile.startsWith(vaultRoot));
  // A scoped retry does not reopen it either.
  await sync({ ...persisted, workId: input.workId, retryNow: true, importer: () => { throw Error('must not send again'); } });
  assert.equal(status(f)[0].phase, 'local_complete');
});

test('built native executable completes isolated local capture', { skip: !process.env.TIMESCHEDULE_RECORD_TEST_EXE }, async t => {
  const f = setup(t), input = source(f);
  await configureVault(f.stateDir, { root: f.vaultRoot });
  await prepare(input, f);
  const { vaultRoot, ...persisted } = f;
  await sync({ ...persisted, exe: process.env.TIMESCHEDULE_RECORD_TEST_EXE, dataDir: path.join(path.dirname(f.stateDir), 'native-data') });
  const record = status(f)[0];
  assert.equal(record.phase, 'local_complete', record.lastError);
  assert.equal(record.vaultSaved, true);
  assert.equal(fs.readFileSync(record.localFile, 'utf8'), input.detailedMarkdown);
  assert.ok(fs.existsSync(path.join(path.dirname(f.stateDir), 'native-data', 'schedule.db')));
});
