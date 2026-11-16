import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { randomUUID } from 'node:crypto';
import { prepare, sync, status, PARENT_PAGE_ID } from '../scripts/project-records.mjs';
import { developmentEvidence } from '../scripts/project-records-git.mjs';
import { gitFixture, git } from './helpers/record-git-fixture.mjs';

// A Git check that fails for a reason outside the selected work (Git missing
// from PATH, the repository briefly unreadable, a timeout) says nothing about
// whether the work has development changes. Such a record must be retried, not
// excluded forever. Only Git's own definite answers exclude it.

function setup(t) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'timeschedule-git-retry-'));
  t.after(() => fs.rmSync(dir, { recursive: true, force: true }));
  const fixture = gitFixture(dir);
  const dotGit = path.join(fixture.repo, '.git');
  const away = path.join(fixture.repo, '.git-away');
  return { ...fixture, dir, stateDir: path.join(dir, 'state'), projectRoot: fixture.repo, vaultRoot: path.join(dir, 'vault'), spaceEnabled: false,
    breakRepo: () => fs.renameSync(dotGit, away), fixRepo: () => fs.renameSync(away, dotGit) };
}
function input(f, gitChanges = f.gitChanges) {
  return { workId: randomUUID(), projectTitle: 'TimeSchedule', repoPath: f.repo, date: '2026-11-16',
    kind: 'development', title: 'Git 일시 실패 기록', status: 'verification_pending',
    detailedMarkdown: '코드를 수정하고 테스트했다. 실제 UI 동작은 미확인.', gitChanges };
}
const saved = r => ({ outcome: 'saved', workId: r.workId, revision: r.revision, entryId: 'entry-1' });
const published = j => ({ pageId: 'page_1234567890abcdef1234567890abcdef', pageUrl: 'https://chatgpt.com/space/page_1234567890abcdef1234567890abcdef',
  parentPageId: PARENT_PAGE_ID, title: j.source.title, markdown: j.pageMarkdown, indexVerified: true });
const refuse = () => { throw new Error('must not deliver unverified work'); };
const readState = (f, workId) => JSON.parse(fs.readFileSync(path.join(f.stateDir, workId, 'state.json'), 'utf8'));
const wake = f => JSON.parse(fs.readFileSync(path.join(f.stateDir, 'wake.json'), 'utf8'));
function withoutGitOnPath(action) {
  const original = process.env.PATH;
  process.env.PATH = '';
  try { return action(); } finally { process.env.PATH = original; }
}

test('Git missing from PATH is a temporary failure, not missing evidence', t => {
  const f = setup(t);
  const result = withoutGitOnPath(() => developmentEvidence(f.repo, f.gitChanges, f.repo));
  assert.equal(result.eligible, false);
  assert.equal(result.transient, true);
  assert.equal(result.reason, 'git_unavailable');
  assert.match(result.detail, /ENOENT/);
});

test('an unreadable repository is temporary even inside another repository', t => {
  const f = setup(t);
  // A parent folder that is itself a repository must not be taken as the
  // repository while this one's .git is briefly unavailable.
  git(f.dir, 'init', '-q');
  f.breakRepo();
  const result = developmentEvidence(f.repo, f.gitChanges, f.repo);
  assert.equal(result.transient, true, JSON.stringify(result));
  assert.equal(result.reason, 'git_unavailable');
  assert.match(result.detail, /128/);
});

test('a repository folder that is briefly missing is temporary', t => {
  const f = setup(t);
  const moved = path.join(f.dir, 'moved');
  fs.renameSync(f.repo, moved);
  const result = developmentEvidence(f.repo, f.gitChanges, f.repo);
  assert.equal(result.transient, true, JSON.stringify(result));
});

test('Git definite answers still fail closed with their own reasons', t => {
  const f = setup(t);
  const missing = developmentEvidence(f.repo, { ...f.gitChanges, head: 'f'.repeat(40) }, f.repo);
  assert.deepEqual(missing, { eligible: false, reason: 'commit_not_found' });
  const backwards = developmentEvidence(f.repo, { base: f.head, head: f.base, paths: ['src/example.js'] }, f.repo);
  assert.deepEqual(backwards, { eligible: false, reason: 'not_ancestor' });
  const tree = git(f.repo, 'rev-parse', `${f.head}^{tree}`);
  assert.deepEqual(developmentEvidence(f.repo, { ...f.gitChanges, head: tree }, f.repo), { eligible: false, reason: 'invalid_git_changes' });
});

