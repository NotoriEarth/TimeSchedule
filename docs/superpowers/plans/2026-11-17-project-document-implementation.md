# TimeSchedule 프로젝트 문서 구현·검증 계획

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

> **사용자 지정 실행 경계:** 위 스킬의 연속 실행은 사용자가 승인한 하위 계획 하나에만 적용한다. 이 전체 계획의 일괄 구현은 승인되지 않았다. Claude Fable/Opus가 구현하고 Codex Astra가 고정한 변경을 검토한다. 단계 결과·사용자 확인 전 다음 하위 계획을 시작하지 않는다. 상세 절차는 [단계별 운영·브레이크·Rust 학습 지침](../../claude-codex-stage-workflow.md)을 따른다.

**Goal:** Windows 로컬 Codex 앱·Claude Code의 작업 근거를 보존하고, 사용자가 확인한 제안으로 같은 프로젝트 문서를 계속 보완하며 실제 비용·품질 결과로 운영 정책을 교정한다.

**Architecture:** 기존 Rust/Iced 앱·SQLite·로컬 문서 저장소를 유지한다. 수집/근거 저장, 문서 수정본/제안, 생성 실행을 분리하고 CLI/API 실행기는 공통 생성 계약에 연결한다. 먼저 격리한 시험 자료로 저장·복구를 검증하고 실제 새 작업, 문서 보완, 작은 비용 비교 순으로 확대한다.

**Tech Stack:** Rust, Iced 0.14.0, SQLite/rusqlite, Node.js ESM, PowerShell. 설치 도구의 구체적인 훅·출력·인증 지원은 M0에서 검증한다.

**Spec:** [요구사항 v0.2](../../TimeSchedule_Product_Requirements_v0.2_2026-11-17.md), [검토안 v0.2](../../TimeSchedule_Requirements_Review_v0.2_2026-11-17.md), [용어집](../../../GLOSSARY.md).

작성일: 2026-11-17. 조사 기준 HEAD: `7112038566d852f2780277cb7a7113e34bddfa0d`, 네이티브 앱 `0.18.0`, SQLite schema `6`.
문서 상태: 구현 계획. 아래 새 파일·타입·함수는 **제안하는 구현 계약**이며 현재 존재하거나 검증 완료된 기능이 아니다. 실제 착수 시 최신 코드와 비교하고 사용자 변경을 보존한다. 현재 문서 작업에서는 앱 수정·실행·훅 등록·원문 이동·제품의 문서 생성 모델 호출·비용 시험을 하지 않았다. 운영 지침의 문서 검토에는 Codex Astra를 사용했다.

## Global Constraints

- 중심은 계속 보완되는 프로젝트 문서 하나이며 날짜별 작업·고민·실패·검증 일지는 근거다. 하루 요약은 보조이며 일지 목록만으로 개편 완료가 아니다.
- Windows 로컬 Codex 앱·Claude Code부터 지원한다. 클라우드 실행 수집은 후속 범위다.
- 새 경로는 처음 한 번 새 프로젝트/기존 연결/제외를 확인하고 이후 자동 연결한다. 승인 전 본문을 저장하지 않는다. 이름·remote만으로 합치지 않는다.
- 핵심 근거는 유지하고 나머지 허용 원문은 기간 한정 보관한다. 기간은 실사용 후 결정하며 미확정 기간에 의한 자동 만료 삭제는 하지 않는다.
- 허용 문서·일지·필요한 근거 발췌까지 자동 전송한다. 추가 원문은 대상·이유·범위를 확인한 뒤 전송한다. 외부 모델로 근거를 고르는 호출에도 동일 경계를 적용한다.
- 앱 안에서 편집하고 Markdown으로 내보낸다. 외부 파일 편집 자동 반영은 후속 검토다.
- 초기에는 요청 시 보완 제안을 생성하고 사용자가 확인한 뒤 본문에 반영한다. 생성 대기·실패·보류가 수집을 막지 않는다.
- 과거 글·사용자 정정을 그대로 보존해 새 작업과 함께 중심 문서의 근거로 연결한다. 필요한 과거 기록만 이후 재분석한다.
- 한국 시간 `Asia/Seoul` 자정 기준으로 일지를 구분하고 실제 발생 시각을 보존한다. 자정을 넘겨도 같은 프로젝트 문서·작업 연결은 유지한다.
- CLI와 API 모두 가능한 구조를 만들고 공식 CLI부터 검증한다. 승인 없이 유료 API로 전환하지 않는다. 미제공 사용량은 0이 아니라 확인 불가다.
- 별도 시험 프로젝트 → 선택한 실제 프로젝트의 새 작업 순서로 확대한다. 비용 시험은 작은 묶음의 결과를 함께 본 뒤 확대한다.
- 기존 자동 개발 기록의 커밋 검증과 Space 기본 꺼짐은 유지한다. 새 활동 수집 경로와 기존 종료 기록 절차를 구분한다. 새 정기 자동화는 만들지 않는다.
- 사용자 테스트 후 명시적 승인 전에는 PR을 머지하지 않는다. 대표 실행 파일은 `artifacts/TimeSchedule.exe`, 빌드는 `scripts/build-native.ps1`, 과거 EXE는 `artifacts/archive/`다. 실행 중인 앱 강제 종료·사용자 자료 이동은 하지 않는다.

## Review Focus

