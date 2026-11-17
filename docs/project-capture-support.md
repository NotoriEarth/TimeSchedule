# 프로젝트 활동 수집 지원표 (M0)

시험일: 2026-11-18 (Asia/Seoul). 상태: **Claude Code CLI는 관측 완료, Codex와 두 데스크톱 앱은 사용자 조치 대기.**

이 문서는 이 PC에 설치된 도구가 훅으로 실제 무엇을 넘겨주는지 격리된 시험 폴더에서 관측한 결과다. `지원`은 표본 파일이 있는 항목, `문서상`은 공식 문서에만 있고 여기서 관측하지 않은 항목, `미확인`은 둘 다 아닌 항목이다. 제품 수집 기능은 아직 없다. 계획은 [M0 하위 계획](superpowers/plans/2026-11-18-m0-capture-support.md)을 따른다.

## 1. 시험 환경

| 항목 | 값 | 확인 방법 |
| --- | --- | --- |
| 운영체제 | Windows 10 Pro 10.0.19045 | 시스템 정보 |
| Claude Code CLI | 2.1.325, 헤드리스 `claude -p`, 진입점 `sdk-cli` | `claude --version`, 시험 세션 원문 로그의 `version`·`entrypoint` |
| Claude 로그인 | claude.ai 구독(API 키 아님) | `claude auth status` |
| Claude 데스크톱 앱 | 5.11492.5.0 | 설치 패키지 버전. 훅 관측은 미확인 |
| Codex 실행 파일 | `codex-cli 0.169.0-alpha.11.1` (Codex 앱에 포함된 파일) | `codex.exe --version` |
| Codex npm CLI | 0.169.1, **실행 불가** | 2026-11-18 07:43 업데이트가 이전 `codex.exe`를 지우지 못해 Windows용 실행 파일이 빠짐 |
| Codex 앱 | 26.1111.2228.0 | 설치 패키지 버전. 훅 관측은 미확인 |
| 시험 폴더 | 저장소 밖 `C:\LSH\_capture-lab\m0-sample`, 합성 내용만 | 프로젝트 범위 훅만 설치. 전역 설정 변경 없음 |

훅 승인 상태:

- Claude Code CLI는 시험 폴더의 `.claude/settings.json` 훅을 별도 승인 절차 없이 실행했다.
- Codex는 신뢰하지 않은 폴더에서 `Untrusted project-local configuration is disabled` 경고를 내고 프로젝트 훅을 실행하지 않았다. 작업 자체는 끝까지 수행했고 훅 기록은 0건이었다.

## 2. 지원표

### Claude Code CLI 2.1.325 (헤드리스)

| 시나리오 | 판정 | 관측한 이벤트 순서 | 근거 |
| --- | --- | --- | --- |
| 정상 완료 | 지원 | SessionStart(`startup`) → UserPromptSubmit → (PreToolUse → PostToolUse) 반복 → Stop → SessionEnd(`other`) | `tests/fixtures/project-activity/claude/s1-normal-cli/` |
| 세션 재개 | 지원 | SessionStart(`resume`)부터 같은 순서. `session_id`는 이전 세션과 같다 | `s2-resume-cli/` |
| 도구 실패 | 지원 | PreToolUse → PostToolUseFailure(`error`, `is_interrupt=false`) → Stop → SessionEnd | `s5-tool-failure-cli/` |
| 프로세스 강제 종료 | 부분 | 종료 전의 SessionStart·UserPromptSubmit만 남는다. Stop과 SessionEnd는 오지 않았다(종료 후 10초 대기) | `s6-kill-cli/` |
| 생성 출처 표시 | 지원 | 실행 환경 변수 `TIMESCHEDULE_ORIGIN=generation`이 훅 프로세스까지 전달되어 8건 모두 제외 대상으로 표시됐다 | `s7-generation-origin-cli/` |
| 사용자 중단(Esc) | 미확인 | 문서상 Stop이 실행되지 않는다. 헤드리스로는 재현할 수 없다 | 문서상 |
| 수동 축약(`/compact`) | 미확인 | 문서상 PreCompact·PostCompact(`manual`) | 문서상 |
| 자동 축약 | 미확인 | 문서상 PreCompact·PostCompact(`auto`). 긴 세션이 필요해 유발하지 않았다 | 문서상 |
| API 오류 | 미확인 | 문서상 Stop 대신 StopFailure. 유발하지 않았다 | 문서상 |

