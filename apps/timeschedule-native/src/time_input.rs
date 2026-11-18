//! Free-form 24-hour inputs with quarter-hour suggestions.
use crate::{
    app::{Field as FormField, Message},
    theme,
};
use chrono::Timelike;
use iced::advanced::{
    layout, mouse, overlay, renderer,
    text::Renderer as TextRenderer,
    widget::{operation, tree, Id, Operation, Tree},
    Clipboard, Layout, Shell, Widget,
};
use iced::widget::{button, column, row, scrollable, space, text, text_input, Column};
use iced::{
    touch, Alignment, Element, Event, Length, Point, Rectangle, Renderer, Size, Theme, Vector,
};
use timeschedule_core::storage::validate_time;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Field {
    Start,
    End,
}

pub fn view<'a>(start: &'a str, end: &'a str, open: Option<Field>) -> Element<'a, Message> {
    let start_minutes = minutes(start);
    row![
        input(Field::Start, start, start_minutes, open),
        input(Field::End, end, start_minutes, open)
    ]
    .spacing(10)
    .into()
}

const QUARTER_HOUR: u16 = 15;
const DAY_MINUTES: u16 = 24 * 60;
const ROW_HEIGHT: f32 = 30.0;
const MENU_HEIGHT: f32 = 6.0 * ROW_HEIGHT;
const MENU_ID: &str = "editor-time-suggestions";

fn minutes(value: &str) -> Option<u16> {
    let time = validate_time(value).ok()?;
    Some((time.hour() * 60 + time.minute()) as u16)
}

fn input<'a>(
    field: Field,
    value: &'a str,
    start: Option<u16>,
    open: Option<Field>,
) -> Element<'a, Message> {
    let (label, id, change): (_, _, fn(String) -> FormField) = match field {
        Field::Start => ("시작 시간", "editor-start", FormField::Start),
        Field::End => ("종료 시간", "editor-end", FormField::End),
    };
    let current = minutes(value);
    let entry = text_input("HH:MM", value)
        .id(id)
        .padding(8)
        .size(14)
        .icon(text_input::Icon {
            font: Renderer::ICON_FONT,
            code_point: Renderer::ARROW_DOWN_ICON,
            size: Some(10.into()),
            spacing: 6.0,
            side: text_input::Side::Right,
        })
        .on_input(move |value| Message::Field(change(value)))
        .on_submit_maybe((open == Some(field)).then_some(Message::DismissTimePicker(field)));
    let picker = TimeInput {
        field,
        entry: entry.into(),
        menu: (open == Some(field)).then(|| suggestions(field, start, current)),
        quarter: selected_quarter(field, start, current),
        current_is_valid: current.is_some(),
    };
    column![
        text(label).size(12).style(theme::muted),
        Element::new(picker)
    ]
    .width(Length::Fill)
    .spacing(5)
    .into()
}

fn selected_quarter(field: Field, start: Option<u16>, value: Option<u16>) -> u16 {
    match (field, start) {
        (Field::End, Some(start)) if value.is_none_or(|end| end <= start) => {
            (start / QUARTER_HOUR + 1).min(DAY_MINUTES / QUARTER_HOUR - 1)
        }
        _ => value.unwrap_or(9 * 60) / QUARTER_HOUR,
    }
}

fn elapsed(start: u16, candidate: u16) -> Option<String> {
    let duration = candidate.checked_sub(start)?;
    match (duration / 60, duration % 60) {
        (0, 0) => None,
        (0, minutes) => Some(format!("{minutes}분")),
        (hours, 0) => Some(format!("{hours}시간")),
        (hours, minutes) => Some(format!("{hours}시간 {minutes}분")),
    }
}

fn suggestions(
    field: Field,
    start: Option<u16>,
    current: Option<u16>,
) -> Element<'static, Message> {
    let mut options = Column::new().padding([0, 3]);
    for candidate in (0..DAY_MINUTES).step_by(usize::from(QUARTER_HOUR)) {
        let time = format!("{:02}:{:02}", candidate / 60, candidate % 60);
        let mut label = row![text(time.clone()).size(14)].align_y(Alignment::Center);
        let mut enabled = true;
        if let (Field::End, Some(start)) = (field, start) {
            if let Some(duration) = elapsed(start, candidate) {
                label = label
                    .push(space().width(Length::Fill))
                    .push(text(duration).size(12).style(theme::muted));
            } else {
                enabled = false;
            }
        }
        let option = button(label)
            .padding([3, 6])
            .height(ROW_HEIGHT)
            .width(Length::Fill)
            .style(if current == Some(candidate) && enabled {
                theme::navigation
            } else {
                theme::text_button
            })
            .on_press_maybe(enabled.then_some(Message::PickEditorTime(field, time)));
        options = options.push(option);
    }
    scrollable(options)
        .id(MENU_ID)
        .width(Length::Fill)
        .height(Length::Fill)
        .spacing(2)
        .style(|theme, status| scrollable::Style {
            container: theme::surface(theme),
            ..scrollable::default(theme, status)
        })
        .into()
}

struct TimeInput<'a> {
    field: Field,
    entry: Element<'a, Message>,
    menu: Option<Element<'static, Message>>,
    quarter: u16,
    current_is_valid: bool,
}

#[derive(Default)]
struct State {
    focused: bool,
    scrolled_to: Option<u16>,
}

