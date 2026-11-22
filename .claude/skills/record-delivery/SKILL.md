---
name: record-delivery
description: Checks, for every order of steps up to a bound, that automatic development record delivery (scripts/project-records.mjs prepare and sync) never rolls the diary back to an older record, never drops or wrongly excludes a record, and always ends with the newest record shown. Use when a change touches scripts/project-records.mjs, scripts/project-records-git.mjs, scripts/document-vault.mjs, the delivery revision or phase of a record job, or the receiving rule in crates/timeschedule-core/src/project_records.rs.
---

# Record delivery check

A development record can be captured several times for one piece of work. Each capture becomes a job that `sync` verifies with Git, saves to the document vault, and delivers to the diary under a delivery revision. The diary keeps whatever has the highest revision. Syncs run at any time, Git can fail for a while, and the user can turn Space on or off between runs. The check runs the real `prepare` and `sync` through every order of those steps, up to a bound, and looks at what the diary ends up showing.

## What is checked

| Property | In plain words | Harm when it breaks |
| --- | --- | --- |
| `no-rollback` | The diary never replaces a newer capture with an older one. | The user reads an outdated record as the current one. |
| `no-rollback-after-space-turned-off` | The same, when the newer capture was delivered while Space was on and the older one was renumbered by a later sync with Space off. | Same. Kept separate because it is a recorded finding. |
| `one-payload-per-revision` | One delivery revision is never sent with two different contents, whatever the app answers. | The app answers with a conflict, or silently keeps one of the two, and the record stops matching what was captured. |
| `no-false-conflict` | No capture ends in `conflict` when the user edited nothing. | A record is stuck until someone repairs it by hand. |
| `no-false-exclusion` | No capture is `excluded` while its commits exist in Git. | A record is dropped for good because Git was briefly unavailable. |
| `no-capture-lost` | A capture that is still waiting never loses its job file. | A record disappears without having been delivered. |
| `delivered-in-the-end` | From any state, more syncs with Git answering leave no capture waiting: three syncs that run every job, or one sync per capture plus two that run one job each, as the app does. | A record stays undelivered although nothing is wrong any more. |
| `newest-shown-in-the-end` | After those syncs the diary shows the newest capture. | The diary settles on outdated content. |

## Run it

From the repository root. It needs Node and Git, which the repository's tests already need. It works in a scratch folder under the system temp folder and does not touch `.cache`, the document vault, the app or its data.

```powershell
node .claude\skills\record-delivery\check.mjs
```

About three minutes. Options: `--depth N` for longer step sequences (4 takes about fifteen minutes), `--captures N` for more captures of the work, `--json FILE` for the result as JSON, `--until P1,P2` to stop as soon as those properties have been violated.

Read the last lines:

- `PASS: no property is violated within the bound` and exit code `0`. Any `KNOWN FINDING, not fixed` printed above it is a recorded finding of the current code, listed below. It does not fail the run.
- `VIOLATION of <property>` and exit code `1`. Something not recorded is broken. The numbered lines are a sequence of steps that leads to it: the first one a breadth-first search found, so no shorter path reaches a violating state.
- `NOT REPRODUCED within this bound` for a recorded finding. With the default bound this means the code no longer does it on the recorded steps.
- `The checker could not finish` and exit code `2`. The checker itself failed. Read the error; nothing was concluded.

## Does my change touch it?

```powershell
git diff --name-only f826227 -- scripts/project-records.mjs scripts/project-records-git.mjs scripts/document-vault.mjs crates/timeschedule-core/src/project_records.rs tests/helpers/record-git-fixture.mjs
```

It compares your working tree, committed or not, with the sync commit `f826227`: the last commit at which the check was confirmed to describe this code (see "Coverage"). Any output means run the check.

## Reading a violation

A counterexample is a list of steps in the words of the code:

- `prepare the next capture of the work` calls `prepare` with new content for the same work ID.
- `sync(space on|off, one job|all jobs, Git ...)` calls `sync` once with `spaceEnabled`, with `maxJobs: 1` (how the app runs it) or without, and `retryNow: true`. `Git answers` is a normal run. `Git fails for every capture` hides the repository's `.git` for that run. `Git fails for capture N only` points only that capture's job at a missing folder for that run. `Git not checked: run budget spent` passes `gitCheckBudgetMs: 0`.
- `long Git outage` is twelve failing syncs in a row, enough to pause a capture.
- `publishing to Space fails` is a sync with Space on in which the diary gets its delivery and the page cannot be published.
- `then 3 syncs of all jobs, with Git answering` and `then 4 syncs of one job each, as the app runs them` are the settling runs that the two end properties look at.

