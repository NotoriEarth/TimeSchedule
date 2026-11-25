import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { audit, begin, check, context, DIARY_END, finish } from '../scripts/development-diary.mjs';
import { git, gitEvidence, inspectBranch } from '../scripts/development-diary-git.mjs';
import { replayTests } from '../scripts/development-diary-tests.mjs';

function fixture(t) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'diary-repo-'));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  git(root, ['init', '-b', 'codex/sample']);
  git(root, ['config', 'user.name', 'Fixture']);
  git(root, ['config', 'user.email', 'fixture@example.invalid']);
  const write = (name, data) => {
    const dest = path.join(root, name);
    fs.mkdirSync(path.dirname(dest), { recursive: true });
    fs.writeFileSync(dest, data);
  };
  const commit = (name, data, id, author) => {
    write(name, data);
    git(root, ['add', '--', name]);
    const args = ['commit', '-m', name];
    if (id) args.push('--trailer', 'Diary-Work-Id: ' + id);
    if (author) args.push('--author', author);
    git(root, args);
    return git(root, ['rev-parse', 'HEAD']).trim();
  };
  commit('README.md', 'fixture\n');
  const ctx = context(root);
  return { root, ctx, commit, write };
}

function recoveryFixture(t) {
  const { root, ctx, commit, write } = fixture(t);
  const { workId } = begin(ctx);
  const directory = path.join(ctx.starts, fs.readdirSync(ctx.starts)[0]);
  const records = JSON.stringify(new URL('../scripts/project-records.mjs', import.meta.url).href);
  // Keep real record preparation and vault storage, with an isolated diary receipt.
  write('scripts/project-records.mjs', `
    export { prepare } from ${records};
    import { sync as deliver } from ${records};
    export const sync = options => deliver({
      ...options, vaultRoot: ${JSON.stringify(path.join(root, '.cache', 'vault'))},
      importer: async ({ workId, revision }) => ({ outcome: 'saved', workId, revision, entryId: workId }),
    });
  `);
  const draft = (summary, fields = {}) => {
    write('draft.json', JSON.stringify({ title: 'Development note', summary, ...fields }));
    return path.join(root, 'draft.json');
  };
  return { ctx, workId, commit, directory, draft };
}

test('finish delivers the selected experience kind and refuses generated portfolio input', async t => {
  const { ctx, workId, directory, commit, draft } = recoveryFixture(t);
  commit('src/design.mjs', 'export const storage = "local";', workId);
  await assert.rejects(finish(ctx, workId, draft('배경', { kind: 'portfolio' })), /경험 종류/);
  assert.equal(fs.existsSync(path.join(directory, 'delivery.json')), false);
  const result = await finish(ctx, workId, draft('원문을 먼저 저장한다.', { kind: 'design' }));
  assert.equal(result.phase, 'local_complete');
  assert.equal(JSON.parse(fs.readFileSync(path.join(directory, 'delivery.json'))).source.kind, 'design');
});

test('the diary shows the summary and detail while the runs and commits stay in the kept document', async t => {
  const { ctx, workId, directory, commit, draft } = recoveryFixture(t);
  const sha = commit('src/feature.mjs', 'export const shown = true;', workId);
  // Only finish writes the end line: a note that holds it would cut the diary short.
  await assert.rejects(finish(ctx, workId, draft('요약 문단', { detail: '예시\n\n' + DIARY_END + '\n\n뒤' })), /다이어리 경계 줄/);
  assert.equal(fs.existsSync(path.join(directory, 'delivery.json')), false);
  await finish(ctx, workId, draft('요약 문단', { detail: '설명 문단', verification: '검사 문단' }));
  const { source } = JSON.parse(fs.readFileSync(path.join(directory, 'delivery.json')));
  // The app shows the text above the end line; the record keeps the rest.
  const [shown, kept] = source.detailedMarkdown.split('\n\n' + DIARY_END + '\n\n');
  assert.equal(shown, '요약 문단\n\n설명 문단');
  assert.ok(kept.includes('검사 문단') && kept.includes('작업 커밋 · ' + sha));
  const app = fs.readFileSync(new URL('../crates/timeschedule-core/src/project_records.rs', import.meta.url), 'utf8');
  assert.ok(app.includes(`pub const DIARY_END: &str = "${DIARY_END}";`), 'the app looks for the same line');
});

