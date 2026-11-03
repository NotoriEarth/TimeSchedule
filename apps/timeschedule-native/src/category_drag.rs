//! ID-keyed category motion. Pointer movement never writes to storage.
use crate::{
    app::Message,
    category_ui::{msg, Action},
    theme,
};
use iced::advanced::{
    layout, mouse, renderer,
    widget::{tree, Operation, Tree},
    Clipboard, Layout, Renderer as _, Shell, Widget,
};
use iced::{
    animation::{Animation, Easing},
    Alignment, Border, Color, Element, Event, Length, Point, Rectangle, Renderer, Shadow, Size,
    Theme, Vector,
};
use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

pub const ROW_HEIGHT: f32 = 50.0;
const GAP: f32 = 8.0;
const PITCH: f32 = ROW_HEIGHT + GAP;
const HANDLE_WIDTH: f32 = 45.0;
const MOTION: Duration = Duration::from_millis(150);
pub const SCROLL_ID: &str = "category-order";

pub fn list<'a>(rows: Vec<(String, Element<'a, Message>)>, enabled: bool) -> Element<'a, Message> {
    let (ids, children) = rows.into_iter().unzip();
    Element::new(CategoryList {
        ids,
        children,
        enabled,
    })
}
struct CategoryList<'a> {
    ids: Vec<String>,
    children: Vec<Element<'a, Message>>,
    enabled: bool,
}
struct RowMotion {
    id: String,
    y: Animation<f32>,
}
struct Drag {
    id: String,
    press: Point,
    grab_y: f32,
    top: f32,
    target: usize,
    moved: bool,
}
struct State {
    rows: Vec<RowMotion>,
    nodes: Vec<layout::Node>,
    drag: Option<Drag>,
    now: Instant,
}
fn motion(y: f32) -> Animation<f32> {
    Animation::new(y)
        .duration(MOTION)
        .easing(Easing::EaseOutCubic)
}
impl State {
    fn new(ids: &[String]) -> Self {
        Self {
            rows: ids
                .iter()
                .enumerate()
                .map(|(i, id)| RowMotion {
                    id: id.clone(),
                    y: motion(i as f32 * PITCH),
                })
                .collect(),
            nodes: Vec::new(),
            drag: None,
            now: Instant::now(),
        }
    }
    fn ids(&self) -> Vec<String> {
        self.rows.iter().map(|r| r.id.clone()).collect()
    }
    fn y(&self, i: usize) -> f32 {
        if let Some(d) = self
            .drag
            .as_ref()
            .filter(|d| d.moved && d.id == self.rows[i].id)
        {
            d.top
        } else {
            self.rows[i].y.interpolate_with(|v| v, self.now)
        }
    }
    fn targets(&mut self, order: &[String]) {
        for row in &mut self.rows {
            if let Some(index) = order.iter().position(|id| *id == row.id) {
                let target = index as f32 * PITCH;
                if row.y.value() != target {
                    row.y.go_mut(target, self.now);
                }
            }
        }
    }
    fn cancel(&mut self) {
        if let Some(drag) = self.drag.take() {
            if drag.moved {
                if let Some(row) = self.rows.iter_mut().find(|r| r.id == drag.id) {
                    row.y = motion(drag.top);
                }
            }
            self.targets(&self.ids());
        }
    }
    fn preview(&self) -> Vec<String> {
        let mut order = self.ids();
        if let Some(d) = &self.drag {
            order.retain(|id| *id != d.id);
            order.insert(d.target.min(order.len()), d.id.clone());
        }
        order
    }
    fn follow(&mut self, point: Point) {
        let Some(d) = self.drag.as_mut() else { return };
        d.moved |= point.distance(d.press) >= 4.0;
        if !d.moved {
            return;
        }
        d.top = point.y - d.grab_y;
        // Use fixed logical slot centers, never animated row bounds. This keeps
        // rows from repeatedly swapping under a stationary pointer.
        let center = d.top + ROW_HEIGHT / 2.0;
        d.target = self
            .rows
            .iter()
            .enumerate()
            .filter(|(i, row)| row.id != d.id && *i as f32 * PITCH + ROW_HEIGHT / 2.0 < center)
            .count();
        self.targets(&self.preview());
    }
    fn animating(&self) -> bool {
        self.rows.iter().any(|r| {
            !self.drag.as_ref().is_some_and(|d| d.moved && d.id == r.id)
                && r.y.is_animating(self.now)
        })
    }
}

