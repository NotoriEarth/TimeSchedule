import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { randomUUID } from 'node:crypto';
import { spawn } from 'node:child_process';
import { pathToFileURL } from 'node:url';
import { prepare, sync, status } from '../scripts/project-records.mjs';
import { verifyPageEvents, createdPageObservation } from '../scripts/project-records-codex.mjs';

const parentPageId = 'page_6ae1b18a44f08191ad2a86276972476c';
const pageId = 'page_1234567890abcdef1234567890abcdef';
const pageUrl = `https://chatgpt.com/space/${pageId}`;
function source() {
  return {
    workId: randomUUID(), projectTitle: 'TimeSchedule', repoPath: process.cwd(), date: '2026-10-31',
    kind: 'troubleshooting', title: '검증 결과와 한계', status: 'verification_pending',
    detailedMarkdown: 'RAM은 감소했지만 유휴 CPU는 증가했다. 실제 입력 성능은 측정하지 않았다.\n\n# 원인\n\nCPU 증가 원인은 아직 확인하지 않았다.',
    diarySummary: 'RAM은 감소했지만 유휴 CPU는 증가했다. 실제 입력 성능은 측정하지 않았다.',
  };
}
function stateDir(t) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'timeschedule-record-delivery-'));
  t.after(() => {
    assert.ok(dir.startsWith(path.join(os.tmpdir(), 'timeschedule-record-delivery-')));
    fs.rmSync(dir, { recursive: true, force: true });
  });
  return dir;
}
function publisherReceipt(job) {
  return { pageId, pageUrl, parentPageId, title: job.source.title, markdown: job.pageMarkdown, indexVerified: true };
}

const saved = r => ({ outcome: 'saved', workId: r.workId, revision: r.revision, entryId: 'entry-1' });
test('no-PR record saves full diary text before Space and later links the same record', async t => {
  const dir = stateDir(t); const input = source();
  const a = await prepare(input, { stateDir: dir });
  assert.equal(a.revision, 1);
  const order = [];
  const options = { stateDir: dir,
    publisher: async job => { order.push('space'); return publisherReceipt(job); },
    importer: async record => { order.push(record.deliveryState); assert.equal(record.spaceUrl, record.deliveryState === 'pending' ? '' : pageUrl); assert.equal(record.detail, input.detailedMarkdown); return saved(record); },
  };
  await sync(options);
  assert.deepEqual(order, ['pending', 'space', 'synced']);
  assert.equal(status({ stateDir: dir })[0].phase, 'complete');
  assert.equal((await prepare(input, { stateDir: dir })).revision, 1);
  await sync(options);
  assert.deepEqual(order, ['pending', 'space', 'synced']);
});

test('Space success survives diary failure without republishing', async t => {
  const dir = stateDir(t); await prepare(source(), { stateDir: dir });
  let publications = 0;
  const publisher = async job => { publications++; return publisherReceipt(job); };
  await sync({ stateDir: dir, publisher, importer: async r => { if(r.deliveryState === 'pending') return saved(r); throw new Error('EXE unavailable'); } });
  assert.equal(status({ stateDir: dir })[0].phase, 'space_saved');
  await sync({ stateDir: dir, retryNow: true, publisher, importer: async r => saved(r) });
  assert.equal(publications, 1);
  assert.equal(status({ stateDir: dir })[0].phase, 'complete');
});

test('queued is not complete and user-edit conflict is retained', async t => {
  const dir = stateDir(t); await prepare(source(), { stateDir: dir });
  const publisher = async job => publisherReceipt(job);
  await sync({ stateDir: dir, publisher, importer: async r => ({ outcome: 'queued', workId: r.workId, revision: r.revision, entryId: null }) });
  assert.equal(status({ stateDir: dir })[0].phase, 'diary_queued');
  await sync({ stateDir: dir, retryNow: true, publisher: async () => { throw new Error('must not republish'); }, importer: async r => ({ outcome: 'conflict', workId: r.workId, revision: r.revision, message: 'user edited' }) });
  assert.equal(status({ stateDir: dir })[0].phase, 'conflict');
});

test('uncertain publication is reconciled without permission to create again', async t => {
  const dir = stateDir(t); const input = source(); await prepare(input, { stateDir: dir });
  await sync({ stateDir: dir, publisher: async () => { throw new Error('connection lost after create'); }, importer: async r => saved(r) });
  assert.equal(status({ stateDir: dir })[0].phase, 'publishing');
  await sync({ stateDir: dir, retryNow: true, publisher: async (job, { recoverOnly }) => { assert.equal(recoverOnly, true); return publisherReceipt(job); }, importer: async r => saved(r) });
  assert.equal(status({ stateDir: dir })[0].phase, 'complete');
});

