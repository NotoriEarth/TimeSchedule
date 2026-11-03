# 2단계 · 프로젝트 기록 가독성과 공통 수정 구현 계획

> **For agentic workers:** Use `superpowers:executing-plans` in the current session after the user approves this plan; request one independent whole-change review. Steps use checkbox syntax. Do not begin product implementation from this document without the user's go-ahead.

**Goal:** 기본 두 줄 기록을 다이어리·위젯에 표시하되 미완료 일정 → 할 일 → 기록의 공간 우선순위를 적용하고, 상세·수정을 같은 원본으로 연결한다.

**Architecture:** 기존 `diary_entries`와 프로젝트 ID를 유지한다. 표시용 메타데이터와 본문 없는 날짜별 조회를 공통 코어에 추가하고, 네이티브 앱·위젯·빠른 창은 공통 표시/편집 경로를 사용한다.

**Tech Stack:** Rust, Iced, rusqlite/SQLite, 현재 텍스트 측정기와 Windows 위젯. 신규 웹뷰·서버·AI 서비스 없음.

**Spec:** `docs/superpowers/specs/2026-11-03-project-record-readability-design.md`

## Global Constraints

- 이 문서는 미구현 계획이다. 최신 선택은 1안 기본 두 줄이며, 2안 프로젝트명 토글은 spec에 참고안으로만 보관한다. 사용자가 구현 전에 요청한 항목 분류/우선순위 정리를 먼저 마친다.
- 기준 커밋 `1a8342a6b36c3b6c3f81cc4b6df2ffcf8a841f3b`. 계획 전용 브랜치 `codex/project-record-readability-plan`; 제품 작업 시작 시 당시 변경/브랜치를 다시 확인한다.
- 원래 제목/본문/ID 보존. 표시명만 바꿀 때 자동 수신 충돌을 만들지 않는다. 원본 편집의 기존 충돌 보호는 유지한다.
- 기본 두 줄 제안 12px/16px + 14px/19px, 간격 2px, 위아래 여백 각 4px. 실제 글꼴 높이·사용자 배율을 반영하고 공간을 이유로 글씨를 축소하지 않는다.
- 미완료 일정 → 해당 날짜 미완료 할 일 → 두 줄 기록 순으로 공간을 배정한다. 기록에 최소 한 칸을 강제하지 않는다. 두 줄이 안 들어가면 개수 진입점만 유지한다. 주 수·셀 높이 강제 변경과 프로젝트명 토글 구현은 제외한다.
- 완료 일정·할 일은 사용자가 선택한 `완료 N개`로 접고 날짜 상세에서 구분해 읽는다. 날짜 없는 할 일과 독립 메모, 프로젝트 자체는 날짜 칸에 자동 배치하지 않는다.
- 달력은 날짜당 최대 4개 머리말과 정확한 전체 개수, 날짜 상세는 50개 단위. 본문은 열 때만 조회한다.
- 위젯/앱 날짜 범위는 독립. 기존 일정 동작, 미저장 초안, 창 배치, 사용자 테마/폰트를 보존한다.
- 기록 승인·여러 저장소 감시·Space 원격 수정·개인 서버는 제외한다. 계획 작성만으로 개발 기록 전송을 실행하지 않는다.

## Review Focus

1. 월/프로젝트 목록의 200개 페이지 제한이 날짜별 개수·위젯 기록 누락을 만들지 않는지 → Task 1, 3.
2. 별칭 편집과 원본 편집을 구분하여 자동 수신이 사용자 수정을 덮어쓰거나 무관한 별칭 때문에 멈추지 않는지 → Task 1, 4.
3. 글꼴 변경·이모지 결합문자·150% 배율·작은 창에서 두 줄/더 보기가 겹치지 않으며 일정·할 일의 표시 공간을 기록이 빼앗지 않는지 → Task 2, 3, 5.
4. 위젯 기록 클릭이 기존 0~1개 일정 자동 편집/더블클릭 일정 생성으로 잘못 이어지지 않는지 → Task 3, 4.
5. 날짜·프로젝트 변경 중 늦은 비동기 응답과 이미 열린 본문 캐시가 이전 기록을 다시 표시하지 않는지 → Task 4.

## 변경할 파일과 책임