1. 동일 폴더명의 다른 저장소, Windows 대소문자·junction·worktree 경로가 엉뚱한 프로젝트로 합쳐지는 문제 → 작업 1·2의 경로/동의 시험.
2. 접수 직후 종료·디스크 쓰기 실패·응답 유실로 기록이 사라지거나 두 번 보이는 문제 → 작업 2·3의 장애 지점 재생 시험.
3. 제안 생성 중 사용자 수정·근거 정정·프로젝트 재연결로 오래된 결과가 채택되는 문제 → 작업 5·7의 기준 수정본 비교 시험.
4. CLI가 추가 파일·MCP를 읽거나 원문 속 지시문을 따라 허용 범위를 넘는 문제 → 작업 6의 격리 입력·도구 제한·가짜 비밀 표본 시험.
5. 사용량 누락·캐시 중복 합산·불확실한 타임아웃 재시도로 비용이 작게 보이는 문제 → 작업 6·8의 사용량/실패/중복 실행 시험.

---

## 1. 확정 사항과 현재 구현의 차이

| 현재 확인한 파일·동작 | 재사용할 부분 | 추가할 부분 |
| --- | --- | --- |
| `crates/timeschedule-core/src/diary.rs`: `Project`, `Entry`, 단일 `repo_path` | 기존 project ID와 과거 일지 | 다중 경로 등록·확인 상태, 활동·근거·시각 정보 |
| `crates/timeschedule-core/src/storage.rs`: schema 6, `Storage::open`, `migrate_to` | 저장·이행 기반 | 새 테이블과 회귀 시험. 기존 테이블을 일괄 덮어쓰지 않음 |
| `crates/timeschedule-core/src/project_records.rs`: `ProjectRecord` v1/v2, receipt | 기존 개발 완료 기록 호환 | 별도 활동 계약. 기존 메시지에 새 의미를 억지로 넣지 않음 |
| `apps/timeschedule-native/src/record_inbox.rs`, `record_cli.rs` | 원자적 접수·수신 결과·앱 미실행 전달 패턴 | 활동용 수신함. 기존 128 KiB 입력 제한에 원문 전체를 넣지 않음 |
| `scripts/document-vault.mjs` | 선택한 로컬 vault 경로·원문 보존·경로 보호 규칙 | 활동 근거 저장 계약·보존 분류·삭제 영향 확인 |
| `scripts/project-records-codex.mjs` | bounded child-process 실행의 참고 | 문서 생성 전용 실행기. 기존 Space 원문 게시기를 생성기로 전용하지 않음 |
| `app.rs`, `worker.rs`, `views.rs`, `calendar_ui.rs` | 백그라운드 작업·기존 일정 진입점 | 프로젝트 문서 화면, 근거 일지, 편집/제안 비교/시험 결과 |

기존 코드의 재사용 가능성은 정적 확인 결과다. 새 수집 호환성·부하·실자료 복구·한국어 생성 품질·비용은 아직 검증되지 않았다.

## 2. 배포 단계와 사용자가 받을 결과

| 단계 | 결과물 | 완료 판단 | 사용자 확인 |
| --- | --- | --- | --- |
| M0 연결 가능 범위 | 설치 버전별 지원표와 시험용 입력 표본 | 관측한 이벤트와 미지원·복구 불가를 구분 | 실제 시험할 경로·자료 확인 |
| M1 수집·저장 | 등록 확인, 두 환경 활동 수신, 근거와 상태, 조회 | 재시작 보존·중복 방지·프로젝트 분리·최소 필터 통과 | 별도 시험 프로젝트에서 누락/연결 확인 후 실제 새 작업 확대 |
| M2 문서와 제안 | 중심 문서·과거/새 근거 연결, 앱 편집, 요청 시 생성, 확인 후 채택, 출력·복구 | 근거 추적·사용자 수정 보존·오래된 제안 차단 | 생성 전후 문장과 근거, Markdown 사본 확인 |
| M3 비용·품질 교정 | 작은 비교 묶음, 사용량·품질 결과표, 정책 이력 | 실측 한계 포함 결과를 사용자가 검토하고 유지/변경/보류 기록 | 다음 시험 범위와 운영 정책 선택 |
| M4 후속 기능 | 별도 회고·포트폴리오·고급 검색 | 별도 설계·사용자 필요 확인 | P0 완료를 대신하지 않는 후속 작업 |

M1만 끝났으면 ‘수집 시범’, M2만 끝났으면 ‘문서 시험’으로 표시한다. P0 전체와 M3 사용자 검토가 남은 동안 개편 완료라고 표시하지 않는다. 기간·공수는 M0 결과 전 확정하지 않는다.

## 3. 파일 배치와 공통 계약

기존 큰 `storage.rs`/`app.rs`에는 연결 코드만 추가하고 새 기능 로직은 아래 책임별 파일에 둔다. 이하 경로는 저장소 루트 기준이다.

| 생성 예정 파일 | 책임 |
| --- | --- |
| `crates/timeschedule-core/src/project_activity.rs` | 등록·활동·근거 DTO와 검증·날짜 귀속 |
| `crates/timeschedule-core/src/activity_store.rs` | 경로·활동·근거 DB 작업과 기존 기록 연결 |
| `crates/timeschedule-core/src/project_documents.rs` | 문서/하루 요약 수정본, 제안·채택·갱신 필요 |
| `crates/timeschedule-core/src/generation.rs` | 입력/정책/허용 범위·사용량·실행 결과 계약 |
| `apps/timeschedule-native/src/activity_inbox.rs`, `activity_cli.rs` | 활동용 접수·영수증·처리 CLI |
| `apps/timeschedule-native/src/evidence_store.rs`, `project_backup.rs` | vault 근거 저장·로컬 스냅샷/복구 |
| `apps/timeschedule-native/src/project_document_ui.rs`, `generation_runner.rs` | 문서·제안 화면, UI를 막지 않는 생성 실행 |
| `scripts/activity-capture.mjs`, `scripts/activity-adapters/{codex,claude}.mjs` | 검증한 입력만 변환·필터·전달·재전송 |
| `scripts/document-generation/{contract,cli-runner,api-runner,runner}.mjs` | 공통 생성 입출력과 CLI/API 경로별 실행 |
| `scripts/document-trials.mjs` | 고정 입력 비교 묶음·보고서 생성 |

