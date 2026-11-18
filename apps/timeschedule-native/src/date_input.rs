//! A native text input with replaceable year, month and day selections.
use crate::{
    app::{Field, Message},
    date_choices::{with_year_month, Part},
};
use chrono::{Datelike, NaiveDate};
use iced::advanced::{
    clipboard, input_method, layout, mouse, renderer, text,
    text::{Paragraph, Renderer as _},
    widget::{tree, Operation, Tree},
    Clipboard, Layout, Shell, Widget,
};
use iced::{
    keyboard::{self, key::Named, Key},
    touch,
    widget::{text_input, TextInput},
    window, Element, Event, Length, Point, Rectangle, Renderer, Size, Theme,
};
use std::{ops::Range, sync::LazyLock};

const TEMPLATE: &str = "YYYY-MM-DD";
static PLACEHOLDER: LazyLock<text_input::Value> =
    LazyLock::new(|| text_input::Value::new(TEMPLATE));
const FONT_SIZE: f32 = 14.0;
type NativeInput = text_input::State<<Renderer as text::Renderer>::Paragraph>;

pub fn input(value: &str, fallback: NaiveDate) -> Element<'_, Message> {
    Element::new(DateInput {
        value,
        fallback,
        field: field(value),
    })
}

fn field(value: &str) -> TextInput<'static, String> {
    let field = text_input(TEMPLATE, value)
        .id("editor-date")
        .size(FONT_SIZE)
        .padding(8)
        .on_input(|value| value);
    if value.is_empty() {
        field.style(|theme, status| {
            let style = text_input::default(theme, status);
            text_input::Style {
                value: style.placeholder,
                ..style
            }
        })
    } else {
        field
    }
}

struct DateInput<'a> {
    value: &'a str,
    fallback: NaiveDate,
    field: TextInput<'static, String>,
}

struct Editing {
    // Iced can deliver multiple changes before rebuilding the view.
    value: String,
    part: Option<Part>,
    digits: Option<Digits>,
}

struct Digits {
    // Use the date at the start of the edit, including its original day.
    // Typing 2028 over leap year 2024 passes through non-leap year 0002.
    base: NaiveDate,
    number: u32,
    count: usize,
}

fn canonical(value: &str) -> Option<NaiveDate> {
    let date: NaiveDate = value.parse().ok()?;
    ((1..=9999).contains(&date.year()) && value == date.to_string()).then_some(date)
}

fn range(part: Part) -> Range<usize> {
    match part {
        Part::Year => 0..4,
        Part::Month => 5..7,
        Part::Day => 8..10,
    }
}

fn neighbor(part: Part, forward: bool) -> Part {
    match (part, forward) {
        (Part::Year, true) | (Part::Day, false) => Part::Month,
        (Part::Month, true) => Part::Day,
        (Part::Month, false) => Part::Year,
        _ => part,
    }
}

fn replace_number(date: NaiveDate, part: Part, number: u32) -> Option<NaiveDate> {
    match part {
        Part::Year => with_year_month(date, number.try_into().ok()?, date.month()),
        Part::Month => with_year_month(date, date.year(), number),
        Part::Day => date.with_day(number),
    }
}

impl Editing {
    fn select(&mut self, part: Option<Part>, native: &mut NativeInput) {
        self.part = part;
        self.digits = None;
        if let Some(part) = part {
            let range = range(part);
            native.select_range(range.start, range.end);
        }
    }

    fn type_digits(&mut self, part: Part, digits: &str, base: NaiveDate) -> Option<NaiveDate> {
        let entry = self.digits.get_or_insert(Digits {
            base,
            number: 0,
            count: 0,
        });
        for digit in digits.bytes() {
            entry.number = entry.number * 10 + u32::from(digit - b'0');
            entry.count += 1;
        }
        let result = replace_number(entry.base, part, entry.number);
        if entry.count >= range(part).len() {
            self.digits = None;
        }
        result
    }
}

// Hit-test the same shaped text as the native field. This also makes the
// month and day of the placeholder clickable when the optional date is empty.
fn shaped(
    value: &str,
    height: f32,
    renderer: &Renderer,
) -> <Renderer as text::Renderer>::Paragraph {
    <Renderer as text::Renderer>::Paragraph::with_text(text::Text {
        content: if value.is_empty() { TEMPLATE } else { value },
        size: FONT_SIZE.into(),
        bounds: Size::new(f32::INFINITY, height),
        font: renderer.default_font(),
        line_height: Default::default(),
        align_x: Default::default(),
        align_y: iced::alignment::Vertical::Center,
        shaping: text::Shaping::Advanced,
        wrapping: Default::default(),
    })
}