| 파일 | 책임 |
|---|---|
| `crates/timeschedule-core/src/record_calendar.rs` (신규) | 표시명 CRUD, 날짜별 머리말/개수, 원본+표시명 저장 트랜잭션 |
| `crates/timeschedule-core/src/lib.rs`, `storage.rs`, `diary.rs` | 모듈 공개·스키마/백업·기존 저장 검증 재사용 |
| `crates/timeschedule-core/src/project_records.rs` | 기존 자동 수신과 표시명 보존/충돌 회귀 테스트 |
| `apps/timeschedule-native/src/record_view.rs` (신규) | 두 줄 항목·개수 초과·공통 읽기 상세·기록 선택 데이터 |
| `apps/timeschedule-native/src/text_layout.rs` | 글꼴 실측·grapheme 단위 생략 |
| `apps/timeschedule-native/src/worker.rs`, `app.rs` | 독립 날짜 범위 조회·할 일 날짜별 투영·선택/응답 세대·갱신·편집 상태 |
| `apps/timeschedule-native/src/shell.rs`, `desktop_widget.rs`, `views.rs` | 각 화면에서 공통 기록 표시 재사용 |
| `apps/timeschedule-native/src/quick_editor.rs`, `editor.rs` | 기록 읽기/수정 진입·공통 Form·충돌/저장/취소 |
| `apps/timeschedule-native/src/app/tests/visual.rs`, `app/tests/scroll_latency.rs` | 실제 Iced 레이아웃·입력·스크롤 회귀 |

## Task 1 · 표시 메타데이터와 정확한 날짜별 조회

**Interfaces** (`record_calendar.rs`, 제안 이름):

- `RecordHeader { id, project_id, project_title, project_short_name, date, kind, title, short_title, updated_at }`: 본문 없음. 각 이름은 String, optional 관계/별칭은 Option<String>.
- `RecordDay { date: String, total: u32, items: Vec<RecordHeader> }`.
- `RecordPage { items: Vec<RecordHeader>, has_more: bool }`.
- `EntryEditSnapshot { entry: Entry, short_title: Option<String> }`: 편집 시 읽은 원본 비교 기준.
- `Storage::record_days(from: &str, to: &str, project_id: Option<&str>) -> Result<Vec<RecordDay>, String>`: 날짜별 최대 4개.
- `Storage::record_day_page(date: &str, project_id: Option<&str>, offset: u32) -> Result<RecordPage, String>`: 50개+다음 페이지 여부. 날짜 목록을 읽는 도중 저장/수신/삭제로 순서가 바뀌면 view generation을 올리고 첫 페이지부터 새로 읽는다. 이전 offset 결과를 새 목록에 이어 붙이지 않는다.
- `Storage::get_entry_edit(id: &str) -> Result<EntryEditSnapshot, String>`.
- `Storage::save_entry_edit(input: EntryInput, short_title: Option<String>, expected: Option<&EntryEditSnapshot>) -> Result<EntryEditSnapshot, String>`: 비교와 본문/별칭 저장을 동일 트랜잭션에서 수행. 새 기록 외에는 expected 필수.
- `Storage::save_project_display(id: &str, short_name: Option<String>, expected: Option<String>) -> Result<(), String>`: expected는 읽었을 때의 별칭 값, 현재 값과 다르면 충돌. 기존 프로젝트 저장과 별칭 변경도 네이티브 편집에서는 한 트랜잭션으로 묶을 공통 helper를 둔다.

- [ ] 스키마 3 fixture에 프로젝트·기록·검색·수신 영수증을 넣은 뒤 migration 보존/멱등/실패 롤백 및 일관된 백업 테스트를 먼저 작성한다.
- [ ] `record_calendar` 테스트에서 날짜 한 곳의 205개, 다른 날짜, 미지정 프로젝트, 삭제된 프로젝트를 확인한다. total=205, preview=4, 전체 페이지 합=205이며 중복 ID가 없고 모든 머리말에 본문이 없도록 검증한다.
- [ ] 원본+별칭 저장 중 실패 시 부분 반영 없음, 두 저장소 연결의 오래된 초안 거절, 별칭만 변경 후 자동 수신 성공·별칭 유지, 본문 변경 후 자동 수신 conflict를 검증한다.
- [ ] 50개 조회 후 기록 수신으로 정렬이 바뀌는 fixture에서 이전 페이지 응답을 폐기하고 처음부터 다시 읽어, 최종 205개 ID의 누락·중복이 없는지 확인한다.
- [ ] 실패 확인 후 스키마 4의 두 메타데이터 테이블과 범위당 집계/상위 항목 쿼리를 구현한다. 기존 `save_entry`, 프로젝트 입력, Tauri 호출은 별칭을 초기화하지 않게 유지한다. 일별 쿼리 반복과 본문 전체 조회를 피한다.
- [ ] `. scripts/toolchain.ps1; cargo test -p timeschedule-core --locked --offline`로 확인하고 Task 1을 커밋한다.

## Task 2 · 가독성 중심 공통 두 줄 항목

**Interfaces** (`record_view.rs`):

