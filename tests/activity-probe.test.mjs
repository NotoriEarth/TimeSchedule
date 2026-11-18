import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { existsSync, mkdtempSync, mkdirSync, readFileSync, readdirSync, rmSync, writeFileSync } from 'node:fs';
import { homedir, tmpdir } from 'node:os';
import path from 'node:path';
import test from 'node:test';
import { fileURLToPath } from 'node:url';

const root = path.dirname(path.dirname(fileURLToPath(import.meta.url)));
const PROBE = path.join(root, 'scripts', 'activity-adapters', 'probe-hook.mjs');
const SANITIZE = path.join(root, 'scripts', 'activity-adapters', 'sanitize-fixture.mjs');

function scratch(t) {
  const dir = mkdtempSync(path.join(tmpdir(), 'ts-probe-'));
  t.after(() => rmSync(dir, { recursive: true, force: true }));
  return dir;
}

// The suite may itself run inside Claude Code or Codex; their variables must not leak into the probe.
const baseEnv = Object.fromEntries(Object.entries(process.env)
  .filter(([name]) => !/^(CLAUDE|CODEX|TIMESCHEDULE_ORIGIN)/i.test(name)));

function run(args, input, env = {}) {
  return spawnSync(process.execPath, [PROBE, ...args], {
    input, encoding: 'utf8', windowsHide: true, env: { ...baseEnv, ...env },
  });
}

function onlyRecord(dir) {
  const files = readdirSync(dir);
  assert.equal(files.length, 1, `expected one record, found: ${files.join(', ')}`);
  const text = readFileSync(path.join(dir, files[0]), 'utf8');
  return { text, record: JSON.parse(text), file: files[0] };
}

test('probe_writes_one_record_and_prints_nothing', t => {
  const dir = scratch(t);
  const r = run(['--source', 'claude-code', '--out', dir, '--tool-version', '2.1.325'],
    '{"hook_event_name":"Stop","session_id":"s1","cwd":"C:\\\\lab"}');
  assert.equal(r.status, 0, r.stderr);
  assert.equal(r.stdout, '');
  const { record, file } = onlyRecord(dir);
  assert.equal(record.capture_schema, 1);
  assert.equal(record.source, 'claude-code');
  assert.equal(record.tool_version, '2.1.325');
  assert.equal(record.origin, 'tool');
  assert.equal(record.payload_error, null);
  assert.deepEqual(record.payload, { hook_event_name: 'Stop', session_id: 's1', cwd: 'C:\\lab' });
  assert.match(record.capture_id, /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/);
  assert.match(record.captured_at, /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{3}Z$/);
  assert.equal(file, `${record.captured_at.replace(/\D/g, '')}-${record.capture_id}.json`);
  assert.equal(typeof record.probe_elapsed_ms, 'number');
  assert.ok(record.payload_bytes > 0);
});

test('probe_marks_invalid_json', t => {
  const dir = scratch(t);
  const r = run(['--source', 'codex', '--out', dir], '{"a":');
  assert.equal(r.status, 0, r.stderr);
  assert.equal(r.stdout, '');
  const { record } = onlyRecord(dir);
  assert.equal(record.payload, null);
  assert.equal(record.payload_error, 'invalid_json');
  assert.equal(record.tool_version, null);
  assert.equal(record.source, 'codex');
});

test('probe_marks_oversized_payload', t => {
  const dir = scratch(t);
  const r = run(['--source', 'codex', '--out', dir], `{"a":"${'x'.repeat(1048576)}"}`);
  assert.equal(r.status, 0, r.stderr);
  const { record, text } = onlyRecord(dir);
  assert.equal(record.payload, null);
  assert.equal(record.payload_error, 'too_large');
  assert.ok(record.payload_bytes > 1048576);
  assert.ok(text.length < 4096, 'an oversized payload must not be stored');
});

test('probe_marks_generation_origin', t => {
  const dir = scratch(t);
  const r = run(['--source', 'claude-code', '--out', dir], '{"hook_event_name":"Stop"}', { TIMESCHEDULE_ORIGIN: 'generation' });
  assert.equal(r.status, 0, r.stderr);
  assert.equal(onlyRecord(dir).record.origin, 'timeschedule_generation');
});

