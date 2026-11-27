# TimeSchedule 화면별 명칭표

대표 Windows 앱(Rust/Iced)의 UI를 가리킬 때 쓰는 이름이다. **부를 이름**은 대화에서 쓰는 표현이고, **화면에서 찾는 법**에는 실제 메뉴 문구나 위치를 적었다. 예를 들어 “월간 다이어리의 선택 날짜 패널”이라고 하면 개발자는 `shell::date_details`를 찾을 수 있다. 사용자가 코드 이름을 알 필요는 없다.

## 다이어리 창과 탐색 메뉴

화면에서 위치를 먼저 찾으려면 [캡처 옆에 표가 있는 PDF](ui-guide/TimeSchedule-UI.pdf) 또는 [확대 가능한 HTML](ui-guide/TimeSchedule-UI.html)을 연다. HTML은 파일을 내려받아 브라우저에서 열면 화면 선택, 번호 강조, 캡처 확대를 사용할 수 있다.

안내서는 네이티브 0.20.0 소스의 캡처 전용 빌드에서 샘플 ‘독서 모임’ 데이터를 열어 촬영했다. 화면과 펼침 상태 33장에 143개의 위치를 표시했다. 각 번호는 해당 장에서 쓰며, “주간 다이어리 ③”처럼 화면 이름과 함께 부른다. 팝업은 내용을 읽을 수 있도록 창 주변을 잘랐다.

| 부를 이름 | 화면에서 찾는 법 · 기능 | 코드에서 찾는 법 |
| --- | --- | --- |
| 다이어리 창 | 창 제목의 ‘만년 다이어리’. 왼쪽 메뉴와 현재 화면 전체를 담는 기본 창 | `app.manager`, `shell::view` |
| 왼쪽 메뉴 | 월간·주간 등 화면을 여는 세로 메뉴. 아래쪽에 테마 전환, 설정, 앱 종료 | `shell::view`의 `navigation`, `sidebar` · `Message::Navigate` |
| 프로젝트 바로가기 | 왼쪽 메뉴 중간의 ‘프로젝트’ 소제목, ＋ 버튼, 개별 프로젝트 이름. 개수 제한 없이 모두 나오며, 프로젝트 화면에서 열린 프로젝트가 강조됨 | `app.snapshot.projects`, `Message::Project` |
| 상단 검색창 / 검색 결과 | ‘일정, 프로젝트, 기록 검색’에 입력하면 현재 본문 자리에 ‘검색 결과’가 표시됨 | `app.search`, `app.search_results`, `views::search_results` |
| 월간 다이어리 | ‘월간’. 연·월 제목 아래 달력과 선택한 날짜의 일정·기록을 표시 | `Page::Month`, `app.month`, `shell::month` |
| 상단 ‘오늘’ 버튼 | 화면 제목 줄 오른쪽, 새로 고침(↻) 왼쪽. 월간·주간 화면에만 보이며 달력을 오늘로 옮김 | `shell::view`의 `heading_row`, `Message::Today` |
| 주간 다이어리 | ‘주간’. 주간 개요와 7개의 날짜 칸, ‘주간 근거 요약’ 버튼 | `Page::Week`, `app.week`, `views::week`, `views::day` |
| 프로젝트 화면 | ‘프로젝트’. 버튼 줄 맨 앞의 프로젝트 선택 상자(‘모든 프로젝트’와 각 프로젝트)로 고름. ‘모든 프로젝트’는 프로젝트 카드 목록(이름, 완료한 프로젝트는 이름 옆 ‘● 완료 · 12월 1일’, 설명, ‘기록 N건 · 마지막 날짜’, 저장소·수집 상태)이고, 카드를 누르면 그 프로젝트가 열림. 모든 기록 목록은 기록 보관함에만 있음. 개별 프로젝트는 맨 위 정보 줄 두 줄을 가짐(완료했고 저장소 경로가 절대 경로인데 그 완료로 만든 프로젝트 기록이 없으면 그 사이에 ‘최종 기록 안내 줄’이 끼어 세 줄): 설명과 ‘프로젝트 수정’·완료 상태(‘● 진행 중’ 또는 ‘● 완료 · 12월 1일’)·‘완료로 표시’/‘진행 중으로’, 그리고 링크가 있을 때만 ‘GitHub’ 버튼(기본 브라우저로 열기)·‘Git 저장소 ● 연결됨/미연결’(경로)·‘채팅 자동 수집 ● 켜짐/꺼짐’과 ‘켜기’/‘끄기’. 그 아래 탭 세 개 ‘기록 · N’, ‘경험 검토 · N’, ‘포트폴리오’ 중 하나만 보이며, 기본과 다른 프로젝트를 열 때는 ‘기록’(맨 위부터). 초안이 도는 동안 탭 줄 끝에 ‘작업 중…’. 기록·채팅은 그 프로젝트를 읽은 뒤에만 보임(`app.rows_project`). 개별 프로젝트의 기본 버튼은 ‘근거 요약’, ‘자료 발췌 초안’(작업 중에는 그대로 두고 누를 수 없음). 기록은 ‘11월 24일 (화) · 3건’ 같은 날짜 제목 아래 묶이고, 기록이 없으면 ‘이 프로젝트에 남긴 기록이 없습니다. ‘+ 기록’으로 시작하세요.’ | `Page::Projects`, `app.project`, `views::projects`, `views::project_picker`, `views::project_choices`, `views::project_cards`, `worker::ProjectCard`, `views::project_info`, `views::completed_label`, `app.snapshot.project_meta`, `Message::SetProjectCompleted`, `Message::OpenGithub`, `views::project_tabs`, `app.project_tab`(`ProjectTab`), `Message::ProjectTab`, `views::date_heading` |
| 기록 보관함 | 왼쪽 메뉴 ‘기록 보관함’(0.40.0 전에는 ‘개발 기록’) → 제목 ‘기록 보관함’. 모든 프로젝트의 여러 날짜·여러 종류 기록을 날짜순으로 조회하는 유일한 화면 | `Page::Records`, `views::records`, `app.snapshot.entries` |
| 날짜 미정 화면 | ‘날짜 미정’. 날짜가 비어 있는 일정을 모아 표시. 여기서 ‘+ 일정’을 누르면 날짜를 비워 둘 수 있음 | `Page::Tasks`, `views::tasks`, `Kind::Task` |
| 노트 화면 | 왼쪽 메뉴 ‘노트’ → 제목 ‘메모’. ‘+ 메모’로 추가하고 제목을 눌러 편집 | `Page::Notes`, `views::notes`, `app.snapshot.notes` |
| 설정 화면 | 왼쪽 메뉴 하단 ‘설정’. 제목 아래 영역 버튼 넷 중 고른 영역 하나만 보임. 위젯 상단의 ‘설정’은 ‘바탕화면 위젯’ 영역을 바로 엶 | `Page::Settings`, `views::settings`, `app.settings_section`, `widget_settings::Change::Open` |

