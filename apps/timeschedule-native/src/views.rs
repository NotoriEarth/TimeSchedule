use crate::{
    app::{App, Field, Message, NativeAction, Page},
    editor::{Confirmation, Form, Kind},
    theme,
};
use chrono::{Datelike, Duration, NaiveDate};
use iced::widget::{
    button, checkbox, column, container, mouse_area, opaque, pick_list, row, scrollable, space,
    stack, text, text_editor, text_input, Column, Row,
};
use iced::{window, Alignment, Color, Element, Length, Theme};
use timeschedule_core::{
    diary::{EntrySummary, SearchHit},
    storage::Task,
};

pub(crate) const EDITOR_SIZE: iced::Size = iced::Size::new(650.0, 560.0);

const WEEKDAYS: [&str; 7] = ["일", "월", "화", "수", "목", "금", "토"];
pub(crate) fn preview(value: &str, limit: usize) -> String {
    let mut chars = value.chars();
    let mut shortened: String = chars.by_ref().take(limit).collect();
    if chars.next().is_some() {
        shortened.push('…');
    }
    shortened
}
const KIND_LABELS: [&str; 6] = [
    "자유 노트",
    "개발 기록",
    "트러블슈팅",
    "Git 커밋",
    "근거 요약",
    "포트폴리오 초안",
];
const KIND_VALUES: [&str; 6] = [
    "note",
    "development",
    "troubleshooting",
    "commit",
    "summary",
    "portfolio",
];

pub(crate) fn control<'a>(
    label: impl Into<String>,
    message: Message,
) -> iced::widget::Button<'a, Message> {
    button(text(label.into()).size(13))
        .padding([7, 10])
        .style(theme::secondary)
        .on_press(message)
}
pub(crate) fn quiet<'a>(value: impl Into<String>) -> Element<'a, Message> {
    text(value.into()).size(12).style(theme::muted).into()
}
pub(crate) fn title<'a>(value: impl Into<String>) -> Element<'a, Message> {
    text(value.into()).size(23).into()
}
fn kind_label(value: &str) -> &'static str {
    KIND_VALUES
        .iter()
        .position(|v| *v == value)
        .map(|i| KIND_LABELS[i])
        .unwrap_or("기록")
}
fn paper_style(theme: &Theme) -> container::Style {
    theme::surface(theme)
}
fn paper<'a>(content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    container(content)
        .padding(14)
        .width(Length::Fill)
        .style(paper_style)
        .into()
}
fn graph_paper<'a>(content: Element<'a, Message>) -> Element<'a, Message> {
    container(content)
        .padding(12)
        .width(Length::Fill)
        .height(Length::Fill)
        .style(theme::surface)
        .into()
}