test('probe_records_env_names_not_values', t => {
  const dir = scratch(t);
  const r = run(['--source', 'codex', '--out', dir], '{"hook_event_name":"Stop"}',
    { CODEX_SECRET_X: 'zzz-secret-value', CLAUDE_CODE_ENTRYPOINT: 'cli', UNRELATED_NAME: 'u',
      CLAUDE_CODE_DESKTOP_APP_VERSION: '9.9.9', CLAUDE_AGENT_SDK_VERSION: '8.8.8' });
  assert.equal(r.status, 0, r.stderr);
  const { record, text } = onlyRecord(dir);
  assert.ok(record.env_names.includes('CODEX_SECRET_X'));
  assert.ok(!record.env_names.includes('UNRELATED_NAME'));
  assert.ok(!text.includes('zzz-secret-value'));
  assert.equal(record.env_allowlisted.CLAUDE_CODE_ENTRYPOINT, 'cli');
  // Version variables are the only way to tell which client build ran the hook.
  assert.equal(record.env_allowlisted.CLAUDE_CODE_DESKTOP_APP_VERSION, '9.9.9');
  assert.equal(record.env_allowlisted.CLAUDE_AGENT_SDK_VERSION, '8.8.8');
  assert.deepEqual(Object.keys(record.env_allowlisted).sort(),
    ['CLAUDE_AGENT_SDK_VERSION', 'CLAUDE_CODE_DESKTOP_APP_VERSION', 'CLAUDE_CODE_ENTRYPOINT']);
});

test('probe_fails_visibly_when_out_is_a_file', t => {
  const dir = scratch(t);
  const blocker = path.join(dir, 'not-a-directory');
  writeFileSync(blocker, 'x');
  const r = run(['--source', 'codex', '--out', blocker], '{"hook_event_name":"Stop"}');
  assert.equal(r.status, 1);
  assert.equal(r.stdout, '');
  assert.match(r.stderr, /^probe-hook: /);
});

test('probe_rejects_unknown_source_without_blocking_code', t => {
  const dir = scratch(t);
  const r = run(['--source', 'other-tool', '--out', dir], '{}');
  assert.equal(r.status, 1);
  assert.equal(r.stdout, '');
  assert.match(r.stderr, /^probe-hook: /);
  assert.deepEqual(readdirSync(dir), []);
});

const sample = () => ({
  capture_schema: 1, capture_id: '11111111-1111-4111-8111-111111111111', source: 'claude-code',
  tool_version: '2.1.325', captured_at: '2026-11-18T01:02:03.004Z', origin: 'tool', payload_bytes: 10,
  payload: {
    hook_event_name: 'Stop', session_id: 'abcd-session-1', cwd: 'C:\\LSH\\_capture-lab\\m0-sample',
    transcript_path: 'C:\\Users\\RealName\\.claude\\projects\\C--LSH--capture-lab-m0-sample\\abcd-session-1.jsonl',
    note: 'c:/users/realname/x and c--Users-realname-y',
    turn_id: '0199aaaa-bbbb-7ccc-8ddd-eeeeeeeeeeee',
    nested: [{ again: '0199aaaa-bbbb-7ccc-8ddd-eeeeeeeeeeee' }],
  },
  payload_error: null, env_names: ['CLAUDECODE'], env_allowlisted: { CLAUDECODE: '1' }, probe_elapsed_ms: 12,
});
const sanitizeOptions = ids => ({ homeDir: 'C:\\Users\\realname', labRoot: 'C:\\LSH\\_capture-lab\\m0-sample', sessionIds: ids });

test('sanitize_replaces_home_lab_and_session_ids', async () => {
  const { sanitizeCapture } = await import('../scripts/activity-adapters/sanitize-fixture.mjs');
  const ids = new Map();
  const input = sample();
  const out = sanitizeCapture(input, sanitizeOptions(ids));
  const text = JSON.stringify(out);
  assert.ok(!/realname/i.test(text), text);
  assert.equal(out.payload.cwd, 'C:\\lab\\m0-sample');
  assert.equal(out.payload.session_id, 'session-0001');
  // The folder name Claude Code derives from the lab path is replaced too.
  assert.equal(out.payload.transcript_path, 'C:\\Users\\lab-user\\.claude\\projects\\C--lab-m0-sample\\session-0001.jsonl');
  assert.equal(sanitizeCapture(sample(), sanitizeOptions(ids)).payload.session_id, 'session-0001');
  assert.equal(out.capture_id, input.capture_id);
  assert.deepEqual(input, sample(), 'the input record must not be modified');
});

