# Rust Native Experiment Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox syntax. 현재 요청은 계획 작성까지이며 구현 체크박스는 미완료 상태로 둔다.

**Goal:** 현재 다이어리와 동등한 기능의 Rust 네이티브 UI가 실제 메모리·응답성·유지보수 측면에서 유리한지 검증한다.

**Architecture:** 기존 Rust 저장 코어를 재사용하고 Tauri 결합을 어댑터로 격리한다. Iced 시제품으로 한글 입력과 Windows 바탕화면 제약을 먼저 검증한 뒤, 통과하면 별도 Rust GUI 바이너리를 만든다. 현재 Svelte/Tauri 실행 경로는 비교 종료까지 보존한다.

**Tech Stack:** Rust, rusqlite/SQLite, chrono, serde, url, Windows API. 우선 GUI 후보 Iced 0.14, 실패 시 egui/eframe의 동일 최소 시제품 비교. 선택한 버전/렌더러/feature는 lockfile과 보고서에 고정한다.

**Spec:** `docs/superpowers/specs/2026-10-29-rust-native-experiment.md`

## Global Constraints

- 작업 브랜치 `codex/rust-native-experiment`, 기준 커밋 `a8f47a1fcbf2bcc6dcd499ba1c545f897d5c0ee5`.
- 사용자 DB 자동 접근/마이그레이션 금지. 별도 데이터 경로·별도 EXE로 비교한다.
- 기존 기능·schema v2·식별자·기록 원문을 유지한다. 모바일/동기화/AI/블록 편집기 개발을 섞지 않는다.
- 한글 입력과 바탕화면 호환성 검증 전 전체 GUI 이식에 들어가지 않는다.
- 각 단계는 테스트/실행 결과와 함께 커밋한다. 실패 시 기준판을 제거하지 않는다.
- 실험 브랜치의 머지는 별도의 사용자 테스트 승인 후에만 한다.

## Review Focus

- 한글 조합 중 검색 Enter·저장·Esc·창 닫기가 조합 문자열이나 작성 내용을 유실하는가: Task 2, 5.
- WorkerW 자식 창·레이어드 투명도와 렌더러가 충돌하거나 다른 Explorer 창의 스타일을 건드리는가: Task 2, 4, 6.
- WAL이 있는 DB의 복사, 새/오래된 schema, 업데이트 실패에서 기존 기록을 보존하는가: Task 1, 3.
- 프로젝트/주간 변경 뒤 도착한 비동기 결과가 잘못된 화면에 들어오거나 작성 중 창을 덮어쓰는가: Task 5.
- 상세 창 닫기·트레이 재열기 10회 후 창/스레드/GPU 리소스가 누적되는가: Task 6, 7.

## 파일 구조

| 경로 | 책임 |
| --- | --- |
| `Cargo.toml`, `Cargo.lock` | 실험용 공통 코어/플랫폼/네이티브 앱 workspace. 기존 src-tauri는 독립 manifest 유지 |
| `crates/timeschedule-core/src/{lib,storage,diary,dates,evidence}.rs` | 기존 저장 로직 이동, 순수 날짜·초안 로직 |
| `crates/timeschedule-core/tests/{compatibility,dates,evidence}.rs` | schema/기존 데이터/날짜/근거 계약 |
| `crates/timeschedule-desktop/src/{lib,windows,preferences,paths,tray}.rs` | Tauri와 독립된 플랫폼 동작·설정 |
| `crates/timeschedule-desktop/tests/desktop_native.rs` | 기존 Windows 통합 테스트 이관 |
| `apps/timeschedule-native/src/{main,app,worker,editor,dialogs,theme}.rs` | 네이티브 앱 수명·작업 큐·편집 상태·팝업 |
| `apps/timeschedule-native/src/views/{mod,month,week,projects,search,widget,settings}.rs` | 기능별 Rust 화면 |
| `apps/timeschedule-native/tests/{navigation,editor,worker,lifecycle}.rs` | GUI와 독립적인 상태 전이 테스트 |
| `experiments/rust-ui-spike/{Cargo.toml,src/main.rs,src/windows_probe.rs}` | 첫 GUI 시제품. production 데이터 사용 안 함 |
| `scripts/{measure-memory,benchmark-ui,build-native}.ps1` | 비교 측정과 별도 바이너리 빌드 |
| `docs/rust-native/{baseline,gui-spike,benchmark,decision}.md` | 근거·실패·선택·최종 판단 기록 |

