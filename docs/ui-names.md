# TimeSchedule 화면별 명칭표

대표 Windows 앱(Rust/Iced)의 UI를 가리킬 때 쓰는 이름이다. **부를 이름**은 대화에서 쓰는 표현이고, **화면에서 찾는 법**에는 실제 메뉴 문구나 위치를 적었다. 예를 들어 “월간 다이어리의 선택 날짜 패널”이라고 하면 개발자는 `shell::date_details`를 찾을 수 있다. 사용자가 코드 이름을 알 필요는 없다.

## 다이어리 창과 탐색 메뉴

화면에서 위치를 먼저 찾으려면 [캡처 옆에 표가 있는 PDF](ui-guide/TimeSchedule-UI.pdf) 또는 [확대 가능한 HTML](ui-guide/TimeSchedule-UI.html)을 연다. HTML은 파일을 내려받아 브라우저에서 열면 화면 선택, 번호 강조, 캡처 확대를 사용할 수 있다.

안내서는 네이티브 0.20.0 소스의 캡처 전용 빌드에서 샘플 ‘독서 모임’ 데이터를 열어 촬영했다. 화면과 펼침 상태 33장에 143개의 위치를 표시했다. 각 번호는 해당 장에서 쓰며, “주간 다이어리 ③”처럼 화면 이름과 함께 부른다. 팝업은 내용을 읽을 수 있도록 창 주변을 잘랐다.

| 부를 이름 | 화면에서 찾는 법 · 기능 | 코드에서 찾는 법 |
| --- | --- | --- |
| 다이어리 창 | 창 제목의 ‘만년 다이어리’. 왼쪽 메뉴와 현재 화면 전체를 담는 기본 창 | `app.manager`, `shell::view` |
| 왼쪽 메뉴 | 월간·주간 등 화면을 여는 세로 메뉴. 아래쪽에 테마 전환, 설정, 앱 종료 | `shell::view`의 `navigation`, `sidebar` · `Message::Navigate` |
| 프로젝트 바로가기 | 왼쪽 메뉴 중간의 ‘프로젝트’ 소제목, ＋ 버튼, 개별 프로젝트 이름 | `app.snapshot.projects`, `Message::Project` |
| 상단 검색창 / 검색 결과 | ‘일정, 프로젝트, 기록 검색’에 입력하면 현재 본문 자리에 ‘검색 결과’가 표시됨 | `app.search`, `app.search_results`, `views::search_results` |
| 월간 다이어리 | ‘월간’. 연·월 제목 아래 달력과 선택한 날짜의 일정·기록을 표시 | `Page::Month`, `app.month`, `shell::month` |
| 주간 다이어리 | ‘주간’. 주간 개요와 7개의 날짜 칸, ‘주간 근거 요약’ 버튼 | `Page::Week`, `app.week`, `views::week`, `views::day` |
| 프로젝트 화면 | ‘프로젝트’. ‘모든 프로젝트’ 또는 프로젝트 이름을 골라 기록을 조회. 개별 프로젝트의 기본 버튼은 ‘근거 요약’, ‘자료 발췌 초안’ | `Page::Projects`, `app.project`, `views::projects` |
| 기록 보관함 (현행 화면 식별용) | 왼쪽 메뉴 ‘개발 기록’ → 제목 ‘기록 보관함’. 여러 날짜와 여러 종류의 기록을 조회하는 기존 화면 | `Page::Records`, `views::records`, `app.snapshot.entries` |
| 날짜 미정 화면 | ‘날짜 미정’. 날짜가 비어 있는 일정을 모아 표시. 여기서 ‘+ 일정’을 누르면 날짜를 비워 둘 수 있음 | `Page::Tasks`, `views::tasks`, `Kind::Task` |
| 노트 화면 | 왼쪽 메뉴 ‘노트’ → 제목 ‘메모’. ‘+ 메모’로 추가하고 제목을 눌러 편집 | `Page::Notes`, `views::notes`, `app.snapshot.notes` |
| 설정 화면 | 왼쪽 메뉴 하단 ‘설정’. 위젯 상단의 ‘설정’도 같은 화면으로 이동 | `Page::Settings`, `views::settings`, `widget_settings::Change::Open` |

