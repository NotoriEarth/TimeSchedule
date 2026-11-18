use super::*;
use iced::{
    advanced::widget::operation,
    keyboard::{key, Modifiers},
};

#[derive(Default)]
struct TestClipboard(String);
impl Clipboard for TestClipboard {
    fn read(&self, _: clipboard::Kind) -> Option<String> {
        Some(self.0.clone())
    }
    fn write(&mut self, _: clipboard::Kind, contents: String) {
        self.0 = contents;
    }
}

struct FieldTest<'a> {
    element: Element<'a, Message>,
    tree: Tree,
    renderer: Renderer,
    clipboard: TestClipboard,
    modifiers: Modifiers,
}

fn fallback() -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 11, 4).unwrap()
}

impl<'a> FieldTest<'a> {
    fn new(date: &'a str) -> Self {
        let element = input(date, fallback());
        Self {
            tree: Tree::new(&element),
            element,
            renderer: Renderer::new(iced::Font::default(), 16.into()),
            clipboard: TestClipboard::default(),
            modifiers: Modifiers::empty(),
        }
    }
    fn rebuild(&mut self, value: &'a str) {
        self.element = input(value, fallback());
        self.tree.diff(self.element.as_widget());
    }
    fn layout(&mut self) -> layout::Node {
        self.element.as_widget_mut().layout(
            &mut self.tree,
            &self.renderer,
            &layout::Limits::new(Size::ZERO, Size::new(280., 60.)),
        )
    }
    fn send(&mut self, event: Event, cursor: mouse::Cursor) -> Vec<Message> {
        let node = self.layout();
        let mut messages = Vec::new();
        self.element.as_widget_mut().update(
            &mut self.tree,
            &event,
            Layout::new(&node),
            cursor,
            &self.renderer,
            &mut self.clipboard,
            &mut Shell::new(&mut messages),
            &Rectangle::with_size(Size::new(280., 60.)),
        );
        messages
    }
    fn click(&mut self, point: Point) -> Vec<Message> {
        self.send(
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
            mouse::Cursor::Available(point),
        )
    }
    fn click_glyph(&mut self, index: usize) -> Vec<Message> {
        let node = self.layout();
        let bounds = Layout::new(&node).children().next().unwrap().bounds();
        let glyph = shaped(self.value(), bounds.height, &self.renderer)
            .grapheme_position(0, index)
            .unwrap();
        self.click(Point::new(bounds.x + glyph.x + 1., bounds.center_y()))
    }
    fn focus(&mut self) {
        let node = self.layout();
        self.element.as_widget_mut().operate(
            &mut self.tree,
            Layout::new(&node),
            &self.renderer,
            &mut operation::focusable::focus::<()>("editor-date".into()),
        );
    }
    fn mods(&mut self, modifiers: Modifiers) {
        self.modifiers = modifiers;
        self.send(
            Event::Keyboard(keyboard::Event::ModifiersChanged(modifiers)),
            mouse::Cursor::Unavailable,
        );
    }
    fn press(&mut self, key: Key, text: Option<&str>) -> Vec<Message> {
        self.send(
            Event::Keyboard(keyboard::Event::KeyPressed {
                modified_key: key.clone(),
                key,
                physical_key: key::Physical::Unidentified(key::NativeCode::Unidentified),
                location: keyboard::Location::Standard,
                modifiers: self.modifiers,
                text: text.map(Into::into),
                repeat: false,
            }),
            mouse::Cursor::Unavailable,
        )
    }
    fn character(&mut self, ch: &str) -> Vec<Message> {
        self.press(Key::Character(ch.into()), Some(ch))
    }
    fn value(&self) -> &str {
        &self.tree.state.downcast_ref::<Editing>().value
    }
}

#[test]
fn day_click_replaces_both_digits_and_resets_after_reclick_blur_and_external_pick() {
    let mut f = FieldTest::new("2026-11-04");
    assert!(matches!(
        &f.click_glyph(9)[..],
        [Message::DatePicker(Part::Day)]
    ));
    assert!(matches!(&f.character("1")[..], [Message::Field(Field::Date(s))] if s == "2026-11-01"));
    f.rebuild("2026-11-01");
    assert!(matches!(&f.character("5")[..], [Message::Field(Field::Date(s))] if s == "2026-11-15"));

    f.character("1");
    f.click_glyph(8);
    f.character("6");
    assert_eq!(f.value(), "2026-11-06");
    assert!(f.click(Point::new(1., 59.)).is_empty());
    assert!(f.character("9").is_empty());
    assert!(matches!(
        &f.click_glyph(8)[..],
        [Message::DatePicker(Part::Day)]
    ));
    f.character("1");
    f.rebuild("2026-11-21");
    f.character("5");
    assert_eq!(f.value(), "2026-11-05");
}