Task 3에서 `src-tauri/Cargo.toml`에 path dependency와 독립 workspace 설정을 명시해 루트 workspace에 묵시적으로 편입되지 않게 한다. 두 경로의 Cargo.lock을 함께 관리하고 기존 빌드 명령을 계속 통과시킨다.

### Task 1: 동일 조건의 기준 측정 만들기

**Files:** 수정 `scripts/measure-memory.ps1`; 추가 `scripts/benchmark-ui.ps1`, `scripts/tests/measure-memory.tests.ps1`, `scripts/tests/benchmark-fixtures.tests.ps1`, `docs/rust-native/baseline.md`, `src-tauri/examples/seed_benchmark.rs`. 생성 도구는 기존 storage/diary 모듈을 path로 참조하고 Task 3에서 `crates/timeschedule-core/examples/seed_benchmark.rs`로 옮긴다.

**Interfaces:** `measure-memory.ps1 -AppProcessId <id> -ExpectedExecutablePath <absolute-path>` → 기존 메모리 JSON + 프로세스 경로/시작 시간/CPU 누적 시간/누락 표본 표시. 현재 스크립트는 프로세스 이름을 TimeSchedule로 고정하므로 새 EXE 이름도 안전하게 검증하도록 바꾼다.

- [ ] 경로가 다른 PID를 지정하면 거부하고, 종료된 자식/누락 카운터를 0으로 오인하지 않는 테스트를 먼저 만든다. 임시 프로세스 트리만 사용한다.
- [ ] 기준 EXE를 `artifacts/baseline/TimeSchedule.exe`로 보존하고 커밋·SHA256·OS·배율·화면 크기를 기록한다. 원본 실행 파일은 변경하지 않는다.
- [ ] 고정 seed로 소형(일정 1,000/기록 2,000/프로젝트 10)과 대형(일정 10,000/기록 20,000/프로젝트 50) fixture를 만든다. 한글·링크·긴 본문(100,000자)을 포함하고 데이터셋 해시를 기록한다.
- [ ] fixture 테스트는 레코드 수·최대 본문·한글 검색 결과를 검증한다. ID/날짜/본문의 정렬된 논리 데이터 해시를 고정하며 생성 시각·SQLite 파일 배치 차이를 결정성 실패로 오인하지 않는다. 테스트 스크립트는 assert 실패 시 비정상 종료하고 추가 테스트 프레임워크 설치 없이 실행한다.
- [ ] 상세+위젯, 위젯만, 긴 프로젝트 노트, 검색, 10회 닫기/재열기 시나리오를 정의한다. 매 실행 60초 안정화 후 1초 간격 120개 표본, 후보별 5회 교차 순서로 측정한다. 누락 표본은 제외 개수와 이유를 남긴다.
- [ ] 현재 스크립트의 공유 working set 중복·private commit 구분을 유지한다. 시작 시간은 프로세스 시작과 첫 사용 가능한 화면 신호로 기록하고 수동 측정이면 그 한계를 명시한다.
- [ ] baseline 보고서에 실제 값과 측정 명령을 채우고 커밋한다. 과거 README 수치를 복사해 이번 기준값으로 사용하지 않는다.

### Task 2: 한글 편집·주간·바탕화면 시제품과 후보 결정

**Files:** 추가 `experiments/rust-ui-spike/Cargo.toml`, `src/main.rs`, `src/windows_probe.rs`, `docs/rust-native/gui-spike.md`.

**Interfaces:** 저장소 없는 시제품. fixture 문자열과 7일 기록만 받아 두 창(상세/위젯)을 표시한다. `--probe=editor|desktop`를 지원하고 사용자 데이터 경로를 사용하지 않는다.

