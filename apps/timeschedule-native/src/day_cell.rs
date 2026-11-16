//! Calendar day cells for the desktop widget and the diary month view.
//!
//! Each item gets one line at the chosen size, with no checkbox, so a cell
//! shows as many titles as its height allows. Items that do not fit are named
//! on the cell's last line instead of being counted.
use crate::{
    app::{App, Message},
    calendar_ui::{self, Action},
    text_layout, theme,
    widget_appearance::Appearance,
};
use iced::widget::{button, column, container, row, space, text, Column, Row};
use iced::{font::Weight, Element, Font, Length};
use timeschedule_core::calendar::{CalendarDay, CalendarItem, CalendarSource};

const BAR: f32 = 3.0;
const BAR_GAP: f32 = 4.0;
const PREFIX_GAP: f32 = 4.0;
const END_GAP: f32 = 2.0;
/// A project name never takes more than this share of a line.
const PROJECT_SHARE: f32 = 0.4;
/// The diary has no line gap setting; a hairline keeps tinted rows apart.
const DIARY_GAP: f32 = 1.0;
const TINT: f32 = 0.16;
const SEPARATOR: &str = " · ";

struct Line<'a> {
    item: &'a CalendarItem,
    prefix: Option<String>,
    title: String,
    title_width: f32,
    first: String,
    second: Option<String>,
}

struct Style {
    colors: theme::Colors,
    font: Font,
    bold: Font,
    size: f32,
    line: f32,
}

pub fn view<'a>(
    app: &'a App,
    day: Option<&'a CalendarDay>,
    width: f32,
    height: f32,
    size: f32,
    widget: bool,
) -> Element<'a, Message> {
    let Some(day) = day else {
        return space().into();
    };
    let (colors, font) = if widget {
        let a = Appearance::resolve(&app.preferences);
        (a.colors, a.font)
    } else {
        (
            theme::palette(app.preferences.theme),
            Font::with_name("Malgun Gothic"),
        )
    };
    let style = Style {
        colors,
        font,
        bold: Font {
            weight: Weight::Bold,
            ..font
        },
        size,
        line: size * 1.3,
    };
    let gap = if widget {
        f32::from(app.preferences.widget.line_gap)
    } else {
        DIARY_GAP
    };
    // `lines` text lines in `blocks` stacked blocks, gaps only between blocks.
    let fits = |lines: usize, blocks: usize| {
        lines as f32 * style.line + blocks.saturating_sub(1) as f32 * gap <= height + 0.01
    };
    let known = day.items.len();
    let pending = (day.total.saturating_sub(day.completed) as usize).max(known);
    let mut lines = Vec::new();
    let mut last = None;
    if pending == known && fits(known, known) {
        lines = day
            .items
            .iter()
            .map(|item| line(item, width, &style))
            .collect::<Vec<_>>();
        // The completed count is kept before spare room goes to cut titles,
        // top first.
        let done = day.completed > 0 && fits(known + 1, known + 1);
        let blocks = known + usize::from(done);
        let mut used = blocks;
        for l in &mut lines {
            if l.first != l.title && fits(used + 1, blocks) && wrap(l, width, &style) {
                used += 1;
            }
        }
        if done {
            last = Some(text_layout::fit_label_with_font(
                &format!("완료 {}", day.completed),
                width,
                size,
                font,
            ));
        }
    } else if let Some(shown) = (0..=known.min(pending - 1))
        .rev()
        .find(|&k| fits(k + 1, k + 1))
    {
        lines = day.items[..shown]
            .iter()
            .map(|item| line(item, width, &style))
            .collect();
        let titles: Vec<&str> = day.items[shown..].iter().map(display_title).collect();
        last = Some(summary(
            &titles,
            (pending - known) as u32,
            day.completed,
            width,
            size,
            font,
        ));
    }
    let enabled = app.editor.is_none() && !app.busy && !app.loading;
    let mut cell = Column::new().width(Length::Fill).height(height);
    for (index, l) in lines.into_iter().enumerate() {
        if index > 0 {
            cell = cell.push(space().height(gap));
        }
        cell = cell.push(line_view(app, l, &style, widget, enabled));
    }
    if let (Some(last), Ok(date)) = (last, day.date.parse()) {
        cell = cell.push(space().height(Length::Fill)).push(
            button(
                text(last)
                    .size(size)
                    .font(font)
                    .color(style.colors.muted)
                    .line_height(text::LineHeight::Relative(1.3))
                    .wrapping(text::Wrapping::None),
            )
            .padding(0)
            .height(style.line)
            .style(theme::text_button)
            .on_press(if widget {
                Message::WidgetDate(date)
            } else {
                Message::ManagerDate(date)
            }),
        );
    }
    cell.into()
}