다음은 타입/JSON 계약의 구현 출발점이다. 경로별 기능은 `capabilities`로 명시하며 없는 기능을 공통 기능인 것처럼 노출하지 않는다.

- `PathRegistration { project_id, canonical_path, decision: new|link|exclude, confirmed_at }`: 불명확한 경로는 후보 메타데이터만 저장. 본문이 있는 미분류 상태는 이미 수집을 허용한 자료에만 적용.
- `ActivityEnvelope { schema_version, source, session_id, event_id, revision, project_id, occurred_at?, received_at, declared_date?, time_basis, kind, body, evidence_refs, origin }`: 작업 도구의 숨은 추론은 수집하지 않음. `origin=timeschedule_generation`은 재수집 제외.
- `EvidenceRef { id, revision, content_hash, managed_relative_path, source_locator, evidence_kind, retention: core|temporary, availability }`: 필터 적용 후 내용의 해시, 원본 발췌 위치, 사용자 보존 지정과 변경 이력 유지.
- 동일 `(source, session_id, event_id, revision)`와 같은 내용은 재전송이다. 같은 키에 다른 내용은 충돌로 보존한다. 서로 다른 이벤트의 비슷한 실패는 별개다.
- `DocumentSnapshot { id, project_id, kind: project|day_summary, day?, revision, markdown, evidence_versions, user_edited, input_fingerprint, policy_version }`: 중심 문서 ID는 지속, 수정본은 누적. 날짜 미상은 수신일을 발생일로 바꾸지 않음.
- `GenerationRequest { run_id, route: cli|api, provider, model, document_snapshot, allowed_evidence, policy_version, trial_id?, limits }`: 승인된 입력만 포함. 추가 원문이 필요하면 `needs_scope` 결과이며 임의 읽기 없음.
- `GenerationResult { run_id, outcome: proposed|needs_scope|failed|cancelled|unknown, proposal?, usage, diagnostics }`: 진단은 필터링된 요약. 비밀값이나 전체 원본 로그를 보관하는 통로로 쓰지 않음.
- `Usage { input_tokens?, cached_input_tokens?, output_tokens?, reasoning_tokens?, inclusion_rules, cost_amount?, currency?, cost_basis: list|provider|billed|unknown, attempts, duration_ms }`: 알려지지 않은 값은 null. 포함 관계에 따라 중복 합산 금지.
- `Proposal { id, document_id, base_revision, input_fingerprint, markdown, evidence_versions, run_id, state: pending|accepted|rejected|stale }`: 모델 성공이 채택을 뜻하지 않음. 사용자 수정 후 채택도 새 수정본과 비교 이력 유지.

예정 DB 테이블은 `project_paths`, `project_activities`, `activity_evidence`, `document_revisions`, `document_proposals`, `generation_runs`, `project_policies`다. 기존 projects/entries/receipts와 연결하며 schema 번호는 착수 시 최신 버전 다음으로 부여한다. 근거 파일은 DB 행보다 먼저 내구성 있게 저장하고, DB에는 파일 ID·해시·참조만 저장한다. 파일 저장 성공 후 DB 실패는 재시도 가능 상태로 남긴다.

## 4. 구현 작업과 검증

공통 절차: 아래 각 작업은 테스트 추가 → 의도한 실패 확인 → 최소 구현 → 해당 시험 통과 → 범위 내 커밋 순으로 수행한다. 독립 배포 경계에서 PR·EXE·검증 보고서를 제공한다. 이 문서 작성 자체에는 제품 테스트를 실행하지 않았다.

### 작업 1 — M0 설치 환경과 수집 계약 확인

**Files:** 생성 `docs/project-capture-support.md`, `tests/fixtures/project-activity/{codex,claude}/`, `tests/activity-adapters.test.mjs`; 이후 구현할 `scripts/activity-adapters/{codex,claude}.mjs`.
**Interfaces:** `normalizeEvent(input, context) -> { outcome: activity|candidate|unsupported|excluded, envelope?, reason? }`. `context`는 등록 경로·동의 상태·수집 출처다. 실제 이벤트 필드 매핑은 설치 버전 표본에서 확인하고 fixture로 고정한다.

- [ ] 기존 앱/CLI의 버전·공식 지원·설정 상태를 읽기 전용으로 확인하고 지원표에 기록한다. 전역 훅을 먼저 설치하거나 전체 사용자 로그를 읽지 않는다.
- [ ] 허용된 시험 위치에서 정상 완료·재개·축약·사용자 중단·오류·강제 종료의 관측 범위를 확인한다. 모델 호출이 필요하면 자료·호출 상한을 먼저 제시한다.
- [ ] `normalize_unknown_version_is_unsupported`, `unconfirmed_path_has_no_body`, `generation_origin_is_excluded` 시험을 작성한다. 예: `assert.equal(normalizeEvent(sample, unconfirmed).envelope, undefined)`.
- [ ] `node --test tests/activity-adapters.test.mjs`로 실패를 확인한 뒤 검증된 버전만 변환한다. 존재하지 않는 훅이나 전체 종료 포착을 전제하지 않는다.
- [ ] 같은 명령 통과 후 표본 출처·미지원/복구 조건을 문서화하고 커밋한다. 훅으로 부족한 경우 허용된 원문 복구/명시적 체크포인트 대안을 기록하고 지원 범위를 축소 표시한다.

