//! Desktop calendar presentation; native window attachment stays in the adapter.
use crate::{
    app::{App, Message, NativeAction},
    month_jump::{JumpField, JumpMessage},
    text_layout::fit_label_with_font,
    theme,
    widget_appearance::Appearance,
    widget_settings::Change,
};
use chrono::{Datelike, Local};
use iced::widget::{
    button, checkbox, column, container, mouse_area, responsive, row, scrollable, slider, space,
    text, Row,
};
use iced::{Alignment, Border, Element, Length};

// Keep the widget's typography and geometry together for later design changes.
const SUMMARY_HEIGHT: f32 = 14.0;
const CONTENT_GAP: f32 = 4.0;
const WEEKDAYS: [&str; 7] = [
    "일요일",
    "월요일",
    "화요일",
    "수요일",
    "목요일",
    "금요일",
    "토요일",
];

fn date_label(
    date: chrono::NaiveDate,
    outside: bool,
    width: f32,
    size: f32,
    font: iced::Font,
) -> String {
    let long = format!("{}/1", date.month());
    if outside
        && date.day() == 1
        && crate::text_layout::text_width_with_font(&long, size, font) + 8.0 <= width
    {
        long
    } else {
        date.day().to_string()
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn enlarged_month_boundary_label_uses_measured_width() {
        let date = chrono::NaiveDate::from_ymd_opt(2026, 10, 1).unwrap();
        let font = crate::widget_appearance::font("Arial");
        let label = super::date_label(date, true, 54., 32., font);
        assert_eq!(label, "1");
        assert!(crate::text_layout::text_width_with_font(&label, 32., font) + 8. <= 54.);
        assert_eq!(super::date_label(date, true, 100., 32., font), "10/1");
    }
}

fn control<'a>(label: &'a str, message: Message) -> iced::widget::Button<'a, Message> {
    button(text(label).size(12))
        .padding([5, 8])
        .style(theme::text_button)
        .on_press(message)
}

pub fn view(app: &App) -> Element<'_, Message> {
    responsive(move |size| {
        let width = (size.width - 16.0).max(0.0);
        let a = Appearance::resolve(&app.preferences);
        let rows = app.widget_days().len() / 7;
        let minimum = (a.date_height + CONTENT_GAP + SUMMARY_HEIGHT + a.padding * 2.0)
            * rows as f32
            + a.gap * rows as f32
            + if width < 560.0 { 160.0 } else { 130.0 };
        let height = (size.height - 16.0).max(minimum);
        let base: Element<'_, Message> = container(
            scrollable(content(app, width).height(height))
                .spacing(4)
                .height(Length::Fill),
        )
        .padding(8)
        .width(Length::Fill)
        .height(Length::Fill)
        .style(|_| container::Style::default())
        .into();
        if app.preferences.widget_locked {
            base
        } else {
            let grip = mouse_area(
                container(text("↘").size(18))
                    .center_x(26)
                    .center_y(26)
                    .style(move |_| {
                        container::Style::default()
                            .background(a.colors.background.scale_alpha(0.95))
                    }),
            )
            .on_press(Message::ResizeWidget)
            .interaction(iced::mouse::Interaction::ResizingDiagonallyDown);
            iced::widget::stack![
                base,
                container(grip)
                    .align_right(Length::Fill)
                    .align_bottom(Length::Fill)
            ]
            .into()
        }
    })
    .into()
}

