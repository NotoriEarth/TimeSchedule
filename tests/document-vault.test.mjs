import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { randomUUID, createHash } from 'node:crypto';
import { createServer } from 'node:net';
import { execFileSync, spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { initVault, saveDocument, adoptDocument, vaultStatus, configureVault, vaultPreferences } from '../scripts/document-vault.mjs';

function fixture(t) {
  const temp = fs.mkdtempSync(path.join(os.tmpdir(), 'timeschedule-vault-'));
  t.after(() => fs.rmSync(temp, { recursive: true, force: true }));
  return { root: path.join(temp, 'vault'), stateDir: path.join(temp, 'state') };
}
const source = () => ({ id: randomUUID(), collection: 'studies', group: 'rust', title: 'Rust 공부', date: '2026-11-16', markdown: '# 소유권\n\n공부한 원문입니다.\n', tags: ['rust'] });

function runCli(stateDir, ...args) {
  const script = fileURLToPath(new URL('../scripts/document-vault.mjs', import.meta.url));
  const result = spawnSync(process.execPath, [script, ...args, '--state-dir', stateDir], { encoding: 'utf8', windowsHide: true });
  assert.ifError(result.error);
  return result;
}

test('vault CLI rejects misplaced options before reading settings or opening the vault', t => {
  const { root, stateDir } = fixture(t);
  fs.mkdirSync(stateDir);
  fs.writeFileSync(path.join(stateDir, 'vault-settings.json'), 'invalid JSON');
  const cases = [
    ['status', '--id', 'abc'],
    ['init', '--root', root, '--id', 'abc'],
    ['import', '--input', 'missing.json', '--id', 'abc'],
    ['adopt', '--id', randomUUID(), '--root', root],
    ['status', '--unknown', 'abc'],
  ];
  for (const args of cases) {
    const result = runCli(stateDir, ...args);
    const flag = args.at(-2);
    assert.equal(result.status, 1, args.join(' '));
    assert.ok(result.stderr.includes(args[0]) && result.stderr.includes(flag), result.stderr);
    assert.equal(result.stdout, '');
  }
  assert.equal(fs.existsSync(root), false);
});

test('vault CLI accepts each command with its own options and a shared state directory', t => {
  const { root, stateDir } = fixture(t);
  const run = (...args) => {
    const result = runCli(stateDir, ...args);
    assert.equal(result.status, 0, result.stderr);
    return JSON.parse(result.stdout);
  };
  assert.equal(run('init', '--root', root).root, fs.realpathSync(root));
  const document = source();
  const input = path.join(stateDir, 'input.json');
  fs.writeFileSync(input, JSON.stringify(document));
  const saved = run('import', '--input', input);
  assert.equal(saved.id, document.id);
  fs.writeFileSync(saved.file, '직접 편집한 문서');
  assert.equal(run('adopt', '--id', document.id).revision, 2);
  assert.equal(run('status').records[0].id, document.id);
});

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

test('selected path persists, and a settings file from before Space was removed is read', async t => {
  const f = fixture(t);
  await configureVault(f.stateDir, { root: f.root });
  const file = path.join(f.stateDir, 'vault-settings.json');
  const root = vaultPreferences(f.stateDir).root;
  assert.deepEqual(vaultPreferences(f.stateDir), { schemaVersion: 1, root });
  // A checkout from before the removal refuses a file without spaceEnabled.
  assert.equal(JSON.parse(fs.readFileSync(file, 'utf8')).spaceEnabled, false);
  fs.writeFileSync(file, JSON.stringify({ schemaVersion: 1, root, spaceEnabled: true }));
  assert.deepEqual(vaultPreferences(f.stateDir), { schemaVersion: 1, root });
  fs.writeFileSync(file, JSON.stringify({ schemaVersion: 1, root }));
  assert.deepEqual(vaultPreferences(f.stateDir), { schemaVersion: 1, root });
  // The command that turned Space on is gone.
  assert.equal(runCli(f.stateDir, 'config', '--space', 'on').status, 1);
  assert.deepEqual(JSON.parse(fs.readFileSync(file, 'utf8')), { schemaVersion: 1, root });
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

// A vault process from before the canonical-path change named its lock after the
// JS-resolved spelling it was given. Given either spelling, the current version
// must be locked out by such a holder of the same vault.
test('vault locks from the earlier naming still exclude writers for both spellings', { skip: process.platform !== 'win32' }, async t => {
  const f = fixture(t);
  await initVault(f.root);
  const script = `(New-Object -ComObject Scripting.FileSystemObject).GetFolder('${f.root.replaceAll("'", "''")}').ShortPath`;
  const short = execFileSync('powershell.exe', ['-NoProfile', '-NonInteractive', '-Command', script], { encoding: 'utf8', windowsHide: true }).trim();
  if (short.toLowerCase() === fs.realpathSync.native(f.root).toLowerCase()) return t.skip('8.3 short names are disabled on this volume');
  const open = new Set();
  t.after(() => Promise.all([...open].map(holder => new Promise(resolve => holder.close(resolve)))));
  const hold = async spelling => {
    const legacy = createHash('sha256').update(fs.realpathSync(spelling).toLowerCase()).digest('hex');
    const holder = createServer(socket => socket.destroy());
    await new Promise((resolve, reject) => { holder.once('error', reject); holder.listen({ path: `\\\\.\\pipe\\timeschedule-vault-${legacy}`, exclusive: true }, resolve); });
    open.add(holder);
    return () => new Promise(resolve => { open.delete(holder); holder.close(resolve); });
  };
  let release = await hold(short);
  await assert.rejects(saveDocument(source(), { root: short }), /다른 문서 작업이 실행 중/);
  await release();
  release = await hold(fs.realpathSync.native(f.root));
  await assert.rejects(saveDocument(source(), { root: short }), /다른 문서 작업이 실행 중/);
  await release();
  assert.equal((await saveDocument(source(), { root: short })).revision, 1);
  assert.equal((await saveDocument(source(), f)).revision, 1);
});
