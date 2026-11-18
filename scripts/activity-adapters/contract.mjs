// Shared rules for turning one CaptureRecord (see probe-hook.mjs) into a result the
// activity pipeline can act on. Pure functions: no file system, no network, no clock.
//
// The order of the checks is the contract. Earlier checks win, so a record never
// yields a body unless its origin, client version, event name and path all pass.

const isText = value => typeof value === 'string' && value.length > 0;
const isObject = value => value !== null && typeof value === 'object' && !Array.isArray(value);

function isWellFormed(record, adapter) {
  const payload = record?.payload;
  return isObject(record) && record.capture_schema === 1 && record.source === adapter.source
    && isText(record.capture_id) && isText(record.captured_at) && isObject(payload)
    && isText(payload.session_id) && isText(payload.cwd) && isText(payload.hook_event_name);
}

// Comparison form only. Junctions, symlinks and worktrees are resolved by the caller
// before registration; this function never touches the file system.
function comparable(value) {
  return value.replace(/\//g, '\\').replace(/\\+$/, '').toLowerCase();
}

// Returns the registration whose path equals `cwd` or is an ancestor folder of it.
// The longest match wins, so an excluded subfolder overrides its registered parent.
// A sibling that merely shares a prefix (`Repo-test` vs `Repo`) does not match.
export function resolveRegistration(cwd, registrations) {
  const target = comparable(cwd);
  let best = null;
  let bestLength = -1;
  for (const registration of registrations ?? []) {
    const base = comparable(registration.canonicalPath);
    if ((target === base || target.startsWith(`${base}\\`)) && base.length > bestLength) {
      best = registration;
      bestLength = base.length;
    }
  }
  return best;
}

export function normalizeCapture(record, context, adapter) {
  if (!isWellFormed(record, adapter)) return { outcome: 'unsupported', reason: 'malformed' };
  const payload = record.payload;
  if (record.origin === 'timeschedule_generation') return { outcome: 'excluded', reason: 'generation_origin' };
  if (!adapter.verifiedVersions.includes(record.tool_version)) return { outcome: 'unsupported', reason: 'unknown_version' };
  if (!Object.hasOwn(adapter.events, payload.hook_event_name)) return { outcome: 'unsupported', reason: 'unknown_event' };
  const event = adapter.events[payload.hook_event_name];
  if (event.skip) return { outcome: 'excluded', reason: event.skip };

  const registration = resolveRegistration(payload.cwd, context.registrations);
  if (!registration) return { outcome: 'candidate', candidate: { source: adapter.source, path: payload.cwd } };
  if (registration.decision === 'exclude') return { outcome: 'excluded', reason: 'path_excluded' };

  return {
    outcome: 'activity',
    envelope: {
      schema_version: 1,
      source: adapter.source,
      session_id: payload.session_id,
      // Issued once by the probe and reused on every resend, so a replay is recognisable.
      event_id: record.capture_id,
      revision: 1,
      project_id: registration.projectId,
      // Hook payloads observed in M0 carry no event time; this is when the hook ran.
      occurred_at: record.captured_at,
      received_at: context.receivedAt,
      time_basis: 'hook_capture',
      kind: event.kind,
      body: event.body(payload),
      evidence_refs: [],
      origin: 'tool',
    },
  };
}

// Says how each turn in a set of captured records ended:
//   completed    - the tool's normal end-of-turn hook arrived (Stop)
//   interrupted  - the tool reported that the user stopped the turn (Codex Interrupt)
//   unterminated - turn activity was captured but no terminal hook ever arrived
// `unterminated` is what a killed process leaves behind. Hooks alone cannot tell it from a
// turn that is still running, so the caller must know the session is over before it
// treats the turn as lost.
//
// Unlike normalizeCapture this looks at skipped events too (a PreToolUse with nothing
// after it is the only trace of an interrupted tool call) and ignores path registration:
// it reads event names and ids only, never a body.
export function classifyTurns(records, adapter) {
  const usable = (records ?? [])
    .filter(record => isWellFormed(record, adapter)
      && adapter.verifiedVersions.includes(record.tool_version)
      && Object.hasOwn(adapter.events, record.payload.hook_event_name)
      && adapter.events[record.payload.hook_event_name].turn)
    .sort((a, b) => a.captured_at.localeCompare(b.captured_at));
  const turns = new Map();
  for (const record of usable) {
    const turnId = adapter.turnKey(record.payload);
    if (!isText(turnId)) continue;
    const key = `${record.payload.session_id}\n${turnId}`;
    const turn = turns.get(key) ?? { session_id: record.payload.session_id, turn_id: turnId, ending: 'unterminated', last_event: null };
    const marker = adapter.events[record.payload.hook_event_name].turn;
    turn.last_event = record.payload.hook_event_name;
    if (marker !== 'open') turn.ending = marker;
    turns.set(key, turn);
  }
  return [...turns.values()];
}

// Tool input and output text is not carried in M0: how much to excerpt is undecided.
export function toolBody(payload, extra = {}) {
  return {
    tool_name: payload.tool_name,
    tool_use_id: payload.tool_use_id,
    input_keys: isObject(payload.tool_input) ? Object.keys(payload.tool_input).sort() : [],
    ...extra,
  };
}
