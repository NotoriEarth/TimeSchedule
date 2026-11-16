import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { randomUUID, createHash } from 'node:crypto';
import { createServer } from 'node:net';
import { prepare, sync, status, PARENT_PAGE_ID, sameResolvedPath } from '../scripts/project-records.mjs';
import { gitFixture } from './helpers/record-git-fixture.mjs';
import { developmentEvidence } from '../scripts/project-records-git.mjs';
import { execFileSync } from 'node:child_process';

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

// Windows can hand the script a short 8.3 path (GitHub's runner temp folder is
// C:\Users\RUNNER~1\...), while Git reports the long one. Both name the same repo.
test('a short Windows path to the repository is the same repository', { skip: process.platform !== 'win32' }, async t => {
  const f = setup(t);
  const script = `(New-Object -ComObject Scripting.FileSystemObject).GetFolder('${f.repo.replaceAll("'", "''")}').ShortPath`;
  const short = execFileSync('powershell.exe', ['-NoProfile', '-NonInteractive', '-Command', script], { encoding: 'utf8', windowsHide: true }).trim();
  if (short.toLowerCase() === f.repo.toLowerCase()) return t.skip('8.3 short names are disabled on this volume');
  assert.deepEqual(developmentEvidence(short, f.gitChanges, short), { eligible: true, gitChanges: f.gitChanges });
  assert.deepEqual(developmentEvidence(short, f.gitChanges, f.repo), { eligible: true, gitChanges: f.gitChanges });
  const prepared = await prepare(input(f), { ...f, projectRoot: short });
  assert.equal(prepared.skipped, undefined, JSON.stringify(prepared));
  assert.equal(prepared.revision, 1);
});

function shortPath(target) {
  const script = `(New-Object -ComObject Scripting.FileSystemObject).GetFolder('${target.replaceAll("'", "''")}').ShortPath`;
  return execFileSync('powershell.exe', ['-NoProfile', '-NonInteractive', '-Command', script], { encoding: 'utf8', windowsHide: true }).trim();
}

// A process from before the canonical-path change names its lock from the
// spelling it was given. Given either spelling, the current version must be
// locked out by such a holder of the same folder.
test('locks from the earlier naming still exclude delivery for both spellings', { skip: process.platform !== 'win32' }, async t => {
  const f = setup(t);
  fs.mkdirSync(f.stateDir, { recursive: true });
  const short = shortPath(f.stateDir);
  if (short.toLowerCase() === f.stateDir.toLowerCase()) return t.skip('8.3 short names are disabled on this volume');
  const open = new Set();
  t.after(() => Promise.all([...open].map(holder => new Promise(resolve => holder.close(resolve)))));
  const hold = async spelling => {
    const legacy = createHash('sha256').update(fs.realpathSync(spelling).toLowerCase()).digest('hex');
    const holder = createServer(socket => socket.destroy());
    await new Promise((resolve, reject) => { holder.once('error', reject); holder.listen({ path: `\\\\.\\pipe\\timeschedule-record-${legacy}`, exclusive: true }, resolve); });
    open.add(holder);
    return () => new Promise(resolve => { open.delete(holder); holder.close(resolve); });
  };
  // An earlier process given the short spelling; this one given the same.
  let release = await hold(short);
  await assert.rejects(prepare(input(f), { ...f, stateDir: short }), /다른 기록 작업이 실행 중/);
  await release();
  // An earlier process given the long spelling; this one given the short one.
  // os.tmpdir() can itself be partly short (C:\Users\RUNNER~1\...), so take the
  // fully long spelling.
  release = await hold(fs.realpathSync.native(f.stateDir));
  await assert.rejects(prepare(input(f), { ...f, stateDir: short }), /다른 기록 작업이 실행 중/);
  await release();
  // Nothing held: delivery proceeds, and releases every name it took.
  assert.equal((await prepare(input(f), { ...f, stateDir: short })).revision, 1);
  assert.equal((await prepare(input(f), f)).revision, 1);
});

// Re-preparing work saved by an earlier version must keep the repository
// spelling it was saved with: the document vault groups records by it.
test('re-preparing keeps the repository spelling an earlier version saved', { skip: process.platform !== 'win32' }, async t => {
  const f = setup(t);
  const short = shortPath(f.repo);
  if (short.toLowerCase() === f.repo.toLowerCase()) return t.skip('8.3 short names are disabled on this volume');
  const candidate = input(f);
  await prepare(candidate, f);
  const file = path.join(f.stateDir, candidate.workId, 'state.json');
  const saved = JSON.parse(fs.readFileSync(file, 'utf8'));
  saved.source.repoPath = short;
  saved.contentHash = createHash('sha256').update(JSON.stringify(saved.source)).digest('hex');
  fs.writeFileSync(file, JSON.stringify(saved));
  const again = await prepare(candidate, f);
  assert.equal(again.unchanged, true, JSON.stringify(again));
  assert.equal(JSON.parse(fs.readFileSync(file, 'utf8')).source.repoPath, short);
});

test('saved repository spellings fold case only on Windows', () => {
  assert.equal(sameResolvedPath('C:\\Work\\Repo', 'c:\\work\\repo', 'win32'), true);
  assert.equal(sameResolvedPath('/work/Repo', '/work/repo', 'linux'), false);
  assert.equal(sameResolvedPath('/work/repo', '/work/repo', 'linux'), true);
});
