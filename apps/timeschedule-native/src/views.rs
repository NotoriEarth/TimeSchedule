use crate::{
    app::{App, Field, Message, NativeAction, Page, Past},
    editor::{Confirmation, EditorPicker, Form, Kind},
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
use timeschedule_desktop::preferences::DailyRecordMode;

pub(crate) const EDITOR_SIZE: iced::Size = iced::Size::new(650.0, 560.0);

pub(crate) const WEEKDAYS: [&str; 7] = ["일", "월", "화", "수", "목", "금", "토"];
pub(crate) fn preview(value: &str, limit: usize) -> String {
    let mut chars = value.chars();
    let mut shortened: String = chars.by_ref().take(limit).collect();
    if chars.next().is_some() {
        shortened.push('…');
    }
    shortened
}
const KIND_LABELS: [&str; 7] = [
    "자유 노트",
    "설계 경험",
    "개발 기록",
    "트러블슈팅",
    "Git 커밋",
    "근거 요약",
    "포트폴리오 초안",
];
const KIND_VALUES: [&str; 7] = [
    "note",
    "design",
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
pub(crate) fn kind_label(value: &str) -> &'static str {
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
    // Cards stay under the editor opened from one of them, and are shown
    // again when it closes.
    let cards = app
        .calendar
        .group
        .as_ref()
        .is_some_and(|g| g.quick == quick);
    if cards && !(app.editor.is_some() && app.editor_in_quick == quick) {
        return modal(
            base,
            crate::calendar_ui::group_view(app),
            Some(crate::calendar_ui::message(
                crate::calendar_ui::Action::CloseGroup,
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
            items = items.push(crate::calendar_ui::item_view(app, item, 13.0, false));
        }
        if day.completed > 0 || day.total > day.items.len() as u32 {
            items = items.push(control(
                format!("전체 {}개 · 완료 {}개", day.total, day.completed),
                Message::ManagerDate(date),
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
                .spacing(crate::calendar_ui::SCROLL_GAP)
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
    use timeschedule_core::timeline::reference_line;
    let expanded = app.expanded.contains(&entry.id);
    // A timeline read when the card was opened (which may have completed a
    // past record's start or commits) outlives folding it again; the list's
    // own copy is only the fallback before the first opening.
    let loaded = app.timelines.get(&entry.id);
    let current = loaded.unwrap_or(&entry.timeline);
    let mut head = column![
        quiet(kind_label(&entry.kind)),
        text(format!(
            "{} {}",
            if expanded { "−" } else { "+" },
            entry.title
        ))
        .size(13)
    ]
    .spacing(3);
    // Folded: the start (or source/typed time) and the summary chosen from the
    // original. Unfolded, the full text replaces the summary: never both.
    if !expanded {
        head = head.push(quiet(reference_line(&entry.date, current)));
        if !entry.summary.is_empty() {
            head = head.push(text(&entry.summary).size(12));
        }
    }
    let mut c = column![button(head)
        .width(Length::Fill)
        .style(theme::text_button)
        .on_press(Message::ToggleEntry(entry.id.clone()))]
    .spacing(8);
    if expanded {
        if let Some(full) = app.bodies.get(&entry.id) {
            let timeline_ref = current;
            // Reading order: start, the record, completion or status, then
            // commits and the original.
            c = c
                .push(crate::calendar_ui::reference_view(&full.date, timeline_ref))
                .push(text(&full.body).size(13));
            if let Some(tail) = loaded.and_then(crate::calendar_ui::timeline_tail) {
                c = c.push(tail);
            }
            if !full.source_url.is_empty() {
                c = c.push(control(
                    "출처 링크 열기 ↗",
                    Message::Source(full.source_url.clone()),
                ));
            }
            // A collected record's unchangeable source. An automatic record's
            // summary is already in its text and its original opens above.
            if let Some(reference) = full
                .source_ref
                .as_ref()
                .filter(|_| !timeline_ref.is_automatic())
            {
                c = c
                    .push(quiet("수집 원본 · 변경되지 않는 근거"))
                    .push(text(reference).size(11));
                if full.source_body.trim() != full.body.trim() {
                    c = c.push(text(&full.source_body).size(12));
                }
            }
            c = c.push(control(
                "기록 수정",
                Message::EditForm(Form::entry(full.clone())),
            ));
        } else if let Some(error) = app.entry_errors.get(&entry.id) {
            c = c.push(text(error).size(12));
        } else {
            c = c
                .push(quiet(reference_line(&entry.date, current)))
                .push(quiet("원문 불러오는 중…"));
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
/// Every record of a date in the current list's scope, never fewer than the
/// rows already listed.
pub(crate) fn date_total(app: &App, date: &str, shown: u32) -> u32 {
    app.snapshot
        .entry_totals
        .get(date)
        .copied()
        .unwrap_or(shown)
        .max(shown)
}
/// Consecutive records of one date, in the list's own newest-first order.
pub(crate) fn date_groups(entries: &[EntrySummary]) -> Vec<(&str, &[EntrySummary])> {
    let mut groups: Vec<(&str, &[EntrySummary])> = Vec::new();
    let mut start = 0;
    for index in 1..=entries.len() {
        if index == entries.len() || entries[index].date != entries[start].date {
            groups.push((entries[start].date.as_str(), &entries[start..index]));
            start = index;
        }
    }
    groups
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
    scrollable(c)
        .id("records-page")
        .spacing(crate::calendar_ui::SCROLL_GAP)
        .height(Length::Fill)
        .into()
}
/// The question the owner is asked before the past days are made. Each day asks the model once.
pub(crate) fn past_days_question(days: &[NaiveDate]) -> String {
    let count = days.len();
    let (Some(first), Some(last)) = (days.first(), days.last()) else {
        return String::new();
    };
    format!(
        "지난 대화가 {count}일 있습니다 ({first} ~ {last}). 모두 정리하면 모델을 {count}번 부릅니다."
    )
}
/// What the project screen says about making the daily records of the days before the
/// capture was turned on: the button, the question with its count, or the progress.
fn past_days_panel<'a>(app: &'a App, project: &str) -> Element<'a, Message> {
    match app.past_days.get(project) {
        Some(Past::Due | Past::Checking) => quiet("지난 대화 확인 중…"),
        Some(Past::Ask(days, _)) => column![
            text(past_days_question(days)).size(13),
            row![
                button(text("지금 모두 정리").size(13))
                    .padding([7, 10])
                    .style(theme::secondary)
                    .on_press_maybe(
                        app.past_days_can_start()
                            .then(|| Message::MakePastDays(project.to_owned()))
                    ),
                control("나중에", Message::DismissPastDays(project.to_owned()))
            ]
            .spacing(6)
        ]
        .spacing(6)
        .into(),
        Some(Past::Making) => row![
            quiet("지난 대화 정리 중…"),
            control("정리 멈추기", Message::StopPastDays(project.to_owned()))
        ]
        .spacing(8)
        .align_y(Alignment::Center)
        .into(),
        _ if app.daily_making => quiet("하루 정리 만드는 중…"),
        _ => control("지난 대화 정리", Message::CheckPastDays(project.to_owned())).into(),
    }
}
pub(crate) fn projects(app: &App) -> Element<'_, Message> {
    let mut list = column![control("모든 프로젝트", Message::Project(None))].spacing(5);
    for p in &app.snapshot.projects {
        list = list.push(control(&p.title, Message::Project(Some(p.id.clone()))));
    }
    let mut c = column![scrollable(list)
        .spacing(crate::calendar_ui::SCROLL_GAP)
        .height(110)]
    .spacing(12);
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
                    control("근거 요약", Message::Draft(false)),
                    control("자료 발췌 초안", Message::Draft(true))
                ]
                .spacing(6),
            );
        }
        let watching = app.snapshot.chat_capture_project.as_ref() == Some(&p.id);
        if watching || !p.repo_path.is_empty() {
            panel = panel.push(control(
                if watching {
                    "채팅 자동 수집 끄기"
                } else {
                    "Codex·Claude Code 채팅 자동 수집 켜기"
                },
                Message::SetChatCapture(p.id.clone(), !watching),
            ));
        }
        if watching {
            panel = panel.push(past_days_panel(app, &p.id));
        }
        if !app.snapshot.reviewable_chats.is_empty() {
            panel = panel.push(crate::chat_review_ui::panel(app, &p.id));
        }
        if let Some(state) = app
            .snapshot
            .portfolio
            .as_ref()
            .filter(|s| s.project_id == p.id)
        {
            panel = panel.push(crate::portfolio_ui::panel(app, state));
        }
        c = c.push(paper(panel));
    }
    // The same diary records, newest date first, one heading per date. The
    // count is every record of that date; a date the loaded rows only begin
    // says how many are shown and that more follow.
    for (date, entries) in date_groups(&app.snapshot.entries) {
        let shown = entries.len() as u32;
        let total = date_total(app, date, shown);
        let mut heading = format!("{date} · {total}건");
        if shown < total {
            heading.push_str(&format!(" · {shown}건 표시 · 더 불러오면 이어집니다"));
        }
        let mut day = column![quiet(heading)].spacing(4);
        for e in entries {
            day = day.push(entry_card(app, e));
        }
        c = c.push(day);
    }
    if app.more {
        c = c.push(more(app));
    }
    scrollable(c)
        .id("projects-page")
        .spacing(crate::calendar_ui::SCROLL_GAP)
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
    scrollable(c)
        .spacing(crate::calendar_ui::SCROLL_GAP)
        .height(Length::Fill)
        .into()
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
    scrollable(c)
        .spacing(crate::calendar_ui::SCROLL_GAP)
        .height(Length::Fill)
        .into()
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
    scrollable(c)
        .spacing(crate::calendar_ui::SCROLL_GAP)
        .height(Length::Fill)
        .into()
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
        space().height(10),
        title("하루 정리 만드는 방식"),
        daily_mode_choice(app),
        quiet(match app.preferences.daily_record {
            DailyRecordMode::Button => "월간 다이어리에서 날짜를 고른 뒤 ‘하루 정리 만들기’를 누를 때만 만듭니다. 모델 사용량은 누를 때만 씁니다.",
            DailyRecordMode::Auto => "앱을 켤 때와 켜 둔 동안 한 시간마다, 최근 7일 가운데 끝난 날의 하루 정리를 만듭니다. 오늘은 만들지 않습니다. 대화가 바뀌지 않은 날은 모델을 부르지 않습니다.",
        }),
        quiet("채팅 자동 수집을 켠 프로젝트의 대화로 만듭니다."),
        title("로컬 일정 MCP"),
        crate::mcp_setup_ui::server_line(&app.mcp_status, app.mcp_endpoint().is_some()),
        row![
            control("MCP 켜기", Message::SetMcpEnabled(true)).on_press_maybe(
                app.can_set_mcp(true)
                    .then_some(Message::SetMcpEnabled(true))
            ),
            control("MCP 끄기", Message::SetMcpEnabled(false)).on_press_maybe(
                app.can_set_mcp(false)
                    .then_some(Message::SetMcpEnabled(false))
            ),
        ]
        .spacing(6),
        quiet("끄면 Claude 중계도 종료됩니다. 다시 켠 뒤 AI 앱에서 재연결하세요."),
        crate::mcp_setup_ui::board(
            &app.mcp_setup,
            app.mcp_endpoint().is_some(),
            &app.mcp_clients(),
            chrono::Local::now()
        ),
        crate::mcp_setup_ui::token_panel(&app.mcp_setup),
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
    c=c.push(title("문서 저장소와 외부 연결")).push(text("개발 기록은 독립된 TimeScheduleData 문서 저장소에 먼저 보관합니다. 저장 경로·문서 가져오기는 docs/document-vault.md를 참고하세요. 모바일·서버 동기화는 아직 제공하지 않습니다. 다이어리와 문서의 수동 편집은 자동 동기화하지 않습니다.").size(14)).push(quiet("상세창 X는 트레이로 숨깁니다. 완전 종료는 왼쪽 앱 종료 또는 트레이 메뉴를 사용하세요."));
    scrollable(paper(c))
        .spacing(crate::calendar_ui::SCROLL_GAP)
        .height(Length::Fill)
        .into()
}
/// The two ways of making daily records, the chosen one highlighted.
fn daily_mode_choice(app: &App) -> Element<'_, Message> {
    let choice = |label: &'static str, mode: DailyRecordMode| {
        let chosen = app.preferences.daily_record == mode;
        control(label, Message::SetDailyMode(mode)).style(if chosen {
            theme::primary
        } else {
            theme::secondary
        })
    };
    row![
        choice("버튼으로 만들기", DailyRecordMode::Button),
        choice("자동으로 만들기", DailyRecordMode::Auto),
    ]
    .spacing(6)
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
pub(crate) fn editor_view(app: &App) -> Element<'_, Message> {
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
        use crate::date_choices::Part;
        let picker = match &e.picker {
            Some(EditorPicker::Date(picker)) => Some(picker),
            _ => None,
        };
        let mut date = column![
            quiet(if form.kind == Kind::Task {
                "날짜 (선택, YYYY-MM-DD)"
            } else {
                "날짜 (YYYY-MM-DD)"
            }),
            row![
                crate::date_input::input(&form.date, app.editor_date_context()),
                button(crate::icons::icon(crate::icons::Icon::Calendar))
                    .style(theme::secondary)
                    .padding(8)
                    .on_press(if picker.is_some_and(|p| p.part == Part::Day) {
                        Message::DismissEditorPicker
                    } else {
                        Message::DatePicker(Part::Day)
                    })
            ]
            .spacing(4)
            .align_y(Alignment::Center)
        ]
        .spacing(5);
        if let Some(picker) = picker {
            date = date.push(picker.view());
            fields = fields.push(crate::frozen::dismiss_on_outside_press(
                date.into(),
                Message::DismissDatePicker,
            ));
        } else {
            fields = fields.push(date);
        }
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
        // Only a hand-written record takes a typed time; a collected Git or
        // automatic record keeps its source time.
        if form.takes_manual_time() {
            fields = fields.push(input(
                "기록 시각 (한국시간 24시간제 HH:mm, 비우면 미상)",
                &form.time,
                Field::Time,
            ));
        }
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
            fields = fields.push(crate::time_input::view(
                &form.start,
                &form.end,
                e.time_picker(),
            ));
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
        // The button sits beside the field. The editor is at least 650 px wide, which
        // leaves the field room for a path.
        fields = fields.push(
            column![
                quiet("로컬 Git 저장소 경로"),
                row![
                    text_input("로컬 Git 저장소 경로", &form.repo_path)
                        .on_input(|v| Message::Field(Field::Repo(v)))
                        .padding(8)
                        .size(14)
                        .width(Length::Fill),
                    button(text("폴더 찾기").size(13))
                        .padding([8, 10])
                        .style(theme::secondary)
                        .on_press_maybe((!e.picking_repo).then_some(Message::PickRepoFolder))
                ]
                .spacing(6)
                .align_y(Alignment::Center)
            ]
            .spacing(5),
        );
    }
    if !e.error.is_empty() {
        fields = fields.push(text(&e.error).size(13).style(theme::danger));
    }
    // Delete sits at the end of the form, away from the corner where the
    // detail view has `수정` and this view has `저장하기`.
    if form.id.is_some() && !e.saving {
        fields = fields.push(space().height(6)).push(row![
            space().width(Length::Fill),
            button(text(format!("이 {} 삭제", form.kind.label())).size(12))
                .style(button::danger)
                .on_press(Message::Delete)
        ]);
    }
    let mut footer = Row::new().spacing(8).align_y(Alignment::Center);
    if e.saving {
        footer = footer.push(quiet("저장 중…"));
    } else {
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
        scrollable(
            container(fields)
                .id("editor-focus-fields")
                .width(Length::Fill)
        )
        .id(format!("editor-fields-{}", e.generation))
        .spacing(crate::calendar_ui::SCROLL_GAP)
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