## 0.30.1 앱과 캡처를 대조하는 법

04번 ‘선택한 프로젝트’ 캡처는 0.20.0 기준이다. 현재 앱에서는 왼쪽에서 프로젝트 이름을 선택한 뒤 아래 문구로 기능을 찾는다.

| 수정 요청에서 가리킬 대상 | 화면에서 찾는 법 (0.30.1 기준, 0.39.0에서 바뀐 곳은 그 줄에 적음) | 코드에서 찾는 법 |
| --- | --- | --- |
| 자료 발췌 초안 | 프로젝트 ‘포트폴리오’ 탭 맨 위의 ‘자료 발췌 초안’ 버튼(‘근거 요약’은 ‘기록’ 탭 맨 위). 기록 편집기의 종류 목록에는 ‘포트폴리오 초안’이라는 이름이 남아 있다. | `views::projects`, `Message::Draft(true)`, `views::KIND_LABELS` |
| 최종 기록 안내 줄 | 완료한 프로젝트의 정보 줄, ‘● 완료 · 날짜’ 아래 줄. "프로젝트를 완료했습니다. 최종 프로젝트 기록을 만들어 두세요."와 ‘최종 기록 만들기’ 버튼(누르면 ‘포트폴리오’ 탭으로 옮겨 입력 확인). 그 완료 날짜로 만든 프로젝트 기록이 생기거나 ‘진행 중으로’ 되돌리면 사라지고, 저장소 경로(절대 경로)가 없는 프로젝트에는 나오지 않음(0.44.2) | `views::project_info`, `project_record_ui::wants_final`, `Message::ProjectRecord(Action::Final)`, `app.snapshot.record_completion` |
| 프로젝트 기록 만들기 영역 | 프로젝트 ‘포트폴리오’ 탭 맨 위, ‘자료 발췌 초안’ 위. 진행 중인 프로젝트는 ‘프로젝트 기록 만들기’ 버튼 아래에 "진행 중에 만들면 ‘완료 전 종합’이 됩니다. …" 안내, 완료한 프로젝트는 버튼 이름이 ‘최종 프로젝트 기록 만들기’(0.44.2). 상태에 따라 그 버튼, ‘프로젝트 기록 입력 확인 중…’, 입력 크기·하루 기록이 없는 대화 날·‘완료 전 종합으로 만듭니다’와 ‘만들기’·‘취소’, ‘프로젝트 기록 만드는 중… (몇 분 걸릴 수 있습니다)’, 결과 문장과 버튼이 나옴. 저장소 경로가 없으면 ‘로컬 Git 저장소 경로를 넣으면 프로젝트 기록을 만들 수 있습니다’. 만든 기록은 다이어리에 ‘프로젝트 기록 · <이름>’(종류 ‘포트폴리오 초안’) 항목 하나로 보임 | `project_record_ui::panel`, `app.project_record`(`project_record_ui::Panel`, `State`: `Checking`·`Ask`·`Making`·`Done`·`Failed`), `Message::ProjectRecord(Action::Check/Make/Cancel)`, `project_record::diary::entry_body` |
| 로컬 채팅 자동 수집 | 저장소가 연결된 프로젝트의 정보 둘째 줄(최종 기록 안내 줄이 보이면 셋째 줄) ‘채팅 자동 수집 ● 켜짐/꺼짐’ 옆 ‘켜기’·‘끄기’ 버튼(Codex·Claude Code 채팅). 이 프로젝트의 기록을 아직 다 읽지 못했으면(전환 직후, 조회 실패) ‘● 확인 중’이고 버튼은 눌리지 않음. 저장소가 없으면 그 줄 아래에 ‘프로젝트 수정에서 로컬 Git 저장소 경로를 넣으면 채팅 자동 수집을 켤 수 있습니다’ | `views::project_info`, `Message::SetChatCapture`, `views::NO_REPO_HINT` |
| 지난 대화 정리 영역 | 채팅 자동 수집이 켜진 프로젝트의 ‘경험 검토’ 탭 맨 위. 상태에 따라 ‘지난 대화 정리’ 버튼, ‘지난 대화 확인 중…’, ‘지난 대화가 N일 있습니다 (…). 모두 정리하면 모델을 N번 부릅니다.’와 ‘지금 모두 정리’·‘나중에’, ‘지난 대화 정리 중…’과 ‘정리 멈추기’가 나옴. 다른 하루 정리 작업이 도는 동안에는 ‘하루 정리 만드는 중…’ | `views::past_days_panel`, `app.past_days`(프로젝트 ID별 `Past`: `Due`·`Checking`·`Ask`·`Making`), `Message::CheckPastDays(ID)`, `MakePastDays(ID)`, `DismissPastDays(ID)`, `StopPastDays` |
| 채팅에서 경험 검토 | 프로젝트 ‘경험 검토’ 탭. 수집된 채팅이 있으면 ‘채팅에서 경험 검토 · Terra’에서 채팅을 선택한다. 설계·트러블슈팅 후보에 체크한 뒤 ‘확인한 후보만 이력에 저장’을 누른다. 수집 중인데 경험 검토에 쓸 채팅이 없으면 이 자리에 ‘경험 검토에 쓸 채팅 발췌가 아직 없습니다’, 수집이 꺼져 있고 검토할 채팅도 없으면 ‘채팅 자동 수집을 켜면 수집된 채팅에서 경험 후보를 검토할 수 있습니다’ | `chat_review_ui::panel`, `views::NO_CHATS_YET` |
| 경험 선택과 Markdown 생성 | 프로젝트 ‘포트폴리오’ 탭의 ‘포트폴리오 경험’에서 ‘경험 선택’으로 기록을 고른다. ‘백엔드 포트폴리오 · Markdown 생성’ 버튼 옆에 ‘기존 v2 재사용 완료’처럼 실제 판본 번호가 표시된다. | `portfolio_ui::panel`, `portfolio_ui::Action::Export` |