강제 종료가 `부분`인 이유: 훅만 보면 그 턴이 끝났는지 실패했는지 알 수 없다. 종료된 세션의 원문 로그 파일은 남아 있었다(4절).

### Claude 데스크톱 앱

모든 시나리오 `미확인`. 간접 근거 하나: 이 작업을 수행한 데스크톱 세션에서 플러그인이 등록한 SessionStart 훅이 실행됐다. 프로젝트 범위 훅이 앱에서 같은 입력을 받는지는 관측하지 않았다.

### Codex (CLI 실행과 앱)

| 시나리오 | 판정 | 관측 | 근거 |
| --- | --- | --- | --- |
| 신뢰 전 폴더 | 미지원 | 프로젝트 훅 0건. 경고만 출력 | 실행 기록(커밋하지 않음) |
| 신뢰·훅 검토 후 전 시나리오 | 미확인 | 사용자가 Codex에서 시험 폴더를 신뢰하고 훅을 검토해야 한다 | — |

문서상 Codex에는 사용자 중단용 `Interrupt` 이벤트가 따로 있고, 웹 검색 같은 호스팅 도구는 도구 훅에 잡히지 않으며, 세션이 끝날 때 끝나지 않은 백그라운드 훅은 취소된다.

## 3. 이벤트별 관측 필드와 변환 (Claude Code CLI 2.1.325)

모든 이벤트에 `session_id`, `transcript_path`, `cwd`, `hook_event_name`이 있었다. SessionStart를 뺀 이벤트에는 `prompt_id`가 있었다.

| 훅 이벤트 | 추가로 관측한 필드 | 어댑터 처리 | 봉투 `body` |
| --- | --- | --- | --- |
| SessionStart | `source`. 재개 시 `seconds_since_last_response`, `context_tokens`, `prompt_cache_likely_expired`, `estimated_cache_write_usd` | `session_start` | `{ source }` |
| UserPromptSubmit | `permission_mode`, `prompt` | `user_prompt` | `{ prompt }` |
| PreToolUse | `permission_mode`, `effort`, `tool_name`, `tool_input`, `tool_use_id` | 제외(`redundant_event`) | — |
| PostToolUse | PreToolUse의 필드 + `tool_response`, `duration_ms` | `tool_result` | `{ tool_name, tool_use_id, input_keys }` |
| PostToolUseFailure | PreToolUse의 필드 + `error`, `is_interrupt`, `duration_ms` | `tool_result` | `{ tool_name, tool_use_id, input_keys, failed: true }` |
| Stop | `permission_mode`, `effort`, `stop_hook_active`, `last_assistant_message`, `background_tasks`, `session_crons` | `turn_end` | `{ last_assistant_message }` |
| SessionEnd | `reason` | `session_end` | `{ reason }` |

- PreToolUse를 제외하는 근거: 관측한 7건 모두 뒤에 PostToolUse 또는 PostToolUseFailure가 왔다. 강제 종료처럼 뒤따르는 이벤트가 없는 경우는 관측하지 못했다.
- 도구 입력·출력의 본문은 봉투에 넣지 않는다. 얼마나 발췌할지는 미확정 항목이다.
- 한국어 프롬프트는 훅 입력에서 원문 그대로였다.
- 표에 없는 이벤트(StopFailure, PreCompact, PostCompact, Notification, SubagentStop, PermissionRequest)는 훅을 걸어 두었지만 이번 시나리오에서 발생하지 않았다. 어댑터는 이들을 `unsupported`로 돌려준다.

