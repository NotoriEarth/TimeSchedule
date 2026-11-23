---
name: verify
description: Drive the real TimeSchedule Windows desktop app (Rust/Iced native EXE) to prove a change works. Launches an isolated second instance with its own data folder and MCP port, then drives it through the diary window (real clicks and typing), the local calendar MCP endpoint, and the record command line, and captures screenshots and transcripts. Use after changing anything under apps/timeschedule-native or crates/, or when asked to verify, prove, screenshot, or demo app behavior.
---

# Verify TimeSchedule

TimeSchedule is a Windows desktop diary and calendar. A user touches it in four places:

- the diary window (title `TimeSchedule <version> · 만년 다이어리`), the primary surface
- the desktop widget, a calendar attached to the Windows desktop, plus a tray icon
- the local calendar MCP, an authenticated HTTP endpoint on `127.0.0.1` that AI apps call
- the EXE's command line: `record ...` and the `--mcp-*` helper modes

Everything below goes through one helper, `.claude\skills\verify\control-timeschedule.ps1`. Run it from the repository root in Windows PowerShell. A command that succeeds prints JSON. A command that fails writes a PowerShell error and exits non-zero.

Read [features/README.md](features/README.md) before driving. It lists the mapped features and the rules every journey follows.

## Before you start

This PC has one desktop and the user is working on it. The instance you launch opens a real window in the middle of their screen and adds a tray icon. The window is 1160×820 pixels at 100% display scaling and larger in proportion at a higher scaling (2320×1640 at 200%). Clicks and typing are real input: the pointer moves and the window takes the foreground. Tell the user before the first click, keep input bursts short, and do not drive while they are typing elsewhere.

Never drive the user's own TimeSchedule. Their app uses `%LOCALAPPDATA%\com.timeschedule.personal.native-experiment` and MCP port `39281`. The helper refuses to act on any process it did not start.

## Launch

Build the source you are verifying. This does not touch `artifacts\TimeSchedule.exe`, the user's app:

```powershell
. .\scripts\toolchain.ps1
cargo build -p timeschedule-native --release --locked
```

The result is `target\release\timeschedule-native.exe`. Do not use `scripts\build-native.ps1` for verification. It replaces the user's EXE, which fails while their app or a Claude MCP relay is running, and belongs to the hand-over step in `AGENTS.md`.

Start an isolated instance:

```powershell
.\.claude\skills\verify\control-timeschedule.ps1 launch
```

It prints `runDir`. Pass that as `-RunDir` to every later command. Launch does this:

