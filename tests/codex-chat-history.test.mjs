import assert from 'node:assert/strict';
import { spawn, spawnSync } from 'node:child_process';
import { randomUUID } from 'node:crypto';
import { once } from 'node:events';
import { existsSync, readdirSync, readFileSync } from 'node:fs';
import { appendFile, mkdtemp, mkdir, readFile, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import path from 'node:path';
import test from 'node:test';
import { setTimeout as delay } from 'node:timers/promises';
import { zstdDecompressSync } from 'node:zlib';

const exe = process.env.TIMESCHEDULE_RECORD_TEST_EXE;

test('native app adds a desktop chat at startup with MCP off, keeps its original, and fully rereads it once', {skip: !exe}, async t => {
  const { DatabaseSync } = await import('node:sqlite');
  const root = await mkdtemp(path.join(tmpdir(), 'timeschedule-codex-history-'));
  t.after(() => rm(root, {recursive: true, force: true}));
  const data = path.join(root, 'data'), home = path.join(root, 'codex'), user = path.join(root, 'user');
  const dateDir = path.join(home, 'sessions', '2026', '11', '19');
  await mkdir(dateDir, {recursive: true});
  // The app finds the default document store and the Claude folder under the user's
  // home. A home of our own keeps this test out of the real ones.
  const vault = path.join(user, 'Documents', 'TimeScheduleData');
  await mkdir(vault, {recursive: true});
  await writeFile(path.join(vault, 'vault.json'), JSON.stringify({schemaVersion: 1}));
  const env = {...process.env, TIMESCHEDULE_DATA_DIR: data, CODEX_HOME: home, USERPROFILE: user,
    CLAUDE_CONFIG_DIR: path.join(user, '.claude'), TIMESCHEDULE_MCP_DISABLED: '1'};
  function record(...args) {
    const run = spawnSync(exe, ['record', ...args], {env, windowsHide: true, encoding: 'utf8', timeout: 15000});
    assert.equal(run.status, 0, run.stderr || run.stdout);
    return JSON.parse(run.stdout);
  }
  record('drain');
  const project = randomUUID(), session = randomUUID();
  const dbPath = path.join(data, 'schedule.db');
  const db = new DatabaseSync(dbPath);
  db.prepare('INSERT INTO projects(id,title,description,repo_path,updated_at) VALUES(?,?,?,?,?)')
    .run(project, '샘플', '', root, '2026-11-19');
  db.close();
  const powerPath = path.join(data, 'mcp-power.json');
  await writeFile(powerPath, JSON.stringify({enabled:false,generation:randomUUID()}));
  const rows = [
    {type:'session_meta', payload:{id:session,cwd:root,originator:'Codex Desktop',timestamp:'2026-11-19T10:10:18Z'}},
    {type:'event_msg',payload:{type:'item_completed',thread_id:session,item:{type:'UserMessage',id:'first',content:[{text:'파란 우산'}]}}},
    {type:'event_msg',payload:{type:'item_completed',thread_id:session,item:{type:'UserMessage',id:'second',content:[{text:'초록 등대로 수정'}]}}},
  ];
  const rollout = path.join(dateDir, 'rollout-test-'+session+'.jsonl');
  await writeFile(rollout, rows.map(r=>JSON.stringify(r)).join('\n')+'\n');
  record('configure-codex-chats','--project',project,'--enabled','true');
  function entries() {
    const reader = new DatabaseSync(dbPath,{readOnly:true});
    try {
      return reader.prepare('SELECT id,body,updated_at FROM diary_entries WHERE project_id=? AND source_ref=?')
        .all(project,'codex-chat:'+project+':'+session);
    } finally { reader.close(); }
  }
  // The whole session file the app kept in the document store, decoded.
  function kept() {
    const originals = path.join(vault, 'originals');
    if (!existsSync(originals)) return null;
    const groups = readdirSync(originals);
    assert.equal(groups.length, 1);
    const file = path.join(originals, groups[0], 'codex', '2026', '11', '19', path.basename(rollout)+'.zst');
    return existsSync(file) ? zstdDecompressSync(readFileSync(file)).toString('utf8') : null;
  }
  // Starts the app, waits until its startup scan shows `text`, and stops it again.
  async function startAndCapture(text) {
    const app = spawn(exe, [], {env,windowsHide:true,stdio:'ignore'});
    const closed = once(app,'close');
    try {
      const deadline = Date.now() + 30000;
      do {
        const rows = entries();
        if (rows.length === 1 && rows[0].body.includes(text)) return rows;
        assert.equal(app.exitCode, null, 'the GUI exited before capture');
        await delay(250);
      } while (Date.now() < deadline);
      assert.fail('the GUI did not capture ' + text);
    } finally {
      app.kill(); // Only our isolated fixture, never an existing user process.
      await closed;
    }
  }
  const first = await startAndCapture('초록 등대');
  assert.equal(kept(), await readFile(rollout, 'utf8'), 'the app keeps the whole session file');
  // The app reads chat files when it starts, when the projects screen opens and once
  // an hour. A reply written meanwhile shows after the next start, in the same entry.
  const followup = {type:'event_msg',payload:{type:'item_completed',thread_id:session,item:{
    type:'AgentMessage',id:'reply',content:[{text:'다음 수집에서 보이는 새 답변'}],
  }}};
  await appendFile(rollout, JSON.stringify(followup)+'\n');
  const updated = await startAndCapture('다음 수집에서 보이는 새 답변');
  assert.equal(updated[0].id, first[0].id);
  assert.equal(kept(), await readFile(rollout, 'utf8'), 'the kept file follows the grown session');
  const replay=record('scan-codex-chats');
  assert.deepEqual(replay.report,{saved:0,unchanged:1,failed:0});
  assert.deepEqual(entries(),updated);
});
