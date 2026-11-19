import assert from 'node:assert/strict';
import { spawn, spawnSync } from 'node:child_process';
import { randomUUID } from 'node:crypto';
import { once } from 'node:events';
import { appendFile, mkdtemp, mkdir, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import path from 'node:path';
import test from 'node:test';
import { setTimeout as delay } from 'node:timers/promises';

const exe = process.env.TIMESCHEDULE_RECORD_TEST_EXE;

test('native app automatically adds a desktop chat with MCP off and fully rereads it once', {skip: !exe}, async t => {
  const { DatabaseSync } = await import('node:sqlite');
  const root = await mkdtemp(path.join(tmpdir(), 'timeschedule-codex-history-'));
  t.after(() => rm(root, {recursive: true, force: true}));
  const data = path.join(root, 'data'), home = path.join(root, 'codex');
  const dateDir = path.join(home, 'sessions', '2026', '11', '19');
  await mkdir(dateDir, {recursive: true});
  const env = {...process.env, TIMESCHEDULE_DATA_DIR: data, CODEX_HOME: home, TIMESCHEDULE_MCP_DISABLED: '1'};
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
  const app = spawn(exe, [], {env,windowsHide:true,stdio:'ignore'});
  const closed = once(app,'close');
  async function waitForText(text, timeout) {
    const deadline = Date.now() + timeout;
    do {
      const rows = entries();
      if (rows.length === 1 && rows[0].body.includes(text)) return rows;
      assert.equal(app.exitCode, null, 'the GUI exited before capture');
      await delay(250);
    } while (Date.now() < deadline);
    assert.fail('the GUI did not capture ' + text);
  }
  let updated;
  try {
    const first = await waitForText('초록 등대', 30000);
    const followup = {type:'event_msg',payload:{type:'item_completed',thread_id:session,item:{
      type:'AgentMessage',id:'reply',content:[{text:'앱 실행 중 새 답변'}],
    }}};
    await appendFile(rollout, JSON.stringify(followup)+'\n');
    updated = await waitForText('앱 실행 중 새 답변', 100000);
    assert.equal(updated[0].id, first[0].id);
  } finally {
    app.kill(); // Only our isolated fixture, never an existing user process.
    await closed;
  }
  const replay=record('scan-codex-chats');
  assert.deepEqual(replay.report,{saved:0,unchanged:1,failed:0});
  assert.deepEqual(entries(),updated);
});
