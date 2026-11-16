# 위젯 날짜 칸 가독성 구현 계획

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 위젯·다이어리 월 칸이 항목을 한 줄씩 크게 보여 주고, 넘치는 항목은 숫자 대신 제목으로 보여 준다.

**Architecture:** 칸 그리기를 새 모듈 `day_cell.rs`로 옮기고 `calendar_ui::day_items`는 그 모듈을 부른다. 날짜 창·옆 목록의 `item_view`는 그대로 둔다. 위젯 틀은 아래쪽 바를 없애고 잠금 버튼을 위쪽 줄로 옮긴다.

**Tech Stack:** Rust, Iced 0.14 (tiny-skia), 기존 `text_layout` 측정 함수.

**Spec:** `docs/superpowers/specs/2026-11-16-widget-cell-readability-design.md`

## Global Constraints

- 새 라이브러리·상주 작업·주기 타이머·새 조회 없음. 날짜당 이미 읽는 16개 항목만 쓴다.
- 이미 저장된 사용자 설정값은 바꾸지 않는다. 기본값만 바꾼다(일정 글자 13, 날짜 글자 13, 셀 안쪽 4, 줄 간격 1).
- 줄 높이 = 글자 크기 × 1.3, 줄 사이 = 위젯은 `line_gap`, 다이어리는 1px.
- 기록 머리말(프로젝트)은 줄 너비의 40%까지.
- 버전 0.17.0, `Cargo.toml`·`Cargo.lock`·`CHANGELOG.md`·README 함께 갱신.

## Review Focus

- 아주 좁거나 낮은 칸(글자 24, 칸 높이가 한 줄도 안 됨): 아무 글씨도 칸 밖으로 나가지 않고, 빈 칸으로 남는다 → Task 1 `day_cell_text_stays_inside_cell`.
- 줄바꿈·탭이 든 제목: 한 줄로 바뀌어 표시된다 → `summary` 단위 테스트와 `text_layout` 기존 정규화 사용.
- 날짜에 17개 이상(16개만 읽힘): 마지막 줄의 `+N`이 읽지 않은 항목까지 센다 → `summary_counts_items_beyond_loaded`.
- 완료 항목만 있는 날: `완료 N`만 보인다 → `day_cell_shows_completed_line_when_room`.
- 프로젝트가 없는 기록: 머리말 없이 작업명만 → `day_cell_record_without_project_has_no_prefix`.

---

### Task 1: 칸 전용 줄과 마지막 줄

**Files:**
- Create: `apps/timeschedule-native/src/day_cell.rs` (+ `main.rs`/모듈 선언)
- Modify: `apps/timeschedule-native/src/calendar_ui.rs` (`day_items`가 `day_cell::view` 호출), `apps/timeschedule-native/src/text_layout.rs` (`split_first_line`)
- Test: `apps/timeschedule-native/src/app/tests/visual.rs` 의 넘침 테스트 4개 교체, `day_cell.rs` 단위 테스트

**Interfaces:**
- Produces: `day_cell::view<'a>(app: &'a App, day: Option<&'a CalendarDay>, width: f32, height: f32, size: f32, widget: bool) -> Element<'a, Message>`
- Produces: `day_cell::summary(titles: &[&str], beyond: u32, completed: u32, width: f32, size: f32, font: Font) -> String`
- Produces: `text_layout::split_first_line(content: &str, width: f32, size: f32, font: Font) -> (String, String)` — 첫 줄에 들어가는 앞부분과 나머지. 뒤쪽 절반 안에 공백이 있으면 그 공백에서 끊는다.

- [ ] **Step 1: 실패하는 테스트 작성** — 렌더링 테스트: 숨은 제목이 `A · B · C`로 마지막 줄에 보임, `개 더` 없음, 기록 한 줄(머리말과 작업명이 같은 y), 남는 줄에서 잘린 제목이 두 줄, 완료만 남으면 `완료 N`, 글자 9·13·24에서 모든 글씨가 칸 안, 기본 크기 위젯(960×640)·다이어리(1160×820)에서 기록 3개가 모두 보임. 단위 테스트: `summary`의 전체·`+N`·완료 접미사·첫 제목 말줄임·16개 초과·폭 0.
- [ ] **Step 2: 실행해 실패 확인** — `cargo test -p timeschedule-native day_cell` → 컴파일 실패 또는 assertion 실패.
- [ ] **Step 3: 구현** — 배치 규칙: 모두 들어가면 전부 표시 → 잘린 제목에 위에서부터 두 번째 줄 → 남으면 `완료 N`. 넘치면 들어가는 줄 수 − 1개를 표시하고 마지막 줄을 `summary`로 채운다(칸 맨 아래, 누르면 날짜 창).
- [ ] **Step 4: 통과 확인** — 같은 명령 PASS.
- [ ] **Step 5: 커밋**

### Task 2: 위젯 틀·기본값·안내 문구

**Files:**
- Modify: `apps/timeschedule-native/src/desktop_widget.rs` (아래쪽 바 제거, 잠금 버튼을 위쪽 줄로, `CONTENT_GAP` 2, 최소 높이 여유 44px 감소)
- Modify: `crates/timeschedule-desktop/src/widget_preferences.rs` (기본값), `crates/timeschedule-desktop/src/preferences.rs` (기본값 테스트 11→13)
- Modify: `apps/timeschedule-native/src/widget_settings.rs` (밀도 프리셋 촘촘 12/11/0/2/0, 기본 13/13/1/4/1, 넉넉 16/15/3/8/3, 안내 문구)
- Modify: `apps/timeschedule-native/src/shell.rs` (다이어리 칸 글자 11→12)

- [ ] **Step 1: 실패하는 테스트** — 위젯 화면에 `배경 NN%` 슬라이더 글씨가 없고 `위치 잠금`/`잠금 해제`가 요일 머리줄보다 위에 있다.
- [ ] **Step 2: 실패 확인** → **Step 3: 구현** → **Step 4: `cargo test --workspace --features timeschedule-desktop/test-support` 전체 통과**
- [ ] **Step 5: 수동 미리보기** — `TIMESCHEDULE_PREVIEW_DIR=.cache/widget-cell-previews cargo test -p timeschedule-native render_calendar_overflow_previews -- --ignored` 로 만든 이미지를 직접 본다.
- [ ] **Step 6: 커밋**

### Task 3: 버전·문서·리뷰·배포

- [ ] 0.17.0으로 버전 갱신, CHANGELOG·README 반영, `cargo fmt --check`.
- [ ] 아스트라(GPT-6 Astra) 독립 리뷰 → 지적은 테스트 먼저 수정 → 막힘 없을 때까지 반복.
- [ ] `scripts/build-native.ps1`로 `artifacts/TimeSchedule.exe` 준비(실행 중이면 교체 보류).
- [ ] 커밋·푸시·PR, 자동 개발 기록(`docs/project-records.md`).