test('a branch reuses its ID, including from a linked worktree', t => {
  const { root, ctx } = fixture(t);
  const first = begin(ctx);
  assert.deepEqual(begin(ctx), first);
  const other = path.join(root, '.cache', 'linked');
  git(root, ['worktree', 'add', '-b', 'codex/other', other]);
  const linked = context(other);
  assert.equal(linked.stateDir, ctx.stateDir);
  assert.notEqual(begin(linked).workId, first.workId);
  assert.equal(audit(linked).length, 2);
});

test('a start interrupted before its ID leaves a visible receipt and an unassigned commit', t => {
  const { root, ctx, commit } = fixture(t);
  const first = begin(ctx);
  const directory = path.join(ctx.starts, fs.readdirSync(ctx.starts)[0]);
  fs.unlinkSync(path.join(directory, 'work.json')); // Recreate the gap between the two writes.
  const orphan = commit('src/mine.mjs', 'export const value=1;');
  assert.deepEqual(audit(ctx), [{ branch: ctx.branch, workId: null, phase: 'started_before_id', ownedHead: null, unassigned: [orphan] }]);
  const resumed = begin(context(root));
  assert.equal(resumed.base, first.base);
  assert.deepEqual(audit(ctx)[0].unassigned, [orphan]);
});

test('recovering two owned commits excludes another author using the same ID and an untagged own commit', t => {
  const { ctx, root, commit } = fixture(t);
  const { workId, base } = begin(ctx);
  commit('src/one.mjs', 'export const one=1;', workId);
  const theirs = commit('src/theirs.mjs', 'export const two=2;', workId, 'Other <other@example.invalid>');
  const unknown = commit('src/unassigned.mjs', 'export const three=3;');
  const last = commit('src/four.mjs', 'export const four=4;', workId);
  const start = { base, owner: ctx.owner };
  const scan = inspectBranch(root, start, workId, last);
  assert.deepEqual(scan.ambiguous, [theirs]);
  assert.deepEqual(scan.unassigned, [unknown]);
  const evidence = gitEvidence(root, root, start, workId, scan);
  assert.deepEqual(evidence.paths, ['src/four.mjs', 'src/one.mjs']);
  assert.equal(git(root, ['cat-file', '-p', evidence.head]).includes('parent ' + base), true);
  assert.equal(gitEvidence(root, root, start, workId, scan).head, evidence.head);
  assert.equal(git(root, ['rev-parse', 'HEAD']).trim(), last);
  assert.equal(audit(ctx)[0].phase, 'missing');
});

test('working failure and commit replay keep skips, TODOs, exit code and evidence scope', t => {
  const { ctx, root, commit, write } = fixture(t);
  const { workId } = begin(ctx);
  const file = 'tests/example.test.mjs';
  const make = value => "import test from 'node:test'; import assert from 'node:assert/strict';\n" +
    "test('actual case', () => assert.equal(" + value + ", true));\ntest.skip('later', () => {});\ntest.todo('future');\n";
  write(file, make(false));
  const red = check(ctx, workId, [file]);
  assert.equal(red.exitCode, 1);
  assert.deepEqual(red.failures, ['actual case']);
  const sha = commit(file, make(true), workId);
  write(file, make(false));
  const green = replayTests(root, sha, [file]);
  assert.equal(green.exitCode, 0);
  assert.deepEqual(green.counts, { tests: 3, pass: 1, fail: 0, cancelled: 0, skipped: 1, todo: 1 });
});

test('documentation alone produces no delivery payload', async t => {
  const { ctx, root, commit, write } = fixture(t);
  const { workId } = begin(ctx);
  commit('docs/notes.md', 'notes\n', workId);
  write('draft.json', JSON.stringify({ title: 'notes', summary: 'notes' }));
  assert.equal((await finish(ctx, workId, path.join(root, 'draft.json'))).reason, 'no_development_changes');
  assert.equal(fs.existsSync(path.join(ctx.stateDir, workId, 'state.json')), false);
});

