# Local calendar MCP

An AI app connected to the running TimeSchedule can list, read, create and update calendar events through an authenticated endpoint on `127.0.0.1`. A write lands in the same database the window uses, and the open diary window shows it without a restart.

## Sub-features

- `mcp-create` saves a new event and returns it with an `id` and `updatedAt`.
- `mcp-list` returns the events of a date range without their notes.
- `mcp-get` returns one event with its notes.
- `mcp-update` changes the given fields of an event when `expectedUpdatedAt` is current.
- `mcp-conflict` refuses an update whose `expectedUpdatedAt` is stale and leaves the event unchanged.
- `mcp-refresh` shows an MCP write in the open diary window.

## How to get to it (user POV)

- An AI app registered through **설정 → 로컬 일정 MCP** calls a calendar tool while TimeSchedule is running.
- A script sends the same JSON-RPC request to the endpoint shown in **설정 → 로컬 일정 MCP** with the app's token.

## Driving it with control-timeschedule

Preconditions:

- `control doctor -RunDir <runDir>` reports `"ok": true`.
- The diary window shows the month that contains the event date.
- No event titled `검증 회의` exists.

- **Create.** Run `control mcp -RunDir <runDir> -Tool calendar_create -Arguments '{"title":"검증 회의","date":"2026-11-24","allDay":false,"startTime":"09:30","endTime":"10:15","color":"blue","notes":"verify run"}'`. Exit code `0`. The reply text holds the event with an `id` and an `updatedAt`. Keep both.
- **List.** Run `control mcp -RunDir <runDir> -Tool calendar_list -Arguments '{"from":"2026-11-01","to":"2026-11-30"}'`. The reply lists `검증 회의` with the same `id`, has no `notes` field, and `truncated` is `false`.
- **Read.** Run `control mcp -RunDir <runDir> -Tool calendar_get -Arguments '{"id":"<id>"}'`. The reply includes `"notes":"verify run"`.
- **See it in the window.** Run `control screenshot -RunDir <runDir> -Name mcp-created`. The cell of the 24th shows `09:30 검증 회의`. The status line at the bottom reads `외부 도구의 일정 변경을 반영했습니다. 작성 중인 초안은 유지됩니다.`
- **Update.** Run `control mcp -RunDir <runDir> -Tool calendar_update -Arguments '{"id":"<id>","expectedUpdatedAt":"<updatedAt>","title":"검증 회의 변경"}'`. Exit code `0`. The reply has the new title and a newer `updatedAt`.
- **Conflict.** Run the update again with the first `updatedAt` and `"title":"덮어쓰기"`. Exit code `1` and the reply has `"isError":true`. Run `calendar_get` again. The title is still `검증 회의 변경`.
- **Proof.** Run `control screenshot -RunDir <runDir> -Name mcp-updated`. The cell shows the changed title. `<runDir>\evidence\mcp-transcript.jsonl` holds every call above in order.

## Gotchas

- A tool refusal answers HTTP 200 with `"isError":true`. Trust the helper's exit code or the `isError` field, not the HTTP status.
- `calendar_create` is not idempotent. A repeated call after a timeout makes a duplicate. List first.
- The window refreshes within about two seconds of a write. Wait before the screenshot.
- `null` values are refused. Omit a field to keep it.
- A timed event needs `allDay:false`, `startTime` and `endTime` together, with the end after the start.
- The helper calls tools without `initialize`, so the run does not appear in the connection board as a connected AI app. Proving the board needs a client that sends `initialize`.
- While MCP is turned off in settings the helper refuses to send the token. Turn it back on first.
