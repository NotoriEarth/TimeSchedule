import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { createHash, randomUUID } from 'node:crypto';
import { createServer } from 'node:net';

const DEFAULT_STATE = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../.cache/project-records');
const UUID = /^[a-f0-9]{8}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{12}$/;
const hash = text => createHash('sha256').update(text).digest('hex');
const readJson = file => JSON.parse(fs.readFileSync(file, 'utf8').replace(/^\uFEFF/, ''));
const conflict = message => Object.assign(new Error(message), { conflict: true });

// Reject links inside the managed tree before reads as well as writes. The root
// itself may live on a redirected Documents volume; its canonical path is fixed.
function safePath(root, ...parts) {
  const target = path.resolve(root, ...parts), relative = path.relative(root, target);
  if (relative.startsWith('..') || path.isAbsolute(relative)) throw new Error('문서 저장소 밖의 경로입니다.');
  let current = root;
  for (const part of relative.split(path.sep).filter(Boolean)) {
    current = path.join(current, part);
    try { if (fs.lstatSync(current).isSymbolicLink()) throw new Error('문서 저장소 내부의 링크 경로는 사용할 수 없습니다.'); }
    catch (error) { if (error.code !== 'ENOENT') throw error; }
  }
  return target;
}
function atomicWrite(file, content) {
  fs.mkdirSync(path.dirname(file), { recursive: true });
  const temp = `${file}.${randomUUID()}.tmp`; let fd;
  try {
    fd = fs.openSync(temp, 'wx'); fs.writeFileSync(fd, content); fs.fsyncSync(fd); fs.closeSync(fd); fd = undefined;
    fs.renameSync(temp, file);
  } finally {
    if (fd !== undefined) fs.closeSync(fd);
    if (fs.existsSync(temp)) fs.unlinkSync(temp);
  }
}
const atomicJson = (file, value) => atomicWrite(file, JSON.stringify(value, null, 2) + '\n');

async function withLock(directory, action) {
  fs.mkdirSync(directory, { recursive: true });
  // Native resolution expands 8.3 short names, so every spelling shares one lock.
  const root = fs.realpathSync.native(directory);
  const identity = hash(process.platform === 'win32' ? root.toLowerCase() : root);
  const server = createServer(socket => socket.destroy());
  const address = process.platform === 'win32' ? { path: `\\\\.\\pipe\\timeschedule-vault-${identity}`, exclusive: true }
    : { host: '127.0.0.1', port: 20000 + parseInt(identity.slice(0, 8), 16) % 40000, exclusive: true };
  await new Promise((resolve, reject) => {
    server.once('error', error => reject(new Error(`다른 문서 작업이 실행 중이거나 잠금을 열지 못했습니다: ${error.code}`)));
    server.listen(address, resolve);
  });
  try { return await action(root); }
  finally { await new Promise(resolve => server.close(resolve)); }
}

function initialize(root) {
  const marker = safePath(root, 'vault.json');
  if (fs.existsSync(marker)) {
    const config = readJson(marker);
    if (config.schemaVersion !== 1 || !UUID.test(config.vaultId)) throw new Error('지원하지 않는 문서 저장소 형식입니다.');
  } else {
    // Never take ownership of an unrelated existing directory.
    if (fs.readdirSync(root).length) throw new Error('비어 있거나 이미 초기화된 문서 저장소 폴더를 선택해 주세요.');
    atomicJson(marker, { schemaVersion: 1, vaultId: randomUUID(), createdAt: new Date().toISOString() });
  }
  for (const folder of ['projects', 'studies', '.sync']) fs.mkdirSync(safePath(root, folder), { recursive: true });
  fs.mkdirSync(safePath(root, '.sync', 'catalog'), { recursive: true });
  const guide = safePath(root, 'README.md');
  if (!fs.existsSync(guide)) atomicWrite(guide, '# TimeScheduleData 개인 문서 저장소\n\nprojects는 프로젝트, studies는 공부 자료입니다. 각 기록의 record.md가 읽을 수 있는 원문이고 metadata.json은 분류·ID·수정 정보입니다. history에는 가져온 수정본이 남습니다. attachments에 첨부 파일을 보관할 수 있습니다.\n\nTimeSchedule 프로젝트 폴더에서 `node scripts/document-vault.mjs status`로 위치와 상태를 확인하고, `import --input FILE`로 JSON 문서를 가져옵니다. record.md를 직접 편집한 뒤 `node scripts/document-vault.mjs adopt --id UUID`로 수정본을 확정할 수 있습니다. adopt 이후 문서는 수동 관리되어 자동 갱신이 덮어쓰지 않습니다. 입력 예제와 전체 사용법은 프로젝트의 docs/document-vault.md를 참고하세요.\n\n다이어리는 별도 표시본이며 서로의 수동 편집을 자동 동기화하지 않습니다. 전체 폴더를 함께 백업하세요. .sync는 위치 목록과 복구 상태입니다. 서버 동기화는 아직 제공하지 않으며 Space는 개발 기록에서 명시적으로 켠 경우에만 사용합니다.\n');
  return { root, ...readJson(marker) };
}

