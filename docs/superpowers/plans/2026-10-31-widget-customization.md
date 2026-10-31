# Widget Customization Implementation Plan

> **For agentic workers:** Use superpowers:executing-plans inline, then a fresh whole-branch reviewer.

**Goal:** Apply resize/week count, appearance controls, and the user's year/month jump interaction.
**Architecture:** Share computed date ranges between view and queries. Store plain validated settings separately from runtime drafts and Iced drawing values. Keep native geometry in the existing owned-window adapter.
**Tech Stack:** Rust, Iced 0.14, chrono, serde JSON, Win32, existing tiny-skia alpha presenter.
**Spec:** `docs/superpowers/specs/2026-10-31-widget-customization.md`

## Constraints and review focus

Preserve previous PR behavior, user data, drafts, themes and Tauri compatibility. No new runtime. Review: year/month rollover and stale results; font/size clipping; independent dark/light overrides; locked/DPI/parented geometry and preference write ordering; popup focus/close and existing editor preservation.

## Task 1: Range and preferences

Files: core `dates.rs`, desktop `preferences.rs` and new `widget_preferences.rs`, native `calendar_viewport.rs`.
Interface: `calendar_range(anchor, weeks: Option<(u8,u8)>) -> Result<WeekRange,String>`; serializable `WidgetPreferences` defaulting monthly, 960×640, 4 rolling weeks/0 past, original typography; per-theme RGB overrides. Runtime anchor/follow state belongs to native module.
- [ ] Add failing date-range and JSON compatibility/validation tests; run core/desktop tests, observe missing behavior.
- [ ] Implement bounded date range and validated preferences. Run the same tests green.

## Task 2: Native behavior and controls

Files: native `widget_settings.rs`, `month_jump.rs`, `app.rs`, `views.rs`, `main.rs`; desktop `windows.rs`.
Interface: nested widget-settings messages; popup fields/selection/draft local to `MonthJump`; `windows::resize_logical` and `logical_size`; app computes widget scope from the shared viewport.
- [ ] Add failing app regressions for range/scope, appearance without reload, year/month initialization/apply/cancel, locked sizing; run native tests.
- [ ] Implement range selection, settings controls, year/month popup and safe numeric resizing/grip. Reuse serialized preference writes and stale-generation protection.
- [ ] Exercise native logical resizing/lock through the existing real child-window fixture. Run workspace and Windows fixtures green.

## Task 3: Rendering and delivery

Files: native `desktop_widget.rs`, `widget_appearance.rs`, `text_layout.rs`, `app/tests/visual.rs`; delivery docs.
- [ ] Extend real renderer tests for custom font metrics, variable rows/dense settings, selected date and separate heading buttons. Run red then implement shared appearance and dynamic grid.
- [ ] Generate/inspect preview PNGs (dark/light, popup, small/dense/week modes); format/check, full tests, final release and isolated smoke test.
- [ ] Fresh whole-branch review; reproduce/fix material findings, rerun relevant checks.
- [ ] Commit/push, create PR stacked on `codex/desktopcal-priority-features`, attach to chat, provide EXE/test steps. No merge.

## Execution record

User has approved implementation of the discussed design and supplied the popup interaction. Use the existing clean checkout/task branch to retain local build caches. Logs and execution ledger live under ignored `artifacts/widget-customization/` on this Windows workspace.
