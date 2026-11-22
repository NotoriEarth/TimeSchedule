# TimeSchedule verification map

This directory is the maintained source for verifying the user-facing behavior of the TimeSchedule native app. Read this index before driving the app, then use the matching feature file as the recipe. The harness is `.claude\skills\verify\control-timeschedule.ps1`, described in [../SKILL.md](../SKILL.md). Below it is written `control` and `<runDir>` is the folder `launch` printed.

## Baseline preconditions

- Build with `cargo build -p timeschedule-native --release --locked` after `. .\scripts\toolchain.ps1`.
- Start with `control launch`. The instance has an empty data folder at `<runDir>\data`, its own MCP port, and a hidden desktop widget.
- `control doctor -RunDir <runDir>` reports `"ok": true`.
- The diary window is 1160×820 and shows the month of today in the Midnight (dark) theme.
- There is no seed data. A journey creates the events it needs and says so in its preconditions.
- Never drive an instance that this run did not launch.

## Driving conventions

- Start every recipe from the baseline state unless its preconditions say otherwise.
- The handle for a control is its Korean label, exactly as written in the recipe. Find the label in a fresh screenshot and click its pixel position.
- Coordinates in the recipes are reference positions for the baseline window in version 0.31.1. Confirm each one in a screenshot taken immediately before the click. A resized window, a scrolled page or a changed layout moves them.
- Take a screenshot after every action. It is both the proof and the source of the next coordinates.
- Clicks and typing are real input on the user's desktop. Tell the user before the first one.
- Treat every command as literal. Keep quoted names and JSON unchanged apart from dates and IDs.
- Dates in recipes are examples. Use dates in the month the window shows.

## Proof and skip reporting

- Capture the user action and the resulting state, not only the final screen.
- Window proof is a screenshot before and after, saved in `<runDir>\evidence`. The `pid` in the screenshot command's output, or the data path on the settings page, identifies the instance. The window title does not: the user's own app has the same one.
- MCP proof is the entry in `<runDir>\evidence\mcp-transcript.jsonl`.
- Command-line proof includes the command, its output and its exit code.
- Mutation proof includes a second view of the stored value: the window for an MCP write, `calendar_list` or a state file for a window action.
- Record the feature ID and the entry point used with every artifact.
- Report an unreachable path with the attempted command and the unmet precondition.
- Do not report a skipped entry point as verified through a different path.

## Feature entry contract

Each feature file starts with an H1 title and one paragraph describing the user-visible behavior. It then uses exactly four H2 sections in this order.

1. `Sub-features` lists short IDs with one line for each behavior.
2. `How to get to it (user POV)` lists every user entry point.
3. `Driving it with control-timeschedule` starts with `Preconditions:` and uses labeled bullets that pair each user action with an exact command and observable result.
4. `Gotchas` lists traps that can waste or invalidate a verification run.

Keep implementation details out of the map. Name only user paths, labels, required state, commands, and observable proof.

## Features

- [Local calendar MCP](./local-calendar-mcp.md) covers listing, reading, creating and updating events from an AI app, the update conflict, and the refresh of the open window.
- [Diary events](./diary-events.md) covers selecting a date and adding an event through the editor from the month diary.
- [Search](./search.md) covers the search box at the top of the diary window with matching, empty and cleared states.
- [MCP power](./mcp-power.md) covers turning the local calendar MCP off and on in settings.

## Not mapped yet

These surfaces have no feature file. Verifying a change to one of them starts with adding its file.

- Desktop widget, quick schedule window, year and month picker, tray menu
- Week diary, projects, record archive, undated events, notes, categories
- Record command line (`record enqueue`, `drain`, `status`) and automatic development records
- AI connection helper, token reissue, chat capture, portfolio generation (see "Never do this" in the skill before touching them)
