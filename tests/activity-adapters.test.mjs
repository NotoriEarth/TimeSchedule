import assert from 'node:assert/strict';
import { existsSync, readFileSync, readdirSync } from 'node:fs';
import { homedir } from 'node:os';
import path from 'node:path';
import test from 'node:test';
import { fileURLToPath } from 'node:url';

const root = path.dirname(path.dirname(fileURLToPath(import.meta.url)));
const FIXTURES = path.join(root, 'tests', 'fixtures', 'project-activity');
const SOURCES = { claude: 'claude.mjs', codex: 'codex.mjs' };
const loadAdapter = name => import(`../scripts/activity-adapters/${SOURCES[name]}`);
const readJson = file => JSON.parse(readFileSync(file, 'utf8'));

function allScenarios() {
  const scenarios = [];
  for (const source of Object.keys(SOURCES)) {
    const sourceDir = path.join(FIXTURES, source);
    if (!existsSync(sourceDir)) continue;
    for (const name of readdirSync(sourceDir).sort()) {
      const dir = path.join(sourceDir, name);
      const files = readdirSync(dir).filter(file => /^\d{2}-.*\.json$/.test(file)).sort().map(file => path.join(dir, file));
      scenarios.push({ source, name, dir, files });
    }
  }
  return scenarios;
}
const allFixtureFiles = () => allScenarios().flatMap(scenario => scenario.files);

test('fixtures_contain_no_private_markers', () => {
  const home = path.basename(homedir());
  const files = allFixtureFiles();
  assert.ok(files.length > 0, 'no fixture files found');
  for (const file of files) {
    const text = readFileSync(file, 'utf8');
    assert.ok(!text.toLowerCase().includes(home.toLowerCase()), `${file} contains the home folder name`);
    assert.ok(!/[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}/.test(text), `${file} contains an email-shaped string`);
    assert.ok(!/\b(sk-[A-Za-z0-9_-]{16,}|ghp_[A-Za-z0-9]{20,}|github_pat_[A-Za-z0-9_]{20,})/.test(text), `${file} contains a token-shaped string`);
    assert.ok(!/[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}/i.test(JSON.stringify(readJson(file).payload ?? {})),
      `${file} payload contains an unreplaced id`);
  }
});

// ---- contract guards --------------------------------------------------------

const REG = [{ canonicalPath: 'C:\\LSH\\TimeSchedule', decision: 'new', projectId: 'p1' }];
const ctx = (registrations = REG) => ({ registrations, receivedAt: '2026-11-18T00:00:00.000Z' });

function capture(adapter, { payload = {}, ...top } = {}) {
  return {
    capture_schema: 1, capture_id: '11111111-1111-4111-8111-111111111111', source: adapter.source,
    tool_version: adapter.verifiedVersions[0], captured_at: '2026-11-17T22:48:02.395Z', origin: 'tool',
    payload_bytes: 100, payload_error: null, env_names: [], env_allowlisted: {}, probe_elapsed_ms: 28,
    ...top,
    payload: payload === null ? null : {
      session_id: 'session-0001', cwd: 'C:\\LSH\\TimeSchedule', hook_event_name: 'UserPromptSubmit',
      prompt: 'SECRET-BODY', ...payload,
    },
  };
}

for (const name of Object.keys(SOURCES)) {
  test(`${name}: adapter_declares_source_versions_and_events`, async () => {
    const { adapter, normalizeEvent } = await loadAdapter(name);
    assert.equal(typeof normalizeEvent, 'function');
    assert.match(adapter.source, /^(claude-code|codex)$/);
    assert.ok(Array.isArray(adapter.verifiedVersions));
    assert.equal(typeof adapter.events, 'object');
  });

  test(`${name}: normalize_unknown_version_is_unsupported`, async () => {
    const { adapter, normalizeEvent } = await loadAdapter(name);
    const record = { ...capture(adapter), tool_version: '0.0.0-unknown' };
    assert.deepEqual(normalizeEvent(record, ctx()), { outcome: 'unsupported', reason: 'unknown_version' });
    assert.deepEqual(normalizeEvent({ ...record, tool_version: null }, ctx()), { outcome: 'unsupported', reason: 'unknown_version' });
  });

  test(`${name}: generation_origin_wins_over_unknown_version`, async () => {
    const { adapter, normalizeEvent } = await loadAdapter(name);
    const record = { ...capture(adapter), origin: 'timeschedule_generation', tool_version: '0.0.0-unknown' };
    assert.deepEqual(normalizeEvent(record, ctx()), { outcome: 'excluded', reason: 'generation_origin' });
  });

  test(`${name}: malformed_capture_is_unsupported`, async () => {
    const { adapter, normalizeEvent } = await loadAdapter(name);
    const malformed = { outcome: 'unsupported', reason: 'malformed' };
    assert.deepEqual(normalizeEvent(capture(adapter, { payload: null }), ctx()), malformed);
    assert.deepEqual(normalizeEvent({ ...capture(adapter), capture_schema: 2 }, ctx()), malformed);
    assert.deepEqual(normalizeEvent(capture(adapter, { payload: { cwd: undefined } }), ctx()), malformed);
    assert.deepEqual(normalizeEvent(capture(adapter, { payload: { session_id: '' } }), ctx()), malformed);
    assert.deepEqual(normalizeEvent({ ...capture(adapter), capture_id: undefined }, ctx()), malformed);
    assert.deepEqual(normalizeEvent({ ...capture(adapter), source: 'some-other-tool' }, ctx()), malformed);
    assert.deepEqual(normalizeEvent(null, ctx()), malformed);
  });
}

