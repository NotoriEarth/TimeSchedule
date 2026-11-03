# Local Calendar MCP Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [x]`) syntax for tracking. The user's explicit step-1 implementation instruction and repository workflow authorize execution without another permission round.

**Goal:** 앱 실행 중 사용할 수 있는 가벼운 인증된 일정 MCP를 제공한다.
**Architecture:** 기존 Rust 앱 안에 HTTP MCP를 넣고 공통 Storage를 재사용한다. 쓰기 후 변경 알림으로 앱과 위젯을 갱신한다.
**Tech Stack:** Rust/Iced/Tokio, SQLite, 호환되는 MCP SDK 또는 표준 전송 라이브러리.
**Spec:** `SOFTWARE_FACTORY.local-calendar-mcp.md` v1, main이 유일 작성자.

## Global Constraints

- localhost 전용, 토큰 인증, Host/Origin 검사, 한정된 입력/조회/동시 실행.
- 일정 조회·추가·수정만. 프로젝트/Space/개인 서버는 후속 작업.
- 기존 데이터와 편집 초안 보존. 동일 저장 기능과 화면 갱신 경로 재사용.
- 앱 시작/완전 종료에 수명 연결. CLI 기록 실행에서는 시작하지 않는다.
- 0.9.0 버전 관련 파일 갱신, 대표 EXE 빌드, 실행 중 사용자 앱 보존.

## Review Focus

- 누락한 수정 필드와 오래된 수정 요청: 기존 값 보존 또는 충돌 반환.
- 인증 없는 요청·다른 Origin·위조 Host·큰 입력: 데이터 노출/변경 없이 거절.
- 제한보다 많은 일정과 긴 본문: 제한된 목록, 상세는 별도 조회.
- 열려 있는 편집기·빠른 일정·트레이 상주: 초안 보존 및 변경 표시.
- 포트 점유·서버 중지·CLI/두 번째 앱: 기존 앱 기능과 데이터 보존.

### Task 1: Shared calendar operations and MCP transport

**Files:** 필요 최소의 `crates/timeschedule-core/src/storage.rs` 또는 신규 일정 모듈; `apps/timeschedule-native/src/mcp*.rs`; 의존성 manifest/lock.
**Interfaces:** 공통 Storage를 소비하고 일정 목록/상세/추가/부분 수정 도구를 제공한다. 수정 충돌과 변경 알림 인터페이스는 구현 시작 시 고정한다.
- [x] 입력 제한, 부분 수정 보존, 조회 제한, 오래된 수정 거절 테스트를 작성하고 실패를 확인한다.
- [x] 기존 일정 검증을 재사용하는 최소 저장 인터페이스를 구현한다.
- [x] 인증 및 HTTP/MCP 도구 왕복 테스트를 작성하고 실패를 확인한다.
- [x] SDK와 호환 버전을 고정하고 전송·도구·제한을 구현한다. SDK 사용이 환경상 불가능할 경우 원인과 작은 대안을 main에 보고한다.
- [x] 해당 테스트를 실행해 정상 및 거절 동작을 검증한다.

### Task 2: App lifecycle, refresh, and setup

**Files:** `apps/timeschedule-native/src/{main,app,views,...}.rs`, `docs/local-mcp.md`, 필요시 `scripts/*mcp*`.
**Interfaces:** Task 1의 서버 핸들과 변경 알림을 사용한다. 앱의 기존 Arc<Storage>, refresh/load_quick 경로를 재사용한다.
- [x] 종료·충돌·변경 알림 및 편집 초안 보존 테스트를 작성하고 실패를 확인한다.
- [x] 앱 시작/완전 종료와 연결하고 안전한 설정·상태 확인 방법을 제공한다.
- [x] Codex/Claude Code 설정 안내를 작성한다. 토큰은 로그/문서/커밋에 넣지 않는다.
- [x] fixture로 일정 왕복 및 실제 서버 종료를 확인한다. 실제 클라이언트가 없는 경우 그 한계를 명시한다.

### Task 3: Release and verification

**Files:** 버전 manifest/lock, CHANGELOG, README, 테스트 및 fixture 자료.
- [x] 버전 0.9.0과 변경 내용을 갱신한다.
- [x] `cargo fmt --check`, `cargo test --workspace --locked`, `node --test tests/*.test.mjs`를 실행한다.
- [x] `scripts/build-native.ps1`로 빌드한다. 사용자 앱이 실행 중이면 종료하지 않고 교체 보류를 보고한다.
- [x] 동일 fixture/화면 조건으로 MCP on/off 유휴 자원 및 요청 후 메모리 상태를 가능한 범위에서 비교한다. 부족한 측정은 미확인으로 남긴다.
- [x] 변경 파일·명령·결과·미확인 한계를 main에 전달하고 중지한다. main이 독립 검토 후 커밋/푸시/PR/개발 기록을 처리한다.
