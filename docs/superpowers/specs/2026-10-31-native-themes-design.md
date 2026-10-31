# Midnight / Porcelain native UI

The user selected the Midnight mockup, requested a Porcelain light-mode toggle, and explicitly asked to apply the design. This records that approved direction; implementation proceeds without another approval gate.

- Keep the Rust + Iced native renderer and existing local data, tray, editor and widget functionality.
- Default to charcoal Midnight with lavender accents. Offer a clearly labeled light/dark toggle; persist the choice in native preferences and apply it to all app windows.
- Use a restrained sidebar, thin calendar rules, colored event chips and a selected-date inspector containing events, dated tasks and diary records. Clicking a calendar date selects the inspector; explicit add buttons open the editor.
- Porcelain uses a white/cool-gray base and blue accents with the same layout and actions.
- Theme every page, editor, confirmation and widget through shared semantic colors. Avoid bitmap backgrounds, blur, animation loops and new runtime dependencies.
- Preserve six calendar rows and existing week navigation. On narrow windows, put the date inspector below the calendar so all actions remain reachable by scrolling.
- Preserve existing widget preferences when migrating files without a theme. Unknown theme values fall back to Midnight.

## Approaches

The preview presented Midnight, Porcelain and warm Atelier. The user chose Midnight with Porcelain as a toggle. This uses existing Iced styling instead of changing the renderer or adding a WebView.

## Delivery

Build a separate review executable, document real checks and remaining manual checks, commit/push a dedicated codex branch, create a PR based on the existing Rust experiment branch, and wait for user testing before merge.