test('sanitize_replaces_other_uuids_consistently', async () => {
  const { sanitizeCapture } = await import('../scripts/activity-adapters/sanitize-fixture.mjs');
  const out = sanitizeCapture(sample(), sanitizeOptions(new Map()));
  assert.equal(out.payload.turn_id, 'id-0001');
  assert.equal(out.payload.nested[0].again, 'id-0001');
});

test('sanitize_cli_orders_files_by_capture_time', t => {
  const dir = scratch(t);
  const raw = path.join(dir, 'raw');
  const out = path.join(dir, 'out');
  mkdirSync(raw);
  const later = sample();
  const earlier = { ...sample(), captured_at: '2026-11-18T01:02:02.000Z', payload: { ...sample().payload, hook_event_name: 'SessionStart' } };
  writeFileSync(path.join(raw, 'a-later.json'), JSON.stringify(later));
  writeFileSync(path.join(raw, 'b-earlier.json'), JSON.stringify(earlier));
  const r = spawnSync(process.execPath, [SANITIZE, '--in', raw, '--out', out, '--home', 'C:\\Users\\realname', '--lab', 'C:\\LSH\\_capture-lab\\m0-sample'],
    { encoding: 'utf8', windowsHide: true });
  assert.equal(r.status, 0, r.stderr);
  assert.deepEqual(readdirSync(out).sort(), ['01-SessionStart.json', '02-Stop.json']);
  assert.ok(!/realname/i.test(readFileSync(path.join(out, '02-Stop.json'), 'utf8')));
});

// ---- review round 1 (gpt-6.2-astra) ----

test('probe_counts_a_very_large_payload_without_keeping_it', t => {
  const dir = scratch(t);
  const input = `{"a":"${'x'.repeat(6 * 1048576)}"}`;
  const r = run(['--source', 'codex', '--out', dir], input);
  assert.equal(r.status, 0, r.stderr);
  const { record, text } = onlyRecord(dir);
  assert.equal(record.payload_error, 'too_large');
  assert.equal(record.payload_bytes, Buffer.byteLength(input));
  assert.ok(text.length < 4096);
});

test('probe_stops_buffering_once_the_limit_is_passed', async () => {
  const { readBounded } = await import('../scripts/activity-adapters/probe-hook.mjs');
  let kept = 0;
  async function* chunks() { for (let index = 0; index < 64; index += 1) yield Buffer.alloc(65536, 0x78); }
  const result = await readBounded(chunks(), 1048576, held => { kept = Math.max(kept, held); });
  assert.equal(result.bytes, 64 * 65536);
  assert.equal(result.buffer, null);
  assert.ok(kept <= 1048576, `held ${kept} bytes at once`);
  const small = await readBounded((async function* () { yield Buffer.from('{"a":1}'); })(), 1048576);
  assert.equal(small.buffer.toString('utf8'), '{"a":1}');
});

test('sanitize_cli_keeps_ids_stable_across_runs_with_an_id_file', t => {
  const dir = scratch(t);
  const ids = path.join(dir, 'ids.json');
  const write = (folder, sessionId) => {
    mkdirSync(path.join(dir, folder));
    writeFileSync(path.join(dir, folder, 'a.json'), JSON.stringify({ ...sample(), payload: { ...sample().payload, session_id: sessionId } }));
  };
  const sanitize = (folder, out) => spawnSync(process.execPath, [SANITIZE, '--in', path.join(dir, folder), '--out', path.join(dir, out),
    '--home', 'C:\\Users\\realname', '--lab', 'C:\\LSH\\_capture-lab\\m0-sample', '--ids', ids], { encoding: 'utf8', windowsHide: true });
  write('raw1', 'real-session-A'); write('raw2', 'real-session-B'); write('raw3', 'real-session-A');
  for (const [folder, out] of [['raw1', 'out1'], ['raw2', 'out2'], ['raw3', 'out3']]) assert.equal(sanitize(folder, out).status, 0);
  const session = out => JSON.parse(readFileSync(path.join(dir, out, '01-Stop.json'), 'utf8')).payload.session_id;
  assert.equal(session('out1'), 'session-0001');
  assert.equal(session('out2'), 'session-0002');
  assert.equal(session('out3'), 'session-0001');
  assert.ok(!readFileSync(path.join(dir, 'out1', '01-Stop.json'), 'utf8').includes('real-session-A'));
});