fn hit_part(value: &str, position: Point, bounds: Rectangle, renderer: &Renderer) -> Part {
    let paragraph = shaped(value, bounds.height, renderer);
    let offset = (position.x - bounds.x).max(0.0);
    match paragraph
        .hit_test(Point::new(offset, bounds.height / 2.0))
        .map(text::Hit::cursor)
        .unwrap_or(10)
    {
        0..=4 => Part::Year,
        5..=7 => Part::Month,
        _ => Part::Day,
    }
}

enum KeyAction<'a> {
    Move(Part),
    Step(i32),
    Type(&'a str),
    SelectAll,
    Paste,
    Open,
    Finish,
}

fn segment_action(event: &Event, part: Part) -> Option<KeyAction<'_>> {
    use KeyAction::*;
    if let Event::InputMethod(input_method::Event::Commit(text)) = event {
        return Some(Type(text));
    }
    let Event::Keyboard(keyboard::Event::KeyPressed {
        key,
        physical_key,
        modifiers,
        text,
        ..
    }) = event
    else {
        return None;
    };

    if modifiers.command() {
        return match key.to_latin(*physical_key) {
            Some('a') => Some(SelectAll),
            Some('v') if !modifiers.alt() => Some(Paste),
            _ => None,
        };
    }
    if modifiers.alt() {
        return (key.as_ref() == Key::Named(Named::ArrowDown)).then_some(Open);
    }
    if modifiers.shift() && matches!(key, Key::Named(_)) {
        return None;
    }
    match key.as_ref() {
        Key::Named(Named::ArrowLeft) => Some(Move(neighbor(part, false))),
        Key::Named(Named::ArrowRight) | Key::Character("-" | "/") => {
            Some(Move(neighbor(part, true)))
        }
        Key::Named(Named::Home) => Some(Move(Part::Year)),
        Key::Named(Named::End) => Some(Move(Part::Day)),
        Key::Named(Named::ArrowUp) => Some(Step(1)),
        Key::Named(Named::ArrowDown) => Some(Step(-1)),
        Key::Named(Named::Enter) => Some(Finish),
        Key::Named(Named::Space) | Key::Character(" ") => Some(Open),
        _ => text
            .as_deref()
            .filter(|s| !s.chars().any(char::is_control))
            .map(Type),
    }
}

fn is_number(text: &str, part: Part) -> bool {
    !text.is_empty() && text.len() <= range(part).len() && text.bytes().all(|b| b.is_ascii_digit())
}

impl DateInput<'_> {
    fn write(&mut self, state: &mut Editing, value: String, shell: &mut Shell<'_, Message>) {
        self.field = field(&value);
        state.value.clone_from(&value);
        shell.publish(Message::Field(Field::Date(value)));
        shell.invalidate_layout();
        shell.request_redraw();
    }

    fn number(
        &mut self,
        state: &mut Editing,
        part: Part,
        text: &str,
        shell: &mut Shell<'_, Message>,
    ) {
        if text.len() > 1 {
            // Pasted or IME-committed digits replace the entire selected part.
            state.digits = None;
        }
        let base = canonical(&state.value).unwrap_or(self.fallback);
        if let Some(date) = state.type_digits(part, text, base) {
            self.write(state, date.to_string(), shell);
        } else {
            // A leading zero can start 04 or 0001 without committing day/year zero.
            shell.publish(Message::DismissEditorPicker);
        }
    }

    fn segment_key(
        &mut self,
        state: &mut Editing,
        native: &mut NativeInput,
        event: &Event,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
    ) -> bool {
        let Some(part) = state.part else { return false };
        let Some(action) = segment_action(event, part) else {
            return false;
        };
        match action {
            KeyAction::Type(text) => {
                if is_number(text, part) {
                    self.number(state, part, text, shell);
                }
            }
            KeyAction::Paste => {
                if let Some(value) = clipboard.read(clipboard::Kind::Standard) {
                    let value = value.trim();
                    if is_number(value, part) {
                        state.digits = None;
                        self.number(state, part, value, shell);
                    } else {
                        state.select(None, native);
                        native.move_cursor_to_end();
                        self.write(state, value.to_owned(), shell);
                    }
                }
            }
            KeyAction::Step(delta) => {
                state.digits = None;
                let date = canonical(&state.value).unwrap_or(self.fallback);
                let current = match part {
                    Part::Year => date.year() as u32,
                    Part::Month => date.month(),
                    Part::Day => date.day(),
                };
                if let Some(date) = current
                    .checked_add_signed(delta)
                    .and_then(|v| replace_number(date, part, v))
                {
                    self.write(state, date.to_string(), shell);
                } else {
                    shell.publish(Message::DismissEditorPicker);
                }
            }
            KeyAction::Move(next) => {
                state.select(Some(next), native);
                shell.publish(Message::DismissEditorPicker);
            }
            KeyAction::SelectAll => {
                state.select(None, native);
                native.select_all();
                shell.publish(Message::DismissEditorPicker);
            }
            KeyAction::Open => {
                state.digits = None;
                shell.publish(Message::DatePicker(part));
            }
            KeyAction::Finish => {
                state.digits = None;
                shell.publish(Message::DismissEditorPicker);
            }
        }
        true
    }
}

