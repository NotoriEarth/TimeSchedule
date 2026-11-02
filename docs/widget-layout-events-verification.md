# 0.9.1 설정 배치·위젯 화면 변경 검증

## 변경 동작

설정의 위젯 버튼, 배경 불투명도 슬라이더, 크기 설정은 Windows 작업 전·중·후 계속 표시한다. 연결/오류 문구는 높이 52px의 고정 스크롤 영역에, 사용자 작업 중 문구는 높이 20px 영역에 표시한다. 사용자 native 요청 중에는 관련 버튼과 크기 입력만 비활성화한다. 배경 opacity는 화면 선호 설정이므로 계속 조절할 수 있다.

기존 1초 Position 요청을 제거했다. `WM_ENTERSIZEMOVE`에서 시작 geometry를 보관하고 `WM_EXITSIZEMOVE`에서 실제로 바뀐 위치/크기만 선호 값으로 갱신한다. 중간 이동이나 자동 화면 보정은 선호 설정을 바꾸지 않는다. 기존 500ms Tick은 트레이·MCP·개발 기록 기능을 유지하며 geometry에 대해서는 메모리 알림만 소비한다. 종료/잠금 요청 전에도 대기 알림을 소비한다.

앱의 기존 최상위 관리 창에 subclass를 설치해 `WM_DISPLAYCHANGE`, `WM_SETTINGCHANGE/SPI_SETWORKAREA`, DPI 알림을 받는다. 관리 창이 트레이에 숨겨져도 같은 HWND를 유지한다. 위젯의 200ms 단발 타이머로 연속 알림을 합친 뒤 보정한다. 알림 누락용 보조 타이머는 30초다. 표시/수동 복구에는 즉시 보정한다. 새 프로세스·스레드·전역 입력 감시는 없다.

기존 `widget_x/y`는 선호 물리 좌표, `widget.width/height`는 선호 논리 크기로 계속 사용한다. native adapter의 현재 사각형과 분리하므로 임시 화면 축소·작업영역 변화가 파일을 덮어쓰지 않는다. 영역이 돌아오면 선호 값으로 복구하며, 숫자 크기 적용은 화면에 제한된 결과 대신 요청한 논리 크기를 저장한다. 새 설정 필드나 데이터 이전은 없다.

## 최소 adapter 계약

- `WidgetGeometry { x, y, width, height }`: 물리 화면 위치와 96-DPI 논리 크기.
- `configure_geometry(&NativeWindow, WidgetGeometry)`: UI thread에서 연결된 소유 창의 선호 값을 구성하고 현재 작업영역·부모 client 영역에 맞춘다. 실패하면 이전 선호 값으로 되돌린다.
- `observe_display_changes(&NativeWindow)`: 앱의 기존 최상위 창에 화면 알림 observer를 설치한다. 외부 창이나 위젯 자식 창에는 설치하지 않는다.
- `take_geometry_events()`: HWND/모니터 조회 없이 메모리 알림을 소비한다. 연결된 HWND마다 최대 한 항목이며, 뒤따른 자동 보정이 미소비 사용자 선호 값을 지우지 않는다. detach/NCDESTROY에서 기록과 대기 알림을 제거한다.
- `GeometryEvent`: 사용자 변경인 경우에만 `preferred`, 현재 논리 `actual_size`, 실패 시 `error`. native callback은 설정 파일/DB 저장이나 로그 작업을 하지 않는다. mutex는 SetWindowPos 같은 재진입 호출 전에 해제한다.

## 확인한 실패와 회귀 검증

기존 코드에서 실제 `views::settings`와 Iced renderer/tree를 반복 전환한 결과, 620×480 Midnight에서 busy 전환으로 전체 스크롤 content 높이가 1411.0001→1331.5px로 줄었다(약 79.5px). 버튼/슬라이더가 제거되고 아래 크기 설정이 함께 이동하는 실패를 확인한 뒤 수정했다.

