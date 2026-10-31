//! Desktop calendar presentation; native window attachment stays in the adapter.
use crate::{
    app::{App, Message, NativeAction},
    text_layout::fit_label,
    theme,
};
use chrono::{Datelike, Local};
use iced::widget::{
    button, column, container, mouse_area, responsive, row, scrollable, slider, space, text, Row,
};
use iced::{Alignment, Border, Element, Length};
use timeschedule_core::dates::calendar_days;

// Keep the widget's typography and geometry together for later design changes.
const DATE_SIZE: f32 = 15.0;
const EVENT_SIZE: f32 = 11.0;
const DATE_HEIGHT: f32 = 20.0;
const EVENT_HEIGHT: f32 = 17.0;
const SUMMARY_HEIGHT: f32 = 14.0;
const CELL_PADDING: f32 = 6.0;
const CONTENT_GAP: f32 = 4.0;
const MIN_CONTENT_HEIGHT: f32 = 430.0;
const WEEKDAYS: [&str; 7] = [
    "일요일",
    "월요일",
    "화요일",
    "수요일",
    "목요일",
    "금요일",
    "토요일",
];

fn control<'a>(label: &'a str, message: Message) -> iced::widget::Button<'a, Message> {
    button(text(label).size(12))
        .padding([5, 8])
        .style(theme::text_button)
        .on_press(message)
}

pub fn view(app: &App) -> Element<'_, Message> {
    responsive(move |size| {
        let width = (size.width - 16.0).max(0.0);
        let height = (size.height - 16.0).max(MIN_CONTENT_HEIGHT);
        container(
            scrollable(content(app, width).height(height))
                .spacing(4)
                .height(Length::Fill),
        )
        .padding(8)
        .width(Length::Fill)
        .height(Length::Fill)
        .style(theme::root)
        .into()
    })
    .into()
}