30~31번 설정 캡처도 0.20.0 기준이다. 0.30.1의 ‘설정 → 로컬 일정 MCP’(0.38.0부터 ‘설정 → AI 연결 (MCP)’)에는 ‘MCP 켜기’와 ‘MCP 끄기’가 있다. 설정에서 전원 제어를 찾을 때는 `views::settings`와 `Message::SetMcpEnabled`를 참고한다.

## 달력, 위젯과 날짜별 보기

| 부를 이름 | 화면에서 찾는 법 · 기능 | 코드에서 찾는 법 |
| --- | --- | --- |
| 월간 달력 날짜 칸 | 월간 다이어리의 날짜 숫자와 그날의 항목. 맨 왼쪽 ‘주’ 열의 화살표로 주간 다이어리를 엶 | `shell::calendar`, `app.selected`, `Message::OpenWeek` |
| 선택 날짜 패널 | 월간 달력 오른쪽의 ‘○월 ○일’, ‘+ 일정’, ‘+ 기록’ 영역. 창이 좁으면 달력 아래에 표시 | `shell::date_details`, `app.selected` |
| 하루 정리 만들기 버튼 | 선택 날짜 패널의 ‘+ 일정’ ‘+ 기록’ 아래 줄. 설정의 하루 정리 방식이 ‘버튼으로 만들기’일 때만 보이고, 고른 날짜의 하루 정리를 만듦 | `shell::date_actions`, `Message::MakeDaily` |
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
| 프로젝트 편집기 | 버튼 줄의 ‘+ 프로젝트’ 또는 정보 줄의 ‘프로젝트 수정’. 프로젝트 이름·설명, 달력용 짧은 프로젝트명, 로컬 Git 저장소 경로, 그 아래 ‘GitHub 저장소 링크’(`https://github.com/<소유자>/<저장소>`만 받음). 경로 입력란 오른쪽의 ‘폴더 찾기’ 버튼이 Windows 폴더 선택창을 열고, 고른 폴더를 입력란에 넣음 | `Kind::Project`, `form.title`, `form.body`, `form.project_short_name`, `form.repo_path`, `form.github_url`, `Message::PickRepoFolder`, `Message::RepoFolderPicked` |
| 메모 편집기 | 노트 화면의 ‘+ 메모’ 또는 기존 메모 제목. 제목과 본문 편집 | `Kind::Note`, `form.title`, `form.body` |
| 편집기 날짜 입력 / 날짜 선택기 | 연·월·일을 나눠 입력하는 날짜 필드와 달력 아이콘으로 여는 선택기 | `date_input::input`, `EditorPicker::Date`, `Message::PickEditorDate` |
| 편집기 시간 선택기 | ‘하루 종일’을 해제했을 때의 시작·종료 시간 입력과 선택 목록 | `time_input::view`, `Message::OpenTimePicker`, `Message::PickEditorTime` |
| 편집기 닫기 확인 / 삭제 확인 | ‘저장하지 않고 닫을까요?’ 또는 ‘이 항목을 삭제할까요?’가 표시되는 확인창 | `Confirmation::Discard`, `Confirmation::Delete` |

