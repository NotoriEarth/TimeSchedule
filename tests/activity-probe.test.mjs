import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdtempSync, mkdirSync, readFileSync, readdirSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
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

function run(args, input, env = {}) {
  return spawnSync(process.execPath, [PROBE, ...args], {
    input, encoding: 'utf8', windowsHide: true, env: { ...process.env, TIMESCHEDULE_ORIGIN: '', ...env },
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
    { CODEX_SECRET_X: 'zzz-secret-value', CLAUDE_CODE_ENTRYPOINT: 'cli', UNRELATED_NAME: 'u' });
  assert.equal(r.status, 0, r.stderr);
  const { record, text } = onlyRecord(dir);
  assert.ok(record.env_names.includes('CODEX_SECRET_X'));
  assert.ok(!record.env_names.includes('UNRELATED_NAME'));
  assert.ok(!text.includes('zzz-secret-value'));
  assert.equal(record.env_allowlisted.CLAUDE_CODE_ENTRYPOINT, 'cli');
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
  assert.equal(out.payload.transcript_path, 'C:\\Users\\lab-user\\.claude\\projects\\C--LSH--capture-lab-m0-sample\\session-0001.jsonl');
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
