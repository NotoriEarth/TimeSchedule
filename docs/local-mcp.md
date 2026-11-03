# 로컬 일정 MCP (0.9.0)

TimeSchedule을 실행하면 같은 Windows PC의 Codex와 Claude Code가 공통 일정 도구를 사용할 수 있다. 앱의 SQLite에 직접 저장하며 앱·위젯·열린 빠른 일정 목록에 다음 500ms 갱신 시점부터 반영한다. 저장 중인 작업이나 UI 부하에 따라 화면 반영은 늦어질 수 있다. 모델 실행·외부 전송·Space 호출은 MCP 자체에 포함되지 않는다.

## 실행 상태

- 대표 앱: `artifacts/TimeSchedule.exe`. **설정 → 로컬 일정 MCP**에서 실행 주소 또는 시작 실패 사유를 확인한다.
- 기본 주소: `http://127.0.0.1:39281/mcp`. 트레이에 있는 동안 동작하며 **완전 종료**하면 서버도 종료한다. 기록 전용 CLI나 두 번째 앱은 서버를 새로 열지 않는다.
- 포트 충돌이나 인증 파일 오류가 있어도 다이어리는 사용할 수 있다. 원인을 해결한 다음 앱을 완전히 종료하고 다시 실행한다.
- `TIMESCHEDULE_MCP_PORT`는 포트를 바꾼다. `TIMESCHEDULE_MCP_DISABLED=1`이면 시작하지 않는다. 환경변수를 바꾼 뒤 앱을 재시작해야 한다.
- 기본 데이터 폴더는 `%LOCALAPPDATA%\com.timeschedule.personal.native-experiment`. `TIMESCHEDULE_DATA_DIR`로 분리할 수 있다. 저장소에서 시험할 때는 `.cache/` 아래의 fixture 폴더를 사용한다.

## 인증 준비

최초 실행 시 데이터 폴더의 `mcp-token.dpapi`에 244비트 이상의 난수 토큰을 Windows CurrentUser DPAPI로 암호화해 보관한다. 같은 Windows 계정에서 다음 명령을 실행하면 복호화한 토큰을 현재 PowerShell 프로세스의 `TIMESCHEDULE_MCP_TOKEN`에 넣는다. 토큰 자체는 출력하지 않는다. `mcp-status.json`에는 주소·PID·버전만 들어간다.

```powershell
# Windows PowerShell에서 스크립트 실행이 제한된 경우, 이 창에만 적용
Set-ExecutionPolicy -Scope Process Bypass
.\scripts\mcp-session.ps1 -Status
# 별도 fixture라면
.\scripts\mcp-session.ps1 -DataDir C:\LSH\TimeSchedule\.cache\mcp-fixture -Status
```

`running`과 네 도구 이름이 나오면 인증된 초기화와 도구 목록 조회가 성공한 것이다. 원문 일정은 조회하지 않는다. 이 스크립트는 기본적으로 사용자 환경변수나 클라이언트 설정 파일을 수정하지 않는다.

## Codex CLI 및 데스크톱 앱

