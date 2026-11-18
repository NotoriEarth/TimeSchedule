# M0 설치 환경·수집 계약 확인 하위 계획 (작업 1)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

> **사용자 지정 실행 경계:** 이 문서는 [전체 계획](2026-11-17-project-document-implementation.md)의 **작업 1만** 다룬다. 사용자가 이 하위 계획을 승인하기 전에는 아무것도 실행하지 않는다. 승인 후에도 연속 실행은 이 문서의 1A~1D에만 적용하며, 끝나면 결과를 제출하고 멈춘다. 작업 2(Rust·DB)는 시작하지 않는다.

**Goal:** 이 PC에 설치된 Claude Code와 Codex가 훅으로 실제 무엇을 넘겨주는지 격리된 시험 폴더에서 관측하고, 관측한 것만 변환하는 어댑터 계약과 지원표를 만든다.

**Architecture:** 시험 폴더에만 프로젝트 범위 훅을 두고, 훅이 받은 입력을 그대로 파일로 남기는 관측 스크립트(probe)를 실행한다. 관측 원본은 저장소 밖에 두고, 개인 경로·세션 ID를 치환한 표본만 fixture로 커밋한다. 어댑터는 파일·네트워크를 쓰지 않는 순수 함수이며 fixture로 동작을 고정한다.

**Tech Stack:** Node.js 24 ESM(`node:test`, `node:assert/strict`), 추가 의존성 없음. Rust·SQLite·앱 UI 변경 없음.

**Spec:** [요구사항 v0.2](../../TimeSchedule_Product_Requirements_v0.2_2026-11-17.md)의 FR-01·FR-04·FR-07·NFR-02·NFR-05·NFR-06과 12절 M0, [검토안 v0.2](../../TimeSchedule_Requirements_Review_v0.2_2026-11-17.md) 2절, [전체 계획](2026-11-17-project-document-implementation.md) 작업 1, [운영 지침](../../claude-codex-stage-workflow.md).

작성일: 2026-11-18. 조사 기준 HEAD: `4cb7fd15259d1716fc8f0f1a0cacd887c6cfa691`(코드는 `main` `ad679cd`와 동일, 네이티브 앱 `0.18.0`, schema `6`).

> **실행 중 바뀐 것(2026-11-18).** 아래 본문은 승인 당시의 계획이다. 실제 결과는 [인계 자료](../handoffs/2026-11-18-m0-capture-support.md)를 따른다.
> - 등록 경로 대조(`resolveRegistration`, `NormalizeContext.registrations`)는 사용자 결정으로 작업 2로 넘겼다. 어댑터는 `context.registrationFor(cwd)`로 폴더 결정을 받는다.
> - 턴 종료 구분용 `classifyTurns`와 제외 사유 `not_an_activity`를 추가했다.
> - Codex 표본은 npm CLI가 아니라 Codex 앱에 포함된 실행 파일로 얻었고, Codex 시나리오는 Codex가 수행했다.
> - 검증한 이벤트는 표본에서 본 필드를 갖춰야 하며, 아니면 `malformed`다.

## Global Constraints

- 전역 설정(`~/.claude/settings.json`, `~/.codex/config.toml`, `~/.codex/hooks.json`)을 수정하지 않는다. 훅은 시험 폴더의 프로젝트 범위 파일에만 둔다.
- 실제 프로젝트(`C:\LSH\TimeSchedule` 포함)에는 훅을 설치하지 않는다. 실제 프로젝트의 대화 로그·`auth.json`·토큰을 읽거나 출력하지 않는다.
- 원문 로그는 시험 세션의 `transcript_path` 파일에 한해, 레코드 종류와 키 이름만 집계한다. 값은 출력하지 않는다.
- 관측 원본(`.capture/raw`)은 저장소에 커밋하지 않는다. 커밋하는 fixture에는 실제 사용자 폴더명·세션 ID·이메일·토큰 형태 문자열이 없어야 한다.
- 확인하지 않은 경로의 이벤트는 경로 후보만 내고 본문을 내지 않는다. `origin=timeschedule_generation`은 수집에서 제외한다. 검증하지 않은 버전·이벤트는 `unsupported`다.
- 설치 환경에서 관측하지 못한 항목은 지원표에 `미확인`으로 적는다. 공식 문서에만 있는 내용은 `문서상`으로 구분한다.
- 발췌 분량·보관 기간·압축·모델·자동 생성 범위는 정하지 않는다. 이 작업은 그 결정에 쓸 실측값(훅 지연, 입력 크기)만 남긴다.
- 모델 호출은 아래 "실행 상한" 안에서만 한다. 유료 API 키 경로로 전환하지 않는다.
- 제품 코드(`crates/`, `apps/`), 버전, `CHANGELOG.md`, EXE는 바꾸지 않는다.
- `git add`는 경로를 지정해서만 한다. 미커밋 사용자 파일 `docs/TimeSchedule_Product_Requirements_v0.1_2026-11-17.md`를 커밋에 넣지 않는다.