test('a follow-up owned development commit needs a fresh note before it can finish', async t => {
  const { ctx, workId, commit, directory, draft } = recoveryFixture(t);
  commit('src/value.mjs', 'export const value = 1;', workId);
  const original = await finish(ctx, workId, draft('Implemented value one.'));
  assert.equal(original.phase, 'local_complete', original.lastError);
  const delivery = path.join(directory, 'delivery.json');
  const previousDelivery = fs.readFileSync(delivery, 'utf8');
  const followUp = commit('src/value.mjs', 'export const value = 2;', workId);

  await assert.rejects(finish(ctx, workId), /원문 JSON/);
  assert.deepEqual(audit(ctx), [{ branch: ctx.branch, workId, phase: 'missing', ownedHead: followUp }]);
  assert.equal(fs.readFileSync(delivery, 'utf8'), previousDelivery);
  assert.match(fs.readFileSync(original.file, 'utf8'), /Implemented value one\./);

  const updated = await finish(ctx, workId, draft('Implemented value two.'));
  assert.equal(updated.phase, 'local_complete', updated.lastError);
  assert.equal(updated.revision, original.revision + 1);
  const { source } = JSON.parse(fs.readFileSync(delivery, 'utf8'));
  assert.equal(source.gitChanges.head, followUp);
  assert.equal(source.diarySummary, 'Implemented value two.');
  assert.deepEqual(audit(ctx), []);
});

test('renaming an input or editing only metadata cannot explain a follow-up commit', async t => {
  const { ctx, workId, commit, directory, draft } = recoveryFixture(t);
  commit('src/value.mjs', 'export const value = 1;', workId);
  const summary = 'Implemented value one.';
  const detail = 'The first version\nreturns one.';
  const input = draft(summary, { detail });
  const original = await finish(ctx, workId, input);
  assert.equal(original.phase, 'local_complete', original.lastError);
  const copy = path.join(path.dirname(input), 'renamed.json');
  fs.copyFileSync(input, copy);
  const replay = await finish(ctx, workId, copy);
  assert.equal(replay.unchanged, true);
  assert.equal(replay.revision, original.revision);

  const followUp = commit('src/value.mjs', 'export const value = 2;', workId);
  const saved = name => fs.readFileSync(path.join(directory, name), 'utf8');
  const noteBefore = saved('note.json'), deliveryBefore = saved('delivery.json');
  await assert.rejects(finish(ctx, workId, copy), /요약|상세/);
  const metadataOnly = draft('  ' + summary, {
    title: 'Follow-up', detail: detail.replace('\n', '\r\n  '),
    verification: 'Ran another check.', status: 'verified', tests: ['tests/missing.test.mjs'],
  });
  await assert.rejects(finish(ctx, workId, metadataOnly), /요약|상세/);
  await assert.rejects(finish(ctx, workId, draft(summary)), /요약|상세/);
  await assert.rejects(finish(ctx, workId, draft(summary, { detail: detail + '\n\n' + summary })), /요약|상세/);
  assert.equal(saved('note.json'), noteBefore);
  assert.equal(saved('delivery.json'), deliveryBefore);
  assert.deepEqual(audit(ctx), [{ branch: ctx.branch, workId, ownedHead: followUp, phase: 'missing' }]);

  const expanded = detail + '\n\nThe follow-up returns two to meet the revised requirement.';
  const updated = await finish(ctx, workId, draft(summary, { detail: expanded }));
  assert.equal(updated.phase, 'local_complete', updated.lastError);
  assert.equal(updated.revision, original.revision + 1);
  assert.equal(updated.entryId, original.entryId);
  assert.ok(fs.readFileSync(updated.file, 'utf8').includes(expanded));
  await assert.rejects(finish(ctx, workId, copy), /요약|상세/);
  assert.deepEqual(audit(ctx), []);
});

test('an older completed revision cannot be submitted as evidence for a third commit', async t => {
  const { ctx, workId, commit, draft } = recoveryFixture(t);
  const original = 'Started with value one.';
  commit('src/value.mjs', 'export const value = 1;', workId);
  await finish(ctx, workId, draft(original));
  commit('src/value.mjs', 'export const value = 2;', workId);
  const second = 'Changed the value from one to two.';
  await finish(ctx, workId, draft(second));
  commit('src/value.mjs', 'export const value = 3;', workId);

  await assert.rejects(finish(ctx, workId, draft(original)), /요약|상세/);
  await assert.rejects(finish(ctx, workId, draft(original, { detail: second })), /요약|상세/);
  assert.equal(audit(ctx)[0].phase, 'missing');
});

