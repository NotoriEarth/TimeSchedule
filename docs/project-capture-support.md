# 프로젝트 활동 수집 지원표 (M0)

시험일: 2026-11-18 (Asia/Seoul). 상태: **Claude Code CLI, Codex CLI 실행, Codex 데스크톱 앱은 관측 완료. Claude 데스크톱 앱은 미확인.**

이 문서는 이 PC에 설치된 도구가 훅으로 실제 무엇을 넘겨주는지 격리된 시험 폴더에서 관측한 결과다. `지원`은 표본 파일이 있는 항목, `문서상`은 공식 문서에만 있고 여기서 관측하지 않은 항목, `미확인`은 둘 다 아닌 항목이다. 확인 범위는 아래 버전·설정·시험 동작에 한정한다. 제품 수집 기능은 아직 없다. 계획은 [M0 하위 계획](superpowers/plans/2026-11-18-m0-capture-support.md)을 따른다.

## 1. 시험 환경

| 항목 | 값 | 확인 방법 |
| --- | --- | --- |
| 운영체제 | Windows 10 Pro 10.0.19045 | 시스템 정보 |
| Claude Code CLI | 2.1.325, 헤드리스 `claude -p`, 진입점 `sdk-cli`, 모델 `claude-haiku-5-5` | `claude --version`, 시험 세션 원문 로그의 `version`·`entrypoint` |
| Claude 로그인 | claude.ai 구독(API 키 아님) | `claude auth status` |
| Claude 데스크톱 앱 | 5.11492.5.0 | 설치 패키지 버전. 훅 관측은 미확인 |
| Codex 실행 파일 | `codex-cli 0.169.0-alpha.11.1`, Codex 앱에 포함된 파일. 모델 `gpt-6-luna`, 추론 `low`, ChatGPT 로그인 | `codex.exe --version`, 훅 입력의 `model` |
| Codex 데스크톱 앱 | 26.1111.2228.0. 위와 같은 실행 파일을 쓴다 | 설치 패키지 버전, 실행 중인 프로세스 경로 |
| Codex npm CLI | 0.169.1, 시험 시점에 실행 불가여서 관측하지 않음 | 2026-11-18 07:43 업데이트가 이전 `codex.exe`를 지우지 못해 Windows용 실행 파일이 빠짐 |
| 시험 폴더 | 저장소 밖 `C:\LSH\_capture-lab\m0-sample`, 합성 내용만 | 프로젝트 범위 훅만 설치 |

훅이 실행되기 위한 조건:

- Claude Code CLI는 시험 폴더의 `.claude/settings.json` 훅을 별도 승인 없이 실행했다.
- Codex는 신뢰하지 않은 폴더에서 `Untrusted project-local configuration is disabled` 경고를 내고 프로젝트 훅을 실행하지 않았다(기록 0건, 작업 자체는 완료). 사용자가 폴더를 신뢰하고 훅을 검토한 뒤에는 실행했다.

## 2. 지원표

### Claude Code CLI 2.1.325 (헤드리스)

| 시나리오 | 판정 | 관측한 이벤트 순서 | 근거 |
| --- | --- | --- | --- |
| 정상 완료 | 지원 | SessionStart(`startup`) → UserPromptSubmit → (PreToolUse → PostToolUse) 반복 → Stop → SessionEnd(`other`) | `tests/fixtures/project-activity/claude/s1-normal-cli/` |
| 세션 재개 | 지원 | SessionStart(`resume`)부터 같은 순서. `session_id`는 이전 세션과 같다 | `claude/s2-resume-cli/` |
| 도구 실패 | 지원 | PreToolUse → PostToolUseFailure(`error`, `is_interrupt=false`) → Stop → SessionEnd | `claude/s5-tool-failure-cli/` |
| 프로세스 강제 종료 | 부분 | SessionStart·UserPromptSubmit만 남는다. Stop과 SessionEnd는 오지 않았다(종료 후 10초 대기) | `claude/s6-kill-cli/` |
| 생성 출처 표시 | 지원 | 환경 변수 `TIMESCHEDULE_ORIGIN=generation`이 훅 프로세스까지 전달되어 8건 모두 제외 대상으로 표시됐다 | `claude/s7-generation-origin-cli/` |
| 사용자 중단(Esc) | 미확인 | 문서상 Stop이 실행되지 않는다. 헤드리스로는 재현할 수 없다 | 문서상 |
| 수동·자동 축약 | 미확인 | 문서상 PreCompact·PostCompact | 문서상 |
| API 오류 | 미확인 | 문서상 Stop 대신 StopFailure | 문서상 |

