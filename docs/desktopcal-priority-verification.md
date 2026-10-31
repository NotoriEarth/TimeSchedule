# DesktopCal 우선 기능 3개 검증

2026-10-31, `codex/desktopcal-priority-features`. 기반은 `codex/desktopcal-widget`의 `1453f4b`이며 기존 위젯 PR 위에 쌓는 변경이다. Rust/Iced 앱에 적용하며 기존 Tauri 앱은 별도로 유지한다.

## 적용 내용

1. **날짜 더블클릭 빠른 편집**: 큰 다이어리 창 대신 440×480 크기의 별도 창을 연다. 빈 날짜는 추가 폼, 일정 하나인 날짜는 수정 폼, 여러 개인 날짜는 선택 목록을 보여준다. 기존 제목·본문·시간·색상·완료 상태와 편집 보호를 재사용한다.
2. **위젯에서 완료 체크**: 보이는 일정의 체크박스로 완료를 변경한다. 저장소에서는 완료와 수정 시각만 갱신하므로, 목록용 요약 데이터에 없는 본문이 지워지지 않는다. 저장 후 다이어리와 위젯을 갱신한다.
3. **배경만 반투명**: 기존 40–100% 조절값을 날짜 칸과 헤더·푸터 배경에 적용한다. 글자와 체크박스는 창 전체 투명도의 영향을 받지 않는다. Midnight/Porcelain 두 테마를 지원한다.

빠른 편집창은 한 개만 열며 다른 창에서 작성 중인 내용을 덮어쓰지 않는다. 저장 중 닫기, 미저장 변경, 한글 조합 상태, 오래된 비동기 응답을 기존 편집 상태와 함께 보호한다. 위젯 체크박스는 비활성 상태에서도 클릭을 소비해 날짜 더블클릭으로 전달하지 않는다. 완료 저장과 목록 갱신 중에는 이전 데이터로 편집을 시작하지 못한다.

Windows 출력은 softbuffer 0.4.8을 고정해 네이티브 위젯 HWND만 픽셀 알파 출력에 참여시킨다. 다른 창은 기존 RGB 출력을 유지한다. 변경 범위·원본 라이선스·업그레이드 주의점은 [패치 기록](../vendor/softbuffer/TIMESCHEDULE-PATCH.md)에 있다. DesktopCal의 코드나 이미지 리소스를 앱에 복사하지 않았다.

## 확인 결과

| 확인 항목 | 결과와 범위 |
| --- | --- |
| `cargo test --workspace --locked --offline` | 70개 통과, 2개 의도된 제외: 자식 프로세스용 fixture와 PNG 미리보기 생성기 |
| Windows 별도 fixture | `cargo test -p timeschedule-desktop --features test-support --test desktop_native --locked --offline` 1개 통과 |
| 실제 softbuffer 출력 fixture | workspace 테스트에 포함. layered 자식 창 출력, 크기 변경, opt-in 없는 RGB 출력, 잘못된 스타일 오류를 검증 |
| 배경·문자 알파 | 실제 Iced/tiny-skia 렌더러에서 두 테마의 40% 배경과 불투명 글자 픽셀을 구분하여 검증 |
| 마우스 이벤트 | 실제 Iced 트리에 이벤트를 보내 단일 클릭·더블클릭·활성/비활성 체크박스 클릭의 메시지를 검증 |
| 저장·편집 보호 | 완료 변경 시 본문 등 보존, 저장 실패 시 초안 유지, 오래된 로드 무시, 변경·조합 중 닫기 보호, 완료 저장 중 이전 데이터 편집 방지 |
| 실제 렌더러 PNG | 제외된 미리보기 테스트를 별도 실행해 1개 통과. 작은 편집창·최소 크기·확인창과 위젯을 직접 이미지로 확인 |
| 형식·공백 검사 | `cargo fmt --all --check`, `git diff --check` 통과 |
| 최종 릴리스 | `cargo build -p timeschedule-native --release --locked --offline` 성공 |
| 별도 데이터 실행 | 최종 EXE를 테스트 전용 DB의 일정 2개로 실행. startup 오류 없음, `attached=true`, `visible=Some(true)` 확인 |
| 독립 코드 리뷰 | 두 데이터/입력 경계 문제를 재현 테스트와 함께 수정하고 재검토 완료 |