## 0.30.1 앱과 캡처를 대조하는 법

04번 ‘선택한 프로젝트’ 캡처는 0.20.0 기준이다. 현재 앱에서는 왼쪽에서 프로젝트 이름을 선택한 뒤 아래 문구로 기능을 찾는다.

| 수정 요청에서 가리킬 대상 | 0.30.1 화면에서 찾는 법 | 코드에서 찾는 법 |
| --- | --- | --- |
| 자료 발췌 초안 | ‘근거 요약’ 옆의 ‘자료 발췌 초안’ 버튼. 기록 편집기의 종류 목록에는 ‘포트폴리오 초안’이라는 이름이 남아 있다. | `views::projects`, `Message::Draft(true)`, `views::KIND_LABELS` |
| 로컬 채팅 자동 수집 | 저장소가 연결된 프로젝트에서 ‘Codex·Claude Code 채팅 자동 수집 켜기’ 또는 ‘채팅 자동 수집 끄기’ | `views::projects`, `Message::SetChatCapture` |
| 채팅에서 경험 검토 | 수집된 채팅이 있으면 ‘채팅에서 경험 검토 · Terra’에서 채팅을 선택한다. 설계·트러블슈팅 후보에 체크한 뒤 ‘확인한 후보만 이력에 저장’을 누른다. | `chat_review_ui::panel` |
| 경험 선택과 Markdown 생성 | ‘포트폴리오 경험’에서 ‘경험 선택’으로 기록을 고른다. ‘백엔드 포트폴리오 · Markdown 생성’ 버튼 옆에 ‘기존 v2 재사용 완료’처럼 실제 판본 번호가 표시된다. | `portfolio_ui::panel`, `portfolio_ui::Action::Export` |

30~31번 설정 캡처도 0.20.0 기준이다. 0.30.1의 ‘설정 → 로컬 일정 MCP’에는 ‘MCP 켜기’와 ‘MCP 끄기’가 있다. 설정에서 전원 제어를 찾을 때는 `views::settings`와 `Message::SetMcpEnabled`를 참고한다.

## 달력, 위젯과 날짜별 보기

