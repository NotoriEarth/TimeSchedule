import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { randomUUID } from 'node:crypto';
import { prepare, sync, status, PARENT_PAGE_ID } from '../scripts/project-records.mjs';
import { gitFixture } from './helpers/record-git-fixture.mjs';

function setup(t) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'timeschedule-git-records-'));
  t.after(() => fs.rmSync(dir, { recursive: true, force: true }));
  const fixture = gitFixture(dir);
  return { ...fixture, stateDir: path.join(dir, 'state'), projectRoot: fixture.repo, vaultRoot: path.join(dir, 'vault'), spaceEnabled: true };
}
function input(f, gitChanges = f.gitChanges) {
  return { workId: randomUUID(), projectTitle: 'TimeSchedule', repoPath: f.repo, date: '2026-11-02',
    kind: 'development', title: '개발 변경 기록', status: 'verification_pending',
    detailedMarkdown: '코드를 수정하고 테스트했다. 실제 UI 동작은 미확인.', gitChanges };
}
const saved = r => ({ outcome: 'saved', workId: r.workId, revision: r.revision, entryId: 'entry-1' });
const published = j => ({ pageId: 'page_1234567890abcdef1234567890abcdef', pageUrl: 'https://chatgpt.com/space/page_1234567890abcdef1234567890abcdef',
  parentPageId: PARENT_PAGE_ID, title: j.source.title, markdown: j.pageMarkdown, indexVerified: true });

test('exploration without task Git evidence creates no record even with unrelated working changes', async t => {
  const f = setup(t);
  fs.writeFileSync(path.join(f.repo, 'src/example.js'), 'unrelated user edit');
  const candidate = input(f); delete candidate.gitChanges;
  const result = await prepare(candidate, f);
  assert.equal(result.skipped, true);
  assert.equal(result.reason, 'missing_git_changes');
  assert.deepEqual(status(f), []);
  assert.equal(fs.existsSync(f.stateDir), false);
});

test('documents, empty ranges and non-development selections do not create diary or Space work', async t => {
  const f = setup(t);
  for (const evidence of [
    { base: f.base, head: f.docs, paths: ['docs/research.md'] },
    { base: f.head, head: f.head, paths: ['src/example.js'] },
    { base: f.base, head: f.head, paths: ['docs/research.md'] },
  ]) assert.equal((await prepare(input(f, evidence), f)).skipped, true);
  assert.deepEqual(status(f), []);
});

test('committed development changes publish once and retain original text and whole-paragraph summary', async t => {
  const f = setup(t), candidate = input(f), order = [];
  const prepared = await prepare(candidate, f);
  assert.equal(prepared.revision, 1);
  const options = { ...f, importer: async r => { order.push(r.deliveryState); assert.equal(r.detail, candidate.detailedMarkdown); return saved(r); },
    publisher: async j => { order.push('space'); assert.deepEqual(j.source.gitChanges, f.gitChanges); return published(j); } };
  await sync(options);
  await prepare(candidate, f);
  await sync(options);
  assert.deepEqual(order, ['pending', 'space', 'synced']);
});

test('invalid commit ancestry, hashes and selected paths fail closed before any delivery', async t => {
  const f = setup(t);
  for (const evidence of [
    { ...f.gitChanges, base: 'HEAD~1' },
    { ...f.gitChanges, head: 'f'.repeat(40) },
    { base: f.head, head: f.base, paths: ['src/example.js'] },
    { ...f.gitChanges, paths: ['src/unchanged.js'] },
    { ...f.gitChanges, paths: ['../src/example.js'] },
  ]) assert.equal((await prepare(input(f, evidence), f)).skipped, true);
  assert.deepEqual(status(f), []);
});

test('sync excludes legacy or altered pending evidence before native registration and model calls', async t => {
  const f = setup(t), candidate = input(f);
  await prepare(candidate, f);
  const file = path.join(f.stateDir, candidate.workId, 'state.json');
  const state = JSON.parse(fs.readFileSync(file, 'utf8'));
  delete state.source.gitChanges;
  state.localSaved = true;
  state.phase = 'publishing';
  state.publicationUncertain = true;
  state.nextAttemptAt = Date.now() + 86400000;
  fs.writeFileSync(file, JSON.stringify(state));
  await sync({ ...f, exe: path.join(f.stateDir, 'must-not-run.exe'), publisher: async () => { throw Error('must not call model'); } });
  const excluded = JSON.parse(fs.readFileSync(file, 'utf8'));
  assert.equal(excluded.phase, 'excluded');
  assert.equal(excluded.publicationUncertain, true);
  assert.equal(excluded.source.detailedMarkdown, state.source.detailedMarkdown);
  assert.equal(excluded.modelCalls, 0);
  assert.equal(JSON.parse(fs.readFileSync(path.join(f.stateDir, 'wake.json'), 'utf8')).pending, 0);
  await sync({ ...f, retryNow: true, exe: 'must-not-run.exe' });
});