PNG는 실제 네이티브 렌더러 출력이지만 **Windows 바탕화면 캡처는 아니다**. 자동화 도구의 창 캡처가 두 차례 시간 초과하여, 실제 Windows 합성 결과·클릭·한글 IME·다중 모니터/DPI 조작은 확인하지 못했다. 연결 성공 로그와 자동 이벤트 테스트를 이 수동 검증의 대체로 간주하지 않는다. 기존 실행 중인 사용자 앱과 데이터는 건드리지 않고 테스트 앱만 별도 데이터로 실행했다.

이번 변경의 RAM·유휴 CPU 사용량은 다시 측정하지 않았다. 아래 EXE 크기는 메모리 사용량을 뜻하지 않는다.

## 테스트 실행 파일

- 파일: `artifacts/TimeSchedule-desktopcal-priority.exe`
- 크기: 5,621,760바이트 (5.36MiB)
- SHA-256: `F308A17F8FD971A2E9DFA60321AE1B5BEE425DC202023FE6A815053ACF1F9B0B`
- 빌드 로그: `artifacts/priority-final-release.log`
- 테스트 로그: `artifacts/priority-reviewed-tests.log`, `artifacts/priority-windows-fixture.log`, `artifacts/priority-preview.log`
- 렌더러 미리보기: `artifacts/priority-previews/`

실행 파일·로그·테스트 데이터는 로컬 산출물이며 Git에 포함하지 않는다. 소스에서 재현하려면 `scripts/toolchain.ps1`을 불러온 뒤 위 검증 명령을 실행한다. PNG 생성은 `TIMESCHEDULE_PREVIEW_DIR`을 설정하고 `cargo test -p timeschedule-native render_theme_previews --locked --offline -- --ignored`를 실행한다.

## 직접 확인할 순서

1. 기존 Rust 네이티브 앱을 트레이에서 완전히 종료하고 새 EXE를 실행한다. 기본 실행은 기존 네이티브 데이터 폴더를 사용하며 Tauri DB를 자동으로 가져오지 않는다.
2. 바탕화면 날짜의 빈 부분을 한 번 누르면 편집창이 열리지 않고, 더블클릭하면 작은 창이 열리는지 확인한다. 빈 날짜·일정 하나·여러 개를 각각 확인한다.
3. 제목·메모·시간·색상을 입력해 저장하고, 다시 열어 유지되는지 확인한다. 한글 조합 중 Enter/Esc, 수정 후 닫기와 **계속 작성**도 확인한다.
4. 위젯의 일정 체크박스로 완료·해제를 반복한다. 편집창이 새로 열리지 않고 큰 다이어리에도 반영되는지 확인한다. 완료 후 다시 편집해 메모가 보존되는지도 확인한다.
5. 배경을 40%와 100%로 바꾸고 글자는 선명하게 유지되는지 확인한다. 밝은 모드에서도 반복한다. 위젯 이동·잠금·Win+D·다른 창에 가려짐도 함께 확인한다.

기존 데이터와 분리해서 확인하려면 PowerShell에서 다음처럼 실행할 수 있다.

```powershell
$env:TIMESCHEDULE_DATA_DIR = 'C:\LSH\TimeSchedule\.test-data\priority-user-test'
& 'C:\LSH\TimeSchedule\artifacts\TimeSchedule-desktopcal-priority.exe'
```

사용자가 준비된 변경을 직접 테스트하고 명시적으로 승인하기 전에는 PR을 머지하지 않는다.