pub fn view(app: &App, id: window::Id) -> Element<'_, Message> {
    if app.widget_ui.jump.as_ref().is_some_and(|j| j.window == id) {
        return crate::month_jump::view(app);
    }
    if Some(id) == app.widget {
        return crate::desktop_widget::view(app);
    }
    let quick = Some(id) == app.quick_window;
    let base = if quick {
        crate::quick_editor::view(app)
    } else {
        crate::shell::view(app)
    };
    if app.categories.open && !quick {
        return modal(base, crate::category_ui::view(app), None, true);
    }
    if app.calendar.detail.is_some() && app.calendar.detail_quick == quick {
        return modal(
            base,
            crate::calendar_ui::detail_view(app),
            Some(crate::calendar_ui::message(
                crate::calendar_ui::Action::CloseDetail,
            )),
            true,
        );
    }
    if let Some(editor) = app.editor.as_ref().filter(|_| app.editor_in_quick == quick) {
        // This child stays at the same tree path both with and without the
        // confirmation. Retain cursor/selection/scroll and freeze its input.
        let editor_layer = crate::frozen::frozen(
            if quick {
                container(panel(editor_view(app), true))
                    .center_x(Length::Fill)
                    .center_y(Length::Fill)
                    .style(theme::root)
                    .into()
            } else {
                modal(base, editor_view(app), Some(Message::CloseEditor), true)
            },
            editor.confirmation.is_some(),
        );
        if let Some(confirmation) = editor.confirmation {
            let (question, explanation, action, label) = match confirmation {
                Confirmation::Discard(_) => (
                    "저장하지 않고 닫을까요?",
                    "작성 중인 변경이 사라집니다.",
                    Message::Discard,
                    "저장하지 않고 닫기",
                ),
                Confirmation::Delete => (
                    "이 항목을 삭제할까요?",
                    if editor.form.kind == Kind::Project {
                        "연결된 기록은 프로젝트 없이 보존됩니다."
                    } else {
                        "삭제한 항목은 되돌릴 수 없습니다."
                    },
                    Message::ConfirmDelete,
                    "삭제 확인",
                ),
            };
            let mut panel = column![title(question), text(explanation).size(14)].spacing(18);
            if !editor.error.is_empty() {
                panel = panel.push(text(&editor.error).style(theme::danger));
            }
            let controls = if editor.saving {
                row![quiet("처리 중…")]
            } else {
                row![
                    control("계속 작성", Message::Keep),
                    button(text(label).size(13))
                        .style(button::danger)
                        .on_press(action)
                ]
                .spacing(12)
            };
            let confirmation_layer = modal(
                space().width(Length::Fill).height(Length::Fill).into(),
                panel.push(controls).into(),
                None,
                false,
            );
            return stack![editor_layer, confirmation_layer].into();
        }
        return stack![editor_layer].into();
    }
    base
}