- [ ] Iced의 고정 릴리스로 4열×2행 주간, 날짜별 100개 기록의 독립 스크롤, 멀티라인 입력, 중앙 미저장 팝업을 만든다. 이 단계는 승인된 실험 목적의 시제품이며 대규모 production 구조를 먼저 만들지 않는다.
- [ ] Windows 한글 IME에서 “만년 다이어리 일정 기록” 조합, 중간 삽입·삭제, 한/영 전환, 선택/붙여넣기/실행 취소·다시 실행, 조합 중 Enter/Esc를 실제 확인한다. 클립보드 붙여넣기만으로 IME 통과라고 하지 않는다.
- [ ] 800×620/일반 크기, 100%/200% 배율에서 주간 전부 표시·날짜별 스크롤·팝업 포커스와 배경 입력 차단을 확인한다. 배율 변경은 사용자 환경을 임의로 바꾸지 않고 별도 테스트 환경 또는 사용자 협조로 확인한다.
- [ ] 소유한 위젯 HWND만 Explorer 바탕화면 호스트에 연결해 40/82/100% 투명도, 클릭, Win+D, 잠금, 창 종료를 확인한다. 기존 desktop.rs의 제약과 Windows 호환성 manifest를 읽고 적용한다. 외부 창 재시작·스타일 변경 금지.
- [ ] Iced가 통과하면 선택한 렌더러/feature와 관찰 근거를 기록한다. 실패하면 원인을 기록하고 egui/eframe으로 같은 최소 시나리오를 한 번 비교한다. 두 후보 모두 실패하면 전면 이식을 멈추고 기준판 유지안을 보고한다.
- [ ] 시제품의 수치는 production 절감률과 구분하고, 결과/선택 근거를 커밋한다. 이후 Task는 선택한 GUI가 통과한 경우에만 진행한다.

### Task 3: 기존 Rust 코어 분리와 데이터 계약 보존

**Files:** 추가 루트 workspace, `crates/timeschedule-core/Cargo.toml`, `src/{lib,storage,diary,dates,evidence}.rs`, 위 테스트; 수정 `src-tauri/{Cargo.toml,Cargo.lock,src/lib.rs,src/commands.rs}` 및 기존 storage/diary 모듈을 재수출 어댑터로 교체.

**Interfaces:** 현재 `Storage::open(path: impl AsRef<Path>) -> Result<Self,String>` 및 CRUD/search/import 서명을 유지한다. path는 디렉터리가 아닌 DB 파일 경로다. `WeekRange { from: NaiveDate, to: NaiveDate }`, `week_after_sunday(NaiveDate) -> Result<WeekRange,String>`, `week_containing(NaiveDate) -> WeekRange`; `Evidence { id:String,title:String,date:NaiveDate,body:String,source_url:String,source_ref:Option<String> }`, `evidence_draft(scope:&str, records:&[Evidence], portfolio:bool, truncated:bool) -> String`.

- [ ] v1→v2 이전, v2 reopen, 미지원 미래 schema 거부, Git 중복/수정 보존 테스트를 독립 코어에서 실패→통과 순서로 이관한다. 원문/ID/날짜가 동일함을 비교한다.
- [ ] 저장/검색/Git 코드를 이동하되 DB 형식은 바꾸지 않는다. `tauri::Url`은 `url::Url`로 대체하고 OS 링크 열기는 Task 4 플랫폼 계층에 둔다. Tauri State/Emitter를 코어에 넣지 않는다.
- [ ] TS 날짜·초안 계약을 Rust 테스트로 고정한다: 2026-12-27 → 2026-12-28~2027-01-03, 2028 윤일, 100개 제한 표시, 출처 ID, 기존 원문 불변. 기존 Node 테스트도 계속 통과시킨다.
- [ ] `cargo test -p timeschedule-core`, `cargo test --manifest-path src-tauri/Cargo.toml`, `npm.cmd test`, `scripts/build.ps1`을 실행한다. 양쪽 앱이 같은 코어를 쓰고 기존 Tauri EXE가 계속 동작해야 통과다.
- [ ] 합성 복사본으로 양방향 reopen을 확인하고 코어 분리만 커밋한다.

### Task 4: Windows 플랫폼 경계와 네이티브 앱 수명

**Files:** 추가 `crates/timeschedule-desktop/{Cargo.toml,build.rs,src/lib.rs,src/windows.rs,src/preferences.rs,src/paths.rs,src/tray.rs,tests/desktop_native.rs}`; 수정 `src-tauri/src/{desktop,lib}.rs`를 어댑터로 축소; 추가 `apps/timeschedule-native/{Cargo.toml,src/main.rs}`.

