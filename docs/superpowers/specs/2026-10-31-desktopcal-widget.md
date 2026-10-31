# DesktopCal-style native desktop widget

The user explicitly requested applying the DesktopCal design style to the desktop widget after selecting the Midnight/Porcelain manager design. This is authorization to implement the widget style; no manager redesign or PR merge is requested.

Reference: https://www.desktopcal.com/ko and its official `first_look_bid_usa.jpg` screenshot, inspected in the browser. Adopt a flat, wallpaper-visible calendar with thin rules, large date cells, date labels at the top left and a compact month toolbar. Use our own rendering and assets.

- Default widget geometry becomes 960×640 logical pixels; native work-area/DPI fitting continues to clamp oversized windows and restore physical position.
- Six full weeks, Sunday first, previous/next month and today navigation remain. Current day gets an outline/date badge; adjacent-month dates are subdued. Entries use colored markers and readable left-aligned titles.
- Preserve existing click-to-add behavior, manager opening, lock, dragging, opacity, visibility, native desktop attachment and persisted preferences.
- Consolidate controls into a compact footer. The old duplicate today agenda is replaced by a today date/count in the header; today's entries remain in calendar cells and manager details.
- Adapt event count and title truncation to actual cell space. Very small fitted windows scroll vertically instead of drawing outside cells or losing controls.
- Separate widget layout/style constants in `desktop_widget.rs`; share text fitting without changing the manager's appearance. Use existing Midnight/Porcelain selection and existing whole-window opacity. No new runtime dependencies, blur or timers.
- Deliver a separate executable, renderer previews, tests, independent review and a PR based on `codex/midnight-porcelain-ui`; do not merge.