**Exit:** 두 도구의 지원표·최소 필터된 표본·실행 방식이 있다. 설치 환경에서 관측되지 않은 항목은 지원 완료가 아니다.

### 작업 2 — 프로젝트 등록·활동 저장·이행

**Files:** 생성 `project_activity.rs`, `activity_store.rs`, `crates/timeschedule-core/tests/project_activity.rs`; 수정 코어 `lib.rs`, `storage.rs`, `diary.rs`.
**Interfaces:** `Storage::register_project_path(&PathRegistration) -> Result<(), String>`, `Storage::ingest_activity(&ActivityEnvelope) -> Result<ActivityReceipt, String>`, `Storage::list_project_activities(&ActivityQuery) -> Result<ActivityPage, String>`. `ActivityReceipt`는 event/revision/outcome(stored|unchanged|conflict)이며 page는 items/next_cursor다.

> **작업 1에서 넘어온 조건(2026-11-18 사용자 결정):** 등록 경로와 도구가 준 작업 폴더의 대조는 이 작업의 책임이다. 작업 1의 어댑터는 `context.registrationFor(cwd)`로 결정을 받기만 하고 경로를 비교하지 않는다. 문자열 비교로 구현했던 대조는 독립 검토에서 네 번 연속 틈이 나와 제거했다. 지켜야 할 조건과 시험 항목은 [수집 지원표](../../project-capture-support.md) 9절의 "등록 경로 대조는 작업 2로 넘겼다"에 있다. 이 작업의 하위 계획은 `registrationFor`의 구현(파일 시스템으로 경로 해석, 가장 깊은 조상, 겹치는 등록의 우선순위)을 포함해야 한다.

- [ ] schema 6 표본에 기존 프로젝트·사용자 편집·대기 receipt를 넣고 이행 후 값이 같다는 시험, 동일명 두 폴더·worktree 별칭·재연결 시험을 작성한다.
- [ ] 이벤트 재전송은 한 항목, 같은 키의 내용 변경은 conflict, 별도 두 실패는 두 항목이라는 assertion을 고정한다. 23:59/00:00 KST 경계·시각 미상·수신 지연·101건 조회의 다음 페이지를 시험한다.
- [ ] `cargo test -p timeschedule-core --test project_activity --locked` 실패 확인 후 테이블·타입·저장 API를 구현한다. Windows 경로 정규화와 등록 확인을 분리하고 표기 차이만으로 다른 승인 범위를 상속하지 않는다.
- [ ] 변경/정정/재연결은 기존 사건과 수정 이력을 보존하며 문서 갱신 필요 대상으로 전달할 변경 ID를 남긴다. 기존 `ProjectRecord` v1/v2 입력을 계속 허용한다.
- [ ] 목적·초기 상태와 결정/문제의 최소 맥락, 관측/추정·테스트 실행/통과·사용자 확인을 별도 필드로 보존한다. 없는 이유는 미기록으로 둔다. 반복 조회의 접기/중요 항목 승격은 표시 분류이며 삭제가 아님을 시험한다.
- [ ] 같은 시험 및 `cargo test -p timeschedule-core --locked` 통과 후 커밋한다. 기존 실제 DB는 시험에 사용하지 않는다.

### 작업 3 — 근거 보관·내구성 수신·두 도구 연결

**Files:** 생성 `activity_inbox.rs`, `activity_cli.rs`, `evidence_store.rs`, `project_backup.rs`의 수집 자료 스냅샷 부분, `scripts/activity-capture.mjs`, `tests/activity-capture.test.mjs`; 수정 native `main.rs`, `launch.rs`, `app.rs`, `worker.rs`; 참고 기존 `record_inbox.rs`, `record_cli.rs`, `document-vault.mjs`.
**Interfaces:** `enqueue_activity(dir: &Path, value: &ActivityEnvelope) -> Result<DeliveryReceipt, String>`, `drain_activities(dir: &Path, store: &Storage) -> Result<DrainReport, String>`. 새 CLI는 `activity enqueue --input FILE --data-dir DIR --result FILE`, `activity drain --data-dir DIR`, `activity status --event-id ID --revision N --data-dir DIR --result FILE`로 구분한다.

- [ ] `queued_survives_restart`, `receipt_loss_replay_is_unchanged`, `evidence_write_failure_has_no_success_receipt`, `invalid_payload_is_quarantined`를 native 모듈 시험에 추가한다. queued는 파일 접수일 뿐 DB 저장 완료가 아니라는 assertion을 둔다.
- [ ] 등록 거절·수집 중지·가짜 비밀값·외부 경로/junction·원문 속 명령문·부분 쓰기·송신 중단을 `tests/activity-capture.test.mjs`에서 실패시킨다. 허용 범위 밖 원문은 durable queue 이전에 차단한다.
- [ ] `cargo test -p timeschedule-native activity_inbox --locked`, `node --test tests/activity-capture.test.mjs tests/activity-adapters.test.mjs` 실패를 확인한 후 atomic spool/receipt와 filtered evidence 저장을 구현한다. 큰 근거는 파일로 분리하고 잘림을 조용히 성공 처리하지 않는다.
- [ ] 두 도구의 검증된 연결만 시험 경로에 활성화한다. 실패 시 송신/수신 상태를 보존하며 미관측 구간은 확인 불가로 표시한다. 지원된 로그 복구는 cursor와 같은 이벤트 ID를 사용한다.
- [ ] 새 활동 정책을 `docs/project-activity-capture.md`로 작성하고 `AGENTS.md`, `docs/project-records.md`에 기존 Git 완료 기록과 새 수집 경로의 구분을 반영한다. 조사·실패 기록은 새 경로에서 허용하고 기존 `prepare`의 Git 검증을 제거하지 않는다. 이중 관측은 provenance로 연결하고 근거 없이 자동 병합하지 않는다.
- [ ] 작업 7에 정의한 `create_project_backup`/`restore_project_backup` 계약의 수집 자료 부분을 먼저 구현한다. DB·근거 파일·등록 경로·대기열·receipt의 일관된 스냅샷을 별도 폴더에 복원해 원문 해시와 전달 상태를 확인한다. `cargo test -p timeschedule-native project_backup --locked` 통과 전 실제 프로젝트로 확대하지 않는다. 작업 7은 문서·제안·생성 기록을 포함하도록 이를 확장한다.
- [ ] 위 시험과 기존 `node --test tests/project-records*.test.mjs tests/document-vault.test.mjs` 통과 후 커밋한다. 기존 Space 꺼짐·과거 excluded/대기 자료는 유지한다.