## 실행 상한 (승인 대상)

| 용도 | 실행 방식과 모델 | 상한 |
| --- | --- | --- |
| 구현·문서 | 이 세션, `claude-fable-5-2` | 구현자 1명. 구현용 하위 에이전트 없음 |
| Claude 시나리오 생성 | `claude -p --model claude-haiku-5-5 --max-turns 3`, 시험 폴더에서만 | 8회(계획 5 + 재시도 3), 회당 180초 |
| Codex 시나리오 생성 | `codex exec -m gpt-6-luna -c model_reasoning_effort="low" --sandbox workspace-write`, 시험 폴더에서만 | 8회(계획 5 + 재시도 3), 회당 180초 |
| 데스크톱 앱 시나리오 | 사용자가 Claude 앱·Codex 앱에서 직접 입력 | 앱마다 프롬프트 5개 이내. 모델은 사용자가 고른다 |
| 독립 검토 | `codex exec -m gpt-6.2-astra --sandbox read-only`, 추론 설정은 `config.toml`의 `high` | 3회(최초 1 + 재검토 2), 회당 20분 |

상한에 닿으면 남은 항목을 `미확인`으로 적고 결과를 제출한다. 상한을 넘겨야 하면 이유를 적고 멈춘다. 시나리오 모델은 훅 입력의 형태를 얻는 용도이며 제품의 문서 생성 모델 후보가 아니다.

## Review Focus

1. 등록 경로 `C:\LSH\TimeSchedule`가 있을 때 `C:\LSH\TimeSchedule-test`의 이벤트가 같은 프로젝트로 붙는 문제 → 1C `sibling_prefix_path_is_not_registered`.
2. 훅 입력이 깨진 JSON이거나 1 MiB를 넘을 때 probe가 도구를 막거나 조용히 성공하는 문제 → 1A `probe_marks_invalid_json`, `probe_marks_oversized_payload`.
3. probe가 표준 출력에 글자를 내보내 Claude·Codex의 대화 맥락에 끼어드는 문제 → 1A `probe_writes_one_record_and_prints_nothing`.
4. 같은 관측 기록을 다시 변환했을 때 이벤트 ID가 달라져 재전송이 중복으로 보이는 문제 → 1C `same_capture_normalizes_to_same_event_id`.
5. fixture에 실제 사용자 폴더명이나 세션 ID가 남아 공개 저장소에 올라가는 문제 → 1B `fixtures_contain_no_private_markers`.

---

## 파일 구조

| 파일 | 책임 |
| --- | --- |
| `scripts/activity-adapters/probe-hook.mjs` (생성) | 시험 전용. 훅 표준 입력을 관측 기록 한 개로 저장 |
| `scripts/activity-adapters/sanitize-fixture.mjs` (생성) | 관측 기록의 개인 경로·세션 ID를 고정 문자열로 치환 |
| `scripts/activity-adapters/contract.mjs` (생성) | 경로 대조와 공통 변환 순서 |
| `scripts/activity-adapters/claude.mjs`, `codex.mjs` (생성) | 도구별 검증 버전·이벤트 표와 `normalizeEvent` |
| `tests/activity-probe.test.mjs` (생성) | probe·sanitize 시험 |
| `tests/activity-adapters.test.mjs` (생성) | 계약 가드 시험과 fixture 고정 시험 |
| `tests/fixtures/project-activity/{claude,codex}/<시나리오>/` (생성) | 치환한 표본과 `expected.json` |
| `docs/project-capture-support.md` (생성) | 지원표, 이벤트별 관측 필드, 실측값, 재현 방법, 한계 |
| `docs/superpowers/handoffs/2026-11-18-m0-capture-support.md` (생성) | Astra 검토·개념 설명용 인계 자료 |

`package.json`의 `test`는 `node --test tests/*.test.mjs`이므로 새 시험 파일은 CI에 자동 포함된다.

### 공통 타입

