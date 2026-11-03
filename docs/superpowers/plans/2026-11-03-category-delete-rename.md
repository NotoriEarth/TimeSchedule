# 카테고리 삭제·이름 변경 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [x]`) syntax for tracking. 기존 작업처럼 본 에이전트가 구현하고 최종 변경은 독립 검토한다. 0.12.0 소스·테스트·빌드를 완료했다. 대표 EXE는 사용자 앱 종료 대기이며 전달 상태는 검증 문서를 따른다.

**Goal:** 1번 시안의 수정 화면에서 이름·색상을 변경하고, 사용자가 선택한 분류로 기록을 보존해 옮긴 뒤 카테고리를 삭제한다.

**Architecture:** 기존 SQLite 저장소에서 영향 조회와 원자적 삭제를 제공하고, Iced 카테고리 관리자가 worker를 통해 호출한다. 삭제 확인은 기존 수정 화면 안에서 표시하며 성공 후 공통 refresh로 다이어리·위젯을 갱신한다. 기존 입력 폼과 `EntryEdit` 충돌 검사를 보존한다.

**Tech Stack:** Rust, Iced 0.14, rusqlite/SQLite, 기존 worker. 새 의존성 없음.

**Spec:** [선택된 1번 시안 설계](../specs/2026-11-03-category-delete-rename-design.md)

## Global Constraints

- 연결 기록이 있으면 사용자가 이동할 카테고리를 직접 고른다. 자동 선택하지 않는다.
- 기록 원문·제목·날짜·프로젝트 연결·출처·짧은 표시 이름·원본 ID는 보존한다.
- 기본 일정·기록 보호는 고정 ID로 판단하고 이름·색상 변경은 허용한다. 이동 후보에서 원본 자신과 일정형 기본 카테고리를 제외한다.
- 취소·Esc는 저장하지 않고 기존 수정 초안을 유지한다. 처리 중에는 중복 실행을 막는다.
- 새 주기 조회·상주 프로세스·AI 호출은 추가하지 않는다. 전체 본문을 읽지 않고 카테고리 ID와 연결 정보로 집계·이동한다.
- 새 이름은 기존 빈 이름·중복 이름·최대 32자 검증을 재사용한다. MCP 일정 계약과 개발 기록 전달 ID·영수증은 유지한다.
- 실행 시작 시 브랜치·미커밋 변경·기준 커밋을 다시 확인한다. 현재 관련 브랜치는 `codex/project-record-readability-plan`, PR #14이며 main에 직접 수정하지 않는다.
- 구현 버전은 기능 추가 묶음인 0.12.0으로 준비한다. 선행 문서 작업은 0.11.3을 유지했으며 이번 구현은 사용자 요청 후 별도 개발 변경으로 진행했다.

## Review Focus

1. 확인창의 건수는 같아도 연결된 기록이 교체된 경우: 잘못된 동의로 이동하지 않도록 ID 집합 변화를 거절한다. Task 1의 동시 변경 테스트.
2. 이동 완료 후 카테고리 제거가 실패하는 경우: 기록 분류·카테고리·순서가 모두 원상태여야 한다. Task 1의 SQLite 실패 주입 테스트.
3. 이름을 바꾼 기본 분류, 확인 중 바뀐 대상 이름: ID 보호와 저장 직전 비교를 적용한다. Task 1의 보호·충돌 테스트.
4. 취소한 조회 응답이 늦게 도착하거나 Esc를 연속 입력한 경우: 이전 확인창을 되살리지 않고 수정 초안을 보존한다. Task 2의 메시지 순서 테스트.
5. 다른 창에서 편집하던 기록의 분류가 삭제된 경우: 초안을 덮어쓰거나 삭제된 분류를 복원하지 않는다. Task 2의 실제 worker/편집 회귀.

### Task 1: 기록 보존과 원자적 카테고리 삭제

