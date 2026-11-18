use super::*;
use iced::advanced::{
    clipboard, input_method, mouse,
    widget::{operation, Id},
};
use iced::{Event, Point, Renderer, Size};
use iced_runtime::UserInterface;

fn dispatch(
    ui: &mut UserInterface<'_, Message, iced::Theme, Renderer>,
    renderer: &mut Renderer,
    cursor: Point,
    events: &[Event],
) -> Vec<Message> {
    let mut messages = Vec::new();
    let _ = ui.update(
        events,
        mouse::Cursor::Available(cursor),
        renderer,
        &mut clipboard::Null,
        &mut messages,
    );
    messages
}

#[test]
fn clicking_the_input_shows_suggestions_and_accepts_exact_minutes() {
    let size = Size::new(320.0, 300.0);
    let mut renderer = Renderer::new(iced::Font::default(), 14.into());
    let mut ui = UserInterface::build(
        view("09:00", "10:00", None),
        size,
        Default::default(),
        &mut renderer,
    );
    let point = Point::new(40.0, 34.0);
    let click = [
        Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
        Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
    ];
    let messages = dispatch(&mut ui, &mut renderer, point, &click);
    assert!(matches!(
        &messages[..],
        [Message::OpenTimePicker(Field::Start)]
    ));

    let mut ui = UserInterface::build(
        view("09:00", "10:00", Some(Field::Start)),
        size,
        ui.into_cache(),
        &mut renderer,
    );
    let messages = dispatch(
        &mut ui,
        &mut renderer,
        point,
        &[Event::InputMethod(input_method::Event::Commit(
            "09:07".into(),
        ))],
    );
    assert!(matches!(&messages[..], [Message::Field(FormField::Start(value))] if value == "09:07"));

    let mut ui = UserInterface::build(
        view("09:07", "10:00", Some(Field::Start)),
        size,
        ui.into_cache(),
        &mut renderer,
    );
    let left = Point::new(8.0, point.y);
    assert!(dispatch(&mut ui, &mut renderer, left, &click).is_empty());
    let messages = dispatch(
        &mut ui,
        &mut renderer,
        left,
        &[Event::InputMethod(input_method::Event::Commit("1".into()))],
    );
    assert!(
        matches!(&messages[..], [Message::Field(FormField::Start(value))] if value == "109:07")
    );

    ui.operate(
        &renderer,
        &mut operation::focusable::focus(Id::new("editor-end")),
    );
    let redraw = Event::Window(iced::window::Event::RedrawRequested(
        iced::time::Instant::now(),
    ));
    let messages = dispatch(&mut ui, &mut renderer, point, &[redraw]);
    assert!(messages
        .iter()
        .any(|m| matches!(m, Message::OpenTimePicker(Field::End))));
    assert!(messages
        .iter()
        .any(|m| matches!(m, Message::DismissTimePicker(Field::Start))));
}

#[test]
fn a_visible_end_suggestion_selects_its_time_with_the_mouse_in_a_narrow_window() {
    use iced::advanced::widget::Operation;
    use iced::{Rectangle, Vector};
    #[derive(Default)]
    struct Options {
        scrolling: Option<(Rectangle, Vector)>,
        option: Option<Rectangle>,
        duration: Option<Rectangle>,
    }
    impl Operation for Options {
        fn traverse(&mut self, visit: &mut dyn FnMut(&mut dyn Operation)) {
            visit(self);
        }
        fn scrollable(
            &mut self,
            _: Option<&Id>,
            bounds: Rectangle,
            _: Rectangle,
            offset: Vector,
            _: &mut dyn operation::Scrollable,
        ) {
            self.scrolling = Some((bounds, offset));
        }
        fn text(&mut self, _: Option<&Id>, bounds: Rectangle, value: &str) {
            match value {
                "10:15" => self.option = Some(bounds),
                "1시간 8분" => self.duration = Some(bounds),
                _ => {}
            }
        }
    }
    let size = Size::new(305.0, 270.0);
    let mut renderer = Renderer::new(iced::Font::default(), 14.into());
    // The quick editor scrolls its form. Place the active input near the bottom of that viewport.
    let form = scrollable(iced::widget::column![
        space().height(350),
        view("09:07", "10:00", Some(Field::End)),
        space().height(90)
    ])
    .id("test-form")
    .height(Length::Fill);
    let mut ui = UserInterface::build(form, size, Default::default(), &mut renderer);
    ui.operate(
        &renderer,
        &mut operation::scrollable::scroll_to(
            Id::new("test-form"),
            scrollable::AbsoluteOffset {
                x: None,
                y: Some(180.0),
            },
        ),
    );
    let mut options = Options::default();
    ui.operate(&renderer, &mut options);
    let (viewport, scroll) = options
        .scrolling
        .expect("opened end field must have a scrollable menu");
    let time = options.option.expect("10:15 suggestion").center() - scroll;
    let duration = options
        .duration
        .expect("duration from the exact start")
        .center()
        - scroll;
    assert!(viewport.contains(time) && viewport.contains(duration));
    assert!(viewport.x >= 0.0 && viewport.x + viewport.width <= size.width);
    assert!(viewport.height < size.height);

    let messages = dispatch(
        &mut ui,
        &mut renderer,
        time,
        &[
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
        ],
    );
    assert!(
        matches!(&messages[..], [Message::PickEditorTime(Field::End, value)] if value == "10:15")
    );
}

#[test]
fn initial_scroll_tracks_the_current_time_or_the_next_valid_end() {
    // An exact, off-grid time is between the 09:00 and 09:15 suggestions.
    assert_eq!(selected_quarter(Field::Start, Some(547), Some(547)), 36);
    assert_eq!(selected_quarter(Field::End, Some(547), Some(608)), 40);
    assert_eq!(selected_quarter(Field::End, Some(547), Some(525)), 37);
    assert_eq!(selected_quarter(Field::End, Some(1439), None), 95);
    assert_eq!(selected_quarter(Field::Start, None, None), 36);
}

#[test]
fn end_durations_use_the_exact_typed_start_and_stay_on_the_same_day() {
    assert_eq!(elapsed(547, 555).as_deref(), Some("8분"));
    assert_eq!(elapsed(547, 615).as_deref(), Some("1시간 8분"));
    assert_eq!(elapsed(540, 600).as_deref(), Some("1시간"));
    assert_eq!(elapsed(540, 540), None);
    assert_eq!(elapsed(1439, 0), None);
}