pub(crate) fn actions(app: &App) -> Element<'_, Message> {
    let mut actions = Row::new().spacing(6);
    match app.page {
        Page::Month => {
            actions = actions
                .push(control("‹ 이전 달", Message::Month(-1)))
                .push(control("다음 달 ›", Message::Month(1)))
                .push(control("+ 일정", Message::New(Kind::Event, None)))
                .push(control("+ 기록", Message::New(Kind::Entry, None)));
        }
        Page::Week => {
            actions = actions
                .push(control("‹", Message::Week(-1)))
                .push(control("›", Message::Week(1)))
                .push(control("월로 돌아가기", Message::Navigate(Page::Month)))
                .push(control("주간 근거 요약", Message::Draft(false)))
                .push(control(
                    "카테고리",
                    crate::category_ui::msg(crate::category_ui::Action::Open),
                ))
                .push(control("+ 일정", Message::New(Kind::Event, None)));
        }
        Page::Projects => {
            actions = actions
                .push(control("+ 프로젝트", Message::New(Kind::Project, None)))
                .push(control("+ 기록", Message::New(Kind::Entry, None)));
            if let Some(project) = app
                .snapshot
                .projects
                .iter()
                .find(|p| Some(&p.id) == app.project.as_ref())
            {
                actions = actions.push(control(
                    "프로젝트 수정",
                    Message::EditForm(Form::project(project.clone())),
                ));
            }
        }
        Page::Records => actions = actions.push(control("+ 기록", Message::New(Kind::Entry, None))),
        Page::Tasks => {
            actions = actions.push(control(
                "+ 일정",
                Message::New(Kind::Task, Some(String::new())),
            ))
        }
        Page::Notes => actions = actions.push(control("+ 메모", Message::New(Kind::Note, None))),
        Page::Settings => {}
    }
    actions.wrap().into()
}
pub(crate) fn week(app: &App) -> Element<'_, Message> {
    let mut overview = column![
        text("WEEKLY").size(12).style(theme::muted),
        text(format!("{}월", app.week.month()))
            .size(32)
            .style(theme::accent),
        quiet(format!(
            "{} — {}",
            app.week.format("%m.%d"),
            (app.week + Duration::days(6)).format("%m.%d")
        )),
        space().height(12),
        text("이번 주의 계획과 기록").size(15),
        quiet("제목을 눌러 상세 보기\n날짜 칸 안에서 스크롤")
    ]
    .spacing(10);
    if app.more {
        overview = overview.push(more(app));
    }
    column![
        row![
            graph_paper(overview.into()),
            day(app, app.week),
            day(app, app.week + Duration::days(1)),
            day(app, app.week + Duration::days(2))
        ]
        .spacing(3)
        .height(Length::Fill),
        row![
            day(app, app.week + Duration::days(3)),
            day(app, app.week + Duration::days(4)),
            day(app, app.week + Duration::days(5)),
            day(app, app.week + Duration::days(6))
        ]
        .spacing(3)
        .height(Length::Fill)
    ]
    .spacing(3)
    .height(Length::Fill)
    .into()
}
fn day(app: &App, date: NaiveDate) -> Element<'_, Message> {
    let key = date.to_string();
    let mut items = Column::new().spacing(8);
    if let Some(day) = app.snapshot.days.iter().find(|d| d.date == key) {
        for item in &day.items {
            items = items.push(crate::calendar_ui::item_view(app, item, 230.0, 13.0, false));
        }
        if day.completed > 0 || day.total > day.items.len() as u32 {
            items = items.push(control(
                format!("전체 {}개 · 완료 {}개", day.total, day.completed),
                Message::WidgetDate(date),
            ));
        }
    } else {
        items = items.push(quiet("오늘의 작은 기록을 남겨 보세요."));
    }
    let header = row![
        text(format!("{}", date.day())).size(25),
        space().width(Length::Fill),
        text(format!(
            "{}요일",
            WEEKDAYS[date.weekday().num_days_from_sunday() as usize]
        ))
        .size(12)
        .style(move |t| text::Style {
            color: Some(if date.weekday().num_days_from_sunday() == 0 {
                theme::colors(t).rose
            } else {
                theme::colors(t).muted
            })
        })
    ]
    .align_y(Alignment::Center);
    mouse_area(graph_paper(
        column![
            header,
            row![
                button(text("+ 일정").size(11))
                    .style(theme::text_button)
                    .on_press(Message::New(Kind::Event, Some(key.clone()))),
                button(text("+ 기록").size(11))
                    .style(theme::text_button)
                    .on_press(Message::New(Kind::Entry, Some(key.clone())))
            ]
            .spacing(3),
            scrollable(items)
                .id(format!("day-{key}"))
                .height(Length::Fill)
        ]
        .spacing(7)
        .height(Length::Fill)
        .into(),
    ))
    .on_press(Message::SelectDay(date))
    .on_double_click(Message::New(Kind::Event, Some(key)))
    .into()
}
pub(crate) fn entry_card<'a>(app: &'a App, entry: &'a EntrySummary) -> Element<'a, Message> {
    let expanded = app.expanded.contains(&entry.id);
    let mut c = column![button(
        column![
            quiet(kind_label(&entry.kind)),
            text(format!(
                "{} {}",
                if expanded { "−" } else { "+" },
                entry.title
            ))
            .size(13)
        ]
        .spacing(3)
    )
    .width(Length::Fill)
    .style(theme::text_button)
    .on_press(Message::ToggleEntry(entry.id.clone()))]
    .spacing(8);
    if expanded {
        if let Some(full) = app.bodies.get(&entry.id) {
            c = c.push(text(&full.body).size(13));
            if !full.source_url.is_empty() {
                c = c.push(control(
                    "출처 링크 열기 ↗",
                    Message::Source(full.source_url.clone()),
                ));
            }
            if let Some(reference) = &full.source_ref {
                c = c
                    .push(quiet("수집 원본 · 변경되지 않는 근거"))
                    .push(text(reference).size(11))
                    .push(text(&full.source_body).size(12));
            }
            c = c.push(control(
                "기록 수정",
                Message::EditForm(Form::entry(full.clone())),
            ));
        } else if let Some(error) = app.entry_errors.get(&entry.id) {
            c = c.push(text(error).size(12));
        } else {
            c = c.push(quiet("원문 불러오는 중…"));
        }
    }
    container(c)
        .padding(10)
        .width(Length::Fill)
        .style(theme::surface)
        .into()
}
pub(crate) fn task_card<'a>(app: &'a App, task: &'a Task) -> Element<'a, Message> {
    let tick =
        button(text(if task.completed { "✓" } else { "□" }).size(18)).style(theme::text_button);
    let tick = if app.busy {
        tick
    } else {
        tick.on_press(Message::ToggleTask(task.id.clone()))
    };
    row![
        tick,
        button(
            column![
                text(&task.title).size(13),
                quiet(task.date.as_deref().unwrap_or("날짜 없음"))
            ]
            .spacing(3)
        )
        .style(theme::text_button)
        .width(Length::Fill)
        .on_press(Message::EditForm(Form::task(task.clone())))
    ]
    .spacing(5)
    .into()
}
pub(crate) fn more(app: &App) -> Element<'_, Message> {
    let b = control(
        format!("기록 더 보기 · {}개", app.snapshot.entries.len()),
        Message::More,
    );
    if app.paging {
        quiet("기록 불러오는 중…")
    } else {
        b.into()
    }
}
pub(crate) fn records(app: &App) -> Element<'_, Message> {
    let mut c = column![quiet(
        "모든 날짜의 노트, 개발 기록, 트러블슈팅과 저장한 초안"
    )]
    .spacing(12);
    for entry in &app.snapshot.entries {
        c = c.push(column![quiet(&entry.date), entry_card(app, entry)].spacing(4));
    }
    if app.snapshot.entries.is_empty() {
        c = c.push(quiet("아직 기록이 없습니다. 첫 기록을 남겨 보세요."));
    }
    if app.more {
        c = c.push(more(app));
    }
    scrollable(c).id("records-page").height(Length::Fill).into()
}
pub(crate) fn projects(app: &App) -> Element<'_, Message> {
    let mut list = column![control("모든 프로젝트", Message::Project(None))].spacing(5);
    for p in &app.snapshot.projects {
        list = list.push(control(&p.title, Message::Project(Some(p.id.clone()))));
    }
    let mut c = column![scrollable(list).height(110)].spacing(12);
    if let Some(p) = app
        .snapshot
        .projects
        .iter()
        .find(|p| Some(&p.id) == app.project.as_ref())
    {
        let mut panel = column![
            text(&p.description).size(14),
            quiet(format!(
                "Git 저장소: {}",
                if p.repo_path.is_empty() {
                    "미연결"
                } else {
                    &p.repo_path
                }
            ))
        ]
        .spacing(10);
        if app.busy {
            panel = panel.push(quiet("작업 중…"));
        } else {
            panel = panel.push(
                row![
                    control("Git 커밋 수집", Message::Import),
                    control("근거 요약", Message::Draft(false)),
                    control("포트폴리오 초안", Message::Draft(true))
                ]
                .spacing(6),
            );
        }
        panel = panel.push(quiet(
            "최근 500개 커밋을 수집하며 중복과 사용자 수정을 보존합니다.",
        ));
        c = c.push(paper(panel));
    }
    for e in &app.snapshot.entries {
        c = c.push(column![quiet(&e.date), entry_card(app, e)].spacing(4));
    }
    if app.more {
        c = c.push(more(app));
    }
    scrollable(c)
        .id("projects-page")
        .height(Length::Fill)
        .into()
}
pub(crate) fn tasks(app: &App) -> Element<'_, Message> {
    let mut c = column![quiet(
        "날짜를 정하면 다이어리와 위젯의 해당 날짜에 함께 표시됩니다."
    )]
    .spacing(10);
    for task in app.snapshot.tasks.iter().filter(|t| t.date.is_none()) {
        c = c.push(paper(task_card(app, task)));
    }
    scrollable(c).height(Length::Fill).into()
}
pub(crate) fn notes(app: &App) -> Element<'_, Message> {
    let mut c = column![quiet("제목을 선택하면 본문을 불러옵니다.")].spacing(10);
    for n in &app.snapshot.notes {
        c = c.push(control(
            format!(
                "{}  ·  {}",
                n.title,
                &n.updated_at[..10.min(n.updated_at.len())]
            ),
            Message::FetchForm(SearchHit {
                id: n.id.clone(),
                kind: "note".into(),
                title: n.title.clone(),
                excerpt: String::new(),
                date: None,
                project_id: None,
            }),
        ));
    }
    scrollable(c).height(Length::Fill).into()
}
pub(crate) fn search_results(app: &App) -> Element<'_, Message> {
    let mut c = column![title("검색 결과"), quiet("최대 50개 · 제목 또는 본문 검색")].spacing(10);
    if app.searching {
        c = c.push(quiet("검색 중…"));
    }
    for hit in &app.search_results {
        c = c.push(
            button(
                column![
                    text(&hit.title).size(16),
                    quiet(&hit.excerpt),
                    quiet(hit.date.as_deref().unwrap_or(""))
                ]
                .spacing(6),
            )
            .padding(12)
            .width(Length::Fill)
            .style(theme::secondary)
            .on_press(if hit.kind == "project" {
                Message::Project(Some(hit.id.clone()))
            } else {
                Message::FetchForm(hit.clone())
            }),
        );
    }
    if !app.searching && app.search_results.is_empty() {
        c = c.push(quiet("일치하는 기록이 없습니다."));
    }
    scrollable(c).height(Length::Fill).into()
}
pub(crate) fn settings(app: &App) -> Element<'_, Message> {
    let mut c = column![
        text(format!("TimeSchedule {}", env!("CARGO_PKG_VERSION"))).size(20),
        title("화면 모드"),
        quiet("Midnight · 어두운 모드 / Porcelain · 밝은 모드"),
        container(crate::shell::theme_toggle(app)).width(220),
        quiet("선택한 모드는 다음 실행에도 유지됩니다."),
        space().height(10),
        title("내 컴퓨터에 보관하는 다이어리"),
        quiet("데이터 저장 위치"),
        text(app.data_dir.display().to_string()).size(14),
        quiet("기존 Tauri 앱과 분리된 네이티브 데이터 폴더입니다."),
        title("로컬 일정 MCP"),
        text(&app.mcp_status).size(13),
        quiet("앱이 트레이에 있는 동안에도 연결됩니다. 완전 종료하면 중지됩니다."),
        crate::mcp_setup_ui::view(&app.mcp_setup),
        title("바탕화면 위젯"),
        scrollable(text(app.widget_status_text()).size(13))
            .id("widget-connection-status")
            .height(52)
            .width(Length::Fill)
    ]
    .spacing(15);
    // Keep controls and scroll content mounted during native requests.
    let enabled = !app.native_busy;
    let native_control = |label: &str, action| {
        control(label, Message::Native(action))
            .on_press_maybe(enabled.then_some(Message::Native(action)))
    };
    c = c
        .push(
            container(quiet(if app.native_busy {
                "Windows 작업 중…"
            } else {
                " "
            }))
            .height(20)
            .width(Length::Fill),
        )
        .push(
            row![
                native_control(
                    if app.preferences.widget_visible && app.widget_attached {
                        "위젯 숨기기"
                    } else {
                        "위젯 표시"
                    },
                    if app.preferences.widget_visible && app.widget_attached {
                        NativeAction::Hide
                    } else {
                        NativeAction::Show
                    }
                ),
                native_control("위젯 위치 복구", NativeAction::Recover),
                native_control(
                    if app.preferences.widget_locked {
                        "잠금 해제"
                    } else {
                        "위치 잠금"
                    },
                    NativeAction::Lock(!app.preferences.widget_locked)
                )
            ]
            .spacing(6),
        )
        .push(
            text(format!(
                "배경 불투명도 {}% · 글자는 선명하게 유지",
                app.preferences.widget_opacity
            ))
            .size(14),
        )
        .push(iced::widget::slider(
            40..=100,
            app.preferences.widget_opacity,
            |v| Message::Native(NativeAction::Opacity(v)),
        ));
    c = c.push(crate::widget_settings::view(app));
    c=c.push(title("근거 초안과 외부 연결")).push(text("요약과 포트폴리오는 저장된 원문을 정리한 편집 가능한 초안입니다. AI 분석 결과가 아닙니다. ChatGPT/Space·모바일·여러 기기 동기화는 아직 연결되지 않았습니다.").size(14)).push(quiet("상세창 X는 트레이로 숨깁니다. 완전 종료는 왼쪽 앱 종료 또는 트레이 메뉴를 사용하세요."));
    scrollable(paper(c)).height(Length::Fill).into()
}
#[derive(Debug, Clone, PartialEq, Eq)]
struct ProjectChoice {
    id: Option<String>,
    title: String,
}
impl std::fmt::Display for ProjectChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.title)
    }
}
fn editor_view(app: &App) -> Element<'_, Message> {
    let e = app.editor.as_ref().unwrap();
    let form = &e.form;
    let input =
        |label: &'static str, value: &str, field: fn(String) -> Field| -> Element<'_, Message> {
            column![
                quiet(label),
                text_input(label, value)
                    .on_input(move |v| Message::Field(field(v)))
                    .padding(8)
                    .size(14)
            ]
            .spacing(5)
            .into()
        };
    let mut fields = column![column![
        quiet("제목"),
        text_input("제목", &form.title)
            .id("editor-title")
            .on_input(|s| Message::Field(Field::Title(s.chars().take(200).collect())))
            .padding(8)
    ]
    .spacing(5)]
    .spacing(12);
    {
        let editor = text_editor(&e.content)
            .id("editor-body")
            .height(140)
            .placeholder("생각과 기록을 자유롭게 적어 보세요.")
            .on_action(Message::Body)
            .key_binding(editor_binding);
        fields = fields
            .push(quiet(if form.kind == Kind::Project {
                "프로젝트 설명"
            } else {
                "본문"
            }))
            .push(editor)
            .push(quiet("Ctrl+Z / Ctrl+Y · 최근 편집 32회 복원"));
    }
    if matches!(form.kind, Kind::Entry | Kind::Event | Kind::Task) {
        fields = fields.push(input(
            if form.kind == Kind::Task {
                "날짜 (선택, YYYY-MM-DD)"
            } else {
                "날짜 (YYYY-MM-DD)"
            },
            &form.date,
            Field::Date,
        ));
    }
    if form.kind == Kind::Entry {
        let choices: Vec<_> = app
            .snapshot
            .categories
            .iter()
            .filter(|c| c.id != timeschedule_core::calendar::SCHEDULE_CATEGORY)
            .map(|c| ProjectChoice {
                id: Some(c.id.clone()),
                title: c.name.clone(),
            })
            .collect();
        let selected = choices
            .iter()
            .find(|c| c.id.as_deref() == Some(&form.category_id))
            .cloned();
        fields = fields.push(column![
            quiet("카테고리"),
            pick_list(choices, selected, |c| Message::Field(Field::Category(
                c.id.unwrap()
            )))
        ]);
        fields = fields.push(input(
            "짧은 작업명 (달력 표시용, 선택)",
            &form.short_title,
            Field::ShortTitle,
        ));
    }
    if form.kind == Kind::Project {
        fields = fields.push(input(
            "짧은 프로젝트명 (달력 표시용, 선택)",
            &form.project_short_name,
            Field::ProjectShortName,
        ));
    }
    if form.kind == Kind::Entry {
        let choices: Vec<_> = std::iter::once(ProjectChoice {
            id: None,
            title: "프로젝트 없음".into(),
        })
        .chain(app.snapshot.projects.iter().map(|p| ProjectChoice {
            id: Some(p.id.clone()),
            title: p.title.clone(),
        }))
        .collect();
        let selected = choices.iter().find(|p| p.id == form.project_id).cloned();
        fields = fields.push(
            row![
                column![
                    quiet("기록 종류"),
                    pick_list(KIND_LABELS, Some(kind_label(&form.entry_kind)), |label| {
                        Message::Field(Field::EntryKind(
                            KIND_VALUES[KIND_LABELS.iter().position(|x| *x == label).unwrap()]
                                .into(),
                        ))
                    })
                ]
                .spacing(5),
                column![
                    quiet("프로젝트"),
                    pick_list(choices, selected, |choice| Message::Field(Field::Project(
                        choice.id
                    )))
                ]
                .spacing(5)
            ]
            .spacing(10),
        );
    }
    if matches!(form.kind, Kind::Event | Kind::Task) {
        fields = fields.push(
            checkbox(form.all_day)
                .label("하루 종일")
                .on_toggle(|v| Message::Field(Field::AllDay(v))),
        );
        if !form.all_day {
            fields = fields.push(
                row![
                    input("시작 (HH:MM)", &form.start, Field::Start),
                    input("종료 (HH:MM)", &form.end, Field::End)
                ]
                .spacing(10),
            );
        }
    }
    if matches!(form.kind, Kind::Event | Kind::Task) {
        fields = fields.push(
            checkbox(form.completed)
                .label("완료")
                .on_toggle(|v| Message::Field(Field::Completed(v))),
        );
    }
    if form.kind == Kind::Entry {
        fields = fields.push(input(
            "출처 링크 (http 또는 https)",
            &form.source_url,
            Field::Source,
        ));
    }
    if form.kind == Kind::Project {
        fields = fields.push(input("로컬 Git 저장소 경로", &form.repo_path, Field::Repo));
    }
    if !e.error.is_empty() {
        fields = fields.push(text(&e.error).size(13).style(theme::danger));
    }
    let mut footer = Row::new().spacing(8).align_y(Alignment::Center);
    if e.saving {
        footer = footer.push(quiet("저장 중…"));
    } else {
        if form.id.is_some() {
            footer = footer.push(
                button(text("삭제").size(13))
                    .style(button::danger)
                    .on_press(Message::Delete),
            );
        }
        footer = footer
            .push(quiet(if e.dirty() { "미저장 변경" } else { "" }))
            .push(space().width(Length::Fill))
            .push(control("취소", Message::CloseEditor))
            .push(button(text("저장하기").size(14)).on_press(Message::Save));
    }
    column![
        row![
            title(format!(
                "{} {}",
                form.kind.label(),
                if form.id.is_some() {
                    "수정"
                } else {
                    "추가"
                }
            )),
            space().width(Length::Fill),
            button("×")
                .style(theme::text_button)
                .on_press(Message::CloseEditor)
        ],
        scrollable(fields)
            .id(format!("editor-fields-{}", e.generation))
            .height(Length::Fill),
        footer
    ]
    .spacing(14)
    .height(Length::Fill)
    .into()
}
// Shared form surface: widget quick window and diary modal use one template.
fn panel<'a>(content: Element<'a, Message>, editor: bool) -> Element<'a, Message> {
    let mut surface = container(content)
        .padding(22)
        .width(EDITOR_SIZE.width)
        .style(paper_style);
    if editor {
        surface = surface.height(EDITOR_SIZE.height);
    }
    opaque(mouse_area(surface).on_scroll(|_| Message::Ignore)).into()
}

