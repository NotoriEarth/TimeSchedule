# Calendar Picker Implementation Plan

> Execute inline with superpowers:executing-plans, with one fresh reviewer after implementation.

**Goal:** Replace year/month text entry with Windows-like year/month grids.
**Architecture:** Typed picker draft and bounded paging in month_jump.rs; reuse current navigation/placement and one grid component. Renderer tests drive real tiles.
**Tech Stack:** Rust/Iced, existing native popup.
**Spec:** docs/superpowers/specs/2026-10-31-calendar-picker.md

## Review focus
Grid1900/9998 edges; adjacent-decade selection; paging without committing; correct widget/diary target; popup reuse retaining draft; exact-date invalid/leap input and narrow layout; existing event draft and cancel.

## Task 1: Picker interaction and rendering
Files: month_jump.rs, popup.rs, app.rs tests, app/tests/visual.rs.
Interfaces: JumpMessage::PickYear(i32), PickMonth(u32), Page(i32), ShowYears, ToggleDate; MonthJump typed year/month, level, decade, direct-date draft.
- [ ] Real renderer test expects16year tiles and12month tiles, then clicks a year/month to navigate; baseline must fail before implementation.
- [ ] Replace text inputs with grids; year selection stages month, month selection commits; bounded paging and secondary direct-date field.
- [ ] Add state tests for bounds/page/cancel/draft/target; update prior input-specific tests to new selection behavior. Full workspace green.

## Task 2: Verify and deliver
- [ ] Render/inspect dark/light year/month, narrow expanded date input and boundary grids. fmt/diff check, Release and isolated launch.
- [ ] Independent immutable diff review against dddcbf6; one material-fix pass if needed. Record live OS input limitations.
- [ ] Docs, commit/push and update attached PR6. Supply new EXE and test steps; no merge.
