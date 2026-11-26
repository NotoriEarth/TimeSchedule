//! Main diary layout and selected-date inspector.
use crate::{
    app::{App, Message, Page},
    editor::Kind,
    icons::{icon, Icon},
    theme, views,
};
use chrono::{Datelike, Duration, Local};
use iced::widget::{
    button, column, container, mouse_area, responsive, row, scrollable, space, text, text_input,
    Column,
};
use iced::{Alignment, Border, Element, Length};
use timeschedule_core::dates::{calendar_days, week_after_sunday};
use timeschedule_desktop::preferences::{AppTheme, DailyRecordMode};
use views::{control, quiet, title};

const WEEKDAYS: [&str; 7] = ["일", "월", "화", "수", "목", "금", "토"];
pub const INSPECTOR_BREAKPOINT: f32 = 1060.0;

pub fn theme_toggle(app: &App) -> Element<'_, Message> {
    button(
        row![
            icon(Icon::Sun),
            text(match app.preferences.theme {
                AppTheme::Midnight => "밝은 모드",
                AppTheme::Porcelain => "어두운 모드",
            })
            .size(12)
        ]
        .spacing(9)
        .align_y(Alignment::Center),
    )
    .padding([9, 10])
    .width(Length::Fill)
    .style(theme::secondary)
    .on_press(Message::ToggleTheme)
    .into()
}