impl Widget<Message, Theme, Renderer> for DateInput<'_> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<Editing>()
    }
    fn state(&self) -> tree::State {
        tree::State::new(Editing {
            value: self.value.into(),
            part: None,
            digits: None,
        })
    }
    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(
            &self.field as &dyn Widget<String, Theme, Renderer>,
        )]
    }
    fn diff(&self, tree: &mut Tree) {
        let state = tree.state.downcast_mut::<Editing>();
        if self.value != state.value {
            // A date picked with the mouse or restored by the editor starts a fresh edit.
            self.value.clone_into(&mut state.value);
            state.digits = None;
        }
        self.field.diff(&mut tree.children[0]);
    }
    fn size(&self) -> Size<Length> {
        Widget::size(&self.field)
    }
    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        // Lay out the mask for highlighting even before the optional date has a value.
        // The native editing buffer stays empty, so typing and copying use the real draft.
        let blank = tree.state.downcast_ref::<Editing>().value.is_empty();
        self.field.layout(
            &mut tree.children[0],
            renderer,
            limits,
            blank.then_some(&*PLACEHOLDER),
        )
    }
    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_ref::<Editing>();
        let show_mask = state.value.is_empty() && state.part.is_some();
        self.field.draw(
            &tree.children[0],
            renderer,
            theme,
            layout,
            cursor,
            show_mask.then_some(&*PLACEHOLDER),
            viewport,
        );
    }
    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        self.field
            .mouse_interaction(&tree.children[0], layout, cursor, viewport, renderer)
    }
    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn Operation,
    ) {
        let child = &mut tree.children[0];
        let native = child.state.downcast_ref::<NativeInput>();
        let (focused, cursor) = (native.is_focused(), native.cursor());
        self.field.operate(child, layout, renderer, operation);
        let native = child.state.downcast_mut::<NativeInput>();
        let state = tree.state.downcast_mut::<Editing>();
        if !focused
            && native.is_focused()
            && (state.value.is_empty() || canonical(&state.value).is_some())
        {
            state.select(Some(Part::Year), native);
        } else if !native.is_focused() || native.cursor() != cursor {
            state.select(None, native);
        }
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
        let state = tree.state.downcast_mut::<Editing>();
        let child = &mut tree.children[0];
        let native = child.state.downcast_mut::<NativeInput>();
        let pressed = match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => cursor.position(),
            Event::Touch(touch::Event::FingerPressed { position, .. }) => Some(*position),
            _ => None,
        };
        if let Some(point) =
            pressed.filter(|p| layout.bounds().contains(*p) && viewport.contains(*p))
        {
            if state.value.is_empty() || canonical(&state.value).is_some() {
                let bounds = layout.children().next().unwrap().bounds();
                let part = hit_part(&state.value, point, bounds, renderer);
                native.focus();
                state.select(Some(part), native);
                shell.publish(Message::DatePicker(part));
                shell.capture_event();
                shell.request_redraw();
                return;
            }
        }
        let focused = native.is_focused();
        if focused && self.segment_key(state, native, event, clipboard, shell) {
            shell.capture_event();
            shell.request_redraw();
            return;
        }
        if focused
            && matches!(
                event,
                Event::Keyboard(keyboard::Event::KeyPressed {
                    key: Key::Named(Named::Escape),
                    ..
                })
            )
        {
            shell.publish(Message::DismissEditorPicker);
        }
        if matches!(event, Event::Window(window::Event::Unfocused)) {
            state.digits = None;
        }
        let before = native.cursor();
        let mut messages = Vec::new();
        let mut inner = Shell::new(&mut messages);
        self.field.update(
            child, event, layout, cursor, renderer, clipboard, &mut inner, viewport,
        );
        let last = std::cell::Cell::new(None);
        shell.merge(inner, |value| {
            last.set(Some(value.clone()));
            Message::Field(Field::Date(value))
        });
        if let Some(value) = last.into_inner() {
            state.value = value;
            state.part = None;
            state.digits = None;
        }
        let native = child.state.downcast_mut::<NativeInput>();
        if !native.is_focused() || native.cursor() != before {
            state.select(None, native);
        }
    }
}

#[cfg(test)]
mod tests;