fn content(app: &App, width: f32) -> iced::widget::Column<'_, Message> {
    let p = theme::palette(app.preferences.theme);
    let today = Local::now().date_naive();
    let heading = text(format!(
        "{} · {:02}",
        app.widget_month.year(),
        app.widget_month.month()
    ))
    .size(if width < 420.0 { 20 } else { 24 });
    let mut drag_title = mouse_area(container(heading).padding([2, 4]));
    if !app.preferences.widget_locked {
        drag_title = drag_title
            .on_press(Message::DragWidget)
            .interaction(iced::mouse::Interaction::Grab);
    }
    let mut toolbar = row![drag_title].spacing(4).align_y(Alignment::Center);
    if width >= 680.0 {
        toolbar = toolbar.push(
            text(format!(
                "오늘 {} ({}) · 일정 {}개",
                today.format("%m.%d"),
                WEEKDAYS[today.weekday().num_days_from_sunday() as usize],
                app.snapshot.today_events.len()
            ))
            .size(11)
            .color(p.muted),
        );
    }
    toolbar = toolbar
        .push(space().width(Length::Fill))
        .push(control("‹", Message::WidgetMonth(-1)))
        .push(control("오늘", Message::WidgetToday))
        .push(control("›", Message::WidgetMonth(1)))
        .push(control("다이어리", Message::ShowManager));
    let mut headings = Row::new().spacing(1);
    for (i, label) in WEEKDAYS.iter().enumerate() {
        headings = headings.push(
            container(
                text(if width < 420.0 { &label[..3] } else { label })
                    .size(11)
                    .color(if i == 0 {
                        p.rose
                    } else if i == 6 {
                        p.blue
                    } else {
                        p.muted
                    }),
            )
            .center_x(Length::Fill)
            .center_y(25),
        );
    }
    let mut grid = column![headings].spacing(1).height(Length::Fill);
    for week in calendar_days(app.widget_month).chunks(7) {
        let mut cells = Row::new().spacing(1).height(Length::Fill);
        for &date in week {
            let key = date.to_string();
            let events: Vec<_> = app
                .snapshot
                .widget_events
                .iter()
                .filter(|event| event.date == key)
                .collect();
            let is_today = date == today;
            let outside = date.month() != app.widget_month.month();
            let weekday = date.weekday().num_days_from_sunday();
            let content = responsive(move |size| {
                let mut date_row = row![
                    container(
                        text(if outside && date.day() == 1 && size.width >= 45.0 {
                            format!("{}/1", date.month())
                        } else {
                            date.day().to_string()
                        })
                        .size(DATE_SIZE)
                    )
                    .padding([0, 4])
                    .height(DATE_HEIGHT)
                    .style(move |_| container::Style::default()
                        .background(if is_today {
                            p.accent
                        } else {
                            iced::Color::TRANSPARENT
                        })
                        .color(if is_today {
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
                            radius: 3.0.into(),
                            ..Default::default()
                        })),
                    space().width(Length::Fill)
                ]
                .align_y(Alignment::Center);
                if is_today && size.width >= 90.0 {
                    date_row = date_row.push(text("TODAY").size(9).color(p.accent));
                }
                let mut lines = column![date_row].spacing(CONTENT_GAP);
                let remaining = (size.height - DATE_HEIGHT - CONTENT_GAP).max(0.0);
                let slots = (remaining / EVENT_HEIGHT).floor() as usize;
                let visible = if events.len() > slots {
                    ((remaining - SUMMARY_HEIGHT).max(0.0) / EVENT_HEIGHT).floor() as usize
                } else {
                    events.len()
                }
                .min(6);
                if visible > 0 {
                    let mut entries = column![].spacing(0);
                    for event in events.iter().take(visible) {
                        let time = if !event.all_day && size.width >= 120.0 {
                            format!("{}  ", event.start_time.as_deref().unwrap_or(""))
                        } else {
                            String::new()
                        };
                        let label = format!(
                            "{}{}{}",
                            if event.completed { "✓ " } else { "" },
                            time,
                            event.title
                        );
                        let ink = theme::event_color(p, &event.color);
                        entries = entries.push(
                            row![
                                container(space().width(2).height(10))
                                    .style(move |_| container::Style::default().background(ink)),
                                text(fit_label(&label, (size.width - 8.0).max(0.0), EVENT_SIZE))
                                    .size(EVENT_SIZE)
                                    .wrapping(text::Wrapping::None)
                                    .color(if event.completed || outside {
                                        p.muted
                                    } else {
                                        p.text
                                    })
                            ]
                            .spacing(6)
                            .height(EVENT_HEIGHT)
                            .align_y(Alignment::Center),
                        );
                    }
                    // Keep the list and its summary together so the reserved
                    // summary row has no extra inter-section spacing.
                    if events.len() > visible {
                        entries = entries.push(
                            text(fit_label(
                                &format!("+{}개 더", events.len() - visible),
                                size.width,
                                10.0,
                            ))
                            .size(10)
                            .height(SUMMARY_HEIGHT)
                            .color(p.muted),
                        );
                    }
                    lines = lines.push(entries);
                } else if !events.is_empty() && remaining >= SUMMARY_HEIGHT {
                    lines = lines.push(
                        text(fit_label(
                            &format!("일정 {}개", events.len()),
                            size.width,
                            10.0,
                        ))
                        .size(10)
                        .height(SUMMARY_HEIGHT)
                        .color(p.muted),
                    );
                }
                lines.into()
            });
            cells = cells.push(
                button(content)
                    .padding(CELL_PADDING)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .clip(true)
                    .on_press(Message::WidgetDate(date))
                    .style(move |_, status| button::Style {
                        background:
                            Some(
                                if matches!(
                                    status,
                                    button::Status::Hovered | button::Status::Pressed
                                ) {
                                    p.raised
                                } else if is_today {
                                    p.selected
                                } else if outside {
                                    p.sidebar
                                } else {
                                    p.background
                                }
                                .into(),
                            ),
                        text_color: p.text,
                        border: Border {
                            color: if is_today { p.accent } else { p.line },
                            width: if is_today { 1.5 } else { 0.5 },
                            radius: 0.0.into(),
                        },
                        ..Default::default()
                    }),
            );
        }
        grid = grid.push(cells);
    }
    let mut footer = row![
        control(
            if app.preferences.widget_locked {
                "잠금 해제"
            } else {
                "위치 잠금"
            },
            Message::Native(NativeAction::Lock(!app.preferences.widget_locked))
        ),
        space().width(Length::Fill),
    ]
    .spacing(8)
    .align_y(Alignment::Center);
    if width >= 680.0 {
        footer = footer
            .push(
                text(if app.preferences.widget_locked {
                    "날짜 클릭으로 일정 추가"
                } else {
                    "월 제목을 드래그해 이동"
                })
                .size(11)
                .color(p.muted),
            )
            .push(space().width(Length::Fill));
    }
    footer = footer
        .push(
            text(format!("불투명도 {}%", app.preferences.widget_opacity))
                .size(11)
                .color(p.muted),
        )
        .push(
            slider(40..=100, app.preferences.widget_opacity, |v| {
                Message::Native(NativeAction::Opacity(v))
            })
            .width(100),
        );
    let mut body = column![toolbar, grid, footer]
        .spacing(6)
        .width(Length::Fill);
    if !app.widget_attached {
        body = body.push(
            button(text("바탕화면 연결 재시도").size(11))
                .style(theme::text_button)
                .on_press(Message::Native(NativeAction::Attach)),
        );
    }
    body
}