```js
// CaptureRecord — probe가 쓰는 파일 한 개
{ capture_schema: 1, capture_id: "<uuid v4>", source: "claude-code" | "codex",
  tool_version: string | null, captured_at: "<UTC ISO, ms>",
  origin: "tool" | "timeschedule_generation",
  payload_bytes: number, payload: object | null,
  payload_error: null | "invalid_json" | "too_large",
  env_names: string[],            // 이름이 CLAUDE 또는 CODEX로 시작하는 환경 변수의 이름만
  env_allowlisted: object,        // CLAUDE_CODE_ENTRYPOINT, CLAUDE_CODE_REMOTE, CLAUDECODE의 값만
  probe_elapsed_ms: number }

// NormalizeContext
{ registrations: [{ canonicalPath: string, decision: "new" | "link" | "exclude", projectId: string }],
  receivedAt: "<UTC ISO>" }

// NormalizeResult
{ outcome: "activity" | "candidate" | "unsupported" | "excluded",
  envelope?: ActivityEnvelope, candidate?: { source, path }, reason?: string }

// ActivityEnvelope (M0 범위)
{ schema_version: 1, source, session_id, event_id, revision: 1, project_id,
  occurred_at, received_at, time_basis: "source_event" | "hook_capture",
  kind, body, evidence_refs: [], origin: "tool" }
```

`reason` 값: `malformed`, `generation_origin`, `unknown_version`, `unknown_event`, `redundant_event`, `path_excluded`.
`kind` 값: `session_start`, `user_prompt`, `tool_result`, `turn_end`, `turn_failed`, `interrupted`, `compaction`, `session_end`.

---

### Task 1A: 관측 스크립트와 표본 치환

**Files:**
- Create: `scripts/activity-adapters/probe-hook.mjs`, `scripts/activity-adapters/sanitize-fixture.mjs`
- Test: `tests/activity-probe.test.mjs`

**Interfaces:**
- Produces: CLI `node probe-hook.mjs --source <claude-code|codex> --out <dir> [--tool-version <text>]`. 표준 입력을 읽어 `<out>/<captured_at의 숫자만>-<capture_id>.json`에 CaptureRecord를 쓴다(임시 파일에 쓴 뒤 이름 변경). 표준 출력에는 아무것도 쓰지 않는다. 저장 성공은 종료 코드 0, 저장 실패는 종료 코드 1과 표준 오류 한 줄. 종료 코드 2는 쓰지 않는다(두 도구에서 차단 신호다).
- Produces: `sanitizeCapture(record, { homeDir, labRoot, sessionIds }) -> CaptureRecord`. `sessionIds`는 호출 간에 공유하는 `Map`이다.
- Produces: CLI `node sanitize-fixture.mjs --in <raw dir> --out <fixture dir> --home <dir> --lab <dir>`. 출력 파일명은 `<두 자리 순번>-<hook_event_name>.json`, 순번은 `captured_at` 순서.

- [ ] **Step 1: 실패하는 시험 작성** — `tests/activity-probe.test.mjs`. 각 시험은 `fs.mkdtemp`로 만든 임시 폴더에 `process.execPath`로 probe를 실행한다.

```js
test('probe_writes_one_record_and_prints_nothing', async () => {
  const r = run(['--source', 'claude-code', '--out', dir, '--tool-version', '2.1.325'],
    '{"hook_event_name":"Stop","session_id":"s1","cwd":"C:\\\\lab"}');
  assert.equal(r.status, 0);
  assert.equal(r.stdout, '');
  const files = readdirSync(dir);
  assert.equal(files.length, 1);
  const rec = JSON.parse(readFileSync(join(dir, files[0]), 'utf8'));
  assert.equal(rec.capture_schema, 1);
  assert.equal(rec.source, 'claude-code');
  assert.equal(rec.tool_version, '2.1.325');
  assert.equal(rec.origin, 'tool');
  assert.equal(rec.payload_error, null);
  assert.deepEqual(rec.payload, { hook_event_name: 'Stop', session_id: 's1', cwd: 'C:\\lab' });
  assert.match(rec.capture_id, /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/);
});
test('probe_marks_invalid_json', ...)        // 입력 '{"a":' → status 0, payload === null, payload_error === 'invalid_json'
test('probe_marks_oversized_payload', ...)   // 입력 1 MiB + 1바이트 → status 0, payload === null, payload_error === 'too_large', payload_bytes > 1048576
test('probe_marks_generation_origin', ...)   // env TIMESCHEDULE_ORIGIN=generation → origin === 'timeschedule_generation'
test('probe_records_env_names_not_values', ...) // env CODEX_SECRET_X=zzz → env_names에 'CODEX_SECRET_X' 포함, 파일 전체 문자열에 'zzz' 없음
test('probe_fails_visibly_when_out_is_a_file', ...) // --out이 일반 파일 → status 1, stdout === '', stderr 비어 있지 않음
test('sanitize_replaces_home_lab_and_session_ids', () => {
  const ids = new Map();
  const out = sanitizeCapture(sample, { homeDir: 'C:\\Users\\realname', labRoot: 'C:\\LSH\\_capture-lab\\m0-sample', sessionIds: ids });
  const text = JSON.stringify(out);
  assert.ok(!/realname/i.test(text));
  assert.equal(out.payload.cwd, 'C:\\lab\\m0-sample');
  assert.equal(out.payload.session_id, 'session-0001');
  assert.equal(sanitizeCapture(sample, { homeDir: 'C:\\Users\\realname', labRoot: 'C:\\LSH\\_capture-lab\\m0-sample', sessionIds: ids }).payload.session_id, 'session-0001');
});
```