test('excluded older revision does not block a valid later development revision', async t => {
  const f = setup(t), candidate = input(f);
  await prepare(candidate, f);
  const file = path.join(f.stateDir, candidate.workId, 'state.json');
  const old = JSON.parse(fs.readFileSync(file, 'utf8'));
  delete old.source.gitChanges;
  fs.writeFileSync(file, JSON.stringify(old));
  await prepare({ ...candidate, detailedMarkdown: candidate.detailedMarkdown + '\n\n후속 개발을 확인했다.' }, f);
  const calls = [];
  await sync({ ...f, importer: async r => saved(r), publisher: async j => { calls.push(j.revision); return published(j); } });
  assert.deepEqual(calls, [2]);
  assert.deepEqual(status(f).map(s => s.phase), ['excluded', 'complete']);
});

test('completed and human-conflicted legacy records are preserved without publication', async t => {
  const f = setup(t);
  const originals = [];
  for (const phase of ['complete', 'conflict']) {
    const candidate = input(f);
    await prepare(candidate, f);
    const file = path.join(f.stateDir, candidate.workId, 'state.json');
    const state = JSON.parse(fs.readFileSync(file, 'utf8'));
    delete state.source.gitChanges; state.phase = phase;
    fs.writeFileSync(file, JSON.stringify(state));
    originals.push([file, fs.readFileSync(file, 'utf8')]);
  }
  await sync({ ...f, exe: 'must-not-run.exe', retryNow: true });
  for (const [file, original] of originals) assert.equal(fs.readFileSync(file, 'utf8'), original);
});

test('sync rechecks selected commits and paths instead of trusting prepared evidence', async t => {
  const f = setup(t), candidate = input(f);
  await prepare(candidate, f);
  const file = path.join(f.stateDir, candidate.workId, 'state.json');
  const state = JSON.parse(fs.readFileSync(file, 'utf8'));
  state.source.gitChanges.paths = ['src/unchanged.js'];
  fs.writeFileSync(file, JSON.stringify(state));
  await sync({ ...f, exe: 'must-not-run.exe' });
  assert.equal(status(f)[0].phase, 'excluded');
  assert.equal(status(f)[0].exclusionReason, 'unchanged_selected_paths');
});

test('re-preparing excluded uncertain work keeps recovery-only publication and the observed page', async t => {
  const f = setup(t), candidate = input(f);
  await prepare(candidate, f);
  const file = path.join(f.stateDir, candidate.workId, 'state.json');
  const state = JSON.parse(fs.readFileSync(file, 'utf8'));
  delete state.source.gitChanges;
  state.phase = 'publishing'; state.publicationUncertain = true;
  state.space = published(state);
  fs.writeFileSync(file, JSON.stringify(state));
  await sync({ ...f, exe: 'must-not-run.exe' });
  await prepare(candidate, f);
  let recoveryOnly;
  await sync({ ...f, importer: async r => saved(r), publisher: async (j, o) => {
    recoveryOnly = o.recoverOnly;
    assert.equal(j.space.pageId, state.space.pageId);
    return published(j);
  } });
  assert.equal(recoveryOnly, true);
});

test('a new revision prepared before exclusion inherits the old uncertain publication', async t => {
  const f = setup(t), candidate = input(f);
  await prepare(candidate, f);
  const file = path.join(f.stateDir, candidate.workId, 'state.json');
  const state = JSON.parse(fs.readFileSync(file, 'utf8'));
  delete state.source.gitChanges;
  state.phase = 'publishing'; state.publicationUncertain = true;
  state.space = published(state);
  fs.writeFileSync(file, JSON.stringify(state));
  await prepare({ ...candidate, detailedMarkdown: candidate.detailedMarkdown + '\n\n후속 수정도 완료했다.' }, f);
  const calls = [];
  await sync({ ...f, importer: async r => saved(r), publisher: async (j, o) => {
    calls.push({ revision: j.revision, recoverOnly: o.recoverOnly, pageId: j.space?.pageId });
    return published(j);
  } });
  assert.deepEqual(calls, [{ revision: 2, recoverOnly: true, pageId: state.space.pageId }]);
});

test('legacy schema-v1 publishing is normalized before exclusion and later re-preparation', async t => {
  const f = setup(t), candidate = input(f);
  await prepare(candidate, f);
  const file = path.join(f.stateDir, candidate.workId, 'state.json');
  const state = JSON.parse(fs.readFileSync(file, 'utf8'));
  delete state.source.gitChanges;
  state.schemaVersion = 1; state.phase = 'publishing';
  delete state.publicationUncertain;
  fs.writeFileSync(file, JSON.stringify(state));
  await sync({ ...f, exe: 'must-not-run.exe' });
  await prepare({ ...candidate, detailedMarkdown: candidate.detailedMarkdown + '\n\n후속 수정도 완료했다.' }, f);
  let recoveryOnly;
  await sync({ ...f, importer: async r => saved(r), publisher: async (j, o) => { recoveryOnly = o.recoverOnly; return published(j); } });
  assert.equal(recoveryOnly, true);
});
