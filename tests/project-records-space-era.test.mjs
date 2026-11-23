import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { prepare, sync, status } from '../scripts/project-records.mjs';
import { setup, input, load, appHolding, jobFiles, readJob, snapshot, WORK_ID, NEWER } from './helpers/space-era.mjs';

// Space publishing is gone. These start from state files the last version
// with Space wrote (tests/fixtures/space-era) and run this version on them.

const refuse = () => { throw new Error('a record that is already in the diary must not be sent again'); };
const wake = f => JSON.parse(fs.readFileSync(path.join(f.stateDir, 'wake.json'), 'utf8'));
const numbers = f => jobFiles(f).map(file => readJob(f, file).localRevision);
const pageUrl = 'https://chatgpt.com/space/page_1234567890abcdef1234567890abcdef';

for (const [name, phase, link] of [['complete', 'complete', pageUrl], ['local-complete', 'local_complete', null], ['space-conflict', 'conflict', null]]) {
  test(`a finished ${name} record is left exactly as it was written`, async t => {
    const f = setup(t); load(name, f);
    const before = snapshot(f);
    for (const options of [{}, { retryNow: true }, { workId: WORK_ID, retryNow: true }, { maxJobs: 1 }]) {
      const results = await sync({ ...f, ...options, importer: refuse });
      assert.deepEqual(results.map(r => [r.phase, r.skipped]), [[phase, true]]);
    }
    assert.deepEqual(snapshot(f), before);
    assert.equal(status(f)[0].phase, phase);
    assert.equal(status(f)[0].pageUrl, link);
    assert.equal(wake(f).pending, 0);
  });
}

for (const name of ['complete', 'local-complete']) {
  test(`preparing the same input again over a ${name} record is unchanged and writes nothing`, async t => {
    const f = setup(t); load(name, f);
    const before = snapshot(f);
    const again = await prepare(input(f), f);
    assert.equal(again.unchanged, true);
    assert.equal(again.revision, 1);
    assert.deepEqual(snapshot(f), before);
  });
}

// The diary took these captures while they waited for a Space page.
for (const name of ['waiting-space', 'publishing-uncertain', 'space-saved', 'queued-linked', 'waiting-prior']) {
  test(`a ${name} record that is already in the diary ends without being sent again`, async t => {
    const f = setup(t); load(name, f);
    const before = numbers(f);
    const results = await sync({ ...f, retryNow: true, importer: refuse });
    assert.equal(results.length, jobFiles(f).length);
    assert.ok(results.every(r => r.phase === 'local_complete' && r.error === null && !r.skipped), JSON.stringify(results));
    assert.deepEqual(numbers(f), before, 'a delivery number changed');
    assert.equal(wake(f).pending, 0);
    const after = snapshot(f);
    await sync({ ...f, retryNow: true, importer: refuse });
    assert.deepEqual(snapshot(f), after);
  });
}

test('a Space page a record got before the removal is still reported', async t => {
  const f = setup(t); load('space-saved', f);
  await sync({ ...f, retryNow: true, importer: refuse });
  assert.deepEqual(status(f).map(s => [s.phase, s.pageUrl]), [['local_complete', pageUrl]]);
});

test('a record the app holds in its inbox is sent again under the same number with the same content', async t => {
  const f = setup(t), app = load('queued-first', f);
  const held = structuredClone(app.inbox['1']);
  await sync({ ...f, retryNow: true, importer: app.importer });
  assert.deepEqual(app.sent, [held]);
  assert.equal(status(f)[0].phase, 'local_complete');
  assert.equal(readJob(f, 'state.json').localRevision, 1);
  assert.equal(wake(f).pending, 0);
});

test('a record prepared before the removal and never sent is delivered under its own number', async t => {
  const f = setup(t), app = load('prepared-unsent', f);
  await sync({ ...f, importer: app.importer });
  assert.deepEqual(app.sent.map(r => [r.revision, r.deliveryState, r.detail, r.spaceUrl]), [[1, 'local', input(f).detailedMarkdown, '']]);
  assert.equal(status(f)[0].phase, 'local_complete');
});

// A version before 0.32.0 renumbers a job that has no diaryMode when it runs
// with Space off. Marked 'local', the job keeps its number there too.
test('a record prepared before the removal and never sent is marked to keep its number, by sync and by the next prepare', async t => {
  for (const touch of [f => sync({ ...f, gitCheckBudgetMs: 0, importer: refuse }), f => prepare({ ...input(f), detailedMarkdown: input(f).detailedMarkdown + NEWER }, f)]) {
    const f = setup(t); load('prepared-unsent', f);
    assert.equal(readJob(f, 'state.json').diaryMode, undefined);
    await touch(f);
    const first = readJob(f, jobFiles(f)[0]);
    assert.deepEqual([first.revision, first.localRevision, first.diaryMode, first.phase], [1, 1, 'local', 'prepared']);
  }
  // One that may already have sent 'pending' under its number is not.
  const f = setup(t); load('queued-first', f);
  await sync({ ...f, gitCheckBudgetMs: 0, importer: refuse });
  assert.equal(readJob(f, 'state.json').diaryMode, undefined);
});

