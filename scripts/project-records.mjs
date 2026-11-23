import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { createHash, randomUUID } from 'node:crypto';
import { createServer } from 'node:net';
import { spawn } from 'node:child_process';
import { developmentEvidence, canonicalPath, ownedCommits } from './project-records-git.mjs';
import { saveDocument, vaultPreferences } from './document-vault.mjs';

export const PROJECT_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const DEFAULT_STATE = path.join(PROJECT_ROOT, '.cache', 'project-records');
const UUID = /^[a-f0-9]{8}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{12}$/;
const LABELS = { in_progress: '진행 중', verification_pending: '검증 대기', verified: '확인한 범위 검증 완료', completed: '작업 정리 완료', merged: '병합 완료' };
const normal = value => value.replace(/\r\n/g, '\n').trim();
const sha = value => createHash('sha256').update(value).digest('hex');
// Statuses that can carry a confirmed completion time.
export const COMPLETING = ['verified', 'completed', 'merged'];
// RFC 3339 with seconds and an explicit offset, as the app parses it.
export const isInstant = value => typeof value === 'string' && value.length <= 40
  && /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d{1,9})?(?:Z|[+-]\d{2}:\d{2})$/.test(value) && !Number.isNaN(Date.parse(value));
export const OWNED_COMMITS_LINE = '작업 커밋 · ';
// The owned commits `development-diary.mjs finish` lists in a record's detail.
export function ownedCommitLine(markdown) {
  const hashes = [];
  for (const line of String(markdown).split('\n')) {
    const list = line.trim().startsWith(OWNED_COMMITS_LINE) ? line.trim().slice(OWNED_COMMITS_LINE.length) : null;
    if (list === null) continue;
    for (const hash of list.split(',').map(h => h.trim())) if (/^(?:[a-f0-9]{40}|[a-f0-9]{64})$/.test(hash) && !hashes.includes(hash)) hashes.push(hash);
  }
  return hashes.slice(0, 200);
}
// The begin receipt of a work: its start time and the owner it was begun by.
export function beginReceipt(stateDir, workId) {
  const starts = path.join(stateDir, 'diary-starts');
  if (!fs.existsSync(starts)) return null;
  for (const entry of fs.readdirSync(starts, { withFileTypes: true })) {
    if (!entry.isDirectory()) continue;
    const directory = path.join(starts, entry.name);
    let work, start;
    try {
      work = readJson(path.join(directory, 'work.json'));
      start = readJson(path.join(directory, 'start.json'));
    } catch { continue; }
    if (work?.workId !== workId) continue;
    return isInstant(start?.at) ? { at: start.at, owner: typeof start.owner === 'string' ? start.owner.toLowerCase() : null } : null;
  }
  return null;
}

