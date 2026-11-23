# 개인 문서 저장소

TimeSchedule 0.13.0은 서버 없이 **프로젝트와 공부 자료를 한 로컬 폴더에 보관**한다. 앱 코드는 프로젝트에 두고 실제 자료는 사용자 Documents의 `TimeScheduleData`에 둔다. Node.js가 문서 명령 실행에 필요하며, 기본 일정·다이어리 사용에는 필요 없다. 문서 저장은 Codex 로그인이나 AI 모델을 쓰지 않는다.

## 위치 확정과 상태 확인

프로젝트 폴더의 PowerShell에서 한 번 실행한다. 사용자가 고른 위치가 `.cache/project-records/vault-settings.json`에 저장되어 CLI와 앱의 대기 기록 재시도에 적용된다. 경로 변경은 새 기록의 목적지만 바꾸며 기존 자료를 자동으로 이동하지 않는다.

```powershell
node scripts/document-vault.mjs init --root "$env:USERPROFILE\Documents\TimeScheduleData"
node scripts/document-vault.mjs status
```

`init`은 빈 폴더 또는 이미 초기화된 저장소만 사용하며 반복 실행해도 문서를 지우지 않는다. 사용자 Documents가 다른 드라이브로 이동된 PC에서는 실제 경로를 `--root`에 지정한다. 파일 탐색기에서 선택한 폴더를 열면 된다. `status`는 실제 루트, 문서 경로·분류·수정 버전·저장 상태를 표시한다. 폴더가 없을 때 상태 조회만으로 새로 만들지 않는다.

```text
TimeScheduleData/
  vault.json                         저장소 ID·형식 버전
  README.md                          저장소 자체 사용 안내
  projects/<분류 ID>/<문서 UUID>/
    record.md                        사람이 읽는 원문
    metadata.json                    제목·날짜·태그·출처·수정 버전
    history/1.json                   원문을 포함한 이전 수정본
    attachments/                     직접 보관하는 첨부 자료
  studies/<분류 ID>/<문서 UUID>/      공부 자료, 동일한 구조
  .sync/catalog/                     문서 위치 목록
```

자동 개발 기록의 분류 ID는 저장소 경로의 해시이며 실제 프로젝트명·저장소 경로는 메타데이터의 `source`에 보존한다. 서로 다른 저장소의 같은 프로젝트명이 충돌하지 않는다. 문서명은 UUID로 안정적으로 유지되므로 제목을 바꿔도 경로는 유지된다. 첨부 파일은 현재 자동 수집·서버 전송하지 않는다.

## 공부 자료와 다른 프로젝트 문서 가져오기

일반 문서는 명시적으로 가져오며 Git 커밋이 없어도 된다. 예를 들어 다음 JSON을 `.cache/study-import.json`에 UTF-8로 저장한다. 새 문서는 새 UUID, 같은 문서의 갱신은 같은 UUID를 사용한다.

```json
{
  "id": "f16531cf-6c03-4b61-8f87-63b534b36085",
  "collection": "studies",
  "group": "rust",
  "title": "Rust 소유권 공부",
  "date": "2026-11-16",
  "tags": ["rust", "ownership"],
  "markdown": "# 소유권\n\n오늘 이해한 내용과 아직 모르는 내용을 적습니다.\n"
}
```

```powershell
node scripts/document-vault.mjs import --input .cache/study-import.json
node scripts/document-vault.mjs status
```

`collection`은 `projects` 또는 `studies`, `group`은 영문 소문자·숫자·하이픈으로 만든 분류 ID다. 다른 프로젝트는 `projects`와 해당 분류 ID를 사용한다. 같은 UUID로 분류·위치를 바꾸려 하면 기존 문서를 보존하고 거절한다. 제목·태그·원문 변경은 수정 이력에 남는다. 동일한 입력의 반복 실행은 새 수정본을 만들지 않는다. 본문은 2MB 이하, 태그는 최대 40개다.

기존 Markdown 파일을 가져오려면 PowerShell에서 JSON을 만들 수도 있다.

```powershell
$document = @{
    id = [guid]::NewGuid().ToString()
    collection = 'projects'
    group = 'my-project'
    title = '설계 기록'
    date = '2026-11-16'
    markdown = Get-Content -LiteralPath 'C:\자료\설계.md' -Raw -Encoding utf8
}
$document | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath '.cache/document-import.json' -Encoding utf8
node scripts/document-vault.mjs import --input .cache/document-import.json
```

