# Search

The search box at the top right of the diary window finds events, projects and records by title or body. Results replace the current page while the box has text and the page returns when the box is cleared.

## Sub-features

- `search-match` lists items whose title or body contains the text.
- `search-empty` shows `일치하는 기록이 없습니다.` for text nothing contains.
- `search-clear` returns to the previous page when the box is emptied.

## How to get to it (user POV)

- Click the box that reads `일정, 프로젝트, 기록 검색` at the top right of any diary page and type.

## Driving it with control-timeschedule

Preconditions:

- `control doctor -RunDir <runDir>` reports `"ok": true`.
- An event titled `검증 회의` with the note `verify run` exists. Create it with `control mcp -RunDir <runDir> -Tool calendar_create -Arguments '{"title":"검증 회의","date":"2026-11-24","notes":"verify run"}'`.
- The search box is empty.

- **Focus the box.** Click `일정, 프로젝트, 기록 검색`. Run `control screenshot -RunDir <runDir> -Name search-before` and `control click -RunDir <runDir> -X 1030 -Y 34`. The box border is highlighted.
- **Search for a match.** Run `control type -RunDir <runDir> -Text '검증'`, wait one second, and run `control screenshot -RunDir <runDir> -Name search-match`. The heading `검색 결과` appears with the line `최대 50개 · 제목 또는 본문 검색` and one card showing `검증 회의`, `verify run` and `2026-11-24`.
- **Search for nothing.** Run `control type -RunDir <runDir> -Text 'zzqq'`, wait one second, and run `control screenshot -RunDir <runDir> -Name search-empty`. The box reads `검증zzqq`. The heading `검색 결과` remains, no card is listed, and the line `일치하는 기록이 없습니다.` appears.
- **Clear.** Press Backspace once for each character in the box: run `control key -RunDir <runDir> -Key Backspace` six times. Run `control screenshot -RunDir <runDir> -Name search-cleared`. The box shows its placeholder again and the page shown before the search is back.

## Gotchas

- Results arrive after a short input delay. A screenshot taken immediately after typing can show the previous results.
- `type` appends to what the box already holds. Check the box in a screenshot before typing.
- Text shorter than three characters is matched by scanning, longer text through the index. A change to either path needs a query of that length.
- Search reads from the database only. An event being typed in an open editor is not found until it is saved.