### 작업 4 — M1 수집 시범 화면과 실제 새 작업 확대

**Files:** 생성 `apps/timeschedule-native/src/project_document_ui.rs`의 등록/일지 부분, `docs/project-activity-verification.md`; 수정 `app.rs`, `worker.rs`, `views.rs`, `calendar_ui.rs`, `main.rs`.
**Interfaces:** `worker::project_activity_snapshot(store, query) -> Result<ProjectActivitySnapshot, String>`; snapshot은 등록 후보·지원 상태·최근 활동·다음 페이지·송신/수신 오류를 포함한다. UI 메시지 `ConfirmProjectPath`, `ActivityPageRequested`, `ActivitiesLoaded`를 연결한다.

- [ ] 등록 확인 전 본문 미표시, 제외 후 추가 수집 없음, 다음 페이지 누락 없음, 두 도구/두 프로젝트 전환 시험을 추가한다. native `project_document_ui` 모듈 시험으로 상태 전이를 검증한다.
- [ ] `cargo test -p timeschedule-native project_document_ui --locked` 실패 확인 후 화면/worker 연결을 구현한다. 느린 읽기·수신 처리는 UI 스레드를 막지 않는다.
- [ ] 필터된 구조화 기록의 중요도 승격/접기, 최소 맥락 편집과 출처·확인 수준을 화면에 연결한다. 미수집 환경의 활동이 없다는 이유로 ‘작업하지 않음’이라고 표시하지 않는다.
- [ ] 시험 프로젝트에서 ‘Codex 작업 → Claude 실패/정정 → 재전송 → 앱 재시작 → 다른 프로젝트 전환’을 확인하고 기대/실제 결과를 기록한다.
- [ ] M1 EXE와 확인 방법을 제공한다. 사용자가 확인한 뒤 선택한 실제 경로와 시작 시점에 새 작업 수집을 확대한다. 시험 자료와 실자료의 통계/문서 범위를 분리한다.
- [ ] 사용자 테스트/자동 시험/미검증을 구분한 보고서와 커밋·PR을 준비한다. M1 성공만으로 전체 개편 완료 처리하지 않는다.

### 작업 5 — M2 중심 문서·근거 일지·과거 글·사용자 편집

**Files:** 생성 `crates/timeschedule-core/src/project_documents.rs`, `crates/timeschedule-core/tests/project_documents.rs`; 수정 코어 `lib.rs`, `storage.rs`, `activity_store.rs`, native `project_document_ui.rs`, `worker.rs`, `app.rs`.
**Interfaces:** `Storage::save_document(&DocumentEdit) -> Result<DocumentSnapshot, String>`, `Storage::save_proposal(&Proposal) -> Result<(), String>`, `Storage::accept_proposal(id: &str, expected_revision: u64, edited_markdown: Option<&str>) -> Result<DocumentSnapshot, String>`, `Storage::link_legacy_entry(project_id: &str, entry_id: &str) -> Result<(), String>`. `DocumentEdit`는 document ID/expected_revision/markdown/evidence_versions다.

- [ ] `same_project_keeps_document_id`, `legacy_body_unchanged`, `accept_stale_revision_is_rejected`, `changed_evidence_marks_proposal_stale`, `declining_proposal_keeps_activity` 시험을 작성한다. 문서 id는 같고 revision만 증가하며 이전 근거와 편집이 남는지 검증한다.
- [ ] `cargo test -p timeschedule-core --test project_documents --locked` 실패 확인 후 문서·제안·수정 이력을 구현한다. 근거 내용/수정본/문서 기준/정책까지 fingerprint에 포함한다. ID 목록만 해시하지 않는다.
- [ ] 과거 기록은 원문과 사용자 편집을 보존한 링크로 연결한다. 날짜·프로젝트가 불명확하면 확인 필요로 표시한다. 과거 데이터 자동 재분석은 실행하지 않는다.
- [ ] 문서 기본 화면에 목적·만든 것·현재 상태·주요 변경 이유·검증 범위·남은 일을 제공하고 각 설명에서 일지/근거로 이동시킨다. 앱 편집/제안 비교/수정 후 채택/보류를 구현한다. 근거를 잃은 주장은 정상 인용으로 보이지 않게 한다.
- [ ] 같은 시험과 UI 시험을 통과시킨 뒤 커밋한다. ‘작은 자료에서만 개선’이라는 사용자 정정, 자정을 넘긴 작업, 과거 보류 후 새 해결 사례를 수동 확인한다.

### 작업 6 — 공통 생성 계약·CLI 우선 연결·API 교체 가능성

