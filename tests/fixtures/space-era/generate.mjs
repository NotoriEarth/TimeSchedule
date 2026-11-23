// Writes the state files in this folder. It ran once against the last version
// of scripts/project-records.mjs that still published to Space (commit f826227,
// app 0.31.1) and cannot run against later versions: they have no publisher.
// Kept to show where the fixtures came from. To regenerate, check out f826227
// and run: node tests/fixtures/space-era/generate.mjs
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { createHash } from 'node:crypto';
import { fileURLToPath } from 'node:url';
import { prepare, sync } from '../../../scripts/project-records.mjs';
import { gitFixture } from '../../helpers/record-git-fixture.mjs';

const here = path.dirname(fileURLToPath(import.meta.url));
const pageId = 'page_1234567890abcdef1234567890abcdef';
const published = job => ({ pageId, pageUrl: `https://chatgpt.com/space/${pageId}`, parentPageId: job.source.parentPageId,
  title: job.source.title, markdown: job.pageMarkdown, indexVerified: true });
const offline = async () => { throw Object.assign(new Error('Space에 연결하지 못했습니다.'), { publicationNotStarted: true }); };
const NEWER = '\n\n후속 수정도 완료했다.';

// Like the app: a received revision keeps its content, other content under it
// is refused, and the diary shows the highest revision it has taken.
function appStub() {
  const app = { received: {}, inbox: {}, revision: 0, detail: null, spaceUrl: null };
  app.importer = async record => {
    const content = JSON.stringify(record);
    if (app.received[record.revision] && JSON.stringify(app.received[record.revision]) !== content) throw new Error('같은 revision에 다른 내용이 이미 수신되었습니다.');
    app.received[record.revision] = record;
    if (record.revision <= app.revision) return { outcome: 'stale', workId: record.workId, revision: record.revision, entryId: 'entry-1' };
    Object.assign(app, { revision: record.revision, detail: record.detail, spaceUrl: record.spaceUrl });
    return { outcome: 'saved', workId: record.workId, revision: record.revision, entryId: 'entry-1' };
  };
  // The app took the payload into its inbox and has not applied it.
  app.queue = async record => {
    app.inbox[record.revision] = record;
    return { outcome: 'queued', workId: record.workId, revision: record.revision, entryId: null };
  };
  return app;
}

const scenarios = {
  // Space published the page and the diary got the link.
  'complete': async (f, input, app) => {
    await prepare(input, f);
    await sync({ ...f, spaceEnabled: true, publisher: async job => published(job), importer: app.importer });
  },
  'local-complete': async (f, input, app) => {
    await prepare(input, f);
    await sync({ ...f, spaceEnabled: false, importer: app.importer });
  },
  // A human edited the Space page: the publisher refused to overwrite it.
  'space-conflict': async (f, input, app) => {
    await prepare(input, f);
    await sync({ ...f, spaceEnabled: true, importer: app.importer, publisher: async job => {
      throw Object.assign(new Error('Space의 사용자 편집을 보존했습니다.'), { conflict: true, space: { ...published(job), markdown: '사용자가 고친 본문', indexVerified: false } });
    } });
  },
  'waiting-space': async (f, input, app) => {
    await prepare(input, f);
    await sync({ ...f, spaceEnabled: true, publisher: offline, importer: app.importer });
  },
  // The create may have succeeded; the answer was lost.
  'publishing-uncertain': async (f, input, app) => {
    await prepare(input, f);
    await sync({ ...f, spaceEnabled: true, importer: app.importer, publisher: async () => { throw new Error('응답을 받지 못했습니다.'); } });
  },
  // The page is verified; sending the link to the diary failed.
  'space-saved': async (f, input, app) => {
    await prepare(input, f);
    await sync({ ...f, spaceEnabled: true, publisher: async job => published(job),
      importer: async record => { if (record.deliveryState === 'synced') throw new Error('EXE를 실행하지 못했습니다.'); return app.importer(record); } });
  },
  // The app took the first payload into its inbox and has not applied it.
  'queued-first': async (f, input, app) => {
    await prepare(input, f);
    await sync({ ...f, spaceEnabled: true, publisher: async job => published(job), importer: app.queue });
  },
  // The app applied the first payload and holds the linked one in its inbox.
  'queued-linked': async (f, input, app) => {
    await prepare(input, f);
    await sync({ ...f, spaceEnabled: true, publisher: async job => published(job),
      importer: async record => record.deliveryState === 'synced' ? app.queue(record) : app.importer(record) });
  },
  // Two captures of one work: the second waits for the first one's page.
  'waiting-prior': async (f, input, app) => {
    await prepare(input, f);
    await sync({ ...f, spaceEnabled: true, publisher: offline, importer: app.importer });
    await prepare({ ...input, detailedMarkdown: input.detailedMarkdown + NEWER }, f);
    await sync({ ...f, spaceEnabled: true, publisher: offline, importer: app.importer });
  },
  'prepared-unsent': async (f, input) => {
    await prepare(input, f);
  },
  // The order behind the diary rollback (docs/record-delivery-redesign.md):
  // Git could not check the first capture while Space was on, so only the
  // second reached the diary.
  'overtaken-capture': async (f, input, app) => {
    await prepare(input, f);
    await prepare({ ...input, detailedMarkdown: input.detailedMarkdown + NEWER }, f);
    const earlierFile = path.join(f.stateDir, input.workId, 'pending-1.json');
    const earlier = JSON.parse(fs.readFileSync(earlierFile, 'utf8'));
    const realRepo = earlier.source.repoPath;
    earlier.source.repoPath = path.join(f.dir, 'briefly-missing');
    fs.writeFileSync(earlierFile, JSON.stringify(earlier, null, 2) + '\n');
    await sync({ ...f, spaceEnabled: true, publisher: async job => published(job), importer: app.importer });
    assert.match(app.detail, /후속 수정/);
    const resumed = JSON.parse(fs.readFileSync(earlierFile, 'utf8'));
    resumed.source.repoPath = realRepo;
    fs.writeFileSync(earlierFile, JSON.stringify(resumed, null, 2) + '\n');
  },
};

