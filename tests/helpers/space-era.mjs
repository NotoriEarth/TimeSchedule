import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { createHash } from 'node:crypto';
import { fileURLToPath } from 'node:url';
import { gitFixture } from './record-git-fixture.mjs';

// State files written by the last version that published to Space (see
// tests/fixtures/space-era/generate.mjs), and the diary as the app held it.
const fixtures = path.join(path.dirname(fileURLToPath(import.meta.url)), '..', 'fixtures', 'space-era');
export const WORK_ID = '11111111-1111-4111-8111-111111111111';
export const NEWER = '\n\n후속 수정도 완료했다.';

const mapStrings = (value, change) => typeof value === 'string' ? change(value)
  : Array.isArray(value) ? value.map(item => mapStrings(item, change))
  : value && typeof value === 'object' ? Object.fromEntries(Object.entries(value).map(([key, item]) => [key, mapStrings(item, change)]))
  : value;

export function setup(t) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'timeschedule-space-era-'));
  t.after(() => fs.rmSync(dir, { recursive: true, force: true }));
  const fixture = gitFixture(dir);
  return { ...fixture, dir, stateDir: path.join(dir, 'state'), projectRoot: fixture.repo, vaultRoot: path.join(dir, 'vault') };
}

// The input the fixtures' first capture was prepared from.
export const input = f => ({ workId: WORK_ID, projectTitle: 'TimeSchedule', repoPath: f.repo, gitChanges: f.gitChanges,
  date: '2026-11-20', kind: 'development', title: 'Space 시절 기록', status: 'verification_pending',
  detailedMarkdown: '원인을 확인했다. 실제 입력은 확인하지 않았다.', diarySummary: '원인을 확인했다. 실제 입력은 확인하지 않았다.' });

// Puts one fixture's state files into f.stateDir, pointing at this test's
// repository, and returns a stand-in for the app that holds what the app held.
export function load(name, f) {
  const saved = JSON.parse(fs.readFileSync(path.join(fixtures, `${name}.json`), 'utf8'));
  const values = { '<REPO>': fs.realpathSync.native(f.repo), '<DIR>': f.dir, '<BASE>': f.gitChanges.base, '<HEAD>': f.gitChanges.head };
  const { files, app: held } = mapStrings(saved, text => Object.entries(values).reduce((out, [from, to]) => out.split(from).join(to), text));
  const workDir = path.join(f.stateDir, WORK_ID);
  fs.mkdirSync(workDir, { recursive: true });
  for (const [file, value] of Object.entries(files)) {
    // That version stored the hash of the source as it wrote it.
    if (value.contentHash) value.contentHash = createHash('sha256').update(JSON.stringify(value.source)).digest('hex');
    fs.writeFileSync(path.join(workDir, file), JSON.stringify(value, null, 2) + '\n');
  }
  return appHolding(held);
}

// Like the app (apply_project_record and the inbox): a number it holds keeps
// its content and other content under it is refused; the same content under a
// number it applied gets the answer it gave then; a lower number than the one
// shown is stale; an entry keeps its link when a record carries none.
// `shown` lists what the diary showed after the fixture was loaded, in order.
// Every capture in these tests extends the text of the one before, so
// `rollbacks` lists each time the diary went back to an earlier capture.
export function appHolding(held = { received: {}, inbox: {}, revision: 0, detail: null, spaceUrl: null }) {
  const app = { ...held, shown: [], sent: [], rollbacks: [], answers: {} };
  for (const record of Object.values(app.received)) app.answers[record.revision] = { outcome: 'saved', workId: record.workId, revision: record.revision, entryId: 'entry-1' };
  app.importer = async record => {
    app.sent.push(record);
    const holds = app.received[record.revision] ?? app.inbox[record.revision];
    if (holds && JSON.stringify(holds) !== JSON.stringify(record)) throw new Error('같은 revision에 다른 내용이 이미 수신되었습니다.');
    if (app.answers[record.revision]) return app.answers[record.revision];
    app.received[record.revision] = record;
    const answer = outcome => app.answers[record.revision] = { outcome, workId: record.workId, revision: record.revision, entryId: 'entry-1' };
    if (record.revision < app.revision) return answer('stale');
    if (record.revision === app.revision) return answer('unchanged');
    if (app.detail && app.detail !== record.detail && app.detail.startsWith(record.detail)) app.rollbacks.push({ from: app.revision, to: record.revision });
    Object.assign(app, { revision: record.revision, detail: record.detail, spaceUrl: record.spaceUrl || app.spaceUrl });
    app.shown.push(record.detail);
    return answer('saved');
  };
  return app;
}

export const jobFiles = f => fs.readdirSync(path.join(f.stateDir, WORK_ID)).filter(file => /^(state|pending-\d+)\.json$/.test(file)).sort();
export const readJob = (f, file) => JSON.parse(fs.readFileSync(path.join(f.stateDir, WORK_ID, file), 'utf8'));
export const snapshot = f => Object.fromEntries(fs.readdirSync(path.join(f.stateDir, WORK_ID)).sort()
  .map(file => [file, fs.readFileSync(path.join(f.stateDir, WORK_ID, file), 'utf8')]));
