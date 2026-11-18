// Shared rules for turning one CaptureRecord (see probe-hook.mjs) into a result the
// activity pipeline can act on. Pure functions: no file system, no network, no clock.
//
// The order of the checks is the contract. Earlier checks win, and every check fails
// closed: a record yields a body only when its origin is exactly `tool`, its client
// version and event are verified, and its folder is registered as `new` or `link`.

const isText = value => typeof value === 'string' && value.length > 0;
const isObject = value => value !== null && typeof value === 'object' && !Array.isArray(value);
const ORIGINS = ['tool', 'timeschedule_generation'];
const CONFIRMED = ['new', 'link'];

// Comparison form of a fully qualified Windows path, or null when the text is not one.
// Two spellings of the same folder must compare equal, otherwise an excluded folder
// could be reached under another spelling, and two different folders must never
// compare equal, otherwise an unregistered folder would inherit a registration.
//
// Ordinary paths (`C:\…`, `\\server\share\…`): case, `/` vs `\`, repeated or trailing
// separators, `.` and `..` segments and trailing dots and spaces (which Windows drops)
// are folded.
// Extended paths (`\\?\C:\…`, `\\?\UNC\…`): Windows takes every segment literally, so
// `repo.` is a different folder from `repo`. Only case is folded; a path with an empty,
// `.` or `..` segment or a `/` is rejected.
// `D:` and `D:x` are relative to the drive's current folder and name no fixed folder;
// `\\.\…` is a device. Both are rejected.
// What needs the file system is not handled: junctions, symlinks, worktrees and 8.3
// short names are resolved by the caller before registration.
function comparable(value) {
  if (!isText(value)) return null;
  let text = value;
  let literal = false;
  if (/^\\\\\?\\UNC\\/i.test(text)) { text = `\\\\${text.slice(8)}`; literal = true; }
  else if (text.startsWith('\\\\?\\')) { text = text.slice(4); literal = true; }
  if (literal && text.includes('/')) return null;
  text = text.replace(/\//g, '\\');
  let root;
  let rest;
  const drive = /^([A-Za-z]):\\/.exec(text);
  const unc = /^\\\\([^\\?.][^\\]*)\\([^\\]+)(\\|$)/.exec(text);
  if (drive) {
    root = `${drive[1].toLowerCase()}:`;
    rest = text.slice(3);
  } else if (unc) {
    root = `\\\\${unc[1].toLowerCase()}\\${unc[2].toLowerCase()}`;
    rest = text.slice(unc[1].length + unc[2].length + 3);
    if (rest.startsWith('\\')) rest = rest.slice(1);
  } else {
    return null;
  }
  const segments = [];
  if (literal) {
    const parts = rest.replace(/\\$/, '') === '' ? [] : rest.replace(/\\$/, '').split('\\');
    for (const raw of parts) {
      if (raw === '' || raw === '.' || raw === '..') return null;
      segments.push(raw.toLowerCase());
    }
  } else {
    for (const raw of rest.split('\\')) {
      if (raw === '..') { segments.pop(); continue; }
      const segment = raw.replace(/[. ]+$/, '').toLowerCase();
      if (segment) segments.push(segment);
    }
  }
  return [root, ...segments].join('\\');
}

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
    && isText(payload.session_id) && isText(payload.hook_event_name) && comparable(payload.cwd) !== null;
}

// Returns the registration whose path equals `cwd` or is an ancestor folder of it.
// The longest match wins, so an excluded subfolder overrides its registered parent.
// A sibling that merely shares a prefix (`Repo-test` vs `Repo`) does not match.
//
// Several registrations can name the same folder (spelled differently, or entered
// twice). The answer must not depend on their order, so the most restrictive one wins:
// an exclusion, then an unconfirmed decision. Confirmed registrations that point the
// same folder at different projects are reported as a conflict, which yields no body.
export function resolveRegistration(cwd, registrations) {
  const target = comparable(cwd);
  if (target === null) return null;
  let matches = [];
  let bestLength = -1;
  for (const registration of registrations ?? []) {
    const base = comparable(registration?.canonicalPath);
    if (base === null || (target !== base && !target.startsWith(`${base}\\`))) continue;
    if (base.length > bestLength) {
      matches = [registration];
      bestLength = base.length;
    } else if (base.length === bestLength) {
      matches.push(registration);
    }
  }
  if (matches.length === 0) return null;
  const excluded = matches.find(registration => registration.decision === 'exclude');
  if (excluded) return excluded;
  const unconfirmed = matches.find(registration => !CONFIRMED.includes(registration.decision));
  if (unconfirmed) return unconfirmed;
  if (new Set(matches.map(registration => registration.projectId)).size > 1) {
    return { canonicalPath: matches[0].canonicalPath, decision: 'conflict', projectId: null };
  }
  return matches[0];
}

export function normalizeCapture(record, context, adapter) {
  if (!isWellFormed(record, adapter)) return { outcome: 'unsupported', reason: 'malformed' };
  const payload = record.payload;
  if (record.origin === 'timeschedule_generation') return { outcome: 'excluded', reason: 'generation_origin' };
  if (!adapter.verifiedVersions.includes(record.tool_version)) return { outcome: 'unsupported', reason: 'unknown_version' };
  if (!Object.hasOwn(adapter.events, payload.hook_event_name)) return { outcome: 'unsupported', reason: 'unknown_event' };
  const event = adapter.events[payload.hook_event_name];
  if (event.skip) return { outcome: 'excluded', reason: event.skip };

  const registration = resolveRegistration(payload.cwd, context?.registrations);
  if (registration?.decision === 'exclude') return { outcome: 'excluded', reason: 'path_excluded' };
  // Not registered, or registered without a confirmed decision: the path only, never a body.
  if (!registration || !CONFIRMED.includes(registration.decision)) {
    return { outcome: 'candidate', candidate: { source: adapter.source, path: payload.cwd } };
  }

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
//   unterminated - turn activity was captured but no usable terminal hook was found
// `unterminated` is what a killed process leaves behind. Hooks alone cannot tell it from
// a turn that is still running, so the caller must know the session is over before it
// treats the turn as lost.
//
// The same gates as normalizeCapture apply (origin, version, event, confirmed folder),
// so the tool's own generation runs and unconfirmed folders never appear here. Skipped
// events are read too: a PreToolUse with nothing after it is the only trace of an
// interrupted tool call. Only event names and ids are read, never a body.
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
    const registration = resolveRegistration(record.payload.cwd, context?.registrations);
    return Boolean(registration) && CONFIRMED.includes(registration.decision);
  }).sort((a, b) => (a.captured_at < b.captured_at ? -1 : a.captured_at > b.captured_at ? 1
    : a.capture_id < b.capture_id ? -1 : a.capture_id > b.capture_id ? 1 : 0));

  const turns = new Map();
  for (const record of usable) {
    const turnId = adapter.turnKey(record.payload);
    if (!isText(turnId)) continue;
    const key = `${record.payload.session_id}\n${turnId}`;
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