test('prepare reports a temporary Git failure as an error and saves nothing', async t => {
  const f = setup(t);
  await assert.rejects(withoutGitOnPath(() => prepare(input(f), f)), /Git.*ENOENT/s);
  assert.equal(fs.existsSync(f.stateDir), false);
});

test('a temporary Git failure during sync retries on the backoff ladder and delivers once Git works', async t => {
  const f = setup(t), candidate = input(f);
  await prepare(candidate, f);
  f.breakRepo();
  const before = Date.now();
  await sync({ ...f, importer: refuse });
  let state = readState(f, candidate.workId);
  assert.equal(state.phase, 'prepared');
  assert.equal(state.exclusionReason, undefined);
  assert.equal(state.gitFailures, 1);
  assert.match(state.lastError, /Git/);
  assert.ok(state.nextAttemptAt >= before + 60000 && state.nextAttemptAt <= Date.now() + 60000);
  assert.deepEqual(wake(f), { pending: 1, nextAttemptAt: state.nextAttemptAt });

  // Not due yet: checking it again must not push its retry further back.
  await sync({ ...f, importer: refuse });
  assert.equal(readState(f, candidate.workId).gitFailures, 1);

  // An explicit retry still re-verifies first and never delivers unverified work.
  const second = Date.now();
  const results = await sync({ ...f, retryNow: true, importer: refuse });
  state = readState(f, candidate.workId);
  assert.equal(state.gitFailures, 2);
  assert.ok(state.nextAttemptAt >= second + 300000);
  assert.match(results[0].error, /Git/);

  f.fixRepo();
  const delivered = [];
  await sync({ ...f, retryNow: true, importer: async r => { delivered.push(r.revision); return saved(r); } });
  state = readState(f, candidate.workId);
  assert.equal(state.phase, 'local_complete');
  assert.equal(state.gitFailures, 0);
  assert.equal(state.lastError, null);
  assert.equal(delivered.length, 1);
});

test('long Git failure pauses retries without waking the app and resumes on a later sync', async t => {
  const f = setup(t), candidate = input(f);
  await prepare(candidate, f);
  f.breakRepo();
  for (let i = 0; i < 12; i++) await sync({ ...f, retryNow: true, importer: refuse });
  let state = readState(f, candidate.workId);
  assert.equal(state.phase, 'git_paused');
  assert.equal(state.pausedFromPhase, 'prepared');
  assert.equal(state.nextAttemptAt, null);
  assert.match(state.lastError, /멈췄/);
  assert.deepEqual(wake(f), { pending: 0, nextAttemptAt: null });
  assert.equal(status(f)[0].phase, 'git_paused');

  // Still broken: a sync rechecks it but leaves it paused and the app asleep.
  const paused = fs.readFileSync(path.join(f.stateDir, candidate.workId, 'state.json'), 'utf8');
  await sync({ ...f, retryNow: true, importer: refuse });
  assert.equal(fs.readFileSync(path.join(f.stateDir, candidate.workId, 'state.json'), 'utf8'), paused);
  assert.equal(wake(f).pending, 0);

  f.fixRepo();
  await sync({ ...f, importer: async r => saved(r) });
  state = readState(f, candidate.workId);
  assert.equal(state.phase, 'local_complete');
  assert.equal(state.pausedFromPhase, undefined);
});

test('a paused record whose commits turn out to be gone is excluded for that reason', async t => {
  const f = setup(t), candidate = input(f);
  await prepare(candidate, f);
  const file = path.join(f.stateDir, candidate.workId, 'state.json');
  const state = readState(f, candidate.workId);
  Object.assign(state, { phase: 'git_paused', pausedFromPhase: 'publishing', gitFailures: 12, nextAttemptAt: null });
  state.source.gitChanges.head = 'f'.repeat(40);
  fs.writeFileSync(file, JSON.stringify(state));
  await sync({ ...f, importer: refuse });
  const excluded = readState(f, candidate.workId);
  assert.equal(excluded.phase, 'excluded');
  assert.equal(excluded.exclusionReason, 'commit_not_found');
  assert.equal(excluded.excludedFromPhase, 'publishing');
});

test('once Git answers again, a record held back only by Git is delivered without waiting out the delay', async t => {
  const f = setup(t), candidate = input(f);
  await prepare(candidate, f);
  // With Space on, nothing else in sync moves the retry time forward.
  const space = { ...f, spaceEnabled: true, publisher: async j => published(j) };
  f.breakRepo();
  await sync({ ...space, importer: refuse });
  assert.ok(readState(f, candidate.workId).nextAttemptAt > Date.now());
  f.fixRepo();
  await sync({ ...space, importer: async r => saved(r) });
  assert.equal(readState(f, candidate.workId).phase, 'complete');
});
