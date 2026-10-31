# Space 중심 자동 프로젝트 기록 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 작업 맥락을 Space에 자동 기록하고 다이어리에 요약과 링크를 저장하며, PR 작성 시 유용한 부분만 선택적으로 재사용한다.

**Architecture:** 작업별 로컬 전달 자료와 재시도 상태를 유지한다. 기록용 Codex CLI는 Space 저장을 담당하고 같은 TimeSchedule EXE의 비 GUI 명령은 앱의 inbox를 통해 코어 저장 계층에 반영한다. 작업 종료 절차와 기존 오전 9시 자동화가 동일 전달 경로를 쓴다.

**Tech Stack:** Rust, rusqlite, serde_json, 기존 Iced worker, Node.js 기본 모듈, Codex CLI와 Pages 도구.

**Spec:** `docs/superpowers/specs/2026-10-31-automatic-project-records-design.md`

## Global Constraints

- 같은 `codex/automatic-project-records` 브랜치에서 작업하고 사용자 변경을 섞지 않는다. 현재 독립 작업 브랜치와 기존 빌드 캐시를 사용한다.
- 기록용 모델은 `gpt-6-luna`, reasoning `medium`; 개발 모델과 기존 heartbeat의 모델 설정은 바꾸지 않는다.
- Space 저장과 다이어리 저장에는 PR이 필요하지 않다. PR 수정은 이 자동 기록기의 기능이 아니다.
- 사용자 편집·삭제·미저장 초안과 Tauri 데이터 경로를 보존한다. 강제 종료 금지.
- 대표 EXE는 `artifacts/TimeSchedule.exe`, 기능 버전은 `0.8.0`; 사용자 테스트 승인 전 병합 금지.

## Review Focus

- 뒤늦게 도착한 이전 revision이 최신 기록이나 사용자가 삭제한 기록을 되살리지 않아야 한다: Task 1.
- 하나의 저장소 경로에 프로젝트가 중복 등록돼 있으면 임의 선택·중복 생성을 하지 않아야 한다: Task 1.
- 큐 처리 중 앱 종료·원자적 저장 실패·동시 enqueue에서 완료되지 않은 작업을 성공 처리하지 않아야 한다: Task 2.
- GUI 편집 중 외부 기록이 도착해도 편집 초안을 교체하거나 변경되지 않은 데이터로 덮어쓰지 않아야 한다: Task 2.
- Space 생성 응답을 잃거나 CLI 출력이 오염되면 같은 페이지를 다시 생성하거나 검증 없는 URL을 다이어리에 넣지 않아야 한다: Task 3.

### Task 1: 공통 코어의 외부 프로젝트 기록 저장

**Files:** `crates/timeschedule-core/src/project_records.rs`, `src/lib.rs`, `src/storage.rs`, `Cargo.toml`.

**Interfaces:** `ProjectRecord { schema_version:u32, work_id:String(UUID), revision:u64, project_title:String, repo_path:String, date:String, kind:String, title:String, summary:String, space_url:String, status:String }` (camelCase JSON). `Storage::apply_project_record(&ProjectRecord) -> Result<RecordReceipt,String>`. `RecordReceipt { work_id, revision, outcome, entry_id, message }`; outcomes saved/unchanged/stale/conflict. Core validation rejects unsupported versions, malformed IDs, nonabsolute paths, oversized text, invalid status and non-Space HTTPS URLs. Maximum input JSON is 64 KiB at the file boundary.

- [ ] Write tests `record_without_pr_round_trips`, `revisions_are_idempotent_and_stale_is_ignored`, `user_edit_and_delete_are_preserved`, `ambiguous_project_path_is_rejected`, `v2_migration_preserves_records`, `invalid_record_is_atomic`; use real temporary/in-memory SQLite and independently asserted Korean titles/URLs.
- [ ] Run `cargo test -p timeschedule-core project_records --offline`; observe expected missing-feature failures before implementation.
- [ ] Implement schema v3 receipt ledger without a foreign key that would erase deletion history; apply project/entry/ledger in one immediate transaction. Store last-managed snapshot separately from editable fields. Use existing search triggers and existing source_ref uniqueness.
- [ ] Run `cargo test -p timeschedule-core --offline`; all core tests pass, including existing v1 migration/search/Git import.
- [ ] Commit the storage deliverable and record tests in the execution ledger.

### Task 2: 단일 EXE의 CLI와 수신 큐 및 화면 반영

**Files:** `apps/timeschedule-native/src/record_inbox.rs`, `src/record_cli.rs`, `src/main.rs`, `src/app.rs`, `Cargo.toml`.