## 4. 훅이 주지 않는 것과 원문 로그

훅 입력에 없던 것:

| 없는 정보 | 영향 | 얻을 수 있는 다른 곳 |
| --- | --- | --- |
| 클라이언트 버전 | 어댑터가 검증한 버전인지 훅 입력만으로 판정할 수 없다 | CLI는 `claude --version`, 원문 로그의 `version`. 데스크톱 앱은 미확인 |
| 이벤트 발생 시각 | 봉투의 `occurred_at`은 훅이 실행된 시각이다(`time_basis: hook_capture`) | 원문 로그의 `timestamp` |
| 이벤트 고유 ID | 재전송 식별용 ID를 관측 스크립트가 한 번 발급한다 | 도구 이벤트에는 `tool_use_id`가 있다 |
| 토큰 사용량·비용 | 훅으로는 기록 비용을 계측할 수 없다 | 원문 로그의 `cost-state` 레코드, 헤드리스 실행의 결과 JSON |

원문 로그(시험 세션에 한해 레코드 종류와 키 이름만 집계했다):

- Claude: `%USERPROFILE%\.claude\projects\<경로를 바꿔 쓴 이름>\<session_id>.jsonl`. 도구를 한두 번 쓴 시험 세션 5개가 각각 217,767~352,558바이트였다. 레코드 종류는 `user`, `assistant`, `attachment`, `system`, `api-request` 계열, `cost-state` 등 13가지였다. 강제 종료한 세션의 파일도 남아 있었다.
- Codex: `%USERPROFILE%\.codex\sessions`에 파일 902개(`.jsonl` 279개, `.zst` 623개), 합계 2.7 GB다. 개수와 크기만 확인했고 내용은 읽지 않았다. 오래된 세션이 `.zst`로 압축되어 있으므로 로그 복구 경로는 이 형식을 읽거나 미지원으로 표시해야 한다.
- 형식의 안정성: Codex 문서는 원문 로그 형식이 안정된 인터페이스가 아니라고 밝힌다. Claude 문서에는 관련 문장을 찾지 못했다.

## 5. 실측값

Claude Code CLI 헤드리스 표본 36건(강제 종료 재시도 전의 실패한 시도 포함) 기준이다. 작은 시험 폴더의 값이며 긴 실제 세션을 대표하지 않는다.

| 항목 | 최소 | 중앙 | 최대 | 표본 수 |
| --- | --- | --- | --- | --- |
| 관측 스크립트 실행 시간(ms) | 28 | 29 | 36 | 36 |
| 훅 입력 크기(바이트), 전체 | 278 | 506 | 1,400 | 36 |
| SessionStart | 278 | 278 | 404 | 6 |
| UserPromptSubmit | 399 | 406 | 423 | 6 |
| PreToolUse | 506 | 518 | 579 | 7 |
| PostToolUse | 650 | 717 | 889 | 6 |
| PostToolUseFailure | 1,400 | 1,400 | 1,400 | 1 |
| Stop | 567 | 647 | 1,356 | 5 |
| SessionEnd | 325 | 325 | 325 | 5 |

- 실행 시간은 스크립트 프로세스가 시작한 뒤 파일을 쓰기 직전까지다. 도구가 훅 프로세스를 띄우는 시간은 포함하지 않는다.
- 훅 입력은 작지만 원문 로그는 세션당 수십만 바이트였다. 원문 보관 기간·압축 결정에 쓸 자료이며 여기서 정책을 정하지 않는다.

## 6. 재현 방법

