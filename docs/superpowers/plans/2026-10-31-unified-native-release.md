# Unified native release implementation plan

> Execute inline with superpowers:executing-plans; request one independent review at the end.

**Goal:** Merge the approved PR stack, provide one current executable, and establish a meaningful three-part version history.
**Architecture:** Keep both application sources. The native package manifest owns the current version. Compile it into the app and Windows executable properties, and publish to one stable path with an atomic replacement and an archived previous binary.
**Tech stack:** Existing Rust/Iced, MSVC resource compiler, PowerShell.
**Spec:** User request in this chat: merge all prepared PRs, consolidate local executables to the latest, and assign versions from 0.0.0 according to update importance.

## Decisions and constraints

- Current native version: 0.7.0. Retrospective milestones are documentation, not claims that these versioned releases were previously published.
- User-facing output: artifacts/TimeSchedule.exe. Historical EXEs go under artifacts/archive; preserve data and Tauri source/build support.
- Native Cargo.toml is the version source; Tauri's existing version remains its own historical baseline.
- No process termination or user database migration. Test with isolated data.
- Prepared PRs #2–#6 are approved for merge. New release housekeeping is a separate PR awaiting user testing under AGENTS.md.
- Work in the existing clean checkout on codex/unified-native-release, preserving local build caches and artifacts.

## Task 1: Merge approved changes

- [x] Recheck remote heads, mergeability, reviews/checks and run workspace/Tauri/frontend tests.
- [x] Merge #6 → #5 → #4 → #3 → #2 using merge commits and head SHA guards. Verify each resulting tree equals the tested f00799c tree.
- [x] Fast-forward local main to d604ad7 and create the release branch.

## Task 2: Version and package

- [x] Add Windows product/file version resources; show the Cargo version in settings, window title and startup log.
- [x] Add publication tests for exact output, archival preservation, mismatched versions, locked destination and missing source.
- [x] Publish a staged, hash-verified EXE using atomic replacement; preserve the prior executable in archive. Build failures must leave the installed output alone.
- [x] Update build-native.ps1, current README and CHANGELOG; record future version policy.
- [x] Run focused packaging tests, workspace tests, format check and release build. Verify file properties and isolated startup.
- [x] Move old top-level EXEs into an archive only after validating their resolved paths and hashes.
- [x] Review and prepare the new PR for commit/push. Leave the new PR unmerged pending user testing; the final chat records the created PR URL and commit.

## Review focus

- Locked/running output must not be truncated or forcefully stopped.
- All old files remain recoverable and unrelated files/data remain unchanged.
- No discrepancy between Cargo, Windows file properties and the displayed version.
- Tauri build instructions must not overwrite the canonical native executable.
- Historical documents must be clearly distinguished from the current launch instructions.

## Progress

Approved stack merged; all merge trees matched 7f082941bc2e6c7f7f8c648be354e01b463710fc. Baseline: workspace 95 passed/2 ignored; native fixture 1 passed; Tauri unit/integration tests passed; Node 3 passed; Svelte 0 errors/warnings. User saved and closed the old app.

Final verification: workspace 95 passed/2 ignored, release build and file properties passed, isolated normal-Windows startup attached/visible. Current artifact SHA-256 ABAF90F3DA939715B2E24C3F786AC620B12989CDBBF85A23AB488094744E443C. All 15 original top-level EXEs preserved.

Independent review found the historical benchmark default referencing the moved EXE. Fixed with a failing-then-passing actual path/hash validation test, preserving the pinned historical SHA. A separate actual-process check showed atomic replacement can replace a running executable; added explicit same-path process detection and proved failure→success with the isolated running-app test. No remaining findings. Detailed verification and user checks: docs/release-0.7.0-verification.md.