## 설정 안의 영역과 Windows 메뉴

| 부를 이름 | 화면에서 찾는 법 · 기능 | 코드에서 찾는 법 |
| --- | --- | --- |
| 설정 영역 버튼 | 설정 제목 아래 줄의 ‘일반’, ‘기록과 하루 정리’, ‘바탕화면 위젯’, ‘AI 연결 (MCP)’. 고른 버튼이 강조되고, 다른 화면에 다녀와도 마지막 영역이 유지됨 | `views::settings_sections`, `SettingsSection`, `Message::SettingsSection` |
| 화면 모드 | 설정 ‘일반’의 ‘화면 모드’에 있는 ‘어두운 모드’·‘밝은 모드’ 두 버튼(쓰는 쪽이 강조됨), 또는 왼쪽 메뉴 아래의 전환 버튼(바꿀 모드 이름이 보임) | `app.preferences.theme`, `views::theme_choice`, `Message::SetTheme`, `shell::theme_toggle` |
| 다이어리 저장 위치 | 설정 ‘일반’의 ‘내 컴퓨터에 보관하는 다이어리’ 아래 데이터 저장 경로. 같은 영역에 ‘창 닫기’ 안내와 버전 | `app.data_dir`, `views::settings_general` |
| 하루 정리 만드는 방식 | 설정 ‘기록과 하루 정리’의 ‘하루 정리 만드는 방식’. ‘버튼으로 만들기’(기본)와 ‘자동으로 만들기’ 중 하나를 고름 | `app.preferences.daily_record`, `views::daily_mode_choice`, `Message::SetDailyMode` |
| 로컬 일정 MCP 연결 설정 | 설정 ‘AI 연결 (MCP)’의 ‘로컬 일정 MCP’. 서버 상태와 앱별 연결 현황이 보이고, 그 아래 ‘고급’(‘펼치기’·‘접기’) 안에 인증 토큰과 AI 연결 도우미가 있음. ‘고급’은 서버가 켜져 있는데 등록된 AI 앱이 하나도 없을 때도 저절로 펼쳐짐. 직접 접거나 펼치면 그 선택을 따르고, 고급 안에서 무엇이든 시작하면(토큰 재발급, 앱 선택, 미리보기 등) 접을 때까지 펼쳐 둠. 저절로 한 번 펼쳐지면 상태가 바뀌어도 다시 접히지 않음. 연결 현황 줄의 ‘다시 등록’·‘등록 준비’·‘검사’·‘다시 확인’을 누르거나 재발급이 실패하면, 또는 시작할 때 토큰을 새로 바꾸면 접어 두었더라도 펼쳐짐 | `views::settings_mcp`, `app.mcp_advanced`, `mcp_setup_ui::advanced_needed`, `app.mcp_setup`, `mcp_setup_ui::board`, `mcp_setup_ui::token_panel`, `mcp_setup_ui::view` |
| 위젯 표시·위치 설정 | 설정 ‘바탕화면 위젯’의 ‘표시와 위치’. 연결 상태, 표시·숨기기, 위치 복구·잠금. 위젯 오류는 이 영역에서는 연결 상태 칸에, 다른 영역과 화면에서는 위쪽 알림 줄에 보임 | `views::settings_widget`, `NativeAction`, `shell::view`의 `native_errors` |
| 위젯 모양 설정 | 같은 영역의 ‘크기와 표시 기간’, ‘글꼴과 간격’, ‘색상과 불투명도’(배경 불투명도 슬라이더와 색상) | `widget_settings::view`, `app.widget_ui`, `app.preferences.widget`, `app.preferences.widget_opacity` |
| 문서 저장소 안내 | 설정 ‘기록과 하루 정리’의 ‘문서 저장소와 외부 연결’. 자동 개발 기록의 보관 경로와 외부 연결에 관한 설명 | `views::settings` · [문서 저장소 동작](document-vault.md) |
| 트레이 메뉴 | Windows 알림 영역의 TimeSchedule 아이콘 메뉴. 다이어리 열기, 위젯 표시·숨기기·위치 복구, 앱 종료 | `NativeTray`, `TrayAction` |