**Files:** 생성 코어 `generation.rs`, `tests/document-generation.test.mjs`, `scripts/document-generation/{contract,cli-runner,api-runner,runner}.mjs`, native `generation_runner.rs`; 수정 코어 `lib.rs`, `storage.rs`, native `main.rs`, `worker.rs`, `app.rs`, `project_document_ui.rs`.
**Interfaces:** JS `executeGeneration(request, dependencies) -> Promise<GenerationResult>`; dependencies는 `cli.execute`, `api.execute`, `clock`, `usageSink`다. Rust `generation_runner::start(request: GenerationRequest) -> Result<RunHandle, String>`와 `RunHandle::cancel(&mut self)`는 공통 JSON stdin/stdout 계약을 사용한다. 프로세스 argv 문자열에 자료·인증을 끼워 넣지 않는다.

- [ ] 공통 contract 시험으로 CLI/API 시험 실행기가 같은 입력·제안 구조를 반환하는지, 미지원 기능은 명시하는지 검증한다. `api_call_count_after_cli_failure == 0`, `unknown_cost != 0`, 취소/시간초과/출력 불완전이 accepted가 아니라는 assertion을 둔다.
- [ ] `node --test tests/document-generation.test.mjs` 실패 확인 후 실행기 경계를 구현한다. API는 실제 공급자 연결 전 `not_configured` 상태와 시험 실행기로 계약을 검증하고 실제 지원 완료로 표시하지 않는다.
- [ ] 승인된 CLI를 별도 실행으로 연결한다. 프로젝트 전체 자동 탐색·외부 도구·MCP·과거 대화 재개를 제한하고 허용된 문서와 근거만 입력한다. 공급자·클라이언트·정책 버전과 격리 제한을 기록한다.
- [ ] native worker에서 요청 버튼·취소·실행 상태·제안 저장·사용량을 연결한다. 생성 중 편집/종료에도 본문을 자동 변경하지 않는다. 초기 구현 동시 실행은 한 개의 생성 작업부터 시작하는 개발 제안이며 큐·요청 합치기를 검증한다.
- [ ] 토큰 포함 관계·캐시/추론 미제공·실패/취소 사용량·SDK/CLI 자체 재시도를 계측한다. 실행 상태가 unknown이면 성공으로 간주하거나 무조건 재호출하지 않는다. 새 기록 수집은 계속한다.
- [ ] 실제 호출 전에 경로·자료·후보 모델·상한을 제시하고 작은 연결 시험을 실행한다. 승인 없는 API 호출·자동 fallback이 없음을 확인한다. CLI 제약 확인 후 승인된 비교 범위에서만 `api-runner.mjs`에 실제 공급자 연결을 추가하고 같은 contract 시험 및 실제 사용량 검증을 수행한다.
- [ ] Node 시험 및 `cargo test -p timeschedule-native generation_runner --locked` 통과 후 커밋한다. 모델·분량·자동 반영은 확정 운영값으로 고정하지 않는다.

### 작업 7 — 출력·백업·보존 정책·하루 요약

**Files:** 생성 `crates/timeschedule-core/tests/project_retention.rs`; 수정 작업 3의 native `project_backup.rs`, `evidence_store.rs`, `project_documents.rs`, `generation.rs`, `storage.rs`, `project_document_ui.rs`, `worker.rs`.
**Interfaces:** `export_project(store: &Storage, project_id: &str, destination: &Path) -> Result<ExportManifest, String>`, `create_project_backup(store: &Storage, vault: &Path, destination: &Path) -> Result<BackupManifest, String>`, `restore_project_backup(manifest: &Path, isolated_destination: &Path) -> Result<RestoreReport, String>`; manifest는 schema/파일 hash/문서 revision/근거 ID/전달 상태를 포함한다.

- [ ] 수정한 내보내기 파일을 자동 덮어쓰지 않음, 원문 삭제가 인덱스/제안/인용에 반영됨, core 근거는 일반 만료에서 제외됨, `retention_days=None`이면 자동 삭제 0건인 시험을 추가한다.
- [ ] `cargo test -p timeschedule-core --test project_retention --locked`, `cargo test -p timeschedule-native project_backup --locked` 실패 확인 후 Markdown/구조화 출력·보존 상태·명시적 삭제 영향 표시를 구현한다. 출력물은 사본이며 외부 수정 자동 반영은 없다.
- [ ] DB와 근거 파일의 일관된 시점을 확보하는 짧은 저장 조정 구간과 SQLite 스냅샷을 구현한다. 기존 DB migration 전 백업을 전체 vault 복구 검증으로 오인하지 않는다. 다른 격리 폴더에 복원해 문서·근거·사용자 수정·대기 전달을 실제 조회한다.
- [ ] 프로젝트·날짜·키워드의 기본 검색을 문서와 근거 일지에 연결하고 현재/이전 수정본·삭제/보관 자료 범위를 표시한다. `project_documents` 시험에서 검색 결과의 인용 대상과 페이지 이동을 검증한다. 의미 검색은 도입하지 않는다.
- [ ] `kind=day_summary`를 같은 제안 체계로 구현한다. KST 날짜별 수동 생성부터 제공하고 변경 없음이면 자동 호출하지 않는다. 정책 설정/승인 전 자동 마감은 꺼짐이며, 이후 켰을 때 앱 미실행 기간을 누락 처리하는 로직은 가짜 시계로 시험한다. 상주 서비스나 정기 자동화를 등록하지 않는다.
- [ ] 기존 `document-vault.test.mjs`, core/native 관련 회귀 시험을 통과한 뒤 커밋한다. 압축은 포맷·전환 기간 실측 전 자동 활성화하지 않는다. 보관량·근거 재조회·보존 지정 지표를 수집해 결정 자료를 제공한다.

