---
name: development-diary
description: TimeSchedule에서 개발 변경을 커밋할 때 작업의 문제와 해결, 실제 테스트 결과를 개인 문서와 다이어리에 한 건으로 저장한다. 새 채팅에서 이전 작업의 누락이나 전달 대기를 복구할 때도 사용한다.
---

# 개발 작업의 일지

저장소 루트에서 `node scripts/development-diary.mjs audit`으로 이 작성자의 이전 작업을 확인한다. 개발을 시작할 때 작업 브랜치에서 `node scripts/development-diary.mjs begin`을 실행한다. 같은 작업 브랜치를 이어갈 때는 기존 ID를 쓴다. 각 개발 커밋에 `git commit --trailer "Diary-Work-Id: <UUID>"`를 붙인다. 사용자가 기록을 따로 언급하지 않아도 커밋과 검증을 마칠 때 아래 절차를 실행한다.

1. 버그를 재현한 Node 회귀 검사는 수정 전후에 `node scripts/development-diary.mjs check <UUID> tests/파일.test.mjs`로 실행한다. 파일을 여러 개 지정할 수 있다. 즉석에서 확인한 입력과 기대값이 수정의 근거라면 이름 있는 테스트로 옮긴다. 다른 종류의 검증은 실제 명령, 종료 결과, 검사한 범위를 적는다.
2. 무시되는 `.cache/`에 UTF-8 JSON으로 `title`, `summary`, 선택적인 `detail`, `tests`(완료 커밋에서 재실행할 Node 테스트 파일 배열)를 쓴다. 문제, 시도, 선택 이유, 남은 점을 당시 확인한 만큼 적는다. Rust·앱 수동 검사처럼 별도로 수행한 결과는 `verification`에 담고 확인 수준에 맞는 `status`를 정한다. 로그 전문과 개인 데이터는 제외한다.
3. `node scripts/development-diary.mjs finish <UUID> <JSON경로>`를 실행한다. 작성자와 커밋의 작업 ID로 귀속을 확인하고, 지정한 테스트를 해당 Git 소스에서 다시 실행한 뒤 기존 문서 저장 및 다이어리 수신 명령을 호출한다. 문서와 다이어리 본문에 전후 집계가 들어간다. 출력의 `local_complete` 또는 `complete`와 `entryId`를 확인한다. 대기 중이면 같은 ID로 `retry`한다.

## 새 채팅에서의 복구

- 진행 중인 같은 작업을 이어갈 때는 기존 ID로 작업을 마친 뒤 한 번 전달한다. 이전 작업의 `missing_with_note`는 `finish <UUID>`로 저장해 둔 설명과 테스트를 사용한다. `missing`은 소유 커밋과 남은 검사 영수증을 확인하고 복원 가능한 사실로 원문을 만든다. 선택 이유가 남아 있지 않으면 그 한계를 쓴다.
- 같은 ID의 후속 개발 커밋이 `missing`이면 이전 설명이나 직전 전달본을 읽어 첫 작업의 문제, 해결, 당시 검증 범위를 보존한다. 직전 기록 이후부터 `ownedHead`까지 내 작업 ID가 붙은 커밋 본문과 diff를 해당 작업의 검사 영수증과 대조한다. `summary`나 `detail`에 후속 변경이 필요했던 이유, 바꾼 내용, 실제 검사와 결과를 새 문단으로 덧붙인다. `tests`에 완료 커밋에서 다시 실행할 검사를 지정하고 `finish <UUID> <NOTE.json>`으로 같은 항목을 갱신한다. 확인할 수 없는 이유나 검증 결과는 그 한계를 적는다.
- `delivery_ready`, `prepared`, `diary_queued` 등 전달 대기는 `retry <UUID>`로 보관한 입력을 다시 보낸다. `conflict`는 기존 사용자 편집을 확인한다. `started_before_id`는 시작 시점만 확인된 작업이다. ID 없는 `unassigned`와 작성자가 섞인 `ambiguous`는 검토 후보로만 알린다.
- Markdown 수정, 조사, 기존 테스트만 수행한 대화에는 개발 일지를 발행하지 않는다. 보관된 자료와 수신 상태의 의미는 `docs/project-records.md`를 따른다.