test('a legacy note without a commit requires explicit replacement', async t => {
  const { ctx, workId, commit, directory, draft } = recoveryFixture(t);
  const ownedHead = commit('src/value.mjs', 'export const value = 1;', workId);
  const noteFile = path.join(directory, 'note.json');
  const legacy = JSON.stringify({ title: 'Legacy note', summary: 'Unknown commit.' });
  fs.writeFileSync(noteFile, legacy);

  assert.deepEqual(audit(ctx), [{ branch: ctx.branch, workId, phase: 'missing', ownedHead }]);
  await assert.rejects(finish(ctx, workId), /원문 JSON/);
  assert.equal(fs.readFileSync(noteFile, 'utf8'), legacy);
  assert.equal(fs.existsSync(path.join(directory, 'delivery.json')), false);
  const result = await finish(ctx, workId, draft('Checked value one.'));
  assert.equal(result.phase, 'local_complete', result.lastError);
  assert.deepEqual(audit(ctx), []);
});

test('an interrupted note follows its owned commit across other authors and documentation', async t => {
  const { ctx, workId, commit, directory, draft } = recoveryFixture(t);
  const other = 'Other <other@example.invalid>';
  commit('src/theirs.mjs', 'export const theirs = 1;', undefined, other);
  const ownedHead = commit('src/mine.mjs', 'export const mine = 1;', workId);
  // Interrupt the first finish after the note is saved, before a delivery is durable.
  // An unreadable delivery path stops finish without taking it as absent.
  const delivery = path.join(directory, 'delivery.json');
  fs.mkdirSync(delivery);
  await assert.rejects(finish(ctx, workId, draft('Implemented my change.')), /이전 전달 기록/);
  fs.rmdirSync(delivery);
  commit('docs/notes.md', 'Documentation only.', workId);
  const theirs = commit('src/later.mjs', 'export const later = 1;', workId, other);

  assert.deepEqual(audit(ctx), [{ branch: ctx.branch, workId, ownedHead, ambiguous: [theirs], phase: 'missing_with_note' }]);
  const recovered = await finish(ctx, workId);
  assert.equal(recovered.phase, 'local_complete', recovered.lastError);
  const { source } = JSON.parse(fs.readFileSync(delivery, 'utf8'));
  assert.equal(source.diarySummary, 'Implemented my change.');
  assert.deepEqual(source.gitChanges.paths, ['src/mine.mjs']);
});

test('an unreadable or damaged earlier delivery is kept and never becomes a first completion', async t => {
  const { ctx, workId, commit, directory, draft } = recoveryFixture(t);
  const done = { status: 'completed', verification: 'User confirmed value one.' };
  commit('src/value.mjs', 'export const value = 1;', workId);
  const original = await finish(ctx, workId, draft('Implemented value one.', done));
  assert.equal(original.phase, 'local_complete', original.lastError);
  const delivery = path.join(directory, 'delivery.json');
  const saved = fs.readFileSync(delivery, 'utf8');
  const first = JSON.parse(saved).firstCompletedAt;
  assert.equal(typeof first, 'string');
  commit('src/value.mjs', 'export const value = 2;', workId);

  // Damaged content: finish stops and the file stays byte for byte.
  const damaged = saved.slice(0, Math.floor(saved.length / 2));
  fs.writeFileSync(delivery, damaged);
  await assert.rejects(finish(ctx, workId, draft('Changed the value to two.', done)), /이전 전달 기록이 손상/);
  assert.equal(fs.readFileSync(delivery, 'utf8'), damaged);

  // A read error other than a missing file (here a directory in its place).
  fs.rmSync(delivery);
  fs.mkdirSync(delivery);
  await assert.rejects(finish(ctx, workId), error => /이전 전달 기록을 읽을 수 없어/.test(error.message)
    && error.cause?.code && error.cause.code !== 'ENOENT');
  assert.equal(fs.statSync(delivery).isDirectory(), true);
  fs.rmdirSync(delivery);

  // Once readable again, the first completion moment is the original one.
  fs.writeFileSync(delivery, saved);
  const resumed = await finish(ctx, workId);
  assert.equal(resumed.phase, 'local_complete', resumed.lastError);
  const after = JSON.parse(fs.readFileSync(delivery, 'utf8'));
  assert.equal(after.firstCompletedAt, first);
  assert.equal(after.source.completedAt, first);
  assert.equal(after.source.diarySummary, 'Changed the value to two.');
});
