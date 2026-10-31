# Native themes implementation plan

> **For agentic workers:** Use superpowers:executing-plans for inline implementation and one independent whole-change review.

**Goal:** Apply approved Midnight/Porcelain designs to the native diary, with a persisted theme toggle.
**Architecture:** A small theme module owns semantic colors and widget styles. Existing preference serialization and serialized save tasks persist the theme; views use the active palette without changing the data model.
**Tech Stack:** Rust, Iced 0.14 tiny-skia, existing SQLite and Windows adapters.
**Spec:** `docs/superpowers/specs/2026-10-31-native-themes-design.md`

## Global constraints

- Keep the native renderer and existing data/tray/editor/widget behavior.
- Default Midnight; persist Porcelain; old/unknown themes default Midnight.
- No new runtime dependencies or animation timers.
- Commit/push/PR are authorized; merge remains pending user testing.

## Review focus

- Old settings retain widget position, opacity and visibility.
- Rapid theme changes during an in-flight settings save keep the last choice.
- Date selection does not open an unwanted editor; explicit create uses that date.
- Narrow windows, long text and both themes keep controls reachable and readable.
- Dirty editor/confirmation and widget interactions still preserve their state.

## Task 1: Preferences and behavior

Files: desktop `preferences.rs`, native `app.rs` and `main.rs`.
Interface: `AppTheme::{Midnight, Porcelain}`, `Preferences.theme`, `Message::ToggleTheme`, `App::theme`.
- [x] Add migration/round-trip and date-selection regression tests; run and observe failures.
- [x] Implement theme persistence, immediate toggling through the existing serialized save path, selection behavior and window-size tracking.
- [x] Run the native and desktop tests; verify rapid-toggle and editor-preservation behavior.

## Task 2: Native visual system

Files: new native `theme.rs` and `icons.rs`; update `views.rs`.
Consumes `Preferences.theme`; produces semantic Iced styles shared by every view.
- [x] Implement both palettes, consistent controls and lightweight vector icons.
- [x] Rebuild sidebar/calendar/date inspector and adapt all remaining views and modal surfaces.
- [x] Verify bounded layouts at minimum/default sizes and test the release executable with isolated sample data.

## Task 3: Review and delivery

- [x] Run `cargo fmt --all -- --check`, `cargo test --workspace --locked`, and release build.
- [x] Independent whole-change review; fix material findings and rerun relevant verification.
- [x] Prepare `artifacts/TimeSchedule-themed.exe`, screenshots and a concise verification guide.
- [ ] Commit, push, create and link a PR to the Rust experiment branch; do not merge.

## Execution record

- Clean starting checkout: `32d192d`, `codex/rust-native-experiment`. Work branch: `codex/midnight-porcelain-ui`.
- Existing workspace/toolchain retained on the dedicated branch to reuse local build tools and cache.
- Pre-flight: Task 2 consumes Task 1's theme preference; no conflicting interfaces.
- Preference/date-selection tests failed before implementation, then passed. Final suite: 59 passed, 2 intentionally ignored fixtures; the manual renderer preview test was separately run and passed.
- Independent review found text overflow and short-window calendar overflow. Actual font measurement, clipping and height-adaptive chip counts resolve both; second review found no remaining P1/P2.
- Final release: 5,575,168 bytes. Isolated Windows startup loaded 6 events / 2 projects / 1 diary entry; both windows initialized and desktop attachment succeeded.
- Windows capture timed out. Same-renderer PNGs were inspected at wide/default/short/narrow sizes; actual clicking, IME and application restart remain manual checks. See `docs/native-themes-verification.md`.