### Claude 데스크톱 앱

모든 시나리오 `미확인`. 간접 근거 하나: 이 작업을 수행한 데스크톱 세션에서 플러그인이 등록한 SessionStart 훅이 실행됐다. 프로젝트 범위 훅이 앱에서 같은 입력을 받는지는 관측하지 않았다.

### Codex CLI 실행 (`codex exec`, 앱 포함 실행 파일 0.169.0-alpha.11.1)

| 시나리오 | 판정 | 관측한 이벤트 순서 | 근거 |
| --- | --- | --- | --- |
| 신뢰 전 폴더 | 미지원 | 프로젝트 훅 0건 | 실행 기록(커밋하지 않음) |
| 정상 완료 | 지원 | SessionStart(`startup`) → UserPromptSubmit → PreModelRequest → PostModelRequest → PreToolUse → PostToolUse → PreModelRequest → PostModelRequest → Stop → SessionEnd(`other`) | `codex/s1-normal-cli/` |
| 세션 재개 | 지원 | SessionStart(`resume`)부터 같은 순서. `session_id`는 이전 세션과 같다 | `codex/s2-resume-cli/`, `codex/s2-resume-literal-cli/` |
| 도구 실패 | 부분 | 정상 완료와 같은 순서다. 실패 전용 이벤트나 필드가 없고 오류 문구가 PostToolUse의 `tool_response` 문자열에만 있다 | `codex/s5-tool-failure-cli/` |
| 프로세스 강제 종료(첫 모델 요청 직후) | 부분 | SessionStart → UserPromptSubmit → PreModelRequest 3건뿐. PostModelRequest·Stop·Interrupt·SessionEnd 없음 | `codex/s6-kill-after-request-cli/` |
| 프로세스 강제 종료(시작 직후) | 미지원 | 훅 0건 | 실행 기록(표본 없음) |
| 생성 출처 표시 | 지원 | 10건 모두 제외 대상으로 표시됐다 | `codex/s7-generation-origin-cli/` |

### Codex 데스크톱 앱 (같은 실행 파일, 한 대화)

| 시나리오 | 판정 | 관측 | 근거 |
| --- | --- | --- | --- |
| 정상 완료 | 지원 | SessionStart(`startup`) → PreModelRequest → PostModelRequest → PreToolUse → PostToolUse → PreModelRequest → PostModelRequest → Stop | `codex/s1-normal-app/`, `codex/s4-esc-attempt-completed-app/` |
| 프롬프트 수집 | 미확인 | 이 시험에서는 앱에 연결된 도구로 프롬프트를 전달했고 UserPromptSubmit이 오지 않았다. 사용자가 입력창에 직접 입력한 경우는 관측하지 않았다 | — |
| 다른 대화로 갔다가 돌아오기 | 지원(새 이벤트 없음) | 같은 `session_id`로 후속 두 턴이 이어졌다. 새 SessionStart는 오지 않았다. 앱 재시작은 시험하지 않았다 | `codex/s2-reentry-app/` |
| 수동 축약(`/compact`) | 지원 | PreCompact → PostCompact, 둘 다 `trigger=manual`. 다음 모델 요청 전에 SessionStart(`compact`)가 한 번 왔다 | `codex/s3-compact-app/`, `codex/s3-compact-sessionstart-app/` |
| 실행 중 사용자 중단 | 지원 | PreModelRequest → PostModelRequest → PreToolUse → Interrupt. Stop은 오지 않았다 | `codex/s4-interrupt-app/` |
| 세션 종료 | 미확인 | 앱 표본에는 SessionEnd가 없다. 대화를 닫거나 앱을 종료하는 시험은 하지 않았다 | — |
| 자동 축약, API 오류, 앱 강제 종료 | 미확인 | 유발하지 않았다 | 문서상 |