**Interfaces:** `DesktopWindow`는 소유권 없는 HWND 참조와 UI 스레드 검증을 감싼다. `DesktopWindow::attach() -> Result<AttachmentInfo,String>`, `set_opacity(u8)`, `set_locked(bool)`, `recover_position()`, `detach()`는 기존 의미를 유지한다. 생성은 GUI 어댑터 내부에서만 허용하며 창 파괴 후 사용 불가를 보장한다. `resolve_data_dir(override_path:Option<PathBuf>, experimental:bool) -> Result<PathBuf,String>`; `TrayAction::{OpenDiary,ShowWidget,HideWidget,RecoverWidget,Quit}`.

- [ ] 기존 전용 창 클래스·부모 클래스 불변·투명도·화면 위치 테스트를 새 계층에서 실패→통과로 이관한다. 잘못된 스레드/파괴된 창은 명시적으로 실패해야 한다.
- [ ] 기존 `preferences.json`의 잠금·위치·불투명도 기본값을 보존한다. 실험판 기본 경로는 `com.timeschedule.personal.native-experiment`로 분리한다. 데이터 폴더별 단일 실행 잠금을 추가하고 잠금 실패는 기존 프로세스를 죽이지 않고 안내한다.
- [ ] UI 스레드에서만 HWND 작업을 실행한다. 기존 manifest 요구를 build.rs에 반영하고 연결 실패 시 숨기지 말고 사용자에게 상태를 보여준다.
- [ ] 상세 창을 닫아도 트레이/위젯은 유지하고, 트레이 완전 종료는 미저장 확인을 거쳐 모든 소유 창·worker를 종료하도록 연결한다.
- [ ] Windows 통합 테스트와 기존 Tauri 빌드를 통과시킨 뒤 플랫폼 경계/실행 뼈대를 커밋한다.

### Task 5: 상세 다이어리와 안전한 비동기 편집 이식

**Files:** 추가 `apps/timeschedule-native/src/{app,worker,editor,dialogs,theme}.rs`, `views/{mod,month,week,projects,search,settings}.rs`, `tests/{navigation,editor,worker}.rs`.

**Interfaces:** `RequestId(u64)`, `ViewGeneration(u64)`, `EditorGeneration(u64)`; worker 요청과 응답은 세 ID 및 명시적 프로젝트/날짜 범위를 동반한다. 결과 적용 전에 ID를 비교한다. `EditorState::{Closed,Editing,ConfirmDiscard,Saving}`와 `CloseIntent::{Editor,Window,Application}`를 구분해 팝업 승인이 실행할 동작을 잃지 않게 한다.

- [ ] 상태 전이 테스트를 먼저 작성한다: 월→주→월 보존, 일요일 범위, 날짜 추가, 독립 스크롤 ID 유지, 팝업 Esc는 Editing 복귀, 저장 실패는 원문 유지, Saving 중 닫기 차단, stale 검색/초안 응답 무시.
- [ ] 기존 색/모눈/4열×2행 배치와 월간 이동, 날짜 일정 입력, 기록 펼치기, 프로젝트·메모·할 일·설정을 Rust 화면으로 재현한다. 폰트의 한글 fallback과 배포 가능 여부를 기록한다.
- [ ] 별도 중앙 확인 팝업, 기본 “계속 편집”, 키보드 포커스 순환과 배경 입력 차단, 작성 내용/스크롤 유지, 내부→외부 드래그로 닫히지 않음을 구현한다.
- [ ] DB/Git 작업은 소유 worker의 유한 큐에서 처리한다. 검색 대기 중 요청은 최신으로 합치고 응답은 stale 검사한다. 목록은 200개/검색 50개, 본문은 열 때만 읽고 닫힌 항목의 캐시를 제한한다.
- [ ] 수동 Git 수집과 출처 링크, 동일 근거 초안을 연결한다. 동기화나 AI 연결로 표시하지 않는다. 입력 조합 중 단축키가 저장/취소로 잘못 해석되지 않게 한다.
- [ ] `cargo test -p timeschedule-native`와 실제 Windows 입력/저장/검색 시나리오를 통과시키고 화면·편집 기능을 커밋한다.

