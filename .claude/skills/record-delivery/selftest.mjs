// Proves the checker can fail. Each case breaks one rule in a scratch copy of
// scripts/project-records.mjs and expects check.mjs to report the properties
// that rule protects. Between them the cases make every property fail once.
// The real files are never changed.
//
//   node .claude/skills/record-delivery/selftest.mjs [name ...]
//
// Exit 0: every broken copy was caught. Exit 1: a break went unnoticed, or a
// case no longer applies because the code it edits has changed.
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const HERE = path.dirname(fileURLToPath(import.meta.url));
const REPO = path.resolve(HERE, '..', '..', '..');

// `find` (or each first element of `edits`) must occur exactly once in
// scripts/project-records.mjs. When it does not, the code moved: update the
// case to the new text of the same rule.
const CASES = [
  { name: 'unverified-capture-reserves-no-revision', expect: 'no-rollback', depth: 3,
    why: 'a capture Git could not check keeps no delivery revision, so it is numbered and delivered after a newer capture',
    find: `      const next = job.nextAttemptAt; toLocal(job); job.nextAttemptAt = next;\n      return true;`,
    replace: `      return false;` },
  { name: 'git-failure-read-as-no-evidence', expect: 'no-false-exclusion', depth: 2,
    why: 'a Git check that failed is treated as a finding that the work has no development changes',
    find: `      if (evidence.transient) {\n        // Nothing is known about this work yet`,
    replace: `      if (false) {\n        // Nothing is known about this work yet` },
  { name: 'paused-capture-never-resumes', expect: 'delivered-in-the-end', depth: 2,
    why: 'a capture paused after a long Git outage keeps its paused phase when Git answers again',
    find: `      if (resumed) { job.phase = job.pausedFromPhase ?? 'prepared'; delete job.pausedFromPhase; job.nextAttemptAt = 0; }`,
    replace: `` },
  { name: 'linked-delivery-reuses-local-revision', expect: 'one-payload-per-revision', depth: 2,
    why: 'the delivery that carries the Space link uses the same revision as the earlier local delivery',
    find: `revision:linked?job.finalRevision:job.localRevision`,
    replace: `revision:job.localRevision` },
  { name: 'any-error-recorded-as-a-conflict', expect: 'no-false-conflict', depth: 2,
    why: 'a delivery that failed for any reason is recorded as a conflict with the user',
    find: `        if (error.conflict) job.phase = 'conflict';`,
    replace: `        job.phase = 'conflict';` },
  { name: 'space-off-skips-the-diary', expect: 'newest-shown-in-the-end', depth: 2,
    why: 'with Space off a capture is marked complete without being delivered to the diary',
    find: `        if(!job.localSaved){if(!await importRecord(false))continue;`,
    replace: `        if(!job.localSaved && spaceEnabled){if(!await importRecord(false))continue;` },
  { name: 'new-capture-drops-the-waiting-one', expect: 'no-capture-lost', depth: 2,
    why: 'preparing a new capture no longer keeps the job of the capture that was still waiting',
    find: '      atomicJson(path.join(stateDir, source.workId, `pending-${previous.revision}.json`), previous);',
    replace: `` },
];

const wanted = process.argv.slice(2);
const cases = wanted.length ? CASES.filter(c => wanted.includes(c.name)) : CASES;
if (wanted.length && cases.length !== wanted.length) { console.error(`Unknown case. Known: ${CASES.map(c => c.name).join(', ')}`); process.exit(1); }

let failed = 0;
for (const test of cases) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'timeschedule-record-selftest-'));
  try {
    fs.cpSync(path.join(REPO, 'scripts'), path.join(root, 'scripts'), { recursive: true });
    fs.cpSync(path.join(REPO, 'tests', 'helpers'), path.join(root, 'tests', 'helpers'), { recursive: true });
    const file = path.join(root, 'scripts', 'project-records.mjs');
    let source = fs.readFileSync(file, 'utf8').replace(/\r\n/g, '\n');
    const edits = test.edits ?? [[test.find, test.replace]];
    const stale = edits.map(([find]) => source.split(find).length - 1).find(count => count !== 1);
    if (stale !== undefined) {
      failed++;
      console.log(`STALE  ${test.name}: text this case edits occurs ${stale} times in scripts/project-records.mjs. Update the case.`);
      continue;
    }
    for (const [find, replace] of edits) source = source.replace(find, replace);
    fs.writeFileSync(file, source);
    const run = spawnSync(process.execPath, [path.join(HERE, 'check.mjs'), '--root', root, '--until', [].concat(test.expect).join(','), '--depth', String(test.depth)], { encoding: 'utf8' });
    const expected = [].concat(test.expect);
    const caught = run.status === 1 && expected.every(property => run.stdout.includes(`VIOLATION of ${property}:`));
    if (!caught) failed++;
    console.log(`${caught ? 'CAUGHT' : 'MISSED'} ${test.name} -> ${expected.join(', ')}\n       broken rule: ${test.why}`);
    if (!caught) console.log(run.stdout.trim().split('\n').map(line => `       | ${line}`).join('\n') + (run.stderr ? `\n       | ${run.stderr.trim()}` : ''));
    else {
      // The steps printed under the expected violation, up to the next blank line.
      const lines = run.stdout.split('\n'), at = lines.findIndex(line => line.startsWith(`VIOLATION of ${expected[0]}:`));
      for (let i = at + 1; i < lines.length && /^\s+\d+\. /.test(lines[i]); i++) console.log(`       ${lines[i].trim()}`);
    }
  } finally { fs.rmSync(root, { recursive: true, force: true }); }
}
console.log(failed ? `\n${failed} of ${cases.length} cases did not behave as expected.` : `\nAll ${cases.length} broken copies were caught.`);
process.exit(failed ? 1 : 0);