- [ ] **Step 2: 실패 확인** — `node --test tests/activity-probe.test.mjs`. 기대: 모듈 없음으로 전부 실패.
- [ ] **Step 3: `probe-hook.mjs` 구현** — 입력 상한 `1048576`바이트. `origin`은 `process.env.TIMESCHEDULE_ORIGIN === 'generation'`일 때만 `timeschedule_generation`. `probe_elapsed_ms`는 프로세스 시작(`process.hrtime` 첫 호출)부터 파일 이름 변경 직전까지.
- [ ] **Step 4: `sanitize-fixture.mjs` 구현** — 문자열 값 전체를 순회해 `homeDir`은 `C:\Users\lab-user`, `labRoot`는 `C:\lab\m0-sample`로 치환한다(대소문자 무시, `/`·`\`·JSON 이스케이프 형태 모두). `session_id`와 그 값이 나타나는 모든 문자열은 `session-NNNN`으로 바꾼다. `env_names`는 유지하고 `capture_id`는 유지한다.
- [ ] **Step 5: 통과 확인** — `node --test tests/activity-probe.test.mjs`. 기대: 7개 통과.
- [ ] **Step 6: 커밋** — `git add scripts/activity-adapters/probe-hook.mjs scripts/activity-adapters/sanitize-fixture.mjs tests/activity-probe.test.mjs docs/superpowers/plans/2026-11-18-m0-capture-support.md` 후 `git commit -m "test: add capture probe and fixture sanitizer for M0"`.

### Task 1B: 격리 시험 폴더와 표본 수집

**Files:**
- Create(저장소 밖): `<lab>\.claude\settings.json`, `<lab>\.codex\hooks.json`, `<lab>\.capture\probe-hook.mjs`(1A 사본), `<lab>\.capture\raw\{claude,codex}\<시나리오>\`
- Create: `tests/fixtures/project-activity/{claude,codex}/<시나리오>/NN-<Event>.json`
- Test: `tests/activity-adapters.test.mjs`의 `fixtures_contain_no_private_markers`

`<lab>`은 사용자가 승인한 시험 폴더다. 제안값은 `C:\LSH\_capture-lab\m0-sample`.

**Interfaces:**
- Consumes: 1A의 probe CLI와 sanitize CLI.
- Produces: 시나리오 폴더 이름 `s1-normal`, `s2-resume`, `s3-compact`, `s4-interrupt`, `s5-tool-failure`, `s6-kill`, `s7-generation-origin`. 표면이 데스크톱 앱이면 접미사 `-app`, 헤드리스 CLI면 `-cli`.

- [ ] **Step 1: 시험 폴더 생성** — `<lab>`을 만들고 `git init`, `README.md` 한 줄, `.gitignore`에 `.capture/`. probe 사본을 `<lab>\.capture\`에 복사한다.
- [ ] **Step 2: 프로젝트 범위 훅 작성** — 두 파일 모두 공식 문서가 나열한 이벤트 전부에 같은 명령을 건다: `node <lab>\.capture\probe-hook.mjs --source <도구> --out <lab>\.capture\raw\<도구>\_inbox --tool-version "<설치 버전>"`. Claude는 `.claude/settings.json`의 `hooks`, Codex는 `.codex/hooks.json`. 설치 버전이 거부하는 이벤트 이름은 지원표에 `설정 거부`로 적고 그 이벤트만 뺀다.
- [ ] **Step 3: 사용자 확인 요청** — Codex 앱에서 `<lab>`을 신뢰하고 훅 검토를 승인해 달라고 요청한다(Codex는 검토하지 않은 훅을 실행하지 않는다). Claude 앱에서 `<lab>` 세션을 열 때 뜨는 신뢰 확인도 사용자가 한다. 응답 전에는 Step 5의 앱 시나리오를 건너뛴다.
- [ ] **Step 4: 헤드리스 시나리오 실행** — 시나리오마다 `_inbox`를 비우고 실행한 뒤 `_inbox`를 `<시나리오>-cli`로 이름 변경한다.

| 시나리오 | Claude (`claude -p --model claude-haiku-5-5 --max-turns 3 --permission-mode acceptEdits`) | Codex (`codex exec -m gpt-6-luna -c model_reasoning_effort="low" --sandbox workspace-write -C <lab>`) |
| --- | --- | --- |
| s1-normal | "note.txt 파일에 hello 한 줄을 써라" | 같은 문장 |
| s2-resume | s1 세션을 `--resume <id>`로 이어 "note.txt에 again 한 줄을 추가해라" | `codex exec resume --last`로 같은 문장 |
| s5-tool-failure | "node missing.js를 실행하고 결과를 한 줄로 보고해라" | 같은 문장 |
| s6-kill | "1부터 200까지 한 줄에 하나씩 천천히 출력해라" 실행 5초 뒤 내가 시작한 그 프로세스만 종료 | 같은 방식 |
| s7-generation-origin | s1과 같은 문장, 환경 변수 `TIMESCHEDULE_ORIGIN=generation` | 같은 방식 |

`s6`에서는 실행 전에 얻은 PID 하나만 종료한다. 데스크톱 앱 프로세스는 종료하지 않는다.

- [ ] **Step 5: 데스크톱 앱 시나리오(사용자 수행)** — 사용자에게 체크리스트를 준다: 각 앱에서 `<lab>`을 열고 ① s1 문장 입력(`s1-normal-app`) ② 같은 세션을 닫았다 다시 열어 s2 문장 입력(`s2-resume-app`) ③ `/compact` 실행(`s3-compact-app`) ④ s6 문장 입력 후 Esc로 중단(`s4-interrupt-app`). 단계가 끝날 때마다 사용자가 알려 주면 `_inbox`를 해당 이름으로 옮긴다. 사용자가 건너뛴 단계는 `미확인`이다.
- [ ] **Step 6: 관측 요약** — 시나리오별로 이벤트 이름 순서, 이벤트별 키 이름, `payload_bytes`, `probe_elapsed_ms`의 최소·중앙·최대, `env_names`를 집계한다. 각 시험 세션의 `transcript_path`가 가리키는 파일이 있는지, 확장자, 레코드 `type`과 키 이름을 집계한다(값 미출력). 자동 축약과 API 오류는 유발하지 않으며 `미확인(문서상)`으로 남긴다.
- [ ] **Step 7: fixture 생성** — 시나리오마다 `node scripts/activity-adapters/sanitize-fixture.mjs --in <raw 시나리오> --out tests/fixtures/project-activity/<도구>/<시나리오> --home %USERPROFILE% --lab <lab>`.
- [ ] **Step 8: 실패하는 시험 작성 후 통과 확인**

```js
test('fixtures_contain_no_private_markers', () => {
  const home = basename(homedir());
  for (const file of allFixtureFiles()) {
    const text = readFileSync(file, 'utf8');
    assert.ok(!text.toLowerCase().includes(home.toLowerCase()), file);
    assert.ok(!/[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}/.test(text), file);
    assert.ok(!/\b(sk-[A-Za-z0-9_-]{16,}|ghp_[A-Za-z0-9]{20,}|github_pat_[A-Za-z0-9_]{20,})/.test(text), file);
    assert.ok(!/[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}/.test(
      JSON.stringify(JSON.parse(text).payload ?? {})), file);
  }
  assert.ok(allFixtureFiles().length > 0);
});
```

Run: `node --test tests/activity-adapters.test.mjs`. 위반이 나오면 `sanitizeCapture`를 고치고 1A 시험에 그 형태를 추가한 뒤 fixture를 다시 만든다. fixture를 손으로 고치지 않는다.

- [ ] **Step 9: 커밋** — `git add tests/fixtures/project-activity tests/activity-adapters.test.mjs`(sanitize를 고쳤으면 그 두 파일도) 후 `git commit -m "test: add sanitized hook samples from the capture lab"`.

### Task 1C: 어댑터 계약

**Files:**
- Create: `scripts/activity-adapters/contract.mjs`, `claude.mjs`, `codex.mjs`
- Modify: `tests/activity-adapters.test.mjs`
- Create: `tests/fixtures/project-activity/<도구>/<시나리오>/expected.json`

**Interfaces:**
- Consumes: 1B의 fixture와 관측 요약.
- Produces: `resolveRegistration(cwd, registrations) -> registration | null`, `normalizeCapture(record, context, adapter) -> NormalizeResult` (`contract.mjs`).
- Produces: `adapter = { source, verifiedVersions: string[], events: { [hookEventName]: { kind, body(payload) } | { skip: 'redundant_event' } } }`와 `normalizeEvent(input, context) -> NormalizeResult` (`claude.mjs`, `codex.mjs`). 작업 3의 `scripts/activity-capture.mjs`가 이 두 `normalizeEvent`를 호출한다.

`normalizeCapture`의 판정 순서(앞에서 걸리면 뒤는 보지 않는다):

1. `capture_schema !== 1`이거나 `payload`가 객체가 아니거나 `payload.session_id`·`payload.cwd`·`payload.hook_event_name` 중 하나가 문자열이 아님 → `unsupported/malformed`
2. `record.origin === 'timeschedule_generation'` → `excluded/generation_origin`
3. `record.tool_version`이 `adapter.verifiedVersions`에 없음 → `unsupported/unknown_version`
4. 이벤트 이름이 `adapter.events`에 없음 → `unsupported/unknown_event`
5. 이벤트가 `skip` → `excluded/redundant_event`
6. `resolveRegistration` 결과가 `null` → `candidate`, `candidate: { source, path: payload.cwd }`만 반환
7. 등록 결정이 `exclude` → `excluded/path_excluded`
8. 그 외 → `activity`

`resolveRegistration`: 양쪽 경로를 소문자화하고 `/`를 `\`로 바꾸고 끝의 `\`를 뗀 뒤, `cwd`가 등록 경로와 같거나 등록 경로 + `\`로 시작하는 등록 중 가장 긴 것을 고른다. 파일 시스템을 조회하지 않는다(junction·worktree의 실제 경로 해석은 작업 2의 책임).

봉투 값: `event_id = record.capture_id`, `occurred_at = record.captured_at`, `time_basis = 'hook_capture'`(훅 입력에 이벤트 시각이 관측되면 그 값과 `'source_event'`), `received_at = context.receivedAt`.

이벤트 표의 출발값이다. 1B에서 관측한 이벤트·필드만 남기고, 관측하지 못한 줄은 표에서 지우고 지원표에 `미확인`으로 적는다.

| 훅 이벤트 | 처리 | `body` |
| --- | --- | --- |
| SessionStart | `session_start` | `{ source }` |
| UserPromptSubmit | `user_prompt` | `{ prompt }` |
| PreToolUse | `skip` | — |
| PostToolUse | `tool_result` | `{ tool_name, tool_use_id, input_keys }` (`input_keys`는 `tool_input`의 키 이름 정렬 배열) |
| PostToolUseFailure (Claude) | `tool_result` | `{ tool_name, tool_use_id, input_keys, failed: true }` |
| Stop | `turn_end` | `{ last_assistant_message }` |
| StopFailure (Claude) | `turn_failed` | `{}` |
| Interrupt (Codex) | `interrupted` | `{}` |
| PreCompact | `skip` | — |
| PostCompact | `compaction` | `{ trigger }` |
| SessionEnd | `session_end` | `{ reason }` |

도구 입력·출력의 본문은 M0 봉투에 넣지 않는다. 얼마나 발췌할지는 미확정 항목이며 1B의 크기 실측을 근거 자료로 남길 뿐이다.

- [ ] **Step 1: 실패하는 가드 시험 작성** — `tests/activity-adapters.test.mjs`. `REG = [{ canonicalPath: 'C:\\LSH\\TimeSchedule', decision: 'new', projectId: 'p1' }]`, `ctx = { registrations: REG, receivedAt: '2026-11-18T00:00:00.000Z' }`, `capture(overrides)`는 `adapter.verifiedVersions[0]`과 `cwd: 'C:\\LSH\\TimeSchedule'`, `hook_event_name: 'UserPromptSubmit'`, `prompt: 'SECRET-BODY'`를 가진 CaptureRecord를 만든다. 두 어댑터 각각에 대해 실행한다.

```js
test('normalize_unknown_version_is_unsupported', () => {
  const r = normalizeEvent(capture({ tool_version: '0.0.0-unknown' }), ctx);
  assert.deepEqual(r, { outcome: 'unsupported', reason: 'unknown_version' });
});
test('unconfirmed_path_has_no_body', () => {
  const r = normalizeEvent(capture({ cwd: 'C:\\Other\\Repo' }), ctx);
  assert.equal(r.outcome, 'candidate');
  assert.equal(r.envelope, undefined);
  assert.deepEqual(r.candidate, { source: adapter.source, path: 'C:\\Other\\Repo' });
  assert.ok(!JSON.stringify(r).includes('SECRET-BODY'));
});
test('generation_origin_is_excluded', () => {
  assert.deepEqual(normalizeEvent(capture({ origin: 'timeschedule_generation' }), ctx),
    { outcome: 'excluded', reason: 'generation_origin' });
});
test('generation_origin_wins_over_unknown_version', ...) // 둘 다 해당 → excluded/generation_origin
test('excluded_path_is_excluded', ...)                   // decision: 'exclude' → excluded/path_excluded, envelope 없음
test('sibling_prefix_path_is_not_registered', ...)       // cwd 'C:\\LSH\\TimeSchedule-test' → candidate
test('path_match_ignores_case_separators_and_subfolders', ...) // 'c:/lsh/timeschedule/crates/' → activity, project_id 'p1'
test('nested_exclude_wins_over_parent', ...)             // 부모 new + 'C:\\LSH\\TimeSchedule\\vendor' exclude, cwd는 vendor 아래 → excluded/path_excluded
test('unknown_event_is_unsupported', ...)                // hook_event_name 'NotARealEvent' → unsupported/unknown_event
test('malformed_capture_is_unsupported', ...)            // payload: null, capture_schema: 2, cwd 누락 각각 → unsupported/malformed
test('same_capture_normalizes_to_same_event_id', () => {
  const c = capture({});
  const a = normalizeEvent(c, ctx), b = normalizeEvent(c, ctx);
  assert.equal(a.outcome, 'activity');
  assert.equal(a.envelope.event_id, c.capture_id);
  assert.deepEqual(a, b);
});
test('envelope_keeps_capture_time_and_receive_time_apart', ...) // occurred_at === captured_at, received_at === ctx.receivedAt, time_basis === 'hook_capture'
```

- [ ] **Step 2: 실패 확인** — `node --test tests/activity-adapters.test.mjs`. 기대: 모듈 없음으로 가드 시험 실패, `fixtures_contain_no_private_markers`는 통과.
- [ ] **Step 3: `contract.mjs`, `claude.mjs`, `codex.mjs` 구현** — `verifiedVersions`에는 1B에서 실제 표본을 얻은 `tool_version` 문자열만 넣는다.
- [ ] **Step 4: fixture 고정 시험 추가** — 시나리오마다 `expected.json`을 쓴다: `[{ "file": "01-SessionStart.json", "outcome": "activity", "kind": "session_start" }, ...]`. 기대값은 1B 관측 요약과 위 이벤트 표에서 나온다. `s7-generation-origin`은 모든 줄이 `excluded`다.

```js
for (const scenario of allScenarios()) test(`fixture ${scenario.name}`, () => {
  const lab = { registrations: [{ canonicalPath: 'C:\\lab\\m0-sample', decision: 'new', projectId: 'lab' }], receivedAt: '2026-11-18T00:00:00.000Z' };
  const actual = scenario.files.map(f => { const r = scenario.normalize(readJson(f), lab);
    return { file: basename(f), outcome: r.outcome, ...(r.envelope ? { kind: r.envelope.kind } : {}) }; });
  assert.deepEqual(actual, scenario.expected);
});
test('every_fixture_event_name_is_listed_or_reported_unsupported', ...) // fixture에 나온 이벤트 이름마다 adapter.events에 있거나 expected가 unsupported
```

- [ ] **Step 5: 통과 확인** — `node --test tests/activity-adapters.test.mjs tests/activity-probe.test.mjs`, 이어서 `node --test tests/*.test.mjs`. 기대: 새 시험 전부 통과, 기존 시험 결과 변화 없음.
- [ ] **Step 6: 커밋** — `git add scripts/activity-adapters/contract.mjs scripts/activity-adapters/claude.mjs scripts/activity-adapters/codex.mjs tests/activity-adapters.test.mjs tests/fixtures/project-activity` 후 `git commit -m "feat: add verified-only hook adapters for Claude Code and Codex"`.

### Task 1D: 지원표, 검토, 인계

**Files:**
- Create: `docs/project-capture-support.md`, `docs/superpowers/handoffs/2026-11-18-m0-capture-support.md`

- [ ] **Step 1: 지원표 작성** — `docs/project-capture-support.md`의 구획: ① 시험 환경(앱·CLI 버전, 실행 표면, 시험일, 훅 승인 상태) ② 지원표(도구 × 표면 × 시나리오: `지원`/`부분`/`미지원`/`미확인`, 근거는 fixture 경로 또는 `문서상`) ③ 이벤트별 관측 필드와 봉투 매핑 ④ 훅이 보지 못하는 것과 원문 로그(위치, 확장자, 레코드 종류, 오래된 세션의 `.zst` 여부)로 복구 가능한지 ⑤ 실측값(`payload_bytes`, `probe_elapsed_ms`의 최소·중앙·최대와 표본 수) ⑥ 재현 방법 ⑦ 자동 수집으로 얻는 것과 에이전트 체크포인트로 보충해야 하는 것 ⑧ 한계와 작업 2·3에 넘기는 조건.
- [ ] **Step 2: 검증** — `node --test tests/*.test.mjs`와 `git diff --check`. 지원표의 `지원` 줄마다 fixture 파일이 실제로 있는지 대조한다.
- [ ] **Step 3: 커밋·푸시·PR** — `git add docs/project-capture-support.md` 후 `git commit -m "docs: record verified capture support for Claude Code and Codex"`, `git push -u origin codex/m0-capture-support`, PR 생성(base는 승인받은 값). 머지하지 않는다.
- [ ] **Step 4: Astra 검토** — BASE=`4cb7fd15259d1716fc8f0f1a0cacd887c6cfa691`, HEAD=푸시한 커밋, 변경 경로 목록을 프롬프트 파일에 적고 `codex exec -m gpt-6.2-astra --sandbox read-only --color never -C C:\LSH\TimeSchedule -o .cache/reviews/m0-round-1.md - < .cache/reviews/m0-prompt.md`를 실행한다. 실행 로그의 `model:` 줄이 `gpt-6.2-astra`인지 확인한다. 아니거나 실행이 거부되면 다른 모델로 바꾸지 않고, 같은 프롬프트를 Codex 앱에 붙여 넣을 요청문으로 사용자에게 전달하고 멈춘다.
- [ ] **Step 5: 지적 처리** — 이 계획 범위 안의 지적은 시험을 먼저 추가해 고치고 새 HEAD로 재검토한다(최대 2회). 범위 밖 지적이나 새 의사결정은 적어 두고 사용자에게 넘긴다.
- [ ] **Step 6: 인계 자료 작성·커밋** — `docs/superpowers/handoffs/2026-11-18-m0-capture-support.md`: 승인 범위, BASE/HEAD, 커밋별 변경 경로, 파일·함수별 역할과 설계 이유(대안 포함), 시험 명령과 결과, 확인한 동작과 미확인 동작, 실제 모델·실행 횟수·관측 사용량과 확인 불가 항목, Astra 검토 원문 위치와 한국어 요약. Rust 코드는 변경하지 않았다고 적고, 학습 주제 "프로그램·프로세스·파일·DB의 차이"를 probe가 별도 프로세스로 실행되어 파일을 남기는 흐름으로 설명할 수 있게 근거 위치를 적는다. 이 문서 커밋은 검토 대상 코드를 바꾸지 않으므로 재검토하지 않는다.
- [ ] **Step 7: 자동 개발 기록** — `docs/project-records.md` 절차를 이 작업의 기준 커밋·완료 커밋·변경 경로로 실행한다. `prepare`가 `skipped`면 끝낸다.
- [ ] **Step 8: 결과 제출 후 정지** — 지원표 요약, 확인·미확인, 시험 결과, Astra 결과, PR 링크, 남은 한계, 시험 폴더 훅을 지우는 방법을 전달한다. 작업 2를 시작하지 않는다.

**Exit:** 두 도구의 지원표, 치환한 표본, 재현 방법, 관측한 버전·이벤트만 변환하는 어댑터가 있다. 관측하지 못한 항목은 `미확인`으로 남아 있다.

## 예상 한계

- 훅 입력에 도구 버전이 없으면 `tool_version`은 시험 폴더 설정 시점에 적은 값이다. 실제 수집에서 버전을 어디서 얻을지는 작업 3의 과제로 넘긴다.
- 자동 축약, API 오류, 데스크톱 앱 강제 종료는 유발하지 않는다. 문서상 동작만 적는다.
- 내가 실행할 수 있는 것은 CLI다. 데스크톱 앱 표면의 관측은 사용자가 수행한 단계까지만 확인된다.
- 작은 시험 폴더의 입력 크기와 지연은 긴 실제 세션을 대표하지 않는다. 표본 수와 함께 적는다.
- MCP 경로는 이 작업에서 시험하지 않는다.