| 부를 이름 | 화면에서 찾는 법 · 기능 | 코드에서 찾는 법 |
| --- | --- | --- |
| 월간 달력 날짜 칸 | 월간 다이어리의 날짜 숫자와 그날의 항목. 맨 왼쪽 ‘주’ 열의 화살표로 주간 다이어리를 엶 | `shell::calendar`, `app.selected`, `Message::OpenWeek` |
| 선택 날짜 패널 | 월간 달력 오른쪽의 ‘○월 ○일’, ‘+ 일정’, ‘+ 기록’ 영역. 창이 좁으면 달력 아래에 표시 | `shell::date_details`, `app.selected` |
| 하루 정리 만들기 버튼 | 선택 날짜 패널의 ‘+ 기록’ 옆. 설정의 하루 정리 방식이 ‘버튼으로 만들기’일 때만 보이고, 고른 날짜의 하루 정리를 만듦 | `shell::date_actions`, `Message::MakeDaily` |
| 주간 날짜 칸 | 주간 다이어리에서 각 날짜의 일정과 기록을 스크롤하는 칸 | `views::day`, `Message::SelectDay` |
| 바탕화면 위젯 | Windows 바탕화면에 붙는 달력. 자체 연·월 제목, 오늘·다이어리·설정 버튼과 날짜 칸이 있음 | `app.widget`, `app.widget_month`, `desktop_widget::view` |
| 위젯 이동·크기 손잡이 | 위젯의 ‘이동’ 손잡이와 모서리의 ↘ 손잡이 | `Message::DragWidget`, `Message::ResizeWidget` |
| 연·월 이동 창 | 월간 다이어리 또는 위젯의 연도·월 제목을 누르면 열림. 어느 달력을 이동할지 출발 창으로 구별 | `month_jump::view`, `app.widget_ui.jump`, `JumpTarget::Diary`, `JumpTarget::Widget` |
| 빠른 일정 창 | 위젯 날짜 칸을 두 번 누르거나 다이어리의 ‘전체 …개 · 완료 …개’를 누르면 열리는 날짜별 목록 창. 창 제목은 ‘날짜 · 빠른 일정’ | `app.quick_window`, `app.quick_date`, `quick_editor::view` |
| 완료 항목 목록 | 빠른 일정 창의 ‘완료 …개’를 펼친 목록. 완료 항목 전용 이전·다음 버튼이 있음 | `app.calendar.show_completed`, `app.calendar.completed_page` |
| 항목 상세 보기 | 달력 목록에서 개별 항목을 열어 본문을 읽는 화면. 아래에 ‘수정’과 ‘닫기’가 있음 | `app.calendar.detail`, `calendar_ui::detail_view` |
| 프로젝트 기록 묶음 | 날짜 칸의 프로젝트 묶음을 열면 해당 날짜·프로젝트의 기록이 카드로 표시됨 | `app.calendar.group`, `calendar_ui::group_view` |
| 기록 카드 | 프로젝트 화면·기록 보관함에서 기록 하나를 펼쳐 본문을 읽는 항목 | `views::entry_card`, `app.expanded`, `Message::ToggleEntry` |
| 카테고리 관리 | 월간·주간의 ‘카테고리’. 이름·색상·표시 순서 변경, 삭제할 때 연결 기록 처리 | `app.categories`, `category_ui::view`, `category_ui::delete_view` |

## 추가·수정 편집기

다이어리와 빠른 일정 창은 같은 편집기를 사용한다. 수정 요청에서는 “기록 편집기의 짧은 작업명”처럼 **편집 대상과 필드**를 함께 부르면 된다. 공통 화면은 `views::editor_view`, 열려 있는 편집기는 `app.editor`, 입력값은 `editor::Form`에 있다.

| 부를 이름 | 화면에서 찾는 법 · 기능 | 코드에서 찾는 법 |
| --- | --- | --- |
| 일정 편집기 | 달력·빠른 일정 창의 ‘+ 일정’ 또는 기존 일정 수정. 날짜, 하루 종일, 시작·종료 시간, 완료 | `Kind::Event`, `form.date`, `form.all_day`, `form.start`, `form.end`, `form.completed` |
| 날짜 미정 일정 편집기 | 날짜 미정 화면의 ‘+ 일정’. 제목은 ‘일정 추가’이고 날짜는 선택 입력 | `Kind::Task`, `form.date` |
| 기록 편집기 | ‘+ 기록’ 또는 기록 수정. 짧은 작업명, 카테고리, 기록 종류, 프로젝트, 출처 링크 | `Kind::Entry`, `form.short_title`, `form.category_id`, `form.entry_kind`, `form.project_id`, `form.source_url` |
| 프로젝트 편집기 | ‘+ 프로젝트’ 또는 ‘프로젝트 수정’. 프로젝트 이름·설명, 달력용 짧은 프로젝트명, 로컬 Git 저장소 경로 | `Kind::Project`, `form.title`, `form.body`, `form.project_short_name`, `form.repo_path` |
| 메모 편집기 | 노트 화면의 ‘+ 메모’ 또는 기존 메모 제목. 제목과 본문 편집 | `Kind::Note`, `form.title`, `form.body` |
| 편집기 날짜 입력 / 날짜 선택기 | 연·월·일을 나눠 입력하는 날짜 필드와 달력 아이콘으로 여는 선택기 | `date_input::input`, `EditorPicker::Date`, `Message::PickEditorDate` |
| 편집기 시간 선택기 | ‘하루 종일’을 해제했을 때의 시작·종료 시간 입력과 선택 목록 | `time_input::view`, `Message::OpenTimePicker`, `Message::PickEditorTime` |
| 편집기 닫기 확인 / 삭제 확인 | ‘저장하지 않고 닫을까요?’ 또는 ‘이 항목을 삭제할까요?’가 표시되는 확인창 | `Confirmation::Discard`, `Confirmation::Delete` |

