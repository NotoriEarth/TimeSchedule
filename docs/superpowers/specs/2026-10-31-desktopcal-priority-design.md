# DesktopCal priority features

User-approved scope: apply the three highest-priority items from the feature comparison: date double-click quick editor, completion directly in the widget, and translucent backgrounds with opaque text. Keep Rust/Iced and the shared SQLite model. Branch from `codex/desktopcal-widget`; build a separate review EXE and PR; do not merge without user testing.

The existing proposal and the user's instruction to apply it authorize implementation. Routine presentation and integration choices below are implementation decisions, not additional user requirements.

## Behavior

- A single click on a date does not open the manager. A double-click opens one small, separate day window, containing the day's events and an add action. Existing entries retain notes/time/color/completion. Empty days start a new event. Only one editor is active across manager and quick window; pending edits cannot be replaced. Save errors, deletion confirmation, unsaved close, IME composition and undo remain protected.
- A checkbox on each visible widget event changes only completion and timestamp. It never replaces the event with the header-only data used by the widget. Clicking the checkbox does not open the day editor. The manager and widget refresh after success; failure remains visible and does not optimistically change stored state.
- The existing 40–100% preference becomes background opacity in the native app. Text and checkbox ink remain opaque. Both Midnight and Porcelain work. The manager and editor remain opaque. Existing Tauri constant-opacity behavior stays compatible.

## Approach

Use the existing editor/storage code inside a separate compact Iced window. Replacing the calendar with a single free-text record would lose existing event metadata; opening the large manager would not satisfy quick editing.

For Windows alpha, retain tiny-skia and add an explicit opt-in to a locally patched softbuffer 0.4.8 Win32 presenter. Only the native widget HWND opts in. Its existing premultiplied pixels are sent to UpdateLayeredWindow; other windows retain the upstream RGB BitBlt path. Preserve upstream licenses and document the small patch. No Explorer injection or hooks. A transparent Iced clear color and per-surface fills provide background-only alpha.

## Acceptance

Tests must cover completion preserving notes, quick-window save/reopen/close and stale loads, dirty/IME protection, checkbox versus double-click event dispatch, and rendered alpha separating solid ink from backgrounds. A Windows fixture must exercise actual softbuffer presentation on a layered child window. Build and launch a separate EXE; distinguish structural API checks from real desktop interaction and visual checks.