export async function initVault(root) {
  if (!root || !path.isAbsolute(root)) throw new Error('문서 저장소의 절대 경로가 필요합니다.');
  return withLock(root, canonical => initialize(canonical));
}

export function vaultPreferences(stateDir = DEFAULT_STATE) {
  const file = path.join(stateDir, 'vault-settings.json');
  if (!fs.existsSync(file)) return { schemaVersion: 1, root: path.join(os.homedir(), 'Documents', 'TimeScheduleData'), spaceEnabled: false };
  const settings = readJson(file);
  if (settings.schemaVersion !== 1 || typeof settings.root !== 'string' || !path.isAbsolute(settings.root)
      || typeof settings.spaceEnabled !== 'boolean') throw new Error('문서 저장소 설정을 확인해 주세요.');
  return settings;
}

export async function configureVault(stateDir = DEFAULT_STATE, changes = {}) {
  return withLock(stateDir, async canonical => {
    const previous = vaultPreferences(canonical);
    if (changes.root !== undefined && (!path.isAbsolute(changes.root) || !changes.root.trim())) throw new Error('저장소 절대 경로가 필요합니다.');
    if (changes.spaceEnabled !== undefined && typeof changes.spaceEnabled !== 'boolean') throw new Error('Space 설정은 on 또는 off입니다.');
    const initialized = await initVault(changes.root ?? previous.root);
    const next = { schemaVersion: 1, root: initialized.root, spaceEnabled: changes.spaceEnabled ?? previous.spaceEnabled };
    atomicJson(safePath(canonical, 'vault-settings.json'), next);
    return next;
  });
}

function documentInput(input) {
  if (!input || !UUID.test(input.id)) throw new Error('문서 ID는 고정된 소문자 UUID여야 합니다.');
  if (!['projects', 'studies'].includes(input.collection)) throw new Error('collection은 projects 또는 studies입니다.');
  if (typeof input.group !== 'string' || !/^[a-z0-9][a-z0-9-]{0,79}$/.test(input.group)
      || /^(con|prn|aux|nul|com[0-9]|lpt[0-9])$/i.test(input.group)) throw new Error('group은 영문 소문자·숫자·하이픈으로 만든 안전한 분류 ID여야 합니다.');
  if (typeof input.title !== 'string' || !input.title.trim() || input.title.length > 200 || /[\0\r\n]/.test(input.title)) throw new Error('문서 제목을 확인해 주세요.');
  if (typeof input.markdown !== 'string' || !input.markdown.trim() || input.markdown.includes('\0') || Buffer.byteLength(input.markdown) > 2_000_000) throw new Error('문서 본문은 비어 있지 않은 2MB 이하 Markdown이어야 합니다.');
  if (typeof input.date !== 'string' || !/^\d{4}-\d{2}-\d{2}$/.test(input.date) || Number(input.date.slice(0, 4)) < 1
      || new Date(`${input.date}T00:00:00Z`).toISOString().slice(0, 10) !== input.date) throw new Error('실제 YYYY-MM-DD 날짜가 필요합니다.');
  const tags = input.tags ?? [];
  if (!Array.isArray(tags) || tags.length > 40 || tags.some(tag => typeof tag !== 'string' || !tag.trim() || tag.length > 80 || /[\0\r\n]/.test(tag))) throw new Error('태그 형식을 확인해 주세요.');
  const source = input.source ?? null;
  if (source !== null && (typeof source !== 'object' || Array.isArray(source) || Buffer.byteLength(JSON.stringify(source)) > 16000)) throw new Error('출처 정보 형식을 확인해 주세요.');
  return { id: input.id, collection: input.collection, group: input.group, title: input.title.trim(), date: input.date, tags, markdown: input.markdown, source };
}

