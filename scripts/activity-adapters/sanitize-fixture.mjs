#!/usr/bin/env node
// Turns raw CaptureRecords from the M0 lab into fixtures that are safe to commit:
// the real home folder, the lab folder and every session/UUID-shaped id are replaced
// by fixed placeholders. Event structure and field names are left as observed.
import { copyFileSync, existsSync, mkdirSync, readFileSync, readdirSync, renameSync, rmSync, statSync, writeFileSync } from 'node:fs';
import { homedir } from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const HOME_PLACEHOLDER = 'C:\\Users\\lab-user';
const LAB_PLACEHOLDER = 'C:\\lab\\m0-sample';
const UUID = /[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}/gi;
const SAMPLE_FILE = /^\d{2,}-.*\.json$/;
// Replacements are written as control-character tokens first and expanded at the very
// end, so a later rule can never rewrite text that an earlier rule put in.
const TOKENS = { lab: ['\u0010', '\u0011', '\u0012'], home: ['\u0013', '\u0014', '\u0015'], labFolder: '\u0016', homeName: '\u0017' };

const escapeRegExp = text => text.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
// A name standing on its own: not part of a longer word in any script.
const wholeWord = (name, flags) => new RegExp(`(?<![\\p{L}\\p{N}_])${escapeRegExp(name)}(?![\\p{L}\\p{N}_])`, flags);