// ---- review round 2 (gpt-6.2-astra) ----

test('sanitize_replaces_the_home_name_only_as_a_whole_name', async () => {
  const { sanitizeCapture } = await import('../scripts/activity-adapters/sanitize-fixture.mjs');
  const record = sample();
  record.payload.hook_event_name = 'UserPromptSubmit';
  record.payload.transcript_path = 'C:\\Users\\user\\.claude\\projects\\C--Users-user-x\\abcd-session-1.jsonl';
  record.payload.note = 'users of the user-facing UserPromptSubmit hook';
  const out = sanitizeCapture(record, { homeDir: 'C:\\Users\\user', labRoot: 'C:\\LSH\\_capture-lab\\m0-sample', sessionIds: new Map() });
  assert.equal(out.payload.hook_event_name, 'UserPromptSubmit');
  assert.ok(Object.hasOwn(out.payload, 'hook_event_name'));
  assert.equal(out.payload.transcript_path, 'C:\\Users\\lab-user\\.claude\\projects\\C--Users-lab-user-x\\session-0001.jsonl');
  // The bare word is left alone: only a profile folder is replaced. A fixture that still
  // carries the account name as a word is caught by the fixture tests, for a person to judge.
  assert.equal(out.payload.note, 'users of the user-facing UserPromptSubmit hook');
});

function sanitizeCli(args) {
  return spawnSync(process.execPath, [SANITIZE, ...args], { encoding: 'utf8', windowsHide: true });
}

test('sanitize_cli_rejects_a_home_or_lab_that_is_not_an_absolute_path', t => {
  const dir = scratch(t);
  const raw = path.join(dir, 'raw');
  mkdirSync(raw);
  writeFileSync(path.join(raw, 'a.json'), JSON.stringify(sample()));
  for (const home of ['%USERPROFILE%', '$env:USERPROFILE', 'realname', '~']) {
    const r = sanitizeCli(['--in', raw, '--out', path.join(dir, 'out'), '--home', home, '--lab', 'C:\\LSH\\_capture-lab\\m0-sample']);
    assert.equal(r.status, 1, home);
    assert.match(r.stderr, /^sanitize-fixture: /);
  }
  const lab = sanitizeCli(['--in', raw, '--out', path.join(dir, 'out'), '--home', 'C:\\Users\\realname', '--lab', '%LAB%']);
  assert.equal(lab.status, 1);
  assert.ok(!existsSync(path.join(dir, 'out')), 'nothing may be written when an argument is rejected');
});

test('sanitize_cli_uses_the_current_home_folder_when_home_is_omitted', t => {
  const dir = scratch(t);
  const raw = path.join(dir, 'raw');
  mkdirSync(raw);
  const record = sample();
  record.payload.transcript_path = path.join(homedir(), '.claude', 'x.jsonl');
  writeFileSync(path.join(raw, 'a.json'), JSON.stringify(record));
  const r = sanitizeCli(['--in', raw, '--out', path.join(dir, 'out'), '--lab', 'C:\\LSH\\_capture-lab\\m0-sample']);
  assert.equal(r.status, 0, r.stderr);
  const out = JSON.parse(readFileSync(path.join(dir, 'out', '01-Stop.json'), 'utf8'));
  assert.equal(out.payload.transcript_path, 'C:\\Users\\lab-user\\.claude\\x.jsonl');
});