**Files:**
- Create: `crates/timeschedule-core/src/calendar/category_lifecycle.rs` — 삭제 미리보기·삭제·기존 분류의 충돌 검사 수정. `calendar.rs`의 private 하위 모듈로 연결해 기존 검증을 공유한다.
- Modify: `crates/timeschedule-core/src/calendar.rs` — 공개 자료형·메서드 노출 및 기존 이름/색상 검증 재사용 경계.
- Modify: `crates/timeschedule-core/src/storage.rs` — schema 5 이전, 이전 백업 이름의 버전 하드코딩 제거.
- Test: `crates/timeschedule-core/tests/calendar_integration.rs`, `crates/timeschedule-core/src/storage.rs`의 기존 tests — 보존·충돌·이전 검증.

**Interfaces:**
- Consumes: `Category`, `EntryEdit`, `SCHEDULE_CATEGORY`, `RECORD_CATEGORY`; 기존 `Storage::save_category(&self, id: Option<&str>, name: &str, color: &str) -> Result<Category, String>`는 기존 호출자를 위해 유지한다.
- Produces: `CategoryDeletePreview { pub category: Category, pub entry_ids: Vec<String> }` (`Debug, Clone, PartialEq, Eq`). ID는 정렬·중복 제거된 현재 연결 목록이며 표시 건수는 `entry_ids.len()`이다.
- Produces: `Storage::preview_category_deletion(&self, id: &str) -> Result<CategoryDeletePreview, String>`.
- Produces: `Storage::delete_category(&self, expected: &CategoryDeletePreview, target: Option<&Category>) -> Result<usize, String>` — 실제 이동 건수 반환. 빈 분류는 `None`, 비어 있지 않으면 유효한 `Some` 필수.
- Produces: `Storage::update_category_checked(&self, expected: &Category, name: &str, color: &str) -> Result<Category, String>` — 같은 ID를 수정하되 최신 Category와 다르면 거절한다.

- [x] **실패 테스트 작성:** `category_delete_moves_all_records_and_preserves_content`는 서로 다른 날짜·프로젝트의 기록 3개와 별칭을 만든다. 반환값 `3`, 원본 category 없음, 대상 category ID로만 변경, 각 `Entry`/별칭/프로젝트·영수증 내용 동일, 남은 position `0..len`을 검증한다. `category_delete_empty_and_protected`는 빈 사용자 분류만 삭제 가능하고 이름을 바꾼 두 기본 ID는 거절함을 확인한다.
- [x] **실패 확인:** `. scripts/toolchain.ps1; cargo test -p timeschedule-core --test calendar_integration category_delete --locked --offline`. 새 API 부재로 실패하는지 확인한다.
- [x] **저장 구현:** 위 API를 `category_lifecycle.rs`에 구현한다. 미리보기의 Category와 연결 ID는 일관된 읽기 트랜잭션에서 읽는다. 삭제는 Immediate 트랜잭션 안에서 source/target Category와 연결 ID를 비교한 뒤 `entry_display.category_id`만 갱신하고 category 삭제·순번 정리를 완료한다. 본문·별칭·원문 updated_at·전달 영수증은 쓰지 않는다. 이름 수정은 기존 검증을 재사용하며 사라진 ID를 UPSERT로 되살리지 않는다.
- [x] **동시 변경·롤백 테스트:** `category_delete_rejects_stale_membership_and_targets`는 같은 건수의 다른 ID, 새 기록 추가, source/target 이름 변경·삭제, 자기 자신·일정형 대상, 대상 미선택을 모두 거절하고 무변경을 확인한다. `category_delete_rolls_back_after_move_failure`는 DELETE를 실패시키는 SQLite trigger로 연결 변경까지 전부 롤백되는지 검증한다. `category_update_checked_rejects_stale`는 외부 수정·삭제·빈 이름·중복·33자 거절, 동일 ID/순서 보존을 검증한다.
- [x] **이전 구현·테스트:** schema 5에서 `CREATE INDEX entry_display_category ON entry_display(category_id, entry_id)`를 추가한다. v4 파일의 이전 백업을 만든 뒤 트랜잭션으로 이전하며 기존 데이터·기본 ID를 보존한다. `category_schema_v5_preserves_v4_data`와 기존 future-schema 거절 회귀를 실행한다. 연결 ID 쿼리의 `EXPLAIN QUERY PLAN`에서 새 인덱스 사용을 확인한다. 미리보기는 해당 분류 ID만 일시 보관하므로 메모리는 연결 건수에 비례하며 확인 종료 때 해제한다.
- [x] **통과 확인·커밋:** `. scripts/toolchain.ps1; cargo test -p timeschedule-core --locked --offline` 전체 통과 후 위 core 파일만 커밋한다. 커밋 메시지: `feat: preserve records when deleting categories`.

