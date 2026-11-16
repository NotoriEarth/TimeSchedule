import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { randomUUID } from 'node:crypto';
import { initVault, saveDocument, adoptDocument, vaultStatus, configureVault, vaultPreferences } from '../scripts/document-vault.mjs';

function fixture(t) {
  const temp = fs.mkdtempSync(path.join(os.tmpdir(), 'timeschedule-vault-'));
  t.after(() => fs.rmSync(temp, { recursive: true, force: true }));
  return { root: path.join(temp, 'vault'), stateDir: path.join(temp, 'state') };
}
const source = () => ({ id: randomUUID(), collection: 'studies', group: 'rust', title: 'Rust 공부', date: '2026-11-16', markdown: '# 소유권\n\n공부한 원문입니다.\n', tags: ['rust'] });

test('independent vault initializes idempotently and saves project and study Markdown without Git', async t => {
  const f = fixture(t); await initVault(f.root); await initVault(f.root);
  const study = source();
  const a = await saveDocument(study, f);
  assert.equal(fs.readFileSync(a.file, 'utf8'), study.markdown);
  assert.equal(a.revision, 1);
  assert.ok(fs.existsSync(path.join(path.dirname(a.file), 'attachments')));
  await saveDocument({ ...source(), collection: 'projects', group: 'other-project' }, f);
  assert.equal((await vaultStatus(f.root)).records.length, 2);
  assert.equal((await saveDocument(study, f)).unchanged, true);
  assert.equal((await saveDocument({ ...study, markdown: study.markdown + '\n후속 공부.' }, f)).revision, 2);
  const old = JSON.parse(fs.readFileSync(path.join(path.dirname(a.file), 'history', '1.json')));
  assert.equal(old.document.markdown, study.markdown);
});

test('manual edits and deleted current files are preserved as conflicts even on identical re-import', async t => {
  const f = fixture(t), input = source(); const a = await saveDocument(input, f);
  fs.writeFileSync(a.file, '사용자 직접 편집');
  await assert.rejects(saveDocument(input, f), /편집|충돌/);
  assert.equal(fs.readFileSync(a.file, 'utf8'), '사용자 직접 편집');
  fs.unlinkSync(a.file);
  await assert.rejects(saveDocument(input, f), /삭제|충돌/);
  assert.equal(fs.existsSync(a.file), false);
});

test('deleting the whole document directory is preserved, not silently recreated', async t => {
  const f = fixture(t), input = source(); const a = await saveDocument(input, f);
  const directory = path.dirname(a.file);
  assert.ok(directory.startsWith(f.root + path.sep));
  fs.rmSync(directory, { recursive: true });
  await assert.rejects(saveDocument(input, f), /삭제|충돌/);
  assert.equal(fs.existsSync(directory), false);
});

test('invalid IDs, paths, dates and collection changes cannot escape or overwrite another record', async t => {
  const f = fixture(t), input = source(); await saveDocument(input, f);
  for (const patch of [{ id: '../escape' }, { group: '../escape' }, { group: 'CON' }, { date: '2026-02-30' }, { collection: 'private/../../' }]) {
    await assert.rejects(saveDocument({ ...input, ...patch }, f));
  }
  await assert.rejects(saveDocument({ ...input, collection: 'projects' }, f), /분류|위치/);
});

test('vault rejects linked managed directories', async t => {
  const f = fixture(t); await initVault(f.root);
  const outside = path.join(path.dirname(f.root), 'outside'); fs.mkdirSync(outside);
  fs.symlinkSync(outside, path.join(f.root, 'studies', 'linked'), process.platform === 'win32' ? 'junction' : 'dir');
  await assert.rejects(saveDocument({ ...source(), group: 'linked' }, f), /링크|경로/);
  assert.deepEqual(fs.readdirSync(outside), []);
});

test('selected path persists, Space defaults off and enabling is explicit for this capture state', async t => {
  const f = fixture(t);
  assert.equal(vaultPreferences(f.stateDir).spaceEnabled, false);
  await configureVault(f.stateDir, { root: f.root });
  assert.equal(vaultPreferences(f.stateDir).root, f.root);
  assert.equal(vaultPreferences(f.stateDir).spaceEnabled, false);
  await configureVault(f.stateDir, { spaceEnabled: true });
  assert.equal(vaultPreferences(f.stateDir).spaceEnabled, true);
  await configureVault(f.stateDir, { spaceEnabled: false });
  assert.equal(vaultPreferences(f.stateDir).root, f.root);
});

test('an interrupted save recovers its committed revision without losing the previous body', async t => {
  const f = fixture(t), input = source(); const first = await saveDocument(input, f);
  const next = { ...input, markdown: input.markdown + '\n새 내용' };
  const rename = fs.renameSync;
  fs.renameSync = function(from, to) {
    if (String(to).endsWith('metadata.json')) throw Error('simulated interruption');
    return rename.call(this, from, to);
  };
  try { await assert.rejects(saveDocument(next, f), /simulated/); }
  finally { fs.renameSync = rename; }
  assert.equal((await vaultStatus(f.root)).records[0].state, 'recovery_pending');
  const recovered = await saveDocument(next, f);
  assert.equal(recovered.revision, 2); assert.equal(recovered.unchanged, true);
  assert.equal(fs.readFileSync(first.file, 'utf8'), next.markdown);
  assert.equal(JSON.parse(fs.readFileSync(path.join(path.dirname(first.file), 'history', '1.json'))).document.markdown, input.markdown);
});

test('explicit adopt records a manual edit as a new revision and keeps later automated imports from overwriting it', async t => {
  const f = fixture(t), input = source(); const first = await saveDocument(input, f);
  fs.writeFileSync(first.file, '직접 고친 문서');
  const adopted = await adoptDocument(input.id, f);
  assert.equal(adopted.revision, 2);
  assert.equal(JSON.parse(fs.readFileSync(path.join(path.dirname(first.file), 'history', '2.json'))).document.markdown, '직접 고친 문서');
  assert.equal((await vaultStatus(f.root)).records[0].state, 'saved_local');
  await assert.rejects(saveDocument(input, f), /수동|충돌/);
});

test('older automatic capture retries never roll the vault back to an older body', async t => {
  const f = fixture(t), input = source();
  const old = { ...input, source: { repoPath: '/same/repository', captureRevision: 1 } };
  const newer = { ...input, markdown: '최신 원문', source: { repoPath: '/same/repository', captureRevision: 2 } };
  await saveDocument(old, f); const latest = await saveDocument(newer, f);
  const retry = await saveDocument(old, f);
  assert.equal(retry.stale, true); assert.equal(retry.revision, latest.revision);
  assert.equal(fs.readFileSync(latest.file, 'utf8'), newer.markdown);
});

test('manual changes after an interrupted save remain untouched during recovery', async t => {
  const f = fixture(t), input = source(); const first = await saveDocument(input, f);
  const next = { ...input, markdown: '두 번째 원문' }; const rename = fs.renameSync;
  fs.renameSync = function(from, to) {
    if (String(to).endsWith('metadata.json')) throw Error('simulated interruption');
    return rename.call(this, from, to);
  };
  try { await assert.rejects(saveDocument(next, f), /simulated/); }
  finally { fs.renameSync = rename; }
  fs.writeFileSync(first.file, '중단 후 직접 편집');
  await assert.rejects(saveDocument(next, f), /충돌/);
  assert.equal(fs.readFileSync(first.file, 'utf8'), '중단 후 직접 편집');
});
