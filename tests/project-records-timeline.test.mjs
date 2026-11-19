import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { randomUUID } from 'node:crypto';
import { prepare, sync, diaryPayload, beginReceipt, ownedCommitLine, isInstant, OWNED_COMMITS_LINE } from '../scripts/project-records.mjs';
import { webRemote } from '../scripts/project-records-git.mjs';
import { completionTime, decideStatus, firstCompletion, readPreviousDelivery } from '../scripts/development-diary.mjs';
import { git, gitFixture } from './helpers/record-git-fixture.mjs';

const OWNER = 'record-test@example.invalid';
const STARTED = '2026-11-19T00:05:00.000Z';

function setup(t) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'timeschedule-timeline-'));
  t.after(() => fs.rmSync(dir, { recursive: true, force: true }));
  const f = gitFixture(dir);
  const workId = randomUUID();
  const stateDir = path.join(dir, 'state');
  const commit = (file, subject, { id = workId, author } = {}) => {
    fs.mkdirSync(path.dirname(path.join(f.repo, file)), { recursive: true });
    fs.writeFileSync(path.join(f.repo, file), `${subject}\n`);
    git(f.repo, 'add', '--', file);
    const args = ['commit', '-qm', subject];
    if (id) args.push('--trailer', `Diary-Work-Id: ${id}`);
    if (author) args.push('--author', author);
    git(f.repo, ...args);
    return git(f.repo, 'rev-parse', 'HEAD');
  };
  const receipt = (at = STARTED, id = workId) => {
    const directory = path.join(stateDir, 'diary-starts', randomUUID());
    fs.mkdirSync(directory, { recursive: true });
    fs.writeFileSync(path.join(directory, 'start.json'), JSON.stringify({ branch: 'codex/x', owner: OWNER, base: f.head, at }));
    fs.writeFileSync(path.join(directory, 'work.json'), JSON.stringify({ workId: id }));
  };
  const input = (head, fields = {}) => ({ workId, projectTitle: 'TimeSchedule', repoPath: f.repo, date: '2026-11-19',
    kind: 'development', title: '기록 시각', status: 'verification_pending',
    detailedMarkdown: '시각을 저장했다. 실제 화면은 미확인.', gitChanges: { base: f.head, head, paths: ['src/time.js'] }, ...fields });
  const options = { stateDir, projectRoot: f.repo, vaultRoot: path.join(dir, 'vault'), spaceEnabled: false };
  const state = () => JSON.parse(fs.readFileSync(path.join(stateDir, workId, 'state.json'), 'utf8'));
  return { ...f, workId, stateDir, commit, receipt, input, options, state };
}
const delivered = async options => {
  const records = [];
  await sync({ ...options, importer: async r => { records.push(r); return { outcome: 'saved', workId: r.workId, revision: r.revision, entryId: 'entry-1' }; } });
  return records;
};

test('a record carries its begin receipt and the owned commits Git confirms, never caller text', async t => {
  const f = setup(t);
  f.receipt();
  const first = f.commit('src/time.js', '시각 저장 추가');
  const last = f.commit('src/time.js', '시각 표시 수정');
  await prepare(f.input(last, { commits: [first, { hash: last, subject: 'invented', committedAt: '1999-01-01T00:00:00Z' }] }), f.options);
  const { source } = f.state();
  assert.equal(source.startedAt, STARTED);
  assert.deepEqual(source.commits.map(c => c.hash), [first, last]);
  assert.deepEqual(source.commits.map(c => c.subject), ['시각 저장 추가', '시각 표시 수정']);
  for (const c of source.commits) {
    assert.ok(isInstant(c.committedAt), c.committedAt);
    assert.equal(c.committedAt, git(f.repo, 'show', '-s', '--format=%cI', c.hash));
    assert.equal(c.url, undefined, 'no origin, no link');
  }
  assert.equal(source.completedAt, undefined, 'pending work has no completion time');
  const [record] = await delivered(f.options);
  assert.equal(record.schemaVersion, 3);
  assert.equal(record.startedAt, STARTED);
  assert.deepEqual(record.commits, source.commits);
  assert.equal(record.completedAt, undefined);
  // The same input again is unchanged: nothing moves the recorded times.
  const again = await prepare(f.input(last, { commits: [first, last] }), f.options);
  assert.equal(again.unchanged, true, JSON.stringify(again));
});

test('commits of another author, another work, or no work are refused before anything is saved', async t => {
  const f = setup(t);
  f.receipt();
  const mine = f.commit('src/time.js', '내 커밋');
  const theirs = f.commit('src/other.js', '남의 커밋', { author: 'Other <other@example.invalid>' });
  const untagged = f.commit('src/plain.js', '태그 없음', { id: null });
  const otherWork = f.commit('src/work.js', '다른 작업', { id: randomUUID() });
  for (const foreign of [theirs, untagged, otherWork, f.base, 'f'.repeat(40)]) {
    await assert.rejects(prepare(f.input(otherWork, { commits: [mine, foreign] }), f.options), /소유한 실제 커밋/, foreign);
  }
  assert.equal(fs.existsSync(path.join(f.stateDir, f.workId)), false);
});

