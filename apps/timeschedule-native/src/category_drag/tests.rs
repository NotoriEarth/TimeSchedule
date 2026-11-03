use super::*;
use iced::widget::{container, scrollable, text};

fn view(ids: &[&str], enabled: bool, scrolling: bool) -> Element<'static, Message> {
    let rows = ids
        .iter()
        .map(|id| {
            (
                id.to_string(),
                container(text(id.to_string()))
                    .width(Length::Fill)
                    .height(ROW_HEIGHT)
                    .into(),
            )
        })
        .collect();
    let list = list(rows, enabled);
    if scrolling {
        scrollable(list).id(SCROLL_ID).height(174).into()
    } else {
        list
    }
}
struct Harness {
    element: Element<'static, Message>,
    tree: Tree,
    node: layout::Node,
    renderer: Renderer,
    now: Instant,
    pointer: Point,
}
impl Harness {
    fn new(ids: &[&str], scrolling: bool) -> Self {
        let mut element = view(ids, true, scrolling);
        let renderer = Renderer::new(iced::Font::with_name("Malgun Gothic"), 14.into());
        let mut tree = Tree::new(element.as_widget());
        let node = element.as_widget_mut().layout(
            &mut tree,
            &renderer,
            &layout::Limits::new(Size::ZERO, Size::new(300., 500.)),
        );
        let mut h = Self {
            element,
            tree,
            node,
            renderer,
            now: Instant::now() + Duration::from_secs(1),
            pointer: Point::new(20., 20.),
        };
        h.frame(0);
        h
    }
    fn event(
        &mut self,
        event: Event,
    ) -> (
        Vec<Message>,
        iced::window::RedrawRequest,
        iced::event::Status,
    ) {
        let mut messages = Vec::new();
        let mut shell = Shell::new(&mut messages);
        self.element.as_widget_mut().update(
            &mut self.tree,
            &event,
            Layout::new(&self.node),
            mouse::Cursor::Available(self.pointer),
            &self.renderer,
            &mut iced::advanced::clipboard::Null,
            &mut shell,
            &Rectangle::new(Point::ORIGIN, Size::new(300., 500.)),
        );
        let request = shell.redraw_request();
        let status = shell.event_status();
        drop(shell);
        for m in &messages {
            if let Message::Category(Action::Scroll(y)) = m {
                let mut op = iced::advanced::widget::operation::scrollable::scroll_by::<()>(
                    iced::advanced::widget::Id::new(SCROLL_ID),
                    iced::widget::operation::AbsoluteOffset { x: 0., y: *y },
                );
                self.element.as_widget_mut().operate(
                    &mut self.tree,
                    Layout::new(&self.node),
                    &self.renderer,
                    &mut op,
                );
            }
        }
        (messages, request, status)
    }
    fn frame(&mut self, ms: u64) -> iced::window::RedrawRequest {
        self.now += Duration::from_millis(ms);
        self.event(Event::Window(iced::window::Event::RedrawRequested(
            self.now,
        )))
        .1
    }
    fn press(&mut self, y: f32) {
        self.pointer = Point::new(20., y);
        self.event(Event::Mouse(mouse::Event::ButtonPressed(
            mouse::Button::Left,
        )));
    }
    fn move_to(&mut self, y: f32) -> Vec<Message> {
        self.pointer.y = y;
        self.event(Event::Mouse(mouse::Event::CursorMoved {
            position: self.pointer,
        }))
        .0
    }
    fn release(&mut self) -> Vec<Message> {
        self.event(Event::Mouse(mouse::Event::ButtonReleased(
            mouse::Button::Left,
        )))
        .0
    }
    fn rebuild(&mut self, ids: &[&str], enabled: bool, scrolling: bool) {
        self.element = view(ids, enabled, scrolling);
        self.tree.diff(self.element.as_widget());
        self.node = self.element.as_widget_mut().layout(
            &mut self.tree,
            &self.renderer,
            &layout::Limits::new(Size::ZERO, Size::new(300., 500.)),
        );
    }
    fn y(&mut self, label: &str) -> f32 {
        #[derive(Default)]
        struct Texts(Vec<(String, f32)>);
        impl Operation for Texts {
            fn traverse(&mut self, visit: &mut dyn FnMut(&mut dyn Operation)) {
                visit(self);
            }
            fn text(
                &mut self,
                _: Option<&iced::advanced::widget::Id>,
                bounds: Rectangle,
                value: &str,
            ) {
                self.0.push((value.into(), bounds.y));
            }
        }
        let mut probe = Texts::default();
        self.element.as_widget_mut().operate(
            &mut self.tree,
            Layout::new(&self.node),
            &self.renderer,
            &mut probe,
        );
        probe.0.into_iter().find(|(s, _)| s == label).unwrap().1
    }
}
fn reorder(messages: &[Message]) -> Vec<(Vec<String>, Vec<String>)> {
    messages
        .iter()
        .filter_map(|m| {
            if let Message::Category(Action::Reorder { expected, order }) = m {
                Some((expected.clone(), order.clone()))
            } else {
                None
            }
        })
        .collect()
}