1. 저장소 밖에 시험 폴더를 만들고 `git init` 한다. `.gitignore`에 `.capture/`를 적는다.
2. `scripts/activity-adapters/probe-hook.mjs`를 `<시험 폴더>\.capture\`에 복사한다.
3. `<시험 폴더>\.claude\settings.json`과 `<시험 폴더>\.codex\hooks.json`에 이벤트마다 아래 명령을 건다. 두 파일의 구조는 같다.

```json
{
  "hooks": {
    "Stop": [
      { "hooks": [ { "type": "command", "timeout": 10,
        "command": "node \"<시험 폴더>/.capture/probe-hook.mjs\" --source claude-code --out \"<시험 폴더>/.capture/raw/claude/_inbox\" --tool-version \"claude-code-cli 2.1.325\"" } ] }
    ]
  }
}
```

4. 시험 폴더에서 `claude -p "<프롬프트>" --model claude-haiku-5-5 --max-turns 3 --permission-mode acceptEdits --output-format json`을 실행한다. 다른 Claude Code 세션 안에서 실행한다면 이름이 `CLAUDE`, `ANTHROPIC`으로 시작하는 환경 변수를 지우고 실행한다.
5. 시나리오가 끝날 때마다 `_inbox`를 시나리오 이름으로 바꾼다.
6. `node scripts/activity-adapters/sanitize-fixture.mjs --in <원본 폴더> --out tests/fixtures/project-activity/<도구>/<시나리오> --home %USERPROFILE% --lab <시험 폴더>`로 표본을 만든다. 원본은 커밋하지 않는다.

강제 종료를 재현할 때 주의: Git Bash의 `/proc/<pid>/winpid`는 래퍼 프로세스를 가리킨다. 그것을 종료하면 `claude.exe`는 살아남아 세션이 정상 완료된다. PowerShell `Start-Process -PassThru`로 얻은 PID를 `taskkill /T /F`로 종료해야 한다.

## 7. 자동으로 얻는 것과 에이전트 기록으로 보충할 것

| 훅으로 자동 수집 가능(Claude CLI 관측) | 훅으로는 얻지 못함 |
| --- | --- |
| 세션 시작·재개·종료 | 강제 종료되거나 중단된 턴의 결과 |
| 사용자 프롬프트 원문 | 판단 이유와 검토한 대안(마지막 응답 문장에 들어 있을 때만 간접적으로 남는다) |
| 도구 이름, 성공·실패, 소요 시간 | 검증 범위와 미검증 범위에 대한 명시적 진술 |
| 턴이 끝날 때의 마지막 응답 | 여러 턴에 걸친 문제 해결의 연결 |

따라서 결정 이유·실패 원인·검증 범위는 작업 에이전트가 남기는 짧은 구조화 기록(체크포인트)으로 보충해야 한다. 훅은 "무엇이 실행됐는가"를 주고 "왜 그렇게 했는가"는 주지 않는다.

## 8. 한계와 다음 작업에 넘기는 조건

- **버전 일치 정책:** 어댑터는 검증한 버전 문자열과 정확히 일치할 때만 변환한다. Claude Code는 자동 업데이트되므로 이 규칙대로면 업데이트 때마다 수집이 `unsupported`로 멈춘다. 필드 형태 검사를 통과하면 `미검증 버전`으로 표시해 받는 방식 등은 작업 3 전에 결정해야 한다.
- **버전의 출처:** 실제 수집에서 클라이언트 버전을 어디서 읽을지 정해야 한다. 훅 입력에는 없다.
- **강제 종료 구간:** 훅만으로는 복구할 수 없다. 원문 로그 재수집을 지원하거나 `확인 불가`로 표시해야 한다.
- **등록 경로 대조:** 어댑터는 문자열만 비교한다. junction·worktree의 실제 경로 해석은 작업 2의 책임이다.
- **Codex와 데스크톱 앱:** 관측 전이다. Codex 어댑터에는 검증한 버전이 없어 모든 Codex 기록이 `unsupported`다.
- **MCP 경로:** 시험하지 않았다.
