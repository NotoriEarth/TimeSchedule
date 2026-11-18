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
  const files = allFixtureFiles();
  assert.ok(files.length > 0, 'no fixture files found');
  for (const file of files) {
    const text = readFileSync(file, 'utf8');
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

// ---- Codex samples (app-bundled codex-cli 0.169.0-alpha.11.1, exec CLI and desktop app) ----

const codexFixture = (scenario, file) => readJson(path.join(FIXTURES, 'codex', scenario, file));

test('codex stop, interrupt and compaction map to distinct kinds', async () => {
  const { normalizeEvent } = await loadAdapter('codex');
  const stop = normalizeEvent(codexFixture('s1-normal-app', '08-Stop.json'), LAB).envelope;
  assert.equal(stop.kind, 'turn_end');
  assert.match(stop.body.last_assistant_message, /note\.txt/);
  const interrupt = normalizeEvent(codexFixture('s4-interrupt-app', '04-Interrupt.json'), LAB).envelope;
  assert.equal(interrupt.kind, 'interrupted');
  assert.deepEqual(interrupt.body, {});
  const compaction = normalizeEvent(codexFixture('s3-compact-app', '02-PostCompact.json'), LAB).envelope;
  assert.equal(compaction.kind, 'compaction');
  assert.deepEqual(compaction.body, { trigger: 'manual' });
  const afterCompaction = normalizeEvent(codexFixture('s3-compact-sessionstart-app', '01-SessionStart.json'), LAB).envelope;
  assert.deepEqual(afterCompaction.body, { source: 'compact', model: 'gpt-6-luna' });
});

test('codex model requests are observed but never become activities', async () => {
  const { normalizeEvent } = await loadAdapter('codex');
  for (const file of ['03-PreModelRequest.json', '04-PostModelRequest.json']) {
    assert.deepEqual(normalizeEvent(codexFixture('s1-normal-cli', file), LAB), { outcome: 'excluded', reason: 'not_an_activity' });
  }
});

test('codex tool result has no failure flag and carries no tool text', async () => {
  const { normalizeEvent } = await loadAdapter('codex');
  const record = codexFixture('s5-tool-failure-cli', '06-PostToolUse.json');
  assert.match(record.payload.tool_response, /Cannot find module/, 'the sample is the failed command');
  const { envelope } = normalizeEvent(record, LAB);
  assert.equal(envelope.kind, 'tool_result');
  assert.deepEqual(Object.keys(envelope.body).sort(), ['input_keys', 'tool_name', 'tool_use_id']);
  assert.deepEqual(envelope.body.input_keys, ['command']);
  assert.ok(!JSON.stringify(envelope).includes('missing.js'));
});

test('codex session start and end keep source, model and reason', async () => {
  const { normalizeEvent } = await loadAdapter('codex');
  assert.deepEqual(normalizeEvent(codexFixture('s1-normal-cli', '01-SessionStart.json'), LAB).envelope.body, { source: 'startup', model: 'gpt-6-luna' });
  assert.deepEqual(normalizeEvent(codexFixture('s2-resume-cli', '01-SessionStart.json'), LAB).envelope.body, { source: 'resume', model: 'gpt-6-luna' });
  assert.deepEqual(normalizeEvent(codexFixture('s1-normal-cli', '10-SessionEnd.json'), LAB).envelope.body, { reason: 'other' });
});

// ---- how a turn ended: completed (Stop), interrupted (Interrupt), or no terminal hook at all ----

async function endings(source, scenario) {
  const { classifyTurns } = await loadAdapter(source);
  const dir = path.join(FIXTURES, source, scenario);
  const records = readdirSync(dir).filter(file => /^\d{2}-/.test(file)).map(file => readJson(path.join(dir, file)));
  // Order must not matter: the function sorts by capture time itself.
  return classifyTurns(records.reverse(), LAB);
}

test('codex desktop turn that finished is completed', async () => {
  const turns = await endings('codex', 's1-normal-app');
  assert.deepEqual(turns.map(turn => [turn.ending, turn.last_event]), [['completed', 'Stop']]);
  assert.deepEqual([turns[0].session_id], sessionOf('codex', 's1-normal-app'));
  assert.equal(typeof turns[0].turn_id, 'string');
  assert.deepEqual((await endings('codex', 's4-esc-attempt-completed-app')).map(turn => turn.ending), ['completed']);
  assert.deepEqual((await endings('codex', 's2-reentry-app')).map(turn => turn.ending), ['completed', 'completed']);
});

test('codex desktop turn stopped by the user is interrupted, not completed', async () => {
  const turns = await endings('codex', 's4-interrupt-app');
  assert.deepEqual(turns.map(turn => [turn.ending, turn.last_event]), [['interrupted', 'Interrupt']]);
});

test('codex cli turn whose process was killed has no terminal hook', async () => {
  const turns = await endings('codex', 's6-kill-after-request-cli');
  assert.deepEqual(turns.map(turn => [turn.ending, turn.last_event]), [['unterminated', 'PreModelRequest']]);
});

test('claude cli turns: finished is completed, killed process is unterminated', async () => {
  assert.deepEqual((await endings('claude', 's1-normal-cli')).map(turn => [turn.ending, turn.last_event]), [['completed', 'Stop']]);
  assert.deepEqual((await endings('claude', 's5-tool-failure-cli')).map(turn => turn.ending), ['completed']);
  assert.deepEqual((await endings('claude', 's6-kill-cli')).map(turn => [turn.ending, turn.last_event]), [['unterminated', 'UserPromptSubmit']]);
});

test('compaction and session events alone are not a turn', async () => {
  assert.deepEqual(await endings('codex', 's3-compact-app'), []);
  assert.deepEqual(await endings('codex', 's3-compact-sessionstart-app'), []);
});

test('classifyTurns ignores records it cannot vouch for', async () => {
  const { classifyTurns } = await loadAdapter('codex');
  const dir = path.join(FIXTURES, 'codex', 's1-normal-app');
  const records = readdirSync(dir).filter(file => /^\d{2}-/.test(file)).map(file => readJson(path.join(dir, file)));
  assert.deepEqual(classifyTurns(records.map(record => ({ ...record, tool_version: '0.0.0-unknown' })), LAB), []);
  assert.deepEqual(classifyTurns([null, { capture_schema: 1 }, ...records], LAB).map(turn => turn.ending), ['completed']);
});

// ---- review round 1 (gpt-6.2-astra): fail closed on spelling, origin, decision and order ----

const NESTED = [...REG, { canonicalPath: 'C:\\LSH\\TimeSchedule\\private', decision: 'exclude', projectId: 'p1' }];

test('excluded_path_cannot_be_bypassed_by_path_spelling', async () => {
  for (const { adapter, normalizeEvent } of await verifiedAdapters()) {
    const outcome = cwd => normalizeEvent(capture(adapter, { payload: { cwd } }), ctx(NESTED));
    // Spellings in canonical form: only case, separator direction and one trailing separator differ.
    for (const cwd of ['c:/LSH/timeschedule/PRIVATE/', 'C:\\LSH\\TimeSchedule\\Private\\sub', 'C:\\lsh\\TIMESCHEDULE\\private']) {
      assert.deepEqual(outcome(cwd), { outcome: 'excluded', reason: 'path_excluded' }, cwd);
    }
    // Anything that would need interpreting is refused rather than guessed at, so it can
    // neither reach the excluded folder nor borrow the parent's registration.
    for (const cwd of [
      'C:\\LSH\\TimeSchedule\\.\\private', 'C:\\LSH\\TimeSchedule\\a\\..\\private', 'C:\\LSH\\TimeSchedule\\\\private',
      'C:/LSH/TimeSchedule/./private/sub', 'C:\\LSH\\TimeSchedule\\private.', 'C:\\LSH\\TimeSchedule\\private ',
      '\\\\?\\C:\\LSH\\TimeSchedule\\private', 'c:\\lsh\\.\\TIMESCHEDULE\\x\\y\\..\\..\\PRIVATE\\',
      'C:\\LSH\\TimeSchedule\\private\\..\\public', 'C:\\LSH\\TimeSchedule\\..\\Other',
      'C:\\LSH\\TimeSchedule...\\sub', 'C:\\LSH\\TimeSchedule \\sub', 'C:\\LSH\\TimeSchedule\\x.\\private',
    ]) {
      assert.deepEqual(outcome(cwd), { outcome: 'unsupported', reason: 'malformed' }, cwd);
    }
    assert.equal(outcome('C:\\LSH\\TimeSchedule\\privateer').outcome, 'activity');
    assert.equal(outcome('C:\\LSH\\TimeSchedule\\.git').outcome, 'activity', 'a leading dot is an ordinary name');
  }
});

test('cwd_that_is_not_an_absolute_windows_path_is_malformed', async () => {
  for (const { adapter, normalizeEvent } of await verifiedAdapters()) {
    for (const cwd of ['private', '.\\private', '..', 'C:private', '\\LSH\\TimeSchedule', '\\\\server']) {
      assert.deepEqual(normalizeEvent(capture(adapter, { payload: { cwd } }), ctx(NESTED)), { outcome: 'unsupported', reason: 'malformed' }, cwd);
    }
  }
});

test('unc_and_drive_root_registrations_match_by_segment', async () => {
  for (const { adapter, normalizeEvent } of await verifiedAdapters()) {
    const unc = [{ canonicalPath: '\\\\server\\share\\repo', decision: 'new', projectId: 'u1' }];
    assert.equal(normalizeEvent(capture(adapter, { payload: { cwd: '\\\\SERVER\\Share\\repo\\sub' } }), ctx(unc)).envelope.project_id, 'u1');
    assert.equal(normalizeEvent(capture(adapter, { payload: { cwd: '\\\\server\\share2\\repo' } }), ctx(unc)).outcome, 'candidate');
    assert.equal(normalizeEvent(capture(adapter, { payload: { cwd: '\\\\?\\UNC\\server\\share\\repo' } }), ctx(unc)).outcome, 'unsupported');
    const drive = [{ canonicalPath: 'D:\\', decision: 'new', projectId: 'd1' }];
    assert.equal(normalizeEvent(capture(adapter, { payload: { cwd: 'd:\\any\\where' } }), ctx(drive)).envelope.project_id, 'd1');
    assert.equal(normalizeEvent(capture(adapter, { payload: { cwd: 'E:\\any' } }), ctx(drive)).outcome, 'candidate');
    // A registration that is not an absolute path can never match.
    const broken = [{ canonicalPath: 'TimeSchedule', decision: 'new', projectId: 'x' }];
    assert.equal(normalizeEvent(capture(adapter), ctx(broken)).outcome, 'candidate');
  }
});

test('unknown_origin_is_malformed', async () => {
  for (const { adapter, normalizeEvent } of await verifiedAdapters()) {
    for (const origin of [undefined, null, '', 'TOOL', 'somewhere-else']) {
      const result = normalizeEvent({ ...capture(adapter), origin }, ctx());
      assert.deepEqual(result, { outcome: 'unsupported', reason: 'malformed' }, String(origin));
    }
  }
});

test('unconfirmed_decision_is_a_candidate_without_body', async () => {
  for (const { adapter, normalizeEvent } of await verifiedAdapters()) {
    for (const decision of ['pending', undefined, 'NEW', null]) {
      const registrations = [{ canonicalPath: 'C:\\LSH\\TimeSchedule', decision, projectId: 'p1' }];
      const result = normalizeEvent(capture(adapter), ctx(registrations));
      assert.deepEqual(result, { outcome: 'candidate', candidate: { source: adapter.source, path: 'C:\\LSH\\TimeSchedule' } }, String(decision));
    }
    const linked = [{ canonicalPath: 'C:\\LSH\\TimeSchedule', decision: 'link', projectId: 'p9' }];
    assert.equal(normalizeEvent(capture(adapter), ctx(linked)).envelope.project_id, 'p9');
  }
});

function turnRecord(adapter, event, at, extra = {}) {
  const base = capture(adapter, { payload: { hook_event_name: event, cwd: 'C:\\lab\\m0-sample', turn_id: 'turn-a', prompt_id: 'turn-a' } });
  return { ...base, capture_id: `${event}-${at}`, captured_at: `2026-11-18T00:00:00.${at}Z`, ...extra };
}

test('classifyTurns leaves out generation runs and paths that are not confirmed', async () => {
  for (const { adapter, classifyTurns } of await verifiedAdapters()) {
    const stop = turnRecord(adapter, 'Stop', '100');
    assert.equal(classifyTurns([stop], LAB).length, 1);
    assert.deepEqual(classifyTurns([{ ...stop, origin: 'timeschedule_generation' }], LAB), []);
    assert.deepEqual(classifyTurns([{ ...stop, origin: undefined }], LAB), []);
    assert.deepEqual(classifyTurns([stop], ctx()), [], 'path is not registered');
    assert.deepEqual(classifyTurns([stop], ctx([{ canonicalPath: 'C:\\lab\\m0-sample', decision: 'exclude', projectId: 'lab' }])), []);
    assert.deepEqual(classifyTurns([stop], ctx([{ canonicalPath: 'C:\\lab\\m0-sample', decision: 'pending', projectId: 'lab' }])), []);
  }
  const { classifyTurns } = await loadAdapter('codex');
  assert.deepEqual(classifyTurns(scenarioRecords('codex', 's7-generation-origin-cli'), LAB), []);
});

test('classifyTurns gives the same answer for any input order', async () => {
  const { adapter, classifyTurns } = await loadAdapter('codex');
  const stop = turnRecord(adapter, 'Stop', '500');
  const interrupt = turnRecord(adapter, 'Interrupt', '500');
  const forward = classifyTurns([stop, interrupt], LAB);
  const backward = classifyTurns([interrupt, stop], LAB);
  assert.deepEqual(forward, backward);
  // Two terminal hooks in the same millisecond: do not claim completion.
  assert.deepEqual(forward.map(turn => [turn.ending, turn.last_event]), [['interrupted', 'Interrupt']]);
  const later = classifyTurns([turnRecord(adapter, 'Interrupt', '400'), turnRecord(adapter, 'Stop', '600')], LAB);
  assert.deepEqual(later.map(turn => [turn.ending, turn.last_event]), [['completed', 'Stop']]);
});

test('classifyTurns names the hook that ended the turn even when more hooks follow', async () => {
  const { adapter, classifyTurns } = await loadAdapter('codex');
  const turns = classifyTurns([turnRecord(adapter, 'PreToolUse', '700'), turnRecord(adapter, 'Stop', '600'), turnRecord(adapter, 'PreModelRequest', '100')], LAB);
  assert.deepEqual(turns.map(turn => [turn.ending, turn.last_event]), [['completed', 'Stop']]);
  const open = classifyTurns([turnRecord(adapter, 'PreToolUse', '700'), turnRecord(adapter, 'PreModelRequest', '100')], LAB);
  assert.deepEqual(open.map(turn => [turn.ending, turn.last_event]), [['unterminated', 'PreToolUse']]);
});

function scenarioRecords(source, scenario) {
  const dir = path.join(FIXTURES, source, scenario);
  return readdirSync(dir).filter(file => /^\d{2}-/.test(file)).map(file => readJson(path.join(dir, file)));
}
const sessionOf = (source, scenario) => [...new Set(scenarioRecords(source, scenario).map(record => record.payload.session_id))];

test('fixtures keep session identity across scenarios', async () => {
  for (const source of ['claude', 'codex']) {
    const first = sessionOf(source, 's1-normal-cli');
    assert.equal(first.length, 1);
    assert.deepEqual(sessionOf(source, 's2-resume-cli'), first, `${source}: a resumed session keeps the id of the session it resumed`);
    const killed = sessionOf(source, source === 'claude' ? 's6-kill-cli' : 's6-kill-after-request-cli');
    assert.notDeepEqual(killed, first, `${source}: a different session must not share an id`);
  }
  assert.deepEqual(sessionOf('codex', 's2-reentry-app'), sessionOf('codex', 's1-normal-app'));
  assert.deepEqual(sessionOf('codex', 's4-interrupt-app'), sessionOf('codex', 's1-normal-app'));
  assert.notDeepEqual(sessionOf('codex', 's1-normal-app'), sessionOf('codex', 's1-normal-cli'));
});

test('a finished turn and a killed turn stay separate when classified together', async () => {
  const claude = await loadAdapter('claude');
  const together = claude.classifyTurns([...scenarioRecords('claude', 's6-kill-cli'), ...scenarioRecords('claude', 's1-normal-cli')], LAB);
  assert.deepEqual(together.map(turn => turn.ending), ['completed', 'unterminated']);
  const codex = await loadAdapter('codex');
  const all = ['s1-normal-app', 's2-reentry-app', 's4-esc-attempt-completed-app', 's4-interrupt-app', 's6-kill-after-request-cli']
    .flatMap(scenario => scenarioRecords('codex', scenario));
  assert.deepEqual(codex.classifyTurns(all, LAB).map(turn => turn.ending).sort(),
    ['completed', 'completed', 'completed', 'completed', 'interrupted', 'unterminated']);
});

test('nothing is listed as supported without a committed sample', async () => {
  for (const source of Object.keys(SOURCES)) {
    const { adapter } = await loadAdapter(source);
    const records = allScenarios().filter(scenario => scenario.source === source).flatMap(scenario => scenario.files.map(readJson));
    const seenEvents = new Set(records.map(record => record.payload.hook_event_name));
    const seenVersions = new Set(records.map(record => record.tool_version));
    for (const event of Object.keys(adapter.events)) assert.ok(seenEvents.has(event), `${source}: ${event} is listed but has no sample`);
    for (const version of adapter.verifiedVersions) assert.ok(seenVersions.has(version), `${source}: ${version} is listed but has no sample`);
  }
});

// ---- review round 2 (gpt-6.2-astra) ----

test('extended_length_paths_keep_dots_and_spaces_as_part_of_the_name', async () => {
  for (const { adapter, normalizeEvent } of await verifiedAdapters()) {
    const registrations = [{ canonicalPath: 'C:\\lab\\repo', decision: 'new', projectId: 'p1' }];
    const outcome = cwd => normalizeEvent(capture(adapter, { payload: { cwd } }), ctx(registrations)).outcome;
    // The caller canonicalises; the adapter does not interpret the \\?\ form at all.
    for (const cwd of ['\\\\?\\C:\\lab\\repo', '\\\\?\\C:\\lab\\repo\\sub', '\\\\?\\C:\\lab\\repo.', '\\\\?\\C:\\lab\\repo ',
      '\\\\?\\C:\\lab\\repo\\..\\repo', '\\\\?\\C:\\lab\\.\\repo', '\\\\?\\C:\\\\', '\\\\?\\UNC\\server\\share\\\\x',
      'C:\\lab\\repo.', 'C:\\lab\\.\\repo', 'C:\\lab\\repo...\\sub', 'C:\\lab\\repo \\sub']) {
      assert.equal(outcome(cwd), 'unsupported', cwd);
    }
    assert.equal(outcome('C:\\lab\\repo'), 'activity');
    assert.equal(outcome('C:\\lab\\repo\\sub'), 'activity');
  }
});

test('drive_relative_and_device_paths_are_malformed', async () => {
  for (const { adapter, normalizeEvent } of await verifiedAdapters()) {
    const registrations = [{ canonicalPath: 'D:\\', decision: 'new', projectId: 'd1' }];
    for (const cwd of ['D:', 'd:', 'D:.', '\\\\?\\D:', '\\\\.\\C:\\lab', '\\\\?\\C:\\lab\\\\repo', '\\\\?\\']) {
      assert.deepEqual(normalizeEvent(capture(adapter, { payload: { cwd } }), ctx(registrations)), { outcome: 'unsupported', reason: 'malformed' }, cwd);
    }
    assert.equal(normalizeEvent(capture(adapter, { payload: { cwd: 'D:\\' } }), ctx(registrations)).outcome, 'activity');
    // A registration written as `D:` names no folder and can never match.
    assert.equal(normalizeEvent(capture(adapter, { payload: { cwd: 'D:\\x' } }), ctx([{ canonicalPath: 'D:', decision: 'new', projectId: 'd1' }])).outcome, 'candidate');
  }
});

test('conflicting_registrations_for_one_folder_fail_closed_in_any_order', async () => {
  for (const { adapter, normalizeEvent, classifyTurns } of await verifiedAdapters()) {
    const open = { canonicalPath: 'C:\\LSH\\TimeSchedule', decision: 'new', projectId: 'p1' };
    const closed = { canonicalPath: 'c:/lsh/timeschedule/', decision: 'exclude', projectId: 'p1' };
    const pending = { canonicalPath: 'C:\\LSH\\TIMESCHEDULE', decision: 'pending', projectId: 'p1' };
    const other = { canonicalPath: 'C:\\LSH\\TimeSchedule\\', decision: 'link', projectId: 'p2' };
    const run = registrations => normalizeEvent(capture(adapter), ctx(registrations));
    for (const order of [[open, closed], [closed, open], [open, pending, closed]]) {
      assert.deepEqual(run(order), { outcome: 'excluded', reason: 'path_excluded' });
    }
    for (const order of [[open, pending], [pending, open], [open, other], [other, open]]) {
      const result = run(order);
      assert.equal(result.outcome, 'candidate');
      assert.equal(result.envelope, undefined);
    }
    assert.equal(run([open, { ...open }]).outcome, 'activity', 'the same registration twice is not a conflict');
    const stop = { ...capture(adapter, { payload: { hook_event_name: 'Stop', turn_id: 't', prompt_id: 't' } }) };
    assert.deepEqual(classifyTurns([stop], ctx([open, closed])), []);
    assert.deepEqual(classifyTurns([stop], ctx([other, open])), []);
    assert.equal(classifyTurns([stop], ctx([open])).length, 1);
  }
});

test('capture_time_must_be_a_utc_millisecond_timestamp', async () => {
  for (const { adapter, normalizeEvent, classifyTurns } of await verifiedAdapters()) {
    for (const captured_at of ['invalid-time', '2026-11-18', '2026-11-18T00:00:00Z', '2026-11-18T00:00:00.000+09:00', '2026-13-45T00:00:00.000Z']) {
      assert.deepEqual(normalizeEvent({ ...capture(adapter), captured_at }, ctx()), { outcome: 'unsupported', reason: 'malformed' }, captured_at);
      const stop = { ...capture(adapter, { payload: { hook_event_name: 'Stop', turn_id: 't', prompt_id: 't' } }), captured_at };
      assert.deepEqual(classifyTurns([stop], ctx()), [], captured_at);
    }
  }
});

test('every_listed_event_has_a_sample_from_every_verified_version', async () => {
  for (const source of Object.keys(SOURCES)) {
    const { adapter } = await loadAdapter(source);
    const seen = new Set(allScenarios().filter(scenario => scenario.source === source)
      .flatMap(scenario => scenario.files.map(readJson)).map(record => `${record.tool_version}\n${record.payload.hook_event_name}`));
    for (const version of adapter.verifiedVersions) {
      for (const event of Object.keys(adapter.events)) {
        assert.ok(seen.has(`${version}\n${event}`), `${source}: ${event} has no sample from ${version}`);
      }
    }
  }
});

test('fixtures contain no unexpanded variable reference', () => {
  for (const file of allFixtureFiles()) {
    const text = readFileSync(file, 'utf8');
    assert.ok(!text.includes('%USERPROFILE%') && !text.includes('$env:'), file);
  }
});

// ---- review round 4 (gpt-6.2-astra): compare canonical paths only, strictly to confirm, loosely to exclude ----

test('a_registration_that_is_not_in_canonical_form_yields_no_body_for_any_folder', async () => {
  for (const { adapter, normalizeEvent, classifyTurns } of await verifiedAdapters()) {
    for (const canonicalPath of ['C:\\LSH\\.\\TimeSchedule', 'C:\\LSH\\TimeSchedule.', '\\\\?\\C:\\LSH\\TimeSchedule', 'TimeSchedule', '', undefined]) {
      for (const decision of ['new', 'exclude', 'pending']) {
        const registrations = [...REG, { canonicalPath, decision, projectId: 'p2' }];
        const result = normalizeEvent(capture(adapter), ctx(registrations));
        assert.equal(result.outcome, 'candidate', `${canonicalPath} ${decision}`);
        assert.equal(result.envelope, undefined);
        const stop = capture(adapter, { payload: { hook_event_name: 'Stop', turn_id: 't', prompt_id: 't' } });
        assert.deepEqual(classifyTurns([stop], ctx(registrations)), []);
      }
    }
  }
});

test('non_ascii_names_confirm_only_on_an_exact_match_but_exclude_on_a_loose_one', async () => {
  for (const { adapter, normalizeEvent } of await verifiedAdapters()) {
    const run = (cwd, registrations) => normalizeEvent(capture(adapter, { payload: { cwd } }), ctx(registrations));
    // U+0130 lower-cases to `i` + U+0307 in JavaScript, but Windows treats the two names as different folders.
    const dotted = [{ canonicalPath: 'C:\\lab\\i\u0307x', decision: 'new', projectId: 'p1' }];
    assert.equal(run('C:\\lab\\\u0130x', dotted).outcome, 'candidate');
    assert.equal(run('C:\\lab\\i\u0307x', dotted).outcome, 'activity');
    const accent = [{ canonicalPath: 'C:\\lab\\caf\u00e9', decision: 'new', projectId: 'p1' }];
    assert.equal(run('C:\\LAB\\caf\u00e9\\src', accent).outcome, 'activity', 'ASCII case still folds');
    assert.equal(run('C:\\lab\\CAF\u00c9', accent).outcome, 'candidate', 'non-ASCII case is not folded to confirm');
    assert.equal(run('C:\\lab\\stra\u00dfe', [{ canonicalPath: 'C:\\lab\\STRASSE', decision: 'new', projectId: 'p1' }]).outcome, 'candidate');
    // To exclude, over-matching is the safe direction.
    const nested = [{ canonicalPath: 'C:\\lab', decision: 'new', projectId: 'p1' },
      { canonicalPath: 'C:\\lab\\priv\u00e9', decision: 'exclude', projectId: 'p1' }];
    assert.deepEqual(run('C:\\lab\\PRIV\u00c9\\notes', nested), { outcome: 'excluded', reason: 'path_excluded' });
    assert.deepEqual(run('C:\\lab\\priv\u00e9', nested), { outcome: 'excluded', reason: 'path_excluded' });
    assert.equal(run('C:\\lab\\public', nested).outcome, 'activity');
    // Korean has no case: an exact name matches, a different name does not.
    const korean = [{ canonicalPath: 'C:\\LSH\\\ubd84\uc11d', decision: 'new', projectId: 'k1' }];
    assert.equal(run('c:\\lsh\\\ubd84\uc11d\\data', korean).envelope.project_id, 'k1');
    assert.equal(run('C:\\LSH\\\ubd84\uc11d2', korean).outcome, 'candidate');
  }
});

test('turn ids that contain a line break do not merge two turns', async () => {
  const { adapter, classifyTurns } = await loadAdapter('codex');
  const open = capture(adapter, { payload: { hook_event_name: 'PreToolUse', cwd: 'C:\\lab\\m0-sample', session_id: 's', turn_id: 'x\ny' } });
  const stop = { ...capture(adapter, { payload: { hook_event_name: 'Stop', cwd: 'C:\\lab\\m0-sample', session_id: 's\nx', turn_id: 'y' } }),
    capture_id: 'second', captured_at: '2026-11-17T22:48:03.000Z' };
  assert.deepEqual(classifyTurns([open, stop], LAB).map(turn => turn.ending).sort(), ['completed', 'unterminated']);
});

test('fixtures do not contain the current account name as a word', () => {
  const home = path.basename(homedir());
  const word = new RegExp(`(?<![A-Za-z0-9_])${home.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')}(?![A-Za-z0-9_])`, 'i');
  for (const file of allFixtureFiles()) {
    assert.ok(!word.test(readFileSync(file, 'utf8').replaceAll('lab-user', '')), `${file} contains the account name`);
  }
});

function profileFolders(value, found = []) {
  if (typeof value === 'string') {
    for (const match of value.matchAll(/users[\\/]+([^\\/"]+)/gi)) found.push({ form: 'path', name: match[1] });
    for (const match of value.matchAll(/users-+([^\\/"]*)/gi)) found.push({ form: 'folder-name', name: match[1] });
  } else if (Array.isArray(value)) {
    value.forEach(item => profileFolders(item, found));
  } else if (value && typeof value === 'object') {
    for (const [key, item] of Object.entries(value)) { profileFolders(key, found); profileFolders(item, found); }
  }
  return found;
}

test('profile folder check recognises names it must reject', () => {
  const names = value => profileFolders(value).map(item => item.name);
  assert.deepEqual(names({ a: 'C:\\Users\\\ud64d\uae38\ub3d9\\x', b: 'c:/users/someone/y', c: ['C:\\Users\\lab-user-2\\z'] }),
    ['\ud64d\uae38\ub3d9', 'someone', 'lab-user-2']);
  assert.deepEqual(profileFolders('C--Users-someone-repo'), [{ form: 'folder-name', name: 'someone-repo' }]);
});

test('fixtures name no profile folder other than the placeholder (any script, any case)', () => {
  let seen = 0;
  for (const file of allFixtureFiles()) {
    for (const { form, name } of profileFolders(readJson(file))) {
      seen += 1;
      if (form === 'path') assert.equal(name, 'lab-user', `${file}: profile folder ${name}`);
      else assert.match(name, /^lab-user(-|$)/, `${file}: profile folder name ${name}`);
    }
  }
  assert.ok(seen > 0, 'the fixtures are expected to contain transcript paths');
});
