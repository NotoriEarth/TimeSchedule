// Explores every order of steps, up to a bound, through the real record
// delivery code (scripts/project-records.mjs `prepare` and `sync`) and checks
// that the diary never loses or rolls back a record. See SKILL.md beside it.
//
//   node .claude/skills/record-delivery/check.mjs [--captures N] [--depth N] [--json FILE]
//                                                 [--stop-at-first] [--until PROPERTY,...] [--root DIR]
//
// Exit 0: no property is violated within the bound, apart from the findings
//         already recorded in known-findings.json (they are printed again).
// Exit 1: a property is violated that is not a recorded finding; a sequence of
//         steps that leads to it is printed.
// Exit 2: the checker itself could not run. Nothing was concluded.
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { createHash } from 'node:crypto';
import { fileURLToPath, pathToFileURL } from 'node:url';

const option = (name, fallback) => {
  const at = process.argv.indexOf(name);
  return at < 0 ? fallback : process.argv[at + 1];
};
const HERE = path.dirname(fileURLToPath(import.meta.url));
const REPO = path.resolve(HERE, '..', '..', '..');
// --root points at a copy of the repository's scripts/ and tests/helpers/, so a
// deliberately broken copy can be checked without touching the real files.
const ROOT = path.resolve(option('--root', REPO));
const CAPTURES = Number(option('--captures', 2));   // revisions of one work item
const DEPTH = Number(option('--depth', 3));          // steps on one path
const JSON_OUT = option('--json', null);
const STOP_AT_FIRST = process.argv.includes('--stop-at-first');
const UNTIL = option('--until', null);                // stop once all of these properties were violated
const SETTLE = 3;                                     // syncs allowed for a backlog to drain
const WORK_ID = '11111111-2222-4333-8444-555555555555';
const sha = value => createHash('sha256').update(value).digest('hex');

let scratch = null;
try {
  process.exitCode = await main();
} catch (error) {
  console.error(`The checker could not finish: ${error.stack ?? error}`);
  process.exitCode = 2;
} finally {
  if (scratch) fs.rmSync(scratch, { recursive: true, force: true });
}