// The guards below need an event that becomes an activity, so they run for every adapter
// that has at least one version verified by a lab sample.
async function verifiedAdapters() {
  const loaded = await Promise.all(Object.keys(SOURCES).map(async name => ({ name, ...(await loadAdapter(name)) })));
  return loaded.filter(item => item.adapter.verifiedVersions.length > 0);
}

test('at_least_one_adapter_has_a_verified_version', async () => {
  assert.ok((await verifiedAdapters()).length > 0);
});

test('unconfirmed_path_has_no_body', async () => {
  for (const { adapter, normalizeEvent } of await verifiedAdapters()) {
    const result = normalizeEvent(capture(adapter, { payload: { cwd: 'C:\\Other\\Repo' } }), ctx());
    assert.equal(result.outcome, 'candidate');
    assert.equal(result.envelope, undefined);
    assert.deepEqual(result.candidate, { source: adapter.source, path: 'C:\\Other\\Repo' });
    assert.ok(!JSON.stringify(result).includes('SECRET-BODY'));
  }
});

test('generation_origin_is_excluded', async () => {
  for (const { adapter, normalizeEvent } of await verifiedAdapters()) {
    assert.deepEqual(normalizeEvent({ ...capture(adapter), origin: 'timeschedule_generation' }, ctx()),
      { outcome: 'excluded', reason: 'generation_origin' });
  }
});

test('excluded_path_is_excluded', async () => {
  for (const { adapter, normalizeEvent } of await verifiedAdapters()) {
    const registrations = [{ canonicalPath: 'C:\\LSH\\TimeSchedule', decision: 'exclude', projectId: 'p1' }];
    assert.deepEqual(normalizeEvent(capture(adapter), ctx(registrations)), { outcome: 'excluded', reason: 'path_excluded' });
  }
});

test('sibling_prefix_path_is_not_registered', async () => {
  for (const { adapter, normalizeEvent } of await verifiedAdapters()) {
    const result = normalizeEvent(capture(adapter, { payload: { cwd: 'C:\\LSH\\TimeSchedule-test' } }), ctx());
    assert.equal(result.outcome, 'candidate');
    assert.equal(result.candidate.path, 'C:\\LSH\\TimeSchedule-test');
  }
});

test('path_match_ignores_case_separators_and_subfolders', async () => {
  for (const { adapter, normalizeEvent } of await verifiedAdapters()) {
    const result = normalizeEvent(capture(adapter, { payload: { cwd: 'c:/lsh/timeschedule/crates/' } }), ctx());
    assert.equal(result.outcome, 'activity');
    assert.equal(result.envelope.project_id, 'p1');
  }
});

test('nested_exclude_wins_over_parent', async () => {
  for (const { adapter, normalizeEvent } of await verifiedAdapters()) {
    const registrations = [...REG, { canonicalPath: 'C:\\LSH\\TimeSchedule\\vendor', decision: 'exclude', projectId: 'p1' }];
    const inside = normalizeEvent(capture(adapter, { payload: { cwd: 'C:\\LSH\\TimeSchedule\\vendor\\lib' } }), ctx(registrations));
    assert.deepEqual(inside, { outcome: 'excluded', reason: 'path_excluded' });
    const sibling = normalizeEvent(capture(adapter, { payload: { cwd: 'C:\\LSH\\TimeSchedule\\vendored' } }), ctx(registrations));
    assert.equal(sibling.outcome, 'activity');
  }
});

test('unknown_event_is_unsupported', async () => {
  for (const { adapter, normalizeEvent } of await verifiedAdapters()) {
    assert.deepEqual(normalizeEvent(capture(adapter, { payload: { hook_event_name: 'NotARealEvent' } }), ctx()),
      { outcome: 'unsupported', reason: 'unknown_event' });
    // A name inherited from Object.prototype must not be mistaken for a listed event.
    assert.deepEqual(normalizeEvent(capture(adapter, { payload: { hook_event_name: 'constructor' } }), ctx()),
      { outcome: 'unsupported', reason: 'unknown_event' });
  }
});

