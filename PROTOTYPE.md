# TimeSchedule prototype — 2026-10-28

This is an exploratory prototype, not a production release. The user chose Tauri and a desktop-only calendar widget: it should remain on the desktop when ordinary windows are minimized, never float above other applications, and emphasize today with today's agenda.

## Scope and defaults

- Windows 10/11 desktop, one person, offline first.
- Tauri 2, Svelte 5 + TypeScript, Rust, SQLite. Only Tauri and the desktop-only presentation were explicitly chosen by the user; the other technologies are implementation defaults.
- Korean interface, monthly calendar, selected-date agenda, event create/edit/delete, today's highlight. Small tasks and notes may be included if they do not delay the desktop experiment.
- Manager window plus a wide desktop month calendar (up to 900×560 logical pixels, fitted to the monitor). Manager close destroys its WebView while the tray and widget remain; explicit Quit exits.
- Revision 0.1.1 follows the user's DesktopCal-style translucency feedback: daily event snippets, an explicit Move/Lock control, a deep header drag region, and adjustable 40–100% whole-widget opacity. Native constant alpha also affects text; this is not per-pixel background-only transparency.
- Revision 0.1.2 recovers off-screen widget geometry after display changes, including while locked, and provides a recovery action in the manager and tray. The UI-thread timer preserves the widget's parent, alpha and visibility; Explorer-restart recreation remains outside the scope.
- SQLite stored in the OS app-data directory. No Google login, AI requests, telemetry, automatic startup, or server deployment in this prototype.
- A browser preview may use explicitly identified local browser storage; the packaged native application uses SQLite. Browser storage must not be presented as native persistence.

## Shared integration contract

Frontend files are owned by the frontend agent. Rust persistence and command implementation are owned by the backend agent. `src-tauri/src/desktop.rs` is owned by the desktop agent. Root owns setup/configuration, tray/window wiring, build environment, integration and verification.

JSON uses camelCase.

```ts
type EventColor = 'sage' | 'blue' | 'amber' | 'rose';
type CalendarEvent = {
  id: string; title: string; date: string; startTime: string | null;
  endTime: string | null; allDay: boolean; notes: string; color: EventColor;
  completed: boolean; updatedAt: string;
};
type EventInput = Omit<CalendarEvent, 'id' | 'updatedAt'> & {id?: string};
type Task = {id: string; title: string; date: string | null; completed: boolean; updatedAt: string};
type Note = {id: string; title: string; body: string; updatedAt: string};
```

Tauri commands:

- `list_events({from: 'YYYY-MM-DD', to: 'YYYY-MM-DD'})` → `CalendarEvent[]`, inclusive date range.
- `save_event({input: EventInput})` → `CalendarEvent`.
- `delete_event({id})` → void.
- `list_tasks()` → `Task[]`.
- `save_task({input: {id?: string,title,date: string|null,completed}})` → `Task`.
- `delete_task({id})` → void.
- `list_notes()` → `Note[]`.
- `save_note({input: {id?: string,title,body}})` → `Note`.
- `delete_note({id})` → void.
- `get_app_info()` → `{storagePath: string, version: string, desktopAttached: boolean, desktopMessage: string, widgetLocked: boolean, widgetVisible: boolean, widgetOpacity: number}`. Visibility and opacity are observed from the live widget, not just saved preferences.
- `show_manager({date?: string})` → void; emit `select-date` with date when provided.
- `show_widget()` → void.
- `hide_widget()` → void.
- `set_widget_locked({locked: boolean})` → void.
- `set_widget_opacity({opacity: number})` → void; integer 40–100, persisted after native validation.
- Successful writes emit `data-changed` for both windows to refresh.
- Desktop visibility/lock changes emit `desktop-changed`; native failures emit `app-error`.

Frontend selects widget mode with `?view=widget`. Desktop widget is read-first; clicking a date opens the manager. Native errors must be visible, never silently treated as a successful save. No fake persistent sample events; an explicit sample-data action is acceptable.

## Desktop acceptance

The desktop module must report whether native attachment actually succeeded. `alwaysOnBottom` alone is insufficient. Explorer shell attachment is an experimental OS-specific technique; unsupported configurations must show a clear status rather than claim desktop integration succeeded. Verify desktop visibility with Show Desktop, ordinary-window occlusion, manager interaction, and clean quit. Do not restart or kill Explorer.

## Verification

Build the frontend, run type checks, compile/check the native application, exercise SQLite validation and persistence, run the app, verify desktop placement and basic event editing, and inspect process memory. Record any unverified behavior or environment prerequisite honestly.
