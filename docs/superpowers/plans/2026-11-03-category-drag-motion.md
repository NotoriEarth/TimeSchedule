# Category drag motion implementation plan

> **For agentic workers:** Use superpowers:executing-plans. This is the inline implementation of the user's approved option 2.

**Goal:** Picked categories follow the pointer immediately; neighboring rows and landing move over 150ms without bounce.

**Architecture:** A focused native `category_drag` widget retains ID-keyed row motion and transient drag state. The existing category controller owns durable reorder and its pending preview; one drop emits one compare-and-swap reorder. Existing Iced scrollable owns wheel scrolling, with bounded edge-scroll requests only during an active drag.

**Tech Stack:** Existing Iced 0.14 / tiny-skia / Rust. No new dependencies.

**Spec:** User approved option 2 in this chat: immediate pointer following, neighboring rows make space, short landing animation, existing arrow controls, persist only on drop. This document records that accepted design; no new product decision needs approval.

## Global Constraints

- Continue clean existing task branch `codex/project-record-readability-plan` (PR #14); baseline `23c558554877a0fd9f11f23b4b05a57504e62abf`.
- Native version 0.11.2. Build/publish with scripts/build-native.ps1; never stop the user's app.
- Separate preview from persisted IDs/order. Preserve name/color drafts, refresh common diary/widget data after saving.
- Capture a real handle press; require 4 logical pixels of movement. Keep stable item IDs and a fixed slot geometry for drop decisions.
- Escape, loss of focus and releasing outside the list cancel without saving. First Escape during dragging cancels, not closes the dialog.
- Animated rows finish within 150ms; request frames only while motion or edge scrolling is active. No animation polling subscription or DB writes per frame.
- Arrow moves remain available and animate through the same keyed positions. Disable new edits/moves while saving/loading; preserve rejection rollback.

## Review Focus

1. Fast direction changes or a second drag during landing: no jump/wrong ID; test actual retained tree.
2. Scroll offset, clipped rows and edge autoscroll: pointer/drop coordinates stay aligned; test inside actual scrollable.
3. Outside release, Escape, window loss and external reorder: no stale drag or unintended save; test cancellation.
4. Duplicate release or stale reorder: one atomic write with original expected order; test app/store path.
5. Finishing or closing animation: no lingering redraw loop, inputs/drafts remain usable; test idle redraw state.

### Task 1: Native drag motion and category integration

**Files:** Create `apps/timeschedule-native/src/category_drag.rs` and its tests; modify category_ui.rs, app.rs, main.rs and app/tests/visual.rs; version files and verification documentation.

**Interfaces:** The widget consumes stable `(id, Element)` rows, enabled state and the existing scrollable viewport. It emits `Action::Reorder { expected: Vec<String>, order: Vec<String> }` once on valid release and `Action::Scroll(f32)` for active edge scrolling. The controller displays pending order, commits it atomically and either refreshes shared data or rolls back visibly on failure.

- [ ] Write a real Iced event regression: press second handle, move upward 20px; its label must move by 20px immediately, with zero reorder messages/storage writes. At a neighboring slot crossing the other row must be between old and new position after 75ms, and at target after 150ms. Existing implementation must fail.
- [ ] Implement widget-local keyed motion, stable slot targeting and retained child identity; wire one-shot controller save and pending preview.
- [ ] Test cancellation, pointer movement threshold, duplicate release, fast reversal/second drag, real scrollable wheel/edge movement and idle redraw cessation.
- [ ] Verify a real app/store reorder refreshes categories and shared projections, preserves unsaved fields and rejects stale expected IDs without overwriting.
- [ ] Run `. scripts/toolchain.ps1; cargo test --workspace --locked --offline` (all pass); render native drag/landing frames at small/default dialog sizes and inspect them.
- [ ] Update version 0.11.2 and verification notes; build with `scripts/build-native.ps1 -Offline`, publish after user exit if needed. Commit all integrated changes after tests/build because the widget and controller form one deliverable.
- [ ] One fresh independent review of the final change plus surrounding PR context; address Important/Critical findings with failing regression tests and a green workspace suite.
- [ ] Push/update PR #14, keep merge pending, and prepare/sync a same-work-ID development record containing only verified changes and limitations.
