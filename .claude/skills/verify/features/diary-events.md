# Diary events

In the month diary a user picks a date, sees that date's events in the panel on the right, and adds an event through the editor. The saved event appears in the date cell and in the panel.

## Sub-features

- `event-select` shows the clicked date as the selected date with its events in the panel.
- `event-open-editor` opens the editor titled `일정 추가` with the date filled in.
- `event-save` stores the title and shows the event in the cell and the panel.
- `event-cancel` closes the editor without storing anything.

## How to get to it (user POV)

- Click a date cell, then `+ 일정` in the selected date panel under the `○월 ○일` heading.
- Click the `+ 일정` button at the top right of the month diary.
- Double-click a date cell. The hint under the calendar reads `두 번 클릭해 일정 추가`.
- Double-click a date in the desktop widget to open the quick schedule window, then `+ 일정` there.

## Driving it with control-timeschedule

Preconditions:

- `control doctor -RunDir <runDir>` reports `"ok": true`.
- The diary window shows the month diary (`월간` is highlighted in the left menu).
- No event titled `화면 일정` exists on the chosen date.

- **Select the date.** Click the cell of the 24th. Run `control screenshot -RunDir <runDir> -Name event-before`, then `control click -RunDir <runDir> -X 458 -Y 500`, then `control screenshot -RunDir <runDir> -Name event-selected`. The cell is highlighted and the panel heading reads `11월 24일`.
- **Open the editor.** Click `+ 일정` in the panel. Run `control click -RunDir <runDir> -X 917 -Y 232` and `control screenshot -RunDir <runDir> -Name event-editor`. A dialog titled `일정 추가` covers the calendar. The cursor is in `제목`, `날짜 (YYYY-MM-DD)` reads `2026-11-24`, `하루 종일` is checked and `완료` is not.
- **Enter the title.** Run `control type -RunDir <runDir> -Text '화면 일정'`. The `제목` field shows the text.
- **Save.** Click `저장하기`. Run `control click -RunDir <runDir> -X 844 -Y 652` and `control screenshot -RunDir <runDir> -Name event-saved`. The dialog is gone. The cell of the 24th and the panel both list `화면 일정`. The status line reads `내 컴퓨터에 저장했습니다.`
- **Confirm it is stored.** Run `control mcp -RunDir <runDir> -Tool calendar_list -Arguments '{"from":"2026-11-24","to":"2026-11-24"}'`. The reply lists `화면 일정` with `"allDay":true`.
- **Double-click entry.** Double-click the cell of the 25th. Run `control click -RunDir <runDir> -X 546 -Y 530 -Double` and `control screenshot -RunDir <runDir> -Name event-doubleclick`. The `일정 추가` dialog opens with the date `2026-11-25`.
- **Cancel.** Click `취소`. Run `control click -RunDir <runDir> -X 775 -Y 652` and `control screenshot -RunDir <runDir> -Name event-cancelled`. The dialog is gone and the 25th has no event. `calendar_list` for the 25th returns no events.

## Gotchas

- Click the empty part of a date cell. A click on an event inside the cell opens that event instead of selecting the date.
- The editor keeps its position only while the window is 1160×820. Read `저장하기` and `취소` from the screenshot of the open dialog.
- Pressing Enter in `제목` does not save. The editor stays open and shows `미저장 변경`. Click `저장하기`.
- Typing goes to the field with the cursor. If the screenshot shows no cursor in `제목`, click the field first.
- Closing an editor that has unsaved text asks `저장하지 않고 닫을까요?`. That confirmation is a separate behavior and is not covered here.
- The top right `+ 일정` button and the widget's quick schedule window were not driven when this map was written. Drive them and add their steps before reporting them verified. The widget path needs `control launch -ShowWidget`.
