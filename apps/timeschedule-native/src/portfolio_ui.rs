use crate::{
    app::{App, Message, Page},
    views::{control, quiet},
    worker,
};
use iced::{
    widget::{button, checkbox, column, row, text, text_input},
    Element, Task,
};
use timeschedule_core::portfolio::{PortfolioExport, PortfolioPanel};

pub struct AnswerEdit {
    project: String,
    value: String,
    original: Option<String>,
}

#[derive(Clone, Debug)]
pub enum Action {
    Edit,
    Cancel,
    Input(String),
    Save,
    Defer,
    SelectMode(bool),
    Toggle(String, bool),
    Export,
    Open,
    Answered(String, Result<(), String>),
    Exported(String, Result<PortfolioExport, String>),
}

fn message(a: Action) -> Message {
    Message::Portfolio(a)
}

fn selection<'a>(
    app: &'a App,
    p: &'a PortfolioPanel,
) -> Option<&'a std::collections::BTreeSet<String>> {
    app.portfolio_selection
        .get(&p.project_id)
        .unwrap_or(&p.last_selection)
        .as_ref()
}

fn needs_answer(p: &PortfolioPanel, ids: Option<&std::collections::BTreeSet<String>>) -> bool {
    p.needs_answer
        && p.choices
            .iter()
            .any(|e| e.needs_answer && ids.is_none_or(|ids| ids.contains(&e.id)))
}

pub fn panel<'a>(app: &'a App, p: &'a PortfolioPanel) -> Element<'a, Message> {
    let mut section =
        column![text(format!("포트폴리오 경험 · {}건", p.experiences)).size(16)].spacing(6);
    let feedback = app
        .portfolio_feedback
        .as_ref()
        .filter(|(id, _)| id == &p.project_id)
        .map(|(_, status)| status.as_str());
    if let Some(file) = &p.last_file {
        section = section
            .push(control("마지막 Markdown 열기", message(Action::Open)))
            .push(quiet(app.data_dir.join(file).display().to_string()));
    }
    if p.experiences == 0 {
        return section
            .push(quiet(
                "이 프로젝트에 설계 경험, 개발 기록 또는 트러블슈팅을 저장하면 여기에 누적됩니다.",
            ))
            .into();
    }
    if app.busy {
        if let Some(status) = feedback {
            section = section.push(text(status));
        }
        return section.into();
    }
    let selected = selection(app, p);
    if let Some(ids) = selected {
        section = section.push(
            row![
                text(format!("선택한 경험 {}건", ids.len())),
                control("전체 경험 사용", message(Action::SelectMode(false)))
            ]
            .spacing(8),
        );
        let mut choices = column![].spacing(5);
        for e in &p.choices {
            let id = e.id.clone();
            choices = choices.push(
                checkbox(ids.contains(&id))
                    .label(format!("{} · {}", e.date, e.title))
                    .on_toggle(move |checked| message(Action::Toggle(id.clone(), checked))),
            );
        }
        section = section.push(choices);
        if ids.is_empty() {
            return section.into();
        }
    } else {
        section = section.push(control("경험 선택", message(Action::SelectMode(true))));
    }
    let input = app
        .portfolio_editor
        .as_ref()
        .filter(|edit| edit.project == p.project_id)
        .map(|edit| edit.value.as_str());
    if needs_answer(p, selected) || input.is_some() {
        let value = input.unwrap_or_default();
        let field = text_input("직접 맡은 역할", value).on_input(|s| message(Action::Input(s)));
        let save = button(text("답변 저장").size(13))
            .on_press_maybe((!value.trim().is_empty()).then_some(message(Action::Save)));
        section = section
            .push(quiet(
                "이 프로젝트에서 직접 맡은 역할은 무엇인가요? 보류하면 프로젝트 사실만 사용합니다.",
            ))
            .push(field)
            .push(row![save, control("개인 기여 보류", message(Action::Defer))].spacing(6));
        if input.is_some() && p.answer.is_some() {
            section = section.push(control("답변 수정 취소", message(Action::Cancel)));
        }
    } else {
        if let Some(role) = &p.answer {
            section = section
                .push(quiet(if role.is_empty() {
                    "개인 기여 보류".into()
                } else {
                    format!("확인한 프로젝트 역할 · {role}")
                }))
                .push(control("답변 수정", message(Action::Edit)));
        }
        let mut actions = row![control(
            "백엔드 포트폴리오 · Markdown 생성",
            message(Action::Export)
        )]
        .spacing(8)
        .align_y(iced::Alignment::Center);
        if let Some(status) = feedback {
            actions = actions.push(text(status).size(14));
        }
        section = section.push(quiet("선택한 경험과 역할 답변을 Codex Terra에 보내 작성합니다. 같은 입력은 편집한 판본을 유지합니다.")).push(actions);
    }
    section.into()
}