fn display_title(item: &CalendarItem) -> &str {
    if item.short_title.is_empty() {
        &item.title
    } else {
        &item.short_title
    }
}

/// Time for a timed schedule item, project for a record; none otherwise.
fn prefix(item: &CalendarItem) -> Option<(&str, f32)> {
    if item.source == CalendarSource::Entry {
        [&item.project_short_name, &item.project_title]
            .into_iter()
            .find(|s| !s.is_empty())
            .map(|s| (s.as_str(), PROJECT_SHARE))
    } else {
        item.start_time
            .as_deref()
            .filter(|_| !item.all_day)
            .map(|t| (t, 1.0))
    }
}

fn line<'a>(item: &'a CalendarItem, width: f32, style: &Style) -> Line<'a> {
    let available = (width - BAR - BAR_GAP - END_GAP).max(0.0);
    let prefix = prefix(item)
        .map(|(p, share)| {
            text_layout::fit_label_with_font(p, available * share, style.size, style.font)
        })
        .filter(|p| !p.is_empty());
    let title_width = (available
        - prefix.as_ref().map_or(0.0, |p| {
            text_layout::text_width_with_font(p, style.size, style.font) + PREFIX_GAP
        }))
    .max(0.0);
    let title = display_title(item).replace(['\n', '\r', '\t'], " ");
    let first = text_layout::fit_label_with_font(&title, title_width, style.size, style.bold);
    Line {
        item,
        prefix,
        title,
        title_width,
        first,
        second: None,
    }
}

/// Give a cut title a second line. False when nothing fits on the first.
fn wrap(line: &mut Line<'_>, width: f32, style: &Style) -> bool {
    let (first, rest) =
        text_layout::split_first_line(&line.title, line.title_width, style.size, style.bold);
    if first.is_empty() || rest.is_empty() {
        return false;
    }
    let available = (width - BAR - BAR_GAP - END_GAP).max(0.0);
    line.first = first;
    line.second = Some(text_layout::fit_label_with_font(
        &rest, available, style.size, style.bold,
    ));
    true
}

fn line_view<'a>(
    app: &App,
    l: Line<'a>,
    style: &Style,
    widget: bool,
    enabled: bool,
) -> Element<'a, Message> {
    let ink = calendar_ui::color(app, &l.item.category_id);
    let label = |content: String, font, color| {
        text(content)
            .size(style.size)
            .font(font)
            .color(color)
            .line_height(text::LineHeight::Relative(1.3))
            .wrapping(text::Wrapping::None)
    };
    let mut first = Row::new().spacing(PREFIX_GAP);
    if let Some(p) = l.prefix {
        let color = if l.item.source == CalendarSource::Entry {
            ink
        } else {
            style.colors.muted
        };
        first = first.push(label(p, style.font, color));
    }
    first = first.push(label(l.first, style.bold, style.colors.text));
    let mut body = column![first];
    let height = style.line * if l.second.is_some() { 2.0 } else { 1.0 };
    if let Some(second) = l.second {
        body = body.push(label(second, style.bold, style.colors.text));
    }
    let content = row![
        container(space().width(BAR).height(Length::Fill))
            .style(move |_| container::Style::default().background(ink)),
        body
    ]
    .spacing(BAR_GAP)
    .height(height);
    let open = if enabled {
        calendar_ui::message(Action::Open(l.item.source, l.item.id.clone(), widget))
    } else {
        Message::Ignore
    };
    button(
        container(content)
            .width(Length::Fill)
            .height(height)
            .style(move |_| container::Style::default().background(ink.scale_alpha(TINT))),
    )
    .padding(0)
    .width(Length::Fill)
    .height(height)
    .style(theme::text_button)
    .on_press(open)
    .into()
}

fn compose(names: &str, rest: u32, completed: u32) -> String {
    let mut s = names.to_owned();
    if rest > 0 {
        if !s.is_empty() {
            s.push(' ');
        }
        s.push_str(&format!("+{rest}"));
    }
    if completed > 0 {
        if !s.is_empty() {
            s.push_str(SEPARATOR);
        }
        s.push_str(&format!("완료 {completed}"));
    }
    s
}

