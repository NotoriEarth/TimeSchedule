# Diary Navigation Implementation Plan

> Execute inline with superpowers:executing-plans, then one independent whole-change review.

**Goal:** Same year/month jump in diary, date double-click creation, origin-centered popups.
**Architecture:** Explicit calendar target in month_jump; existing guarded editor; native physical client bounds for separate popup placement.
**Tech Stack:** Existing Rust/Iced/Win32.
**Spec:** docs/superpowers/specs/2026-10-31-diary-navigation.md

## Constraints / review focus
Keep independent calendar months and unsaved drafts. Preserve single click and nested card buttons. Negative monitor coordinates and small work areas; child-window client-to-screen conversion; source/target close while asynchronous placement is pending; reused popup focus and origin changes.

## Task 1: Diary interactions
Files: month_jump.rs, shell.rs, views.rs, app.rs and app/tests/visual.rs.
Interface: JumpTarget::{Widget,Diary}; JumpMessage::Open(JumpTarget,JumpField). Existing Message::New(Event,Some(date)) for date double click.
- [ ] Add real renderer tests for diary heading clicks and single/double date clicks (monthly/weekly). Expected: fail with missing double click or jump behavior.
- [ ] Implement target-aware jump and reuse guarded form creation; tests must show only origin calendar moves and existing drafts survive.
- [ ] Run native tests green.

## Task 2: Popup placement and delivery
Files: new desktop popup.rs, new native popup.rs, quick_editor.rs, month_jump.rs, app.rs, module declarations and tests.
Interface: popup::anchor(hwnd) -> Result<Anchor,String>; popup::center(hwnd,Anchor) -> Result<(),String>. UI-thread callbacks capture live source physical client/work bounds then move target; hidden-open windows revealed after placement.
- [ ] Add literal geometry tests for negative coordinates, edge clamp, popup larger than source, and invalid/foreign window rejection. Expected: fail until helper is implemented.
- [ ] Add owned-window Windows fixture proving screen coordinates from a child origin and centered top-level popup. Run tests green.
- [ ] Full workspace, fmt/diff checks, meaningful renderer previews, release build and isolated launch. Independently review immutable change range e33925f..HEAD; fix material findings once, rerun affected checks.
- [ ] Update verification doc, commit/push and update PR6, attach, deliver EXE and manual checks. No merge.

## Added during implementation: common event form
User explicitly requested matching widget/diary event-add style and one template. Add a renderer-operation test comparing title-input bounds relative to each form panel. Observe old padding14vs22 fail; extract the existing diary panel into shared panel(), use it in both surfaces, and set quick default size to the shared editor size. Inspect quick-editor and diary-editor PNGs; retain narrow-window and dirty-confirmation tests. Reviewer finding about reused jump position also gets a red/green regression preserving its draft.