test('sanitize_cli_keeps_existing_fixtures_when_the_id_file_is_unreadable', t => {
  const dir = scratch(t);
  const raw = path.join(dir, 'raw');
  const out = path.join(dir, 'out');
  const ids = path.join(dir, 'ids.json');
  mkdirSync(raw);
  mkdirSync(out);
  writeFileSync(path.join(raw, 'a.json'), JSON.stringify(sample()));
  writeFileSync(path.join(out, '01-Old.json'), 'keep me');
  writeFileSync(ids, '[["real", "session-0001"');
  const args = ['--in', raw, '--out', out, '--home', 'C:\\Users\\realname', '--lab', 'C:\\LSH\\_capture-lab\\m0-sample', '--ids', ids];
  const broken = sanitizeCli(args);
  assert.equal(broken.status, 1);
  assert.deepEqual(readdirSync(out), ['01-Old.json']);
  assert.equal(readFileSync(path.join(out, '01-Old.json'), 'utf8'), 'keep me');
  assert.equal(readFileSync(ids, 'utf8'), '[["real", "session-0001"', 'a broken id file is left for a person to look at');
  writeFileSync(path.join(raw, 'b.json'), '{ not json');
  writeFileSync(ids, '[]');
  assert.equal(sanitizeCli(args).status, 1, 'an unreadable record also stops before anything is replaced');
  assert.deepEqual(readdirSync(out), ['01-Old.json']);
});

// ---- review round 4 (gpt-6.2-astra) ----

test('sanitize_never_rewrites_field_names_or_schema_values_whatever_the_account_is_called', async () => {
  const { sanitizeCapture } = await import('../scripts/activity-adapters/sanitize-fixture.mjs');
  for (const account of ['session', 'tool', 'user', 'payload', 'ab', '\ud64d\uae38']) {
    const record = sample();
    record.payload.hook_event_name = 'PostToolUse';
    record.payload.tool_name = 'Bash';
    record.payload.transcript_path = `C:\\Users\\${account}\\.claude\\projects\\C--Users-${account}-repo\\abcd-session-1.jsonl`;
    record.payload.note = `c:/users/${account}/x`;
    const out = sanitizeCapture(record, { homeDir: `C:\\Users\\${account}`, labRoot: 'C:\\LSH\\_capture-lab\\m0-sample', sessionIds: new Map() });
    assert.deepEqual(Object.keys(out), Object.keys(record), account);
    assert.deepEqual(Object.keys(out.payload), Object.keys(record.payload), account);
    assert.equal(out.origin, 'tool', account);
    assert.equal(out.source, 'claude-code', account);
    assert.equal(out.tool_version, '2.1.325', account);
    assert.equal(out.payload.hook_event_name, 'PostToolUse', account);
    assert.equal(out.payload.tool_name, 'Bash', account);
    assert.equal(out.payload.session_id, 'session-0001', account);
    assert.equal(out.payload.transcript_path, 'C:\\Users\\lab-user\\.claude\\projects\\C--Users-lab-user-repo\\session-0001.jsonl', account);
    assert.equal(out.payload.note, 'C:/Users/lab-user/x', account);
    assert.ok(!JSON.stringify(out).includes('abcd-session-1'), account);
  }
});

test('sanitize_cli_refuses_an_empty_input_and_keeps_existing_fixtures', t => {
  const dir = scratch(t);
  const raw = path.join(dir, 'raw');
  const out = path.join(dir, 'out');
  mkdirSync(raw);
  mkdirSync(out);
  writeFileSync(path.join(out, '01-Old.json'), 'keep me');
  writeFileSync(path.join(out, 'expected.json'), '[]');
  const r = sanitizeCli(['--in', raw, '--out', out, '--home', 'C:\\Users\\realname', '--lab', 'C:\\LSH\\_capture-lab\\m0-sample']);
  assert.equal(r.status, 1);
  assert.match(r.stderr, /^sanitize-fixture: /);
  assert.deepEqual(readdirSync(out).sort(), ['01-Old.json', 'expected.json']);
});

test('sanitize_cli_replaces_a_scenario_as_a_whole_and_leaves_no_temporary_files', t => {
  const dir = scratch(t);
  const raw = path.join(dir, 'raw');
  const out = path.join(dir, 'out');
  mkdirSync(raw);
  mkdirSync(out);
  writeFileSync(path.join(out, '01-Old.json'), 'old');
  writeFileSync(path.join(out, '02-Older.json'), 'old');
  writeFileSync(path.join(out, 'expected.json'), '[]');
  writeFileSync(path.join(raw, 'a.json'), JSON.stringify(sample()));
  const r = sanitizeCli(['--in', raw, '--out', out, '--home', 'C:\\Users\\realname', '--lab', 'C:\\LSH\\_capture-lab\\m0-sample']);
  assert.equal(r.status, 0, r.stderr);
  assert.deepEqual(readdirSync(out).sort(), ['01-Stop.json', 'expected.json']);
});