function text(value, name, max) {
  if (typeof value !== 'string' || !value.trim() || value.includes('\0') || [...value].length > max) throw new Error(`${name} 값을 확인해 주세요.`);
  return normal(value);
}
function normalizeSource(input, projectRoot = PROJECT_ROOT) {
  if (!input || !UUID.test(input.workId)) throw new Error('고정된 UUID workId가 필요합니다. 같은 작업의 갱신에는 같은 ID를 사용하세요.');
  const repoPath = text(input.repoPath, 'repoPath', 2000);
  if (!path.isAbsolute(repoPath) || canonicalPath(repoPath).toLowerCase() !== canonicalPath(projectRoot).toLowerCase()) throw new Error('현재 설정된 TimeSchedule 저장소의 기록만 처리합니다.');
  const date = text(input.date, 'date', 10);
  if (!/^\d{4}-\d{2}-\d{2}$/.test(date) || Number(date.slice(0, 4)) < 1 || new Date(`${date}T00:00:00Z`).toISOString().slice(0, 10) !== date) throw new Error('실제 YYYY-MM-DD 날짜가 필요합니다.');
  if (!Object.hasOwn(LABELS, input.status)) throw new Error('지원하지 않는 작업 상태입니다.');
  if (!['design', 'development', 'troubleshooting', 'portfolio', 'summary'].includes(input.kind)) throw new Error('지원하지 않는 기록 종류입니다.');
  const detailedMarkdown = text(input.detailedMarkdown, '상세 기록', 48000);
  if (Buffer.byteLength(detailedMarkdown) > 48000) throw new Error('상세 기록은 UTF-8 48,000바이트 이하로 작업 단위를 나눠 주세요.');
  const paragraphs = detailedMarkdown.split(/\n{2,}/);
  const diarySummary = text(input.diarySummary ?? paragraphs.find(p => !/^\s*(#|```|\|)/.test(p)), '다이어리 요약', 10000);
  // Extract whole paragraphs, including their caveats. No free-form second
  // summary can invent a cause, change a number, or drop half a paragraph.
  if (!diarySummary.split(/\n{2,}/).every(p => paragraphs.includes(p))) throw new Error('다이어리 요약은 한계를 포함한 원문 문단을 그대로 골라 주세요.');
  // Completion is a confirmed moment of a completed status. Saving, editing,
  // retrying or waiting for verification never creates one.
  if (input.completedAt != null && (!isInstant(input.completedAt) || !COMPLETING.includes(input.status))) {
    throw new Error('completedAt은 확인된 완료 상태(verified·completed·merged)의 시간대 있는 RFC 3339 시각이어야 합니다.');
  }
  if (input.commits != null && (!Array.isArray(input.commits) || input.commits.length > 200)) throw new Error('commits는 최대 200개의 커밋 해시 목록이어야 합니다.');
  const source = { workId: input.workId, projectTitle: text(input.projectTitle, '프로젝트 제목', 200), repoPath: canonicalPath(repoPath), date,
    kind: input.kind, title: text(input.title, '기록 제목', 200), status: input.status, detailedMarkdown, diarySummary };
  return { source, completedAt: input.completedAt ?? null,
    commits: input.commits == null ? null : input.commits.map(c => typeof c === 'string' ? c : c?.hash) };
}
// Versions that published to Space kept the parent page's ID in the source,
// after diarySummary, and hashed it. Hashing it in that place still gives an
// unchanged record the hash those versions stored, so preparing it again is
// "unchanged" in either version and not a new capture.
function contentHashOf(source) {
  const entries = Object.entries(source);
  entries.splice(entries.findIndex(([key]) => key === 'diarySummary') + 1, 0, ['parentPageId', 'page_6ae1b18a44f08191ad2a86276972476c']);
  return sha(JSON.stringify(Object.fromEntries(entries)));
}
function atomicJson(file, value) {
  fs.mkdirSync(path.dirname(file), { recursive: true });
  const temporary = `${file}.${randomUUID()}.tmp`;
  let fd;
  try {
    fd = fs.openSync(temporary, 'wx'); fs.writeFileSync(fd, JSON.stringify(value, null, 2) + '\n'); fs.fsyncSync(fd); fs.closeSync(fd); fd = undefined;
    fs.renameSync(temporary, file);
  } finally {
    if (fd !== undefined) fs.closeSync(fd);
    if (fs.existsSync(temporary)) fs.unlinkSync(temporary);
  }
}
const readJson = file => JSON.parse(fs.readFileSync(file, 'utf8').replace(/^\uFEFF/, ''));
function loadJobs(stateDir, workId) {
  if (workId && !UUID.test(workId)) throw new Error('work-id 형식을 확인해 주세요.');
  if (!fs.existsSync(stateDir)) return [];
  return fs.readdirSync(stateDir, { withFileTypes: true }).filter(e => e.isDirectory() && UUID.test(e.name) && (!workId || e.name === workId))
    .flatMap(e => fs.readdirSync(path.join(stateDir, e.name)).filter(f => f === 'state.json' || /^pending-\d+\.json$/.test(f))
      .map(f => ({ file: path.join(stateDir, e.name, f), job: readJson(path.join(stateDir, e.name, f)) })))
    .sort((a,b) => a.job.source.workId === b.job.source.workId ? a.job.revision-b.job.revision : (a.job.createdAt ?? a.job.updatedAt).localeCompare(b.job.createdAt ?? b.job.updatedAt));
}
// A capture ends as local_complete (the diary has it), conflict (the user's
// edit was kept) or excluded (Git shows no development change). 'complete' is
// the end of a record that also got a Space page, before Space publishing was
// removed. Such a record stays as it is.
const finished = job => ['complete', 'local_complete', 'conflict', 'excluded'].includes(job.phase);
// git_paused is not finished: every sync rechecks it and resumes it once Git
// answers. It only stops waking the app.
const waiting = job => !finished(job) && job.phase !== 'git_paused';
const RETRY_DELAYS = [60000, 300000, 900000, 3600000];
const retryDelay = failures => RETRY_DELAYS[Math.min(failures, RETRY_DELAYS.length) - 1];
// Twelve failures on the ladder above span about nine hours of the app being
// open (1+5+15 minutes, then hourly). Past that the cause is not passing.
export const GIT_RETRY_LIMIT = 12;
// The number a capture's diary payload carries. The app shows the highest
// number it has taken for a work and refuses other content under a number it
// holds. So prepare gives each capture a higher number than every capture
// before it, and nothing changes it afterwards. A schemaVersion 1 job has none
// stored and uses revision + 1.
const deliveryRevision = job => job.localRevision ?? job.revision + 1;
// The highest number a job has used or reserved.
const usedRevision = job => Math.max(job.finalRevision ?? 0, job.diaryReceipt?.revision ?? 0, deliveryRevision(job));
// Whether the app confirmed that the diary took this capture, under any number.
const inDiary = job => job.localSaved === true || ['complete', 'local_complete'].includes(job.phase) || ['saved', 'unchanged'].includes(job.diaryReceipt?.outcome);
// The Space page of a work published before Space publishing was removed.
const pageUrlOf = job => job.pageUrl ?? (job.spaceVerified || job.phase === 'complete' ? job.space?.pageUrl ?? null : null);
// A job that a version before 0.32.0 prepared has no diaryMode until that
// version's sync reaches it. Run again with Space off, that version gives such
// a job a new number above its work's other captures, which can put it over a
// later capture the diary already shows. Marked 'local' it keeps its number in
// both versions. Only a job that never sent anything is marked: one that may
// have sent 'pending' under its number has to send the same content again.
function keepNumber(file, job) {
  if (finished(job) || job.diaryMode === 'local' || job.localSaved || job.diaryReceipt || job.attempts) return;
  // The payload file is written before the app's command line is started.
  if (fs.existsSync(path.join(path.dirname(file), `diary-${deliveryRevision(job)}.json`))) return;
  job.diaryMode = 'local';
  atomicJson(file, job);
}
function writeWake(stateDir) {
  const pending = loadJobs(stateDir).map(x=>x.job).filter(waiting);
  atomicJson(path.join(stateDir,'wake.json'), {pending:pending.length,nextAttemptAt:pending.length ? Math.min(...pending.map(j=>j.nextAttemptAt ?? 0)) : null});
}
// Runs the app's command line with a time limit and a cap on what it prints.
function runBounded(executable, args, { cwd, timeoutMs }) {
  return new Promise((resolve, reject) => {
    const child = spawn(executable, args, { cwd, windowsHide: true, stdio: ['pipe', 'pipe', 'pipe'] });
    let printed = 0, stderr = '', failure;
    const fail = message => { failure ??= new Error(message); child.kill(); };
    const timer = setTimeout(() => fail('기록 실행 제한 시간을 초과했습니다.'), timeoutMs);
    child.on('error', error => { clearTimeout(timer); reject(error); });
    child.stdout.on('data', data => { printed += data.length; if (printed > 8 * 1024 * 1024) fail('기록 실행 출력 크기 제한을 초과했습니다.'); });
    child.stderr.setEncoding('utf8');
    child.stderr.on('data', data => { stderr += data; if (Buffer.byteLength(stderr) > 65536) fail('기록 실행 오류 출력 크기 제한을 초과했습니다.'); });
    child.stdin.on('error', () => {}); child.stdin.end();
    child.on('close', exitCode => {
      clearTimeout(timer);
      if (failure) reject(failure); else resolve({ exitCode, stderr });
    });
  });
}
// Takes every named lock or none. Each name is an OS-owned named pipe, which
// disappears on process exit: no stale PID marker or check/unlink race. The
// loopback fallback is only for non-Windows tests.
async function holdLocks(prefix, identities, busyMessage) {
  const held = [];
  const release = () => Promise.all(held.map(server => new Promise(resolve => server.close(resolve))));
  try {
    for (const identity of identities) {
      const address = process.platform === 'win32'
        ? { path: `\\\\.\\pipe\\${prefix}-${identity}`, exclusive: true }
        : { host: '127.0.0.1', port: 20000 + parseInt(identity.slice(0, 8), 16) % 40000, exclusive: true };
      const server = createServer(socket => socket.destroy());
      await new Promise((resolve, reject) => {
        server.once('error', error => reject(new Error(`${busyMessage}: ${error.code}`)));
        server.listen(address, resolve);
      });
      held.push(server);
    }
  } catch (error) {
    await release();
    throw error;
  }
  return release;
}
async function locked(stateDir, action) {
  fs.mkdirSync(stateDir, { recursive: true });
  // Earlier versions named the lock after the JS-resolved spelling they were
  // given. Holding that name as well as the canonical one keeps such a process
  // excluded when it was given the same spelling as this one, or the long one.
  // A process from before this change given a different short spelling than
  // this one cannot be detected: Node has no cheap way to compute short names.
  const identities = [...new Set([fs.realpathSync(stateDir), canonicalPath(stateDir)].map(p => sha(p.toLowerCase())))];
  const release = await holdLocks('timeschedule-record', identities, '다른 기록 작업이 실행 중이거나 잠금을 열지 못했습니다');
  try {
    return await action();
  } finally { await release(); }
}
// Windows paths are case-insensitive; elsewhere /work/Repo and /work/repo can
// be different repositories.
export function sameResolvedPath(a, b, platform = process.platform) {
  return platform === 'win32' ? a.toLowerCase() === b.toLowerCase() : a === b;
}
function samePlace(a, b) {
  try { return sameResolvedPath(canonicalPath(a), canonicalPath(b)); } catch { return false; }
}

// Times and commits a record may show, all from evidence: the begin receipt,
// a confirmed completion, and commits Git confirms this work owns. Keys are
// added only when known, so a source without them hashes exactly as before.
function attachTimeline(source, { completedAt, commits }, stateDir) {
  const start = beginReceipt(stateDir, source.workId);
  if (start) source.startedAt = start.at;
  if (completedAt) {
    if (start && Date.parse(completedAt) < Date.parse(start.at)) throw new Error('완료 시각이 작업 시작 영수증보다 앞설 수 없습니다.');
    source.completedAt = completedAt;
  }
  // Explicit commits must all verify. Without them, the owned-commit line an
  // earlier finish wrote is exact evidence; only what still verifies is shown.
  const explicit = commits !== null;
  const hashes = explicit ? commits : ownedCommitLine(source.detailedMarkdown);
  if (!hashes.length) return;
  const owned = ownedCommits(source.repoPath, { workId: source.workId, base: source.gitChanges.base, hashes, owner: start?.owner ?? null });
  if (owned.transient) throw new Error(`Git으로 작업 커밋을 확인하지 못했습니다 (${owned.detail}). 기록을 만들지 않았습니다. 원인을 해결한 뒤 같은 입력으로 다시 prepare하세요.`);
  if (owned.invalid) {
    if (explicit) throw new Error(`관련 커밋을 이 작업이 소유한 실제 커밋으로 확인하지 못했습니다 (${owned.invalid}). 기록을 만들지 않았습니다.`);
    return;
  }
  if (owned.commits.length) source.commits = owned.commits;
}

export async function prepare(input, { stateDir = DEFAULT_STATE, projectRoot = PROJECT_ROOT } = {}) {
  const { source, ...timeline } = normalizeSource(input, projectRoot);
  const evidence = developmentEvidence(source.repoPath, input.gitChanges, projectRoot);
  // A temporary Git failure is not a finding that the work has no development
  // changes, so it must not read as "skipped" and end the record.
  if (evidence.transient) throw new Error(`Git으로 개발 변경을 확인하지 못했습니다 (${evidence.detail}). 기록을 만들지 않았습니다. 원인을 해결한 뒤 같은 입력으로 다시 prepare하세요.`);
  if (!evidence.eligible) return { workId: source.workId, skipped: true, reason: evidence.reason };
  source.gitChanges = evidence.gitChanges;
  attachTimeline(source, timeline, stateDir);
  return locked(stateDir, async () => {
    const file = path.join(stateDir, source.workId, 'state.json');
    const earlier = loadJobs(stateDir, source.workId);
    const previous = earlier.find(x => x.file === file)?.job ?? null;
    // Keep the repository spelling this work was first saved with: the document
    // vault groups a work's records by it, so a new spelling of the same folder
    // would look like a move and be refused.
    if (previous?.source?.repoPath && previous.source.repoPath !== source.repoPath && samePlace(previous.source.repoPath, source.repoPath)) source.repoPath = previous.source.repoPath;
    const contentHash = contentHashOf(source);
    if (previous?.contentHash === contentHash && previous.phase !== 'excluded') return { workId: source.workId, revision: previous.revision, unchanged: true, phase: previous.phase };
    for (const { file: earlierFile, job } of earlier) keepNumber(earlierFile, job);
    if (previous && !finished(previous)) atomicJson(path.join(stateDir, source.workId, `pending-${previous.revision}.json`), previous);
    const revision = (previous?.revision ?? 0) + 1;
    // Above every number a capture of this work used or reserved, whichever
    // file it is in.
    const localRevision = Math.max(0, ...earlier.map(x => usedRevision(x.job))) + 1;
    const pageUrl = earlier.map(x => pageUrlOf(x.job)).findLast(Boolean);
    // schemaVersion, finalRevision and diaryMode are for a checkout from before
    // Space publishing was removed, which may run on this folder again: with
    // them it sends this capture once under localRevision and numbers the next
    // capture above it. This version reads finalRevision only as a used number.
    const job = { schemaVersion: 2, source, revision, contentHash, phase: 'prepared',
      localRevision, finalRevision: localRevision + 1, diaryMode: 'local', localSaved: false, ...(pageUrl ? { pageUrl } : {}),
      attempts: 0, nextAttemptAt: 0, lastError: null, createdAt: new Date().toISOString(), updatedAt: new Date().toISOString() };
    atomicJson(path.join(stateDir, source.workId, `${revision}.json`), { source, revision, contentHash });
    atomicJson(file, job);
    writeWake(stateDir);
    return { workId: source.workId, revision, unchanged: false, phase: job.phase };
  });
}

// Jobs prepared before times were recorded keep sending the v2 payload the app
// already accepted for them; only a source with evidence sends v3.
// deliveryState and spaceUrl are fields of that payload format. A job prepared
// while Space publishing existed may already have sent 'pending' under its
// number, and the app refuses other content under a number it holds, so such a
// job sends 'pending' again. Every job prepared since sends 'local'.
export function diaryPayload(job) {
  const { source } = job;
  const timeline = {
    ...(source.startedAt ? { startedAt: source.startedAt } : {}),
    ...(source.completedAt ? { completedAt: source.completedAt } : {}),
    ...(source.commits?.length ? { commits: source.commits } : {}),
  };
  return { schemaVersion: Object.keys(timeline).length ? 3 : 2, workId: source.workId, revision: deliveryRevision(job), projectTitle: source.projectTitle, repoPath: source.repoPath, date: source.date,
    kind: source.kind, title: source.title, summary: source.diarySummary, detail:source.detailedMarkdown,
    deliveryState: job.diaryMode === 'local' ? 'local' : 'pending', spaceUrl: '', status: source.status, ...timeline };
}
async function registerNative(options) {
  const resultFile=path.join(options.stateDir,`registration-${randomUUID()}.json`);
  const args=['record','configure-sync','--node',process.execPath,'--script',fileURLToPath(import.meta.url),'--state-dir',options.stateDir,'--result',resultFile];
  if(options.dataDir) args.push('--data-dir',path.resolve(options.dataDir));
  const result=await runBounded(path.resolve(options.exe ?? path.join(PROJECT_ROOT,'artifacts','TimeSchedule.exe')),args,{cwd:PROJECT_ROOT,timeoutMs:10000});
  if(result.exitCode!==0 || !fs.existsSync(resultFile) || readJson(resultFile).outcome!=='configured') throw new Error('앱의 기록 재시도 경로를 등록하지 못했습니다. 최신 EXE를 확인해 주세요.');
  fs.unlinkSync(resultFile);
}
async function importNative(record, options) {
  const directory = path.join(options.stateDir, record.workId);
  const input = path.join(directory, `diary-${record.revision}.json`);
  const resultFile = path.join(directory, `receipt-${randomUUID()}.json`);
  atomicJson(input, record);
  const args = ['record', 'enqueue', '--input', input, '--result', resultFile];
  if (options.dataDir) args.push('--data-dir', path.resolve(options.dataDir));
  const result = await runBounded(path.resolve(options.exe ?? path.join(PROJECT_ROOT, 'artifacts', 'TimeSchedule.exe')), args, { cwd: PROJECT_ROOT, timeoutMs: 30000 });
  if (!fs.existsSync(resultFile)) throw new Error(`다이어리 수신 결과가 없습니다 (exit ${result.exitCode}). ${result.stderr.slice(-500)}`);
  const receipt = readJson(resultFile);
  if (receipt.outcome === 'error') throw new Error(receipt.message ?? '다이어리 저장 오류');
  return receipt;
}

export async function sync(options = {}) {
  const stateDir = path.resolve(options.stateDir ?? DEFAULT_STATE);
  return locked(stateDir, async () => {
    const vaultRoot = options.vaultRoot ?? vaultPreferences(stateDir).root;
    const results = []; const jobs=loadJobs(stateDir); let processed=0;
    let registered = false;
    const now=Date.now();
    // Jobs Git could not check in this run. They are not delivered.
    const unverified = new Set(), failedNow = new Set();
    // Each check can wait up to Git's 10-second timeout. The app stops a run
    // after 240 seconds, so checking stops at a budget and wake.json is still
    // written. Unchecked jobs are left as they are and stay undelivered.
    const checkDeadline = Date.now() + Number(options.gitCheckBudgetMs ?? 60000);
    let gitFailing = false;
    // Apply the policy before registration, backoff and per-run limits: old
    // research jobs must stop waking the app even if their retry is not due.
    for (const { file, job } of jobs) {
      if (finished(job) || (options.workId && job.source.workId !== options.workId)) continue;
      keepNumber(file, job);
      const due = job.phase !== 'git_paused' && (options.retryNow || (job.nextAttemptAt ?? 0) <= now);
      // Once Git has failed in this run, jobs only waiting out a Git delay are
      // not checked again: a hanging Git must not spend the run on them.
      if (Date.now() >= checkDeadline || (gitFailing && !due && (job.gitFailures || job.phase === 'git_paused'))) {
        unverified.add(job);
        continue;
      }
      const evidence = developmentEvidence(job.source.repoPath, job.source.gitChanges, options.projectRoot ?? PROJECT_ROOT);
      if (evidence.transient) {
        // Nothing is known about this work yet: never deliver it unverified,
        // even on --retry-now. Only a check that was due counts as a failed
        // attempt, so rechecking waiting jobs does not push their retry back.
        gitFailing = true;
        unverified.add(job);
        if (!due) continue;
        failedNow.add(job);
        job.gitFailures = (job.gitFailures ?? 0) + 1;
        job.lastError = `Git 확인 일시 실패 ${job.gitFailures}회 (${evidence.detail}).`;
        if (job.gitFailures >= GIT_RETRY_LIMIT) {
          job.pausedFromPhase = job.phase; job.phase = 'git_paused'; job.nextAttemptAt = null;
          job.lastError += ' 자동 재시도를 멈췄습니다. 원인을 해결한 뒤 sync를 실행하면 다시 확인합니다.';
        } else job.nextAttemptAt = now + retryDelay(job.gitFailures);
        job.updatedAt = new Date().toISOString(); atomicJson(file, job);
        continue;
      }
      const resumed = job.phase === 'git_paused';
      if (resumed) { job.phase = job.pausedFromPhase ?? 'prepared'; delete job.pausedFromPhase; job.nextAttemptAt = 0; }
      if (job.gitFailures || resumed) {
        // The pending delay was only for Git, which now answers.
        job.gitFailures = 0; job.lastError = null; job.nextAttemptAt = 0;
        job.updatedAt = new Date().toISOString(); atomicJson(file, job);
      }
      if (!evidence.eligible) {
        job.excludedFromPhase = job.phase;
        job.phase = 'excluded'; job.exclusionReason = evidence.reason;
        job.nextAttemptAt = null; job.updatedAt = new Date().toISOString();
        atomicJson(file, job);
      }
    }
    writeWake(stateDir);
    for (const { file, job } of jobs) {
      if(options.workId && job.source.workId!==options.workId) continue;
      if (unverified.has(job) || job.phase === 'git_paused') {
        results.push({ workId: job.source.workId, revision: job.revision, phase: job.phase, skipped: true,
          ...(failedNow.has(job) || job.phase === 'git_paused' ? { error: job.lastError } : {}) });
        continue;
      }
      if (finished(job) || (!options.retryNow && (job.nextAttemptAt ?? 0)>now) || processed >= Number(options.maxJobs ?? Infinity)) {
        results.push({ workId: job.source.workId,revision:job.revision, phase: job.phase, skipped: true }); continue;
      }
      processed++;
      const save = () => { job.updatedAt = new Date().toISOString(); atomicJson(file, job); };
      const later = jobs.filter(x => x.job.source.workId === job.source.workId && x.job.revision > job.revision).map(x => x.job);
      // Whether the app confirmed this capture is in the diary, or holds a
      // higher number for its work while a later capture exists.
      const deliver = async () => {
        const record=diaryPayload(job);
        const receipt=await (options.importer ?? importNative)(record,{...options,stateDir});
        if(receipt?.workId!==record.workId || receipt?.revision!==record.revision) throw new Error('다이어리 수신 결과의 작업 ID 또는 revision이 다릅니다.');
        job.diaryReceipt=receipt;
        if(receipt.outcome==='conflict') {job.phase='conflict';throw new Error(receipt.message ?? '사용자 편집 충돌');}
        if(receipt.outcome==='queued'){job.phase='diary_queued';job.nextAttemptAt=now+60000;save();return false;}
        if(receipt.outcome==='stale' && receipt.entryId && later.length)return true;
        if(!(['saved','unchanged'].includes(receipt.outcome)&&receipt.entryId)) throw new Error(`다이어리 저장을 확인하지 못했습니다: ${receipt.outcome}`);
        return true;
      };
      try {
        // Preserve one source revision as an independent document before the
        // native executable is required.
        const repoIdentity = process.platform === 'win32' ? job.source.repoPath.toLowerCase() : job.source.repoPath;
        const receipt = await saveDocument({ id: job.source.workId, collection: 'projects', group: `repo-${sha(repoIdentity).slice(0, 16)}`,
          title: job.source.title, date: job.source.date, markdown: job.source.detailedMarkdown,
          tags: [job.source.kind], source: { projectTitle: job.source.projectTitle, repoPath: job.source.repoPath, gitChanges: job.source.gitChanges, status: job.source.status, captureRevision: job.revision } }, { root: vaultRoot });
        job.vaultSaved = true; job.vaultReceipt = receipt; save();
        if (!options.importer && !registered) { await registerNative({...options,stateDir}); registered = true; }
        // A job that was waiting for its Space page when Space publishing was
        // removed is already in the diary (localSaved). It ends here and is
        // not sent again. Nor is a capture sent once a later capture of its
        // work is in the diary: a version before 0.32.0 could give an earlier
        // capture the higher number, and the diary shows the highest.
        if (!job.localSaved && !later.some(inDiary) && !await deliver()) continue;
        job.localSaved = true;
        job.phase='local_complete';job.lastError=null;job.nextAttemptAt=null;save();
      } catch (error) {
        // The document vault kept the user's edit of the saved document.
        if (error.conflict) job.phase = 'conflict';
        job.attempts=(job.attempts??0)+1;
        job.nextAttemptAt=now+retryDelay(job.attempts);
        job.lastError = String(error.message ?? error).slice(0, 2000); save();
      } finally {
        results.push({ workId: job.source.workId, revision: job.revision, phase: job.phase, error: job.lastError });
      }
    }
    writeWake(stateDir);
    return results;
  });
}

export function status({ stateDir = DEFAULT_STATE, workId } = {}) {
  return loadJobs(stateDir, workId).map(({ job }) => ({ workId: job.source.workId, title: job.source.title, revision: job.revision, phase: job.phase,
    pageUrl: pageUrlOf(job), localSaved:job.localSaved,
    vaultSaved:job.vaultSaved ?? false, localFile:job.vaultReceipt?.file ?? null,
    nextAttemptAt:job.nextAttemptAt,lastError: job.lastError, exclusionReason: job.exclusionReason, updatedAt: job.updatedAt }));
}

async function main(args) {
  const command = args.shift(); const options = {};
  const names = { '--state-dir': 'stateDir', '--work-id': 'workId', '--exe': 'exe', '--data-dir': 'dataDir', '--input': 'input', '--max-jobs':'maxJobs' };
  while (args.length) {
    const key = args.shift(); if(key==='--retry-now'){options.retryNow=true;continue;} const value = args.shift();
    if (!names[key] || !value || value.startsWith('--') || Object.hasOwn(options, names[key])) throw new Error('명령 인자를 확인해 주세요.');
    options[names[key]] = value;
  }
  if(options.maxJobs && (!/^\d+$/.test(options.maxJobs) || Number(options.maxJobs)<1 || Number(options.maxJobs)>32))throw new Error('max-jobs는 1~32여야 합니다.');
  let result;
  if (command === 'prepare' && options.input) result = await prepare(readJson(options.input), options);
  else if (command === 'sync') result = await sync(options);
  else if (command === 'status') result = status(options);
  else throw new Error('사용법: node scripts/project-records.mjs prepare --input FILE | sync | status');
  console.log(JSON.stringify(result, null, 2));
  if (command === 'sync' && result.some(r => r.error || r.phase === 'conflict')) process.exitCode = 1;
}
if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main(process.argv.slice(2)).catch(error => { console.error(error.message); process.exitCode = 1; });
}