pub fn view(app: &App) -> Element<'_, Message> {
    let p = theme::palette(app.preferences.theme);
    let mut navigation = column![
        container(
            row![
                container(icon(Icon::Calendar)).padding(7).style(|t| {
                    let p = theme::colors(t);
                    container::Style::default()
                        .background(p.accent)
                        .color(p.on_accent)
                        .border(Border {
                            radius: 7.0.into(),
                            ..Default::default()
                        })
                }),
                text("TimeSchedule").size(17)
            ]
            .spacing(10)
            .align_y(Alignment::Center)
        )
        .padding([8, 0]),
        space().height(17),
    ]
    .spacing(5);
    for (page, glyph, label) in [
        (Page::Month, Icon::Calendar, "월간"),
        (Page::Week, Icon::Week, "주간"),
        (Page::Projects, Icon::Folder, "프로젝트"),
        (Page::Records, Icon::Record, "기록 보관함"),
        (Page::Tasks, Icon::Task, "날짜 미정"),
        (Page::Notes, Icon::Note, "노트"),
    ] {
        navigation = navigation.push(
            button(
                row![icon(glyph), text(label).size(14)]
                    .spacing(12)
                    .align_y(Alignment::Center),
            )
            .padding([11, 12])
            .width(Length::Fill)
            .style(if app.page == page {
                theme::navigation
            } else {
                theme::text_button
            })
            .on_press(Message::Navigate(page)),
        );
    }
    navigation = navigation.push(space().height(18)).push(
        row![
            quiet("프로젝트"),
            space().width(Length::Fill),
            button(text("+").size(18))
                .style(theme::text_button)
                .on_press(Message::New(Kind::Project, None))
        ]
        .align_y(Alignment::Center),
    );
    // Every project gets a shortcut; the navigation scrolls when they do not fit.
    for project in &app.snapshot.projects {
        let open = app.page == Page::Projects && app.project.as_ref() == Some(&project.id);
        navigation = navigation.push(
            button(
                row![
                    text("•").color(p.accent),
                    text(views::preview(&project.title, 13)).size(12)
                ]
                .spacing(9),
            )
            .padding([7, 10])
            .width(Length::Fill)
            .style(if open {
                theme::navigation
            } else {
                theme::text_button
            })
            .on_press(Message::Project(Some(project.id.clone()))),
        );
    }
    if app.snapshot.projects.is_empty() {
        navigation = navigation.push(quiet("프로젝트를 추가해 보세요."));
    }
    // The navigation scrolls independently; the theme and settings remain reachable.
    let sidebar = container(
        column![
            scrollable(navigation)
                .spacing(crate::calendar_ui::SCROLL_GAP)
                .height(Length::Fill),
            theme_toggle(app),
            button(row![icon(Icon::Settings), text("설정").size(13)].spacing(12))
                .padding([9, 12])
                .width(Length::Fill)
                .style(if app.page == Page::Settings {
                    theme::navigation
                } else {
                    theme::text_button
                })
                .on_press(Message::Navigate(Page::Settings)),
            button(text("앱 종료").size(11))
                .padding([6, 12])
                .style(theme::text_button)
                .on_press(Message::Quit),
        ]
        .spacing(8)
        .height(Length::Fill),
    )
    .padding(14)
    .width(194)
    .height(Length::Fill)
    .style(theme::sidebar);
    let heading = match app.page {
        Page::Month => format!("{}년 {}월", app.month.year(), app.month.month()),
        Page::Week => format!(
            "{} — {}",
            app.week.format("%m.%d"),
            (app.week + Duration::days(6)).format("%m.%d")
        ),
        Page::Projects => app
            .snapshot
            .projects
            .iter()
            .find(|p| Some(&p.id) == app.project.as_ref())
            .map(|p| views::preview(&p.title, 24))
            .unwrap_or("프로젝트".into()),
        _ => app.page.label().into(),
    };
    let heading: Element<'_, Message> = if app.page == Page::Month {
        use crate::month_jump::{JumpField, JumpMessage, JumpTarget};
        row![
            button(text(format!("{}년", app.month.year())).size(23))
                .padding([2, 4])
                .style(theme::text_button)
                .on_press(Message::Jump(JumpMessage::Open(
                    JumpTarget::Diary,
                    JumpField::Year
                ))),
            button(text(format!("{}월", app.month.month())).size(23))
                .padding([2, 4])
                .style(theme::text_button)
                .on_press(Message::Jump(JumpMessage::Open(
                    JumpTarget::Diary,
                    JumpField::Month
                ))),
        ]
        .into()
    } else {
        title(heading)
    };
    let search = text_input("일정, 프로젝트, 기록 검색", &app.search)
        .on_input(|s| Message::Search(s.chars().take(200).collect()))
        .width(220)
        .size(12)
        .padding(9);
    // ‘오늘’ moves the calendars only, so only the calendars show it.
    let mut heading_row = row![heading, space().width(Length::Fill)]
        .spacing(8)
        .align_y(Alignment::Center);
    if matches!(app.page, Page::Month | Page::Week) {
        heading_row = heading_row.push(control("오늘", Message::Today));
    }
    heading_row = heading_row.push(
        button(text("↻").size(18))
            .style(theme::text_button)
            .on_press(Message::Load),
    );
    let mut top = column![
        row![
            quiet(format!("{}", Local::now().format("%Y년 %m월 %d일"))),
            space().width(Length::Fill),
            search
        ]
        .align_y(Alignment::Center),
        heading_row,
    ]
    .spacing(12);
    let toolbar: Element<'_, Message> = if app.page == Page::Month {
        row![
            control("‹", Message::Month(-1)),
            control("›", Message::Month(1)),
            space().width(Length::Fill),
            button(text("월간").size(12))
                .padding([7, 10])
                .style(theme::navigation)
                .on_press(Message::Navigate(Page::Month)),
            control(
                "주간",
                Message::OpenWeek(
                    app.selected
                        - Duration::days(app.selected.weekday().num_days_from_monday() as i64 + 1)
                )
            ),
            control(
                "카테고리",
                crate::category_ui::msg(crate::category_ui::Action::Open)
            ),
            control("+ 일정", Message::New(Kind::Event, None)).style(theme::primary),
        ]
        .spacing(6)
        .align_y(Alignment::Center)
        .into()
    } else {
        views::actions(app)
    };
    top = top.push(toolbar);
    // Omit the native banner only while the fixed settings status is rendered: the
    // widget section of Settings. Search replaces that content even when Settings
    // remains the selected page, and the other sections do not show it.
    let native_errors = if app.page == Page::Settings
        && app.settings_section == crate::app::SettingsSection::Widget
        && app.search.trim().is_empty()
    {
        String::new()
    } else {
        app.widget_errors()
    };
    let error = [app.error.as_str(), native_errors.as_str()]
        .into_iter()
        .filter(|message| !message.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    if !error.is_empty() {
        top = top.push(
            container(text(error).size(12))
                .padding(8)
                .width(Length::Fill)
                .style(theme::alert),
        );
    }
    let content = if !app.search.trim().is_empty() {
        views::search_results(app)
    } else {
        match app.page {
            Page::Month => month(app),
            Page::Week => views::week(app),
            Page::Projects => views::projects(app),
            Page::Records => views::records(app),
            Page::Tasks => views::tasks(app),
            Page::Notes => views::notes(app),
            Page::Settings => views::settings(app),
        }
    };
    let mut main = column![top, content].spacing(16).height(Length::Fill);
    if app.loading {
        main = main.push(quiet("기록을 불러오는 중…"));
    } else if !app.notice.is_empty() {
        main = main.push(quiet(&app.notice));
    }
    container(
        row![
            sidebar,
            container(main)
                .padding(18)
                .width(Length::Fill)
                .height(Length::Fill)
        ]
        .height(Length::Fill),
    )
    .style(theme::root)
    .into()
}

