//! One item renderer and detail route for diary, widget and day popup.
use crate::{
    app::{App, Message},
    editor::{Form, Kind},
    theme, worker,
};
use iced::widget::{button, checkbox, column, container, row, scrollable, space, text};
use iced::{Color, Element, Length, Task};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use timeschedule_core::calendar::{CalendarDay, CalendarItem, CalendarPage, CalendarSource};
use timeschedule_core::diary::{DayEntries, Entry, DAY_PAGE};
use timeschedule_core::storage::Storage;
use timeschedule_core::timeline::{self as times, EntryTimeline};

/// A step of a day's cards with the day's full count, and by ID when each
/// record happened and its evidence.
pub type GroupRecords = (DayEntries, HashMap<String, EntryTimeline>);

/// Space kept between a list and its scrollbar so the bar never covers text.
pub const SCROLL_GAP: f32 = 6.0;
/// A card shows this much of a record until it is unfolded.
const CARD_LINES: usize = 4;
const CARD_CHARS: usize = 220;

/// One project's records of one day, shown as cards.
pub struct Group {
    pub date: String,
    pub project_id: String,
    pub quick: bool,
    /// The cards read so far, in order, each record once.
    pub entries: Vec<Entry>,
    /// Every record of this project and day, read or not.
    pub total: u32,
    pub loading_more: bool,
    pub timelines: HashMap<String, EntryTimeline>,
    pub unfolded: HashSet<String>,
}

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
    pub group: Option<Group>,
    pub generation: u64,
}
#[derive(Debug, Clone)]
pub enum Action {
    Open(CalendarSource, String, bool),
    /// Date, project, and whether it was opened from the widget side.
    OpenGroup(String, String, bool),
    GroupLoaded(u64, String, String, bool, Result<GroupRecords, String>),
    /// Read the next step of the open cards.
    GroupMore,
    GroupMoreLoaded(u64, String, String, Result<GroupRecords, String>),
    CloseGroup,
    ToggleCard(String),
    EditCard(String),
    CardForm(u64, bool, Result<Form, String>),
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
                app.open_quick(date.unwrap(), app.widget.unwrap_or(app.manager))
            } else {
                Task::none()
            };
            app.calendar.generation += 1;
            let generation = app.calendar.generation;
            app.calendar.detail = None;
            app.calendar.group = None;
            let dir = app.data_dir.clone();
            let detail = worker::job(
                store,
                move |s| {
                    let form = match source {
                        CalendarSource::Event => Form::event(s.get_event(&id)?),
                        CalendarSource::Task => Form::task(s.task_edit(&id)?.task),
                        CalendarSource::Entry => {
                            worker::complete_evidence(s, &dir, std::slice::from_ref(&id));
                            Form::entry(s.get_entry(&id)?)
                        }
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
        Action::OpenGroup(date, project_id, quick)
            if app.editor.is_none() && !app.busy && !app.loading =>
        {
            let day = date.parse().ok();
            let open = if quick
                && day.is_some()
                && (app.quick_window.is_none() || app.quick_date != day)
            {
                app.open_quick(day.unwrap(), app.widget.unwrap_or(app.manager))
            } else {
                Task::none()
            };
            app.calendar.generation += 1;
            app.calendar.detail = None;
            app.calendar.group = None;
            return Task::batch([open, load_group(app, date, project_id, quick)]);
        }
        Action::GroupLoaded(g, date, project_id, quick, result) if g == app.calendar.generation => {
            if quick && app.quick_window.is_none() {
                return Task::none();
            }
            match result {
                // The last record was deleted or moved: nothing left to show.
                Ok((page, _)) if page.entries.is_empty() => app.calendar.group = None,
                Ok((page, timelines)) => {
                    let unfolded = app
                        .calendar
                        .group
                        .take()
                        .filter(|g| g.date == date && g.project_id == project_id)
                        .map(|g| g.unfolded)
                        .unwrap_or_default();
                    app.calendar.group = Some(Group {
                        date,
                        project_id,
                        quick,
                        entries: page.entries,
                        total: page.total,
                        loading_more: false,
                        timelines,
                        unfolded,
                    });
                }
                Err(e) => {
                    app.calendar.group = None;
                    app.error = e;
                }
            }
        }
        Action::GroupMore => {
            let (Some(store), Some(group)) = (app.store.clone(), app.calendar.group.as_mut())
            else {
                return Task::none();
            };
            if group.loading_more || group.entries.len() as u32 >= group.total {
                return Task::none();
            }
            group.loading_more = true;
            let generation = app.calendar.generation;
            let (date, project_id) = (group.date.clone(), group.project_id.clone());
            let (d, p) = (date.clone(), project_id.clone());
            let offset = group.entries.len() as u32;
            let dir = app.data_dir.clone();
            return worker::job(
                store,
                move |s| read_group(s, &dir, &d, &p, offset, DAY_PAGE),
                move |r| {
                    message(Action::GroupMoreLoaded(
                        generation,
                        date.clone(),
                        project_id.clone(),
                        r,
                    ))
                },
            );
        }
        Action::GroupMoreLoaded(g, date, project_id, result) if g == app.calendar.generation => {
            if let Some(group) = app
                .calendar
                .group
                .as_mut()
                .filter(|c| c.date == date && c.project_id == project_id)
            {
                group.loading_more = false;
                match result {
                    Ok((page, timelines)) => {
                        group.total = page.total;
                        // A record that moved between steps is shown once.
                        let mut known: HashSet<String> =
                            group.entries.iter().map(|e| e.id.clone()).collect();
                        for entry in page.entries {
                            if known.insert(entry.id.clone()) {
                                group.entries.push(entry);
                            }
                        }
                        group.timelines.extend(timelines);
                    }
                    Err(e) => app.error = e,
                }
            }
        }
        Action::CloseGroup => {
            app.calendar.group = None;
            app.calendar.generation += 1;
        }
        Action::ToggleCard(id) => {
            if let Some(group) = &mut app.calendar.group {
                if !group.unfolded.remove(&id) {
                    group.unfolded.insert(id);
                }
            }
        }
        Action::EditCard(id) if app.editor.is_none() && !app.busy && !app.loading => {
            let (Some(store), Some(group)) = (app.store.clone(), &app.calendar.group) else {
                return Task::none();
            };
            let (generation, quick) = (app.calendar.generation, group.quick);
            return worker::job(
                store,
                move |s| worker::hydrate_form(s, Form::entry(s.get_entry(&id)?)),
                move |r| message(Action::CardForm(generation, quick, r)),
            );
        }
        // Only for the cards it was asked from, while their window is open.
        Action::CardForm(g, quick, result)
            if g == app.calendar.generation
                && app.editor.is_none()
                && app
                    .calendar
                    .group
                    .as_ref()
                    .is_some_and(|c| c.quick == quick)
                && (!quick || app.quick_window.is_some()) =>
        {
            match result {
                Ok(form) => {
                    let task = app.open_form(form);
                    app.editor_in_quick = quick && app.editor.is_some();
                    return task;
                }
                Err(e) => app.error = e,
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
/// One step of a day's cards: the records, the day's full count and, after a
/// past automatic record's evidence is completed once, their timelines.
fn read_group(
    s: &Storage,
    dir: &Path,
    date: &str,
    project_id: &str,
    offset: u32,
    limit: u32,
) -> Result<GroupRecords, String> {
    let page = s.project_day_page(date, project_id, offset, limit)?;
    let ids: Vec<String> = page.entries.iter().map(|e| e.id.clone()).collect();
    worker::complete_evidence(s, dir, &ids);
    let timelines = s.entry_timelines(&ids)?;
    Ok((page, timelines))
}
/// Read the cards of one project's day. Also used to refresh an open group:
/// as many cards as were already read come back (within one page of 200),
/// the rest stay a step away.
pub fn load_group(app: &App, date: String, project_id: String, quick: bool) -> Task<Message> {
    let Some(store) = app.store.clone() else {
        return Task::none();
    };
    let generation = app.calendar.generation;
    let read = app
        .calendar
        .group
        .as_ref()
        .filter(|g| g.date == date && g.project_id == project_id)
        .map_or(0, |g| g.entries.len() as u32);
    let limit = read.clamp(DAY_PAGE, 200);
    let (d, p) = (date.clone(), project_id.clone());
    let dir = app.data_dir.clone();
    worker::job(
        store,
        move |s| read_group(s, &dir, &d, &p, 0, limit),
        move |r| {
            message(Action::GroupLoaded(
                generation,
                date.clone(),
                project_id.clone(),
                quick,
                r,
            ))
        },
    )
}
/// What pressing a calendar line does: a line that stands for several records
/// opens their cards, any other line opens the item itself.
pub fn open_message(item: &CalendarItem, quick: bool) -> Message {
    match &item.project_id {
        Some(project) if item.source == CalendarSource::Entry && item.count > 1 => {
            message(Action::OpenGroup(item.date.clone(), project.clone(), quick))
        }
        _ => message(Action::Open(item.source, item.id.clone(), quick)),
    }
}
/// A project's records of a day show only the project (short name first) and
/// how many there are. None for every other line.
pub fn project_line_name(item: &CalendarItem) -> Option<&str> {
    if item.source != CalendarSource::Entry || item.project_id.is_none() {
        return None;
    }
    [&item.project_short_name, &item.project_title]
        .into_iter()
        .find(|s| !s.trim().is_empty())
        .map(|s| s.as_str())
}
/// `+ 3` after a project's line: every record it stands for, even one. Other
/// lines have no count.
pub fn more_label(item: &CalendarItem) -> Option<String> {
    match project_line_name(item) {
        Some(_) => Some(format!("+ {}", item.count.max(1))),
        None => (item.count > 1).then(|| format!("+{}", item.count - 1)),
    }
}
/// The name a cell and its overflow line use: the project for a project's
/// records (never a record title), the short or full title otherwise.
pub fn cell_title(item: &CalendarItem) -> &str {
    if let Some(name) = project_line_name(item) {
        name
    } else if item.short_title.is_empty() {
        &item.title
    } else {
        &item.short_title
    }
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
pub fn label(item: &CalendarItem) -> String {
    if let Some(name) = project_line_name(item) {
        return name.into();
    }
    let title = if item.short_title.is_empty() {
        &item.title
    } else {
        &item.short_title
    };
    // Like a calendar cell: the project before a record, the time before a
    // timed schedule item.
    let prefix = if item.source == CalendarSource::Entry {
        [&item.project_short_name, &item.project_title]
            .into_iter()
            .find(|s| !s.is_empty())
    } else {
        item.start_time.as_ref().filter(|_| !item.all_day)
    };
    match prefix {
        Some(prefix) => format!("{prefix} {title}"),
        None => title.clone(),
    }
}
pub fn item_height(size: f32) -> f32 {
    size * 1.3 + 6.0
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
            item_height(size)
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
    let first = label(item);
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
    // The count stays whole; only the title before it is cut.
    let more = more_label(item)
        .map(|m| format!(" {m}"))
        .unwrap_or_default();
    let room =
        (width - 34.0 - crate::text_layout::text_width_with_font(&more, size, font)).max(0.0);
    let fitted = crate::text_layout::fit_label_with_font(&first, room, size, font);
    let piece = |value: String, color: Color| {
        text(value)
            .size(size)
            .font(font)
            .wrapping(text::Wrapping::None)
            .color(color)
    };
    // A project's line is its name in the category's colour and its count.
    let lines: Element<'a, Message> = if project_line_name(item).is_some() {
        row![piece(fitted, ink), piece(more, p.muted)].into()
    } else {
        piece(fitted + &more, p.text).into()
    };
    let open = open_message(item, quick);
    let content = button(lines)
        .padding([3, 2])
        .width(Length::Fill)
        .style(theme::text_button)
        .on_press(if app.editor.is_none() && !app.busy && !app.loading {
            open
        } else {
            Message::Ignore
        });
    let mut row = row![container(space().width(3).height(item_height(size) - 5.0))
        .style(move |_| container::Style::default().background(ink))]
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
            item_height(size)
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
    crate::day_cell::view(app, day, width, height, size, quick)
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
    // Reading order: title, start (or source/typed time), the record itself,
    // completion or status, then related commits and the original.
    let timeline = form
        .entry_version
        .as_ref()
        .filter(|_| form.kind == Kind::Entry)
        .map(|v| &v.timeline);
    if let Some(timeline) = timeline {
        content = content.push(reference_view(&form.date, timeline));
    }
    content = content.push(
        text(if form.body.is_empty() {
            "내용이 없습니다."
        } else {
            &form.body
        })
        .size(14),
    );
    if let Some(tail) = timeline.and_then(timeline_tail) {
        content = content.push(tail);
    }
    if !form.source_url.is_empty() {
        content =
            content.push(button("출처 열기").on_press(Message::Source(form.source_url.clone())))
    }
    column![
        scrollable(content).spacing(SCROLL_GAP).height(Length::Fill),
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

/// The part of a record a folded card shows, and whether anything was cut.
fn card_preview(body: &str) -> (String, bool) {
    let body = body.trim();
    let mut end = body.len();
    if let Some((at, _)) = body.match_indices('\n').nth(CARD_LINES - 1) {
        end = at;
    }
    if let Some((at, _)) = body.char_indices().nth(CARD_CHARS) {
        end = end.min(at);
    }
    if end == body.len() {
        (body.into(), false)
    } else {
        (format!("{}…", body[..end].trim_end()), true)
    }
}
/// What a folded card shows. An automatic development record folds to the
/// summary chosen from its original text; unfolding shows the full display
/// copy. Any other record folds to the start of its own text.
pub fn card_fold(entry: &Entry, timeline: &EntryTimeline) -> (String, bool) {
    let summary = entry.source_body.trim();
    if timeline.is_automatic() && !summary.is_empty() {
        let (preview, cut) = card_preview(summary);
        (preview, cut || summary != entry.body.trim())
    } else {
        card_preview(&entry.body)
    }
}
fn commit_line(commit: &times::RecordCommit) -> String {
    format!(
        "커밋 · {} · {} · {}",
        times::kst_minute(commit.committed_at.as_deref()),
        times::short_hash(&commit.hash),
        if commit.subject.is_empty() {
            "제목 미상"
        } else {
            &commit.subject
        }
    )
}
const EDITED_LINE: &str = "표시본 직접 편집됨 · 저장된 원문은 그대로 보존됩니다";
/// What follows a record's own text: completion or current status of an
/// automatic record, then its commits, then whether the display copy was
/// edited. KST to the minute; unknown times say so. The text form of
/// `timeline_tail`, used to check the reading order.
#[cfg(test)]
fn tail_lines(timeline: &EntryTimeline) -> Vec<String> {
    let mut lines: Vec<String> = times::completion_line(timeline).into_iter().collect();
    lines.extend(timeline.commits.iter().map(commit_line));
    if timeline.edited {
        lines.push(EDITED_LINE.into());
    }
    lines
}
/// Every timeline line in reading order: the start (or source or typed time)
/// that follows the title, then what follows the record's text. Nothing is
/// filled in from the moment a record was saved.
#[cfg(test)]
pub fn timeline_lines(date: &str, timeline: &EntryTimeline) -> Vec<String> {
    let mut lines = vec![times::reference_line(date, timeline)];
    lines.extend(tail_lines(timeline));
    lines
}
/// The line under a record's title: when it began or happened.
pub fn reference_view<'a>(date: &str, timeline: &EntryTimeline) -> Element<'a, Message> {
    text(times::reference_line(date, timeline))
        .size(12)
        .style(theme::muted)
        .into()
}
/// Under a record's text: completion or status, each commit with a link when
/// its remote page is known, the edited note and, for an automatic record,
/// the way to its original `record.md`. None when there is nothing to show.
pub fn timeline_tail<'a>(timeline: &EntryTimeline) -> Option<Element<'a, Message>> {
    let mut list = column![].spacing(3);
    let mut any = false;
    if let Some(line) = times::completion_line(timeline) {
        list = list.push(text(line).size(12).style(theme::muted));
        any = true;
    }
    for commit in &timeline.commits {
        let line = text(commit_line(commit)).size(12).style(theme::muted);
        list = list.push(if commit.url.is_empty() {
            Element::from(line)
        } else {
            row![
                line,
                button(text("커밋 열기").size(11))
                    .padding([1, 4])
                    .style(theme::text_button)
                    .on_press(Message::Source(commit.url.clone()))
            ]
            .spacing(6)
            .align_y(iced::Alignment::Center)
            .into()
        });
        any = true;
    }
    if timeline.edited {
        list = list.push(text(EDITED_LINE).size(12).style(theme::muted));
        any = true;
    }
    if let Some(work_id) = &timeline.work_id {
        list = list.push(
            button(text("원본 열기").size(12))
                .padding([2, 6])
                .style(theme::text_button)
                .on_press(Message::OpenOriginal(work_id.clone())),
        );
        any = true;
    }
    any.then(|| list.into())
}
/// One project's records of a day, one card each.
pub fn group_view(app: &App) -> Element<'_, Message> {
    let group = app.calendar.group.as_ref().unwrap();
    let project = app
        .snapshot
        .projects
        .iter()
        .find(|p| p.id == group.project_id)
        .map_or("프로젝트", |p| p.title.as_str());
    let enabled = app.editor.is_none() && !app.busy && !app.loading;
    let unknown = EntryTimeline::default();
    let mut cards = column![].spacing(10);
    for entry in &group.entries {
        let unfolded = group.unfolded.contains(&entry.id);
        let timeline = group.timelines.get(&entry.id).unwrap_or(&unknown);
        let (preview, cut) = card_fold(entry, timeline);
        let body = if entry.body.trim().is_empty() {
            text("내용이 없습니다.").size(13).style(theme::muted)
        } else if unfolded {
            text(entry.body.trim()).size(13)
        } else {
            text(preview).size(13)
        };
        let mut controls = row![].spacing(8).align_y(iced::Alignment::Center);
        if cut {
            controls = controls.push(
                button(text(if unfolded { "접기" } else { "전체 보기" }).size(12))
                    .style(theme::text_button)
                    .on_press(message(Action::ToggleCard(entry.id.clone()))),
            );
        }
        if !entry.source_url.is_empty() {
            controls = controls.push(
                button(text("출처 열기").size(12))
                    .style(theme::text_button)
                    .on_press(Message::Source(entry.source_url.clone())),
            );
        }
        controls = controls.push(space().width(Length::Fill)).push(
            button(text("수정").size(12))
                .on_press_maybe(enabled.then(|| message(Action::EditCard(entry.id.clone())))),
        );
        // Reading order: title, start (or source/typed time), the record,
        // completion or status, then commits and the original.
        let mut card = column![
            text(&entry.title).size(15),
            text(crate::views::kind_label(&entry.kind))
                .size(12)
                .style(theme::muted),
            reference_view(&entry.date, timeline),
            body,
        ]
        .spacing(6);
        if let Some(tail) = timeline_tail(timeline) {
            card = card.push(tail);
        }
        cards = cards.push(
            container(card.push(controls))
                .padding(12)
                .width(Length::Fill)
                .style(theme::surface),
        );
    }
    // The 51st record and later are a step away, never silently hidden.
    let read = group.entries.len() as u32;
    if read < group.total {
        cards = cards.push(if group.loading_more {
            Element::from(text("기록 불러오는 중…").size(12).style(theme::muted))
        } else {
            button(text(format!("기록 더 보기 · {read}/{}개", group.total)).size(13))
                .padding([7, 10])
                .style(theme::secondary)
                .on_press(message(Action::GroupMore))
                .into()
        });
    }
    column![
        text(project).size(21),
        text(format!("{} · 기록 {}개", group.date, group.total.max(read)))
            .size(12)
            .style(theme::muted),
        scrollable(cards).spacing(SCROLL_GAP).height(Length::Fill),
        row![
            space().width(Length::Fill),
            button("닫기").on_press(message(Action::CloseGroup))
        ]
    ]
    .spacing(12)
    .height(Length::Fill)
    .into()
}