/// The last line of a full cell: the hidden titles that fit, then how many
/// are left (`beyond` counts items that were never loaded) and how many are
/// done. `장보기 · CI 정리`, `장보기 +2`, `+3 · 완료 1`.
pub fn summary(
    titles: &[&str],
    beyond: u32,
    completed: u32,
    width: f32,
    size: f32,
    font: Font,
) -> String {
    let measure = |s: &str| text_layout::text_width_with_font(s, size, font);
    let titles: Vec<String> = titles
        .iter()
        .map(|t| t.replace(['\n', '\r', '\t'], " "))
        .collect();
    let hidden = titles.len() as u32 + beyond;
    // The count suffix shrinks as names are added, so a longer candidate can
    // fit where a shorter one did not. Only the names themselves only grow.
    let mut best = None;
    for n in 1..=titles.len() {
        let names = titles[..n].join(SEPARATOR);
        let s = compose(&names, hidden - n as u32, completed);
        if measure(&s) <= width {
            best = Some(s);
        } else if measure(&names) > width {
            break;
        }
    }
    if let Some(s) = best {
        return s;
    }
    if let Some(first) = titles.first() {
        // The tail exactly as it follows a name: " +2" or " · 완료 1".
        let tail = compose("\0", hidden - 1, completed)[1..].to_owned();
        let room = width - measure(&tail);
        let fitted = text_layout::fit_label_with_font(first, room.max(0.0), size, font);
        if fitted.chars().count() >= 3 {
            return fitted + &tail;
        }
    }
    for s in [compose("", hidden, completed), compose("", hidden, 0)] {
        if !s.is_empty() && measure(&s) <= width {
            return s;
        }
    }
    text_layout::fit_label_with_font(&compose("", hidden, completed), width, size, font)
}

#[cfg(test)]
mod tests {
    use super::summary;
    use crate::text_layout::text_width_with_font as measure;
    use iced::Font;

    #[test]
    fn summary_names_hidden_items_while_they_fit() {
        let f = Font::with_name("Malgun Gothic");
        let titles = ["장보기", "CI 속도 정리"];
        assert_eq!(
            summary(&titles, 0, 0, 1000., 12., f),
            "장보기 · CI 속도 정리"
        );
        assert_eq!(
            summary(&titles, 0, 2, 1000., 12., f),
            "장보기 · CI 속도 정리 · 완료 2"
        );
        let narrow = measure("장보기 +1", 12., f) + 0.5;
        assert_eq!(summary(&titles, 0, 0, narrow, 12., f), "장보기 +1");
        let tiny = measure("+2", 12., f) + 0.5;
        assert_eq!(summary(&titles, 0, 0, tiny, 12., f), "+2");
        assert_eq!(summary(&titles, 0, 0, 0., 12., f), "");
        assert_eq!(summary(&["한 줄\n다음"], 0, 0, 1000., 12., f), "한 줄 다음");
        assert_eq!(summary(&[], 0, 3, 1000., 12., f), "완료 3");
    }

    #[test]
    fn summary_counts_items_beyond_loaded() {
        let f = Font::with_name("Malgun Gothic");
        assert_eq!(summary(&["장보기"], 3, 0, 1000., 12., f), "장보기 +3");
        assert_eq!(
            summary(&["장보기"], 3, 1, 1000., 12., f),
            "장보기 +3 · 완료 1"
        );
    }

    #[test]
    fn summary_tries_every_prefix_because_widths_are_not_monotonic() {
        let f = Font::with_name("Malgun Gothic");
        let width = measure("A · i", 12., f) + 0.1;
        assert!(measure("A +1", 12., f) > width, "font precondition");
        assert_eq!(summary(&["A", "i"], 0, 0, width, 12., f), "A · i");
    }

    #[test]
    fn summary_shortens_a_long_first_title_before_dropping_it() {
        let f = Font::with_name("Malgun Gothic");
        let long = "아주 긴 회의 제목입니다 정말 길어요";
        let width = measure("아주 긴 회… +1", 12., f) + 0.5;
        let s = summary(&[long, "B"], 0, 0, width, 12., f);
        assert!(s.starts_with("아주") && s.ends_with("… +1"), "{s}");
        assert!(measure(&s, 12., f) <= width + 0.1);
    }
}