문서상 웹 검색 같은 호스팅 도구는 Codex 도구 훅에 잡히지 않으며, 세션이 끝날 때 끝나지 않은 백그라운드 훅은 취소된다.

## 3. 턴이 끝난 방식의 구분

세 경우는 훅에 남는 흔적이 다르다. `classifyTurns`가 이 구분을 계산한다.

| 끝난 방식 | 훅에 남는 것 | 분류 | 표본 |
| --- | --- | --- | --- |
| 정상 종료 | 그 턴에 Stop이 오고 `last_assistant_message`가 있다. 관측한 표본에서는 Stop이 그 턴의 마지막 훅이었다 | `completed` | Codex 앱 `s1-normal-app`, Codex CLI `s1-normal-cli`, Claude CLI `s1-normal-cli` |
| 사용자 중단 | Codex가 Interrupt를 보낸다. Stop은 없고, 진행 중이던 도구의 PostToolUse도 없다 | `interrupted` | Codex 앱 `s4-interrupt-app` |
| 프로세스 강제 종료 | 종료 신호에 해당하는 훅이 없다. 마지막으로 남은 훅이 UserPromptSubmit이나 PreModelRequest다 | `unterminated` | Codex CLI `s6-kill-after-request-cli`, Claude CLI `s6-kill-cli` |

- `unterminated`는 "분류에 쓸 수 있는 종료 훅을 찾지 못했다"는 뜻이다. 훅만으로는 강제 종료된 턴과 아직 실행 중인 턴을 구분할 수 없다. 세션이 끝났다는 별도 확인이 있어야 그 턴을 유실로 판정할 수 있다.
- 분류 규칙: 입력 순서와 무관하게 같은 결과를 낸다. 한 턴에 종료 훅이 여러 개면 관측 시각이 가장 늦은 것이 결정하고, Stop과 Interrupt의 시각이 같으면 완료로 단정하지 않고 `interrupted`로 둔다. 종료 훅 뒤에 다른 훅이 더 와도 판정은 바뀌지 않는다. 시각이 겹치는 경우와 종료 뒤 훅은 실제 표본에 없었고 합성 입력으로만 시험했다.
- 분류 대상: `normalizeEvent`와 같은 조건을 통과한 기록만 센다. 생성 출처 기록, 검증하지 않은 버전, 확인하지 않았거나 제외한 폴더의 기록은 턴으로 집계하지 않는다.
- Interrupt 입력에는 물리 키와 화면의 중단 버튼을 구분하는 필드가 없다.
- 사용자가 중단을 눌렀어도 턴이 먼저 끝나면 정상 종료와 같은 Stop이 온다(`s4-esc-attempt-completed-app`).
- Claude Code의 사용자 중단은 관측하지 않았다. Claude 어댑터에는 `interrupted`로 분류되는 이벤트가 없다.
- 데스크톱 앱의 강제 종료는 시험하지 않았다. CLI 강제 종료 결과를 앱에 그대로 적용하지 않는다.

## 4. 이벤트별 관측 필드와 변환

### Claude Code CLI 2.1.325

모든 이벤트에 `session_id`, `transcript_path`, `cwd`, `hook_event_name`이 있었다. SessionStart를 뺀 이벤트에는 `prompt_id`가 있었고, 한 턴의 훅은 같은 `prompt_id`를 가진다.

| 훅 이벤트 | 추가로 관측한 필드 | 어댑터 처리 | 봉투 `body` |
| --- | --- | --- | --- |
| SessionStart | `source`. 재개 시 `seconds_since_last_response`, `context_tokens`, `prompt_cache_likely_expired`, `estimated_cache_write_usd` | `session_start` | `{ source }` |
| UserPromptSubmit | `permission_mode`, `prompt` | `user_prompt` | `{ prompt }` |
| PreToolUse | `permission_mode`, `effort`, `tool_name`, `tool_input`, `tool_use_id` | 제외(`redundant_event`) | — |
| PostToolUse | PreToolUse의 필드 + `tool_response`, `duration_ms` | `tool_result` | `{ tool_name, tool_use_id, input_keys }` |
| PostToolUseFailure | PreToolUse의 필드 + `error`, `is_interrupt`, `duration_ms` | `tool_result` | `{ tool_name, tool_use_id, input_keys, failed: true }` |
| Stop | `permission_mode`, `effort`, `stop_hook_active`, `last_assistant_message`, `background_tasks`, `session_crons` | `turn_end` | `{ last_assistant_message }` |
| SessionEnd | `reason` | `session_end` | `{ reason }` |

