#!/usr/bin/env node
// M0 lab probe. A hook command for Claude Code or Codex that stores the hook's
// stdin as one CaptureRecord file. It is an observation tool for an isolated test
// folder, not the product's capture path: it applies no path consent and no filter.
//
// It never writes to stdout (both tools can feed hook stdout back into the
// conversation) and never exits with code 2 (both tools treat 2 as "block").
import { randomUUID } from 'node:crypto';
import { mkdirSync, renameSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const MAX_PAYLOAD_BYTES = 1048576;
const SOURCES = new Set(['claude-code', 'codex']);
// Values are recorded only for these names: they identify the surface and client build
// and carry no credential. Every other CLAUDE*/CODEX* variable is recorded by name only.
const ENV_VALUE_ALLOWLIST = [
  'CLAUDE_CODE_ENTRYPOINT', 'CLAUDE_CODE_REMOTE', 'CLAUDECODE',
  'CLAUDE_CODE_DESKTOP_APP_VERSION', 'CLAUDE_AGENT_SDK_VERSION',
];

function parseArgs(argv) {
  const options = {};
  for (let index = 0; index < argv.length; index += 2) {
    const name = argv[index];
    const value = argv[index + 1];
    if (!['--source', '--out', '--tool-version'].includes(name) || value === undefined) {
      throw new Error(`unexpected argument ${name}`);
    }
    options[name.slice(2)] = value;
  }
  if (!SOURCES.has(options.source)) throw new Error('--source must be claude-code or codex');
  if (!options.out) throw new Error('--out is required');
  return options;
}

// Reads a stream to its end but keeps at most `limit` bytes. Past the limit the input
// is still drained (so the tool writing to the hook is never blocked) and counted, but
// nothing is held, so memory does not grow with the size of a tool's output.
export async function readBounded(stream, limit, onHeld = () => {}) {
  let chunks = [];
  let held = 0;
  let total = 0;
  for await (const chunk of stream) {
    total += chunk.length;
    if (total <= limit) {
      chunks.push(chunk);
      held += chunk.length;
    } else {
      chunks = [];
      held = 0;
    }
    onHeld(held);
  }
  return { bytes: total, buffer: total <= limit ? Buffer.concat(chunks) : null };
}

function parsePayload(buffer) {
  if (buffer === null) return { payload: null, payload_error: 'too_large' };
  try {
    const value = JSON.parse(buffer.toString('utf8'));
    if (value === null || typeof value !== 'object' || Array.isArray(value)) return { payload: null, payload_error: 'invalid_json' };
    return { payload: value, payload_error: null };
  } catch {
    return { payload: null, payload_error: 'invalid_json' };
  }
}

async function main() {
  const options = parseArgs(process.argv.slice(2));
  const input = await readBounded(process.stdin, MAX_PAYLOAD_BYTES);
  const capturedAt = new Date().toISOString();
  const captureId = randomUUID();
  const envNames = Object.keys(process.env).filter(name => /^(CLAUDE|CODEX)/i.test(name)).sort();
  const envAllowlisted = {};
  for (const name of ENV_VALUE_ALLOWLIST) if (process.env[name] !== undefined) envAllowlisted[name] = process.env[name];
  const record = {
    capture_schema: 1,
    capture_id: captureId,
    source: options.source,
    tool_version: options['tool-version'] ?? null,
    captured_at: capturedAt,
    origin: process.env.TIMESCHEDULE_ORIGIN === 'generation' ? 'timeschedule_generation' : 'tool',
    payload_bytes: input.bytes,
    ...parsePayload(input.buffer),
    env_names: envNames,
    env_allowlisted: envAllowlisted,
    probe_elapsed_ms: 0,
  };
  mkdirSync(options.out, { recursive: true });
  const name = `${capturedAt.replace(/\D/g, '')}-${captureId}.json`;
  const temporary = path.join(options.out, `.${name}.tmp`);
  record.probe_elapsed_ms = Math.round(performance.now());
  writeFileSync(temporary, `${JSON.stringify(record, null, 2)}\n`, { flag: 'wx' });
  renameSync(temporary, path.join(options.out, name));
}

// Run only as a command; importing the module (the tests do) must not read stdin.
if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main().catch(error => {
    process.stderr.write(`probe-hook: ${error?.message ?? error}\n`);
    process.exitCode = 1;
  });
}