fn content(app: &App, width: f32) -> iced::widget::Column<'_, Message> {
    let a = Appearance::resolve(&app.preferences);
    let p = a.colors;
    let font = a.font;
    let background_alpha = f32::from(app.preferences.widget_opacity) / 100.0;
    let today = Local::now().date_naive();
    let heading_size = if width < 420.0 { 20 } else { 24 };
    let heading = column![
        text(format!(
            "{} · 일정 {}개",
            today.format("%Y년 %m월 %d일"),
            app.snapshot.today_events.len()
        ))
        .size(11)
        .font(font)
        .color(p.muted),
        row![
            button(
                text(format!("{}년", app.widget_month.year()))
                    .size(heading_size)
                    .font(font)
                    .color(p.text)
            )
            .padding([2, 4])
            .style(theme::text_button)
            .on_press(Message::Jump(JumpMessage::Open(JumpField::Year))),
            button(
                text(format!("{}월", app.widget_month.month()))
                    .size(heading_size)
                    .font(font)
                    .color(p.text)
            )
            .padding([2, 4])
            .style(theme::text_button)
            .on_press(Message::Jump(JumpMessage::Open(JumpField::Month))),
        ]
        .spacing(0)
    ]
    .spacing(3);
    let mut actions = row![
        control("‹", Message::WidgetMonth(-1)),
        control("오늘", Message::WidgetToday),
        control("›", Message::WidgetMonth(1)),
        control("다이어리", Message::ShowManager),
        control("설정", Message::WidgetSetting(Change::Open))
    ]
    .spacing(3)
    .align_y(Alignment::Center);
    if !app.preferences.widget_locked {
        actions = actions.push(
            mouse_area(container(text("이동").size(11)).padding(6))
                .on_press(Message::DragWidget)
                .interaction(iced::mouse::Interaction::Grab),
        );
    }
    let toolbar: Element<'_, Message> = if width < 560.0 {
        column![heading, actions].spacing(5).into()
    } else {
        row![heading, space().width(Length::Fill), actions]
            .align_y(Alignment::Center)
            .into()
    };
    let mut headings = Row::new().spacing(a.gap);
    for (i, label) in WEEKDAYS.iter().enumerate() {
        headings = headings.push(
            container(
                text(if width < 420.0 { &label[..3] } else { label })
                    .size(11)
                    .font(font)
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
    let mut grid = column![headings].spacing(a.gap).height(Length::Fill);
    for week in app.widget_days().chunks(7) {
        let mut cells = Row::new().spacing(a.gap).height(Length::Fill);
        for &date in week {
            let key = date.to_string();
            let events: Vec<_> = app
                .snapshot
                .widget_events
                .iter()
                .filter(|event| event.date == key)
                .collect();
            let is_today = date == today;
            let outside =
                !app.preferences.widget.rolling && date.month() != app.widget_month.month();
            let selected = app.widget_ui.selected == Some(date);
            let weekday = date.weekday().num_days_from_sunday();
            let content = responsive(move |size| {
                let mut date_row = row![
                    container(
                        text(date_label(date, outside, size.width, a.date_size, font))
                            .size(a.date_size)
                            .font(font)
                    )
                    .padding([0, 4])
                    .height(a.date_height)
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
                    date_row = date_row.push(text("TODAY").size(9).font(font).color(p.accent));
                }
                let mut lines = column![date_row].spacing(CONTENT_GAP);
                let remaining = (size.height - a.date_height - CONTENT_GAP).max(0.0);
                let slots = (remaining / a.event_height).floor() as usize;
                let visible = if events.len() > slots {
                    ((remaining - SUMMARY_HEIGHT).max(0.0) / a.event_height).floor() as usize
                } else {
                    events.len()
                }
                .min(50);
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
                        let ink = a.event_color(&event.color);
                        let completion_enabled = !app.busy && !app.loading && app.editor.is_none();
                        let completion_id = event.id.clone();
                        let completed = event.completed;
                        entries = entries.push(
                            row![
                                // Consume even disabled clicks so they cannot
                                // become a date double-click in the parent.
                                checkbox(event.completed)
                                    .size(12)
                                    .on_toggle(move |value| {
                                        if completion_enabled {
                                            Message::CompleteWidgetEvent(
                                                completion_id.clone(),
                                                value,
                                            )
                                        } else {
                                            Message::Ignore
                                        }
                                    })
                                    .style(move |theme, status| checkbox::primary(
                                        theme,
                                        if completion_enabled {
                                            status
                                        } else {
                                            checkbox::Status::Disabled {
                                                is_checked: completed,
                                            }
                                        }
                                    )),
                                container(space().width(2).height(10))
                                    .style(move |_| container::Style::default().background(ink)),
                                text(fit_label_with_font(
                                    &label,
                                    (size.width - 26.0).max(0.0),
                                    a.event_size,
                                    font
                                ))
                                .size(a.event_size)
                                .font(font)
                                .wrapping(text::Wrapping::None)
                                .color(
                                    if event.completed || outside {
                                        p.muted
                                    } else {
                                        p.text
                                    }
                                )
                            ]
                            .spacing(4)
                            .height(a.event_height)
                            .align_y(Alignment::Center),
                        );
                    }
                    // Keep the list and its summary together so the reserved
                    // summary row has no extra inter-section spacing.
                    if events.len() > visible {
                        entries = entries.push(
                            text(fit_label_with_font(
                                &format!("+{}개 더", events.len() - visible),
                                size.width,
                                10.0,
                                font,
                            ))
                            .size(10)
                            .font(font)
                            .height(SUMMARY_HEIGHT)
                            .color(p.muted),
                        );
                    }
                    lines = lines.push(entries);
                } else if !events.is_empty() && remaining >= SUMMARY_HEIGHT {
                    lines = lines.push(
                        text(fit_label_with_font(
                            &format!("일정 {}개", events.len()),
                            size.width,
                            10.0,
                            font,
                        ))
                        .size(10)
                        .font(font)
                        .height(SUMMARY_HEIGHT)
                        .color(p.muted),
                    );
                }
                lines.into()
            });
            cells = cells.push(
                mouse_area(
                    container(content)
                        .padding(a.padding)
                        .width(Length::Fill)
                        .height(Length::Fill)
                        .clip(true)
                        .style(move |_| container::Style {
                            background: Some(
                                if is_today {
                                    p.selected
                                } else if outside {
                                    p.sidebar
                                } else {
                                    p.background
                                }
                                .scale_alpha(background_alpha)
                                .into(),
                            ),
                            text_color: Some(p.text),
                            border: Border {
                                color: if is_today || selected {
                                    p.accent
                                } else {
                                    p.line
                                },
                                width: if is_today || selected { 1.5 } else { 0.5 },
                                radius: 0.0.into(),
                            },
                            ..Default::default()
                        }),
                )
                .on_double_click(Message::WidgetDate(date))
                .interaction(iced::mouse::Interaction::Pointer),
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
                    "날짜 더블클릭 · 체크로 완료"
                } else {
                    "이동 영역을 드래그 · 오른쪽 아래 크기 조정"
                })
                .size(11)
                .color(p.muted),
            )
            .push(space().width(Length::Fill));
    }
    footer = footer
        .push(
            text(format!("배경 {}%", app.preferences.widget_opacity))
                .size(11)
                .color(p.muted),
        )
        .push(
            slider(40..=100, app.preferences.widget_opacity, |v| {
                Message::Native(NativeAction::Opacity(v))
            })
            .width(if width < 420.0 { 70 } else { 100 }),
        );
    if !app.preferences.widget_locked {
        footer = footer.push(space().width(22));
    }
    let min_grid_width = 7.0
        * (crate::text_layout::text_width_with_font("88", a.date_size, font)
            + 8.0
            + a.padding * 2.0)
        + a.gap * 6.0;
    let grid: Element<'_, Message> = if min_grid_width > width {
        scrollable(grid.width(min_grid_width))
            .direction(scrollable::Direction::Horizontal(
                scrollable::Scrollbar::default(),
            ))
            .height(Length::Fill)
            .into()
    } else {
        grid.into()
    };
    let panel = move |_: &iced::Theme| {
        container::Style::default().background(p.background.scale_alpha(background_alpha))
    };
    let mut body = column![
        container(toolbar)
            .padding(4)
            .width(Length::Fill)
            .style(panel),
        grid,
        container(footer)
            .padding(4)
            .width(Length::Fill)
            .style(panel)
    ]
    .spacing(6)
    .width(Length::Fill);
    if app.preferences.widget.rolling {
        let range = app.widget_range();
        body = column![
            text(format!(
                "{} – {} · {}주",
                range.from.format("%Y.%m.%d"),
                range.to.format("%Y.%m.%d"),
                app.preferences.widget.weeks
            ))
            .size(11)
            .font(font)
            .color(p.muted),
            body.height(Length::Fill)
        ]
        .spacing(3)
        .width(Length::Fill);
    }
    if !app.widget_attached {
        body = body.push(
            button(text("바탕화면 연결 재시도").size(11))
                .style(theme::text_button)
                .on_press(Message::Native(NativeAction::Attach)),
        );
    }
    if !app.error.is_empty() {
        body = body.push(
            container(text(&app.error).size(11).color(p.danger))
                .padding(4)
                .style(panel),
        );
    }
    body
}