// docs/record-delivery-redesign.md: with Space on, Git could not check the
// first capture, so only the second reached the diary. Turning Space off then
// gave the first capture a higher number and it replaced the second.
for (const options of [{ retryNow: true }, { retryNow: true, maxJobs: 1 }]) {
  test(`an earlier capture that was overtaken cannot replace the newer one the diary shows (${JSON.stringify(options)})`, async t => {
    const f = setup(t), app = load('overtaken-capture', f);
    assert.match(app.detail, /후속 수정/);
    const before = numbers(f);
    for (let run = 0; run < 3; run++) await sync({ ...f, ...options, importer: app.importer });
    assert.deepEqual(app.shown, [], 'the diary changed');
    assert.match(app.detail, /후속 수정/);
    assert.deepEqual(numbers(f), before, 'a delivery number changed');
    assert.deepEqual(status(f).map(s => s.phase), ['local_complete', 'local_complete']);
    assert.equal(wake(f).pending, 0);
  });

  // One step further in that version: with Space off and Git failing it gave
  // both captures new numbers and sent nothing. The first capture's number (5)
  // is above the one the diary shows the second under (3).
  test(`an earlier capture that the old version renumbered above the newer one is not sent (${JSON.stringify(options)})`, async t => {
    const f = setup(t), app = load('renumbered-overtaken', f);
    assert.match(app.detail, /후속 수정/);
    assert.deepEqual(numbers(f), [5, 7]);
    for (let run = 0; run < 3; run++) await sync({ ...f, ...options, importer: app.importer });
    assert.deepEqual(app.rollbacks, []);
    assert.deepEqual(app.sent.map(r => r.revision), [7]);
    assert.ok(app.shown.every(detail => /후속 수정/.test(detail)), 'the earlier capture was shown');
    assert.match(app.detail, /후속 수정/);
    assert.deepEqual(numbers(f), [5, 7], 'a delivery number changed');
    assert.deepEqual(status(f).map(s => s.phase), ['local_complete', 'local_complete']);
    assert.equal(wake(f).pending, 0);
  });
}

const eras = ['complete', 'local-complete', 'space-conflict', 'waiting-space', 'publishing-uncertain', 'space-saved', 'queued-first', 'queued-linked', 'waiting-prior', 'prepared-unsent', 'overtaken-capture', 'renumbered-overtaken'];
for (const name of eras) {
  test(`a new capture over a ${name} record gets a number above every number the work used`, async t => {
    const f = setup(t), app = load(name, f);
    const used = jobFiles(f).map(file => readJob(f, file)).flatMap(job => [job.localRevision, job.finalRevision, job.diaryReceipt?.revision ?? 0]);
    const link = status(f).map(s => s.pageUrl).findLast(Boolean) ?? null, shownLink = app.spaceUrl || null;
    const latest = jobFiles(f).includes('pending-1.json') ? input(f).detailedMarkdown + NEWER : input(f).detailedMarkdown;
    const next = { ...input(f), detailedMarkdown: latest + '\n\n제거 뒤에 덧붙인 내용.' };
    await prepare(next, f);
    const job = readJob(f, 'state.json');
    assert.ok(job.localRevision > Math.max(...used), `${job.localRevision} is not above ${used}`);
    for (let run = 0; run < 3; run++) await sync({ ...f, retryNow: true, importer: app.importer });
    assert.equal(app.detail, next.detailedMarkdown);
    assert.deepEqual(app.rollbacks, [], 'the diary went back to an earlier capture');
    assert.equal(status(f).at(-1).phase, 'local_complete');
    assert.equal(wake(f).pending, 0);
    // A Space link the work had is still reported and still on the diary entry.
    assert.equal(status(f).at(-1).pageUrl, link);
    assert.equal(app.spaceUrl || null, shownLink);
  });
}

test('a job from before delivery numbers were stored is sent under revision + 1', async t => {
  const f = setup(t), app = appHolding();
  await prepare(input(f), f);
  const file = path.join(f.stateDir, WORK_ID, 'state.json');
  const { schemaVersion, localRevision, finalRevision, diaryMode, localSaved, ...old } = readJob(f, 'state.json');
  fs.writeFileSync(file, JSON.stringify({ ...old, revision: 4 }));
  await sync({ ...f, importer: app.importer });
  assert.deepEqual(app.sent.map(r => r.revision), [5]);
  assert.equal(status(f)[0].phase, 'local_complete');
  await prepare({ ...input(f), detailedMarkdown: input(f).detailedMarkdown + NEWER }, f);
  assert.ok(readJob(f, 'state.json').localRevision > 5);
});
