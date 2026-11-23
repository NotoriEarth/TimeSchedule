# MCP power

In settings a user turns the local calendar MCP off and on without restarting TimeSchedule. Off closes the endpoint and tells separately launched helpers to stop. On starts it again on the same port.

## Sub-features

- `power-off` stops the endpoint and shows `꺼짐`.
- `power-on` starts the endpoint again and shows `실행 중` with its address.
- `power-persist` records the choice in the data folder.

## How to get to it (user POV)

- Click `설정` at the bottom of the left menu, find the section `로컬 일정 MCP`, and click `MCP 끄기` or `MCP 켜기`.

## Driving it with control-timeschedule

Preconditions:

- `control doctor -RunDir <runDir>` reports `"ok": true`.
- `<runDir>\data\mcp-power.json` has `"enabled": true`.

- **Open settings.** Click `설정`. Run `control click -RunDir <runDir> -X 68 -Y 753` and `control screenshot -RunDir <runDir> -Name power-settings`. The page heading reads `설정`. Under `데이터 저장 위치` the path is `<runDir>\data`. Under `로컬 일정 MCP` a green dot is followed by `실행 중 · http://127.0.0.1:<port>/mcp · 토큰 인증`, and `MCP 끄기` is the enabled button.
- **Turn off.** Click `MCP 끄기` at the position the last screenshot shows (about `349, 595` on an unscrolled page). Run `control click -RunDir <runDir> -X <x> -Y <y>`, wait two seconds, and run `control screenshot -RunDir <runDir> -Name power-off`. The dot is red and the text is `꺼짐`. `MCP 켜기` is now the enabled button. Each row under `연결 상태` reads `서버 꺼짐`.
- **Check the side effects of off.** `Test-Path <runDir>\data\mcp-status.json` is `False`. `Get-Content <runDir>\data\mcp-power.json` shows `"enabled": false`. `control doctor -RunDir <runDir>` exits `1` with the two MCP checks failed. `control mcp -RunDir <runDir> -Tool calendar_list -Arguments '{"from":"2026-11-01","to":"2026-11-30"}'` fails with `The credential was not sent.`
- **Turn on.** Click `MCP 켜기` (about `264` on the same row). Run `control click -RunDir <runDir> -X <x> -Y <y>`, wait two seconds, and run `control screenshot -RunDir <runDir> -Name power-on`. The dot is green and the text is `실행 중` with the same address as before.
- **Check the side effects of on.** `mcp-status.json` exists again with the instance's PID and the same port. `mcp-power.json` shows `"enabled": true` with a new `generation`. `control doctor -RunDir <runDir>` reports `"ok": true`. The `calendar_list` call succeeds.

## Gotchas

- The settings page scrolls. Between two screenshots in the seeding run the page had moved by 60 pixels and both clicks landed on plain text. Read the button position from a screenshot taken immediately before each click.
- A click on a disabled button does nothing and looks like a missed click. Check which of the two buttons is enabled first.
- The rows under `연결 상태` describe the user's real AI app configuration files. Their `다시 등록`, `등록 준비` and `검사` buttons are outside this journey. Applying a registration rewrites the user's real configuration.
- A Claude Desktop relay (`--mcp-stdio`) belongs to the user's own app and data folder. This journey cannot show that a relay closes. That needs the user's Claude Desktop and their confirmation.
- Leave MCP on when the journey ends. The other journeys need it.
