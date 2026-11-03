//! One item renderer and detail route for diary, widget and day popup.
use crate::{
    app::{App, Message},
    editor::{Form, Kind},
    theme, worker,
};
use iced::widget::{button, checkbox, column, container, row, scrollable, space, text, Column};
use iced::{Color, Element, Length, Task};
use timeschedule_core::calendar::{CalendarDay, CalendarItem, CalendarPage, CalendarSource};

#[derive(Default)]
pub struct State {
    pub quick: CalendarPage,
    pub offset: u32,
    pub show_completed: bool,
    pub completed_page: CalendarPage,
    pub completed_offset: u32,
    pub completed_loading: bool,
    pub detail: Option<Form>,
    pub detail_quick: bool,
    pub generation: u64,
}
#[derive(Debug, Clone)]
pub enum Action {
    Open(CalendarSource, String, bool),
    Detail(u64, bool, Result<Form, String>),
    CloseDetail,
    EditDetail,
    New(Kind, bool),
    Completed(CalendarSource, String, bool),
    ToggleCompleted,
    Page(i32),
    QuickPage(u64, bool, Result<CalendarPage, String>),
    CompletedPage(i32),
    CompletedLoaded(u64, Result<CalendarPage, String>),
}
pub fn message(action: Action) -> Message {
    Message::Calendar(action)
}
pub fn update(app: &mut App, action: Action) -> Task<Message> {
    match action {
        Action::CompletedLoaded(g, result)
            if g == app.quick_generation && app.quick_window.is_some() =>
        {
            app.calendar.completed_loading = false;
            match result {
                Ok(page) => app.calendar.completed_page = page,
                Err(e) => app.error = e,
            }
        }
        Action::CompletedPage(delta) if !app.calendar.completed_loading => {
            if delta < 0 {
                app.calendar.completed_offset = app.calendar.completed_offset.saturating_sub(50)
            } else if app.calendar.completed_page.has_more {
                app.calendar.completed_offset = app.calendar.completed_offset.saturating_add(50)
            } else {
                return Task::none();
            }
            return load_completed(app);
        }
        Action::QuickPage(g, initial, result)
            if g == app.quick_generation && app.quick_window.is_some() =>
        {
            app.quick_loading = false;
            match result {
                Ok(page) => {
                    let empty = page.total == 0;
                    app.calendar.quick = page;
                    if initial && empty && app.editor.is_none() {
                        return app.edit_quick();
                    }
                }
                Err(e) => app.error = e,
            }
        }
        Action::Open(source, id, quick) if app.editor.is_none() && !app.busy && !app.loading => {
            let Some(store) = app.store.clone() else {
                return Task::none();
            };
            let date = app
                .snapshot
                .widget_days
                .iter()
                .flat_map(|d| &d.items)
                .find(|i| i.source == source && i.id == id)
                .and_then(|i| i.date.parse().ok());
            let open = if quick
                && date.is_some()
                && (app.quick_window.is_none() || app.quick_date != date)
            {
                app.open_quick(date.unwrap())
            } else {
                Task::none()
            };
            app.calendar.generation += 1;
            let generation = app.calendar.generation;
            app.calendar.detail = None;
            let detail = worker::job(
                store,
                move |s| {
                    let form = match source {
                        CalendarSource::Event => Form::event(s.get_event(&id)?),
                        CalendarSource::Task => Form::task(s.task_edit(&id)?.task),
                        CalendarSource::Entry => Form::entry(s.get_entry(&id)?),
                    };
                    worker::hydrate_form(s, form)
                },
                move |r| message(Action::Detail(generation, quick, r)),
            );
            return Task::batch([open, detail]);
        }
        Action::Detail(g, quick, result)
            if g == app.calendar.generation && app.editor.is_none() =>
        {
            if quick && app.quick_window.is_none() {
                return Task::none();
            }
            match result {
                Ok(form) => {
                    app.calendar.detail = Some(form);
                    app.calendar.detail_quick = quick
                }
                Err(e) => {
                    app.calendar.detail = None;
                    app.error = e;
                }
            }
        }
        Action::CloseDetail => {
            app.calendar.detail = None;
            app.calendar.generation += 1;
        }
        Action::EditDetail if !app.loading && !app.busy => {
            if let Some(form) = app.calendar.detail.take() {
                let quick = app.calendar.detail_quick;
                let task = app.open_form(form);
                app.editor_in_quick = quick && app.editor.is_some();
                return task;
            }
        }
        Action::New(kind, quick) if app.editor.is_none() => {
            app.calendar.detail = None;
            let date = if quick {
                app.quick_date.unwrap_or(app.selected)
            } else {
                app.selected
            };
            let task = app.open_form(Form::new(kind, date.to_string(), None));
            app.editor_in_quick = quick && app.editor.is_some();
            return task;
        }
        Action::Completed(source, id, value)
            if !app.busy && !app.loading && app.editor.is_none() =>
        {
            if let Some(store) = app.store.clone() {
                app.busy = true;
                app.calendar.detail = None;
                app.calendar.generation += 1;
                return worker::job(
                    store,
                    move |s| s.set_calendar_completed(source, &id, value),
                    Message::MutationDone,
                );
            }
        }
        Action::ToggleCompleted => {
            app.calendar.show_completed = !app.calendar.show_completed;
            if app.calendar.show_completed {
                app.calendar.completed_offset = 0;
                return load_completed(app);
            }
        }
        Action::Page(delta) if !app.quick_loading => {
            if delta < 0 {
                app.calendar.offset = app.calendar.offset.saturating_sub(50)
            } else if app.calendar.quick.has_more {
                app.calendar.offset = app.calendar.offset.saturating_add(50)
            } else {
                return Task::none();
            }
            return app.load_quick(false);
        }
        _ => {}
    }
    Task::none()
}
pub fn load_completed(app: &mut App) -> Task<Message> {
    let (Some(store), Some(date), Some(_)) = (app.store.clone(), app.quick_date, app.quick_window)
    else {
        return Task::none();
    };
    let g = app.quick_generation;
    let offset = app.calendar.completed_offset;
    app.calendar.completed_loading = true;
    worker::job(
        store,
        move |s| s.calendar_section(&date.to_string(), offset, true),
        move |r| message(Action::CompletedLoaded(g, r)),
    )
}
pub fn color(app: &App, id: &str) -> Color {
    app.snapshot
        .categories
        .iter()
        .find(|c| c.id == id)
        .and_then(|c| parse_color(&c.color))
        .unwrap_or(theme::palette(app.preferences.theme).accent)
}
pub fn parse_color(value: &str) -> Option<Color> {
    let value = u32::from_str_radix(value.strip_prefix('#')?, 16).ok()?;
    Some(Color::from_rgb8(
        (value >> 16) as u8,
        (value >> 8) as u8,
        value as u8,
    ))
}
pub fn labels(app: &App, item: &CalendarItem) -> (String, Option<String>) {
    let title = if item.short_title.is_empty() {
        &item.title
    } else {
        &item.short_title
    };
    if item.source == CalendarSource::Entry {
        let project = if !item.project_short_name.is_empty() {
            &item.project_short_name
        } else if !item.project_title.is_empty() {
            &item.project_title
        } else {
            app.snapshot
                .categories
                .iter()
                .find(|c| c.id == item.category_id)
                .map(|c| c.name.as_str())
                .unwrap_or("기록")
        };
        (project.into(), Some(title.clone()))
    } else {
        (
            format!(
                "{}{}",
                item.start_time
                    .as_ref()
                    .filter(|_| !item.all_day)
                    .map(|t| format!("{t} "))
                    .unwrap_or_default(),
                title
            ),
            None,
        )
    }
}
pub fn item_height(item: &CalendarItem, size: f32) -> f32 {
    if item.source == CalendarSource::Entry {
        size * 2.6 + 8.0
    } else {
        size * 1.3 + 6.0
    }
}
pub fn item_view<'a>(
    app: &'a App,
    item: &'a CalendarItem,
    size: f32,
    quick: bool,
) -> Element<'a, Message> {
    iced::widget::responsive(move |bounds| item_contents(app, item, bounds.width, size, quick))
        .width(Length::Fill)
        .height(
            item_height(item, size)
                + if quick {
                    f32::from(app.preferences.widget.line_gap)
                } else {
                    0.0
                },
        )
        .into()
}
fn item_contents<'a>(
    app: &'a App,
    item: &'a CalendarItem,
    width: f32,
    size: f32,
    quick: bool,
) -> Element<'a, Message> {
    let (first, second) = labels(app, item);
    let ink = color(app, &item.category_id);
    let (p, font) = if quick {
        let a = crate::widget_appearance::Appearance::resolve(&app.preferences);
        (a.colors, a.font)
    } else {
        (
            theme::palette(app.preferences.theme),
            iced::Font::with_name("Malgun Gothic"),
        )
    };
    let fit =
        |s: &str| crate::text_layout::fit_label_with_font(s, (width - 34.0).max(0.0), size, font);
    let mut lines = column![text(fit(&first))
        .size(size)
        .font(font)
        .wrapping(text::Wrapping::None)
        .color(p.text)]
    .spacing(2);
    if let Some(second) = second {
        lines = lines.push(
            text(fit(&second))
                .size(size)
                .font(font)
                .wrapping(text::Wrapping::None)
                .color(p.text),
        )
    }
    let open = message(Action::Open(item.source, item.id.clone(), quick));
    let content = button(lines)
        .padding([3, 2])
        .width(Length::Fill)
        .style(theme::text_button)
        .on_press(if app.editor.is_none() && !app.busy && !app.loading {
            open
        } else {
            Message::Ignore
        });
    let mut row = row![
        container(space().width(3).height(item_height(item, size) - 5.0))
            .style(move |_| container::Style::default().background(ink))
    ]
    .spacing(3);
    if item.source != CalendarSource::Entry {
        let id = item.id.clone();
        let source = item.source;
        let enabled = app.editor.is_none() && !app.busy && !app.loading;
        row = row.push(checkbox(item.completed).size(12).on_toggle(move |value| {
            if enabled {
                message(Action::Completed(source, id.clone(), value))
            } else {
                Message::Ignore
            }
        }));
    }
    container(row.push(content).align_y(iced::Alignment::Center))
        .height(
            item_height(item, size)
                + if quick {
                    f32::from(app.preferences.widget.line_gap)
                } else {
                    0.0
                },
        )
        .into()
}
pub fn day_items<'a>(
    app: &'a App,
    day: Option<&'a CalendarDay>,
    width: f32,
    height: f32,
    size: f32,
    quick: bool,
) -> Element<'a, Message> {
    let Some(day) = day else {
        return space().into();
    };
    let mut items = Column::new().spacing(0).height(height);
    // Reserve the footer's actual line height. A fixed 18px budget used to
    // hide the first two-line record even when it and the footer both fit.
    let footer_height = size * 1.3;
    let mut used = 0.0;
    let mut shown = 0;
    for (index, item) in day.items.iter().enumerate() {
        let next = item_height(item, size)
            + if quick {
                f32::from(app.preferences.widget.line_gap)
            } else {
                0.0
            };
        let reserve =
            if day.completed > 0 || day.total.saturating_sub(day.completed) > index as u32 + 1 {
                footer_height
            } else {
                0.0
            };
        if used + next + reserve > height {
            break;
        }
        items = items.push(item_view(app, item, size, quick));
        used += next;
        shown += 1;
    }
    let pending = day
        .total
        .saturating_sub(day.completed)
        .saturating_sub(shown);
    if height - used >= footer_height && (pending > 0 || day.completed > 0) {
        let caption = match (pending, day.completed) {
            (0, n) => format!("완료 {n}개"),
            (n, 0) => format!("+{n}개 더"),
            (n, c) => format!("+{n} · 완료 {c}개"),
        };
        if let Ok(date) = day.date.parse() {
            items = items.push(space().height(Length::Fill)).push(
                button(
                    text(crate::text_layout::fit_label(&caption, width - 4.0, size))
                        .size(size)
                        .line_height(text::LineHeight::Relative(1.3)),
                )
                .padding(0)
                .height(footer_height)
                .style(theme::text_button)
                .on_press(Message::WidgetDate(date)),
            )
        }
    }
    items.into()
}
pub fn detail_view(app: &App) -> Element<'_, Message> {
    let form = app.calendar.detail.as_ref().unwrap();
    let project = app
        .snapshot
        .projects
        .iter()
        .find(|p| Some(&p.id) == form.project_id.as_ref())
        .map(|p| p.title.as_str());
    let mut content = column![
        text(&form.title).size(23),
        text(format!("{} · {}", form.date, form.kind.label())).size(12)
    ]
    .spacing(12);
    if let Some(project) = project {
        content = content.push(text(project).size(15).style(theme::accent))
    }
    if matches!(form.kind, Kind::Event | Kind::Task) {
        content = content.push(
            text(if form.all_day {
                "종일".into()
            } else {
                format!("{}–{}", form.start, form.end)
            })
            .size(13),
        )
    }
    content = content.push(
        text(if form.body.is_empty() {
            "내용이 없습니다."
        } else {
            &form.body
        })
        .size(14),
    );
    if !form.source_url.is_empty() {
        content =
            content.push(button("출처 열기").on_press(Message::Source(form.source_url.clone())))
    }
    column![
        scrollable(content).height(Length::Fill),
        row![
            button("수정").on_press(message(Action::EditDetail)),
            space().width(Length::Fill),
            button("닫기").on_press(message(Action::CloseDetail))
        ]
        .spacing(8)
    ]
    .spacing(16)
    .height(Length::Fill)
    .into()
}
