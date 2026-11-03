use crate::{
    app::{App, Message},
    theme, worker,
};
use iced::widget::{
    button, column, container, mouse_area, row, scrollable, space, text, text_input,
};
use iced::{Element, Length, Task};
#[derive(Default)]
pub struct State {
    pub open: bool,
    pub editing: Option<String>,
    pub name: String,
    pub color: String,
    pub drag: Option<(String, usize)>,
    pub saving: bool,
    pub error: String,
}
#[derive(Debug, Clone)]
pub enum Action {
    Open,
    Close,
    Edit(String),
    Add,
    Name(String),
    Color(String),
    Save,
    Drag(String),
    Over(usize),
    Drop,
    Move(String, i32),
    Saved(Result<(), String>),
}
pub fn msg(action: Action) -> Message {
    Message::Category(action)
}
pub fn update(app: &mut App, action: Action) -> Task<Message> {
    if app.categories.saving && !matches!(action, Action::Saved(_)) {
        return Task::none();
    }
    match action {
        Action::Open if app.editor.is_none() => {
            app.categories = State {
                open: true,
                color: "#82A8D8".into(),
                ..State::default()
            };
        }
        Action::Close => app.categories.open = false,
        Action::Add => {
            app.categories.editing = None;
            app.categories.name.clear();
            app.categories.color = "#82A8D8".into();
        }
        Action::Edit(id) => {
            if let Some(c) = app.snapshot.categories.iter().find(|c| c.id == id) {
                app.categories.editing = Some(id);
                app.categories.name = c.name.clone();
                app.categories.color = c.color.clone();
            }
        }
        Action::Name(value) => app.categories.name = value,
        Action::Color(value) => app.categories.color = value,
        Action::Save => {
            if let Some(store) = app.store.clone() {
                let id = app.categories.editing.clone();
                let name = app.categories.name.clone();
                let color = app.categories.color.clone();
                app.categories.saving = true;
                app.categories.error.clear();
                return worker::job(
                    store,
                    move |s| s.save_category(id.as_deref(), &name, &color).map(|_| ()),
                    |r| msg(Action::Saved(r)),
                );
            }
        }
        Action::Drag(id) => {
            if let Some(i) = app.snapshot.categories.iter().position(|c| c.id == id) {
                app.categories.drag = Some((id, i));
            }
        }
        Action::Over(i) => {
            if let Some((_, index)) = &mut app.categories.drag {
                *index = i;
            }
        }
        Action::Drop => {
            if let Some((id, index)) = app.categories.drag.take() {
                return reorder(app, id, index);
            }
        }
        Action::Move(id, delta) => {
            if let Some(i) = app.snapshot.categories.iter().position(|c| c.id == id) {
                let target = (i as i32 + delta)
                    .clamp(0, app.snapshot.categories.len().saturating_sub(1) as i32)
                    as usize;
                return reorder(app, id, target);
            }
        }
        Action::Saved(result) => {
            app.categories.saving = false;
            match result {
                Ok(()) => {
                    app.categories.error.clear();
                    app.categories.editing = None;
                    app.categories.name.clear();
                    return Task::batch([app.refresh(), app.load_quick(false)]);
                }
                Err(e) => app.categories.error = e,
            }
        }
        _ => {}
    }
    Task::none()
}
fn reorder(app: &mut App, id: String, index: usize) -> Task<Message> {
    let expected: Vec<_> = app
        .snapshot
        .categories
        .iter()
        .map(|c| c.id.clone())
        .collect();
    let Some(from) = expected.iter().position(|v| *v == id) else {
        return Task::none();
    };
    if from == index {
        return Task::none();
    }
    let mut order = expected.clone();
    order.remove(from);
    order.insert(index.min(order.len()), id);
    if let Some(store) = app.store.clone() {
        app.categories.saving = true;
        return worker::job(
            store,
            move |s| s.reorder_categories(&expected, &order),
            |r| msg(Action::Saved(r)),
        );
    }
    Task::none()
}
pub fn view(app: &App) -> Element<'_, Message> {
    let state = &app.categories;
    let mut rows = column![
        text("위에서부터 날짜 칸에 먼저 표시됩니다.").size(13),
        text("손잡이를 드래그하거나 ↑ ↓ 버튼으로 순서를 바꾸세요.")
            .size(12)
            .style(theme::muted)
    ]
    .spacing(8);
    for (i, c) in app.snapshot.categories.iter().enumerate() {
        let ink = crate::calendar_ui::color(app, &c.id);
        let target = state.drag.as_ref().is_some_and(|(_, n)| *n == i);
        let handle = mouse_area(container(text("⠿").size(20)).padding([7, 12]))
            .on_press(msg(Action::Drag(c.id.clone())))
            .interaction(iced::mouse::Interaction::Grab);
        let line = row![
            handle,
            text(format!("{}", i + 1)).size(12),
            container(space().width(12).height(12))
                .style(move |_| container::Style::default().background(ink)),
            button(text(&c.name))
                .width(Length::Fill)
                .style(theme::text_button)
                .on_press(msg(Action::Edit(c.id.clone()))),
            button("↑").on_press_maybe((i > 0).then(|| msg(Action::Move(c.id.clone(), -1)))),
            button("↓").on_press_maybe(
                (i + 1 < app.snapshot.categories.len()).then(|| msg(Action::Move(c.id.clone(), 1)))
            )
        ]
        .spacing(8)
        .align_y(iced::Alignment::Center);
        rows = rows.push(
            mouse_area(
                container(line)
                    .padding(5)
                    .width(Length::Fill)
                    .style(move |t| {
                        let mut style = theme::surface(t);
                        style.border = iced::Border {
                            color: if target {
                                theme::colors(t).accent
                            } else {
                                theme::colors(t).line
                            },
                            width: if target { 2.0 } else { 1.0 },
                            radius: 5.0.into(),
                        };
                        style
                    }),
            )
            .on_enter(msg(Action::Over(i)))
            .on_release(msg(Action::Drop)),
        );
    }
    let mut swatches = row![].spacing(6);
    for value in [
        "#80B6A5", "#82A8D8", "#D9AD65", "#CE8DAB", "#9A90CD", "#82BABD",
    ] {
        let color = crate::calendar_ui::parse_color(value).unwrap();
        swatches = swatches.push(
            button(
                container(space().width(22).height(18))
                    .style(move |_| container::Style::default().background(color)),
            )
            .padding(3)
            .on_press(msg(Action::Color(value.into()))),
        );
    }
    let fields = column![
        row![
            text(if state.editing.is_some() {
                "카테고리 수정"
            } else {
                "새 카테고리"
            })
            .size(16),
            space().width(Length::Fill),
            button("+ 추가").on_press(msg(Action::Add))
        ],
        text_input("카테고리 이름", &state.name)
            .on_input(|v| msg(Action::Name(v)))
            .padding(8),
        swatches,
        text_input("색상 #RRGGBB", &state.color)
            .on_input(|v| msg(Action::Color(v)))
            .padding(8),
        text(&state.error).size(12).style(theme::danger),
        button(if state.saving {
            "저장 중…"
        } else {
            "저장"
        })
        .on_press_maybe((!state.saving && !app.loading).then_some(msg(Action::Save)))
    ]
    .spacing(10);
    column![
        row![
            text("카테고리").size(23),
            space().width(Length::Fill),
            button("닫기").on_press_maybe((!state.saving).then_some(msg(Action::Close)))
        ],
        scrollable(rows).height(Length::Fill),
        fields
    ]
    .spacing(16)
    .height(Length::Fill)
    .into()
}