async function main() {
  const load = file => import(pathToFileURL(path.join(ROOT, file)).href);
  const { prepare, sync, GIT_RETRY_LIMIT } = await load('scripts/project-records.mjs');
  const { gitFixture } = await load('tests/helpers/record-git-fixture.mjs');
  // Violations of the current code that were reported and not fixed. They do
  // not fail the run, so that a new violation is not lost among them.
  const KNOWN = JSON.parse(fs.readFileSync(path.join(HERE, 'known-findings.json'), 'utf8'));
  const isKnown = property => KNOWN.some(k => k.property === property);

  // ---- the world outside the code under check -----------------------------

  scratch = fs.mkdtempSync(path.join(os.tmpdir(), 'timeschedule-record-delivery-'));
  const fixture = gitFixture(scratch);
  const live = { state: path.join(scratch, 'live', 'state'), vault: path.join(scratch, 'live', 'vault') };
  const dotGit = path.join(fixture.repo, '.git'), away = path.join(fixture.repo, '.git-away');
  const nowhere = path.join(scratch, 'briefly-missing');
  const captureInput = capture => ({ workId: WORK_ID, projectTitle: 'TimeSchedule', repoPath: fixture.repo, date: '2026-11-16',
    kind: 'development', title: 'record delivery check', status: 'verification_pending',
    detailedMarkdown: `capture ${capture}: what was changed and tested.`, gitChanges: fixture.gitChanges });
  const captureOf = record => Number(/^capture (\d+):/.exec(record.detail)[1]);

  // The app's receiving rule, crates/timeschedule-core/src/project_records.rs
  // `apply_project_record`, for a valid record and a diary entry the user never
  // edits: an older delivery revision is stale, the same revision is unchanged
  // when the payload is identical and a conflict when it is not, a newer one is
  // saved. The app's validation of the record's fields is not repeated here.
  function receive(diary, record) {
    const payload = JSON.stringify(record);
    const answer = outcome => ({ outcome, workId: record.workId, revision: record.revision, entryId: 'entry-1' });
    if (diary.revision !== null) {
      if (record.revision < diary.revision) return answer('stale');
      if (record.revision === diary.revision) return answer(payload === diary.payload ? 'unchanged' : 'conflict');
    }
    diary.revision = record.revision; diary.payload = payload; diary.capture = captureOf(record);
    return answer('saved');
  }

  // ---- job files ------------------------------------------------------------

  const workDir = () => path.join(live.state, WORK_ID);
  function jobFiles() {
    if (!fs.existsSync(workDir())) return [];
    return fs.readdirSync(workDir()).filter(f => f === 'state.json' || /^pending-\d+\.json$/.test(f)).sort()
      .map(f => ({ name: f, file: path.join(workDir(), f), job: JSON.parse(fs.readFileSync(path.join(workDir(), f), 'utf8')) }));
  }
  const UNFINISHED = job => !['complete', 'local_complete', 'conflict', 'excluded'].includes(job.phase);
  // Git cannot answer for one capture only: its repository is briefly missing.
  // The same device as tests/project-records-git-retry.test.mjs uses. Returns
  // the spelling the job had, which is put back exactly afterwards.
  function setRepoOf(capture, repoPath) {
    let previous = null;
    for (const { file, job } of jobFiles()) {
      if (job.revision !== capture) continue;
      previous = job.source.repoPath;
      job.source.repoPath = repoPath;
      fs.writeFileSync(file, JSON.stringify(job));
    }
    return previous;
  }

  // ---- steps a user, the app or the environment can take ---------------------

  const PREPARE = { name: 'prepare the next capture of the work' };
  const gitWords = git => git === 'up' ? 'Git answers' : git === 'down' ? 'Git fails for every capture'
    : git === 'unchecked' ? 'Git not checked: run budget spent' : `Git fails for capture ${git} only`;
  function stepsFrom(world) {
    const steps = world.captures < CAPTURES ? [PREPARE] : [];
    if (!world.captures) return steps;
    const gits = ['up', 'down', 'unchecked', ...(world.captures > 1 ? Array.from({ length: world.captures }, (_, i) => i + 1) : [])];
    const jobsWords = jobs => jobs === 1 ? 'one job, as the app runs it' : 'all jobs';
    for (const jobs of [1, Infinity]) for (const git of gits) {
      steps.push({ name: `sync(${jobsWords(jobs)}, ${gitWords(git)})`, jobs, git, repeat: 1 });
    }
    // The app takes each delivery of the run and the script never gets its
    // answer: the command line ended after the record was saved.
    for (const jobs of [1, Infinity]) for (const git of gits.filter(git => git === 'up' || typeof git === 'number')) {
      steps.push({ name: `sync(${jobsWords(jobs)}, ${gitWords(git)}, the app saves each delivery and its answer is lost)`, jobs, git, answer: 'lost', repeat: 1 });
    }
    // A shortcut to the paused state, which otherwise needs twelve steps of the path.
    if (jobFiles().some(({ job }) => UNFINISHED(job) && job.phase !== 'git_paused')) {
      steps.push({ name: `long Git outage: ${GIT_RETRY_LIMIT} failing syncs`, jobs: Infinity, git: 'down', repeat: GIT_RETRY_LIMIT });
    }
    return steps;
  }
  async function runSync(step, world, events) {
    const importer = async record => {
      const before = world.diary.capture;
      // Every content ever sent under a revision, and what the app answered.
      const hash = sha(JSON.stringify(record)), sent = world.sent[record.revision];
      const resent = sent !== undefined && sent.hash !== hash;
      // The app's inbox (record_inbox.rs `enqueue`) refuses other content under a
      // revision it has received, before the receiving rule is consulted.
      if (resent) {
        events.push({ delivered: record.revision, capture: captureOf(record), outcome: 'error', shownBefore: before, resent });
        throw new Error('the inbox already holds other content under this revision');
      }
      // The same content under a revision it has handled gets the receipt it
      // stored then. The diary is not touched.
      const again = sent !== undefined;
      const receipt = again ? { outcome: sent.outcome, workId: record.workId, revision: record.revision, entryId: 'entry-1' } : receive(world.diary, record);
      if (!again) world.sent[record.revision] = { hash, outcome: receipt.outcome };
      events.push({ delivered: record.revision, capture: captureOf(record), outcome: receipt.outcome, shownBefore: before, resent, again });
      if (step.answer === 'lost') throw new Error('the answer of the app did not arrive');
      return receipt;
    };
    const options = { stateDir: live.state, vaultRoot: live.vault, projectRoot: fixture.repo, retryNow: true,
      importer, ...(step.jobs === 1 ? { maxJobs: 1 } : {}), ...(step.git === 'unchecked' ? { gitCheckBudgetMs: 0 } : {}) };
    let spelling = null;
    if (step.git === 'down') fs.renameSync(dotGit, away);
    if (typeof step.git === 'number') spelling = setRepoOf(step.git, nowhere);
    try { for (let i = 0; i < step.repeat; i++) await sync(options); }
    finally {
      if (step.git === 'down') fs.renameSync(away, dotGit);
      if (spelling !== null) setRepoOf(step.git, spelling);
    }
  }
  async function take(step, world, events) {
    if (step === PREPARE) { world.captures += 1; await prepare(captureInput(world.captures), { stateDir: live.state, projectRoot: fixture.repo }); }
    else await runSync(step, world, events);
  }

  // ---- states -----------------------------------------------------------------

  // What decides future behavior, without clock values or paths.
  function summary(world) {
    return { captures: world.captures, sent: world.sent,
      diary: { revision: world.diary.revision, capture: world.diary.capture, payload: world.diary.payload && sha(world.diary.payload) },
      jobs: jobFiles().map(({ name, job: j }) => ({ file: name, capture: j.revision, phase: j.phase, local: j.localRevision,
        localSaved: !!j.localSaved, vaultSaved: !!j.vaultSaved,
        mode: j.diaryMode ?? null, gitFailures: j.gitFailures ?? 0, pausedFrom: j.pausedFromPhase ?? null, excludedFrom: j.excludedFromPhase ?? null,
        reason: j.exclusionReason ?? null, receipt: j.diaryReceipt ? `${j.diaryReceipt.revision}:${j.diaryReceipt.outcome}` : null,
        attempts: Math.min(j.attempts ?? 0, 1) })) };
  }
  const snapshots = path.join(scratch, 'snapshots');
  function save(id) {
    for (const part of ['state', 'vault']) if (fs.existsSync(live[part])) fs.cpSync(live[part], path.join(snapshots, id, part), { recursive: true });
  }
  function restore(id) {
    for (const part of ['state', 'vault']) {
      fs.rmSync(live[part], { recursive: true, force: true });
      const from = path.join(snapshots, id, part);
      if (fs.existsSync(from)) fs.cpSync(from, live[part], { recursive: true });
    }
  }

  // ---- properties -------------------------------------------------------------

  // Checked on every step. Each entry says which property the step broke and how.
  function stepViolations(before, after, events) {
    const found = [];
    for (const e of events) {
      if (e.outcome === 'saved' && !e.again && e.shownBefore !== null && e.capture < e.shownBefore) {
        found.push({ property: 'no-rollback', text: `the diary showed capture ${e.shownBefore} and delivery revision ${e.delivered} replaced it with the older capture ${e.capture}` });
      }
      if (e.resent || e.outcome === 'conflict') {
        found.push({ property: 'one-payload-per-revision', text: `delivery revision ${e.delivered} was sent with content different from what it carried before (capture ${e.capture}); the app answered "${e.outcome}"` });
      }
    }
    for (const job of after.jobs) {
      if (job.phase === 'conflict') found.push({ property: 'no-false-conflict', text: `capture ${job.capture} ended in "conflict" although the user edited nothing` });
      if (job.phase === 'excluded') found.push({ property: 'no-false-exclusion', text: `capture ${job.capture} was excluded (${job.reason}) although its commits exist in Git` });
    }
    for (const job of before.jobs.filter(j => !['complete', 'local_complete'].includes(j.phase))) {
      if (!after.jobs.some(j => j.capture === job.capture)) found.push({ property: 'no-capture-lost', text: `capture ${job.capture} was "${job.phase}" and its job is gone` });
    }
    return found;
  }
  // Checked from every state: once Git answers and syncs that run every job
  // keep coming, the newest capture is what the diary shows and nothing waits.
  const settleRuns = jobs => jobs === 1 ? CAPTURES + 2 : SETTLE;
  const settleWords = jobs => `${settleRuns(jobs)} syncs of ${jobs === 1 ? 'one job each, as the app runs them' : 'all jobs'}, with Git answering`;
  async function settleViolations(world, jobs) {
    const events = [], before = summary(world);
    await runSync({ jobs, git: 'up', repeat: settleRuns(jobs) }, world, events);
    const end = summary(world), found = stepViolations(before, end, events);
    if (world.captures > 0) {
      const waiting = end.jobs.filter(j => !['complete', 'local_complete'].includes(j.phase));
      const tail = `after ${settleWords(jobs)}`;
      if (waiting.length) found.push({ property: 'delivered-in-the-end', text: `${tail}, ${waiting.map(j => `capture ${j.capture} is still "${j.phase}"`).join(', ')}` });
      if (end.diary.capture !== world.captures) found.push({ property: 'newest-shown-in-the-end', text: `${tail}, the diary shows ${end.diary.capture === null ? 'nothing' : `capture ${end.diary.capture}`} but the newest capture is ${world.captures}` });
    }
    return found;
  }

  // ---- exploration ------------------------------------------------------------

  const started = Date.now();
  const seen = new Map();      // state key -> id
  const nodes = [];            // id -> { world, depth, parent, step }
  const findings = new Map();  // property -> the first violation found, breadth first
  const pathTo = id => {
    const steps = [];
    for (let node = nodes[id]; node.parent !== null; node = nodes[node.parent]) steps.unshift(node.step);
    return steps;
  };
  const report = (violation, id, extra) => {
    if (!findings.has(violation.property)) findings.set(violation.property, { ...violation, steps: [...pathTo(id), ...extra] });
  };
  const cloneWorld = world => ({ captures: world.captures, sent: { ...world.sent }, diary: { ...world.diary } });
  const fresh = () => [...findings.keys()].some(property => !isKnown(property));
  const enough = () => UNTIL ? UNTIL.split(',').every(property => findings.has(property)) : STOP_AT_FIRST && fresh();

  let transitions = 0, complete = true;
  const first = { captures: 0, sent: {}, diary: { revision: null, payload: null, capture: null } };
  nodes.push({ world: first, depth: 0, parent: null, step: null });
  seen.set(JSON.stringify(summary(first)), 0); save('0');
  explore: for (let id = 0; id < nodes.length; id++) {
    const node = nodes[id];
    // Single steps first, so that a violation one step away is reported as that step.
    if (node.depth < DEPTH) {
      restore(String(id));
      for (const step of stepsFrom(node.world)) {
        restore(String(id));
        const world = cloneWorld(node.world), before = summary(world), events = [];
        await take(step, world, events);
        transitions++;
        const after = summary(world);
        for (const violation of stepViolations(before, after, events)) report(violation, id, [step.name]);
        const key = JSON.stringify(after);
        if (!seen.has(key)) {
          seen.set(key, nodes.length);
          nodes.push({ world, depth: node.depth + 1, parent: id, step: step.name });
          save(String(nodes.length - 1));
        }
        if (enough()) { complete = false; break explore; }
      }
    }
    // From this state: does the backlog drain?
    for (const jobs of [Infinity, 1]) {
      restore(String(id));
      for (const violation of await settleViolations(cloneWorld(node.world), jobs)) report(violation, id, [`then ${settleWords(jobs)}`]);
    }
    if (enough()) { complete = false; break; }
  }

  const result = { bound: { captures: CAPTURES, depth: DEPTH }, complete, states: nodes.length, transitions, seconds: Math.round((Date.now() - started) / 1000),
    findings: [...findings.values()] };
  if (JSON_OUT) fs.writeFileSync(JSON_OUT, JSON.stringify(result, null, 2));
  console.log(`record delivery: ${result.states} states, ${result.transitions} steps, ${result.seconds}s (captures <= ${CAPTURES}, path length <= ${DEPTH})`);
  const show = finding => finding.steps.forEach((step, index) => console.log(`  ${index + 1}. ${step}`));
  const unrecorded = result.findings.filter(f => !isKnown(f.property));
  for (const finding of result.findings.filter(f => isKnown(f.property))) {
    console.log(`\nKNOWN FINDING, not fixed: ${finding.property}: ${finding.text}`);
    show(finding);
  }
  // Only a search that ran to its bound can say that something did not happen.
  if (complete) {
    for (const known of KNOWN.filter(k => DEPTH >= (k.depth ?? 0) && !result.findings.some(f => f.property === k.property))) {
      console.log(`\nNOT REPRODUCED within this bound: the recorded finding ${known.property}. If the code was fixed, remove it from known-findings.json.`);
    }
  }
  for (const finding of unrecorded) {
    console.log(`\nVIOLATION of ${finding.property}: ${finding.text}`);
    show(finding);
  }
  if (unrecorded.length) return 1;
  console.log(`\nPASS: no property is violated within the bound${result.findings.length ? ', apart from the recorded findings above' : ''}.`);
  return 0;
}