한 번만 실행하여 서버를 등록한다. [Codex MCP 설정](https://developers.openai.com/codex/mcp)의 Streamable HTTP와 환경변수 인증을 사용한다.

```powershell
codex mcp add timeschedule --url http://127.0.0.1:39281/mcp --bearer-token-env-var TIMESCHEDULE_MCP_TOKEN
.\scripts\mcp-session.ps1 -Status
codex
```

동일한 설정의 TOML 형식은 다음과 같다. 이미 같은 이름의 항목이 있으면 중복 추가하지 않고 해당 항목을 수정한다.

```toml
[mcp_servers.timeschedule]
url = "http://127.0.0.1:39281/mcp"
bearer_token_env_var = "TIMESCHEDULE_MCP_TOKEN"
```

현재 실행 중인 Codex 데스크톱 앱은 PowerShell에서 새로 설정한 환경변수를 받지 않는다. 데스크톱에서 지속적으로 사용하려면 아래 옵션을 **사용자가 선택해 실행**한 뒤 Windows에서 로그아웃/로그인하고 Codex를 새로 실행한다. 이 옵션은 사용자 환경변수에 토큰을 **평문으로 저장**하므로 DPAPI 파일과 보호 수준이 다르다. 이를 원하지 않으면 매번 위 스크립트를 실행한 PowerShell에서 Codex CLI를 시작한다. 개발 작업 중 이 옵션이나 전역 설정 변경을 자동 실행하지 않는다.

```powershell
.\scripts\mcp-session.ps1 -PersistToken -Status
# 나중에 사용자 환경변수에서 제거할 때
[Environment]::SetEnvironmentVariable('TIMESCHEDULE_MCP_TOKEN', $null, 'User')
```

## Claude Code

[Claude Code MCP 설정](https://code.claude.com/docs/en/mcp)의 HTTP와 환경변수 치환을 사용한다. 프로젝트 `.mcp.json`의 기존 `mcpServers`에 다음 항목을 합친다. 비밀값을 직접 쓰지 않는다.

```json
{
  "mcpServers": {
    "timeschedule": {
      "type": "http",
      "url": "http://127.0.0.1:39281/mcp",
      "headers": { "Authorization": "Bearer ${TIMESCHEDULE_MCP_TOKEN}" }
    }
  }
}
```

```powershell
.\scripts\mcp-session.ps1 -Status
claude
```

Claude Code의 `/mcp`에서 연결 상태와 네 도구를 확인한다. 처음 추가한 프로젝트 서버의 신뢰 확인은 해당 제품에서 처리한다. 포트를 바꾸면 양쪽 클라이언트의 주소도 바꾼다.

## 도구와 충돌 처리

| 도구 | 필수 입력 | 결과 |
| --- | --- | --- |
| `calendar_list` | `from`, `to` (YYYY-MM-DD) | 본문 없는 일정 목록, `truncated` |
| `calendar_get` | `id` | 메모와 `updatedAt`을 포함한 일정 |
| `calendar_create` | `title`, `date` | 실제 저장된 일정 |
| `calendar_update` | `id`, `expectedUpdatedAt`, 변경 필드 1개 이상 | 실제 저장된 일정 또는 충돌 |

생성 기본값은 `allDay=true`, `notes=""`, `color="sage"`, `completed=false`다. 시간 일정은 `allDay=false`, `startTime`, `endTime`을 함께 지정하고 같은 날짜에서 종료가 시작보다 늦어야 한다. 시간은 로컬 달력의 `HH:mm`이며 시간대 변환이나 여러 날 일정은 제공하지 않는다. 색상은 `sage`, `blue`, `amber`, `rose` 중 하나다.

수정할 때 상세 조회 결과의 `updatedAt`을 `expectedUpdatedAt`에 그대로 넣는다. 생략한 필드는 유지하고 `null`은 거절한다. `allDay=true`로 바꾸면 시간을 비운다. 앱과 다른 클라이언트의 변경이 먼저 저장됐다면 충돌을 반환한다. 다시 상세를 읽고 변경 내용을 확인한 다음 새 버전으로 수정한다. 열린 앱 편집 초안도 유지되며, 오래된 초안을 저장하면 충돌을 알려준다. 초안을 보관한 뒤 편집을 닫고 최신 일정을 다시 열어 내용을 합칠 수 있다.

응답을 받지 못했거나 시간이 초과된 쓰기는 이미 저장됐을 수도 있다. 특히 생성은 같은 입력을 다시 호출하면 중복 일정이 생길 수 있으므로 먼저 목록·상세를 확인한다. 삭제·할 일·프로젝트·개발 기록·임의 파일/SQL/명령 실행 도구는 없다.

## 전송과 자원 제한

공식 Rust SDK `rmcp 3.8.0`과 `axum 0.8.9`를 고정한다. [Streamable HTTP 표준](https://modelcontextprotocol.io/specification/2025-11-25/basic/transports)의 JSON 응답 모드를 사용한다. 지원 버전은 `2025-11-25`, `2025-06-18`, `2025-03-26`이며 세션·상주 SSE·서버 푸시·OAuth는 사용하지 않는다. GET은 405다. 클라이언트는 `Accept: application/json, text/event-stream`, `MCP-Protocol-Version`과 Bearer 헤더를 보낸다.

- 127.0.0.1에만 바인딩. Host는 정확한 주소/포트만 허용. 브라우저 Origin이 있는 요청은 모두 403으로 거절한다.
- 모든 경로·메서드에 인증 필요. 인증 없는 도구 목록/데이터 공개 없음. 요청/응답 본문과 토큰을 로그에 남기지 않는다.
- 인증 전 연결부터 최대 16개로 제한하고, HTTP 헤더를 5초 안에 완성하지 않는 연결을 닫는다. 연결 상한을 넘으면 HTTP 파서/작업을 만들기 전에 소켓을 닫는다.
- 본문 128KiB, 동시 HTTP 요청 8개, 동시 저장소 작업 4개. 헤더 이후 HTTP 처리 제한 10초. 초과 요청은 413/429 또는 도구 오류를 반환한다.
- 목록은 최대 366일·100개. `truncated=true`이면 날짜 범위를 좁힌다. 메모는 상세 조회로 읽는다. 제목은 200자, 메모는 20,000자.
- 이미 시작한 저장 작업은 요청 연결이 끊겨도 완료될 수 있다. 완전 종료 시 HTTP 요청에 최대 1초, 저장소 작업 정리에 최대 5초를 기다린다. 불완전한 헤더/본문을 보낸 클라이언트가 종료를 무한히 붙잡지 못한다. 응답 없는 쓰기는 재실행 전에 저장 결과를 확인한다.

## 검증 범위

자동 검증에는 공통 저장소 왕복·범위/개수 제한·두 작성자의 동시 수정·부분 수정·메모 생략·오래된 GUI 초안 거절, 실제 TCP의 인증/Origin/Host/초기화/도구 호출/큰 본문/종료/포트 충돌, DPAPI 재사용, 앱 갱신 시 초안 보존을 포함한다. 설정 스크립트도 분리된 fixture로 확인한다. 실제 Codex/Claude Code 제품 연결과 사용자 화면 테스트, 유휴 추가 작업집합 20MiB·CPU 1코어 평균 0.1% 이내 목표는 별도 실측/사용자 확인 결과를 따른다. 목표 수치를 보장하지 않는다.
