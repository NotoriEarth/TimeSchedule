use crate::{
    app::{App, Message},
    theme, worker,
};
use iced::widget::{
    button, column, container, responsive, row, scrollable, space, text, text_input,
};
use iced::{Element, Length, Task};
use timeschedule_core::calendar::{
    Category, CategoryDeletePreview, RECORD_CATEGORY, SCHEDULE_CATEGORY,
};
#[derive(Default)]
pub struct State {
    pub open: bool,
    pub editing: Option<String>,
    pub editing_version: Option<Category>,
    pub name: String,
    pub color: String,
    pub pending_order: Option<Vec<String>>,
    pub saving: bool,
    pub error: String,
    pub notice: String,
    pub delete_preview: Option<CategoryDeletePreview>,
    pub delete_target: Option<Category>,
    pub delete_generation: u64,
    pub delete_loading: bool,
}
#[derive(Debug, Clone)]
pub enum Action {
    Open,
    Close,
    Edit(String),
    NewDraft,
    Name(String),
    Color(String),
    Save,
    RequestDelete,
    DeletePreviewed {
        generation: u64,
        result: Result<CategoryDeletePreview, String>,
    },
    DeleteTarget(String),
    CancelDelete,
    ConfirmDelete,
    Deleted(Result<usize, String>),
    Reorder {
        expected: Vec<String>,
        order: Vec<String>,
    },
    Scroll(f32),
    Move(String, i32),
    Saved(Result<(), String>),
    Reordered(Result<(), String>),
}
pub fn msg(action: Action) -> Message {
    Message::Category(action)
}
pub fn update(app: &mut App, action: Action) -> Task<Message> {
    if app.categories.saving
        && !matches!(
            action,
            Action::Saved(_) | Action::Reordered(_) | Action::Deleted(_)
        )
    {
        return Task::none();
    }
    if (app.categories.delete_loading || app.categories.delete_preview.is_some())
        && !matches!(
            action,
            Action::Close
                | Action::CancelDelete
                | Action::DeletePreviewed { .. }
                | Action::DeleteTarget(_)
                | Action::ConfirmDelete
                | Action::Deleted(_)
        )
    {
        return Task::none();
    }
    let fields_saved = matches!(action, Action::Saved(_));
    match action {
        Action::Open if app.editor.is_none() => {
            app.categories = State {
                open: true,
                color: "#82A8D8".into(),
                delete_generation: app.categories.delete_generation.wrapping_add(1),
                ..State::default()
            };
        }
        Action::Close => {
            if app.categories.delete_loading || app.categories.delete_preview.is_some() {
                cancel_delete(&mut app.categories);
            } else {
                app.categories.open = false;
                cancel_delete(&mut app.categories);
            }
        }
        Action::CancelDelete => cancel_delete(&mut app.categories),
        Action::NewDraft => {
            app.categories.editing = None;
            app.categories.editing_version = None;
            app.categories.name.clear();
            app.categories.color = "#82A8D8".into();
            app.categories.error.clear();
            app.categories.notice.clear();
        }
        Action::Edit(id) => {
            if let Some(c) = app.snapshot.categories.iter().find(|c| c.id == id) {
                app.categories.editing = Some(id);
                app.categories.editing_version = Some(c.clone());
                app.categories.name = c.name.clone();
                app.categories.color = c.color.clone();
                app.categories.error.clear();
                app.categories.notice.clear();
            }
        }
        Action::Name(value) => app.categories.name = value,
        Action::Color(value) => app.categories.color = value,
        Action::Save => {
            if let Some(store) = app.store.clone() {
                let expected = app.categories.editing_version.clone();
                let name = app.categories.name.clone();
                let color = app.categories.color.clone();
                app.categories.saving = true;
                app.categories.error.clear();
                return worker::job(
                    store,
                    move |s| {
                        match expected {
                            Some(category) => s.update_category_checked(&category, &name, &color),
                            None => s.save_category(None, &name, &color),
                        }
                        .map(|_| ())
                    },
                    |r| msg(Action::Saved(r)),
                );
            }
        }
        Action::RequestDelete if app.categories.open && !app.loading => {
            if let (Some(store), Some(id)) = (app.store.clone(), app.categories.editing.clone()) {
                if [SCHEDULE_CATEGORY, RECORD_CATEGORY].contains(&id.as_str()) {
                    app.categories.error = "기본 카테고리는 삭제할 수 없습니다.".into();
                    return Task::none();
                }
                app.categories.delete_generation = app.categories.delete_generation.wrapping_add(1);
                let generation = app.categories.delete_generation;
                app.categories.delete_loading = true;
                app.categories.delete_preview = None;
                app.categories.delete_target = None;
                app.categories.notice.clear();
                app.categories.error.clear();
                return worker::job(
                    store,
                    move |s| s.preview_category_deletion(&id),
                    move |result| msg(Action::DeletePreviewed { generation, result }),
                );
            }
        }
        Action::DeletePreviewed { generation, result } => {
            if !app.categories.open
                || !app.categories.delete_loading
                || generation != app.categories.delete_generation
            {
                return Task::none();
            }
            app.categories.delete_loading = false;
            match result {
                Ok(preview) => app.categories.delete_preview = Some(preview),
                Err(error) => app.categories.error = error,
            }
        }
        Action::DeleteTarget(id) => {
            if let Some(preview) = &app.categories.delete_preview {
                app.categories.delete_target = app
                    .snapshot
                    .categories
                    .iter()
                    .find(|c| {
                        c.id == id && c.id != preview.category.id && c.id != SCHEDULE_CATEGORY
                    })
                    .cloned();
            }
        }
        Action::ConfirmDelete => {
            if let (Some(store), Some(preview)) =
                (app.store.clone(), app.categories.delete_preview.clone())
            {
                let target = app.categories.delete_target.clone();
                if !preview.entry_ids.is_empty() && target.is_none() {
                    return Task::none();
                }
                app.categories.saving = true;
                app.categories.error.clear();
                return worker::job(
                    store,
                    move |s| s.delete_category(&preview, target.as_ref()),
                    |result| msg(Action::Deleted(result)),
                );
            }
        }
        Action::Deleted(result) => {
            app.categories.saving = false;
            match result {
                Ok(count) => {
                    app.categories.notice = if count == 0 {
                        "카테고리를 삭제했습니다.".into()
                    } else {
                        format!(
                            "기록 {count}개를 ‘{}’으로 옮기고 카테고리를 삭제했습니다.",
                            app.categories
                                .delete_target
                                .as_ref()
                                .map(|c| c.name.as_str())
                                .unwrap_or_default()
                        )
                    };
                    cancel_delete(&mut app.categories);
                    app.categories.editing = None;
                    app.categories.editing_version = None;
                    app.categories.name.clear();
                    app.categories.error.clear();
                    return app.refresh();
                }
                Err(error) => {
                    cancel_delete(&mut app.categories);
                    let preview = update(app, Action::RequestDelete);
                    app.categories.error = error;
                    return Task::batch([preview, app.refresh()]);
                }
            }
        }
        Action::Reorder { expected, order } if app.categories.open && !app.loading => {
            return commit_order(app, expected, order)
        }
        Action::Scroll(y) if app.categories.open && !app.loading => {
            return iced::widget::operation::scroll_by(
                crate::category_drag::SCROLL_ID,
                iced::widget::operation::AbsoluteOffset { x: 0., y },
            )
        }
        Action::Move(id, delta) => {
            if let Some(i) = app.snapshot.categories.iter().position(|c| c.id == id) {
                let target = (i as i32 + delta)
                    .clamp(0, app.snapshot.categories.len().saturating_sub(1) as i32)
                    as usize;
                return reorder(app, id, target);
            }
        }
        Action::Saved(result) | Action::Reordered(result) => {
            app.categories.saving = false;
            let pending = app.categories.pending_order.take();
            match result {
                Ok(()) => {
                    if let Some(order) = pending {
                        if let Some(version) = &mut app.categories.editing_version {
                            if let Some(position) = order.iter().position(|id| *id == version.id) {
                                version.position = position as u32;
                            }
                        }
                        app.snapshot.categories.sort_by_key(|c| {
                            order
                                .iter()
                                .position(|id| *id == c.id)
                                .unwrap_or(usize::MAX)
                        });
                    }
                    app.categories.error.clear();
                    if fields_saved {
                        app.categories.editing = None;
                        app.categories.editing_version = None;
                        app.categories.name.clear();
                    }
                    return app.refresh();
                }
                Err(e) => {
                    app.categories.error = e;
                    if !fields_saved {
                        // A rejected compare-and-swap leaves storage untouched.
                        // Reload its latest order so retry can succeed, while
                        // preserving the user's name/color draft and the error.
                        return app.refresh();
                    }
                }
            }
        }
        _ => {}
    }
    Task::none()
}
fn cancel_delete(state: &mut State) {
    state.delete_generation = state.delete_generation.wrapping_add(1);
    state.delete_loading = false;
    state.delete_preview = None;
    state.delete_target = None;
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
    commit_order(app, expected, order)
}
fn commit_order(app: &mut App, expected: Vec<String>, order: Vec<String>) -> Task<Message> {
    let current: Vec<_> = app
        .snapshot
        .categories
        .iter()
        .map(|c| c.id.clone())
        .collect();
    let mut sorted = order.clone();
    sorted.sort();
    sorted.dedup();
    if expected != current
        || expected == order
        || sorted.len() != expected.len()
        || sorted.iter().any(|id| !expected.contains(id))
    {
        return Task::none();
    }
    if let Some(store) = app.store.clone() {
        app.categories.saving = true;
        app.categories.error.clear();
        app.categories.pending_order = Some(order.clone());
        return worker::job(
            store,
            move |s| s.reorder_categories(&expected, &order),
            |r| msg(Action::Reordered(r)),
        );
    }
    Task::none()
}
pub fn view(app: &App) -> Element<'_, Message> {
    let state = &app.categories;
    if state.delete_loading || state.delete_preview.is_some() {
        return delete_view(app);
    }
    let instructions = column![
        text("위에서부터 날짜 칸에 먼저 표시됩니다.").size(13),
        text("손잡이를 드래그하거나 ↑ ↓ 버튼으로 순서를 바꾸세요.")
            .size(12)
            .style(theme::muted)
    ]
    .spacing(8);
    let mut ordered: Vec<_> = app.snapshot.categories.iter().collect();
    if let Some(order) = &state.pending_order {
        ordered.sort_by_key(|c| {
            order
                .iter()
                .position(|id| *id == c.id)
                .unwrap_or(usize::MAX)
        });
    }
    let mut rows = Vec::new();
    let enabled = !state.saving && !app.loading;
    for (i, c) in ordered.into_iter().enumerate() {
        let ink = crate::calendar_ui::color(app, &c.id);
        let handle = container(text("⠿").size(20)).padding([7, 12]);
        let name = responsive(move |bounds| {
            text(crate::text_layout::fit_label(&c.name, bounds.width, 16.))
                .size(16)
                .wrapping(text::Wrapping::None)
                .into()
        })
        .height(21)
        .width(Length::Fill);
        let line = row![
            handle,
            text(format!("{}", i + 1)).size(12),
            container(space().width(12).height(12))
                .style(move |_| container::Style::default().background(ink)),
            button(name)
                .width(Length::Fill)
                .style(theme::text_button)
                .on_press_maybe(enabled.then(|| msg(Action::Edit(c.id.clone())))),
            button("수정").on_press_maybe(enabled.then(|| msg(Action::Edit(c.id.clone())))),
            button("↑")
                .on_press_maybe((enabled && i > 0).then(|| msg(Action::Move(c.id.clone(), -1)))),
            button("↓").on_press_maybe(
                (enabled && i + 1 < app.snapshot.categories.len())
                    .then(|| msg(Action::Move(c.id.clone(), 1)))
            )
        ]
        .spacing(8)
        .align_y(iced::Alignment::Center);
        rows.push((
            c.id.clone(),
            container(line)
                .padding(5)
                .width(Length::Fill)
                .height(crate::category_drag::ROW_HEIGHT)
                .clip(true)
                .style(move |t| {
                    let mut style = theme::surface(t);
                    style.border = iced::Border {
                        color: theme::colors(t).line,
                        width: 1.0,
                        radius: 5.0.into(),
                    };
                    style
                })
                .into(),
        ));
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
            button(if state.editing.is_some() {
                "새로 작성"
            } else {
                "+ 추가"
            })
            .on_press_maybe(enabled.then(|| msg(if state.editing.is_some() {
                Action::NewDraft
            } else {
                Action::Save
            })))
        ],
        text("이름").size(12).style(theme::muted),
        text_input("카테고리 이름", &state.name)
            .on_input(|v| msg(Action::Name(v)))
            .padding(8),
        swatches,
        text_input("색상 #RRGGBB", &state.color)
            .on_input(|v| msg(Action::Color(v)))
            .padding(8),
        text(&state.error).size(12).style(theme::danger),
        text(&state.notice).size(12).style(theme::muted),
        edit_actions(state, enabled)
    ]
    .spacing(10);
    column![
        row![
            text("카테고리").size(23),
            space().width(Length::Fill),
            button("닫기").on_press_maybe((!state.saving).then_some(msg(Action::Close)))
        ],
        instructions,
        scrollable(crate::category_drag::list(rows, enabled))
            .id(crate::category_drag::SCROLL_ID)
            .height(Length::Fill),
        fields
    ]
    .spacing(16)
    .height(Length::Fill)
    .into()
}

