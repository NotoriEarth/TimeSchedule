import fs from 'node:fs';
import path from 'node:path';
import { createHash, randomUUID } from 'node:crypto';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { git, gitEvidence, inspectBranch } from './development-diary-git.mjs';
import { describeRun, replayTests, runNodeTests } from './development-diary-tests.mjs';

const read = file => JSON.parse(fs.readFileSync(file, 'utf8').replace(/^\uFEFF/, ''));
const optional = file => fs.existsSync(file) ? read(file) : null;
const digest = text => createHash('sha256').update(text).digest('hex');
const complete = phase => ['local_complete', 'complete'].includes(phase);

function write(file, value, once = false) {
  fs.mkdirSync(path.dirname(file), { recursive: true });
  const temp = file + '.' + randomUUID() + '.tmp';
  try {
    fs.writeFileSync(temp, JSON.stringify(value, null, 2) + '\n', { flag: 'wx', flush: true });
    if (!once) fs.renameSync(temp, file);
    else {
      try { fs.linkSync(temp, file); }
      catch (error) { if (error.code !== 'EEXIST') throw error; }
    }
  } finally { fs.rmSync(temp, { force: true }); }
  return read(file);
}

export function context(repo) {
  const root = git(repo, ['rev-parse', '--show-toplevel']).trim();
  const primary = git(root, ['worktree', 'list', '--porcelain', '-z']).split('\0')[0].slice('worktree '.length);
  const stateDir = path.join(primary, '.cache', 'project-records');
  const author = git(root, ['var', 'GIT_AUTHOR_IDENT']).match(/<([^<>]+)> \d+ [+-]\d{4}\s*$/);
  if (!author) throw new Error('작업에 사용할 Git 작성자 이메일을 확인해 주세요.');
  const branch = git(root, ['branch', '--show-current']).trim();
  return { root, primary, stateDir, branch, owner: author[1].toLowerCase(), starts: path.join(stateDir, 'diary-starts') };
}

export function begin(ctx) {
  if (!ctx.branch) throw new Error('작업 브랜치에서 begin을 실행하세요.');
  const directory = path.join(ctx.starts, digest(JSON.stringify([ctx.branch, ctx.owner])));
  const start = write(path.join(directory, 'start.json'), {
    branch: ctx.branch, owner: ctx.owner, base: git(ctx.root, ['rev-parse', 'HEAD']).trim(), at: new Date().toISOString(),
  }, true);
  // If interrupted between these writes, audit can identify the branch and base.
  const { workId } = write(path.join(directory, 'work.json'), { workId: randomUUID() }, true);
  return { workId, base: start.base, branch: start.branch };
}

function receipts(ctx) {
  if (!fs.existsSync(ctx.starts)) return [];
  return fs.readdirSync(ctx.starts, { withFileTypes: true }).filter(entry => entry.isDirectory()).flatMap(entry => {
    const directory = path.join(ctx.starts, entry.name), start = optional(path.join(directory, 'start.json'));
    if (!start || start.owner !== ctx.owner) return [];
    return [{ start, directory, workId: optional(path.join(directory, 'work.json'))?.workId ?? null }];
  });
}

function taskById(ctx, id) {
  const task = id && receipts(ctx).find(row => row.workId === id);
  if (!task) throw new Error('이 작성자의 작업 시작 영수증과 ID를 확인해 주세요.');
  return task;
}

function branchHead(ctx, task) {
  const ref = 'refs/heads/' + task.start.branch;
  return git(ctx.root, ['for-each-ref', '--format=%(refname) %(objectname)', ref]).trim().split(/\r?\n/).find(line => line.startsWith(ref + ' '))?.split(' ')[1];
}

function stateFor(ctx, id) {
  return id ? optional(path.join(ctx.stateDir, id, 'state.json')) : null;
}

function readNoteForCommit(directory, ownedHead) {
  const saved = optional(path.join(directory, 'note.json'));
  return saved?.ownedHead === ownedHead ? saved.note : null;
}

export function audit(ctx) {
  const result = [];
  for (const task of receipts(ctx)) {
    const { start, workId, directory } = task;
    const head = branchHead(ctx, task), job = stateFor(ctx, workId);
    const saved = optional(path.join(directory, 'delivery.json'));
    const delivered = job && saved && job.source.gitChanges?.head === saved.source.gitChanges.head
      && job.source.title === saved.source.title && job.source.detailedMarkdown === saved.source.detailedMarkdown
      && job.source.status === saved.source.status;
    const row = { branch: start.branch, workId };
    if (!head) {
      if (!complete(job?.phase)) result.push({ ...row, phase: 'branch_missing' });
      continue;
    }
    if (delivered && complete(job.phase) && saved.branchHead === head && !saved.unassigned?.length && !saved.ambiguous?.length) continue;
    const scan = inspectBranch(ctx.root, start, workId, head);
    if (scan.unassigned.length) row.unassigned = scan.unassigned;
    if (scan.ambiguous.length) row.ambiguous = scan.ambiguous;
    const ownHead = scan.owned.at(-1)?.sha;
    let phase;
    if (!workId) phase = 'started_before_id';
    else if (ownHead && saved?.ownedHead === ownHead && !delivered) phase = 'delivery_ready';
    else if (ownHead && saved?.ownedHead === ownHead && job) phase = job.phase;
    else if (ownHead) phase = readNoteForCommit(directory, ownHead) ? 'missing_with_note' : 'missing';
    else phase = 'started';
    if (!complete(phase) || row.unassigned || row.ambiguous) result.push({ ...row, phase, ownedHead: ownHead ?? null });
  }
  return result;
}