## ‘개발 기록’이라고 했을 때

| 가리킬 수 있는 대상 | 확인할 위치 |
| --- | --- |
| 왼쪽 ‘개발 기록’ 메뉴 | 0.40.0에서 ‘기록 보관함’으로 이름을 바꾼 메뉴(`Page::Records`). 사용자는 개발 기록 전용 란을 의도하지 않았다. |
| 기록 종류 ‘개발 기록’ | 기록 편집기의 ‘기록 종류’ 선택값(`form.entry_kind`의 `development`). ‘자유 노트’, ‘트러블슈팅’ 등과 같은 목록에 있다. |
| 자동 개발 기록 | 커밋한 작업을 로컬 문서와 다이어리에 보관하는 절차. [자동 개발 기록 절차](project-records.md) |

예를 들어 “개발 기록 고쳐줘”라면 “왼쪽 메뉴, 기록 편집기의 종류 선택값, 커밋 뒤 자동 저장 중 어디를 말씀하시나요?”라고 묻는다. 새 란이 필요해 보이면 위치·이름·용도를 제시하고 답을 기다린다.

화면 구현은 [창·페이지 상태](../apps/timeschedule-native/src/app.rs), [다이어리 메뉴와 월간 배치](../apps/timeschedule-native/src/shell.rs), [페이지·편집기·팝업 분기](../apps/timeschedule-native/src/views.rs)에서 확인할 수 있다.