StopFailure, PreCompact, PostCompact, Notification, SubagentStop, PermissionRequest는 훅을 걸어 두었지만 이번 시나리오에서 발생하지 않았다. 어댑터는 이들을 `unsupported`로 돌려준다.

### Codex 0.169.0-alpha.11.1 (CLI 실행과 앱에서 필드 구성이 같았다)

모든 이벤트에 `session_id`, `transcript_path`, `cwd`, `hook_event_name`이 있었다. SessionStart와 SessionEnd를 뺀 이벤트에는 `turn_id`가 있었고, SessionEnd를 뺀 이벤트에는 `model`이 있었다.

| 훅 이벤트 | 추가로 관측한 필드 | 어댑터 처리 | 봉투 `body` |
| --- | --- | --- | --- |
| SessionStart | `permission_mode`, `source`(`startup`, `resume`, `compact`) | `session_start` | `{ source, model }` |
| UserPromptSubmit (CLI만) | `permission_mode`, `prompt` | `user_prompt` | `{ prompt }` |
| PreModelRequest | 없음 | 제외(`not_an_activity`) | — |
| PostModelRequest | `status`(`completed`만 관측), `error`, `token_usage` | 제외(`not_an_activity`) | — |
| PreToolUse | `permission_mode`, `tool_name`, `tool_input`, `tool_use_id` | 제외(`redundant_event`) | — |
| PostToolUse | PreToolUse의 필드 + `tool_response`(문자열) | `tool_result` | `{ tool_name, tool_use_id, input_keys }` |
| Stop | `permission_mode`, `stop_hook_active`, `last_assistant_message` | `turn_end` | `{ last_assistant_message }` |
| Interrupt (앱만) | `permission_mode` | `interrupted` | `{}` |
| PreCompact (앱만) | `trigger` | 제외(`redundant_event`) | — |
| PostCompact (앱만) | `trigger` | `compaction` | `{ trigger }` |
| SessionEnd (CLI만) | `reason` | `session_end` | `{ reason }` |

- PostModelRequest의 `token_usage`에는 `input_tokens`, `cached_input_tokens`, `cache_write_input_tokens`, `output_tokens`, `reasoning_output_tokens`, `total_tokens`가 있었다. 이것은 개발 세션의 사용량이며 기록 기능의 비용이 아니므로 활동으로 만들지 않는다. 구독 실제 청구액은 측정하지 않았다.
- PermissionRequest, SubagentStart, SubagentStop은 훅을 걸어 두었지만 발생하지 않았다. PostModelRequest의 `failed`·`cancelled` 상태도 관측하지 않았다.

### 두 도구 공통

- PreToolUse를 제외하는 근거: 정상 흐름에서는 항상 뒤에 결과 이벤트가 왔다. 예외는 Codex 사용자 중단으로, PreToolUse 뒤에 결과 없이 Interrupt가 왔다. 훅 입력의 PreToolUse에는 도구 이름이 있지만 어댑터가 이 이벤트를 제외하므로, 중단 당시 실행 중이던 도구는 활동 봉투에 남지 않는다.
- 도구 입력·출력의 본문은 봉투에 넣지 않는다. 얼마나 발췌할지는 미확정 항목이다.
- 한국어: Claude CLI는 한국어 프롬프트와 응답이 훅 입력에서 원문 그대로였다. Codex는 한국어 응답(앱)을 확인했다. Codex CLI의 프롬프트 표본 6건은 모두 영어여서 Codex의 한국어 프롬프트 보존은 미확인이다.
- 판정은 닫힌 쪽으로 한다. 출처가 `tool`이 아니거나 알 수 없는 값이면 변환하지 않는다. 등록 결정이 `new`나 `link`가 아니면(없음, 미정 포함) 경로 후보만 내고 본문은 내지 않는다.
- 봉투의 `event_id`는 관측 스크립트가 기록을 저장할 때 한 번 발급한 값이다. 저장된 기록을 다시 변환하거나 재전송하면 같은 값이 유지된다. 훅이 다시 실행되면 새 이벤트이며 새 값을 받는다.
- 커밋한 표본의 세션·턴 ID는 실제 값을 치환한 것이다. 도구별로 모든 시나리오에 같은 치환표를 써서, 같은 실제 세션은 같은 표시를 갖고 다른 세션은 다른 표시를 갖는다. 재개 시나리오가 직전 세션과 같은 `session_id`를 갖는다는 점을 표본에서 확인할 수 있다.