export function check(ctx, id, files) {
  const task = taskById(ctx, id);
  if (ctx.branch !== task.start.branch) throw new Error('이 작업의 브랜치에서 작업 트리 테스트를 실행하세요.');
  const head = git(ctx.root, ['rev-parse', 'HEAD']).trim();
  const receipt = runNodeTests(ctx.root, files);
  const name = new Date().toISOString().replace(/:/g, '-') + '-' + randomUUID() + '.json';
  write(path.join(task.directory, 'checks', name), { head, ...receipt });
  return receipt;
}

function requiredText(value, label) {
  if (typeof value !== 'string' || !value.trim()) throw new Error(label + ' 값을 작성하세요.');
  return value.trim().replace(/\r\n/g, '\n');
}

export async function finish(ctx, id, file) {
  const task = taskById(ctx, id), { start, directory } = task;
  const head = branchHead(ctx, task);
  if (!head) throw new Error('작업 브랜치의 커밋을 확인할 수 없습니다.');
  const scan = inspectBranch(ctx.root, start, id, head);
  const ownedHead = scan.owned.at(-1)?.sha;
  if (!ownedHead) return { workId: id, skipped: true, reason: scan.assigned ? 'no_development_changes' : 'no_owned_commits' };
  const changes = gitEvidence(ctx.root, directory, start, id, scan);
  if (!changes.paths.length) return { workId: id, skipped: true, reason: 'no_development_changes' };
  const note = file ? read(file) : readNoteForCommit(directory, ownedHead);
  if (!note) throw new Error(`마지막 개발 커밋 ${ownedHead.slice(0, 8)}의 새 설명이 필요합니다. finish ${id} <NOTE.json>으로 작업 원문 JSON 경로를 지정하세요.`);
  const title = requiredText(note.title, 'title'), paragraph = requiredText(note.summary, 'summary');
  const files = note.tests ?? [];
  if (!Array.isArray(files)) throw new Error('tests에는 실행할 파일 목록을 지정하세요.');
  if (file) write(path.join(directory, 'note.json'), { ownedHead, note });
  const blocks = [paragraph];
  if (note.detail) blocks.push(requiredText(note.detail, 'detail'));
  const checks = path.join(directory, 'checks');
  if (fs.existsSync(checks)) for (const name of fs.readdirSync(checks).filter(n => n.endsWith('.json')).sort()) {
    const receipt = read(path.join(checks, name));
    blocks.push(describeRun('작업 트리에서 실행 · 당시 HEAD', receipt.head, receipt));
  }
  const final = files.length ? replayTests(ctx.root, changes.head, files) : null;
  if (final) blocks.push(describeRun('커밋에서 재실행', changes.head, final));
  if (note.verification) blocks.push(requiredText(note.verification, 'verification'));
  if (!final && !note.verification) blocks.push('이 작업에는 실행 결과가 첨부되지 않았습니다.');
  blocks.push('작업 커밋 · ' + scan.owned.map(commit => commit.sha).join(', '));
  const status = final ? (final.exitCode === 0 && final.counts.pass > 0 ? 'verified' : 'verification_pending')
    : (note.verification && note.status) || 'verification_pending';
  const date = new Intl.DateTimeFormat('sv-SE', { timeZone: 'Asia/Seoul', year: 'numeric', month: '2-digit', day: '2-digit' }).format(new Date(start.at));
  const source = { workId: id, projectTitle: 'TimeSchedule', repoPath: ctx.primary, date, kind: 'development',
    title, status, diarySummary: paragraph, detailedMarkdown: blocks.join('\n\n'), gitChanges: changes };
  // The complete, deterministic input is durable before the first external write.
  write(path.join(directory, 'delivery.json'), { source, ownedHead, branchHead: head, unassigned: scan.unassigned, ambiguous: scan.ambiguous });
  return retry(ctx, id);
}

export async function retry(ctx, id) {
  const { directory } = taskById(ctx, id), file = path.join(directory, 'delivery.json'), saved = read(file);
  // A linked worktree must register the primary checkout's permanent sync script
  // with the desktop app. It also shares its configured vault and project identity.
  const dispatcher = await import(pathToFileURL(path.join(ctx.primary, 'scripts', 'project-records.mjs')).href);
  const options = { stateDir: ctx.stateDir, projectRoot: ctx.primary, workId: id, retryNow: true };
  const prepared = await dispatcher.prepare(saved.source, options);
  if (prepared.skipped) return prepared;
  await dispatcher.sync(options);
  const job = stateFor(ctx, id);
  return { workId: id, revision: prepared.revision, unchanged: prepared.unchanged, phase: job?.phase, vaultSaved: job?.vaultSaved ?? false,
    entryId: job?.diaryReceipt?.entryId, file: job?.vaultReceipt?.file, lastError: job?.lastError };
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    const [action, id, ...args] = process.argv.slice(2), ctx = context(process.cwd());
    let result;
    if (action === 'begin' && !id) result = begin(ctx);
    else if (action === 'audit' && !id) result = audit(ctx);
    else if (action === 'check') result = check(ctx, id, args);
    else if (action === 'finish' && args.length <= 1) result = await finish(ctx, id, args[0]);
    else if (action === 'retry' && !args.length) result = await retry(ctx, id);
    else throw new Error('사용법: development-diary.mjs begin | audit | check ID TEST... | finish ID [NOTE.json] | retry ID');
    console.log(JSON.stringify(result, null, 2));
    if (action === 'check') process.exitCode = result.exitCode;
    if (['finish', 'retry'].includes(action) && !result.skipped && !complete(result.phase)) process.exitCode = 1;
  } catch (error) { console.error(error.message); process.exitCode = 1; }
}