pub(crate) fn calendar(app: &App) -> Element<'_, Message> {
    let p = theme::palette(app.preferences.theme);
    let today = Local::now().date_naive();
    let dates = calendar_days(app.month);
    let mut headings = row![container(quiet("주")).width(26).center_x(26)];
    for (i, label) in WEEKDAYS.iter().enumerate() {
        headings = headings.push(
            container(text(*label).size(12).color(if i == 0 {
                p.rose
            } else if i == 6 {
                p.blue
            } else {
                p.muted
            }))
            .center_x(Length::Fill),
        );
    }
    let mut grid = column![container(headings).padding([10, 0])].spacing(1);
    for days in dates.chunks(7) {
        let sunday = days[0];
        let selector = mouse_area(
            button(text("›").size(14))
                .padding(0)
                .width(26)
                .height(Length::Fill)
                .style(theme::text_button)
                .on_press(Message::OpenWeek(sunday)),
        )
        .on_enter(Message::HoverWeek(Some(sunday)))
        .on_exit(Message::HoverWeek(None));
        let mut cells = row![selector].spacing(1).height(Length::Fill);
        for date in days {
            let date = *date;
            let key = date.to_string();
            let day = app.snapshot.days.iter().find(|d| d.date == key);
            let selected = date == app.selected;
            let outside = date.month() != app.month.month();
            let weekday = date.weekday().num_days_from_sunday();
            let highlight = app
                .hovered_week
                .and_then(|s| week_after_sunday(s).ok())
                .is_some_and(|w| date >= w.from && date <= w.to);
            let content = responsive(move |size| {
                let label = container(text(date.day().to_string()).size(13))
                    .center_x(27)
                    .center_y(27)
                    .style(move |_| {
                        container::Style::default()
                            .background(if selected {
                                p.accent
                            } else {
                                iced::Color::TRANSPARENT
                            })
                            .color(if selected {
                                p.on_accent
                            } else if outside {
                                p.muted
                            } else if weekday == 0 {
                                p.rose
                            } else if weekday == 6 {
                                p.blue
                            } else {
                                p.text
                            })
                            .border(Border {
                                radius: 14.0.into(),
                                color: p.accent,
                                width: if date == today && !selected { 1.0 } else { 0.0 },
                            })
                    });
                let cell = column![
                    label,
                    crate::calendar_ui::day_items(
                        app,
                        day,
                        size.width,
                        (size.height - 31.0).max(0.0),
                        12.0,
                        false
                    )
                ]
                .spacing(4);
                cell.into()
            });
            cells = cells.push(
                button(
                    mouse_area(content)
                        .on_press(Message::SelectDay(date))
                        .on_double_click(Message::New(Kind::Event, Some(date.to_string()))),
                )
                .padding(6)
                .width(Length::Fill)
                .height(Length::Fill)
                .clip(true)
                .on_press(Message::SelectDay(date))
                .style(move |_, status| {
                    let hovered =
                        matches!(status, button::Status::Hovered | button::Status::Pressed);
                    button::Style {
                        background: Some(
                            if selected {
                                p.selected
                            } else if hovered || highlight {
                                p.raised
                            } else {
                                p.background
                            }
                            .into(),
                        ),
                        text_color: p.text,
                        border: Border {
                            color: if selected { p.accent } else { p.line },
                            width: 1.0,
                            radius: 2.0.into(),
                        },
                        ..Default::default()
                    }
                }),
            );
        }
        grid = grid.push(cells);
    }
    let calendar = column![
        grid.height(Length::Fill),
        quiet("한 번 클릭해 상세 보기 · 두 번 클릭해 일정 추가 · 왼쪽 › 주간 보기")
    ]
    .spacing(8)
    .width(Length::Fill)
    .height(Length::Fill);
    calendar.into()
}

