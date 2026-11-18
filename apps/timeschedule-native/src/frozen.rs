//! Input handling around a mounted editor or its open chooser.
use crate::app::Message;
use iced::advanced::{
    layout, mouse, overlay, renderer,
    widget::{tree, Operation, Tree},
    Clipboard, Layout, Shell, Widget,
};
use iced::{Element, Event, Length, Rectangle, Renderer, Size, Theme, Vector};

pub fn frozen(content: Element<'_, Message>, blocked: bool) -> Element<'_, Message> {
    Element::new(EditorInput {
        content,
        blocked,
        outside: None,
    })
}
pub fn dismiss_on_outside_press(
    content: Element<'_, Message>,
    message: Message,
) -> Element<'_, Message> {
    Element::new(EditorInput {
        content,
        blocked: false,
        outside: Some(message),
    })
}
struct EditorInput<'a> {
    content: Element<'a, Message>,
    blocked: bool,
    outside: Option<Message>,
}
impl Widget<Message, Theme, Renderer> for EditorInput<'_> {
    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
    }
    fn size_hint(&self) -> Size<Length> {
        self.content.as_widget().size_hint()
    }
    fn tag(&self) -> tree::Tag {
        self.content.as_widget().tag()
    }
    fn state(&self) -> tree::State {
        self.content.as_widget().state()
    }
    fn children(&self) -> Vec<Tree> {
        self.content.as_widget().children()
    }
    fn diff(&self, tree: &mut Tree) {
        self.content.as_widget().diff(tree);
    }
    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        self.content.as_widget_mut().layout(tree, renderer, limits)
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
        self.content
            .as_widget()
            .draw(tree, renderer, theme, style, layout, cursor, viewport);
    }
    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn Operation,
    ) {
        if !self.blocked {
            self.content
                .as_widget_mut()
                .operate(tree, layout, renderer, operation);
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
        if !self.blocked {
            if let Some(dismiss) = &self.outside {
                let position = match event {
                    Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                        cursor.position()
                    }
                    Event::Touch(iced::touch::Event::FingerPressed { position, .. }) => {
                        Some(*position)
                    }
                    _ => None,
                };
                if position.is_some_and(|p| !layout.bounds().contains(p)) {
                    shell.publish(dismiss.clone());
                }
            }
            self.content.as_widget_mut().update(
                tree, event, layout, cursor, renderer, clipboard, shell, viewport,
            );
        }
        // Leave Escape unclaimed so the app subscription can dismiss confirmation.
    }
    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        if self.blocked {
            mouse::Interaction::None
        } else {
            self.content
                .as_widget()
                .mouse_interaction(tree, layout, cursor, viewport, renderer)
        }
    }
    fn overlay<'a>(
        &'a mut self,
        tree: &'a mut Tree,
        layout: Layout<'a>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'a, Message, Theme, Renderer>> {
        if self.blocked {
            None
        } else {
            self.content
                .as_widget_mut()
                .overlay(tree, layout, renderer, viewport, translation)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn blocked_editor_does_not_handle_keyboard_but_resumes_same_selection() {
        use iced::{
            advanced::{clipboard, text::Renderer as TextRenderer},
            keyboard::{self, key},
            widget::text_input,
        };
        type InputState = text_input::State<<Renderer as TextRenderer>::Paragraph>;
        let input = || text_input("", "abcdef").on_input(Message::Search).into();
        let renderer = Renderer::new(iced::Font::default(), 16.into());
        let mut element = frozen(input(), true);
        let mut tree = Tree::new(element.as_widget());
        let node = element.as_widget_mut().layout(
            &mut tree,
            &renderer,
            &layout::Limits::new(Size::ZERO, Size::new(200.0, 80.0)),
        );
        let state = tree.state.downcast_mut::<InputState>();
        state.focus();
        state.select_range(1, 3);
        let expected = state.cursor();
        let event = Event::Keyboard(keyboard::Event::KeyPressed {
            key: keyboard::Key::Character("a".into()),
            modified_key: keyboard::Key::Character("a".into()),
            physical_key: key::Physical::Code(key::Code::KeyA),
            location: keyboard::Location::Standard,
            modifiers: keyboard::Modifiers::empty(),
            text: Some("a".into()),
            repeat: false,
        });
        let mut messages = vec![];
        element.as_widget_mut().update(
            &mut tree,
            &event,
            Layout::new(&node),
            mouse::Cursor::Unavailable,
            &renderer,
            &mut clipboard::Null,
            &mut Shell::new(&mut messages),
            &Rectangle::with_size(Size::new(200.0, 80.0)),
        );
        assert!(messages.is_empty());
        assert_eq!(tree.state.downcast_ref::<InputState>().cursor(), expected);
        element = frozen(input(), false);
        tree.diff(element.as_widget());
        assert_eq!(tree.state.downcast_ref::<InputState>().cursor(), expected);
        element.as_widget_mut().update(
            &mut tree,
            &event,
            Layout::new(&node),
            mouse::Cursor::Unavailable,
            &renderer,
            &mut clipboard::Null,
            &mut Shell::new(&mut messages),
            &Rectangle::with_size(Size::new(200.0, 80.0)),
        );
        assert!(matches!(&messages[..], [Message::Search(value)] if value == "aadef"));
    }
}