- 실제 retained tree에서 정상→작업 중→완료→오류/작업 중→오류/완료 전환, 620×480 및 1160×820, 두 테마를 검사한다. 컨트롤 좌표/높이, 상태 영역 사각형, 바깥 스크롤 content/offset이 유지된다. 정상 WorkerW 문구와 4줄 오류를 모두 사용한다. 오류는 같은 영역 안에서 스크롤 가능하다.
- 두 테마의 620×480 renderer 이미지는 동일 retained tree의 스크롤 offset 460에서 생성했다. frame0=정상, 1=작업 중, 2=완료, 3=오류/작업 중. 실제 데스크톱 캡처가 아닌 Iced 소프트웨어 renderer 결과다.
- 소유한 숨김 Win32 parent/child fixture로 축소 60×40 후 선호 80×60이 복원되지 않는 실패, 이동 종료 알림 부재, 실패한 크기 적용의 선호 오염을 먼저 확인했다. 수정 후 실제 사각형으로 검증했다.
- 소유한 최상위 HWND에 display/workarea/DPI 메시지를 20회씩 전달하고 실제 PeekMessage/DispatchMessage 루프로 처리했다. 60ms에는 보정하지 않고 360ms 안에는 보정하며, 30초를 기다리지 않는다. 잠김·숨김 상태, 축소→확장, 임시 축소 상태의 사용자 이동, 자동 보정 뒤에도 대기 사용자 알림 유지, adapter 재구성 후 복구, HWND 파괴 시 대기 이벤트 제거를 검사한다. 시스템 모니터 설정과 Explorer는 조작하지 않는다.
- 앱 idle Tick 120회에 native busy/Position 요청이 생기지 않는다. unlocked Iced Resized와 Attach/Show/Recover/Lock 보고가 선호를 오염시키지 않는다. 실제 임시 preferences.json 저장·load로 재시작 보존을 확인하며, 같은 최종 geometry는 추가 저장을 예약하지 않는다. 기존 음수 물리 좌표 clamp와 DPI 크기 제한 테스트를 유지한다.

로컬 원본 증거는 `.cache/widget-layout-events/`의 `layout-red.log`, `native-red.log`, `native-events-red.log`, `idle-red.log`, `preferences-red.log`, `configure-failure-red.log`, `status-width-red.log`, 최종 테스트 로그와 `images/`에 있다. manifest는 작업 인계 시 이 변경 경로와 산출물 SHA256을 고정한다. 테스트의 RED 로그는 실제 실패 증거이므로 보존한다.

## 독립 검토 후 전체 shell 오류 전환 보강

첫 검증은 `views::settings` 내부를 그렸다. 독립 검토에서 실제 geometry/NativeDone 오류가 `app.error`에 들어가면 외부 shell 경고 배너가 추가되는 경로를 발견했다. 이를 실제 오류→앱 상태→`app.view(manager)`로 재현했으며 800×620 Midnight에서 설정 viewport가 y125.0→184.2로 59.2px 이동하고 높이가 477.0→417.8로 줄었다. 원본은 `full-shell-red.log`에 보존한다.

위젯의 geometry·native 작업·display observer 오류를 출처별 UI 상태로 분리했다. 설정에서는 기존 52px 상태 영역에 표시하고, 다른 페이지에서는 오류 배너에 계속 표시한다. 다이어리/저장 등 기존 `app.error`를 지우거나 숨기지 않는다. geometry 정상 이벤트는 이전 geometry 오류만 해제하고 복구 상태를 표시한다. native 작업과 observer 오류도 각각의 성공만 해제한다. native adapter나 이벤트/타이머 경로는 바꾸지 않았다.

full-shell retained tree 테스트는 두 테마, 800×620/1160×820, 세 오류 출처, 별도 앱 오류 유무 각각에서 정상→오류→정상→오류→정상을 반복한다. 설정 viewport/상태 viewport/전체 컨트롤 좌표/스크롤 offset이 유지되고 실제 오류 본문이 표시되며 복구 후 stale 본문이 사라지는지 확인한다. 다른 페이지에서 세 오류가 동시에 표시되고 geometry 복구가 native/observer/다이어리 오류를 지우지 않는지도 검사한다.

