# DesktopCal Priority Implementation Plan

> **For agentic workers:** Use superpowers:executing-plans for implementation and a fresh whole-branch review.

**Goal:** Add quick day editing, safe widget completion, and background-only transparency.

**Architecture:** Shared storage/editor, one additional Iced window, opt-in Windows softbuffer alpha output.

**Tech Stack:** Rust, Iced 0.14/tiny-skia, SQLite, Win32.

**Spec:** `docs/superpowers/specs/2026-10-31-desktopcal-priority-design.md`

## Constraints and review focus

Keep existing data, manager themes, and Tauri behavior. No new remote services or copied DesktopCal graphics. Verify header-only completion cannot erase notes; late day loads cannot reopen dismissed windows; dirty/IME input survives focus and close; checkbox clicks cannot trigger the day popup; alpha presentation cannot affect unrelated windows. Work in the user-requested task branch in this clean checkout, preserving local build caches.

## Tasks

- [x] Add failing storage regression for completion preserving all other fields; implement `Storage::set_event_completed(&str, bool)` and verify.
- [x] Add quick-window state tests, then a separate `quick_editor` module using the existing Form/Editor and guarded window lifecycle. Integrate date double-click and completion controls, then run native tests.
- [x] Add a failing actual-renderer alpha test and a layered-child softbuffer fixture. Vendor the pinned softbuffer sources/licenses, implement HWND opt-in and background-only fills, then verify both themes and unchanged normal-window path.
- [x] Run workspace tests and required Windows fixtures, format check, release build, renderer previews and a separate-data desktop smoke test. Inspect UI when available.
- [x] Fresh whole-branch review and fix material findings with regression tests; re-review the fixes.

Delivery after committing this record: push, create a stacked PR with validation and limits, attach it to this chat, and deliver the test EXE. Do not merge. The final chat response records the resulting PR URL.

## Execution record

Base: `1453f4b4ea841d400b5b919595fd1beef3224350`. Implementation follows the user's selection without another design approval round. Workspace: 70 passed, 2 intentionally ignored; dedicated Windows fixture: 1 passed; optional PNG generator: 1 passed. Final release and isolated desktop attach succeeded. Renderer images were inspected, but actual window capture timed out twice, so live visual/input/IME checks remain user acceptance items. See [delivery verification](../../desktopcal-priority-verification.md).