function recover(root, directory) {
  const journal = safePath(root, directory, '.pending.json');
  if (!fs.existsSync(journal)) return;
  const pending = readJson(journal);
  const current = safePath(root, directory, 'record.md');
  const actual = fs.existsSync(current) ? hash(fs.readFileSync(current)) : null;
  const target = pending.target;
  if (!target || target.schemaVersion !== 1 || !Number.isSafeInteger(target.revision) || target.revision < 1
      || target.contentHash !== hash(target.document?.markdown ?? '') || target.inputHash !== hash(JSON.stringify(documentInput(target.document)))) throw new Error('문서 복구 기록이 손상되었습니다.');
  if (actual !== pending.previousHash && actual !== target.contentHash) throw conflict('중단된 저장 이후 사용자 편집·삭제 충돌입니다. 원문을 보존했습니다.');
  const history = safePath(root, directory, 'history', `${target.revision}.json`);
  if (fs.existsSync(history)) {
    if (JSON.stringify(readJson(history)) !== JSON.stringify(target)) throw conflict('문서 수정 이력이 다릅니다. 원문을 보존했습니다.');
  } else atomicJson(history, target);
  if (actual !== target.contentHash) atomicWrite(current, target.document.markdown);
  const { document, ...metadata } = target;
  atomicJson(safePath(root, directory, 'metadata.json'), { ...metadata, ...document, markdown: undefined });
  fs.unlinkSync(journal);
}

export async function saveDocument(input, { root } = {}) {
  const document = documentInput(input);
  if (!root || !path.isAbsolute(root)) throw new Error('문서 저장소 절대 경로가 필요합니다.');
  return withLock(root, canonical => saveUnlocked(document, canonical));
}

function saveUnlocked(document, canonical, adopt = false) {
    initialize(canonical);
    const relative = path.join(document.collection, document.group, document.id);
    const catalog = safePath(canonical, '.sync', 'catalog', `${document.id}.json`);
    const entry = fs.existsSync(catalog) ? readJson(catalog) : null;
    if (entry && entry.relative !== relative) throw conflict('같은 ID의 문서 분류·위치를 바꿀 수 없습니다. 새 ID를 사용하세요.');
    const directory = safePath(canonical, relative);
    if (entry?.established && !fs.existsSync(safePath(canonical, relative, 'metadata.json'))) throw conflict('문서 폴더 또는 메타데이터가 삭제되었습니다. 자동 복구하지 않고 충돌을 보존했습니다.');
    if (!fs.existsSync(catalog)) {
      if (fs.existsSync(directory)) throw conflict('관리되지 않은 문서 위치가 이미 있습니다.');
      atomicJson(catalog, { schemaVersion: 1, id: document.id, relative });
    }
    fs.mkdirSync(safePath(canonical, relative, 'attachments'), { recursive: true });
    recover(canonical, relative);
    const metadataFile = safePath(canonical, relative, 'metadata.json');
    const file = safePath(canonical, relative, 'record.md');
    const previous = fs.existsSync(metadataFile) ? readJson(metadataFile) : null;
    const actual = fs.existsSync(file) ? hash(fs.readFileSync(file)) : null;
    if (!adopt && (previous ? actual !== previous.contentHash : actual !== null)) throw conflict('문서가 직접 편집·삭제되었습니다. 사용자 변경과 충돌하므로 보존했습니다.');
    const inputHash = hash(JSON.stringify(document));
    if (previous?.inputHash === inputHash) {
      if (!entry?.established) atomicJson(catalog, { schemaVersion: 1, id: document.id, relative, established: true });
      return { id: document.id, root: canonical, file, revision: previous.revision, unchanged: true };
    }
    if (previous?.manual && !adopt) throw conflict('수동 편집한 문서입니다. 자동 갱신 충돌을 보존했습니다. 직접 편집 후 adopt를 사용하세요.');
    // Retrying an older development capture must not roll back a newer local
    // document while the older Space publication is still pending.
    if (Number.isSafeInteger(previous?.source?.captureRevision) && Number.isSafeInteger(document.source?.captureRevision)
        && previous.source.repoPath === document.source.repoPath && previous.source.captureRevision > document.source.captureRevision) {
      return { id: document.id, root: canonical, file, revision: previous.revision, unchanged: true, stale: true };
    }
    const revision = (previous?.revision ?? 0) + 1;
    const target = { schemaVersion: 1, revision, contentHash: hash(document.markdown), inputHash, savedAt: new Date().toISOString(), manual: adopt || previous?.manual === true, document };
    atomicJson(safePath(canonical, relative, '.pending.json'), { previousHash: actual, target });
    recover(canonical, relative);
    if (!entry?.established) atomicJson(catalog, { schemaVersion: 1, id: document.id, relative, established: true });
    return { id: document.id, root: canonical, file, revision, unchanged: false };
}