## 5. 훅이 주지 않는 것과 원문 로그

| 없는 정보 | 영향 | 얻을 수 있는 다른 곳 |
| --- | --- | --- |
| 클라이언트 버전 | 어댑터가 검증한 버전인지 훅 입력만으로 판정할 수 없다 | Claude CLI는 `claude --version`과 원문 로그의 `version`. Codex CLI 실행에서는 훅 환경에 `CODEX_VERSION` 변수가 있었다(값은 기록하지 않음). Codex 앱 훅 환경에는 그 변수가 없었다 |
| 이벤트 발생 시각 | 봉투의 `occurred_at`은 훅이 실행된 시각이다(`time_basis: hook_capture`) | 두 도구 모두 원문 로그 레코드에 `timestamp`가 있다 |
| 이벤트 고유 ID | 재전송 식별용 ID를 관측 스크립트가 한 번 발급한다 | 도구 이벤트에는 `tool_use_id`가 있다 |
| 모델 이름 | Claude 훅 입력에는 없다 | Codex 훅 입력에는 `model`이 있다 |
| 토큰 사용량 | Claude 훅으로는 계측할 수 없다 | Codex는 PostModelRequest에 있다. Claude는 원문 로그의 `cost-state`, 헤드리스 결과 JSON |
| 실행 표면(CLI·앱) | 훅 입력만으로는 구분되지 않는다 | 훅 환경 변수 이름의 차이(Claude `CLAUDE_CODE_ENTRYPOINT` 값, Codex CLI의 `CODEX_SESSION_ID` 등) |

원문 로그(시험 세션에 한해 레코드 종류와 키 이름만 집계했다):

- Claude: `%USERPROFILE%\.claude\projects\<경로를 바꿔 쓴 이름>\<session_id>.jsonl`. 시험 세션 5개가 각각 217,767~352,558바이트였다. 레코드 종류는 `user`, `assistant`, `attachment`, `system`, `api-request` 계열, `cost-state` 등 13가지였다. 강제 종료한 세션의 파일도 남아 있었다.
- Codex: `%USERPROFILE%\.codex\sessions\<연>\<월>\<일>\rollout-….jsonl`. 시험 세션 5개가 280,717~722,817바이트였다. 레코드는 `timestamp`, `ordinal`, `type`, `payload`를 가지며 종류는 `session_meta`, `turn_context`, `response_item`, `event_msg`, `token_usage_record` 등이었다. 강제 종료한 세션의 파일도 남아 있었다.
- Codex 전체 보관 상태: 같은 폴더에 파일 902개(`.jsonl` 279개, `.zst` 623개), 합계 2.7 GB다. 개수와 크기만 확인했다. 오래된 세션이 `.zst`로 압축되어 있으므로 로그 복구 경로는 이 형식을 읽거나 미지원으로 표시해야 한다.
- 형식의 안정성: Codex 문서는 원문 로그 형식이 안정된 인터페이스가 아니라고 밝힌다. Claude 문서에는 관련 문장을 찾지 못했다.

## 6. 실측값

작은 시험 폴더의 값이며 긴 실제 세션을 대표하지 않는다. 관측 스크립트 실행 시간은 스크립트 프로세스가 시작한 뒤 파일을 쓰기 직전까지이고, 도구가 훅 프로세스를 띄우는 시간은 포함하지 않는다.