#[test]
fn neighbors_animate_and_drop_emits_once_then_frames_stop() {
    let mut h = Harness::new(&["A", "B", "C"], false);
    h.press(83.);
    assert!(reorder(&h.move_to(15.)).is_empty());
    assert!(
        (h.y("B") + 10.).abs() < 0.1,
        "picked row follows cursor without easing"
    );
    assert!(h.y("A").abs() < 0.1);
    h.frame(75);
    assert!(
        h.y("A") > 0. && h.y("A") < 58.,
        "neighbor must visibly move between positions"
    );
    h.frame(75);
    assert!((h.y("A") - 58.).abs() < 0.1);
    assert_eq!(
        reorder(&h.release()),
        vec![(
            vec!["A".into(), "B".into(), "C".into()],
            vec!["B".into(), "A".into(), "C".into()]
        )]
    );
    assert!(reorder(&h.release()).is_empty());
    h.rebuild(&["B", "A", "C"], false, false);
    assert!(
        (h.y("B") + 10.).abs() < 0.1,
        "saving must not snap the landing position"
    );
    h.frame(75);
    assert!(h.y("B") > -10. && h.y("B") < 0.);
    h.frame(75);
    assert!(h.y("B").abs() < 0.1);
    assert_eq!(h.frame(16), iced::window::RedrawRequest::Wait);
}

#[test]
fn cancellation_and_small_handle_click_never_reorder() {
    for event in [
        Event::Window(iced::window::Event::Unfocused),
        Event::Mouse(mouse::Event::CursorLeft),
        Event::Keyboard(iced::keyboard::Event::KeyPressed {
            key: iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape),
            modified_key: iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape),
            physical_key: iced::keyboard::key::Physical::Code(iced::keyboard::key::Code::Escape),
            location: iced::keyboard::Location::Standard,
            modifiers: iced::keyboard::Modifiers::empty(),
            text: None,
            repeat: false,
        }),
    ] {
        let mut h = Harness::new(&["A", "B", "C"], false);
        h.press(83.);
        h.move_to(15.);
        assert_eq!(h.event(event).2, iced::event::Status::Captured);
        assert!(reorder(&h.release()).is_empty());
        h.frame(150);
        assert!((h.y("B") - 58.).abs() < 0.1);
        assert_eq!(h.frame(16), iced::window::RedrawRequest::Wait);
    }
    let mut h = Harness::new(&["A", "B"], false);
    h.press(83.);
    h.move_to(81.);
    assert!(reorder(&h.release()).is_empty());
    h.press(83.);
    h.move_to(15.);
    h.pointer.x = 350.;
    assert!(
        reorder(&h.release()).is_empty(),
        "outside release must cancel"
    );
}

#[test]
fn reversal_and_second_drag_keep_id_and_visual_position() {
    let mut h = Harness::new(&["A", "B", "C"], false);
    h.press(83.);
    h.move_to(15.);
    h.frame(60);
    let a_before = h.y("A");
    h.move_to(145.);
    assert!(
        (h.y("A") - a_before).abs() < 0.1,
        "retarget from current position, not old start"
    );
    h.release();
    h.rebuild(&["A", "C", "B"], true, false);
    h.frame(50);
    let y = h.y("B");
    h.press(y + 25.);
    assert!((h.y("B") - y).abs() < 0.1);
    h.move_to(15.);
    assert_eq!(reorder(&h.release())[0].1, vec!["B", "A", "C"]);
}

#[test]
fn scroll_and_edge_drag_preserve_target_coordinates() {
    let ids = ["A", "B", "C", "D", "E", "F", "G", "H"];
    let mut h = Harness::new(&ids, true);
    h.press(25.);
    h.move_to(160.);
    for _ in 0..55 {
        h.frame(16);
    }
    let result = reorder(&h.release());
    assert_eq!(result.len(), 1);
    assert!(
        result[0].1.iter().position(|id| id == "A").unwrap() >= 5,
        "edge scrolling should reach initially clipped rows: {result:?}"
    );
    h.frame(150);
    assert_eq!(h.frame(16), iced::window::RedrawRequest::Wait);
    let mut h = Harness::new(&ids, true);
    h.press(25.);
    h.event(Event::Mouse(mouse::Event::WheelScrolled {
        delta: mouse::ScrollDelta::Lines { x: 0., y: -3. },
    }));
    h.move_to(100.);
    h.frame(16);
    assert!(
        reorder(&h.release())[0]
            .1
            .iter()
            .position(|id| id == "A")
            .unwrap()
            >= 2
    );
}

#[test]
fn external_list_change_cancels_drag_and_arrow_reorder_animates() {
    let mut h = Harness::new(&["A", "B", "C"], false);
    h.press(83.);
    h.move_to(15.);
    h.rebuild(&["A", "C"], true, false);
    assert!(reorder(&h.release()).is_empty());
    h.frame(150);
    h.rebuild(&["C", "A"], false, false);
    h.frame(75);
    assert!(h.y("C") > 0. && h.y("C") < 58.);
    h.frame(75);
    assert!(h.y("C").abs() < 0.1);
    h.rebuild(&["A", "C"], true, false); // Failed save restores original order.
    h.frame(150);
    assert!(h.y("A").abs() < 0.1);
}
