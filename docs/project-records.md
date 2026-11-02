# 작업 종료 기록과 Space 연결

작업이 끝날 때 **다이어리에 상세 원문 저장 → Space 반영 → 같은 다이어리 항목에 검증된 링크 연결** 순서로 처리한다. Space가 끊겨도 원문은 다이어리에 남고, 앱이 켜져 있을 때 누적된 기록만 재시도한다. 연결 후에도 원문을 지우지 않는다. Space 내용은 PR 작성에 도움이 될 때 선택적으로 재사용하며 PR이 없어도 기록한다.

현재 대상은 [TimeSchedule 개발 포트폴리오](https://chatgpt.com/space/page_6ae1b18a44f08191ad2a86276972476c)와 이 스크립트가 있는 저장소다. 다른 채팅·프로젝트를 자동 탐색하지 않는다.

## 언제 실행하는가

Codex는 `AGENTS.md`에 따라 의미 있는 작업을 마칠 때 기존 설명을 기록한다. 별도 오전 9시 포트폴리오 자동화는 사용자 요청으로 중지했다. 앱은 이미 준비된 대기 기록만 전달하며 새 내용을 작성하지 않는다.

최초 전달에서 Node·스크립트·상태 폴더 경로를 앱에 등록한다. 앱은 시작 후 약 1분 간격으로 작은 대기 상태 파일을 확인한다. 전송할 기록이 있을 때만 숨겨진 전달 프로세스를 하나 실행하고 한 번에 한 작업을 처리한다. 실패하면 1분→5분→15분→1시간 간격으로 늘린다. 앱을 닫거나 PC를 끄면 다음 앱 실행 후 재개한다. 접속 복구를 즉시 감지하는 서버 알림은 아니며 다음 재시도 시각에 반영한다.

Node.js, 로그인된 Codex CLI와 Pages 연결이 외부 전달에 필요하다. 기록용 모델은 GPT-6 Luna medium이다. 앱의 기본 일정·다이어리 사용에는 Node.js가 필요 없다. API 키를 파일에 저장하지 않으며 CLI 인증·이용 한도는 기존 계정을 따른다.

## 작업 전달

아래 JSON을 `.cache/`에 저장한다. 같은 작업의 후속 수정은 같은 workId, 새로운 작업은 새 UUID를 사용한다. 날짜는 작업의 한국 날짜다.

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

`kind`: development / troubleshooting / portfolio / summary. `status`: in_progress / verification_pending / verified / completed / merged. `verified`는 기록에 명시한 검증 범위이며 사용자 승인·병합을 의미하지 않는다. 요약은 상세 원문의 완전한 문단을 선택하고, 생략하면 첫 일반 문단을 쓴다. 제한·미확인을 잘라 성과만 남기지 않는다. 상세 원문은 UTF-8 48,000바이트 이하로 나눈다.

```powershell
node scripts/project-records.mjs prepare --input .cache/project-record-input.json
node scripts/project-records.mjs sync
node scripts/project-records.mjs status
```

`prepare`만 하면 전달 자료 준비에 그치므로 작업 종료 시 반드시 `sync`까지 실행한다. `sync --work-id UUID`는 한 작업, `--retry-now`는 대기 시간을 생략한 명시적 재시도다. 앱은 `--max-jobs 1`로 한 번에 하나만 처리한다. 전송 중에도 앱의 편집과 inbox 수신은 독립적으로 진행된다.

테스트에서는 `--state-dir DIR --data-dir DIR --exe FILE`로 격리한다. 별도 CLI 설치는 `--codex-exe FILE` 또는 `TIMESCHEDULE_CODEX_EXE`로 지정한다. 인증정보와 본문을 셸 명령 문자열에 끼워 넣지 않는다.

## 상태와 보존

| 상태 | 의미 |
|---|---|
| prepared | 원문 준비. 아직 다이어리 저장 확인 전이다. |
| diary_queued | 앱 수신 큐에 접수됐다. 편집·저장이 끝난 안전한 시점에 반영된다. |
| waiting_space | 다이어리에 원문이 저장됐고 Space 연결을 기다린다. |
| publishing | 게시 중이거나 생성 결과 불확실. 불확실한 경우 기존 페이지만 확인한다. |
| waiting_prior | 같은 작업의 앞선 수정본 처리를 기다린다. |
| space_saved | Space를 확인했으며 다이어리 링크 연결을 기다린다. |
| complete | Space 검증과 다이어리 링크 연결까지 확인했다. |
| conflict | 사용자 편집·삭제 등 충돌을 보존하며 자동 처리를 중단했다. |

같은 작업의 새 수정이 대기 중 들어오면 앞선 상태를 `pending-<revision>.json`으로 보존하고 순서대로 처리한다. 다른 작업은 하나의 실패 때문에 막히지 않는다. 로컬 저장과 링크 연결은 각각 증가하는 전달 revision을 사용한다. 이미 다이어리에 더 최신 수정본이 있으면 이전 전달이 덮어쓰지 않는다.

앱이 닫혀 있으면 CLI가 동일 코어로 저장하고, 열려 있으면 `record-inbox`에 접수한다. 앱은 시작 시 및 약 60초 간격으로 처리하되 편집·다른 저장 중에는 미룬다. 결과는 `record-receipts`, 손상된 입력은 `record-failed`에 보관한다. Windows 결과 파일이 신뢰 가능한 확인 수단이며 콘솔 출력은 보이지 않을 수 있다.

Space 제목·전체 본문·위치·URL·부모 링크를 실제 Pages 읽기 결과로 검사한다. 모델의 완료 선언만으로 성공 처리하지 않는다. 원문 차이가 있으면 전달을 보류한다. 연결 실패 전에 게시를 시작하지 않았다는 실행 결과가 확인되면 정상 재시도한다. 생성 여부가 불확실하면 새 페이지로 우회하지 않는다. 사용자 편집·삭제는 자동 덮어쓰기·복구하지 않는다.

전달 상태·원문·로그는 Git에 넣지 않는 `.cache/project-records`에 보관한다. 다이어리와 별도로 재시도·매핑에 필요한 파일이므로 대기 작업이 있을 때 지우지 않는다. Windows named pipe 잠금으로 동시 실행을 막고 앱 소유 프로세스는 실행 시간 제한과 종료 시 정리를 적용한다. 원문 로그를 외부 공유하거나 인증정보·개인 일정·메모를 넣지 않는다.