| 항목 | 최소 | 중앙 | 최대 | 표본 수 |
| --- | --- | --- | --- | --- |
| 관측 스크립트 실행 시간(ms), Claude CLI | 28 | 29 | 36 | 36 |
| 관측 스크립트 실행 시간(ms), Codex CLI·앱 | 28 | 30 | 44 | 89 |
| 훅 입력 크기(바이트), Claude CLI 전체 | 278 | 506 | 1,400 | 36 |
| 훅 입력 크기(바이트), Codex 전체 | 283 | 481 | 1,318 | 89 |

중앙값은 표본 수가 짝수이면 가운데 두 값의 평균이다.

이벤트별 훅 입력 크기(바이트, 최소/중앙/최대, 표본 수):

| 이벤트 | Claude CLI | Codex |
| --- | --- | --- |
| SessionStart | 278 / 278 / 404 (6) | 336 / 336 / 342 (8) |
| UserPromptSubmit | 399 / 403 / 423 (6) | 457 / 487.5 / 636 (6) |
| PreModelRequest | — | 341 / 341 / 348 (20) |
| PostModelRequest | — | 534 / 538 / 546 (19) |
| PreToolUse | 506 / 518 / 579 (7) | 478 / 528 / 711 (10) |
| PostToolUse | 650 / 703.5 / 889 (6) | 542 / 600 / 1,318 (9) |
| PostToolUseFailure | 1,400 (1) | — |
| Stop | 567 / 647 / 1,356 (5) | 413 / 514 / 1,301 (9) |
| Interrupt | — | 363 (1) |
| PreCompact / PostCompact | — | 355 (1) / 356 (1) |
| SessionEnd | 325 / 325 / 325 (5) | 283 / 283 / 290 (5) |

Claude 표본 36건에는 강제 종료 첫 시도(엉뚱한 프로세스를 종료해 세션이 정상 완료된 실행)의 4건이 들어 있다. 그 4건은 fixture로 쓰지 않았다.

훅 입력은 수백 바이트지만 원문 로그는 세션당 수십만 바이트였다. 원문 보관 기간·압축 결정에 쓸 자료이며 여기서 정책을 정하지 않는다.

## 7. 재현 방법