### 작업 8 — M3 작은 비용·품질 비교와 사용자 교정

**Files:** 생성 `scripts/document-trials.mjs`, `tests/document-trials.test.mjs`, `docs/project-document-trial-template.md`; 수정 `generation.rs`, `document-generation/runner.mjs`, `project_document_ui.rs`. 실제 입력·결과는 Git 제외 `.cache/document-trials/<trialId>/`, 공유용 보고서는 민감자료를 뺀 요약이다.
**Interfaces:** `runTrial(plan, executors) -> Promise<TrialReport>`; plan은 input_snapshot/route/model/policy/variants/repeats/limits, report는 runs/document_diffs/evidence_checks/usage/quality_feedback/unknowns/user_decision다.

- [ ] fake executor로 같은 입력 비교, 승인 범위 초과 시 추가 실행 0건, 실패·재시도 포함, 캐시 중복 합산 금지, 실행 중 중단 비용 unknown 보존을 시험한다. `node --test tests/document-trials.test.mjs` 실패 확인 후 제한·보고서 로직을 구현한다.
- [ ] 첫 묶음은 측정·결과 확인용 소규모 표본으로 제시한다. 동일 모델·시작 문서·근거·실행 시점에서 전체/부분 갱신을 비교하고, 갱신 범위를 고정한 뒤 즉시/묶음을 비교한다. 모델 비교는 나머지 조건을 고정한다.
- [ ] 사용자 확인 후 긴 문서·중요 설계 변경·누락 근거·과거 정정·사용자 편집·실패/취소·변경 없음·반복 실행을 포함해 확대한다. 미지원·원문 소실을 생성으로 채우지 않는다.
- [ ] 같은 표본의 문서 차이, 조건·수치·미검증 보존, 근거 정확성, 수정 시간, 지연, input/output/cache/reasoning, 실패 포함 호출 수, 비용 성격을 보여준다. 개발 세션 전체 비용을 기록 비용으로 돌리거나 기록 작성 비용을 누락하지 않는다. 분리 불가는 표시한다.
- [ ] 각 묶음의 유지/변경/보류와 이유를 저장한다. 정책 변경 시 입력·정책 버전을 남기고 영향받는 시험만 다시 수행한다. 자동 생성/반영 확대는 사용자 선택 후 적용한다.
- [ ] 시험 도구 검증과 실제 관측 결과를 구분해 커밋·보고한다. 작은 표본의 값으로 월간 실제 청구나 안정된 평균을 단정하지 않는다.

### 작업 9 — 단계별 출시·회귀·인수 확인

**Files:** 각 단계의 구현 파일과 `apps/timeschedule-native/Cargo.toml`, `Cargo.lock`, `CHANGELOG.md`, `README.md`, `docs/project-document-verification.md`.

- [ ] 작업 시작 기준 커밋·작업 브랜치·변경 경로를 확보한다. 기존 미커밋 사용자 자료를 커밋에 섞지 않는다. 단계 간 PR 의존성을 명시한다.
- [ ] 제품 변경 중요도에 맞춰 native 버전과 lock/changelog/README를 함께 갱신한다. 계획 문서 작성만으로 버전을 올리지 않는다.
- [ ] 아래 공통 검사와 `scripts/build-native.ps1`을 수행한다. 사용자 앱 실행 중이면 강제 종료하지 않고 교체가 가능한 상태에서 대표 EXE를 준비한다.
- [ ] 수집/등록/실패/재시작/문서 편집/제안 채택/근거 이동/출력/격리 복구 중 이번 배포의 실제 확인 범위를 보고한다. 사용자 테스트 승인 전 머지하지 않는다.
- [ ] 실제 개발 변경을 커밋한 작업에만 `docs/project-records.md`를 따른다. 작업 기준/완료 커밋과 해당 변경 경로만 전달한다. 이 계획 문서 작업은 자동 기록 대상이 아니다.

## 5. 검증 명령과 결과 판정

Windows 저장소 루트에서 `scripts/toolchain.ps1`로 도구 환경을 먼저 로드한다. 아래는 **향후 구현 검증 명령**이며 이번에 실행한 결과가 아니다. 새 파일 시험은 해당 작업 구현 후 실행한다.

```powershell
. .\scripts\toolchain.ps1
cargo test -p timeschedule-core --locked
cargo test -p timeschedule-native --locked
node --test tests/*.test.mjs
powershell -NoProfile -File .\scripts\build-native.ps1
git diff --check
```

판정은 exit code만 보지 않고 새 시험이 실제 실행됐는지 확인한다. 자동 테스트와 UI 수동 확인을 구분하고, 빌드 성공은 새 도구 연결/실자료 비용/복구 성공의 증거가 아니다. UI나 외부 도구 미검증은 검증 보고서에 남긴다.

## 6. 요구사항 추적표와 완료 범위

