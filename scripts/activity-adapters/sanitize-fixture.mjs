#!/usr/bin/env node
// Turns raw CaptureRecords from the M0 lab into fixtures that are safe to commit:
// the real home folder, the lab folder and every session/UUID-shaped id are replaced
// by fixed placeholders. Event structure and field names are left as observed.
import { existsSync, mkdirSync, readFileSync, readdirSync, renameSync, rmSync, writeFileSync } from 'node:fs';
import { homedir } from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const HOME_PLACEHOLDER = 'C:\\Users\\lab-user';
const LAB_PLACEHOLDER = 'C:\\lab\\m0-sample';
const UUID = /[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}/gi;
// Replacements are written as control-character tokens first and expanded at the very
// end, so a later rule can never rewrite text that an earlier rule put in.
const TOKENS = { lab: ['\u0010', '\u0011', '\u0012'], home: ['\u0013', '\u0014', '\u0015'], labFolder: '\u0016', homeName: '\u0017' };

const escapeRegExp = text => text.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');

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

function mapStrings(value, transform) {
  if (typeof value === 'string') return transform(value);
  if (Array.isArray(value)) return value.map(item => mapStrings(item, transform));
  if (value && typeof value === 'object') {
    return Object.fromEntries(Object.entries(value).map(([key, item]) => [transform(key), mapStrings(item, transform)]));
  }
  return value;
}

function placeholder(ids, key, prefix) {
  const known = ids.get(key.toLowerCase());
  if (known) return known;
  const count = [...ids.values()].filter(item => item.startsWith(`${prefix}-`)).length + 1;
  const created = `${prefix}-${String(count).padStart(4, '0')}`;
  ids.set(key.toLowerCase(), created);
  return created;
}

export function sanitizeCapture(record, { homeDir, labRoot, sessionIds }) {
  const homeName = path.win32.basename(homeDir.replace(/\//g, '\\'));
  // The account name is replaced only where it stands as a whole name, so an account
  // called `user` does not rewrite `UserPromptSubmit`.
  const wholeHomeName = new RegExp(`(?<![A-Za-z0-9])${escapeRegExp(homeName)}(?![A-Za-z0-9])`, 'gi');
  const paths = text => {
    let current = replacePath(text, labRoot, TOKENS.lab);
    current = current.replace(new RegExp(escapeRegExp(folderName(labRoot)), 'gi'), () => TOKENS.labFolder);
    current = replacePath(current, homeDir, TOKENS.home);
    if (homeName.length >= 3) current = current.replace(wholeHomeName, () => TOKENS.homeName);
    return expand(current);
  };
  const cleaned = mapStrings(record, paths);
  if (!cleaned.payload || typeof cleaned.payload !== 'object') return cleaned;
  const sessionId = typeof cleaned.payload.session_id === 'string' && cleaned.payload.session_id ? cleaned.payload.session_id : null;
  const sessionPlaceholder = sessionId ? placeholder(sessionIds, sessionId, 'session') : null;
  const identifiers = text => {
    const current = sessionId ? text.replace(new RegExp(escapeRegExp(sessionId), 'gi'), () => sessionPlaceholder) : text;
    return current.replace(UUID, match => placeholder(sessionIds, match, 'id'));
  };
  return { ...cleaned, payload: mapStrings(cleaned.payload, identifiers) };
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

function main() {
  const options = parseArgs(process.argv.slice(2));
  // Read and convert everything first. Existing fixtures are replaced only after every
  // input, including the id table, was read without error.
  const records = readdirSync(options.in)
    .filter(name => name.endsWith('.json'))
    .map(name => JSON.parse(readFileSync(path.join(options.in, name), 'utf8')))
    .sort((a, b) => `${a.captured_at}|${a.capture_id}`.localeCompare(`${b.captured_at}|${b.capture_id}`));
  // With --ids the replacement table is kept between runs, so one real session keeps one
  // placeholder across scenarios and different sessions never share one. The file holds
  // real ids and must stay outside the repository.
  const sessionIds = new Map(options.ids && existsSync(options.ids) ? JSON.parse(readFileSync(options.ids, 'utf8')) : []);
  const outputs = records.map((record, index) => {
    const event = String(record.payload?.hook_event_name ?? 'unknown').replace(/[^A-Za-z0-9_]/g, '') || 'unknown';
    const cleaned = sanitizeCapture(record, { homeDir: options.home, labRoot: options.lab, sessionIds });
    return { name: `${String(index + 1).padStart(2, '0')}-${event}.json`, text: `${JSON.stringify(cleaned, null, 2)}\n` };
  });

  mkdirSync(options.out, { recursive: true });
  for (const name of readdirSync(options.out)) {
    if (/^\d{2,}-.*\.json$/.test(name)) rmSync(path.join(options.out, name));
  }
  for (const output of outputs) writeFileSync(path.join(options.out, output.name), output.text);
  if (options.ids) {
    const temporary = `${options.ids}.tmp`;
    writeFileSync(temporary, `${JSON.stringify([...sessionIds], null, 2)}\n`);
    renameSync(temporary, options.ids);
  }
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