pub fn update(app: &mut App, a: Action) -> Task<Message> {
    match a {
        Action::Answered(id, result) => {
            app.busy = false;
            match result {
                Ok(()) => {
                    if app
                        .portfolio_editor
                        .as_ref()
                        .is_some_and(|edit| edit.project == id)
                    {
                        app.portfolio_editor = None;
                    }
                    app.notice = "역할 확인 결과를 저장했습니다.".into();
                    return app.refresh();
                }
                Err(e) => {
                    app.error = e;
                    return app.refresh();
                }
            }
        }
        Action::Exported(id, result) => {
            app.busy = false;
            let status = match result {
                Ok(outcome) => {
                    let (event, label) = if outcome.reused {
                        ("reused", format!("기존 v{} 재사용 완료", outcome.revision))
                    } else {
                        ("created", format!("새 v{} 생성 완료", outcome.revision))
                    };
                    app.log(&format!(
                        "portfolio export {event} project={id} revision={}",
                        outcome.revision
                    ));
                    label
                }
                Err(e) => {
                    app.log(&format!("portfolio export failed project={id} error={e}"));
                    format!("Markdown 생성 실패 · {e}")
                }
            };
            if app.project.as_ref() == Some(&id) {
                app.portfolio_feedback = Some((id, status));
                return app.refresh();
            }
        }
        action => {
            let Some(state) = app
                .snapshot
                .portfolio
                .as_ref()
                .filter(|s| app.project.as_ref() == Some(&s.project_id))
            else {
                return Task::none();
            };
            // A periodic refresh keeps the last snapshot for this project on screen.
            // Selection and export are validated by the store against current data.
            if app.page != Page::Projects || app.busy || app.editor.is_some() {
                return Task::none();
            }
            let id = state.project_id.clone();
            let Some(store) = app.store.clone() else {
                return Task::none();
            };
            if matches!(
                action,
                Action::SelectMode(_)
                    | Action::Toggle(..)
                    | Action::Edit
                    | Action::Save
                    | Action::Defer
            ) {
                app.portfolio_feedback = None;
            }
            match action {
                Action::SelectMode(subset) => {
                    app.portfolio_selection
                        .insert(id, subset.then(Default::default));
                }
                Action::Toggle(entry, checked) => {
                    if let Some(mut ids) = selection(app, state).cloned() {
                        if state.choices.iter().any(|e| e.id == entry) {
                            if checked {
                                ids.insert(entry);
                            } else {
                                ids.remove(&entry);
                            }
                            app.portfolio_selection.insert(id, Some(ids));
                        }
                    }
                }
                Action::Cancel => app.portfolio_editor = None,
                Action::Open => {
                    if let Some(file) = &state.last_file {
                        let path = app.data_dir.join(file);
                        return Task::perform(
                            async move {
                                tokio::task::spawn_blocking(move || {
                                    timeschedule_desktop::open_local_document(&path)
                                })
                                .await
                                .map_err(|e| e.to_string())?
                            },
                            Message::SourceDone,
                        );
                    }
                }
                _ if state.experiences == 0 => {}
                Action::Edit => {
                    app.portfolio_editor = Some(AnswerEdit {
                        project: id,
                        value: state.answer.clone().unwrap_or_default(),
                        original: state.answer.clone(),
                    })
                }
                Action::Input(value) => {
                    if let Some(edit) = app
                        .portfolio_editor
                        .as_mut()
                        .filter(|edit| edit.project == id)
                    {
                        edit.value = value;
                    } else {
                        app.portfolio_editor = Some(AnswerEdit {
                            project: id,
                            value,
                            original: state.answer.clone(),
                        });
                    }
                }
                Action::Save | Action::Defer => {
                    let edit = app
                        .portfolio_editor
                        .as_ref()
                        .filter(|edit| edit.project == id);
                    let answer = if matches!(action, Action::Defer) {
                        String::new()
                    } else {
                        let Some(edit) = edit.filter(|edit| !edit.value.trim().is_empty()) else {
                            return Task::none();
                        };
                        edit.value.clone()
                    };
                    let expected =
                        edit.map_or_else(|| state.answer.clone(), |edit| edit.original.clone());
                    let key = id.clone();
                    app.busy = true;
                    return worker::job(
                        store,
                        move |s| s.save_portfolio_answer(&id, expected.as_deref(), &answer),
                        move |r| message(Action::Answered(key.clone(), r)),
                    );
                }
                Action::Export if !needs_answer(state, selection(app, state)) => {
                    let selected = selection(app, state).cloned();
                    app.busy = true;
                    let root = app.data_dir.clone();
                    let writer = app.portfolio_writer;
                    app.portfolio_feedback = Some((id.clone(), "포트폴리오 확인 중…".into()));
                    app.log(&format!("portfolio export requested project={id}"));
                    let project = id.clone();
                    return worker::job(
                        store,
                        move |s| {
                            s.export_portfolio_with_outcome(&id, &root, selected.as_ref(), writer)
                        },
                        move |r| message(Action::Exported(project.clone(), r)),
                    );
                }
                _ => {}
            }
        }
    }
    Task::none()
}