test('completion is only a confirmed moment of a completed status', async t => {
  const f = setup(t);
  f.receipt();
  const head = f.commit('src/time.js', '완료 확인');
  await assert.rejects(prepare(f.input(head, { completedAt: '2026-11-19T10:00:00+09:00' }), f.options), /completedAt/);
  await assert.rejects(prepare(f.input(head, { status: 'verified', completedAt: '2026-11-19 10:00' }), f.options), /completedAt/);
  await assert.rejects(prepare(f.input(head, { status: 'verified', completedAt: '2026-11-18T10:00:00+09:00' }), f.options), /작업 시작/);
  await prepare(f.input(head, { status: 'verified', completedAt: '2026-11-19T10:00:00+09:00' }), f.options);
  const [record] = await delivered(f.options);
  assert.equal(record.completedAt, '2026-11-19T10:00:00+09:00');
  assert.equal(record.status, 'verified');

  const at = () => '2026-11-19T12:00:00.000Z';
  const first = '2026-11-19T01:00:00.000Z';
  const previous = { ownedHead: head, source: { status: 'verified', completedAt: first } };
  assert.equal(completionTime(() => previous, head, 'verified', at), first, 'retry keeps it');
  // A follow-up commit, an edit or a merge reflected later keeps the first moment.
  assert.equal(completionTime(() => previous, 'a'.repeat(40), 'verified', at), first);
  assert.equal(completionTime(() => previous, head, 'merged', at), first);
  assert.equal(completionTime(() => previous, head, 'verification_pending', at), null);
  // The first moment survives a pending delivery in between.
  const between = { ownedHead: head, firstCompletedAt: first, source: { status: 'verification_pending' } };
  assert.deepEqual(firstCompletion(between), { at: first, unknown: false });
  assert.equal(completionTime(() => between, 'b'.repeat(40), 'merged', at), first);
  // An unreadable earlier delivery is not "none": it is never a first completion.
  assert.throws(() => completionTime(() => { throw new Error('unreadable'); }, head, 'merged', at), /unreadable/);
  assert.equal(completionTime(() => null, head, 'verified', at), at(), 'the first real completion');
});

test('a delivery saved before completion times stays unknown when finish runs again', () => {
  const at = () => '2026-11-19T12:00:00.000Z';
  const head = 'c'.repeat(40);
  // Already complete, but no moment was ever saved: re-running is not completing.
  const legacy = { ownedHead: head, source: { status: 'verified' } };
  assert.deepEqual(firstCompletion(legacy), { at: null, unknown: true });
  for (const status of ['verified', 'completed', 'merged']) {
    assert.equal(completionTime(() => legacy, head, status, at), null, status);
    assert.equal(completionTime(() => legacy, 'd'.repeat(40), status, at), null, status);
  }
  // Once marked unknown it stays unknown, even after a pending delivery.
  const marked = { ownedHead: head, completionUnknown: true, source: { status: 'verification_pending' } };
  assert.equal(completionTime(() => marked, head, 'merged', at), null);
  // A legacy delivery that was still waiting completes now, for the first time.
  const waiting = { ownedHead: head, source: { status: 'verification_pending' } };
  assert.equal(completionTime(() => waiting, head, 'verified', at), at());
});

test('only a missing earlier delivery reads as none; other failures are raised and the file kept', t => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'timeschedule-delivery-'));
  t.after(() => fs.rmSync(dir, { recursive: true, force: true }));
  const file = path.join(dir, 'delivery.json');
  assert.equal(readPreviousDelivery(file), null);
  const saved = { ownedHead: 'a'.repeat(40), firstCompletedAt: '2026-11-19T01:00:00.000Z', source: { status: 'verified' } };
  fs.writeFileSync(file, '﻿' + JSON.stringify(saved));
  assert.deepEqual(readPreviousDelivery(file), saved);
  fs.writeFileSync(file, '{"source":');
  assert.throws(() => readPreviousDelivery(file), /손상/);
  assert.equal(fs.readFileSync(file, 'utf8'), '{"source":');
  // Valid JSON that lost the earlier source is damaged too, never "none".
  for (const lost of ['null', '{}', '[]', '{"source":null}', '{"source":{}}', '{"source":{"status":"invented"}}']) {
    fs.writeFileSync(file, lost);
    assert.throws(() => readPreviousDelivery(file), /손상/, lost);
    assert.equal(fs.readFileSync(file, 'utf8'), lost, lost);
    assert.throws(() => completionTime(() => readPreviousDelivery(file), 'a'.repeat(40), 'verified', () => 'new-finish-time'), /손상/, lost);
  }
  // A genuine older source without completion fields is still accepted.
  const older = { ownedHead: 'a'.repeat(40), source: { workId: 'w', status: 'verified' } };
  fs.writeFileSync(file, JSON.stringify(older));
  assert.deepEqual(readPreviousDelivery(file), older);
  assert.equal(completionTime(() => readPreviousDelivery(file), 'a'.repeat(40), 'verified', () => 'new-finish-time'), null);
  fs.rmSync(file);
  fs.mkdirSync(file);
  assert.throws(() => readPreviousDelivery(file), error => error.cause?.code !== undefined && error.cause.code !== 'ENOENT');
  assert.equal(fs.statSync(file).isDirectory(), true);
});

