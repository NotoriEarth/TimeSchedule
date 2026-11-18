# 작업 1(M0) 인계 자료 — 훅 수집 지원표와 어댑터 계약

작성일: 2026-11-18. 대상: Codex Astra 검토와 개념 설명, 사용자 확인.

## 승인 범위와 상태

- 승인 범위: [M0 하위 계획](../plans/2026-11-18-m0-capture-support.md)의 1A~1D. 작업 2(Rust·DB)는 범위 밖이며 시작하지 않았다.
- 저장소: `NotoriEarth/TimeSchedule`, 브랜치 `codex/m0-capture-support`, PR [#29](https://github.com/NotoriEarth/TimeSchedule/pull/29) (base `codex/project-document-requirements`, PR #28 위에 쌓임).
- BASE: `4cb7fd15259d1716fc8f0f1a0cacd887c6cfa691`
- 코드 HEAD: `3c9b69b`. 이 문서를 고친 커밋은 그 뒤에 오며 코드를 바꾸지 않는다.
- **Astra 검토 상태: 통과하지 못했다.** 검토한 네 번(1·2·4·5차) 모두 중요 지적 6건이 나왔다. 5차 뒤 사용자 결정으로 등록 경로 대조를 작업 2로 넘기고 나머지 지적을 고쳤다(`3c9b69b`). **이 마지막 변경은 Astra가 검토하지 않았다.** 아래 "Astra 검토" 참고.
- 머지하지 않았다. 사용자 확인과 머지 승인이 필요하다.

## 커밋과 변경 경로

| 커밋 | 내용 | 주요 경로 |
| --- | --- | --- |
| `71f5b0b` | 관측 스크립트, 표본 치환기, 하위 계획 | `scripts/activity-adapters/probe-hook.mjs`, `sanitize-fixture.mjs`, `tests/activity-probe.test.mjs` |
| `b5dc44b` | Claude 표본, 프로브의 버전 변수 허용 목록 | `tests/fixtures/project-activity/claude/` |
| `6d9cdc3` | 어댑터 계약과 Claude 어댑터 | `scripts/activity-adapters/contract.mjs`, `claude.mjs`, `codex.mjs`, `tests/activity-adapters.test.mjs` |
| `a0a30b9` | 지원표 초안 | `docs/project-capture-support.md` |
| `14f0ad5` | Codex 표본과 어댑터, 턴 종료 구분 | `tests/fixtures/project-activity/codex/`, `contract.mjs`의 `classifyTurns` |
| `c53903f` | 지원표 완성 | `docs/project-capture-support.md` |
| `79f583a` | Astra 1차 지적 수정 | 어댑터·프로브·치환기·시험·표본·문서 |
| `4b5a758` | Astra 2차 지적 수정 | `contract.mjs`, `sanitize-fixture.mjs`, 시험, 문서 |
| `0a2fa1d` | 이 인계 자료 | 문서만 |
| `b38b5c1` | Astra 4차 지적 수정(경로 비교 방식 변경) | `contract.mjs`, `sanitize-fixture.mjs`, 시험, 문서 |
| `3c9b69b` | 등록 경로 대조를 작업 2로 이관, Astra 5차의 나머지 지적 수정. **미검토** | 어댑터 3개, `sanitize-fixture.mjs`, 시험, 지원표, 계획 2개 |

표본은 17개 시나리오의 관측 기록 121건과 `expected.json` 17개다. 제품 코드(`crates/`, `apps/`), 버전, EXE는 바꾸지 않았다.

## 파일·함수별 역할과 설계 이유

### `scripts/activity-adapters/probe-hook.mjs`

훅 명령으로 실행되어 표준 입력을 CaptureRecord 파일 한 개로 저장하는 시험 전용 스크립트다.

- `readBounded(stream, limit)`: 입력을 끝까지 읽되 1 MiB를 넘으면 버퍼를 버리고 길이만 센다. 끝까지 읽는 이유는 훅을 호출한 도구가 쓰기에서 막히지 않게 하기 위해서다.
- `main()`: 관측 ID(`capture_id`)와 관측 시각을 한 번 발급하고, 임시 파일에 쓴 뒤 이름을 바꾼다.
- 표준 출력을 쓰지 않는다. 두 도구 모두 훅의 표준 출력을 대화에 넣을 수 있다. 종료 코드 2를 쓰지 않는다. 두 도구에서 차단 신호다.
- 환경 변수는 `CLAUDE`·`CODEX`로 시작하는 이름만 기록하고, 값은 표면·버전을 알려 주는 5개만 기록한다.
- 고려한 대안: 훅에서 곧바로 변환·필터까지 하는 방식. 관측 단계에서는 원본 형태를 그대로 봐야 해서 택하지 않았다. 제품 수집 경로는 작업 3에서 따로 만든다.

### `scripts/activity-adapters/sanitize-fixture.mjs`

- `sanitizeCapture(record, { homeDir, labRoot, sessionIds })`: 홈 폴더, 시험 폴더, 세션 ID, UUID 형태의 값을 고정 표시로 바꾼다. 치환은 제어 문자 토큰으로 먼저 바꾸고 마지막에 펼친다. 뒤 규칙이 앞 규칙의 결과를 다시 바꾸지 않게 하기 위해서다.
- 경로와 계정 이름은 값에서만 바꾸고 필드 이름은 건드리지 않는다. 계정 이름은 홈 경로와 `Users` 뒤의 프로필 폴더 이름일 때만 바꾼다. 계정이 `user`·`tool`·`session`이어도 `UserPromptSubmit`, `origin: "tool"`, `session_id`가 깨지지 않는다.
- 본문에 계정 이름이 단어로 남으면 아무것도 쓰지 않고 오류로 끝난다. 원본이 어느 계정에서 왔는지는 치환할 때만 알 수 있어서, 나중에 도는 시험이 아니라 이 명령이 검사한다.
- 세션 ID는 온전한 토큰일 때만 바꾸고(`tool`이라는 ID가 `PostToolUse`를 바꾸지 않는다), 객체의 키가 ID 그 자체이면 키도 바꾼다. 대소문자만 다른 두 ID는 서로 다른 표시를 받는다.
- `--ids`: 실제 ID와 표시의 대응표를 실행 사이에 유지한다. 같은 실제 세션은 모든 시나리오에서 같은 표시를 갖는다.
- `swapScenario(out, outputs)`: 새 표본 세트를 옆 폴더에 다 만든 뒤 폴더째로 바꾼다. 새 폴더를 옮기지 못하면 옛 폴더를 되돌린다. 입력이 비어 있으면 아무것도 바꾸지 않고 오류로 끝난다.

### `scripts/activity-adapters/contract.mjs`

- 폴더가 어느 프로젝트에 속하는지는 이 모듈이 판단하지 않는다. 호출자가 `context.registrationFor(cwd)`로 알려 준다. `folderDecision`은 그 답을 `confirmed`·`excluded`·`unconfirmed` 셋 중 하나로만 읽는다.
  - 설계 이유: 처음에는 등록 경로와 `cwd`를 문자열로 대조했다(`comparable`, 이어서 `parsePath`와 엄격·느슨 두 비교). 검토 네 번이 매번 다른 틈을 찾았다. 표기만 다른 같은 폴더를 놓치면 제외가 우회되고, 다른 폴더를 같게 보면 미등록 폴더가 등록을 상속한다. 5차에서는 두 지적이 서로 반대 방향이었다(Windows가 같다고 보는 이름을 놓침 / 대소문자를 구분하는 디렉터리에서 다른 폴더를 같게 봄). 어느 쪽이 맞는지는 파일 시스템이 정하므로 문자열만 보는 함수로는 풀 수 없다고 보고, 사용자 결정으로 작업 2로 넘겼다. 조건은 지원표 9절과 구현 계획의 작업 2에 적었다.
  - 고려한 대안: 확정은 글자 그대로 일치할 때만, 제외는 글자 단위로 넓게 보는 방식으로 한 번 더 고치는 안. 작업 2가 어차피 junction·worktree 때문에 파일 시스템으로 경로를 다뤄야 하므로 택하지 않았다.
- `hasObservedShape`: 검증한 이벤트라도 표본에서 본 문자열 필드(각 어댑터의 `requires`)와, 턴에 속한 이벤트라면 턴 식별자가 있어야 한다. 없으면 `malformed`다.
- `normalizeCapture(record, context, adapter)`: 형식 → 생성 출처 → 버전 → 이벤트 → 제외 이벤트 → 등록 경로 순으로 판정한다. 앞 단계에서 걸리면 뒤를 보지 않으며, 호출자에게 폴더를 묻는 것은 마지막이다. 본문은 출처가 `tool`이고 버전·이벤트·필드가 검증됐고 호출자가 폴더를 `new`·`link`와 프로젝트 ID로 답했을 때만 나온다.
- `classifyTurns(records, context, adapter)`: 턴마다 `completed`(Stop), `interrupted`(Codex Interrupt), `unterminated`(종료 훅 없음)를 계산한다. 강제 종료는 이벤트가 없어 `normalizeCapture`의 결과로 표현할 수 없기 때문에 별도 함수로 두었다. 입력 순서와 무관하다.
- 고려한 대안: 강제 종료를 합성 이벤트로 만들어 활동에 넣는 방식. 훅만으로는 강제 종료와 실행 중을 구분할 수 없어 사실이 아닌 사건을 만들게 되므로 택하지 않았다.

### `scripts/activity-adapters/claude.mjs`, `codex.mjs`

검증한 버전 문자열, 턴 식별 키(Claude `prompt_id`, Codex `turn_id`), 이벤트 표를 선언한다. 표에 없는 이벤트는 `unsupported`다. 시험이 "표본 없는 이벤트·버전은 표에 둘 수 없다"를 강제한다.

## 시험과 결과

| 명령 | 결과 |
| --- | --- |
| `node --test tests/*.test.mjs` (HEAD `3c9b69b`) | 150개 중 149개 통과, 0개 실패, 1개는 기존 건너뜀 |
| `git diff --check` | 통과 |
| Rust 시험·빌드 | 실행하지 않음(Rust 변경 없음) |

새 시험 81개(`activity-probe` 24개, `activity-adapters` 57개)는 구현 전에 실패를 확인했다. 경로 대조를 덜어내면서 그 시험들을 지워 4차 때(93개)보다 줄었다. 예외는 표본을 검사하는 방어용 시험들로, 표본이 올바르면 처음부터 통과하므로 표본을 일부러 오염시켜 실패하는 것을 확인했다(개인정보 표식, 프로필 폴더 이름, 계정 이름).

## 확인한 동작과 미확인 동작

확인(표본 있음): [지원표](../../project-capture-support.md) 2절의 `지원`·`부분` 행. 요약하면 Claude Code CLI 2.1.325의 정상·재개·도구 실패·강제 종료·생성 출처, Codex 0.169.0-alpha.11.1의 CLI 실행 6개 시나리오와 데스크톱 앱 4개 시나리오다.

미확인: Claude 데스크톱 앱 전체, Claude의 사용자 중단·축약·API 오류, Codex npm CLI, Codex 앱의 입력창 직접 입력 프롬프트·세션 종료·자동 축약·앱 강제 종료, MCP 경로.

실행하지 않은 검증: 실제 동시 훅 실행, 디스크 쓰기 실패, 긴 실제 세션에서의 지연, 다른 Windows 계정에서의 치환.

## Astra 검토

실행: Codex 앱에 포함된 `codex.exe`(0.169.0-alpha.11.1)로 `exec -m gpt-6.2-astra --sandbox read-only`. 실행 로그 머리말은 다섯 번 모두 `model: gpt-6.2-astra`, `reasoning effort: high`였다. 원문은 로컬 `.cache/reviews/m0-round-{1,2,3,4,5}.md`에 있다(커밋하지 않음).

| 회차 | 대상 HEAD | 결과 | 처리 |
| --- | --- | --- | --- |
| 1 | `c53903f` | Important 6, Minor 6 | Important 6건과 Minor 5건을 시험 먼저 추가해 수정(`79f583a`). Minor 3건 중 필드 누락 표시·도구 호출 ID·측정 구간은 문서에 한계로 기재 |
| 2 | `79f583a` | 1차 Important 5건 해결, 1건 부분 해결 확인. 새 Important 6, Minor 4 | 10건 모두 시험 먼저 추가해 수정(`4b5a758`) |
| 3 | `4b5a758` | **검토하지 않음.** 답변이 "실제 모델 식별자 확인 불가"였고, 요청문의 "Astra가 아니면 중단" 조건에 따라 코드 검토 전에 멈췄다 | 승인된 3회를 모두 써서 멈추고 사용자에게 보고했다 |
| 4 (사용자가 추가 1회 승인) | `0a2fa1d` (코드는 `4b5a758`) | 2차 수정 중 경로 2건이 다시 열림. 새 Important 6, Minor 4 | 10건 모두 시험 먼저 추가해 수정(`b38b5c1`) |
| 5 (사용자가 1회 더 승인) | `6273080` (코드는 `b38b5c1`) | Important 6, Minor 3. 커밋된 표본의 실제 개인정보 노출은 없다고 확인 | **수정하지 않음.** 아래 "5차 지적과 열린 결정" 참고 |

1차 주요 지적: 제외 폴더가 경로 표기(`.\private` 등)로 우회됨, 출처·등록 결정이 불명확해도 본문 반환, `classifyTurns`에 생성 출처 제외 없음, 종료 판정이 입력 순서에 의존, 표본의 세션 ID가 모두 같은 표시, 프로브가 상한과 무관하게 전체 입력을 보관.

2차 주요 지적: `\\?\` 확장 경로에서 `repo.`와 `repo`가 같게 비교됨, `D:`를 절대 경로로 인정, 같은 폴더의 `new`·`exclude` 등록이 순서에 따라 결정됨, 문서의 `%USERPROFILE%`가 PowerShell에서 펼쳐지지 않아 홈 경로가 남을 수 있음, 계정 이름 `user`가 이벤트 이름을 손상, 대응표를 읽기 전에 기존 표본 삭제.

4차 주요 지적: 일반 경로에서 이름 끝의 점·공백을 지워 `repo...\sub`가 `repo`의 등록을 상속, 유니코드 소문자화가 서로 다른 이름(`İ`와 `i`+결합점)을 합침, `\\?\D:\\`의 빈 이름이 통과, 계정 이름이 `session`·`tool`이면 치환기가 필드 이름과 값을 바꿔 세션 ID가 남거나 표본이 깨짐, 두 글자 계정 이름이 남음, 빈 입력으로 기존 표본을 지우고 성공.

4차 수정의 방향: 경로 지적 세 건은 개별로 막지 않고 비교 방식을 바꿨다(위 `parsePath` 설계 이유). 이 변경은 검토에서 제안된 것이 아니라 구현자의 판단이며 검토받지 않았다.

### 5차 지적과 처리

5차 지적 중 어댑터의 세 건은 모두 등록 경로 대조에 관한 것이었다: 제외를 찾는 넓은 비교가 Windows가 같다고 보는 이름 한 쌍을 놓침, 대소문자를 구분하는 디렉터리에서 `Repo`와 `repo`를 같게 봄, 드라이브·UNC 루트 뒤의 중복 구분자가 통과. 앞의 두 건은 요구가 서로 반대 방향이라 파일 시스템 없이 동시에 만족할 수 없다.

사용자 결정(2026-11-18): 등록 경로 대조를 M0에서 덜어내고 작업 2에서 파일 시스템으로 다시 설계한다. 이에 따라 `3c9b69b`에서 경로 비교 코드를 모두 지웠고, 세 지적은 해당 코드가 없어져 이 PR에서는 더 발생하지 않는다. 작업 2가 지켜야 할 조건은 지원표 9절에 적었다.

같은 커밋에서 고친 나머지 5차 지적: 다른 계정에서 얻은 원본의 계정 이름 검사(치환기가 직접 검사), 객체 키로 쓰인 세션 ID, 교체 도중 실패 시 기존 표본 손실(폴더째 교체와 되돌리기), 불투명 세션 ID가 이벤트 이름을 바꾸거나 대소문자만 다른 ID가 합쳐짐, 100번째 이후 표본이 시험에서 빠짐, 필수 필드가 없어도 활동이 됨.

남은 일: `3c9b69b`에 대한 독립 검토. 지금까지 검토한 네 번 모두 중요 지적이 나왔으므로 이 변경에도 문제가 있을 수 있다. 요청문은 로컬 `.cache/reviews/m0-astra-request.md`에 있다.

## 사용한 모델과 관측 사용량

| 용도 | 모델·실행 | 횟수 / 상한 | 관측 사용량 |
| --- | --- | --- | --- |
| 구현·문서 | `claude-fable-5-2`, Claude 데스크톱 세션 | 1개 세션 | 세션 단위 토큰·비용은 확인 불가. 종료 시점의 대화 문맥 약 53만 토큰 |
| Claude 시나리오 | `claude-haiku-5-5`, `claude -p` | 6 / 8 | 완료된 5회 합계 약 $0.024(CLI가 보고한 가격표 기준 값, 구독 실제 청구 아님). 강제 종료한 1회는 확인 불가 |
| Codex CLI 시나리오 | `gpt-6-luna` low, `codex exec` | 8 / 8 (Claude 1회 + Codex 7회) | 훅에 잡힌 모델 요청 10건 합계 입력 194,119(캐시 148,608) / 출력 446 토큰. 신뢰 전 실행 1회는 입력 38,607 / 출력 95. 강제 종료 실행은 확인 불가 |
| Codex 앱 시나리오 | `gpt-6-luna` low, Codex가 앱에서 수행 | 앱 프롬프트 5개 안팎 | 집계하지 않음 |
| Astra 검토 | `gpt-6.2-astra` high | 5 / 5 (승인 3 + 사용자 추가 승인 2) | CLI 보고 79,511 + 104,742 + 21,058 + 112,388 + 98,525 토큰. 구독 실제 청구는 확인 불가 |

## Rust 학습용 근거 (이번 작업에는 Rust 변경이 없다)

이번 단계의 학습 주제는 "프로그램·프로세스·파일·DB의 차이"다. 설명에 쓸 수 있는 실제 근거:

- **프로세스:** Claude Code와 Codex는 훅마다 `node probe-hook.mjs`를 별도 프로세스로 띄운다. 그 프로세스는 표준 입력으로 JSON을 받아 파일 한 개를 쓰고 끝난다. 프로세스가 시작한 뒤 파일을 쓰기 직전까지가 28~44ms였다(`probe_elapsed_ms`). 파일을 쓰는 시간과 도구가 프로세스를 띄우는 시간은 이 값에 들어 있지 않다. 부모 프로세스를 강제 종료하면 종료 훅 프로세스는 아예 생기지 않는다(`claude/s6-kill-cli`, `codex/s6-kill-after-request-cli`).
- **파일:** 프로브는 임시 파일에 쓰고 이름을 바꾼다(`probe-hook.mjs`의 `main`). 쓰다가 죽어도 반쯤 쓰인 파일이 정식 이름으로 남지 않는다. 이는 메모리 안전성과 무관한 디스크 저장 절차다.
- **순수 함수:** `contract.mjs`의 함수들은 파일도 시계도 쓰지 않는다. 같은 입력에 같은 결과를 내므로 표본 파일만으로 동작을 고정할 수 있다.
- **DB:** 이번 작업에는 DB가 없다. 관측 기록을 SQLite에 넣는 것은 작업 2·3이며, 그때 `Storage::connection`의 잠금과 트랜잭션이 등장한다.

## 사용자 확인 방법

1. `node --test tests/activity-adapters.test.mjs tests/activity-probe.test.mjs`를 실행해 통과를 확인한다.
2. [지원표](../../project-capture-support.md) 2절과 3절이 실제 사용 방식과 맞는지 읽는다. 특히 "Codex 앱에서 프롬프트가 수집되지 않을 수 있다", "Codex 훅에는 도구 실패 표시가 없다"가 받아들일 수 있는 한계인지 판단한다.
3. 표본 하나를 열어 본다. 예: `tests/fixtures/project-activity/codex/s4-interrupt-app/04-Interrupt.json`.

시험 폴더 `C:\LSH\_capture-lab\m0-sample`에는 프로젝트 범위 훅이 남아 있다. 그 폴더에서 Claude Code나 Codex를 쓰면 계속 기록된다. 없애려면 그 폴더의 `.claude\settings.json`과 `.codex\hooks.json`을 지운다.