### Task 6: 바탕화면 위젯과 재열기 완성

**Files:** 추가 `views/widget.rs`, `tests/lifecycle.rs`, `scripts/build-native.ps1`; 수정 `main.rs`, `app.rs`, `worker.rs`.

**Interfaces:** `UiIntent::AddEvent(NaiveDate)`는 위젯 날짜에서 상세 편집기로 전달한다. `DataChanged`는 저장 완료 후 두 창의 필요한 범위만 갱신한다. 창별 ViewGeneration을 분리한다.

- [ ] 상세 창 재생성 시 날짜 의도가 한 번만 전달되고, 편집 중 날짜 클릭으로 기존 글을 덮어쓰지 않는 테스트를 작성한다. 종료 요청 중 팝업 취소는 창과 worker를 유지해야 한다.
- [ ] 위젯 월간·날짜 클릭·위치 이동/잠금·투명도·위치 복구와 트레이를 연결한다. 유휴 시 계속 repaint하거나 DB를 읽지 않도록 한다.
- [ ] 10회 닫기/재열기, 모니터 구성 변경, Win+D, 연결 실패, 절전 복귀를 실제 확인한다. 미확인 환경은 명시한다. 다른 Explorer 창의 스타일/클래스는 변경하지 않는다.
- [ ] `scripts/build-native.ps1`은 `cargo build -p timeschedule-native --release` 후 별도 `artifacts/TimeSchedule-native.exe`를 준비한다. `cargo tree -p timeschedule-native`와 실행 프로세스 트리에서 Tauri/wry/WebView 의존이 빠졌는지 확인한다.
- [ ] 위젯을 포함한 기능 동등성 표를 채우고 커밋한다. 구현하지 않은 기능을 표시해 둔 상태로 최종 성능 승리를 주장하지 않는다.

### Task 7: 동등 기능 비교와 유지 여부 결정

**Files:** 추가 `docs/rust-native/benchmark.md`, `docs/rust-native/decision.md`; 수정 `README.md`, `docs/diary-verification.md`.

**Interfaces:** Task 1의 동일 fixture·측정 스키마를 사용한다. 보고서에 기준/후보 커밋, EXE hash, renderer, 기능 동등성, 원시 표본 경로, 중앙값/p95/누락 이유를 포함한다.

- [ ] 기준판과 최종 후보를 같은 화면·DB 복사본·전원 상태에서 교차 측정한다. 전체 자식 프로세스와 GPU 관찰, 시작·검색·유휴 CPU·상세 닫기 전후를 구분한다.
- [ ] private working set 30% 절감 목표, 검색 p95 300ms, 시작 허용 범위, 유휴 CPU +0.5%p 조건을 판정한다. 초과 시 원인과 비용을 기록하고 기능 축소로 목표를 맞추지 않는다.
- [ ] 10회 재열기 후 리소스가 안정화되는지 시간 추세를 확인한다. 프레임워크 캐시의 유지와 반복 증가를 구분하고 입력/저장 정확성을 재검증한다.
- [ ] 결과를 “네이티브 채택 권고 / Tauri 유지 권고 / 추가 실험 필요” 중 하나로 정리한다. 장점뿐 아니라 한글 편집·스타일링·향후 모바일·유지보수 비용을 함께 적는다.
- [ ] 새 EXE·측정 보고서·사용자 테스트 절차를 제공하고 계획 PR을 실제 결과에 맞게 갱신한다. 사용자 승인 전 머지하지 않는다.

## 계획 자체 검토

현재 요구의 기능은 Task 3~6에, 성능 가설은 Task 1/7에, 가장 위험한 한글·바탕화면 제약은 Task 2의 초기 통과 조건에 배치했다. 데이터 형식 변경을 목표에서 제외하고 Tauri 기준판을 계속 빌드하도록 했다. 구현 중 프레임워크 API 세부 차이는 고정 버전 문서로 해결하되 위 인터페이스·데이터·사용자 동작 계약은 유지한다. 실제 구현 시작 시 첫 작업은 Task 1 기준 측정이다.