test('an explicit waiting status is kept even when replayed tests pass', () => {
  const passed = { exitCode: 0, counts: { pass: 3 } };
  const failed = { exitCode: 1, counts: { pass: 2 } };
  assert.equal(decideStatus({ status: 'verification_pending' }, passed), 'verification_pending');
  assert.equal(decideStatus({ status: 'in_progress' }, passed), 'in_progress');
  // Passing tests verify the checked scope; an explicit completion is its own claim.
  assert.equal(decideStatus({}, passed), 'verified');
  assert.equal(decideStatus({ status: 'merged' }, passed), 'merged');
  assert.equal(decideStatus({ status: 'completed' }, failed), 'verification_pending');
  assert.equal(decideStatus({}, failed), 'verification_pending');
  // Without replayed tests a completion needs a written verification.
  assert.equal(decideStatus({ status: 'completed' }, null), 'verification_pending');
  assert.equal(decideStatus({ status: 'completed', verification: '사용자 확인' }, null), 'completed');
  assert.equal(decideStatus({ status: 'invented' }, null), 'verification_pending');
});

test('earlier sources keep their v2 payload; an earlier owned-commit line is restored only if it verifies', async t => {
  const f = setup(t);
  const head = f.commit('src/time.js', '이전 작업');
  // No receipt, no commits: the same source and payload as before this change.
  await prepare(f.input(head), f.options);
  assert.deepEqual(Object.keys(f.state().source).filter(k => ['startedAt', 'completedAt', 'commits'].includes(k)), []);
  const [legacy] = await delivered(f.options);
  assert.equal(legacy.schemaVersion, 2);
  for (const key of ['startedAt', 'completedAt', 'commits']) assert.equal(key in legacy, false, key);

  const g = setup(t);
  g.receipt();
  const owned = g.commit('src/time.js', '이전 finish 커밋');
  const markdown = `작업 내용. 화면은 미확인.\n\n${OWNED_COMMITS_LINE}${owned}`;
  assert.deepEqual(ownedCommitLine(markdown), [owned]);
  await prepare(g.input(owned, { detailedMarkdown: markdown }), g.options);
  assert.deepEqual(g.state().source.commits.map(c => c.hash), [owned]);
  // A line naming a commit this work does not own adds nothing and blocks nothing.
  const h = setup(t);
  const other = h.commit('src/time.js', '남의 작업', { id: randomUUID() });
  await prepare(h.input(other, { detailedMarkdown: `요약.\n\n${OWNED_COMMITS_LINE}${other}` }), h.options);
  assert.equal(h.state().source.commits, undefined);
});

test('begin receipts and remote links are read conservatively', t => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'timeschedule-receipt-'));
  t.after(() => fs.rmSync(dir, { recursive: true, force: true }));
  const id = randomUUID();
  assert.equal(beginReceipt(dir, id), null);
  const write = (name, start, work) => {
    const d = path.join(dir, 'diary-starts', name);
    fs.mkdirSync(d, { recursive: true });
    fs.writeFileSync(path.join(d, 'start.json'), start);
    fs.writeFileSync(path.join(d, 'work.json'), work);
  };
  write('broken', '{', '{');
  write('other', JSON.stringify({ at: STARTED, owner: 'A@B' }), JSON.stringify({ workId: randomUUID() }));
  assert.equal(beginReceipt(dir, id), null);
  write('mine', JSON.stringify({ at: STARTED, owner: 'Me@Example.invalid' }), JSON.stringify({ workId: id }));
  assert.deepEqual(beginReceipt(dir, id), { at: STARTED, owner: 'me@example.invalid' });

  assert.equal(webRemote('git@github.com:o/r.git'), 'https://github.com/o/r');
  assert.equal(webRemote('https://github.com/o/r.git/'), 'https://github.com/o/r');
  for (const unsafe of ['https://user:token@github.com/o/r', 'http://github.com/o/r', 'C:\\repo', 'file:///repo', 'https://github.com/o/r?x=1']) {
    assert.equal(webRemote(unsafe), '', unsafe);
  }
  const payload = diaryPayload({ source: { workId: id, projectTitle: 'T', repoPath: dir, date: '2026-11-19', kind: 'development', title: 't',
    diarySummary: 's', detailedMarkdown: 's', status: 'verified', completedAt: '2026-11-19T10:00:00+09:00' }, localRevision: 1, diaryMode: 'local' }, false);
  assert.equal(payload.schemaVersion, 3);
  assert.equal(payload.completedAt, '2026-11-19T10:00:00+09:00');
});