1. 저장소 밖에 시험 폴더를 만들고 `git init` 한다. `.gitignore`에 `.capture/`를 적는다.
2. `scripts/activity-adapters/probe-hook.mjs`를 `<시험 폴더>\.capture\`에 복사한다.
3. `<시험 폴더>\.claude\settings.json`과 `<시험 폴더>\.codex\hooks.json`에 이벤트마다 아래 명령을 건다. 두 파일의 구조는 같다. Codex는 `--source codex`, 출력 폴더 `raw/codex/_inbox`, 버전 표시는 `codex-cli 0.169.0-alpha.11.1 app-bundled`를 쓴다.

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

4. Codex는 앱에서 시험 폴더를 신뢰하고 훅 검토를 승인한다. 승인 전에는 훅이 실행되지 않는다.
5. Claude: 시험 폴더에서 `claude -p "<프롬프트>" --model claude-haiku-5-5 --max-turns 3 --permission-mode acceptEdits --output-format json`. 다른 Claude Code 세션 안에서 실행한다면 이름이 `CLAUDE`, `ANTHROPIC`으로 시작하는 환경 변수를 지우고 실행한다.
6. Codex: `codex exec -m gpt-6-luna -c model_reasoning_effort="low" --sandbox workspace-write -C <시험 폴더> --json "<프롬프트>"`. 앱 시나리오는 같은 폴더의 대화에서 수행한다.
7. 시나리오가 끝날 때마다 `_inbox`를 시나리오 이름으로 바꾼다.
8. `node scripts/activity-adapters/sanitize-fixture.mjs --in <원본 폴더> --out tests/fixtures/project-activity/<도구>/<시나리오> --home %USERPROFILE% --lab <시험 폴더> --ids <시험 폴더>\.capture\id-map-<도구>.json`으로 표본을 만든다. `--ids` 파일은 실제 ID와 치환 표시의 대응을 시나리오 사이에 유지한다. 원본과 이 파일은 커밋하지 않는다.

강제 종료를 재현할 때 주의: Git Bash의 `/proc/<pid>/winpid`는 래퍼 프로세스를 가리킨다. 그것을 종료하면 `claude.exe`는 살아남아 세션이 정상 완료된다. PowerShell `Start-Process -PassThru`로 얻은 PID를 `taskkill /T /F`로 종료해야 한다.

시험을 마친 뒤 시험 폴더의 훅을 없애려면 `<시험 폴더>\.claude\settings.json`과 `<시험 폴더>\.codex\hooks.json`을 지운다.

## 8. 자동으로 얻는 것과 에이전트 기록으로 보충할 것

| 훅으로 자동 수집 가능(관측) | 훅으로는 얻지 못함 |
| --- | --- |
| 세션 시작·재개·축약·종료 | 강제 종료된 턴의 결과 |
| 사용자 프롬프트 원문(Claude CLI, Codex CLI) | 판단 이유와 검토한 대안(마지막 응답 문장에 들어 있을 때만 간접적으로 남는다) |
| 도구 이름과 호출 사실. Claude는 성공·실패와 소요 시간까지 | Codex 도구의 성공·실패 여부 |
| 턴이 끝날 때의 마지막 응답, Codex의 사용자 중단 | 검증 범위와 미검증 범위에 대한 명시적 진술, 여러 턴에 걸친 문제 해결의 연결 |

따라서 결정 이유·실패 원인·검증 범위는 작업 에이전트가 남기는 짧은 구조화 기록(체크포인트)으로 보충해야 한다. 훅은 "무엇이 실행됐는가"를 주고 "왜 그렇게 했는가"는 주지 않는다.

## 9. 한계와 다음 작업에 넘기는 조건

- **버전 일치 정책:** 어댑터는 검증한 버전 문자열과 정확히 일치할 때만 변환한다. 두 도구 모두 자동 업데이트되므로 이 규칙대로면 업데이트 때마다 수집이 `unsupported`로 멈춘다. 필드 형태 검사를 통과하면 `미검증 버전`으로 표시해 받는 방식 등은 작업 3 전에 결정해야 한다.
- **버전의 출처:** 시험에서는 훅 명령에 버전 문자열을 직접 적었다. 실제 수집에서 클라이언트 버전을 어디서 읽을지 정해야 한다.
- **종료 훅이 없는 턴:** 훅만으로는 강제 종료와 진행 중을 구분할 수 없다. 원문 로그 재수집을 지원하거나 `확인 불가`로 표시해야 한다.
- **Codex 도구 실패:** 훅에 실패 표시가 없다. 실패 여부가 필요하면 출력 문구 해석이나 원문 로그가 필요하며, 그 전에는 "실행됨"까지만 말할 수 있다.
- **Codex 앱의 프롬프트:** 입력창 직접 입력에서 UserPromptSubmit이 오는지 확인해야 한다. 오지 않으면 앱에서는 프롬프트 없이 도구·응답만 수집된다.
- **등록 경로 대조:** 어댑터는 파일 시스템을 조회하지 않고 문자열만 비교한다. 대소문자, 구분자, `.`·`..`, 끝의 점·공백, `\\?\` 접두사처럼 문자열로 정리할 수 있는 표기 차이는 같은 폴더로 본다. junction·symlink·worktree의 실제 경로와 8.3 짧은 이름은 구분하지 못하므로 작업 2에서 등록 전에 해석해야 한다. 절대 경로가 아닌 `cwd`는 변환하지 않는다.
- **필드 누락:** 검증한 이벤트에서 기대한 필드(예: Stop의 `last_assistant_message`)가 빠져도 어댑터는 빈 값으로 활동을 만든다. 누락을 따로 표시하지 않는다.
- **표본에 남은 식별자:** 도구 호출 ID(`toolu_…`, `call_…`)는 치환하지 않았다. 인증 정보는 아니다.
- **Claude 데스크톱 앱, Codex npm CLI, MCP 경로:** 관측하지 않았다.