// The same order, one step further: Space is turned off and a sync runs while
// Git cannot check either capture. That version sends nothing and gives both
// captures new numbers. The first capture's new number is above the number
// the diary shows the second capture under.
scenarios['renumbered-overtaken'] = async (f, input, app) => {
  await scenarios['overtaken-capture'](f, input, app);
  const dotGit = path.join(f.projectRoot, '.git'), away = path.join(f.projectRoot, '.git-away');
  fs.renameSync(dotGit, away);
  try { await sync({ ...f, spaceEnabled: false, retryNow: true, importer: async () => { throw new Error('nothing may be sent while Git fails'); } }); }
  finally { fs.renameSync(away, dotGit); }
  const [first, second] = ['pending-1.json', 'state.json'].map(name => JSON.parse(fs.readFileSync(path.join(f.stateDir, input.workId, name), 'utf8')));
  assert.ok(first.localRevision > app.revision && first.localRevision < second.localRevision && !first.localSaved && !second.localSaved);
};

const mapStrings = (value, change) => typeof value === 'string' ? change(value)
  : Array.isArray(value) ? value.map(item => mapStrings(item, change))
  : value && typeof value === 'object' ? Object.fromEntries(Object.entries(value).map(([key, item]) => [key, mapStrings(item, change)]))
  : value;

for (const [name, run] of Object.entries(scenarios)) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'timeschedule-space-era-'));
  try {
    const git = gitFixture(dir);
    const f = { dir, stateDir: path.join(dir, 'state'), projectRoot: git.repo, vaultRoot: path.join(dir, 'vault') };
    const input = { workId: '11111111-1111-4111-8111-111111111111', projectTitle: 'TimeSchedule', repoPath: git.repo, gitChanges: git.gitChanges,
      date: '2026-11-20', kind: 'development', title: 'Space 시절 기록', status: 'verification_pending',
      detailedMarkdown: '원인을 확인했다. 실제 입력은 확인하지 않았다.', diarySummary: '원인을 확인했다. 실제 입력은 확인하지 않았다.' };
    const app = appStub();
    await run(f, input, app);
    const workDir = path.join(f.stateDir, input.workId), files = {};
    for (const file of fs.readdirSync(workDir).sort()) {
      if (!/^(state|pending-\d+|space-state|\d+)\.json$/.test(file)) continue;
      const value = JSON.parse(fs.readFileSync(path.join(workDir, file), 'utf8'));
      // What the reader of these fixtures recomputes after it puts its own
      // repository in: the hash this version stored is that of its source.
      if (value.contentHash) assert.equal(value.contentHash, createHash('sha256').update(JSON.stringify(value.source)).digest('hex'));
      files[file] = value;
    }
    const placeholders = [[git.repo, '<REPO>'], [fs.realpathSync.native(git.repo), '<REPO>'], [dir, '<DIR>'], [fs.realpathSync.native(dir), '<DIR>'],
      [git.gitChanges.base, '<BASE>'], [git.gitChanges.head, '<HEAD>']];
    const portable = value => mapStrings(value, text => placeholders.reduce((out, [from, to]) => out.split(from).join(to), text));
    const { importer, queue, ...diary } = app;
    fs.writeFileSync(path.join(here, `${name}.json`), JSON.stringify(portable({ files, app: diary }), null, 2) + '\n');
    console.log(name, Object.entries(files).filter(([file]) => /^(state|pending)/.test(file)).map(([file, job]) => `${file}:${job.phase}`).join(' '));
  } finally { fs.rmSync(dir, { recursive: true, force: true }); }
}
