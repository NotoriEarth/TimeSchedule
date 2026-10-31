use crate::{
    app::{App, Field, Message, NativeAction, Page},
    editor::{Confirmation, Form, Kind},
    theme,
};
use chrono::{Datelike, Duration, Local, NaiveDate};
use iced::widget::{
    button, checkbox, column, container, mouse_area, opaque, pick_list, row, scrollable, space,
    stack, text, text_editor, text_input, Column, Row,
};
use iced::{window, Alignment, Color, Element, Length, Theme};
use timeschedule_core::{
    dates::calendar_days,
    diary::{EntrySummary, SearchHit},
    storage::{CalendarEvent, Task},
};

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
    if Some(id) == app.widget {
        return widget(app);
    }
    let base = crate::shell::view(app);
    if let Some(editor) = &app.editor {
        // This child stays at the same tree path both with and without the
        // confirmation. Retain cursor/selection/scroll and freeze its input.
        let editor_layer = crate::frozen::frozen(
            modal(base, editor_view(app), Some(Message::CloseEditor), true),
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
                .push(control("주간 근거 요약", Message::Draft(false)));
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
        Page::Tasks => actions = actions.push(control("+ 할 일", Message::New(Kind::Task, None))),
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
    for event in app.snapshot.events.iter().filter(|e| e.date == key) {
        items = items.push(event_card(app, event));
    }
    for entry in app.snapshot.entries.iter().filter(|e| e.date == key) {
        items = items.push(entry_card(app, entry));
    }
    for task in app
        .snapshot
        .tasks
        .iter()
        .filter(|t| t.date.as_deref() == Some(key.as_str()))
    {
        items = items.push(task_card(app, task));
    }
    if !app.snapshot.events.iter().any(|e| e.date == key)
        && !app.snapshot.entries.iter().any(|e| e.date == key)
        && !app
            .snapshot
            .tasks
            .iter()
            .any(|t| t.date.as_deref() == Some(key.as_str()))
    {
        items = items.push(quiet("오늘의 작은 기록을\n남겨 보세요."));
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
    graph_paper(
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
    )
}
pub(crate) fn event_card<'a>(app: &'a App, event: &'a CalendarEvent) -> Element<'a, Message> {
    let expanded = app.expanded_events.contains(&event.id);
    let mut contents = column![button(
        column![
            text(format!(
                "{}{}",
                if event.completed {
                    "✓ "
                } else {
                    if expanded {
                        "− "
                    } else {
                        "+ "
                    }
                },
                event.title
            ))
            .size(13),
            quiet(if event.all_day {
                "종일".into()
            } else {
                format!(
                    "{}–{}",
                    event.start_time.as_deref().unwrap_or(""),
                    event.end_time.as_deref().unwrap_or("")
                )
            })
        ]
        .spacing(3)
    )
    .style(theme::text_button)
    .width(Length::Fill)
    .on_press(Message::ToggleEvent(event.id.clone()))]
    .spacing(8);
    if expanded {
        contents = contents.push(text(&event.notes).size(13)).push(control(
            "일정 수정",
            Message::EditForm(Form::event(event.clone())),
        ));
    }
    let category = event.color.clone();
    container(contents)
        .padding(10)
        .width(Length::Fill)
        .style(move |t| theme::event_surface(t, &category))
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
    let mut c = column![quiet(format!(
        "{} / {}개 완료",
        app.snapshot.tasks.iter().filter(|t| t.completed).count(),
        app.snapshot.tasks.len()
    ))]
    .spacing(10);
    for task in &app.snapshot.tasks {
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
        title("화면 모드"),
        quiet("Midnight · 어두운 모드 / Porcelain · 밝은 모드"),
        container(crate::shell::theme_toggle(app)).width(220),
        quiet("선택한 모드는 다음 실행에도 유지됩니다."),
        space().height(10),
        title("내 컴퓨터에 보관하는 다이어리"),
        quiet("데이터 저장 위치"),
        text(app.data_dir.display().to_string()).size(14),
        quiet("기존 Tauri 앱과 분리된 네이티브 데이터 폴더입니다."),
        title("바탕화면 위젯"),
        text(&app.desktop_message).size(13)
    ]
    .spacing(15);
    if app.native_busy {
        c = c.push(quiet("Windows 작업 중…"));
    } else {
        c = c.push(
            row![
                control(
                    if app.preferences.widget_visible && app.widget_attached {
                        "위젯 숨기기"
                    } else {
                        "위젯 표시"
                    },
                    Message::Native(if app.preferences.widget_visible && app.widget_attached {
                        NativeAction::Hide
                    } else {
                        NativeAction::Show
                    })
                ),
                control("위젯 위치 복구", Message::Native(NativeAction::Recover)),
                control(
                    if app.preferences.widget_locked {
                        "잠금 해제"
                    } else {
                        "위치 잠금"
                    },
                    Message::Native(NativeAction::Lock(!app.preferences.widget_locked))
                )
            ]
            .spacing(6),
        );
        c = c
            .push(text(format!("불투명도 {}%", app.preferences.widget_opacity)).size(14))
            .push(iced::widget::slider(
                40..=100,
                app.preferences.widget_opacity,
                |v| Message::Native(NativeAction::Opacity(v)),
            ));
    }
    c=c.push(title("근거 초안과 외부 연결")).push(text("요약과 포트폴리오는 저장된 원문을 정리한 편집 가능한 초안입니다. AI 분석 결과가 아닙니다. ChatGPT/Space·모바일·여러 기기 동기화는 아직 연결되지 않았습니다.").size(14)).push(quiet("상세창 X는 트레이로 숨깁니다. 완전 종료는 왼쪽 앱 종료 또는 트레이 메뉴를 사용하세요."));
    scrollable(paper(c)).height(Length::Fill).into()
}
fn widget(app: &App) -> Element<'_, Message> {
    let dates = calendar_days(app.widget_month);
    let mut c = column![
        row![
            mouse_area(
                text(format!(
                    "{}년 {}월",
                    app.widget_month.year(),
                    app.widget_month.month()
                ))
                .size(22)
            )
            .on_press(Message::DragWidget),
            space().width(Length::Fill),
            button("‹")
                .style(theme::text_button)
                .on_press(Message::WidgetMonth(-1)),
            button(text("오늘").size(12))
                .style(theme::text_button)
                .on_press(Message::WidgetToday),
            button("›")
                .style(theme::text_button)
                .on_press(Message::WidgetMonth(1))
        ]
        .align_y(Alignment::Center),
        Row::with_children(
            WEEKDAYS
                .iter()
                .map(|d| container(text(*d).size(12)).center_x(Length::Fill).into())
        )
    ]
    .spacing(4);
    for week in dates.chunks(7) {
        let mut cells = Row::new().spacing(2);
        for date in week {
            let events: Vec<_> = app
                .snapshot
                .widget_events
                .iter()
                .filter(|e| e.date == date.to_string())
                .collect();
            let mut day = column![text(format!(
                "{}{}",
                date.day(),
                if events.len() > 2 {
                    format!(" +{}", events.len() - 2)
                } else {
                    String::new()
                }
            ))
            .size(15)]
            .spacing(1);
            for event in events.iter().take(2) {
                day = day.push(
                    text(format!(
                        "{}{}",
                        if event.completed { "✓" } else { "" },
                        preview(&event.title, 5)
                    ))
                    .size(9),
                );
            }
            cells = cells.push(
                button(day.align_x(Alignment::Center))
                    .width(Length::Fill)
                    .height(46)
                    .padding(2)
                    .style(if *date == Local::now().date_naive() {
                        theme::primary
                    } else {
                        theme::text_button
                    })
                    .on_press(Message::WidgetDate(*date)),
            );
        }
        c = c.push(cells);
    }
    let today = Local::now().date_naive();
    let today_events = &app.snapshot.today_events;
    let mut today_list = column![button(
        text(format!(
            "오늘 {} · 일정 {}개",
            today.format("%m.%d"),
            today_events.len()
        ))
        .size(12)
    )
    .style(theme::text_button)
    .on_press(Message::WidgetDate(today))]
    .spacing(2);
    for event in today_events.iter().take(3) {
        today_list = today_list.push(
            button(
                text(format!(
                    "{} {}{}",
                    if event.all_day {
                        "종일"
                    } else {
                        event.start_time.as_deref().unwrap_or("")
                    },
                    if event.completed { "✓ " } else { "" },
                    preview(&event.title, 28)
                ))
                .size(11),
            )
            .style(theme::text_button)
            .on_press(Message::WidgetDate(today)),
        );
    }
    if today_events.is_empty() {
        today_list = today_list.push(text("오늘 등록된 일정이 없어요.").size(11));
    }
    c = c.push(scrollable(today_list).height(70));
    c = c.push(row![
        button(text("다이어리 열기").size(12))
            .on_press(Message::ShowManager)
            .style(theme::text_button),
        space().width(Length::Fill),
        button(
            text(if app.preferences.widget_locked {
                "잠금 해제"
            } else {
                "잠금"
            })
            .size(11)
        )
        .on_press(Message::Native(NativeAction::Lock(
            !app.preferences.widget_locked
        )))
        .style(theme::text_button)
    ]);
    c = c.push(
        row![
            text(format!("불투명도 {}%", app.preferences.widget_opacity)).size(11),
            iced::widget::slider(40..=100, app.preferences.widget_opacity, |v| {
                Message::Native(NativeAction::Opacity(v))
            })
        ]
        .spacing(10)
        .align_y(Alignment::Center),
    );
    c = c.push(
        text(if app.widget_attached {
            "바탕화면에 연결됨"
        } else {
            "바탕화면 연결 확인 필요 · 설정에서 재시도"
        })
        .size(10),
    );
    container(scrollable(c))
        .padding(10)
        .width(Length::Fill)
        .height(Length::Fill)
        .style(paper_style)
        .into()
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
    if form.kind == Kind::Event {
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
        let colors = ["세이지", "블루", "앰버", "로즈"];
        let values = ["sage", "blue", "amber", "rose"];
        let selected = colors[values.iter().position(|v| *v == form.color).unwrap_or(0)];
        fields = fields.push(pick_list(colors, Some(selected), move |label| {
            Message::Field(Field::Color(
                values[colors.iter().position(|x| *x == label).unwrap()].into(),
            ))
        }));
    }
    if matches!(form.kind, Kind::Event | Kind::Task) {
        fields = fields.push(
            checkbox(form.completed)
                .label("완료")
                .on_toggle(|v| Message::Field(Field::Completed(v))),
        );
    }
    if form.kind != Kind::Task {
        let editor = text_editor(&e.content)
            .id("editor-body")
            .height(220)
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
    let mut box_ = container(content).padding(22).width(650).style(paper_style);
    if editor {
        box_ = box_.height(560);
    }
    let box_ = opaque(mouse_area(box_).on_scroll(|_| Message::Ignore));
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