export async function adoptDocument(id, { root } = {}) {
  if (!UUID.test(id) || !root || !path.isAbsolute(root)) throw new Error('문서 UUID와 저장소 절대 경로가 필요합니다.');
  return withLock(root, canonical => {
    const entry = readJson(safePath(canonical, '.sync', 'catalog', `${id}.json`));
    if (fs.existsSync(safePath(canonical, entry.relative, '.pending.json'))) throw conflict('중단된 저장 복구가 먼저 필요합니다. 원문과 복구 기록을 보존했습니다.');
    const metadata = readJson(safePath(canonical, entry.relative, 'metadata.json'));
    const markdown = fs.readFileSync(safePath(canonical, entry.relative, 'record.md'), 'utf8');
    return saveUnlocked(documentInput({ ...metadata, markdown }), canonical, true);
  });
}

export async function vaultStatus(root) {
  if (!fs.existsSync(root)) return { root, initialized: false, records: [] };
  return withLock(root, async canonical => {
    const marker = safePath(canonical, 'vault.json');
    if (!fs.existsSync(marker)) return { root: canonical, initialized: false, records: [] };
    const info = readJson(marker);
    if (info.schemaVersion !== 1) throw new Error('지원하지 않는 저장소 형식입니다.');
    const catalog = safePath(canonical, '.sync', 'catalog');
    const records = [];
    for (const name of fs.existsSync(catalog) ? fs.readdirSync(catalog) : []) {
      if (!UUID.test(name.replace(/\.json$/, ''))) continue;
      const entry = readJson(safePath(canonical, '.sync', 'catalog', name));
      const metadata = safePath(canonical, entry.relative, 'metadata.json');
      const file = safePath(canonical, entry.relative, 'record.md');
      const pending = fs.existsSync(safePath(canonical, entry.relative, '.pending.json'));
      const data = fs.existsSync(metadata) ? readJson(metadata) : {};
      const actual = fs.existsSync(file) ? hash(fs.readFileSync(file)) : null;
      records.push({ id: entry.id, title: data.title, collection: data.collection, group: data.group, revision: data.revision, file,
        state: pending ? 'recovery_pending' : !data.contentHash ? entry.established ? 'edited_or_deleted' : 'incomplete' : actual === data.contentHash ? 'saved_local' : 'edited_or_deleted' });
    }
    return { root: canonical, initialized: true, vaultId: info.vaultId, records };
  });
}

async function main(args) {
  const command = args.shift(), options = {};
  const names = { '--root': 'root', '--state-dir': 'stateDir', '--input': 'input', '--space': 'space', '--id': 'id' };
  while (args.length) {
    const key = args.shift(), value = args.shift();
    if (!names[key] || !value || value.startsWith('--') || Object.hasOwn(options, names[key])) throw new Error('명령 인자를 확인해 주세요.');
    options[names[key]] = value;
  }
  const stateDir = path.resolve(options.stateDir ?? DEFAULT_STATE);
  let result;
  if (command === 'init') {
    if (!options.root || options.input || options.space) throw new Error('init --root ABSOLUTE_PATH를 사용하세요.');
    result = await configureVault(stateDir, { root: options.root });
  } else if (command === 'config') {
    if (!['on', 'off'].includes(options.space) || options.root || options.input) throw new Error('config --space on|off를 사용하세요.');
    result = await configureVault(stateDir, { spaceEnabled: options.space === 'on' });
  } else if (command === 'import') {
    if (!options.input || options.space || options.root) throw new Error('import --input FILE를 사용하세요. 저장 경로는 init으로 선택합니다.');
    const settings = vaultPreferences(stateDir);
    result = await saveDocument(readJson(options.input), { root: settings.root });
  } else if (command === 'adopt') {
    if (!options.id || options.input || options.space || options.root) throw new Error('adopt --id UUID를 사용하세요.');
    result = await adoptDocument(options.id, { root: vaultPreferences(stateDir).root });
  } else if (command === 'status') {
    if (options.root || options.input || options.space) throw new Error('status에는 --state-dir만 지정할 수 있습니다.');
    const settings = vaultPreferences(stateDir);
    result = { ...settings, ...(await vaultStatus(settings.root)) };
  } else throw new Error('사용법: node scripts/document-vault.mjs init --root PATH | import --input FILE | adopt --id UUID | status | config --space on|off');
  console.log(JSON.stringify(result, null, 2));
}
if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main(process.argv.slice(2)).catch(error => { console.error(error.message); process.exitCode = 1; });
}