fn modal<'a>(
    base: Element<'a, Message>,
    content: Element<'a, Message>,
    backdrop: Option<Message>,
    editor: bool,
) -> Element<'a, Message> {
    let shade = mouse_area(
        container(space())
            .width(Length::Fill)
            .height(Length::Fill)
            .style(|_| {
                container::Style::default().background(Color::from_rgba(0.02, 0.03, 0.05, 0.62))
            }),
    )
    .on_press(backdrop.unwrap_or(Message::Ignore))
    .on_scroll(|_| Message::Ignore);
    let box_ = panel(content, editor);
    stack![
        base,
        shade,
        container(box_)
            .center_x(Length::Fill)
            .center_y(Length::Fill)
    ]
    .into()
}
fn editor_binding(key: text_editor::KeyPress) -> Option<text_editor::Binding<Message>> {
    if matches!(key.status, text_editor::Status::Focused { .. }) && key.modifiers.command() {
        match key.key.to_latin(key.physical_key) {
            Some('z') => {
                return Some(text_editor::Binding::Custom(if key.modifiers.shift() {
                    Message::Redo
                } else {
                    Message::Undo
                }))
            }
            Some('y') => return Some(text_editor::Binding::Custom(Message::Redo)),
            _ => {}
        }
    }
    text_editor::Binding::from_key_press(key)
}
