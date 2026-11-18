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
// Returned when the registrations cannot be trusted to answer for a folder.
const CONFLICT = Object.freeze({ canonicalPath: null, decision: 'conflict', projectId: null });

// ---- paths -------------------------------------------------------------------
//
// Windows decides whether two spellings name one folder; a string function can only
// imitate that, and every imitation so far either let an excluded folder through under
// another spelling or let a different folder borrow a registration. So this module does
// not interpret paths. It accepts a path only in canonical form and compares names:
//
//   accepted   `C:\a\b`, `\\server\share\a`; `/` for `\`; one trailing separator
//   refused    `.` and `..` names, empty names (doubled separators), names ending in a
//              dot or space, names with `< > : " | ? *` or control characters,
//              `\\?\` and `\\.\` forms, drive-relative `D:` and `D:x`, relative paths
//
// The caller canonicalises with the file system (junctions, symlinks, worktrees, 8.3
// short names, `\\?\`) before a path gets here. A refused path is `malformed`.
function parsePath(value) {
  if (!isText(value)) return null;
  const text = value.replace(/\//g, '\\');
  let parts;
  let rest;
  const drive = /^([A-Za-z]):\\/.exec(text);
  const unc = /^\\\\([^\\]+)\\([^\\]+)(?:\\|$)/.exec(text);
  if (drive) {
    parts = [`${drive[1]}:`];
    rest = text.slice(3);
  } else if (unc) {
    if (!isPlainName(unc[1]) || !isPlainName(unc[2])) return null;
    parts = [`\\\\${unc[1]}`, unc[2]];
    rest = text.slice(unc[0].length);
  } else {
    return null;
  }
  if (rest.endsWith('\\')) rest = rest.slice(0, -1);
  for (const name of rest === '' ? [] : rest.split('\\')) {
    if (!isPlainName(name)) return null;
    parts.push(name);
  }
  return parts;
}

function isPlainName(name) {
  // eslint-disable-next-line no-control-regex
  return name !== '' && name !== '.' && name !== '..' && !/[. ]$/.test(name) && !/[<>:"|?*\u0000-\u001f]/.test(name);
}

// Two ways to compare names, because the two possible mistakes are not equally safe.
// To CONFIRM a folder a false match is the danger, so only ASCII letters are folded and
// every other character must be identical. To EXCLUDE (or to notice an unconfirmed or
// conflicting registration) a missed match is the danger, so names are folded as widely
// as JavaScript can: compatibility normalisation plus full upper and lower casing.
const strictKey = parts => parts.map(name => name.replace(/[A-Z]/g, letter => letter.toLowerCase()));
const looseKey = parts => parts.map(name => name.normalize('NFKC').toUpperCase().toLowerCase());
const isPrefix = (base, target) => base.length <= target.length && base.every((name, index) => name === target[index]);

// Returns the registration that answers for `cwd`, or null when none does.
//
// The deepest registered ancestor (or the folder itself) answers, so an excluded
// subfolder overrides its registered parent, and a sibling that merely shares a prefix
// (`Repo-test` vs `Repo`) is not an ancestor. Ancestors are found with the loose
// comparison. Among registrations at that depth the most restrictive wins whatever
// their order: an exclusion, then an unconfirmed decision. A confirmed registration
// answers only if it also matches strictly; if it does not, or if confirmed
// registrations name different projects, the result is a conflict and yields no body.
// One registration that is not in canonical form makes every answer a conflict: it
// might be the exclusion that applies.
export function resolveRegistration(cwd, registrations) {
  const target = parsePath(cwd);
  if (target === null) return null;
  const parsed = [];
  for (const registration of registrations ?? []) {
    const base = parsePath(registration?.canonicalPath);
    if (base === null) return CONFLICT;
    parsed.push({ registration, base });
  }
  const targetLoose = looseKey(target);
  let depth = -1;
  let top = [];
  for (const item of parsed) {
    if (!isPrefix(looseKey(item.base), targetLoose)) continue;
    if (item.base.length > depth) {
      depth = item.base.length;
      top = [item];
    } else if (item.base.length === depth) {
      top.push(item);
    }
  }
  if (top.length === 0) return null;
  const excluded = top.find(item => item.registration.decision === 'exclude');
  if (excluded) return excluded.registration;
  const unconfirmed = top.find(item => !CONFIRMED.includes(item.registration.decision));
  if (unconfirmed) return unconfirmed.registration;
  const targetStrict = strictKey(target);
  if (!top.every(item => isPrefix(strictKey(item.base), targetStrict))) return CONFLICT;
  if (new Set(top.map(item => item.registration.projectId)).size > 1) return CONFLICT;
  return top[0].registration;
}

// ---- records -------------------------------------------------------------------

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
    && isText(payload.session_id) && isText(payload.hook_event_name) && parsePath(payload.cwd) !== null;
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
  // Not registered, not confirmed, or in conflict: the path only, never a body.
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
