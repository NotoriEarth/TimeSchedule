use crate::{
    app::{App, Message, Page},
    views::{control, quiet},
    worker,
};
use iced::{
    widget::{button, column, row, text, text_input},
    Element, Task,
};
use std::path::PathBuf;
use timeschedule_core::portfolio::PortfolioPanel;

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
    Export,
    Open,
    Answered(String, Result<(), String>),
    Exported(Result<PathBuf, String>),
}

fn message(a: Action) -> Message {
    Message::Portfolio(a)
}

pub fn panel<'a>(app: &'a App, p: &'a PortfolioPanel) -> Element<'a, Message> {
    let mut section =
        column![text(format!("포트폴리오 경험 · {}건", p.experiences)).size(16)].spacing(6);
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
    if app.busy || app.loading {
        return section.into();
    }
    let input = app
        .portfolio_editor
        .as_ref()
        .filter(|edit| edit.project == p.project_id)
        .map(|edit| edit.value.as_str());
    if p.needs_answer || input.is_some() {
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
        section = section.push(control(
            "백엔드 포트폴리오 · Markdown 생성",
            message(Action::Export),
        ));
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
        Action::Exported(result) => {
            app.busy = false;
            match result {
                Ok(path) => {
                    app.notice = format!("Markdown 저장 완료 · {}", path.display());
                    return app.refresh();
                }
                Err(e) => app.error = e,
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
            if app.page != Page::Projects || app.busy || app.loading || app.editor.is_some() {
                return Task::none();
            }
            let id = state.project_id.clone();
            let Some(store) = app.store.clone() else {
                return Task::none();
            };
            match action {
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
                Action::Export if !state.needs_answer => {
                    app.busy = true;
                    let root = app.data_dir.clone();
                    return worker::job(
                        store,
                        move |s| s.export_portfolio(&id, &root),
                        |r| message(Action::Exported(r)),
                    );
                }
                _ => {}
            }
        }
    }
    Task::none()
}