impl Widget<Message, Theme, Renderer> for CategoryList<'_> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }
    fn state(&self) -> tree::State {
        tree::State::new(State::new(&self.ids))
    }
    fn children(&self) -> Vec<Tree> {
        self.children.iter().map(Tree::new).collect()
    }
    fn diff(&self, tree: &mut Tree) {
        let state = tree.state.downcast_mut::<State>();
        let old_ids = state.ids();
        let mut old: HashMap<_, _> = old_ids
            .iter()
            .cloned()
            .zip(std::mem::take(&mut tree.children))
            .collect();
        tree.children = self
            .ids
            .iter()
            .zip(&self.children)
            .map(|(id, child)| {
                let mut node = old.remove(id).unwrap_or_else(|| Tree::new(child));
                node.diff(child.as_widget());
                node
            })
            .collect();
        if old_ids != self.ids {
            state.cancel();
            let mut motions: HashMap<_, _> = std::mem::take(&mut state.rows)
                .into_iter()
                .map(|r| (r.id.clone(), r))
                .collect();
            state.rows = self
                .ids
                .iter()
                .enumerate()
                .map(|(i, id)| {
                    motions.remove(id).unwrap_or_else(|| RowMotion {
                        id: id.clone(),
                        y: motion(i as f32 * PITCH),
                    })
                })
                .collect();
            state.targets(&self.ids);
        }
        if !self.enabled {
            state.cancel();
        }
    }
    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, Length::Shrink)
    }
    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let node = layout::flex::resolve(
            layout::flex::Axis::Vertical,
            renderer,
            limits,
            Length::Fill,
            Length::Shrink,
            0.into(),
            GAP,
            Alignment::Start,
            &mut self.children,
            &mut tree.children,
        );
        // Cache only on layout; redraw frames shift these existing nodes without
        // relaying out text or allocating a new child tree for every frame.
        tree.state.downcast_mut::<State>().nodes = node.children().to_vec();
        node
    }
    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn Operation,
    ) {
        let state = tree.state.downcast_ref::<State>();
        operation.container(None, layout.bounds());
        operation.traverse(&mut |operation| {
            for (i, (child, child_tree)) in
                self.children.iter_mut().zip(&mut tree.children).enumerate()
            {
                child.as_widget_mut().operate(
                    child_tree,
                    row_layout(state, layout, i),
                    renderer,
                    operation,
                );
            }
        });
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
        let state = tree.state.downcast_mut::<State>();
        let now = match event {
            Event::Window(iced::window::Event::RedrawRequested(at)) => *at,
            _ => Instant::now(),
        }
        .max(state.now);
        let elapsed = now
            .saturating_duration_since(state.now)
            .as_secs_f32()
            .min(0.033);
        state.now = now;
        let bounds = layout.bounds();
        let visible = bounds.intersection(viewport);
        let pointer = cursor
            .position()
            .filter(|p| visible.is_some_and(|v| v.contains(*p)));
        let local = pointer.map(|p| p - Vector::new(bounds.x, bounds.y));
        if !self.enabled {
            state.cancel();
        }
        let cancel = matches!(
            event,
            Event::Window(iced::window::Event::Unfocused) | Event::Mouse(mouse::Event::CursorLeft)
        ) || matches!(
            event,
            Event::Keyboard(iced::keyboard::Event::KeyPressed {
                key: iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape),
                ..
            })
        );
        if cancel && state.drag.is_some() {
            state.cancel();
            shell.capture_event();
            shell.request_redraw();
            return;
        }
        if self.enabled
            && matches!(
                event,
                Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
            )
        {
            if let Some(point) = local.filter(|p| p.x < HANDLE_WIDTH) {
                if let Some(i) = (0..state.rows.len())
                    .rev()
                    .find(|i| (state.y(*i)..state.y(*i) + ROW_HEIGHT).contains(&point.y))
                {
                    let y = state.y(i);
                    state.drag = Some(Drag {
                        id: self.ids[i].clone(),
                        press: point,
                        grab_y: point.y - y,
                        top: y,
                        target: i,
                        moved: false,
                    });
                    shell.capture_event();
                    shell.request_redraw();
                    return;
                }
            }
        }
        if state.drag.is_some() {
            if let Some(point) = local {
                state.follow(point);
            }
            if matches!(
                event,
                Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))
            ) {
                let order = state.preview();
                let drag = state.drag.take().unwrap();
                let expected = state.ids();
                if drag.moved {
                    if let Some(row) = state.rows.iter_mut().find(|r| r.id == drag.id) {
                        row.y = motion(drag.top);
                    }
                }
                if drag.moved && pointer.is_some() && order != expected {
                    state.targets(&order);
                    shell.publish(msg(Action::Reorder { expected, order }));
                } else {
                    state.targets(&expected);
                }
                shell.capture_event();
                shell.request_redraw();
                return;
            }
            if matches!(
                event,
                Event::Window(iced::window::Event::RedrawRequested(_))
            ) {
                if let Some(p) = pointer.filter(|_| state.drag.as_ref().is_some_and(|d| d.moved)) {
                    let edge = if p.y - viewport.y < 24. && viewport.y > bounds.y + 0.5 {
                        -1.
                    } else if viewport.y + viewport.height - p.y < 24.
                        && viewport.y + viewport.height < bounds.y + bounds.height - 0.5
                    {
                        1.
                    } else {
                        0.
                    };
                    if edge != 0. {
                        shell.publish(msg(Action::Scroll(edge * elapsed * 240.)));
                        shell.request_redraw();
                    }
                }
            }
            // Let the outer scrollable consume wheel input while dragging.
            if matches!(event, Event::Mouse(mouse::Event::CursorMoved { .. })) {
                shell.capture_event();
                shell.request_redraw();
            }
        } else if self.enabled {
            for (i, (child, child_tree)) in
                self.children.iter_mut().zip(&mut tree.children).enumerate()
            {
                child.as_widget_mut().update(
                    child_tree,
                    event,
                    row_layout(state, layout, i),
                    cursor,
                    renderer,
                    clipboard,
                    shell,
                    viewport,
                );
            }
        }
        if state.animating() {
            shell.request_redraw();
        }
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
        let Some(visible) = layout.bounds().intersection(viewport) else {
            return;
        };
        let state = tree.state.downcast_ref::<State>();
        let active = state.drag.as_ref().filter(|d| d.moved);
        renderer.with_layer(visible, |renderer| {
            if let Some(d) = active {
                renderer.fill_quad(
                    renderer::Quad {
                        bounds: Rectangle {
                            y: layout.bounds().y + d.target as f32 * PITCH,
                            height: ROW_HEIGHT,
                            ..layout.bounds()
                        },
                        border: Border {
                            color: theme::colors(theme).accent,
                            width: 1.,
                            radius: 5.into(),
                        },
                        ..Default::default()
                    },
                    theme::colors(theme).accent.scale_alpha(0.08),
                );
            }
            for lifted in [false, true] {
                for (i, child) in self.children.iter().enumerate() {
                    let picked = active.is_some_and(|d| d.id == self.ids[i]);
                    if picked != lifted {
                        continue;
                    }
                    let row = row_layout(state, layout, i);
                    if !row.bounds().intersects(&visible) {
                        continue;
                    }
                    if picked {
                        renderer.fill_quad(
                            renderer::Quad {
                                bounds: row.bounds(),
                                border: Border {
                                    radius: 5.into(),
                                    ..Default::default()
                                },
                                shadow: Shadow {
                                    color: Color::BLACK.scale_alpha(0.22),
                                    offset: Vector::new(0., 2.),
                                    blur_radius: 5.,
                                },
                                ..Default::default()
                            },
                            theme::colors(theme).raised,
                        );
                    }
                    child.as_widget().draw(
                        &tree.children[i],
                        renderer,
                        theme,
                        style,
                        row,
                        if active.is_some() {
                            mouse::Cursor::Unavailable
                        } else {
                            cursor
                        },
                        &visible,
                    );
                    if picked {
                        renderer.fill_quad(
                            renderer::Quad {
                                bounds: row.bounds(),
                                border: Border {
                                    color: theme::colors(theme).accent,
                                    width: 2.,
                                    radius: 5.into(),
                                },
                                ..Default::default()
                            },
                            Color::TRANSPARENT,
                        );
                    }
                }
            }
        });
    }
    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        let state = tree.state.downcast_ref::<State>();
        if state.drag.is_some() {
            return mouse::Interaction::Grabbing;
        }
        if !self.enabled {
            return mouse::Interaction::None;
        }
        if let Some(p) = cursor
            .position()
            .filter(|p| viewport.contains(*p) && layout.bounds().contains(*p))
        {
            if p.x - layout.bounds().x < HANDLE_WIDTH {
                return mouse::Interaction::Grab;
            }
        }
        self.children
            .iter()
            .enumerate()
            .map(|(i, child)| {
                child.as_widget().mouse_interaction(
                    &tree.children[i],
                    row_layout(state, layout, i),
                    cursor,
                    viewport,
                    renderer,
                )
            })
            .max()
            .unwrap_or_default()
    }
}
fn row_layout<'a>(state: &'a State, layout: Layout<'_>, index: usize) -> Layout<'a> {
    let node = &state.nodes[index];
    Layout::with_offset(
        Vector::new(
            layout.position().x,
            layout.position().y + state.y(index) - node.bounds().y,
        ),
        node,
    )
}

#[cfg(test)]
mod tests;