// A Windows path can appear with doubled backslashes (a JSON document embedded in a
// string), with backslashes, or with forward slashes.
function pathForms(value) {
  const backslash = value.replace(/\//g, '\\').replace(/\\+$/, '');
  return [backslash.replace(/\\/g, '\\\\'), backslash, backslash.replace(/\\/g, '/')];
}

// Claude Code names a project's transcript folder after its path, with every character
// that is not a letter or digit turned into `-`.
const folderName = value => value.replace(/\//g, '\\').replace(/\\+$/, '').replace(/[^A-Za-z0-9]/g, '-');

function replacePath(text, from, tokens) {
  return pathForms(from).reduce(
    (current, form, index) => current.replace(new RegExp(escapeRegExp(form), 'gi'), () => tokens[index]), text);
}

function expand(text) {
  let current = text;
  pathForms(LAB_PLACEHOLDER).forEach((form, index) => { current = current.replaceAll(TOKENS.lab[index], form); });
  pathForms(HOME_PLACEHOLDER).forEach((form, index) => { current = current.replaceAll(TOKENS.home[index], form); });
  return current.replaceAll(TOKENS.labFolder, folderName(LAB_PLACEHOLDER)).replaceAll(TOKENS.homeName, 'lab-user');
}

function mapStrings(value, onValue, onKey = key => key) {
  if (typeof value === 'string') return onValue(value);
  if (Array.isArray(value)) return value.map(item => mapStrings(item, onValue, onKey));
  if (value && typeof value === 'object') {
    return Object.fromEntries(Object.entries(value).map(([key, item]) => [onKey(key), mapStrings(item, onValue, onKey)]));
  }
  return value;
}

// Session ids are opaque and case-sensitive; UUIDs are not. They are kept apart in the
// table so `Session-A` and `session-a` never share a placeholder.
function placeholder(ids, key, prefix) {
  const known = ids.get(key);
  if (known) return known;
  const count = [...ids.values()].filter(item => item.startsWith(`${prefix}-`)).length + 1;
  const created = `${prefix}-${String(count).padStart(4, '0')}`;
  ids.set(key, created);
  return created;
}

export function sanitizeCapture(record, { homeDir, labRoot, sessionIds }) {
  const homeName = path.win32.basename(homeDir.replace(/\//g, '\\'));
  // Besides the full home path, the account name is replaced only where it is a profile
  // folder: right after `Users` and a separator, including the `C--Users-name-…` form
  // Claude Code derives from a path. The bare word is never replaced, because an account
  // can be called `user`, `tool` or `session` and those words are part of the schema.
  // The command refuses to write a fixture that still shows the account name as a word.
  const profileFolder = new RegExp(`(users[\\\\/-]+)${escapeRegExp(homeName)}(?![\\p{L}\\p{N}])`, 'giu');
  const paths = text => {
    let current = replacePath(text, labRoot, TOKENS.lab);
    current = current.replace(new RegExp(escapeRegExp(folderName(labRoot)), 'gi'), () => TOKENS.labFolder);
    current = replacePath(current, homeDir, TOKENS.home);
    current = current.replace(profileFolder, (match, lead) => `${lead}${TOKENS.homeName}`);
    return expand(current);
  };
  // Field names are the schema the adapters read, so paths are replaced in values only.
  const cleaned = mapStrings(record, paths);
  if (!cleaned.payload || typeof cleaned.payload !== 'object') return cleaned;

  const sessionId = typeof cleaned.payload.session_id === 'string' && cleaned.payload.session_id ? cleaned.payload.session_id : null;
  if (sessionId) {
    // An id that is also one of the record's own field names or its event name cannot be
    // told apart from the schema, and replacing it would break the record. Refuse instead
    // of guessing. Keys deeper inside the payload are data, not schema: there a key equal
    // to the id is how a tool indexes by session, and it is replaced.
    const reserved = new Set([cleaned.source, cleaned.origin, cleaned.payload.hook_event_name,
      ...Object.keys(cleaned), ...Object.keys(cleaned.payload)]);
    if (reserved.has(sessionId)) {
      throw new Error('the session id of a record is also a field name or event name and cannot be replaced safely');
    }
    placeholder(sessionIds, `session:${sessionId}`, 'session');
  }
  // Every session id the table knows is replaced, not only this record's: a later
  // session can mention an earlier one. Longer ids first, so an id that contains
  // another is replaced whole.
  const known = [...sessionIds.entries()].filter(([key]) => key.startsWith('session:'))
    .map(([key, value]) => ({ id: key.slice('session:'.length), value, token: wholeWord(key.slice('session:'.length), 'gu') }))
    .sort((a, b) => b.id.length - a.id.length);
  const uuids = text => text.replace(UUID, match => placeholder(sessionIds, `uuid:${match.toLowerCase()}`, 'id'));
  // In a value an id is replaced wherever it stands as a whole token, never inside a
  // longer word (`Tool` must not rewrite `PostToolUse`). A key is replaced only when
  // the whole key is an id, which is how tools index data by session.
  const inValue = text => uuids(known.reduce((current, item) => current.replace(item.token, () => item.value), text));
  const inKey = key => known.find(item => item.id === key)?.value ?? uuids(key);
  return { ...cleaned, payload: mapStrings(cleaned.payload, inValue, inKey) };
}

// True when `word` matches any string value or any key. The check runs on the values
// themselves, not on serialised JSON: there a line break becomes a backslash and an
// `n`, and that `n` would hide a name standing at the start of a line.
function anyText(value, word) {
  if (typeof value === 'string') return word.test(value.replaceAll('lab-user', ''));
  if (Array.isArray(value)) return value.some(item => anyText(item, word));
  if (value && typeof value === 'object') {
    return Object.entries(value).some(([key, item]) => word.test(key.replaceAll('lab-user', '')) || anyText(item, word));
  }
  return false;
}

// `%USERPROFILE%` (left unexpanded by PowerShell) or a bare name would be accepted as a
// folder and silently replace nothing, leaving the real profile path in the fixtures.
function requireAbsolute(name, value) {
  if (!/^([A-Za-z]:[\\/]|\\\\[^\\]+\\[^\\]+)/.test(value) || /[%$]/.test(value)) {
    throw new Error(`--${name} must be an absolute folder path, got "${value}"`);
  }
  return value;
}

function parseArgs(argv) {
  const options = {};
  for (let index = 0; index < argv.length; index += 2) {
    if (!['--in', '--out', '--home', '--lab', '--ids'].includes(argv[index]) || argv[index + 1] === undefined) {
      throw new Error(`unexpected argument ${argv[index]}`);
    }
    options[argv[index].slice(2)] = argv[index + 1];
  }
  for (const name of ['in', 'out', 'lab']) if (!options[name]) throw new Error(`--${name} is required`);
  options.home = requireAbsolute('home', options.home ?? homedir());
  requireAbsolute('lab', options.lab);
  return options;
}

// Replaces the samples of one scenario as a whole. The new set is built in a folder
// beside the old one (files that are not samples, such as expected.json, are carried
// over), then the folders are swapped with two renames. If the new folder cannot be
// moved in, the old one is put back, so a failure never leaves the scenario empty or
// half replaced.
export function swapScenario(out, outputs, { rename = renameSync } = {}) {
  const parent = path.dirname(path.resolve(out));
  const base = path.basename(path.resolve(out));
  const staging = path.join(parent, `.${base}.new-${process.pid}`);
  const retired = path.join(parent, `.${base}.old-${process.pid}`);
  mkdirSync(parent, { recursive: true });
  rmSync(staging, { recursive: true, force: true });
  rmSync(retired, { recursive: true, force: true });
  // A scenario folder holds files only. A subfolder means `--out` points at something
  // else (for example the folder of all scenarios), and replacing it would delete them.
  if (existsSync(out) && readdirSync(out).some(name => statSync(path.join(out, name)).isDirectory())) {
    throw new Error(`${out} has a subfolder; --out must be the folder of one scenario`);
  }
  mkdirSync(staging);
  try {
    if (existsSync(out)) {
      for (const name of readdirSync(out)) {
        if (!SAMPLE_FILE.test(name)) copyFileSync(path.join(out, name), path.join(staging, name));
      }
    }
    for (const output of outputs) writeFileSync(path.join(staging, output.name), output.text);
    if (existsSync(out)) {
      rename(out, retired);
      try {
        rename(staging, out);
      } catch (error) {
        renameSync(retired, out);
        throw error;
      }
      rmSync(retired, { recursive: true, force: true });
    } else {
      rename(staging, out);
    }
  } catch (error) {
    rmSync(staging, { recursive: true, force: true });
    throw error;
  }
}

function main() {
  const options = parseArgs(process.argv.slice(2));
  // Read and convert everything first. Nothing on disk changes until every input,
  // including the id table, was read and every check below passed.
  const records = readdirSync(options.in)
    .filter(name => name.endsWith('.json'))
    .map(name => JSON.parse(readFileSync(path.join(options.in, name), 'utf8')))
    .sort((a, b) => `${a.captured_at}|${a.capture_id}`.localeCompare(`${b.captured_at}|${b.capture_id}`));
  // An empty folder usually means the hooks never ran. Replacing a scenario with nothing
  // would delete its samples and still report success.
  if (records.length === 0) throw new Error(`no capture records in ${options.in}; existing fixtures were left as they are`);

  // With --ids the replacement table is kept between runs, so one real session keeps one
  // placeholder across scenarios and different sessions never share one. The file holds
  // real ids and must stay outside the repository.
  const sessionIds = new Map(options.ids && existsSync(options.ids) ? JSON.parse(readFileSync(options.ids, 'utf8')) : []);
  const outputs = records.map((record, index) => {
    const event = String(record.payload?.hook_event_name ?? 'unknown').replace(/[^A-Za-z0-9_]/g, '') || 'unknown';
    const cleaned = sanitizeCapture(record, { homeDir: options.home, labRoot: options.lab, sessionIds });
    return { name: `${String(index + 1).padStart(2, '0')}-${event}.json`, cleaned, text: `${JSON.stringify(cleaned, null, 2)}\n` };
  });

  // The account the capture came from is known only here, not to the tests that run
  // later under whatever account checks out the repository. A name left standing as a
  // word (in a prompt, a tool output) needs a person: edit or drop the raw record.
  const homeName = path.win32.basename(options.home.replace(/\//g, '\\'));
  const accountWord = wholeWord(homeName, 'iu');
  for (const output of outputs) {
    if (anyText(output.cleaned, accountWord)) {
      throw new Error(`${output.name} would still contain the account name as a word; nothing was written`);
    }
  }

  // The id table is saved first. If that fails nothing was replaced; if the swap fails
  // afterwards the table only knows a few ids more, which keeps later runs consistent.
  // The other order could install samples whose placeholders the table has forgotten.
  if (options.ids) {
    const temporary = `${options.ids}.tmp`;
    writeFileSync(temporary, `${JSON.stringify([...sessionIds], null, 2)}\n`);
    renameSync(temporary, options.ids);
  }
  swapScenario(options.out, outputs);
  process.stdout.write(`${records.length} fixture file(s) written to ${options.out}\n`);
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    main();
  } catch (error) {
    process.stderr.write(`sanitize-fixture: ${error?.message ?? error}\n`);
    process.exitCode = 1;
  }
}