## 설정 안의 영역과 Windows 메뉴

| 부를 이름 | 화면에서 찾는 법 · 기능 | 코드에서 찾는 법 |
| --- | --- | --- |
| 화면 모드 | 설정의 ‘화면 모드’ 또는 왼쪽 메뉴의 밝은 모드·어두운 모드 전환 | `app.preferences.theme`, `shell::theme_toggle` |
| 다이어리 저장 위치 | 설정의 ‘내 컴퓨터에 보관하는 다이어리’ 아래 데이터 저장 경로 | `app.data_dir`, `views::settings` |
| 하루 정리 만드는 방식 | 설정의 ‘하루 정리 만드는 방식’. ‘버튼으로 만들기’(기본)와 ‘자동으로 만들기’ 중 하나를 고름 | `app.preferences.daily_record`, `views::daily_mode_choice`, `Message::SetDailyMode` |
| 로컬 일정 MCP 연결 설정 | 설정의 ‘로컬 일정 MCP’. 서버 상태, 앱별 연결 현황, 토큰 관리, 연결 설정 | `app.mcp_setup`, `mcp_setup_ui::board`, `mcp_setup_ui::token_panel`, `mcp_setup_ui::view` |
| 위젯 표시·위치 설정 | 설정의 ‘바탕화면 위젯’. 연결 상태, 표시·숨기기, 위치 복구·잠금, 배경 불투명도 | `views::settings`, `NativeAction`, `app.preferences.widget_opacity` |
| 위젯 모양 설정 | 같은 설정 화면의 ‘크기와 표시 기간’, ‘글꼴과 간격’, ‘위젯 색상’ | `widget_settings::view`, `app.widget_ui`, `app.preferences.widget` |
| 문서 저장소 안내 | 설정 하단의 ‘문서 저장소와 외부 연결’. 자동 개발 기록의 보관 경로와 외부 연결에 관한 설명 | `views::settings` · [문서 저장소 동작](document-vault.md) |
| 트레이 메뉴 | Windows 알림 영역의 TimeSchedule 아이콘 메뉴. 다이어리 열기, 위젯 표시·숨기기·위치 복구, 앱 종료 | `NativeTray`, `TrayAction` |

## ‘개발 기록’이라고 했을 때

| 가리킬 수 있는 대상 | 확인할 위치 |
| --- | --- |
| 왼쪽 ‘개발 기록’ 메뉴 | 현재 ‘기록 보관함’으로 들어가는 메뉴(`Page::Records`). 사용자는 개발 기록 전용 란을 의도하지 않았다. 메뉴의 향후 처리는 별도 확인 사항이다. |
| 기록 종류 ‘개발 기록’ | 기록 편집기의 ‘기록 종류’ 선택값(`form.entry_kind`의 `development`). ‘자유 노트’, ‘트러블슈팅’ 등과 같은 목록에 있다. |
| 자동 개발 기록 | 커밋한 작업을 로컬 문서와 다이어리에 보관하는 절차. [자동 개발 기록 절차](project-records.md) |

예를 들어 “개발 기록 고쳐줘”라면 “왼쪽 메뉴, 기록 편집기의 종류 선택값, 커밋 뒤 자동 저장 중 어디를 말씀하시나요?”라고 묻는다. 새 란이 필요해 보이면 위치·이름·용도를 제시하고 답을 기다린다.

화면 구현은 [창·페이지 상태](../apps/timeschedule-native/src/app.rs), [다이어리 메뉴와 월간 배치](../apps/timeschedule-native/src/shell.rs), [페이지·편집기·팝업 분기](../apps/timeschedule-native/src/views.rs)에서 확인할 수 있다.