### Task 2: 선택된 수정 화면·확인 흐름·공통 갱신

**Files:**
- Modify: `apps/timeschedule-native/src/category_ui.rs` — 명시적 수정 버튼, 이름 변경, 삭제 확인 상태/worker/피드백.
- Modify: `apps/timeschedule-native/src/app.rs` — 카테고리 확인창의 Esc 우선순위와 공통 refresh 연결.
- Test: `apps/timeschedule-native/src/app/tests/visual.rs` — 실제 Iced press/release부터 SQLite와 두 화면까지 회귀. `editor.rs`의 기존 분류 검증·EntryEdit 비교는 유지하며 초안 보존에 필요한 변경만 한다.
- Modify: `apps/timeschedule-native/Cargo.toml`, `Cargo.lock`, `CHANGELOG.md`, `README.md`, `docs/calendar-categories-verification.md` — 실제 구현 후 0.12.0 및 검증 범위.

**Interfaces:**
- Consumes: Task 1의 세 Storage 메서드, `CategoryDeletePreview`, `Category`, 기존 `worker::job`, `App::refresh`와 `EntryEdit` 저장 시 비교.
- Produces: category state에 `editing_version: Option<Category>`, `delete_preview: Option<CategoryDeletePreview>`, `delete_target: Option<Category>`, `delete_generation: u64`, `delete_loading: bool`, `notice: String`을 추가한다. 기존 이름·색상 draft는 별도로 유지한다.
- Produces: `Action::RequestDelete`, `DeletePreviewed { generation: u64, result: Result<CategoryDeletePreview, String> }`, `DeleteTarget(String)`, `CancelDelete`, `ConfirmDelete`, `Deleted(Result<usize, String>)`. 기존 Close는 확인 단계에서 취소로 처리하고 다음 Esc에서 관리창을 닫는다.