#[test]
fn zero_padding_year_month_limits_and_segment_navigation_use_real_calendar_dates() {
    let mut f = FieldTest::new("2024-02-29");
    f.focus();
    for digit in ["2", "0", "2", "8"] {
        f.character(digit);
    }
    assert_eq!(f.value(), "2028-02-29");
    f.press(Named::ArrowRight.into(), None);
    f.character("0");
    f.character("4");
    assert_eq!(f.value(), "2028-04-29");
    f.character("-");
    f.character("3");
    f.character("1");
    // April 31 cannot be published. The preceding 03 is still a valid day.
    assert_eq!(f.value(), "2028-04-03");
    f.press(Named::ArrowUp.into(), None);
    assert_eq!(f.value(), "2028-04-04");

    let mut f = FieldTest::new("2026-01-31");
    assert!(matches!(
        &f.click_glyph(6)[..],
        [Message::DatePicker(Part::Month)]
    ));
    f.character("2");
    assert_eq!(f.value(), "2026-02-28");
    assert!(matches!(
        &f.click_glyph(2)[..],
        [Message::DatePicker(Part::Year)]
    ));
    for digit in ["0", "0", "0", "1"] {
        f.character(digit);
    }
    assert_eq!(f.value(), "0001-02-28");
}

#[test]
fn blank_dates_support_picker_keyboard_paste_exact_iso_and_clearing() {
    let mut f = FieldTest::new("");
    assert!(matches!(
        &f.click_glyph(8)[..],
        [Message::DatePicker(Part::Day)]
    ));
    assert_eq!(f.value(), "");
    f.character("0");
    f.character("9");
    assert_eq!(f.value(), "2026-11-09");
    f.mods(Modifiers::COMMAND);
    f.character("a");
    f.clipboard.0 = "2000-02-29".into();
    assert!(matches!(&f.character("v")[..], [Message::Field(Field::Date(s))] if s == "2000-02-29"));
    f.character("a");
    f.mods(Modifiers::empty());
    f.press(Named::Delete.into(), None);
    assert_eq!(f.value(), "");
    f.mods(Modifiers::COMMAND);
    f.character("a");
    f.mods(Modifiers::empty());
    for ch in ["2", "0", "2", "6", "-", "0", "2", "-", "2", "9"] {
        f.character(ch);
    }
    assert_eq!(f.value(), "2026-02-29");
    assert!(timeschedule_core::storage::validate_date(f.value()).is_err());
    f.press(Named::Backspace.into(), None);
    f.character("8");
    assert!(timeschedule_core::storage::validate_date(f.value()).is_ok());
}

#[test]
fn enter_finishes_the_number_and_picker_shortcuts_only_handle_the_focused_field() {
    let mut f = FieldTest::new("2026-11-04");
    assert!(f.press(Named::Space.into(), None).is_empty());
    f.click_glyph(8);
    f.character("1");
    assert!(matches!(
        &f.press(Named::Enter.into(), None)[..],
        [Message::DismissEditorPicker]
    ));
    f.character("5");
    assert_eq!(f.value(), "2026-11-05");
    assert!(matches!(
        &f.press(Named::Space.into(), None)[..],
        [Message::DatePicker(Part::Day)]
    ));
    f.mods(Modifiers::ALT);
    assert!(matches!(
        &f.press(Named::ArrowDown.into(), None)[..],
        [Message::DatePicker(Part::Day)]
    ));
    f.mods(Modifiers::empty());
    assert!(matches!(
        &f.press(Named::Escape.into(), None)[..],
        [Message::DismissEditorPicker]
    ));
    assert!(f.character("7").is_empty());
    assert_eq!(f.value(), "2026-11-05");
}

#[test]
fn an_undated_task_highlights_the_clicked_placeholder_digits() {
    use iced::advanced::Renderer as _;
    let mut f = FieldTest::new("");
    f.click_glyph(8);
    assert_eq!(f.value(), "");
    let bounds = Rectangle::with_size(Size::new(280., 60.));
    f.renderer.reset(bounds);
    f.send(
        Event::Window(window::Event::RedrawRequested(std::time::Instant::now())),
        mouse::Cursor::Unavailable,
    );
    let node = f.layout();
    let layout = Layout::new(&node);
    let text = layout.children().next().unwrap().bounds();
    let day_x = text.x
        + shaped("", text.height, &f.renderer)
            .grapheme_position(0, 8)
            .unwrap()
            .x;
    f.element.as_widget().draw(
        &f.tree,
        &mut f.renderer,
        &Theme::Light,
        &renderer::Style::default(),
        layout,
        mouse::Cursor::Unavailable,
        &bounds,
    );
    let selection = text_input::default(
        &Theme::Light,
        text_input::Status::Focused { is_hovered: false },
    )
    .selection;
    assert!(f
        .renderer
        .layers()
        .iter()
        .flat_map(|layer| &layer.quads)
        .any(|(quad, fill)| *fill == selection.into()
            && (quad.bounds.x - day_x).abs() < 1.
            && quad.bounds.width > 2.));
}