**일반 import는 파일 저장만 하며 다이어리에 자동 반영하지 않는다.** 현재 자동 개발 기록은 이 TimeSchedule 저장소의 커밋을 검사한다. 다른 프로젝트는 일반 import로 자료를 모을 수 있고, 각 프로젝트의 자동 수집 연결은 별도 작업이다.

## 직접 편집과 충돌

`record.md`를 편집기로 직접 수정해도 다음 가져오기가 조용히 덮어쓰지 않는다. `status`의 `edited_or_deleted`는 직접 편집 또는 삭제가 감지됐다는 뜻이다. 수정한 원문을 새 수정본으로 확정하려면 실행한다.

```powershell
node scripts/document-vault.mjs adopt --id f16531cf-6c03-4b61-8f87-63b534b36085
```

`adopt`는 현재 Markdown을 검증해 수정 이력에 남긴다. 이후 이 문서는 수동 관리 문서로 표시되어 다른 자동 입력으로 덮어쓰지 않는다. 계속 `record.md`를 편집하고 `adopt`하면 된다. 삭제된 파일을 자동 복구하지 않으며 필요한 원문은 `history`에서 확인해 직접 복원한 뒤 `adopt`한다. `metadata.json`, `history`, `.sync`는 일반 편집 대상이 아니다.

저장 중 중단되면 `.pending.json`으로 복구 의도를 보존한다. 같은 입력을 다시 가져오면 안전하게 완료한다. 그 사이 원문이 직접 바뀌었다면 자동 덮어쓰지 않고 충돌로 멈춘다. `recovery_pending`은 완료가 아니라 확인이 필요한 상태다.

다이어리에 표시되는 자동 개발 기록은 **문서 원문의 표시본**이다. 다이어리에서 직접 수정한 내용과 파일 편집을 자동 왕복 동기화하지 않는다. 각각의 사용자 편집은 보존하며 다음 자동 반영에서 충돌을 표시한다. 일반 공부 문서 열람·편집 GUI와 양방향 동기화는 이번 버전에 포함하지 않는다. 기존 일정·메모·다이어리 DB는 그대로 유지한다.

## Space 게시 제거 (0.32.0)

0.32.0에서 Space 게시를 없앴다. `config --space` 명령은 없고, 자동 개발 기록은 문서 저장과 다이어리 반영으로 끝난다. 이미 게시된 페이지는 지우지 않는다. 그 링크가 붙어 있던 다이어리 항목도 그대로다.

`vault-settings.json`의 `spaceEnabled`는 읽지 않는다. 0.32.0 이전 코드를 체크아웃해 실행하면 그 코드는 이 값이 없는 설정 파일을 거절하고 `true`이면 게시한다. 그래서 `init`은 설정 파일을 쓸 때 `spaceEnabled: false`를 함께 쓴다. 자세한 내용은 [자동 개발 기록](project-records.md)의 "Space 게시 제거"에 있다.

## 백업과 향후 서버

전체 `TimeScheduleData` 폴더를 백업하면 원문·메타데이터·수정 이력·첨부 자료를 함께 보존할 수 있다. 동기화는 삭제도 전파할 수 있으므로 과거 버전을 보존하는 백업과 구분한다. `.cache/project-records`에는 다이어리 전달 상태와 저장 경로 설정이 있으므로 대기 작업이 있다면 함께 보존한다. 기록 작성 중인 파일의 단순 복사 대신 명령이 끝난 시점에 백업한다.

저장소 형식 버전, 고정 ID, 수정 버전, 내용 해시를 이미 보관한다. 후속 서버 연동은 이 문서 단위로 변경분을 보내는 프로그램을 붙일 수 있으며, 현재 PC의 로컬 저장을 서버 가용성에 의존시키지 않는다. 이번 버전에는 서버·기기 간 전송·상주 프로그램·AI 검색이 없다.

## 검증과 사용자 확인

격리된 임시 저장소로 초기화 반복, 프로젝트·공부 import, 재가져오기, 수정 이력, 직접 편집·삭제 충돌, adopt, 경로·링크 거절, 중단 복구, Space 게시 제거 이전에 쓴 설정 파일 읽기를 테스트한다. 실제 사용자 문서로 자동 테스트하지 않는다.

사용자는 `status`의 루트를 확인하고 새 공부 문서를 import한 뒤 반환된 `record.md`를 열어 원문을 확인한다. 파일을 직접 수정하고 adopt한 뒤 수정 버전 증가와 history 보존을 확인한다. 앱의 새 자동 개발 기록은 원문 뒤에 `상태:` 줄로 끝난다. 실제 사용자 확인과 미래 서버 전송은 자동 테스트 결과와 별개다.
