# DesktopCal widget implementation plan

> **For agentic workers:** Use superpowers:executing-plans inline, then one independent whole-change review.

**Goal:** Apply the requested DesktopCal-style layout to the native desktop widget.
**Architecture:** Dedicated widget module owns layout and small style constants; shared text measurement bounds titles. Existing app messages, preferences and native Windows adapter retain behavior.
**Tech Stack:** Rust / Iced 0.14 / tiny-skia.
**Spec:** `docs/superpowers/specs/2026-10-31-desktopcal-widget.md`

## Constraints and review focus

- Preserve manager design, native attachment, physical-pixel position restoration, lock/opacity/visibility and draft protection.
- No new runtime dependencies or broad design-system rewrite.
- Check fitted small/high-DPI windows, dense/long Korean events, adjacent months, both themes, and locked dragging.
- Git commit/push/PR already authorized; no merge without explicit testing approval.

## Task 1: Widget layout

- [x] Add and run a narrow-widget regression check using the existing view and dense events; record baseline behavior.
- [x] Extract shared text fitting into `text_layout.rs`; move widget rendering from `views.rs` to `desktop_widget.rs`.
- [x] Implement flat 7×6 grid, adaptive event rows, colored markers, today outline, compact controls and 960×640 default geometry. Preserve all message behaviors; guard drag when locked.
- [x] Run native tests and inspect real-renderer previews at default and fitted sizes.

## Task 2: Verification and delivery

- [x] Independent review of diff against `05be6bf`; resolve material findings.
- [x] Run formatting, workspace tests and release build; smoke-test with isolated data and document actual vs pending checks.
- [x] Prepare `artifacts/TimeSchedule-desktopcal.exe` and screenshots; commit/push/create/attach PR to the manager theme branch. Hold merge.

## Execution record

- Start: clean checkout on `codex/midnight-porcelain-ui`, base `05be6bf`. Dedicated branch `codex/desktopcal-widget` reuses the local toolchain/cache.
- Pre-flight: shared text fitting must preserve the manager calendar's 10px behavior; widget font sizes are explicit inputs.
- Ruling: retain click-to-add rather than adding DesktopCal's double-click editor — user requested visual style, and existing draft handling remains authoritative.
- Ruling: default landscape size is 960×640, retaining native fitting and existing position/opacity — a different preferred footprint can be adjusted independently later.
- Task 1: complete. Baseline narrow layout check already passed (it does not prove text raster containment). Kept as regression coverage, added actual font measurement checks for widget size and inspected renderer output. Locked-drag dispatch test failed with 1 native task vs expected 0, then passed after guard.
- Verification: workspace 61 passed / 2 intentionally ignored fixtures, manual renderer preview separately passed. Default/dark/light/fitted/small previews inspected; scrollbar now reserves space when visible.
- Final review: no actionable findings; ready for user testing, not merge. Actual DPI/drag/physical-position behavior and wallpaper readability were explicitly deferred to executable/user checks; native fitting regression tests and startup/attachment logs pass, actual visual/drag acceptance remains documented.
- Final build: `TimeSchedule-desktopcal.exe`, 5,579,264 bytes. Isolated Windows smoke loaded 6 events/2 projects/1 entry and reported attached=true, visible=true at (30,40). Own test process stopped after checking; existing user app left running.
- Delivery: implementation `44b23f5` pushed; PR #4 created against `codex/midnight-porcelain-ui` and attached to this chat. Unmerged: https://github.com/NotoriEarth/TimeSchedule/pull/4.