- [x] **실제 화면 실패 테스트:** 기존 `click_category_control`/`run_message`를 사용해 `category_delete_controls_move_records_and_refresh_both_surfaces`를 추가한다. 수정→삭제→대상 미선택 확정 불가→직접 선택→확정 후 source 없음, 기록 3개 보존, 위젯·다이어리의 동일 ID/분류 표시를 검증한다. `category_rename_control_keeps_id`는 명시적 수정→변경 저장으로 기존 ID·순서를 유지하고 두 화면 이름을 갱신함을 검증한다.
- [x] **실패 확인:** `. scripts/toolchain.ps1; cargo test -p timeschedule-native category_delete --locked --offline`. 버튼/액션이 없어 실패하는지 확인한다. 저장소 직접 호출로 화면 테스트를 대체하지 않는다.
- [x] **수정·확인 화면 구현:** 행의 `수정`, 입력 `이름`, `변경 저장`, 하단의 분리된 `카테고리 삭제`를 배치한다. 새 입력은 `카테고리 추가`로 구분한다. 확인창은 `‘{이름}’ 카테고리를 삭제할까요?`, 전체 건수, `기록을 옮길 카테고리` 선택과 `이동 후 삭제`/`취소`를 표시한다. 빈 분류는 이동 선택 없이 `카테고리 삭제`, 기본 분류는 `기본 카테고리는 삭제할 수 없습니다`를 표시한다. 목록 행에는 삭제 버튼을 넣지 않는다.
- [x] **worker·상태 연결:** 미리보기/최종 저장만 worker로 실행한다. 미선택·조회 중·저장 중 확정을 비활성화하고 확인 중 배경 수정·드래그를 막는다. 기존 saving guard는 `Deleted` 완료도 허용한다. 취소/닫기 시 generation을 증가시키고 늦은 조회 결과를 무시하며 관리창을 다시 열어도 generation을 초기화하지 않는다. 오류는 draft를 유지하며 최신 미리보기와 후보를 재조회하고 선택을 비워 재확인한다. 성공 시 `기록 {N}개를 ‘{대상}’으로 옮기고 카테고리를 삭제했습니다` 또는 빈 분류 삭제 안내를 보이고 공통 refresh를 한 번 요청한다.
- [x] **순서·충돌 회귀:** `category_delete_cancel_preserves_draft_and_ignores_late_preview`에서 취소→늦은 응답→다른 분류 수정, Esc 두 단계와 무저장을 검증한다. `category_delete_duplicate_confirm_writes_once`에서 단일 worker 저장을 검증한다. `category_delete_preserves_open_record_draft`에서 다른 창의 편집 초안은 그대로이며 저장 시 최신 분류 선택/충돌 안내가 나오고 기록이 복제되지 않음을 확인한다. 기존 추가·이름 검증·드래그·공통 편집 회귀도 유지한다.
- [x] **통과·가독성 확인:** `. scripts/toolchain.ps1; cargo test --workspace --locked --offline` 전체 통과. 기존 native 렌더 경로로 800×620/1160×820, 두 테마, 32자 이름·긴 대상 이름·빈 분류/기록 있는 분류/기본 분류 화면을 생성해 버튼·선택·오류가 잘리지 않는지 확인한다. 실제 사용자 마우스 체감과 자동 검증 결과를 구분해 기록한다.
- [ ] **빌드·리뷰·전달:** 버전 파일·검증 문서를 갱신하고 `scripts/build-native.ps1 -Offline`로 대표 EXE를 준비한다. 최종 변경을 독립 검토하고 지적을 보완한다. 사용자 앱이 실행 중이면 강제 종료하지 않고 교체 대기를 알린다. 정상 교체 후 파일 버전/해시를 검증한다. 시작·확인창·종료 후 idle 자원을 같은 조건으로 비교하고 새 폴링/지속 redraw가 없음을 확인하되 측정하지 않은 값을 주장하지 않는다.
- [ ] **커밋·PR·기록:** UI/검증/버전 변경을 `feat: add category deletion and rename controls`로 커밋하고 관련 브랜치를 push, PR을 실제 완료 결과로 갱신한다. 소스 변경·커밋이 생긴 이 구현 작업에 한해서 `docs/project-records.md`의 기준·완료 커밋 전달 절차를 적용한다. 삭제 기능 작업의 기존 state가 있으면 같은 workId를 유지하고 새 작업이면 새 UUID를 사용한다. 앞선 추가 버튼 수정 기록을 임의로 재사용하지 않는다. 테스트 방법·실행 파일·PR을 전달하고 사용자의 새 테스트 승인 전 머지하지 않는다.

## 실행 결과와 남은 전달

- 최종 전체 197개 통과, 8개 기본 제외. 독립 검토 P2 1건은 실제 창 간 경합 회귀 2개의 실패→통과로 보완했다.
- 새 실행 파일 빌드 완료. 사용자 0.11.3 앱 실행 중이므로 대표 EXE 교체만 대기한다. 머지하지 않았다.
- 확인창/취소 후 자원 측정은 Windows 캡처 두 차례 시간 초과로 미검증이다. 시작 후 짧은 측정만 수행했으며 동등 조건의 성능 비교로 해석하지 않는다.
