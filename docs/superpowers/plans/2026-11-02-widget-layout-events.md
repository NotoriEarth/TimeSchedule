# Widget Layout Events Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. The user selected the prior behavior table and explicitly requested research followed by implementation. Preserve that authorization without another approval loop.

**Goal:** 설정창의 반복 배치 흔들림을 없애고 이동/화면 변경 이벤트로 위젯 위치·크기를 보정한다.
**Architecture:** 기존 native subclass와 앱의 이벤트 소비 경로를 활용한다. 사용자 선호 geometry와 임시 표시 geometry를 구분하고, UI 레이아웃은 작업 상태와 분리한다.
**Tech Stack:** 기존 Rust/Iced/Win32, 기존 renderer 및 native fixture. 새 상주 프로세스 없음.
**Spec:** `SOFTWARE_FACTORY.widget-layout-events.md` v1, main sole writer.

## Global Constraints

- 사용자 지시의 4행 비교표와 선호 geometry 보존을 구현한다. 0.9.1, 대표 native EXE, 기존 Tauri/MCP/기록 기능 보존.
- 기본 display 알림 합치기 200ms, 보조 복구 30초. 다른 500ms Tick 기능은 유지.
- 사용자 모니터 설정/데이터/실행 앱을 변경하지 않는다. product worker만 제품 코드 작성, main은 계획·검증·전달 담당.

## Review Focus

- 짧은 native 작업과 background 알림이 반복될 때 실제 설정 layout/scroll bounds가 달라지는가.
- 임시 축소/화면 이동이 사용자 선호 설정을 덮어써 재확장·재시작 복원이 실패하는가.
- 사용자 이동 중 화면 변경, 자동 SetWindowPos의 재진입, 잠긴 위젯의 화면 밖 복구가 서로 충돌하는가.
- 음수 좌표·작은 parent work area·DPI/모니터 제거 및 사라진 HWND를 안전하게 처리하는가.
- 알림 누락 대비가 숨김/표시·종료 경로를 깨거나 다시 1초 반복 조회를 만드는가.

### Task 1: Stable settings layout

**Files:** `apps/timeschedule-native/src/views.rs`, `widget_settings.rs`, `app.rs`, `app/tests/visual.rs`.
**Interfaces:** `App::native`/`NativeDone`의 background 처리와 사용자 작업 표시를 분리하고 실제 `views::settings`를 고정 배치로 그린다.
- [x] 실제 설정 element/tree를 retained UI 상태로 정상→작업 중→완료 반복해 버튼/슬라이더와 전체/scroll geometry가 동일해야 한다는 테스트를 작성하고 기존 코드의 RED를 확인한다.
- [x] 버튼·슬라이더·고정 높이 상태 영역을 항상 유지한다. 사용자 작업 중 필요한 컨트롤만 비활성화하며 background 작업이 UI busy를 켜지 않게 한다.
- [x] 실제 renderer 이미지 및 좁은/일반 창 크기·두 테마의 관련 geometry 테스트를 통과시킨다. 단순 snapshot 이름 갱신으로 실패를 숨기지 않는다.

### Task 2: Event-driven geometry and adaptive recovery

**Files:** `crates/timeschedule-desktop/src/windows.rs`, 필요한 `preferences.rs` 또는 작은 geometry 모듈; native `app.rs`, `widget_settings.rs`.
**Interfaces:** native adapter는 이동 종료/geometry 변경과 display 변경을 합쳐 앱에 전달한다. 선호 위치/논리 크기는 별도로 보존한다. 실제 공개 타입/함수는 worker가 기존 구조에 맞게 최소화하고 첫 구현 체크포인트에서 고정해 main에게 알린다.
- [x] 소유 Win32 fixture에서 이동 종료 저장 신호·동일 값 중복 방지·idle 주기 조회 없음의 RED를 확인한다.
- [x] 기존 callback의 이동 종료와 프로그램 변경을 구분하고 UI 스레드의 안전한 신호 전달을 구현한다. 기존 1초 Position poll을 제거한다.
- [x] 화면 변경 알림의 실제 수신 경로를 연결하고 200ms 합치기/30초 보조 복구를 구현한다. geometry query/저장/표시 변경을 필요한 경우로 제한한다.
- [x] 선호와 현재 geometry를 구분해 작은 화면→보정→큰 화면의 복원, 사용자 새 이동의 선호 갱신, 잠금, 재시작, 음수 좌표/배율을 검증한다. 실제 모니터 설정 대신 소유 parent HWND의 표시 영역과 주입 가능한 경계를 사용한다.
- [x] 관련 RED→GREEN 및 실제 native 메시지/사각형 검증을 통과시킨다. 사용자 시스템 모니터를 변경하지 않는다.

### Task 3: Release and verification

**Files:** 버전 manifest/lock, README, CHANGELOG, `docs/widget-layout-events-verification.md`, 필요 시 scoped fixture script/tests.
- [x] 0.9.1과 검증/한계 안내를 갱신한다.
- [x] fmt/diff check, `cargo test --workspace --locked --offline`, `node --test tests/*.test.mjs`, 필요한 renderer/native fixture 검증을 실행한다.
- [x] `scripts/build-native.ps1 -Offline`로 릴리스 빌드하고 사용자 앱 실행 중이면 대표 EXE 교체를 보류한다.
- [x] 바뀐 파일 해시 manifest, 로그/이미지/EXE 정보를 main에게 전달하고 제품 쓰기·관련 실행을 중지한다. main이 고정 변경 독립 검토·가능한 격리 실측·커밋/푸시/별도 PR·실제 개발 기록을 마무리한다.