test('summary must retain a whole source paragraph including limitations', async t => {
  const dir = stateDir(t); const input = source();
  await assert.rejects(prepare({ ...input, diarySummary: 'RAM은 감소했다. 모든 성능이 향상되었다.' }, { stateDir: dir }), /원문|문단/);
  const { diarySummary, ...withoutSummary } = input;
  await prepare(withoutSummary, { stateDir: dir });
  await sync({ stateDir: dir, publisher: async job => { assert.equal(job.source.diarySummary, diarySummary); return publisherReceipt(job); }, importer: async r => ({ outcome: 'saved', workId: r.workId, revision: r.revision, entryId: 'entry-1' }) });
});

test('wrong publication body or URL never reaches the diary', async t => {
  for (const change of [{ pageUrl: 'https://evil.test/page' }, { markdown: '모든 성능이 향상되었다.' }]) {
    const dir = stateDir(t); await prepare(source(), { stateDir: dir }); let imports = 0;
    await sync({ stateDir: dir, publisher: async job => ({ ...publisherReceipt(job), ...change }), importer: async r => { if(r.spaceUrl) imports++; return saved(r); } });
    assert.equal(imports, 0);
    assert.notEqual(status({ stateDir: dir })[0].phase, 'complete');
  }
});

function readEvent(id, title, markdown, parent = parentPageId) {
  return { type: 'item.completed', item: { type: 'mcp_tool_call', server: 'codex_apps', tool: 'chatgpt_space.read_page', arguments: { page_id: id }, status: 'completed', error: null,
    result: { content: [], structured_content: { metadata: { page_id: id, title, url: `https://chatgpt.com/space/${id}`, parent: { page_id: parent }, access: { can_read: true, can_write: true }, deleted_at: null }, content: { page_id: id, blocks: markdown.split('\n\n').map((text, i) => ({ id: `b${i}`, kind: 'markdown', markdown: text, hash: `hash${i}`, metadata: {} })) } } },
  } };
}

test('publication requires actual read_page tool receipts, not model self-report', () => {
  const job = { source: { ...source(), parentPageId }, pageMarkdown: '정확한 원문\n\n검증 범위는 제한적이다.', space: null };
  assert.throws(() => verifyPageEvents([{ type: 'item.completed', item: { type: 'agent_message', text: JSON.stringify({ pageId, pageUrl, markdown: job.pageMarkdown }) } }], job), /읽기|read|검증/);
  const events = [readEvent(pageId, job.source.title, job.pageMarkdown), readEvent(parentPageId, '포트폴리오', `[검증 결과와 한계](${pageUrl})`, 'page_abcdefabcdefabcdefabcdefabcdefab')];
  assert.equal(verifyPageEvents(events, job).pageUrl, pageUrl);
  assert.throws(() => verifyPageEvents([...events, readEvent(pageId, job.source.title, '나중에 사용자가 편집함')], job), /읽기|read|검증/);
  const selected = structuredClone(events);
  selected[0].item.result.structured_content.selection = { whole_page_complete: false };
  assert.throws(() => verifyPageEvents(selected, job), /읽기|read|검증/);
  assert.throws(() => verifyPageEvents([readEvent(pageId, job.source.title, '다른 내용')], job), /읽기|read|검증/);
});

test('a changed word is rejected but actual create receipts retain the page for repair', () => {
  const job = { source: { ...source(), parentPageId }, pageMarkdown: '확인했다. 미검증 사항은 남아 있다.', space: null };
  const changed = readEvent(pageId, job.source.title, '확인함. 미검증 사항은 남아 있다.');
  const created = structuredClone(changed); created.item.tool = 'chatgpt_space.create_page';
  const events = [created, changed];
  assert.throws(() => verifyPageEvents(events, job), /검증/);
  assert.equal(createdPageObservation(events, job).pageId, pageId);
  assert.equal(createdPageObservation(events, job).markdown, '확인함. 미검증 사항은 남아 있다.');
  assert.equal(createdPageObservation([changed], job), null);
});