- `RecordText { project: String, title: String }`와 `record_text(&RecordHeader) -> RecordText`: 별칭 우선, 원문 fallback, 미지정 프로젝트 문구.
- `record_chip(header: &RecordHeader, metrics: RecordMetrics, on_open: Message) -> Element<Message>`: 전체 행 하나가 클릭/키보드 대상.
- `RecordMetrics { font, project_size, title_size, project_line_height, title_line_height, padding }`: height는 고정 문자열 길이가 아닌 글꼴/배율에서 계산.
- `fit_record_labels(text: &RecordText, width: f32, metrics: RecordMetrics) -> RecordText`: 원본은 바꾸지 않고 grapheme 경계에서만 생략.
- 날짜별 배치 helper는 일정/할 일/기록의 실측 높이·미완료/완료 개수·사용 높이를 받아 온전히 들어가는 항목과 종류별 숨긴 개수를 반환한다. 날짜/필요한 개수 행 확보 → 미완료 일정 → 미완료 할 일 → 기록 순으로 채운다. 앞선 종류가 넘치면 뒤의 기록을 끼워 넣지 않는다. 완료 항목은 개별 행 대신 요약으로만 노출한다.
- `DayOverflow { events, tasks, records, completed }`는 각각 숨긴 미완료 일정 수, 숨긴 미완료 할 일 수, 숨긴 기록 수, 완료 일정+할 일 수다. 중복 집계하지 않는다. 폭이 부족하면 합계 `더 보기 N개` 진입점 하나로 전환하고 상세에서 네 구역을 구분한다.

- [ ] 별칭/원제목 fallback, 동일 프로젝트명인데 ID가 다른 경우, 공백·개행 정리, 0px/좁은 폭·긴 한글·ZWJ 이모지 생략 테스트를 작성하고 실패를 확인한다.
- [ ] 기존 텍스트 측정기를 재사용한다. grapheme 처리는 lockfile에 이미 있는 `unicode-segmentation` 재사용 가능성을 확인하고 필요한 직접 의존성만 선언한다. 문자 수로 픽셀 폭을 추정하지 않는다.
- [ ] 두 줄 기본안과 읽기 전용 상세의 Iced fixture를 만들고 실제 렌더링 이미지로 글꼴·대비·여백을 검토한다. 원문 전체를 상세에서 읽을 수 있게 한다.
- [ ] 일정만 많은 날, 할 일만 많은 날, 세 종류 혼합, 완료만 있는 날의 배치를 확인한다. 미완료 일정/할 일이 남았는데 기록이 대신 표시되거나 완료 수가 숨긴 미완료 수와 중복되면 실패한다.
- [ ] `cargo test -p timeschedule-native record_ --locked --offline` 및 관련 텍스트 테스트로 확인하고 Task 2를 커밋한다.

## Task 3 · 다이어리·위젯의 날짜별 표시와 진입점

**Interfaces:** Task 1 `RecordDay`를 `Snapshot`의 `diary_records`, `widget_records`로 각각 보관한다. Task 2의 동일 칩/배치 함수를 양쪽에서 사용한다. `Message::OpenRecord { id, origin }`, `Message::OpenRecordDay { date, origin }` 및 `RecordOrigin::{Manager, Quick}`를 도입한다.

기존 `Snapshot.tasks`와 `Task` 원본을 재사용하며 앱/위젯의 날짜별 할 일 조회를 분리해 투영한다. 날짜 미지정/다른 날짜는 제외하고 `completed`로 미완료/완료를 나눈다. `Message::OpenDaySection { date, section, origin }`, `DaySection::{Events, Tasks, Records, Completed, All}`로 더 보기·할 일·완료의 빠른 창 진입 의도를 구분한다. 기존 일정 0~1개 자동 편집은 이 진입 경로에 적용하지 않는다.

- [ ] 앱과 위젯이 서로 다른 월/rolling 범위를 보는 fixture에서 각각 맞는 기록만 보이고, 앱 필터가 위젯에 영향을 주지 않는 테스트를 작성한다.
- [ ] 실제 높이별 혼합 일정/기록 배치, `+ 기록 N개 더`의 정확한 개수, 0개 기록, 205개 기록 접근을 테스트한다. 글자가 칸 경계를 넘거나 행 일부만 보이면 실패해야 한다.
- [ ] 기본 글꼴의 45px 두 줄 행과 실제 확대 글꼴 높이 경계에서 두 줄→집계 전환을 검증한다. 일정/할 일 추가·완료·삭제로 배정 항목은 바뀌어도 모든 날짜 rect와 스크롤 오프셋은 같아야 한다.
- [ ] 현재 위젯에 없는 할 일 표시를 추가한다. 기존 ID와 완료 저장을 재사용하고 체크/수정 후 앱·위젯을 함께 갱신한다. 날짜 미지정/다른 날짜의 할 일을 오늘에 자동 배치하지 않으며, 완료 개수 클릭은 날짜 상세 완료 구역으로 연결한다.
- [ ] `worker.rs`에서 두 범위를 조회하고 `shell.rs`, `desktop_widget.rs`에서 두 줄로 표시한다. 기존 날짜 행·일정 시간·완료 표시·이동/크기 조절/잠금 동작은 유지한다.
- [ ] 기록 칩/할 일 체크·편집/완료·더 보기 클릭은 날짜 더블클릭과 분리한다. 일정 0~1개일 때도 이 진입은 의도한 상세/목록으로 가고 일정 입력창을 만들지 않는 실제 입력 테스트를 추가한다.
- [ ] 관련 native/visual 테스트를 통과시키고 Task 3을 커밋한다.