impl Widget<Message, Theme, Renderer> for TimeInput<'_> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }
    fn state(&self) -> tree::State {
        tree::State::new(State::default())
    }
    fn children(&self) -> Vec<Tree> {
        std::iter::once(&self.entry)
            .chain(self.menu.iter())
            .map(Tree::new)
            .collect()
    }
    fn diff(&self, tree: &mut Tree) {
        if let Some(menu) = &self.menu {
            tree.diff_children(&[&self.entry, menu]);
        } else {
            tree.diff_children(&[&self.entry]);
            tree.state.downcast_mut::<State>().scrolled_to = None;
        }
    }
    fn size(&self) -> Size<Length> {
        self.entry.as_widget().size()
    }
    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        self.entry
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, limits)
    }
    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        self.entry.as_widget().draw(
            &tree.children[0],
            renderer,
            theme,
            style,
            layout,
            cursor,
            viewport,
        );
    }
    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        op: &mut dyn Operation,
    ) {
        self.entry
            .as_widget_mut()
            .operate(&mut tree.children[0], layout, renderer, op);
    }
    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        self.entry.as_widget().mouse_interaction(
            &tree.children[0],
            layout,
            cursor,
            viewport,
            renderer,
        )
    }
    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        type NativeInput = text_input::State<<Renderer as TextRenderer>::Paragraph>;
        let child = &mut tree.children[0];
        let was_focused = child.state.downcast_ref::<NativeInput>().is_focused();
        self.entry.as_widget_mut().update(
            child, event, layout, cursor, renderer, clipboard, shell, viewport,
        );
        let native = child.state.downcast_mut::<NativeInput>();
        let focused = native.is_focused();
        let state = tree.state.downcast_mut::<State>();
        let pressed = matches!(
            event,
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
                | Event::Touch(touch::Event::FingerPressed { .. })
        );
        if pressed && focused && !was_focused {
            native.select_all();
        }
        if self.menu.is_none()
            && focused
            && (!state.focused || (pressed && cursor.is_over(layout.bounds())))
        {
            shell.publish(Message::OpenTimePicker(self.field));
        } else if self.menu.is_some()
            && ((!focused && (state.focused || pressed))
                || matches!(event, Event::Window(iced::window::Event::Unfocused)))
        {
            shell.publish(Message::DismissTimePicker(self.field));
        }
        state.focused = focused;
    }
    fn overlay<'a>(
        &'a mut self,
        tree: &'a mut Tree,
        layout: Layout<'a>,
        _renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'a, Message, Theme, Renderer>> {
        let menu = self.menu.as_mut()?;
        let anchor = layout.bounds() + translation;
        anchor.intersection(viewport)?;
        let state = tree.state.downcast_mut::<State>();
        // Keep a partially typed time from jumping the list back to the default.
        let quarter = if self.current_is_valid {
            self.quarter
        } else {
            state.scrolled_to.unwrap_or(self.quarter)
        };
        Some(overlay::Element::new(Box::new(TimeMenu {
            menu,
            tree: &mut tree.children[1],
            anchor,
            quarter,
            scrolled_to: &mut state.scrolled_to,
        })))
    }
}

struct TimeMenu<'a> {
    menu: &'a mut Element<'static, Message>,
    tree: &'a mut Tree,
    anchor: Rectangle,
    quarter: u16,
    scrolled_to: &'a mut Option<u16>,
}

impl overlay::Overlay<Message, Theme, Renderer> for TimeMenu<'_> {
    fn layout(&mut self, renderer: &Renderer, window: Size) -> layout::Node {
        let below = (window.height - self.anchor.y - self.anchor.height - 3.0).max(0.0);
        let above = (self.anchor.y - 3.0).max(0.0);
        let opens_below = below >= MENU_HEIGHT || below >= above;
        let size = Size::new(
            self.anchor.width.clamp(180.0, 260.0).min(window.width),
            MENU_HEIGHT.min(if opens_below { below } else { above }),
        );
        let x = self.anchor.x.clamp(0.0, window.width - size.width);
        let y = if opens_below {
            self.anchor.y + self.anchor.height + 3.0
        } else {
            self.anchor.y - size.height - 3.0
        };
        let node = self
            .menu
            .as_widget_mut()
            .layout(self.tree, renderer, &layout::Limits::new(size, size))
            .move_to(Point::new(x, y));
        if *self.scrolled_to != Some(self.quarter) {
            let offset = f32::from(self.quarter.saturating_sub(1)) * ROW_HEIGHT;
            let mut scroll = operation::scrollable::scroll_to::<()>(
                Id::new(MENU_ID),
                scrollable::AbsoluteOffset {
                    x: None,
                    y: Some(offset),
                },
            );
            self.menu
                .as_widget_mut()
                .operate(self.tree, Layout::new(&node), renderer, &mut scroll);
            *self.scrolled_to = Some(self.quarter);
        }
        node
    }
    fn draw(
        &self,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
    ) {
        self.menu.as_widget().draw(
            self.tree,
            renderer,
            theme,
            style,
            layout,
            cursor,
            &layout.bounds(),
        );
    }
    fn operate(&mut self, layout: Layout<'_>, renderer: &Renderer, operation: &mut dyn Operation) {
        self.menu
            .as_widget_mut()
            .operate(self.tree, layout, renderer, operation);
    }
    fn update(
        &mut self,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
    ) {
        self.menu.as_widget_mut().update(
            self.tree,
            event,
            layout,
            cursor,
            renderer,
            clipboard,
            shell,
            &layout.bounds(),
        );
        // A press on a disabled row or the scrollbar still belongs to the popup.
        if cursor.is_over(layout.bounds()) && matches!(event, Event::Mouse(_) | Event::Touch(_)) {
            shell.capture_event();
        }
    }
    fn mouse_interaction(
        &self,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        if cursor.is_over(layout.bounds()) {
            self.menu
                .as_widget()
                .mouse_interaction(self.tree, layout, cursor, &layout.bounds(), renderer)
                .max(mouse::Interaction::Idle)
        } else {
            mouse::Interaction::None
        }
    }
}

#[cfg(test)]
mod tests;