test('redundant_event_is_excluded_without_body', async () => {
  for (const { adapter, normalizeEvent } of await verifiedAdapters()) {
    assert.deepEqual(normalizeEvent(capture(adapter, { payload: { hook_event_name: 'PreToolUse', tool_name: 'Write' } }), ctx()),
      { outcome: 'excluded', reason: 'redundant_event' });
  }
});

test('same_capture_normalizes_to_same_event_id', async () => {
  for (const { adapter, normalizeEvent } of await verifiedAdapters()) {
    const record = capture(adapter);
    const first = normalizeEvent(record, ctx());
    const second = normalizeEvent(record, ctx());
    assert.equal(first.outcome, 'activity');
    assert.equal(first.envelope.event_id, record.capture_id);
    assert.deepEqual(first, second);
  }
});

test('envelope_keeps_capture_time_and_receive_time_apart', async () => {
  for (const { adapter, normalizeEvent } of await verifiedAdapters()) {
    const { envelope } = normalizeEvent(capture(adapter), ctx());
    assert.deepEqual(envelope, {
      schema_version: 1, source: adapter.source, session_id: 'session-0001',
      event_id: '11111111-1111-4111-8111-111111111111', revision: 1, project_id: 'p1',
      occurred_at: '2026-11-17T22:48:02.395Z', received_at: '2026-11-18T00:00:00.000Z', time_basis: 'hook_capture',
      kind: 'user_prompt', body: { prompt: 'SECRET-BODY' }, evidence_refs: [], origin: 'tool',
    });
  }
});

// ---- fixtures pinned to observed samples -------------------------------------

const LAB = { registrations: [{ canonicalPath: 'C:\\lab\\m0-sample', decision: 'new', projectId: 'lab' }], receivedAt: '2026-11-18T00:00:00.000Z' };

for (const scenario of allScenarios()) {
  test(`fixture ${scenario.source}/${scenario.name}`, async () => {
    const { normalizeEvent } = await loadAdapter(scenario.source);
    const expected = readJson(path.join(scenario.dir, 'expected.json'));
    const actual = scenario.files.map(file => {
      const result = normalizeEvent(readJson(file), LAB);
      return {
        file: path.basename(file), outcome: result.outcome,
        ...(result.envelope ? { kind: result.envelope.kind } : {}),
        ...(result.reason ? { reason: result.reason } : {}),
      };
    });
    assert.deepEqual(actual, expected);
  });
}

test('every_fixture_event_name_is_listed_or_reported_unsupported', async () => {
  for (const scenario of allScenarios()) {
    const { adapter, normalizeEvent } = await loadAdapter(scenario.source);
    for (const file of scenario.files) {
      const record = readJson(file);
      const listed = Object.hasOwn(adapter.events, record.payload.hook_event_name);
      const result = normalizeEvent({ ...record, origin: 'tool' }, LAB);
      assert.equal(result.outcome === 'unsupported', !listed || !adapter.verifiedVersions.includes(record.tool_version), file);
    }
  }
});

test('claude tool failure keeps tool identity but not tool input or output text', async () => {
  const { normalizeEvent } = await loadAdapter('claude');
  const file = path.join(FIXTURES, 'claude', 's5-tool-failure-cli', '04-PostToolUseFailure.json');
  const { envelope } = normalizeEvent(readJson(file), LAB);
  assert.equal(envelope.kind, 'tool_result');
  assert.deepEqual(Object.keys(envelope.body).sort(), ['failed', 'input_keys', 'tool_name', 'tool_use_id']);
  assert.equal(envelope.body.tool_name, 'Bash');
  assert.equal(envelope.body.failed, true);
  assert.deepEqual(envelope.body.input_keys, ['command', 'description']);
  assert.ok(!JSON.stringify(envelope).includes('missing.js'));
});

test('claude prompt and final message survive as Korean text', async () => {
  const { normalizeEvent } = await loadAdapter('claude');
  const dir = path.join(FIXTURES, 'claude', 's1-normal-cli');
  const prompt = normalizeEvent(readJson(path.join(dir, '02-UserPromptSubmit.json')), LAB).envelope;
  assert.deepEqual(prompt.body, { prompt: 'note.txt 파일에 hello 한 줄을 써라' });
  const stop = normalizeEvent(readJson(path.join(dir, '07-Stop.json')), LAB).envelope;
  assert.equal(stop.kind, 'turn_end');
  assert.match(stop.body.last_assistant_message, /note\.txt/);
  const start = normalizeEvent(readJson(path.join(dir, '01-SessionStart.json')), LAB).envelope;
  assert.deepEqual(start.body, { source: 'startup' });
  const resumed = normalizeEvent(readJson(path.join(FIXTURES, 'claude', 's2-resume-cli', '01-SessionStart.json')), LAB).envelope;
  assert.deepEqual(resumed.body, { source: 'resume' });
  const end = normalizeEvent(readJson(path.join(dir, '08-SessionEnd.json')), LAB).envelope;
  assert.deepEqual(end.body, { reason: 'other' });
});
