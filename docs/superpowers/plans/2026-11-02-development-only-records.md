# Development-only records Implementation Plan

**Goal:** Git에 실제 개발 변경이 있는 작업만 다이어리와 Space에 전달한다.
**Architecture:** prepare에서 커밋 범위와 작업별 경로를 검증하고 sync에서도 재검증한다. 제외 후보는 외부 호출 없이 종료한다. 기존 완료/충돌 자료는 유지한다.
**Tech Stack:** Node.js built-ins, Git CLI, 기존 Rust/Iced 실행 파일.
**Spec:** ../specs/2026-11-02-development-only-records-design.md

## Constraints and decisions

- 시작 기준: e01d727, 작업 브랜치: codex/development-only-records. 사용자 미커밋 변경 없음.
- 사용자 요청의 실제 변경 조건을 적용한다. 현재 세션에서 구현하고 마지막 독립 리뷰 한 번을 수행한다.
- Git 근거는 gitChanges {base, head, paths}. 전체 SHA와 작업에 속한 상대 경로만 받는다. 조사/문서만으로는 통과하지 않는다.
- 기존 기록의 삭제 및 강제 덮어쓰기 없음. 이미 게시 여부가 불확실한 기록도 원문과 상태를 보존한다.

## Review focus

- 깨끗한 HEAD에 커밋된 개발 변경도 기록한다.
- 탐색 중 다른 작업의 미커밋 변경은 기록 근거가 아니다.
- 문서뿐인 커밋, 빈 범위, 잘못된/무관한 경로·커밋은 외부 호출하지 않는다.
- 준비 후 변경된 상태 파일과 과거 근거 없는 대기 기록도 sync에서 차단한다.
- 제외된 이전 revision 때문에 같은 작업의 유효한 후속 revision이 막히지 않는다.

## Tasks

- [x] 실제 임시 Git 저장소 기반 실패 테스트 작성: 무변경/문서만/개발 변경/잘못된 근거, 저장·모델 호출 0회, 기존 대기 기록 보존.
- [x] scripts/project-records-git.mjs와 prepare/sync의 검증 구현. 기존 전달 회귀 테스트에 실제 Git fixture 연결.
- [ ] AGENTS.md, 전달 안내와 버전 0.8.1 갱신. Node 테스트, Rust 버전 빌드와 EXE 준비.
- [ ] 독립 리뷰, 필요한 수정, 커밋·푸시·PR 연결. 실제 개발 변경인 이번 작업만 새 규칙으로 기록.

## Progress

- 브랜치 생성 완료. 외부 게시 없이 로컬 테스트부터 진행한다.
- 새 Git 조건 테스트의 실패를 확인하고 구현 후 통과했다. 제외된 불확실 게시를 다시 준비할 때 recovery-only를 유지하는 추가 실패도 재현 후 수정했다.
- Node 전체 29개 통과. native 67개 통과/수동 미리보기 1개 제외. release 0.8.1 컴파일 완료. 실행 중인 사용자 앱 때문에 대표 EXE 교체만 보류됐다.