test('Space list-item blocks verify against the original compact list without rewriting it', () => {
  for (const list of ['- 검증 136개 통과\n- [PR #8](https://github.com/NotoriEarth/TimeSchedule/pull/8)', '1. 원문 저장\n2. 링크 연결']) {
    const job = { source: { ...source(), parentPageId }, pageMarkdown: `실제 입력은 미확인.\n\n## 관련 자료\n\n${list}`, space: null };
    const child = readEvent(pageId, job.source.title, job.pageMarkdown);
    const blocks = child.item.result.structured_content.content.blocks;
    const listBlock = blocks.pop();
    blocks.push(...listBlock.markdown.split('\n').map((markdown, i) => ({ ...listBlock, id: `list${i}`, markdown })));
    // Real Pages readbacks return each list item as a separate markdown block.
    const events = [child, readEvent(parentPageId, '포트폴리오', `[기록](${pageUrl})`)];
    const verified = verifyPageEvents(events, job);
    assert.equal(verified.markdown, job.pageMarkdown);
    job.space = { ...verified, markdown: blocks.map(b => b.markdown).join('\n\n') };
    assert.equal(verifyPageEvents(events, job).pageId, pageId);
    // A subsequent revision must recognize the previously verified snapshot
    // even before the page receives the new text, without accepting it as new.
    const next = { ...job, space: verified, pageMarkdown: `${job.pageMarkdown}\n\n후속 검증: 완료.` };
    const created = structuredClone(child); created.item.tool = 'chatgpt_space.create_page';
    assert.equal(createdPageObservation([created, child], next).markdown, verified.markdown);
    assert.throws(() => verifyPageEvents(events, next), /검증/);
  }
});

test('Space block reconciliation rejects changed facts, links, order and code whitespace', () => {
  const original = '- 검증 136개 통과\n- 실제 입력은 미확인.\n- [PR #8](https://github.com/NotoriEarth/TimeSchedule/pull/8)';
  const job = { source: { ...source(), parentPageId }, pageMarkdown: original, space: null };
  const parent = readEvent(parentPageId, '포트폴리오', `[기록](${pageUrl})`);
  for (const changed of [original.replace('136', '137'), original.replace('미확인', '확인'), original.replace('/pull/8', '/pull/7'), original.split('\n').reverse().join('\n')]) {
    const child = readEvent(pageId, job.source.title, changed.replaceAll('\n', '\n\n'));
    assert.throws(() => verifyPageEvents([child, parent], job), /검증/);
  }
  for (const [expected, actual] of [
    ['첫 줄\n둘째 줄', '첫 줄\n\n둘째 줄'],
    ['```text\n- 원문\n- 공백 보존\n```', '```text\n- 원문\n\n- 공백 보존\n```'],
    ['- 첫 항목\n  내부 줄\n- 다음 항목', '- 첫 항목\n\n  내부 줄\n\n- 다음 항목'],
  ]) {
    assert.throws(() => verifyPageEvents([readEvent(pageId, job.source.title, actual), parent], { ...job, pageMarkdown: expected }), /검증/);
  }
});

test('a stale lock observer cannot enter while another process owns delivery', async t => {
  const dir = stateDir(t); await prepare(source(), { stateDir: dir });
  const moduleUrl = pathToFileURL(path.resolve('scripts/project-records.mjs')).href;
  const child = spawn(process.execPath, ['--input-type=module', '-e', `
    import {sync} from ${JSON.stringify(moduleUrl)};
    try { await sync({stateDir:process.argv[1],importer:async r=>({outcome:'saved',workId:r.workId,revision:r.revision,entryId:'entry-1'}),publisher:async()=>{
      process.send('entered'); await new Promise(r=>process.once('message',r)); throw Error('test finished');
    }}); } catch {} finally { process.disconnect(); }
  `, dir], { windowsHide: true, stdio: ['ignore', 'ignore', 'pipe', 'ipc'] });
  t.after(() => child.kill());
  await new Promise((resolve, reject) => { child.once('message', resolve); child.once('error', reject); child.once('exit', () => reject(Error('holder exited early'))); });
  const originalRead = fs.readFileSync;
  // Reproduce the TOCTOU interleaving: this caller observed a dead owner's
  // marker just before the child acquired the live lock.
  fs.readFileSync = function(file, ...args) {
    if (String(file) === path.join(dir, '.sync.lock')) return JSON.stringify({pid:2147483647});
    return originalRead.call(this, file, ...args);
  };
  try {
    await assert.rejects(sync({stateDir:dir, publisher:async job=>publisherReceipt(job), importer:async r=>({outcome:'saved',workId:r.workId,revision:r.revision,entryId:'unexpected'})}), /다른 기록 작업/);
  } finally { fs.readFileSync = originalRead; child.send('finish'); }
  await new Promise(resolve => child.once('exit', resolve));
});
