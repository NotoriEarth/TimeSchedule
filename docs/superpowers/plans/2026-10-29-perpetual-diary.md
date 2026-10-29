# Perpetual Diary Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development or superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Make the detail window the primary expandable perpetual diary, with project records, Git import and lightweight search.

**Architecture:** Retain Tauri/Rust SQLite ownership. Add cohesive diary persistence and command interfaces. Svelte diary views query bounded summaries; rich bodies load on demand. Widget preserves existing native behavior.

**Tech Stack:** Rust, rusqlite, Tauri 2, Svelte 5, TypeScript.

**Spec:** docs/superpowers/specs/2026-10-29-perpetual-diary-design.md

## Global Constraints
- Work on codex/perpetual-diary; preserve v1 data and desktop integration.
- Korean UI, local offline persistence, no mandatory cloud/Python service.
- Monday after row Sunday through next Sunday; back preserves displayed month.
- Source records and user writing survive repeated imports and summary generation.
- Backend bounds search results; frontend debounces and ignores stale responses.
- No external-service access claims or fabricated AI output.

## Review Focus
- Week selection across month/year boundaries and following Sunday in another calendar row.
- Existing database upgrade and failed validation preserving data.
- Korean substring search, punctuation, empty query and rapid replacement of input.
- Git repository paths with spaces, invalid repositories, repeat import and user edits.
- Unsaved editor content and widget refresh avoiding unrelated note bodies.

## Task 1: Integrated PC diary
**Files:** src-tauri/src/storage.rs, new src-tauri/src/diary.rs as appropriate, src-tauri/src/commands.rs, src-tauri/src/lib.rs, src/lib/api.ts, src/lib/dates.ts, src/App.svelte, new src/lib/Diary.svelte and focused components/styles, README.md.
**Interfaces:** existing widget commands stay compatible; diary/project/search/import commands are typed consistently in Rust and TypeScript. Store stable IDs and provenance. Date helpers explicitly expose week from preceding Sunday.
- [x] Add meaningful failing storage/date/search/import tests for the review focus; confirm failing behavior before implementation.
- [x] Implement migrations, project/entry persistence, bounded search and safe Git import (argument array, no shell interpolation).
- [x] Implement primary detail diary view, monthly-week navigation, entry expansion, project editing, source links and search navigation.
- [x] Implement editable evidence-based summaries clearly labelled, preserving manual edits. Keep unresolved external AI/Space integration explicit.
- [x] Reduce widget reads; retain working browser preview with matching behavior or explicit unavailable notices for native-only Git.
- [x] Run npm.cmd run check/build, Rust tests/fmt; review and fix functional defects.

## Task 2: Independent review and delivery
- [x] Review whole diff against spec and actual commands; fix significant findings.
- [x] Build release executable using scripts/build.ps1 and copy to artifacts/TimeSchedule-diary.exe.
- [x] Validate available UI paths and report native visual checks not performed.
- [x] Update documentation with accurate tests and user steps, commit, push, create PR and attach to chat. Do not merge.

## Progress
- Branch created; initial working tree clean.
- Preflight: Task 1 owns implementation files; Task 2 consumes its diff and binary. No competing writers. User explicitly requested proceeding; no additional design approval needed. Existing checkout on dedicated branch is used to preserve configured local toolchain.

- Implementation and independent review completed. Rust 21 unit + 1 Windows integration and Node 3 tests passed; check/build/fmt passed. Browser CRUD/search/week navigation verified; isolated release startup and desktop attachment logged. Native click/tray/user-database checks remain user verification. Delivery commit and draft PR follow this record; merge remains withheld.