| 요구 | 담당 작업 | 핵심 증거 |
| --- | --- | --- |
| FR-01~03 | 1, 2, 4 | 설치 환경 지원표, 등록 후보/제외, 다중 경로, 초기 상태 미상 |
| FR-04~07 | 1~4 | 두 도구 활동, 커밋 없는 실패, 재전송/중단/복구, 가치 분류·접기/승격 |
| FR-08~10 | 2, 3, 5 | 출처·근거 버전, 관측/추정·테스트/사용자 확인 분리 |
| FR-11,12,14,15,27 | 2, 4, 5, 6 | 같은 문서 ID, 날짜/프로젝트 연결, 사용자 편집, stale 차단, 페이지 이동 |
| FR-13 | 7, 8 | 수동 날짜 요약, 승인한 자동 마감 정책의 가짜 시계 시험·실측 후 적용 |
| FR-16~18,28 | 6, 8 | CLI/API 계약, 제한·실패 사용량, 제안 차이·비용·사용자 정책 교정 |
| FR-19~21 | 2, 3, 4, 7 | 필터/추가 전송 확인, 보존·삭제 파급, 백업·복구, 상태·지원 범위 |
| NFR-01~08 | 1~9 | 모델 실패와 저장 독립, 훅/화면 지연 측정, 입력 추적, 회귀·필터·호환·한국어·실측 |
| FR-22~25(P1), FR-26(P2) | M4 이후 별도 계획 | 회고·대표 사례·포트폴리오·고급 검색·원격/다기기. 이번 P0 완료에 포함하지 않음 |

M1 실제 새 작업 확대 전: 등록·필터·접수/저장 구분·시험 데이터 격리·백업 복구 확인이 필요하다. M2 생성 전: 허용 입력·실행 경로·사용량·시험 상한이 필요하다. M3 완료 전: 비교 자료와 한계를 사용자가 보고 정책을 결정해야 한다. 기존 v0.1 원문은 보존한다.

## 7. 실측 후 결정할 항목

| 미확정 항목 | 보고 판단할 자료 | 결정 시점/그 전 동작 |
| --- | --- | --- |
| 최종 모델·추론 설정·발췌 분량 | 같은 근거의 한국어 문서 품질, 누락/왜곡, 토큰·비용, 수정 시간 | M3에서 교정. 시험 후보는 실행 전만 지정 |
| 전체/부분 갱신·묶음 간격 | 조건을 고정한 비교, 긴 문서 문맥·비용·지연 | 초기 요청 시 생성; 운영 정책은 결과 후 |
| 자동 생성 조건·하루 요약 실행 시각 | 미반영 지연·요청 부담·읽지 않은 제안 비용 | 초기 자동 생성/정기 작업 꺼짐; KST 날짜 기준은 이미 확정 |
| 자동 반영 범위 | 잘못된 변경, 사용자 편집 충돌, 확인 부담 | 초기 모든 제안 확인 후 채택 |
| 원문 보관 기간 | 저장 증가·과거 재조회·누락 교정·보존 지정 | core 유지, 기간 미확정 자동 만료 삭제 없음 |
| 압축 형식·전환 시점 | ZIP/7z/Zstd의 실제 용량·해제 지연·검색/복원·손상 시험 | ZIP은 우선 후보, 30일은 예시. 자동 압축·원본 정리 미활성 |
| 실제 API 경로 채택 | CLI의 구체적 제약, 같은 입력 API 비용·품질·실행 제어 | 공통 구조 확보, 승인 없이 유료 API 호출/전환 없음 |
| 운영 예산·수정 부담 허용치 | 작은 묶음 결과와 누적 실사용, 실제/추정 구분 | 실측 후 확정. 시험 상한은 실제 호출 전에 별도로 정함 |

실측을 기다릴 필요 없이 실행 전에 정할 항목도 구분한다: 시험 프로젝트의 실제 경로, 수집 시작 시점, 시험 자료 범위, 최초 모델 후보, 실행/재시도/시간·입출력 상한, 필요한 인증/외부 전송 범위. 정책 선택이 무제한 실행 승인을 뜻하지 않는다.

## 8. 남는 한계와 대응

- 도구가 이벤트를 제공하지 않고 원문도 사라진 구간은 복구할 수 없다. 지원표·마지막 관측·복구 가능 여부를 표시하고 허용된 체크포인트/원문 경로로 보완한다.
- 발췌 필터와 생성 모델은 완벽하지 않다. 전송 입력 범위 제한·근거 연결·조건 보존 시험·사용자 확인을 함께 사용하며 모든 정보가 검증됐다고 표시하지 않는다.
- 구독 비용을 실행별 실제 금액으로 나눌 수 없을 수 있다. 관측 토큰/실행·한도 정보를 제공하고 정확한 분리 불가를 유지한다. 승인된 API 비교는 대안이지만 별도 과금이다.
- 작은 시험은 현실의 모든 긴 작업을 대표하지 않는다. 같은 표본 반복과 실제 새 작업으로 확대하되 추정과 관측을 구분한다.
- 압축은 토큰 절감이나 백업을 대신하지 않는다. archive 전환 후에도 근거 ID·검색·삭제·복원을 검증해야 하며 기간·포맷 확정 후 별도 작업으로 구현한다.

## 9. 이번 문서 검증과 다음 실행

이번에 확인한 것은 파일/기존 코드의 위치·계약, 확정 요구의 반영, 단계·파일·시험 계획 간 대응이다. 제품 기능·비용·압축률·새 EXE는 검증하지 않았다. 계획의 체크박스는 모두 미완료로 둔다.

계획 검토 후 첫 실행은 작업 1(M0)의 설치 환경·지원 범위 확인이다. Claude Fable/Opus의 담당 범위를 하위 계획으로 나누고, 각 범위의 결과를 Codex Astra가 검토한다. 사용자에게 결과·실행 가능한 경우 시험 방법·실제 코드에 근거한 Rust 설명을 전달한 뒤 다음 범위 승인을 기다린다. 같은 작업 트리에 여러 구현자를 동시에 쓰지 않는다. 실제 모델·검토 커밋·사용량을 기록하며 모델 이용 불가 시 임의 대체하지 않는다. 이 계획 문서가 유료 시험·전역 연결 설정·PR 머지 승인까지 대신하지 않는다.
