import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { audit, begin, check, context, finish } from '../scripts/development-diary.mjs';
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
