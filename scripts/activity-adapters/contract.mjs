// Shared rules for turning one CaptureRecord (see probe-hook.mjs) into a result the
// activity pipeline can act on. Pure functions: no file system, no network, no clock.
//
// The order of the checks is the contract. Earlier checks win, and every check fails
// closed: a record yields a body only when its origin is exactly `tool`, its client
// version and event are verified, the event carries the fields it was observed with,
// and the caller says its folder is confirmed.
//
// Which project a folder belongs to is NOT decided here. Whether two spellings name one
// folder is a property of the file system (case sensitivity is set per directory;
// junctions, symlinks, worktrees and short names need a lookup), and a function that
// only sees strings cannot get it right in both directions: every attempt either let an
// excluded folder through under another spelling or let a different folder borrow a
// registration. So the caller resolves the folder, with the file system, and tells the
// adapter the decision through `context.registrationFor(cwd)`:
//
//   { decision: 'new' | 'link', projectId }  the folder is confirmed for that project
//   { decision: 'exclude' }                  the folder is excluded
//   anything else, or no answer             not confirmed: path candidate only, no body

const isText = value => typeof value === 'string' && value.length > 0;
const isObject = value => value !== null && typeof value === 'object' && !Array.isArray(value);
const ORIGINS = ['tool', 'timeschedule_generation'];
const CONFIRMED = ['new', 'link'];

// The probe writes UTC timestamps with milliseconds; anything else is a damaged record.
function isCaptureTime(value) {
  if (!isText(value) || !/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{3}Z$/.test(value)) return false;
  const time = Date.parse(value);
  return !Number.isNaN(time) && new Date(time).toISOString() === value;
}

function isWellFormed(record, adapter) {
  const payload = record?.payload;
  return isObject(record) && record.capture_schema === 1 && record.source === adapter.source
    && ORIGINS.includes(record.origin)
    && isText(record.capture_id) && isCaptureTime(record.captured_at) && isObject(payload)
    && isText(payload.session_id) && isText(payload.hook_event_name) && isText(payload.cwd);
}

// A verified event must still look like the samples it was verified with: the string
// fields its body is built from, and, for an event that belongs to a turn, the turn id.
function hasObservedShape(payload, event, adapter) {
  if (!(event.requires ?? []).every(name => typeof payload[name] === 'string')) return false;
  return !event.turn || isText(adapter.turnKey(payload));
}

// 'confirmed' | 'excluded' | 'unconfirmed', from the caller's answer for this folder.
function folderDecision(context, cwd) {
  if (typeof context?.registrationFor !== 'function') return { state: 'unconfirmed' };
  const answer = context.registrationFor(cwd);
  if (!isObject(answer)) return { state: 'unconfirmed' };
  if (answer.decision === 'exclude') return { state: 'excluded' };
  if (CONFIRMED.includes(answer.decision) && isText(answer.projectId)) return { state: 'confirmed', projectId: answer.projectId };
  return { state: 'unconfirmed' };
}

export function normalizeCapture(record, context, adapter) {
  if (!isWellFormed(record, adapter)) return { outcome: 'unsupported', reason: 'malformed' };
  const payload = record.payload;
  if (record.origin === 'timeschedule_generation') return { outcome: 'excluded', reason: 'generation_origin' };
  if (!adapter.verifiedVersions.includes(record.tool_version)) return { outcome: 'unsupported', reason: 'unknown_version' };
  if (!Object.hasOwn(adapter.events, payload.hook_event_name)) return { outcome: 'unsupported', reason: 'unknown_event' };
  const event = adapter.events[payload.hook_event_name];
  if (!hasObservedShape(payload, event, adapter)) return { outcome: 'unsupported', reason: 'malformed' };
  if (event.skip) return { outcome: 'excluded', reason: event.skip };

  const folder = folderDecision(context, payload.cwd);
  if (folder.state === 'excluded') return { outcome: 'excluded', reason: 'path_excluded' };
  // The folder as the tool reported it, for the caller to ask the user about. No body.
  if (folder.state !== 'confirmed') return { outcome: 'candidate', candidate: { source: adapter.source, path: payload.cwd } };

  return {
    outcome: 'activity',
    envelope: {
      schema_version: 1,
      source: adapter.source,
      session_id: payload.session_id,
      // Issued once by the probe and stored with the record, so resending the stored
      // record is recognisable. Running the hook again is a new event with a new id.
      event_id: record.capture_id,
      revision: 1,
      project_id: folder.projectId,
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
//   unterminated - turn activity was captured but no usable terminal hook was found
// `unterminated` is what a killed process leaves behind. Hooks alone cannot tell it from
// a turn that is still running, so the caller must know the session is over before it
// treats the turn as lost.
//
// The same gates as normalizeCapture apply (origin, version, event, observed shape,
// folder confirmed by the caller), so the tool's own generation runs and unconfirmed
// folders never appear here. Skipped events are read too: a PreToolUse with nothing
// after it is the only trace of an interrupted tool call. Only event names and ids are
// read, never a body.
//
// The answer does not depend on the order of `records`. The terminal hook with the
// latest capture time decides; if a Stop and an Interrupt share that time the turn is
// reported as interrupted rather than claimed as completed. `last_event` is that
// terminal hook, or the latest turn hook when there is none.
export function classifyTurns(records, context, adapter) {
  const usable = (records ?? []).filter(record => {
    if (!isWellFormed(record, adapter) || record.origin !== 'tool') return false;
    if (!adapter.verifiedVersions.includes(record.tool_version)) return false;
    const name = record.payload.hook_event_name;
    if (!Object.hasOwn(adapter.events, name) || !adapter.events[name].turn) return false;
    if (!hasObservedShape(record.payload, adapter.events[name], adapter)) return false;
    return folderDecision(context, record.payload.cwd).state === 'confirmed';
  }).sort((a, b) => (a.captured_at < b.captured_at ? -1 : a.captured_at > b.captured_at ? 1
    : a.capture_id < b.capture_id ? -1 : a.capture_id > b.capture_id ? 1 : 0));

  const turns = new Map();
  for (const record of usable) {
    const turnId = adapter.turnKey(record.payload);
    // Encoded as a pair, so no character inside an id can make two turns look like one.
    const key = JSON.stringify([record.payload.session_id, turnId]);
    const turn = turns.get(key)
      ?? { session_id: record.payload.session_id, turn_id: turnId, ending: 'unterminated', last_event: null, terminalAt: null };
    const name = record.payload.hook_event_name;
    const marker = adapter.events[name].turn;
    if (marker === 'open') {
      if (turn.terminalAt === null) turn.last_event = name;
    } else if (turn.terminalAt === null || record.captured_at > turn.terminalAt
      || (record.captured_at === turn.terminalAt && marker === 'interrupted')) {
      turn.ending = marker;
      turn.last_event = name;
      turn.terminalAt = record.captured_at;
    }
    turns.set(key, turn);
  }
  return [...turns.values()].map(({ terminalAt, ...turn }) => turn);
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