**Interfaces:** `enqueue(data_dir:&Path, record:&ProjectRecord) -> Result<RecordReceipt,String>`, `drain(data_dir:&Path, store:&Storage) -> Result<DrainReport,String>`, `receipt(data_dir:&Path, work_id:&str, revision:u64)`. CLI: `TimeSchedule.exe record enqueue --input FILE [--data-dir DIR] [--result FILE]`, `record drain`, `record status --work-id UUID --revision N`. A result file is the reliable Windows GUI-subsystem boundary. JSON stdout is also emitted where available.

- [ ] Write queue tests for duplicate submission, revision ordering, malformed/oversized file isolation, restart after DB commit before receipt, and status distinguishing queued/saved/conflict. Observe RED with `cargo test -p timeschedule-native record_ --offline`.
- [ ] Implement queue writes through unique temporary files and atomic rename, validated UUID/revision filenames, dedicated inbox/receipts/failed folders and bounded drain. GUI instance lock gates DB access; CLI queues if GUI owns the lock. Receipt is written before removing queue data.
- [ ] Dispatch CLI before DPI/GUI initialization; use the existing default data directory and InstanceLock. Invalid commands return nonzero without opening GUI.
- [ ] Add `RecordsDue/RecordsDrained` worker messages, one drain in flight, initial and 60-second polling; skip while an editor is open or mutation is pending, then drain when free. Preserve editor state and allow completion messages through modal filters. Refresh data only when changed.
- [ ] Test actual temporary DB CLI inputs and GUI message handling; run `cargo test -p timeschedule-native --offline` and commit.

### Task 3: Space 기록 전달과 자동 실행

**Files:** `scripts/project-records.mjs`, `tests/project-records.test.mjs`, `docs/project-records.md`, `AGENTS.md`.

**Interfaces:** CLI `node scripts/project-records.mjs prepare --input FILE`, `sync [--work-id UUID] [--state-dir DIR] [--exe FILE]`, `status`. Prepare accepts workId, revision-independent substantive content, KST date, repo/project, title/kind/status, detailedMarkdown, diarySummary, and evidence paragraphs/links. State directory defaults to ignored `.cache/project-records`; prepare determines the monotonic revision from normalized content and persists immutable payloads. `sync` publishes then invokes Task 2 with the verified Space URL. A lock prevents concurrent sync for one state directory.

- [ ] Write Node tests using real state directories and only external publisher/importer adapters as test doubles. Cover no-PR work, unchanged skip, wrong read-back body/URL, uncertain publish requiring reconciliation, diary-only retry after Space success, queued receipt, user edit conflict, and exclusion of unsupported summary claims.
- [ ] Run `node --test tests/project-records.test.mjs`, observe RED, then implement prepare/status/sync state transitions. Keep shell arguments arrays, no prompt command interpolation, no authentication values in logs.
- [ ] Use `codex exec --ephemeral --json --output-schema` with explicit Luna medium and bounded process/output lifetime. Prompt supplies exact body, current page mapping, scoped parent and recovery mode; preserve source paragraphs and requires read-back. Persist intent before publication; an uncertain create cannot blindly retry. Validate Page URL and matching read-back content before importing.
- [ ] Prefer already authored diarySummary; where absent use extractive source paragraphs, preserving result/limitation groups. No model-generated invented facts. Actual live writer trial uses the real feature's record, marked in-progress, rather than fabricated portfolio accomplishments.
- [ ] Add the automatic completion procedure to AGENTS.md and document retry/status/explicit conflict resolution. Update existing heartbeat through `automation_update`, preserving id, schedule, target and notification intent; its prompt uses the same state and scoped evidence, records only meaningful new work and reuses Space for PRs optionally.
- [ ] Run Node tests and one real Luna/Pages write/read plus native isolated-DB import; verify duplicates skip model and Page writes. Commit implementation and verified results.

### Task 4: 검증과 릴리스 및 PR

**Files:** native `Cargo.toml`, `Cargo.lock`, `CHANGELOG.md`, `README.md`, `docs/project-records-verification.md`.

- [ ] Bump native version to 0.8.0; update lock/version docs. Run `cargo fmt --all -- --check`, workspace Rust tests, relevant Tauri shared-core regression, and all Node tests.
- [ ] Run `scripts/build-native.ps1`; if representative EXE is running, finish building first and request user exit without forced termination. Verify Windows file version and CLI against final release EXE.
- [ ] Exercise automatic queue ingestion with a test-owned GUI instance and isolated data. Verify source URL preservation and existing link-open adapter; distinguish actual click testing from code/integration verification.
- [ ] Request one independent whole-branch review on the most capable available model per executing-plans. Fix substantive findings with regression tests and rerun affected/full checks as needed.
- [ ] Update this PR's title/body to the final implementation, attach to chat, deliver one EXE and test instructions. No merge until explicit user test approval.

Execution choice: implement inline in the current session to avoid fresh implementation contexts per task; one independent final review. User's correction to the design confirmation fixes the direction as Space first, with PR reuse optional; no further workflow-choice questions are needed.
