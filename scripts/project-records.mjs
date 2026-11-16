import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { createHash, randomUUID } from 'node:crypto';
import { createServer } from 'node:net';
import { publishCodex, validatePublication, runBounded } from './project-records-codex.mjs';
import { developmentEvidence } from './project-records-git.mjs';
import { saveDocument, vaultPreferences } from './document-vault.mjs';

export const PROJECT_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
export const PARENT_PAGE_ID = 'page_6ae1b18a44f08191ad2a86276972476c';
const DEFAULT_STATE = path.join(PROJECT_ROOT, '.cache', 'project-records');
const UUID = /^[a-f0-9]{8}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{12}$/;
const LABELS = { in_progress: '진행 중', verification_pending: '검증 대기', verified: '확인한 범위 검증 완료', completed: '작업 정리 완료', merged: '병합 완료' };
const normal = value => value.replace(/\r\n/g, '\n').trim();
const sha = value => createHash('sha256').update(value).digest('hex');

function text(value, name, max) {
  if (typeof value !== 'string' || !value.trim() || value.includes('\0') || [...value].length > max) throw new Error(`${name} 값을 확인해 주세요.`);
  return normal(value);
}
function normalizeSource(input, projectRoot = PROJECT_ROOT) {
  if (!input || !UUID.test(input.workId)) throw new Error('고정된 UUID workId가 필요합니다. 같은 작업의 갱신에는 같은 ID를 사용하세요.');
  const repoPath = text(input.repoPath, 'repoPath', 2000);
  if (!path.isAbsolute(repoPath) || fs.realpathSync.native(repoPath).toLowerCase() !== fs.realpathSync.native(projectRoot).toLowerCase()) throw new Error('현재 설정된 TimeSchedule 저장소의 기록만 처리합니다.');
  const date = text(input.date, 'date', 10);
  if (!/^\d{4}-\d{2}-\d{2}$/.test(date) || Number(date.slice(0, 4)) < 1 || new Date(`${date}T00:00:00Z`).toISOString().slice(0, 10) !== date) throw new Error('실제 YYYY-MM-DD 날짜가 필요합니다.');
  if (!Object.hasOwn(LABELS, input.status)) throw new Error('지원하지 않는 작업 상태입니다.');
  if (!['development', 'troubleshooting', 'portfolio', 'summary'].includes(input.kind)) throw new Error('지원하지 않는 기록 종류입니다.');
  const detailedMarkdown = text(input.detailedMarkdown, '상세 기록', 48000);
  if (Buffer.byteLength(detailedMarkdown) > 48000) throw new Error('상세 기록은 UTF-8 48,000바이트 이하로 작업 단위를 나눠 주세요.');
  const paragraphs = detailedMarkdown.split(/\n{2,}/);
  const diarySummary = text(input.diarySummary ?? paragraphs.find(p => !/^\s*(#|```|\|)/.test(p)), '다이어리 요약', 10000);
  // Extract whole paragraphs, including their caveats. No free-form second
  // summary can invent a cause, change a number, or drop half a paragraph.
  if (!diarySummary.split(/\n{2,}/).every(p => paragraphs.includes(p))) throw new Error('다이어리 요약은 한계를 포함한 원문 문단을 그대로 골라 주세요.');
  return { workId: input.workId, projectTitle: text(input.projectTitle, '프로젝트 제목', 200), repoPath: fs.realpathSync.native(repoPath), date,
    kind: input.kind, title: text(input.title, '기록 제목', 200), status: input.status, detailedMarkdown, diarySummary, parentPageId: PARENT_PAGE_ID };
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
const finished = job => ['complete', 'local_complete', 'conflict', 'excluded'].includes(job.phase);
const uncertainPublication = job => job?.publicationUncertain === true
  || (job && job.schemaVersion !== 2 && (job.phase === 'publishing' || job.excludedFromPhase === 'publishing'));
function writeWake(stateDir) {
  const pending = loadJobs(stateDir).map(x=>x.job).filter(j=>!finished(j));
  atomicJson(path.join(stateDir,'wake.json'), {pending:pending.length,nextAttemptAt:pending.length ? Math.min(...pending.map(j=>j.nextAttemptAt ?? 0)) : null});
}
function upgradeJob(job) {
  if(job.schemaVersion===2 || finished(job)) return;
  job.schemaVersion=2; job.localRevision=job.revision+1; job.finalRevision=job.revision+2;
  job.localSaved=false; job.spaceVerified=['space_saved','diary_queued'].includes(job.phase);
  job.publicationUncertain=job.phase==='publishing';
}
async function locked(stateDir, action) {
  fs.mkdirSync(stateDir, { recursive: true });
  // Native resolution expands 8.3 short names, so every spelling of one folder
  // shares one lock.
  const identity = sha(fs.realpathSync.native(stateDir).toLowerCase());
  // An OS-owned named pipe disappears on process exit: no stale PID marker or
  // check/unlink race. The loopback fallback is only for non-Windows tests.
  const address = process.platform === 'win32'
    ? { path: `\\\\.\\pipe\\timeschedule-record-${identity}`, exclusive: true }
    : { host: '127.0.0.1', port: 20000 + parseInt(identity.slice(0, 8), 16) % 40000, exclusive: true };
  const server = createServer(socket => socket.destroy());
  await new Promise((resolve, reject) => {
    server.once('error', error => reject(new Error(`다른 기록 작업이 실행 중이거나 잠금을 열지 못했습니다: ${error.code}`)));
    server.listen(address, resolve);
  });
  try {
    return await action();
  } finally { await new Promise(resolve => server.close(resolve)); }
}

export async function prepare(input, { stateDir = DEFAULT_STATE, projectRoot = PROJECT_ROOT } = {}) {
  const source = normalizeSource(input, projectRoot);
  const evidence = developmentEvidence(source.repoPath, input.gitChanges, projectRoot);
  if (!evidence.eligible) return { workId: source.workId, skipped: true, reason: evidence.reason };
  source.gitChanges = evidence.gitChanges;
  return locked(stateDir, async () => {
    const file = path.join(stateDir, source.workId, 'state.json');
    const previous = fs.existsSync(file) ? readJson(file) : null;
    const contentHash = sha(JSON.stringify(source));
    if (previous?.contentHash === contentHash && previous.phase !== 'excluded') return { workId: source.workId, revision: previous.revision, unchanged: true, phase: previous.phase };
    if (previous && !finished(previous)) {
      upgradeJob(previous);
      atomicJson(path.join(stateDir, source.workId, `pending-${previous.revision}.json`), previous);
    }
    const revision = (previous?.revision ?? 0) + 1;
    const pageMarkdown = `**프로젝트:** ${source.projectTitle}\n\n**작업일:** ${source.date}\n\n**상태:** ${LABELS[source.status]}\n\n${source.detailedMarkdown}`;
    const localRevision = (previous?.finalRevision ?? previous?.diaryReceipt?.revision ?? previous?.revision ?? 0)+1;
    const job = { schemaVersion: 2, source, revision, contentHash, pageMarkdown, phase: 'prepared', space: previous?.space ?? null,
      localRevision, finalRevision:localRevision+1, localSaved:false, spaceVerified:false,
      publicationUncertain: !!uncertainPublication(previous),
      attempts:0,nextAttemptAt:0,modelCalls: previous?.modelCalls ?? 0, lastError: null, createdAt:new Date().toISOString(), updatedAt: new Date().toISOString() };
    atomicJson(path.join(stateDir, source.workId, `${revision}.json`), { source, revision, contentHash, pageMarkdown });
    atomicJson(file, job);
    writeWake(stateDir);
    return { workId: source.workId, revision, unchanged: false, phase: job.phase };
  });
}

function diaryPayload(job, linked) {
  const { source, space } = job;
  return { schemaVersion: 2, workId: source.workId, revision:linked?job.finalRevision:job.localRevision, projectTitle: source.projectTitle, repoPath: source.repoPath, date: source.date,
    kind: source.kind, title: source.title, summary: source.diarySummary, detail:source.detailedMarkdown,
    deliveryState:linked?'synced':job.diaryMode==='local'?'local':'pending', spaceUrl:linked?space.pageUrl:'', status: source.status };
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
    const preferences = vaultPreferences(stateDir);
    const vaultRoot = options.vaultRoot ?? preferences.root;
    const spaceEnabled = options.spaceEnabled ?? preferences.spaceEnabled;
    if (typeof spaceEnabled !== 'boolean') throw new Error('Space 설정을 확인해 주세요.');
    const results = []; const jobs=loadJobs(stateDir); let processed=0;
    let registered = false;
    const now=Date.now();
    // Reserve new delivery revisions across the entire work item, including
    // queued/unsent newer captures. Per-job increments can reuse a revision
    // already allocated to another body.
    const nextDelivery = new Map();
    for (const { job } of jobs) {
      if (!finished(job)) upgradeJob(job);
      const greatest = Math.max(nextDelivery.get(job.source.workId) ?? 0,
        job.localRevision ?? 0, job.finalRevision ?? 0, job.diaryReceipt?.revision ?? 0, job.revision ?? 0);
      nextDelivery.set(job.source.workId, greatest);
    }
    // Apply the policy before registration, backoff and per-run limits: old
    // research jobs must stop waking the app even if their retry is not due.
    for (const { file, job } of jobs) {
      if (options.workId && job.source.workId !== options.workId) continue;
      if (job.phase === 'local_complete' && spaceEnabled && options.workId === job.source.workId) { job.phase = 'waiting_space'; job.nextAttemptAt = 0; }
      if (finished(job) || (options.workId && job.source.workId !== options.workId)) continue;
      upgradeJob(job);
      const evidence = developmentEvidence(job.source.repoPath, job.source.gitChanges, options.projectRoot ?? PROJECT_ROOT);
      if (!evidence.eligible) {
        job.excludedFromPhase = job.phase;
        job.phase = 'excluded'; job.exclusionReason = evidence.reason;
        job.nextAttemptAt = null; job.updatedAt = new Date().toISOString();
        atomicJson(file, job);
      } else if (!spaceEnabled && job.diaryMode !== 'local') {
        // A legacy pending receipt may already be in the app's inbox. A new
        // revision avoids changing the meaning of that accepted payload.
        job.localRevision = nextDelivery.get(job.source.workId) + 1;
        job.finalRevision = job.localRevision + 1;
        nextDelivery.set(job.source.workId, job.finalRevision);
        job.localSaved = false;
        job.diaryMode = 'local'; job.nextAttemptAt = 0;
        atomicJson(file, job);
      }
    }
    writeWake(stateDir);
    for (const { file, job } of jobs) {
      if(options.workId && job.source.workId!==options.workId) continue;
      if (finished(job) || (!options.retryNow && (job.nextAttemptAt ?? 0)>now) || processed >= Number(options.maxJobs ?? Infinity)) {
        results.push({ workId: job.source.workId,revision:job.revision, phase: job.phase, skipped: true }); continue;
      }
      processed++; upgradeJob(job);
      // A previously prepared revision may predate the policy migration. An
      // excluded older attempt can still have created a page remotely.
      const uncertainPrior = jobs.filter(({job: prior}) => prior.source.workId === job.source.workId
        && prior.revision < job.revision && prior.phase === 'excluded' && uncertainPublication(prior)).at(-1)?.job;
      if (uncertainPrior) {
        job.publicationUncertain = true;
        job.space ??= uncertainPrior.space;
      }
      const inheritedUncertainty=job.publicationUncertain;
      const save = () => { job.updatedAt = new Date().toISOString(); atomicJson(file, job); };
      const importRecord=async linked=>{
        const record=diaryPayload(job,linked);
        const receipt=await (options.importer ?? importNative)(record,{...options,stateDir});
        if(receipt?.workId!==record.workId || receipt?.revision!==record.revision) throw new Error('다이어리 수신 결과의 작업 ID 또는 revision이 다릅니다.');
        job.diaryReceipt=receipt;
        if(receipt.outcome==='conflict') {job.phase='conflict';throw new Error(receipt.message ?? '사용자 편집 충돌');}
        if(receipt.outcome==='queued'){job.phase='diary_queued';job.nextAttemptAt=now+60000;save();return false;}
        if(receipt.outcome==='stale' && receipt.entryId && jobs.some(x=>x.job.source.workId===job.source.workId && x.job.revision>job.revision && x.job.localSaved))return true;
        if(!(['saved','unchanged'].includes(receipt.outcome)&&receipt.entryId)) throw new Error(`다이어리 저장을 확인하지 못했습니다: ${receipt.outcome}`);
        return true;
      };
      try {
        // Preserve one source revision as an independent document before any
        // native executable or optional external publisher is required.
        const repoIdentity = process.platform === 'win32' ? job.source.repoPath.toLowerCase() : job.source.repoPath;
        const receipt = await saveDocument({ id: job.source.workId, collection: 'projects', group: `repo-${sha(repoIdentity).slice(0, 16)}`,
          title: job.source.title, date: job.source.date, markdown: job.source.detailedMarkdown,
          tags: [job.source.kind], source: { projectTitle: job.source.projectTitle, repoPath: job.source.repoPath, gitChanges: job.source.gitChanges, status: job.source.status, captureRevision: job.revision } }, { root: vaultRoot });
        job.vaultSaved = true; job.vaultReceipt = receipt; save();
        if (!options.importer && !registered) { await registerNative({...options,stateDir}); registered = true; }
        if(!job.localSaved){if(!await importRecord(false))continue;job.localSaved=true;job.phase='waiting_space';save();}
        if (!spaceEnabled) { job.phase='local_complete'; job.lastError=null; job.nextAttemptAt=null; save(); continue; }
        // Keep revisions of one work item ordered, but let other work proceed.
        const earlier=jobs.find(x=>x.job.source.workId===job.source.workId && x.job.revision<job.revision && !['complete', 'local_complete', 'excluded'].includes(x.job.phase));
        if(earlier){job.phase=earlier.job.phase==='conflict'?'conflict':'waiting_prior';job.nextAttemptAt=Math.max(now+60000,earlier.job.nextAttemptAt??0);save();continue;}
        const mappingFile=path.join(stateDir,job.source.workId,'space-state.json');
        if(!job.spaceVerified && fs.existsSync(mappingFile)){
          const mapping=readJson(mappingFile);if(mapping.revision<=job.revision)job.space=mapping.space;
        }
        if (!job.spaceVerified) {
          const recoverOnly = job.publicationUncertain;
          job.phase = 'publishing'; job.modelCalls++; save();
          job.publicationUncertain=true; save();
          const published = await (options.publisher ?? publishCodex)(job, { ...options, stateDir, recoverOnly, cwd: PROJECT_ROOT });
          validatePublication(published, job);
          job.space = published; job.spaceVerified=true;job.publicationUncertain=false;job.phase = 'space_saved'; job.lastError = null;
          atomicJson(mappingFile,{revision:job.revision,space:published});save();
        }
        if(!await importRecord(true))continue;
        job.phase='complete';job.lastError=null;job.nextAttemptAt=null;save();
      } catch (error) {
        if (error.space) job.space = error.space;
        if (error.conflict) job.phase = 'conflict';
        if(error.publicationNotStarted && !error.conflict && !inheritedUncertainty){job.publicationUncertain=false;job.phase='waiting_space';}
        job.attempts=(job.attempts??0)+1;
        job.nextAttemptAt=now+[60000,300000,900000,3600000][Math.min(job.attempts-1,3)];
        job.lastError = String(error.message ?? error).slice(0, 2000); save();
      } finally {
        results.push({ workId: job.source.workId, revision: job.revision, phase: job.phase, pageUrl: job.spaceVerified ? job.space?.pageUrl ?? null : null, error: job.lastError });
      }
    }
    writeWake(stateDir);
    return results;
  });
}

export function status({ stateDir = DEFAULT_STATE, workId } = {}) {
  return loadJobs(stateDir, workId).map(({ job }) => ({ workId: job.source.workId, title: job.source.title, revision: job.revision, phase: job.phase,
    pageUrl: job.spaceVerified || job.phase==='complete' ? job.space?.pageUrl ?? null : null, modelCalls: job.modelCalls, localSaved:job.localSaved,
    vaultSaved:job.vaultSaved ?? false, localFile:job.vaultReceipt?.file ?? null,
    nextAttemptAt:job.nextAttemptAt,lastError: job.lastError, exclusionReason: job.exclusionReason, updatedAt: job.updatedAt }));
}

async function main(args) {
  const command = args.shift(); const options = {};
  const names = { '--state-dir': 'stateDir', '--work-id': 'workId', '--exe': 'exe', '--data-dir': 'dataDir', '--codex-exe': 'codexExe', '--input': 'input', '--max-jobs':'maxJobs' };
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
