# Space 개발 기록과 다이어리 연결

Space 상세 기록 → 다이어리 원문 요약·링크 순서로 저장한다. 개발자가 PR을 작성할 때 도움이 되는 내용만 Space에서 재사용할 수 있다. PR 없는 작업도 기록한다.

현재 연결 대상은 [TimeSchedule 개발 포트폴리오](https://chatgpt.com/space/page_6ae1b18a44f08191ad2a86276972476c)이며 저장소 경로는 이 스크립트가 있는 TimeSchedule 저장소로 제한한다. 다른 프로젝트를 자동 수집하지 않는다.

## 자동 실행 범위

이 저장소에서 작업하는 Codex가 `AGENTS.md`의 종료 절차로 전달한다. 기존 오전 9시 Codex 자동화는 누락을 보충한다. 모든 프로그램 종료나 Git 커밋을 감시하는 운영체제 서비스는 아니다. Codex의 예약 실행 환경이 사용 가능해야 예약 작업이 실행된다.

기록 작성에는 로그인된 Codex CLI와 연결된 Pages, Node.js가 필요하다. 다이어리 수신은 0.8.0 이상의 `artifacts/TimeSchedule.exe`가 처리한다. 앱 자체 실행에는 Node.js가 필요 없다. 별도의 API 키를 파일에 저장하지 않으며 CLI 이용 한도·인증은 기존 계정을 따른다.

## 전달 자료

아래 JSON을 `.cache/` 안에 저장한다. 동일한 작업을 갱신할 때 `workId`를 유지한다. `revision`은 내용 변경에 따라 자동 증가한다. 날짜는 작업의 한국 날짜다.

```json
{
  "workId": "96ebef77-f456-4d3b-b82d-30a92b7da0b5",
  "projectTitle": "TimeSchedule",
  "repoPath": "C:\\LSH\\TimeSchedule",
  "date": "2026-10-31",
  "kind": "development",
  "title": "작업 제목",
  "status": "verification_pending",
  "detailedMarkdown": "확인한 결과와 아직 확인하지 않은 범위를 함께 쓴다.\n\n## 해결 과정\n\n실제 시도와 근거를 기록한다.",
  "diarySummary": "확인한 결과와 아직 확인하지 않은 범위를 함께 쓴다."
}
```

`kind`: development / troubleshooting / portfolio / summary. `status`: in_progress / verification_pending / verified / completed / merged. `verified`는 기록에 명시한 검증 범위에만 해당하며 사용자 승인이나 병합을 의미하지 않는다.

요약은 상세 원문의 완전한 문단을 그대로 선택한다. 생략하면 첫 일반 문단을 사용한다. 제한·미확인 내용을 문단 중간에서 잘라 성과만 남기지 않는다. 상세 원문은 UTF-8 48,000바이트 이하여야 한다. 지나치게 큰 작업은 실제 작업 단위로 나눈다.

```powershell
node scripts/project-records.mjs prepare --input .cache/project-record-input.json
node scripts/project-records.mjs sync
node scripts/project-records.mjs status
```

`sync --work-id UUID`로 한 작업만 처리할 수 있다. 테스트에서는 `--state-dir DIR --data-dir DIR --exe FILE`로 전달 상태·앱 데이터를 격리한다. 별도 CLI 설치는 `--codex-exe FILE` 또는 `TIMESCHEDULE_CODEX_EXE`로 지정한다. 셸 프롬프트에 본문을 끼워 넣지 않고 파일·표준입력으로 전달한다.

## 저장 확인과 재시도

기록용 `codex exec`는 GPT-6 Luna medium을 명시하고 주어진 원문을 저장한다. 모델의 완료 선언만 믿지 않고 실제 Pages `read_page` 실행 결과의 제목·전체 본문·부모 위치·URL과 부모 페이지의 링크를 검사한다. 한 실행은 최대 3분, 출력은 8MiB로 제한한다. 변경 없는 기록이나 다이어리만 재시도할 때는 모델을 호출하지 않는다.

| 상태 | 의미와 후속 조치 |
|---|---|
| prepared | 원문 준비 완료. sync가 Space 저장을 시작한다. |
| publishing | 저장 시도 중이거나 결과 불확실. 재시도는 기존 페이지 확인만 하며 새 페이지 생성은 금지한다. |
| space_saved | Space를 확인했다. EXE·권한 문제를 해결하고 sync하면 다이어리만 재시도한다. |
| diary_queued | 앱에 접수됐다. 편집창을 저장·닫으면 앱이 안전한 시점에 처리한다. 다음 sync가 수신 결과를 확인한다. |
| complete | Space 검증과 다이어리 수신 결과를 모두 확인했다. |
| conflict | 사용자 편집·삭제 또는 버전 충돌을 보존하며 중단했다. 자동 덮어쓰기를 하지 않는다. |

불확실한 생성에서 일치하는 페이지를 찾을 수 없으면 생성 여부·제목 변경·접근 권한을 사람이 확인해야 한다. `.cache/project-records`를 지우거나 새 workId로 우회하지 않는다. 충돌은 Space와 다이어리의 사용자 편집을 먼저 확인하고, 합의된 원문을 같은 workId의 다음 revision으로 준비한다. 다이어리에서 직접 수정·삭제한 기록은 다음 revision에서도 자동 복구하지 않는다. 의도한 재생성은 별도의 명시적 결정이 필요하다.

앱이 닫혀 있으면 CLI가 동일 코어를 통해 저장한다. 열려 있으면 `record-inbox`에 원자적으로 접수하고 앱이 시작 시 및 약 60초 간격으로 처리한다. 편집·다른 저장 작업 중에는 미룬다. 결과는 `record-receipts`, 손상된 입력은 `record-failed`에 보존한다. 사용자 DB나 실행 중인 앱을 외부에서 강제 종료하지 않는다.

Windows GUI 실행 파일은 콘솔 출력이 보이지 않을 수 있으므로 직접 사용할 때도 결과 파일을 지정한다.

```powershell
Start-Process .\artifacts\TimeSchedule.exe -ArgumentList 'record status --work-id 96ebef77-f456-4d3b-b82d-30a92b7da0b5 --revision 1 --result .cache\receipt.json' -WindowStyle Hidden -Wait
Get-Content .cache/receipt.json
```

전달 상태·원문·실행 로그는 Git에 포함하지 않는 `.cache/project-records`에 보관한다. 로컬 로그에는 기록 본문이 포함되므로 일반 개발 로그처럼 취급하고 외부 공유하지 않는다. 이 상태를 삭제하면 기존 페이지와의 연결·재시도 정보가 사라진다.
