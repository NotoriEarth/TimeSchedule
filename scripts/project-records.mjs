import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { createHash, randomUUID } from 'node:crypto';
import { createServer } from 'node:net';
import { publishCodex, validatePublication, runBounded } from './project-records-codex.mjs';

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
function normalizeSource(input) {
  if (!input || !UUID.test(input.workId)) throw new Error('고정된 UUID workId가 필요합니다. 같은 작업의 갱신에는 같은 ID를 사용하세요.');
  const repoPath = text(input.repoPath, 'repoPath', 2000);
  if (!path.isAbsolute(repoPath) || fs.realpathSync(repoPath).toLowerCase() !== fs.realpathSync(PROJECT_ROOT).toLowerCase()) throw new Error('현재 설정된 TimeSchedule 저장소의 기록만 처리합니다.');
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
  return { workId: input.workId, projectTitle: text(input.projectTitle, '프로젝트 제목', 200), repoPath: fs.realpathSync(repoPath), date,
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
    .map(e => ({ file: path.join(stateDir, e.name, 'state.json'), job: readJson(path.join(stateDir, e.name, 'state.json')) }));
}
async function locked(stateDir, action) {
  fs.mkdirSync(stateDir, { recursive: true });
  const identity = sha(fs.realpathSync(stateDir).toLowerCase());
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

export async function prepare(input, { stateDir = DEFAULT_STATE } = {}) {
  const source = normalizeSource(input);
  return locked(stateDir, async () => {
    const file = path.join(stateDir, source.workId, 'state.json');
    const previous = fs.existsSync(file) ? readJson(file) : null;
    const contentHash = sha(JSON.stringify(source));
    if (previous?.contentHash === contentHash) return { workId: source.workId, revision: previous.revision, unchanged: true, phase: previous.phase };
    if (previous?.phase === 'publishing') throw new Error('Space 저장 결과가 불확실합니다. 기존 전달을 확인한 뒤 내용을 갱신해 주세요.');
    const revision = (previous?.revision ?? 0) + 1;
    const pageMarkdown = `**프로젝트:** ${source.projectTitle}\n\n**작업일:** ${source.date}\n\n**상태:** ${LABELS[source.status]}\n\n${source.detailedMarkdown}`;
    const job = { schemaVersion: 1, source, revision, contentHash, pageMarkdown, phase: 'prepared', space: previous?.space ?? null,
      modelCalls: previous?.modelCalls ?? 0, lastError: null, updatedAt: new Date().toISOString() };
    atomicJson(path.join(stateDir, source.workId, `${revision}.json`), { source, revision, contentHash, pageMarkdown });
    atomicJson(file, job);
    return { workId: source.workId, revision, unchanged: false, phase: job.phase };
  });
}

function diaryPayload(job) {
  const { source, revision, space } = job;
  return { schemaVersion: 1, workId: source.workId, revision, projectTitle: source.projectTitle, repoPath: source.repoPath, date: source.date,
    kind: source.kind, title: source.title, summary: source.diarySummary, spaceUrl: space.pageUrl, status: source.status };
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
    const results = [];
    for (const { file, job } of loadJobs(stateDir, options.workId)) {
      if (['complete', 'conflict'].includes(job.phase)) { results.push({ workId: job.source.workId, phase: job.phase, skipped: true }); continue; }
      const save = () => { job.updatedAt = new Date().toISOString(); atomicJson(file, job); };
      try {
        if (['prepared', 'publishing'].includes(job.phase)) {
          const recoverOnly = job.phase === 'publishing';
          job.phase = 'publishing'; job.modelCalls++; save();
          const published = await (options.publisher ?? publishCodex)(job, { ...options, stateDir, recoverOnly, cwd: PROJECT_ROOT });
          validatePublication(published, job);
          job.space = published; job.phase = 'space_saved'; job.lastError = null; save();
        }
        const record = diaryPayload(job);
        const receipt = await (options.importer ?? importNative)(record, { ...options, stateDir });
        if (receipt?.workId !== record.workId || receipt?.revision !== record.revision) throw new Error('다이어리 수신 결과의 작업 ID 또는 revision이 다릅니다.');
        job.diaryReceipt = receipt;
        if (['saved', 'unchanged'].includes(receipt.outcome) && receipt.entryId) job.phase = 'complete';
        else if (receipt.outcome === 'queued') job.phase = 'diary_queued';
        else if (['conflict', 'stale'].includes(receipt.outcome)) job.phase = 'conflict';
        else throw new Error(`다이어리 저장을 확인하지 못했습니다: ${receipt?.outcome}`);
        job.lastError = job.phase === 'conflict' ? (receipt.message ?? '기록 상태 충돌') : null; save();
      } catch (error) {
        if (error.space) job.space = error.space;
        if (error.conflict) job.phase = 'conflict';
        job.lastError = String(error.message ?? error).slice(0, 2000); save();
      }
      results.push({ workId: job.source.workId, revision: job.revision, phase: job.phase, pageUrl: job.space?.pageUrl ?? null, error: job.lastError });
    }
    return results;
  });
}

export function status({ stateDir = DEFAULT_STATE, workId } = {}) {
  return loadJobs(stateDir, workId).map(({ job }) => ({ workId: job.source.workId, title: job.source.title, revision: job.revision, phase: job.phase,
    pageUrl: job.space?.pageUrl ?? null, modelCalls: job.modelCalls, lastError: job.lastError, updatedAt: job.updatedAt }));
}

async function main(args) {
  const command = args.shift(); const options = {};
  const names = { '--state-dir': 'stateDir', '--work-id': 'workId', '--exe': 'exe', '--data-dir': 'dataDir', '--codex-exe': 'codexExe', '--input': 'input' };
  while (args.length) {
    const key = args.shift(); const value = args.shift();
    if (!names[key] || !value || value.startsWith('--') || Object.hasOwn(options, names[key])) throw new Error('명령 인자를 확인해 주세요.');
    options[names[key]] = value;
  }
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
