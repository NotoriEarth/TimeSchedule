# Widget customization

The user accepted the preceding Rust design and requested all three features, clarifying the year/month interaction with an image. This is implementation authorization, not a request for another approval round. Keep the existing Rust/Iced app and previous priority features. Base: `3307e0c`, branch `codex/widget-customization`, stacked on PR #5; no merge until user testing.

## Behavior

- Geometry: unlock to drag the bottom-right resize grip or enter logical width/height in settings. Persist size; clamp to monitor/parent bounds and preserve normal controls while locked. Existing position, opacity and desktop integration survive. Default size remains 960×640; manual inputs 320–3840 by 220–2160 logical pixels; OS recovery may fit smaller screens.
- Range: default monthly view shows the whole month in 4–6 Sunday-first weeks. Optional rolling view shows 2–8 weeks with 0–2 past weeks (strictly less than total). Default rolling selection is 4 weeks/0 past. UI and query use exactly the same date range. Today-following and historical browsing are distinct: browsing must not jump back at midnight. Today resumes following.
- Appearance: installed font selection with fallback, date/event font sizes, cell gap/padding/event-line spacing, compact/default/comfortable presets and reset. Per-theme overrides for background/text/today/weekend/border; background-only alpha retained. Text measurement uses the same font as drawing. Settings immediately preview and coalesce persistence; appearance changes do not reload the DB.
- Navigation: year and month in the widget heading are separate buttons, separate from drag. Either opens the same small window displaying both current year and month; the clicked field is focused/selected. User can edit both and apply, or cancel/Escape without changing the calendar. Support exact date input in the same window. Invalid/out-of-range dates show an inline error; accepted navigation highlights the selected day without opening an event editor. Supported anchor dates: 1900-01-01 through 9998-12-31.
- Existing unsaved event drafts and settings compatibility must survive. No event DB migration; extend JSON preferences with defaults. Color data contains values, not Iced types. No JS/WebView/runtime dependency.

## Implementation decisions

Use a date-only core viewport function, serializable desktop widget preferences, and native presentation/settings/navigation modules. Do not add scattered constants and separate date calculations; a shared result prevents mismatched fetches. Use Iced window resizing actions for the grip and the owned Windows adapter for validated numeric resizing, with native size lock and recovery. Keep settings in the manager, accessible through the widget gear. Year/month navigation uses one separate compact Iced window.

## Acceptance

Verify legacy and custom preference round trips, validation, month lengths/leap/year boundaries, rolling ranges, DB scope equality, font measurement, dense/small layout, per-theme color isolation, navigation focus/apply/cancel/invalid input, no editor loss, size lock and native resize fixture. Render previews, build a separate EXE and smoke-test isolated data. Distinguish native API/renderer tests from live desktop pointer/IME verification.