## Task 4 · 상세와 양방향 수정 연결

**Interfaces:** Task 1의 `EntryEditSnapshot`을 Form/Editor에 보관하고 `save_entry_edit`를 호출한다. 기존 전역 편집기 하나와 종료 확인을 재사용한다. 상세 응답은 기록 ID+view/quick generation을 함께 확인한다.

- [ ] 앱에서 원제목·날짜·프로젝트·본문·별칭을 저장하면 위젯/열린 상세가 같은 ID로 갱신되고, 위젯에서 저장한 변경도 앱에서 보이는 왕복 테스트를 작성한다. 날짜 이동은 옛 날짜에서 제거되는지 확인한다.
- [ ] 저장 실패·자동 수신 충돌·열린 초안 중 다른 기록 선택·취소·창 닫기·앱 종료를 테스트한다. 늦게 도착한 원문/날짜 페이지 응답이 현재 선택을 덮어쓰면 실패한다.
- [ ] `views.rs`와 빠른 창에서 공통 상세를 재사용하고, 수정에서만 같은 Form을 연다. 프로젝트 편집에는 선택적인 표시 이름, 기록 편집에는 선택적인 달력용 작업명과 미리보기를 붙인다.
- [ ] 저장 성공 및 기존 기록 수신 완료 경로에서 snapshot과 필요한 상세 캐시를 무효화하고 열린 창을 다시 읽는다. 별도 타이머/폴더 감시/Space 전송을 만들지 않는다.
- [ ] 관련 core/native 테스트를 통과시키고 Task 4를 커밋한다.

## Task 5 · 실제 화면 검증과 전달

- [ ] 두 테마, 기본/다른 설치 글꼴, 100/150/200% 배율, 메인 800×620/1160×820, 위젯 350×330/640×480/960×640와 2/4/6주 표시에서 검사한다. 크기 이름이 아닌 실제 셀 높이와 일정·할 일 우선 배정 후 남은 공간으로 두 줄/집계 기준을 판단한다. 모니터 작업 영역 내 빠른 창 배치도 확인한다.
- [ ] 한 날짜의 0/1/3/205개 기록, 긴 제목·동일 이름 프로젝트·기존 Space 연결/대기 기록을 합성 fixture로 검증한다. 개인 원문은 시안/로그/PR에 사용하지 않는다.
- [ ] 저장/수신/로딩 상태가 바뀌어도 달력 칸 크기와 스크롤 위치가 유지되는지, 클릭 영역이 텍스트와 일치하는지, 기존 스크롤 픽셀 회귀가 없는지 검증한다.
- [ ] `cargo test --workspace --locked --offline`, 기존 Node 회귀, fmt/diff 검사, release 빌드 후 동일 fixture에서 화면 로딩/스크롤·유휴 자원을 비교한다. 실패나 새 우려가 없으면 반복 측정하지 않는다.
- [ ] 사용자 시안 확인과 독립 코드 검토를 거친다. 큰 화면만 보고 통과시키지 않는다. 구현 완료 예상 버전은 0.11.0이며 실제 구현 시 Cargo.toml/Cargo.lock/README/CHANGELOG를 함께 갱신한다.
- [ ] `scripts/build-native.ps1`로 대표 EXE를 준비한다. 실행 중 앱은 종료시키지 않으며 필요한 경우 저장·완전 종료를 요청한다. 소스 커밋·푸시·PR·채팅 연결 및 실제 개발 변경에 한해 자동 개발 기록을 수행한다.
- [ ] 실제 EXE 확인 방법과 미확인 환경을 전달하고 사용자 테스트 승인 전 머지는 보류한다.

## 이번 계획 작업의 전달

계획 문서와 가상 자료로 만든 비교 화면만 제공한다. 제품 코드, 현재 0.10.1 EXE, 사용자 일정·기록·MCP 설정은 수정하지 않는다. 문서만 작성하므로 버전을 올리거나 자동 개발 기록의 `prepare`/`sync`를 실행하지 않는다.