Decide whether it is a bug or a modeling error before changing anything:

1. Walk the steps through `sync` in `scripts/project-records.mjs`, following `localRevision`, `finalRevision`, `diaryMode` and `phase` of each job. If the code really does what the steps say, it is a bug.
2. It is a modeling error if the step cannot happen for real, or if `receive` in `check.mjs` no longer matches `apply_project_record` in `crates/timeschedule-core/src/project_records.rs`. Fix `check.mjs` in that case and say so.
3. A real bug is worth a named test. `tests/project-records-git-retry.test.mjs` shows the pattern: an importer stub that keeps the highest revision, and the same way of failing Git for one capture.

Report a bug with its steps. Do not relax a property, drop a step kind or raise a bound to make the run pass.

## Recorded findings

`known-findings.json` lists violations of the current code that were reported and not fixed. One entry, found at `f826227`:

- `no-rollback-after-space-turned-off`. Two captures of one work are waiting and Space is on. Git cannot check the first capture in a run, so only the second is delivered (revision 3, shown in the diary). The user turns Space off. The next sync numbers both captures again in capture order, above everything used so far: the first gets revision 5 and replaces the second in the diary. The same happens when the renumbering sync delivers nothing and a later sync, with Space on again, delivers the first capture. The second is then delivered again as revision 7. When all jobs run in one sync the wrong content is on show for a moment. When the app runs it, one job per run, it stays until the next run.

When the code is fixed, the check prints `NOT REPRODUCED`. Remove the entry from `known-findings.json` then. Do not add an entry to make a new violation pass: a finding is recorded only after it has been reported to the user.

## When a change is intended

Update the check in the same change, then rerun both commands in this file.

- A new field on a job that changes what `sync` does later: add it to `summary` in `check.mjs`, or two different states will be treated as one.
- A new phase: check `UNFINISHED` and the finished list in `settleViolations`.
- A new way to run `sync`, or a new thing the user or the environment can do: add a step in `stepsFrom`.
- A change to the receiving rule in the app: change `receive` to match.
- A rule that a self-test case edits was rewritten: update that case's `find` and `replace` in `selftest.mjs` to the same break in the new text.

## Prove the check can still fail

```powershell
node .claude\skills\record-delivery\selftest.mjs
```

About two minutes. It copies `scripts/` to a scratch folder seven times, breaks one rule in each copy, and expects `check.mjs` to report the property that rule protects. Between them the seven breaks make every property in the table fail once, except `no-rollback-after-space-turned-off`, which the current code already violates. `CAUGHT` on every line and exit code `0` is the pass. `MISSED` means the checker no longer sees that kind of break. `STALE` means the code a case edits has changed and the case must be updated. Run it whenever `check.mjs` or `selftest.mjs` changes, and after a change to `sync` that makes a case stale.

## Coverage

Run for real, as written in the repository:

- `scripts/project-records.mjs`: `prepare`, `sync` with its `toLocal`, `holdUnverified` and `importRecord`, `diaryPayload`, `loadJobs`, `upgradeJob`, the job files `state.json` and `pending-N.json`
- `scripts/project-records-git.mjs`: `developmentEvidence`, against a real scratch Git repository
- `scripts/document-vault.mjs`: `saveDocument`, into a scratch vault

Stood in for, by code in `check.mjs`:

- The app's receiving rule `apply_project_record` (`crates/timeschedule-core/src/project_records.rs`), as `receive`: an older revision is stale, the same revision with the same payload is unchanged and with a different one a conflict, a newer one is saved.
- The app's inbox refusing other content under a revision it has received (`record_inbox.rs` `enqueue`): the importer throws, as the command line does.
- Publishing to Space: succeeds, or fails before anything is written in the steps that say so.

Sync commit: `f826227`. Move it forward only after rerunning `check.mjs` and `selftest.mjs` at the new commit and confirming that the lists above still describe the code. Change it in this file in both places it appears.

## What a pass does not cover

A pass covers the sequences inside the bound, not the system.

- One work item, two captures, sequences of up to three steps by default.
- Every sync runs with `retryNow`, so behavior that depends on a retry time not being due yet is not explored.
- `receive` does not repeat the app's validation of a record's fields. A payload the app would reject counts as delivered.
- The app answers at once. Its `queued` answer, a drain of its inbox later and in file-name order, and an error returned after a record was queued are not explored.
- The user never edits or deletes the diary entry.
- No crash in the middle of `prepare` or `sync`, and no two syncs at once.
- Publishing to Space fails only before anything is written; a publication that may have been made is not explored.
- Git fails for a whole run or for one capture's check, not at a single Git command.
