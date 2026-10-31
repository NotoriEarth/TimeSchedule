//! Tiny resolution-independent native glyphs, drawn without fonts or images.
use crate::app::Message;
use iced::advanced::Renderer as _;
use iced::advanced::{layout, mouse, renderer, widget::Tree, Layout, Widget};
use iced::{Border, Color, Element, Length, Rectangle, Renderer, Size, Theme};

#[derive(Clone, Copy)]
pub enum Icon {
    Calendar,
    Week,
    Folder,
    Record,
    Task,
    Note,
    Settings,
    Sun,
}
pub fn icon(kind: Icon) -> Element<'static, Message> {
    Element::new(kind)
}

impl Widget<Message, Theme, Renderer> for Icon {
    fn size(&self) -> Size<Length> {
        Size::new(Length::Fixed(18.0), Length::Fixed(18.0))
    }
    fn layout(&mut self, _: &mut Tree, _: &Renderer, limits: &layout::Limits) -> layout::Node {
        layout::Node::new(limits.resolve(18.0, 18.0, Size::new(18.0, 18.0)))
    }
    fn draw(
        &self,
        _: &Tree,
        renderer: &mut Renderer,
        _: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        _: mouse::Cursor,
        _: &Rectangle,
    ) {
        let b = layout.bounds();
        let ink = style.text_color;
        let mut rect = |x, y, w, h, outline, radius: f32| {
            renderer.fill_quad(
                renderer::Quad {
                    bounds: Rectangle {
                        x: b.x + x,
                        y: b.y + y,
                        width: w,
                        height: h,
                    },
                    border: Border {
                        color: ink,
                        width: if outline { 1.2 } else { 0.0 },
                        radius: radius.into(),
                    },
                    ..Default::default()
                },
                if outline { Color::TRANSPARENT } else { ink },
            );
        };
        match self {
            Self::Calendar | Self::Week => {
                rect(2.0, 3.0, 14.0, 13.0, true, 2.0);
                rect(2.5, 6.5, 13.0, 1.1, false, 0.0);
                rect(5.0, 1.5, 1.3, 4.0, false, 0.5);
                rect(11.5, 1.5, 1.3, 4.0, false, 0.5);
                if matches!(self, Self::Week) {
                    rect(6.0, 8.0, 1.0, 7.0, false, 0.0);
                    rect(11.0, 8.0, 1.0, 7.0, false, 0.0);
                } else {
                    rect(5.0, 10.0, 3.0, 3.0, false, 0.5);
                }
            }
            Self::Folder => {
                rect(2.0, 5.0, 14.0, 11.0, true, 2.0);
                rect(2.0, 2.5, 6.0, 3.0, true, 1.0);
            }
            Self::Record | Self::Note => {
                rect(3.0, 1.5, 12.0, 15.0, true, 2.0);
                for y in [6.0, 9.0, 12.0] {
                    rect(6.0, y, if y == 12.0 { 4.0 } else { 6.0 }, 1.0, false, 0.0);
                }
            }
            Self::Task => {
                rect(2.0, 2.0, 14.0, 14.0, true, 3.0);
                rect(5.0, 8.0, 2.0, 4.0, false, 0.5);
                rect(6.0, 11.0, 6.0, 1.5, false, 0.5);
                rect(11.0, 5.0, 1.5, 7.0, false, 0.5);
            }
            Self::Settings => {
                for (y, x) in [(4.0, 6.0), (9.0, 12.0), (14.0, 7.0)] {
                    rect(2.0, y, 14.0, 1.0, false, 0.0);
                    rect(x, y - 1.5, 2.0, 4.0, false, 1.0);
                }
            }
            Self::Sun => {
                rect(5.0, 5.0, 8.0, 8.0, true, 4.0);
                rect(8.5, 0.5, 1.0, 2.5, false, 0.0);
                rect(8.5, 15.0, 1.0, 2.5, false, 0.0);
                rect(0.5, 8.5, 2.5, 1.0, false, 0.0);
                rect(15.0, 8.5, 2.5, 1.0, false, 0.0);
            }
        }
    }
}