fn month(app: &App) -> Element<'_, Message> {
    let calendar = calendar(app);
    let details = date_details(app);
    if app.manager_size.width < INSPECTOR_BREAKPOINT {
        scrollable(
            column![
                container(calendar).height(660),
                container(details)
                    .padding(18)
                    .width(Length::Fill)
                    .style(theme::inspector)
            ]
            .spacing(16),
        )
        .id("month-compact")
        .spacing(crate::calendar_ui::SCROLL_GAP)
        .height(Length::Fill)
        .into()
    } else {
        row![
            calendar,
            container(
                scrollable(details)
                    .spacing(crate::calendar_ui::SCROLL_GAP)
                    .height(Length::Fill),
            )
            .padding(16)
            .width(270)
            .height(Length::Fill)
            .style(theme::inspector)
        ]
        .spacing(16)
        .height(Length::Fill)
        .into()
    }
}

/// The selected day's buttons. In the button mode of 하루 정리 the day's record is made
/// by a button on its own row, since a third button does not fit beside the other two in
/// the 270-wide panel; it is off while records are being made.
fn date_actions<'a>(app: &'a App, key: &str) -> Column<'a, Message> {
    let mut actions = column![row![
        control("+ 일정", Message::New(Kind::Event, Some(key.to_owned()))),
        control("+ 기록", Message::New(Kind::Entry, Some(key.to_owned())))
    ]
    .spacing(6)]
    .spacing(6);
    if app.preferences.daily_record == DailyRecordMode::Button {
        let working = app.daily_making;
        let label = if working {
            "하루 정리 만드는 중…"
        } else {
            "하루 정리 만들기"
        };
        actions = actions.push(
            control(label, Message::MakeDaily(app.selected))
                .on_press_maybe((!working).then_some(Message::MakeDaily(app.selected))),
        );
    }
    actions
}
fn date_details(app: &App) -> Column<'_, Message> {
    let key = app.selected.to_string();
    let mut details = column![
        text(format!(
            "{}월 {}일",
            app.selected.month(),
            app.selected.day()
        ))
        .size(26),
        date_actions(app, &key)
    ]
    .spacing(10);
    if let Some(day) = app.snapshot.days.iter().find(|d| d.date == key) {
        for item in &day.items {
            details = details.push(crate::calendar_ui::item_view(app, item, 13.0, false));
        }
        if day.completed > 0 || day.total > day.items.len() as u32 {
            details = details.push(control(
                format!("전체 {}개 · 완료 {}개", day.total, day.completed),
                Message::ManagerDate(app.selected),
            ));
        }
    } else {
        details = details.push(quiet("일정과 기록을 추가해 보세요."));
    }
    details
}