`images-v2/full-settings-{Midnight,Porcelain}-{0,1,2}.png` 6장은 실제 전체 shell의 정상·geometry 오류·복구 renderer 출력이다. 물리 사용자 창 캡처는 아니다. `full-shell-green.log`, `native-error-sources.log`, `workspace-tests-v2.log`가 영향 수정의 근거이며 최초 manifest/RED 기록도 그대로 보존한다. Node와 native adapter의 기존 검증은 변경 범위가 없어 재사용한다.

## 설정에서 검색한 동안의 오류 가시성

v2 영향 검토에서 Settings가 선택돼 있어도 검색어가 있으면 실제 내용은 검색 결과로 교체되는 경로를 발견했다. native 배너를 생략하는 조건도 `page == Settings && search.trim().is_empty()`로 맞췄다. 검색 결과와 검색 로직 자체는 변경하지 않았다.

실제 `Message::Search` 후 geometry 오류를 전달한 전체 view에서 오류 텍스트가 0회(기대 1회) 표시되는 RED를 `settings-search-red.log`에 보존했다. 수정 후 두 테마와 geometry/native/observer 세 출처에서 검색 중·검색 완료 모두 오류가 1회 표시된다. 검색을 빈 문자열 또는 공백으로 비우면 고정 설정 영역으로 돌아와 오류를 1회 표시하며, 중복 배너와 별도 앱 오류 손실이 없다. `settings-search-green.log`와 v3 native 테스트/빌드 로그가 근거다. 제품 수정은 shell 조건 한 곳이며 native adapter·Node 검증은 기존 결과를 재사용한다.

## 최종 검증 명령

| 명령 | 결과 |
|---|---|
| `cargo test --workspace --locked --offline` (v2) | 136 통과, 2 ignored(core38/desktop13/native85); 기존 수동 preview/성능 테스트는 자동 실행 제외 |
| `node --test tests/*.test.mjs` | 33 통과 |
| `cargo fmt --all --check` / `git diff --check` | 통과; Git의 기존 CRLF 정규화 안내만 있음 |
| `scripts/build-native.ps1 -Offline` (v3) | native86 통과/1 ignored, release 빌드 성공(0.9.1, 8,271,360bytes, 1m38s). 실행 중인 앱이 없어 기존 EXE를 archive에 보존하고 `artifacts/TimeSchedule.exe` 교체 완료. 최종 SHA256은 manifest-v3에 고정 |
| `TIMESCHEDULE_SETTINGS_PREVIEW_DIR`를 지정한 위 retained-layout 테스트 | 두 테마 × 4상태 PNG 생성; 정상/완료 파일은 테마별 SHA256이 동일 |

## 사용자 확인 방법과 한계

1. 현재 앱은 트레이 메뉴에서 직접 완전 종료한 뒤 `artifacts/TimeSchedule.exe`를 실행한다. 빌드 시 기존 앱이 실행 중이면 안전한 교체가 보류되며 별도 제품 이름의 EXE는 만들지 않는다.
2. 설정을 위젯 섹션까지 스크롤하고 표시/숨기기·잠금·복구를 반복한다. 버튼, 불투명도와 크기 입력 위치가 유지되고 정상 상태에 Windows 작업 중이 반복되지 않는지 확인한다.
3. 잠금을 풀고 위젯을 이동/크기 조절한다. 조작 종료 후 재시작해 마지막 선호 배치가 유지되는지 확인한다.
4. 실제 사용하는 다중 모니터/배율 환경에서 작업영역 축소와 복원을 확인한다. 작은 영역에는 표시 geometry만 맞추고 원래 영역이 돌아오면 선호 위치·크기가 복구되는지 본다.

이 작업의 자동 검증은 실제 Iced renderer와 격리된 소유 Win32 창/메시지 경로다. 물리 모니터 연결·분리, 실제 해상도·배율 변경, Explorer 재시작, 사용자 앱의 마우스 드래그 전체 흐름은 아직 사용자 확인 대상이다. Computer Use 창 캡처 timeout은 시각 확인 성공으로 간주하지 않는다. 기존 사용자 앱을 종료하거나 데이터/디스플레이 설정을 변경하지 않았다. 성능 개선 수치는 별도 실측이 없으면 주장하지 않는다. 자동 테스트 통과는 PR 머지 승인이 아니다.