- creates `.test-data\verify\<timestamp>-<id>\` with `data\`, `evidence\` and `instance.json`, the record of what it started
- sets `TIMESCHEDULE_DATA_DIR` to `data\` and `TIMESCHEDULE_MCP_PORT` to a free port, for the child only
- writes `data\preferences.json` with `{"widgetVisible":false}` so no second calendar appears on the user's desktop
- waits until the instance is usable

Usable means all three: `data\mcp-status.json` names the new PID, the listener on the port proves it holds the run's token, and the diary window is visible. The app creates and migrates `schedule.db` itself, so an empty data folder is a valid start. There is no seed data. Each journey creates what it needs.

Pass `-ShowWidget` only when the change is about the desktop widget. Pass `-Exe <path>` to drive another build.

If launch prints `"ready": false`, run `stop` with the printed `runDir` before trying again.

A run directory is always a folder directly under `.test-data\verify\`. The helper refuses any other path, and any run folder that is a link. Do not edit `instance.json`. It is how the helper knows which process is its own.

## Doctor

```powershell
.\.claude\skills\verify\control-timeschedule.ps1 doctor -RunDir <runDir>
```

Read-only. Run it first whenever something looks off. It checks that:

- the process launch started is alive (same PID, start time and EXE)
- the MCP status file belongs to that process and the listener proves it holds the run token
- the data folder is inside the run folder and is not the user's data folder
- `native.log` shows storage opened at startup
- the version of the running EXE equals `apps\timeschedule-native\Cargo.toml`
- the EXE on disk is the one that was launched and is not older than the newest non-test source

A failed `EXE is not older than the source` means rebuild and launch again. If `cargo build` then finishes without compiling anything, the newer file is not part of the binary. Say so in the proof.

Doctor fails on the two MCP checks while MCP is turned off in settings. That is expected during the [MCP power](features/mcp-power.md) journey and nowhere else.

## Drive

### Diary window

The app draws its own pixels and exposes no accessibility tree. The handles are the Korean labels on screen, named in `docs\ui-names.md`. Coordinates are found, not remembered:

```powershell
.\.claude\skills\verify\control-timeschedule.ps1 screenshot -RunDir <runDir> -Name 01-month
```

Read the PNG, find the label, and use its pixel position. Screenshot pixels and click coordinates are the same client-area pixels, whatever the display scaling. The `width` and `height` in the screenshot command's output are the size of that area.

The numbers below are examples from one screenshot. They are not positions to reuse: the same label is somewhere else at another display scaling, window size or scroll position.

```powershell
.\.claude\skills\verify\control-timeschedule.ps1 click -RunDir <runDir> -X 917 -Y 232
.\.claude\skills\verify\control-timeschedule.ps1 click -RunDir <runDir> -X 546 -Y 515 -Double
.\.claude\skills\verify\control-timeschedule.ps1 type  -RunDir <runDir> -Text '화면 일정'
.\.claude\skills\verify\control-timeschedule.ps1 key   -RunDir <runDir> -Key Enter
```

- `click` brings the window to the foreground, then moves the pointer and clicks in one burst. It refuses when Windows does not give the window the foreground or another window covers the point. Nothing is sent in either case.
- While the screen is locked no window has the foreground. `click`, `type` and `key` are then refused with `Windows did not give the window the foreground` and nothing is sent. `launch`, `doctor` and `screenshot` still work. Input needs the user at an unlocked desktop.
- `type` sends Unicode text to the focused field. It works for Korean whatever the IME state. Use `-TextFile <utf8 file>` for long text. It stops as soon as the window loses the foreground.
- `key` accepts `Enter`, `Escape`, `Tab`, `Backspace`, `Delete`, `Left`, `Right`, `Up`, `Down`, `Home`, `End`.
- `-Window quick` targets the quick schedule window (`… · 빠른 일정`) and `-Window jump` the year and month picker (`TimeSchedule · 연·월 이동`). The default is `diary`.
- `windows -RunDir <runDir>` lists the instance's top-level windows with titles and sizes.

Take a screenshot before every click and after every action. Scrollable pages move between screenshots.

Input is real input. It reaches whichever window is in front at the instant it is sent, and the helper can only check that immediately before. The app ignores input posted to its window in the background, so there is no safer route. That is the reason for telling the user first.

### Local calendar MCP

```powershell
.\.claude\skills\verify\control-timeschedule.ps1 mcp -RunDir <runDir> -Tool calendar_create -Arguments '{"title":"검증 회의","date":"2026-11-24"}'
```

Tools are `calendar_list`, `calendar_get`, `calendar_create` and `calendar_update`. Inputs are in `docs\local-mcp.md` under "도구와 충돌 처리". The helper confirms the listener is this run before it sends the token, prints the raw JSON-RPC reply, and exits `1` when the reply is an error or `isError`. Use `-ArgumentsFile <utf8 json>` when quoting gets in the way. The token is never printed.

### Command line

```powershell
.\.claude\skills\verify\control-timeschedule.ps1 cli -RunDir <runDir> -CliArguments record,drain
```

`cli` runs the EXE the run was launched from, so the command line and the window are the same build. It adds `--data-dir <runDir>\data` itself, waits up to 60 seconds, prints `exitCode`, `stdout` and `stderr`, and exits `1` when the command failed. Subcommands and their meaning are in `docs\project-records.md`.

Do not call the EXE directly. Without `--data-dir` it acts on the user's data folder. `cli` needs the instance running, and while it runs the instance holds the data folder's lock: `record drain` answers `"outcome": "queued"`. A journey that needs the command line to own the data folder is not covered by this helper yet.

### Files

The run's `data\` folder is the stored state: `schedule.db`, `preferences.json`, `mcp-power.json`, `mcp-status.json` and `native.log`. Reading them is a valid second view of a change. Do not edit them while the instance runs, except where a journey says so.

## Evidence

Everything goes to `<runDir>\evidence\`. Screenshots land there by name (a name is used once; an existing file is never overwritten), and the command's output gives the `pid` the picture was taken from. Every TimeSchedule of one version has the same window title, so that PID, or the data path on the settings page, is what ties a picture to the run. Every `cli` call is appended to `cli-transcript.jsonl`. Every MCP call is appended to `mcp-transcript.jsonl` (UTF-8, one call per line) with its arguments, HTTP status and reply. A call that got no reply is recorded with status `0` and `outcome unknown`: a write may still have been stored, so list before repeating it. Save copies of state files there yourself.

A proof meets these standards:

- It exercises the path the user takes. A change to the editor is proven through the editor, not by creating the same row over MCP.
- It shows the action and the resulting state: a screenshot before, the action, a screenshot after.
- It checks the side effect through a second view. A save in the window is confirmed with `calendar_list` or the file. An MCP write is confirmed in the window.
- It uses the EXE built from the change. Doctor passed after the last build.
- It names what was not driven. A proof through one entry point does not cover the others the feature file lists.

`.test-data\` is ignored by Git. Cite evidence paths in the PR and verification notes, or copy the few files that must be kept.

## Cleanup

```powershell
.\.claude\skills\verify\control-timeschedule.ps1 stop -RunDir <runDir>
```

Stops only the process that launch started, identified by PID, start time and EXE path. It never matches by process name, so the user's app is safe. It removes `<runDir>\data\` and keeps `<runDir>\evidence\`. Its output lists the evidence files. Pass `-KeepData` to keep the data folder for inspection.

Stop retires the run by renaming `instance.json` to `instance.stopped.json` before it deletes anything. Every later command on that run is refused, and a second `stop` deletes nothing. When the output has a `dataLeft` path and you did not pass `-KeepData`, the data folder could not be fully deleted and `stop` exits `1`: delete that folder yourself once nothing holds it open.

If `stop` says the process cannot be opened or its EXE cannot be read, nothing was changed. Run it from 64-bit Windows PowerShell.

Run `stop` after every run, including failed ones. A leftover instance keeps its tray icon and its port.

## Helpers

`control-timeschedule.ps1` is the only helper. Its commands are `launch`, `doctor`, `mcp`, `cli`, `windows`, `screenshot`, `click`, `type`, `key` and `stop`, each shown above. The file is ASCII-only on purpose: Windows PowerShell 5.1 reads a script without a byte-order mark in the system code page and would mangle Korean literals.

## Never do this in a verification run

- Do not apply a registration in **설정 → 로컬 일정 MCP** (`다시 등록`, `등록 준비`, then `등록 적용`). It rewrites the user's real `~\.codex\config.toml`, `~\.claude.json` or Claude Desktop configuration to point at the scratch instance. The connection board itself only reads them.
- Do not turn on `Codex·Claude Code 채팅 자동 수집` for a project. It copies the user's real chat history into the run's database and into your evidence.
- Do not press `채팅에서 경험 검토 · Terra` or `백엔드 포트폴리오 · Markdown 생성`. They send content to an external model through the user's Codex login.

When a change is about one of these, stop and ask the user how they want it verified.