fn edit_actions(state: &State, enabled: bool) -> Element<'_, Message> {
    let save = button(if state.saving {
        "저장 중…"
    } else if state.editing.is_some() {
        "변경 저장"
    } else {
        "카테고리 추가"
    })
    .style(theme::primary)
    .on_press_maybe(enabled.then_some(msg(Action::Save)));
    match state.editing.as_deref() {
        Some(SCHEDULE_CATEGORY | RECORD_CATEGORY) => column![
            text("기본 카테고리는 삭제할 수 없습니다")
                .size(12)
                .style(theme::muted),
            save
        ]
        .spacing(8)
        .into(),
        Some(_) => row![
            button(text("카테고리 삭제").style(theme::danger))
                .style(theme::text_button)
                .on_press_maybe(enabled.then_some(msg(Action::RequestDelete))),
            space().width(Length::Fill),
            save
        ]
        .align_y(iced::Alignment::Center)
        .into(),
        None => save.into(),
    }
}

fn delete_view(app: &App) -> Element<'_, Message> {
    let state = &app.categories;
    let cancel =
        button("취소").on_press_maybe((!state.saving).then_some(msg(Action::CancelDelete)));
    let Some(preview) = &state.delete_preview else {
        return column![
            text("카테고리 삭제").size(23),
            text("연결된 기록을 확인하고 있습니다…").size(14),
            cancel
        ]
        .spacing(18)
        .into();
    };
    let count = preview.entry_ids.len();
    let mut content = column![
        text("카테고리 삭제").size(23),
        text(format!(
            "‘{}’ 카테고리를 삭제할까요?",
            preview.category.name
        ))
        .size(19),
        text(if count == 0 {
            "연결된 기록이 없습니다. 카테고리만 삭제합니다.".into()
        } else {
            format!("전체 날짜의 기록 {count}개를 옮깁니다. 원문과 날짜는 그대로 유지됩니다.")
        })
        .size(14),
    ]
    .spacing(18);
    if count > 0 {
        content = content.push(text("기록을 옮길 카테고리").size(14));
        for category in &app.snapshot.categories {
            if category.id == preview.category.id || category.id == SCHEDULE_CATEGORY {
                continue;
            }
            let selected = state
                .delete_target
                .as_ref()
                .is_some_and(|c| c.id == category.id);
            content = content.push(
                button(
                    row![text(if selected { "●" } else { "○" }), text(&category.name)].spacing(10),
                )
                .width(Length::Fill)
                .padding(10)
                .style(if selected {
                    theme::primary
                } else {
                    theme::secondary
                })
                .on_press_maybe(
                    (!state.saving).then(|| msg(Action::DeleteTarget(category.id.clone()))),
                ),
            );
        }
    }
    content = content.push(text(&state.error).size(13).style(theme::danger));
    column![
        scrollable(content)
            .spacing(crate::calendar_ui::SCROLL_GAP)
            .height(Length::Fill),
        row![
            cancel,
            space().width(Length::Fill),
            button(if state.saving {
                "처리 중…"
            } else if count == 0 {
                "카테고리 삭제"
            } else {
                "이동 후 삭제"
            })
            .style(theme::primary)
            .on_press_maybe(
                (!state.saving && (count == 0 || state.delete_target.is_some()))
                    .then_some(msg(Action::ConfirmDelete))
            )
        ]
        .spacing(12)
    ]
    .spacing(18)
    .height(Length::Fill)
    .into()
}
