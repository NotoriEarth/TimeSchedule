use crate::{
    app::{App, Message, Page},
    views::quiet,
    worker,
};
use iced::{
    widget::{button, checkbox, column, pick_list, scrollable, text},
    Element, Task,
};
use timeschedule_core::chat_experiences::{ChatChoice, Kind, Review, Saved};

#[derive(Debug, Clone)]
pub enum Action {
    Choose(ChatChoice),
    Read(String, Result<Review, String>),
    Check(Kind, bool),
    Save,
    Finished(String, Result<Saved, String>),
}
#[derive(Default)]
pub struct State {
    review: Option<Review>,
    checked: Vec<Kind>,
}

fn msg(a: Action) -> Message {
    Message::ChatReview(a)
}
pub fn panel<'a>(app: &'a App, project: &str) -> Element<'a, Message> {
    let review = app
        .chat_review
        .review
        .as_ref()
        .filter(|r| r.project() == project);
    let mut panel = column![text("채팅에서 경험 검토 · Terra").size(16)].spacing(6);
    panel = panel.push(quiet(
        "채팅을 선택하면 해당 채팅의 발췌를 로그인한 Codex의 GPT-6.1 Terra에 보내 후보를 검토합니다.",
    ));
    // While a model call or a refresh runs, the review on screen stays, only unpressable.
    // Reading a chat again starts a new review, so its old candidates go.
    let waiting = app.busy || app.loading;
    let idle = |message: Message| (!waiting).then_some(message);
    if waiting {
        panel = panel.push(quiet("불러오는 중…"));
    } else {
        let selected = review.map(|r| r.chat().clone());
        panel = panel.push(
            pick_list(app.snapshot.reviewable_chats.as_slice(), selected, |chat| {
                msg(Action::Choose(chat))
            })
            .placeholder("수집된 채팅 선택"),
        );
    }
    if let Some(r) = review {
        let again = msg(Action::Choose(r.chat().clone()));
        panel = panel.push(
            crate::views::control("후보 다시 읽기", again.clone()).on_press_maybe(idle(again)),
        );
        if r.candidates().is_empty() {
            return panel
                .push(quiet(
                    "설계 판단이나 문제 해결 과정을 뒷받침하는 대화 근거가 부족합니다.",
                ))
                .into();
        }
        for c in r.candidates() {
            let kind = c.kind;
            let toggle = checkbox(app.chat_review.checked.contains(&kind)).label(format!(
                "{} · {}",
                kind.label(),
                c.title
            ));
            panel = panel
                .push(if waiting {
                    toggle
                } else {
                    toggle.on_toggle(move |v| msg(Action::Check(kind, v)))
                })
                .push(scrollable(text(&c.body).size(13)).height(135));
        }
        panel = panel.push(
            button(text("확인한 후보만 이력에 저장").size(13)).on_press_maybe(
                (!app.chat_review.checked.is_empty())
                    .then_some(msg(Action::Save))
                    .and_then(idle),
            ),
        );
    }
    panel.into()
}

pub fn update(app: &mut App, action: Action) -> Task<Message> {
    match action {
        Action::Read(project, result) => {
            app.busy = false;
            if app.project.as_ref() == Some(&project) {
                match result {
                    Ok(r) => {
                        app.notice = format!(
                            "모델 후보 {}건 · 인용문을 확인하고 저장할 항목을 선택하세요.",
                            r.candidates().len()
                        );
                        app.chat_review.review = Some(r);
                    }
                    Err(err) => {
                        app.notice.clear();
                        app.error = err;
                    }
                }
            }
        }
        Action::Finished(project, result) => {
            app.busy = false;
            if app.project.as_ref() == Some(&project) {
                match result {
                    Ok(saved) => {
                        app.chat_review.checked.clear();
                        app.notice = format!(
                            "새 경험 {}건 · 기존 경험 {}건 · 추가한 근거 {}건",
                            saved.added, saved.kept, saved.evidence_added
                        );
                    }
                    Err(err) => app.error = err,
                }
            }
            return app.refresh();
        }
        _ if app.page != Page::Projects || app.busy || app.loading || app.editor.is_some() => {}
        Action::Choose(chat) => {
            let (Some(project), Some(store)) = (app.project.clone(), app.store.clone()) else {
                return Task::none();
            };
            if !app.snapshot.reviewable_chats.contains(&chat) {
                return Task::none();
            }
            app.chat_review = State::default();
            app.busy = true;
            app.notice = "Terra가 선택한 채팅을 읽고 있습니다.".into();
            let id = project.clone();
            return worker::job(
                store,
                move |s| s.review_chat_with_model(&id, &chat.id),
                move |r| msg(Action::Read(project.clone(), r)),
            );
        }
        Action::Check(kind, on) => {
            if app.chat_review.review.as_ref().is_some_and(|r| {
                app.project.as_deref() == Some(r.project())
                    && r.candidates().iter().any(|c| c.kind == kind)
            }) {
                app.chat_review.checked.retain(|k| *k != kind);
                if on {
                    app.chat_review.checked.push(kind);
                }
            }
        }
        Action::Save => {
            let (Some(review), Some(store)) = (app.chat_review.review.clone(), app.store.clone())
            else {
                return Task::none();
            };
            if app.project.as_deref() != Some(review.project())
                || app.chat_review.checked.is_empty()
            {
                return Task::none();
            }
            let project = review.project().to_owned();
            let selected = app.chat_review.checked.clone();
            app.busy = true;
            return worker::job(
                store,
                move |s| s.confirm_chat_candidates(&review, &selected),
                move |r| msg(Action::Finished(project.clone(), r)),
            );
        }
    }
    Task::none()
}
