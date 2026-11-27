//! ‘프로젝트 기록 만들기’ at the top of a project's ‘포트폴리오’ tab. The first press only
//! reads what a run would send (`record project --dry-run`) and shows its size and the
//! days with dialogue and no daily record; the model is called only when ‘만들기’ is
//! pressed after that. The record is then shown in the diary at once.
//!
//! One project's record is checked or made at a time. Its state is its own, apart from
//! the app's `busy` (a run can take many minutes) and from the daily record jobs: the
//! two read and write different files.
use crate::{
    app::{App, Message},
    views::{control, quiet},
    worker,
};
use iced::{
    widget::{button, column, row, text},
    Alignment, Element, Task,
};
use std::path::Path;
use timeschedule_core::{
    daily_record::record::estimated_tokens,
    diary::Project,
    project_record::{self, Outcome},
    storage::Storage,
};

/// What a run would send, as `check` read it.
#[derive(Clone, Debug, PartialEq)]
pub struct Check {
    pub days: usize,
    pub records: usize,
    pub tokens: usize,
    pub model: String,
    /// Days with dialogue and no daily record, oldest first.
    pub missing: Vec<String>,
    pub warnings: Vec<String>,
    pub completed_on: Option<String>,
    /// The latest kept record was made from this input: a run would not call the model.
    pub current: bool,
    /// Why a run would not be sent at all.
    pub refused: Option<String>,
    /// The key of the folder the check read. ‘만들기’ makes the record of that folder
    /// only: a project moved to another folder since is checked again.
    pub folder: String,
}

#[derive(Clone, Debug, PartialEq)]
pub enum State {
    Checking,
    Ask(Check),
    Making,
    Done(String),
    Failed(String),
}

/// The record panel of one project.
#[derive(Clone, Debug, PartialEq)]
pub struct Panel {
    pub project: String,
    pub state: State,
}

impl Panel {
    /// Whether a job of this panel is running.
    pub fn running(&self) -> bool {
        matches!(self.state, State::Checking | State::Making)
    }
}

#[derive(Clone, Debug)]
pub enum Action {
    Check(String),
    /// ‘최종 기록 만들기’ on the info rows: the ‘포트폴리오’ tab, then `Check`.
    Final(String),
    Checked(String, Result<Check, String>),
    Make(String),
    Made(String, Result<String, String>),
    Cancel(String),
}

fn message(a: Action) -> Message {
    Message::ProjectRecord(a)
}

pub(crate) const NO_FOLDER: &str = "로컬 Git 저장소 경로를 넣으면 프로젝트 기록을 만들 수 있습니다";
pub(crate) const FINAL_ASK: &str = "프로젝트를 완료했습니다. 최종 프로젝트 기록을 만들어 두세요.";
pub(crate) const FINAL_BUTTON: &str = "최종 기록 만들기";
pub(crate) const IN_PROGRESS_HINT: &str = "진행 중에 만들면 ‘완료 전 종합’이 됩니다. 프로젝트가 끝나면 ‘완료로 표시’한 뒤 다시 만들어 최종본으로 쓰세요.";

/// The day `p` was completed, if it was.
fn completed_on<'a>(app: &'a App, p: &Project) -> Option<&'a str> {
    app.snapshot
        .project_meta
        .get(&p.id)
        .and_then(|m| m.completed_on.as_deref())
}

/// Whether the info rows of `p` ask for its final record: it is completed, a record can
/// be made for it, and no kept record was made for that completion day. Only once the
/// screen was read for `p`: right after switching to it, or after a record was made,
/// the record's day on hand is not its own yet.
pub(crate) fn wants_final(app: &App, p: &Project) -> bool {
    let Some(day) = completed_on(app, p) else {
        return false;
    };
    app.entries_project.as_ref() == Some(&p.id)
        && Path::new(&p.repo_path).is_absolute()
        && app
            .snapshot
            .record_completion
            .get(&p.id)
            .is_none_or(|made| made.as_deref() != Some(day))
}

/// `2026-11-13` as "11월 13일"; anything else as it is.
fn day(date: &str) -> String {
    match chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d") {
        Ok(d) => {
            use chrono::Datelike;
            format!("{}월 {}일", d.month(), d.day())
        }
        Err(_) => date.to_owned(),
    }
}

fn tokens(n: usize) -> String {
    if n < 10_000 {
        format!("{n}")
    } else {
        format!("{:.1}만", n as f64 / 10_000.0)
    }
}

/// The lines a check shows, top to bottom.
pub(crate) fn check_lines(c: &Check) -> Vec<String> {
    let mut lines = vec![format!(
        "하루 기록 {}일 · 그 밖의 기록 {}건 · 추정 입력 약 {} 토큰 · {}",
        c.days,
        c.records,
        tokens(c.tokens),
        c.model
    )];
    if !c.missing.is_empty() {
        let first: Vec<String> = c.missing.iter().take(5).map(|d| day(d)).collect();
        let rest = c.missing.len().saturating_sub(5);
        let more = if rest > 0 {
            format!(" 외 {rest}일")
        } else {
            String::new()
        };
        lines.push(format!(
            "하루 기록이 없는 대화 날 {}일: {}{more}",
            c.missing.len(),
            first.join(", ")
        ));
    }
    lines.extend(c.warnings.iter().take(3).cloned());
    if c.warnings.len() > 3 {
        lines.push(format!("그 밖의 경고 {}건", c.warnings.len() - 3));
    }
    lines.push(match &c.completed_on {
        None => "완료 전 종합으로 만듭니다".into(),
        Some(on) => format!("완료({}) 뒤 종합으로 만듭니다", day(on)),
    });
    if let Some(why) = &c.refused {
        lines.push(why.clone());
    } else if c.current {
        lines.push("지난 기록과 입력이 같아 모델을 부르지 않습니다".into());
    }
    lines
}

pub fn panel<'a>(app: &'a App, p: &'a Project) -> Element<'a, Message> {
    // A record is made for a project with an absolute folder only (`gather`).
    if !Path::new(&p.repo_path).is_absolute() {
        return quiet(NO_FOLDER);
    }
    let mine = app.project_record.as_ref().filter(|r| r.project == p.id);
    // Another project's record is being checked or made.
    let elsewhere = app
        .project_record
        .as_ref()
        .is_some_and(|r| r.project != p.id && r.running());
    let completed = completed_on(app, p).is_some();
    let start = || -> Element<'a, Message> {
        let label = if completed {
            "최종 프로젝트 기록 만들기"
        } else {
            "프로젝트 기록 만들기"
        };
        let button = control(label, message(Action::Check(p.id.clone()))).on_press_maybe(
            (!elsewhere && app.store.is_some()).then(|| message(Action::Check(p.id.clone()))),
        );
        if completed {
            button.into()
        } else {
            column![
                button,
                text(IN_PROGRESS_HINT).size(12).style(crate::theme::muted)
            ]
            .spacing(6)
            .into()
        }
    };
    match mine.map(|r| &r.state) {
        Some(State::Checking) => quiet("프로젝트 기록 입력 확인 중…"),
        Some(State::Making) => quiet("프로젝트 기록 만드는 중… (몇 분 걸릴 수 있습니다)"),
        Some(State::Ask(c)) => {
            let mut lines = column![].spacing(4);
            for line in check_lines(c) {
                lines = lines.push(text(line).size(13));
            }
            let mut actions = row![].spacing(6);
            if c.refused.is_none() && !c.current {
                actions = actions.push(
                    button(text("만들기").size(13))
                        .padding([7, 10])
                        .style(crate::theme::primary)
                        .on_press(message(Action::Make(p.id.clone()))),
                );
            }
            actions = actions.push(control("취소", message(Action::Cancel(p.id.clone()))));
            column![lines, actions].spacing(8).into()
        }
        Some(State::Done(said)) | Some(State::Failed(said)) => {
            column![text(said).size(13), start()].spacing(8).into()
        }
        None => row![start()].align_y(Alignment::Center).into(),
    }
}

pub fn update(app: &mut App, a: Action) -> Task<Message> {
    match a {
        // Only the input is checked: the model is called when ‘만들기’ is pressed there.
        Action::Final(id) => {
            if app.project.as_ref() != Some(&id) {
                return Task::none();
            }
            let tab = app.update(Message::ProjectTab(crate::app::ProjectTab::Portfolio));
            Task::batch([tab, update(app, Action::Check(id))])
        }
        Action::Check(id) => {
            if app.project_record.as_ref().is_some_and(Panel::running) {
                return Task::none();
            }
            let Some(store) = app.store.clone() else {
                return Task::none();
            };
            app.project_record = Some(Panel {
                project: id.clone(),
                state: State::Checking,
            });
            let data_dir = app.data_dir.clone();
            let project = id.clone();
            worker::job(
                store,
                move |s| check(s, &data_dir, &project),
                move |r| message(Action::Checked(id.clone(), r)),
            )
        }
        Action::Checked(id, result) => {
            if let Some(panel) = app
                .project_record
                .as_mut()
                .filter(|r| r.project == id && r.state == State::Checking)
            {
                panel.state = match result {
                    Ok(c) => State::Ask(c),
                    Err(e) => {
                        State::Failed(format!("프로젝트 기록 입력을 확인하지 못했습니다: {e}"))
                    }
                };
            }
            Task::none()
        }
        Action::Make(id) => {
            let folder = match app.project_record.as_ref() {
                Some(Panel {
                    project,
                    state: State::Ask(c),
                }) if *project == id => c.folder.clone(),
                _ => return Task::none(),
            };
            // Only the one app instance writes, and nothing new starts while it quits.
            let Some(store) = app.store.clone().filter(|_| app.may_start_writing()) else {
                return Task::none();
            };
            app.project_record = Some(Panel {
                project: id.clone(),
                state: State::Making,
            });
            let data_dir = app.data_dir.clone();
            let project = id.clone();
            worker::job(
                store,
                move |s| make(s, &data_dir, &project, &folder),
                move |r| message(Action::Made(id.clone(), r)),
            )
        }
        Action::Made(id, result) => {
            if let Some(panel) = app
                .project_record
                .as_mut()
                .filter(|r| r.project == id && r.state == State::Making)
            {
                panel.state = match result {
                    Ok(said) => State::Done(said),
                    Err(e) => State::Failed(format!("프로젝트 기록을 만들지 못했습니다: {e}")),
                };
            }
            // The diary may show the new entry.
            app.refresh()
        }
        Action::Cancel(id) => {
            if app
                .project_record
                .as_ref()
                .is_some_and(|r| r.project == id && matches!(r.state, State::Ask(_)))
            {
                app.project_record = None;
            }
            Task::none()
        }
    }
}

/// What a run for `project` would send. Nothing is written.
fn check(store: &Storage, data_dir: &Path, project: &str) -> Result<Check, String> {
    let vault = crate::record_original::originals_vault(data_dir)?;
    let dossier = project_record::gather(store, &vault, project)?;
    let model = project_record::MODEL;
    let tokens = estimated_tokens(dossier.chars());
    let refused = if dossier.citable() == 0 {
        Some("근거로 들 하루 기록 항목도 다른 기록도 없어 만들 수 없습니다.".to_owned())
    } else {
        project_record::check_size(dossier.chars()).err()
    };
    Ok(Check {
        days: dossier.days.len(),
        records: dossier.records.len(),
        tokens,
        model: model.into(),
        missing: dossier.missing_days.clone(),
        warnings: dossier.warnings.clone(),
        completed_on: dossier.completed_on.clone(),
        current: project_record::is_current(&vault, &dossier, model, project_record::EFFORT)?,
        refused,
        folder: dossier.repo_key.clone(),
    })
}

/// Makes the record of `project` from what its folder, the key `folder` the check read,
/// holds now, and shows it in the diary. The bundle is read again: the check may be
/// minutes old.
fn make(store: &Storage, data_dir: &Path, project: &str, folder: &str) -> Result<String, String> {
    let vault = crate::record_original::originals_vault(data_dir)?;
    make_in(store, &vault, project, folder)
}

/// `make` with the document store at `vault`.
fn make_in(store: &Storage, vault: &Path, project: &str, folder: &str) -> Result<String, String> {
    let dossier = project_record::gather(store, vault, project)?;
    if dossier.repo_key != folder {
        return Err(
            "프로젝트의 폴더가 입력을 확인한 때와 달라 만들지 않았습니다. 다시 눌러 확인해 주세요."
                .into(),
        );
    }
    let said = match crate::record_project::summarize_project(
        store,
        vault,
        &dossier,
        project_record::MODEL,
    )
    .map_err(|e| e.message)?
    {
        Outcome::Saved { record, .. } => {
            format!("프로젝트 기록을 만들었습니다(판 {}).", record.revision)
        }
        Outcome::Unchanged { revision, .. } => {
            format!("지난 기록(판 {revision})과 입력이 같아 모델을 부르지 않았습니다.")
        }
        Outcome::Busy { .. } => {
            return Err(
                "다른 실행이 이 프로젝트의 기록을 만들고 있습니다. 끝난 뒤 다시 눌러 주세요."
                    .into(),
            )
        }
    };
    let shown = match store.import_project_record(vault, &dossier.project_id) {
        Ok(report) if report.failed > 0 => {
            "다이어리에 반영하지 못했습니다. 다음 채팅 확인 때 다시 반영합니다.".to_owned()
        }
        Ok(report) if report.added > 0 => format!(
            "다이어리에 ‘{}’로 보입니다.",
            project_record::diary::entry_title(&dossier.title)
        ),
        // The owner may have renamed the entry: its title is not said.
        Ok(report) if report.updated > 0 => {
            "다이어리의 프로젝트 기록 항목을 새 판으로 바꿨습니다.".to_owned()
        }
        Ok(report) if report.kept > 0 => {
            "다이어리의 항목은 직접 고쳤거나 옮겼거나 지운 것이라 바꾸지 않았습니다.".to_owned()
        }
        Ok(_) => String::new(),
        Err(e) => format!("다이어리에 반영하지 못했습니다: {e}"),
    };
    Ok(if shown.is_empty() {
        said
    } else {
        format!("{said} {shown}")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn checked() -> Check {
        Check {
            days: 3,
            records: 71,
            tokens: 91_234,
            model: "gpt-6.2-astra".into(),
            missing: [
                "2026-11-13",
                "2026-11-14",
                "2026-11-15",
                "2026-11-16",
                "2026-11-17",
                "2026-11-18",
            ]
            .map(String::from)
            .to_vec(),
            warnings: vec![],
            completed_on: None,
            current: false,
            refused: None,
            folder: "c:/lsh/timeschedule".into(),
        }
    }

    #[test]
    fn a_project_moved_to_another_folder_since_the_check_is_not_made() {
        let dir = crate::record_inbox::tests::TestDir::new();
        let repo = dir.0.join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        let store = Storage::open(":memory:").unwrap();
        let id = store
            .save_project(timeschedule_core::diary::ProjectInput {
                id: None,
                title: "앱".into(),
                description: String::new(),
                repo_path: repo.to_string_lossy().into_owned(),
            })
            .unwrap()
            .id;
        let error = make_in(&store, &dir.0, &id, "c:/somewhere/else").unwrap_err();
        assert!(error.contains("입력을 확인한 때와 달라"), "{error}");
        assert!(!dir.0.join("project").exists(), "nothing is written");
    }
    #[test]
    fn a_check_says_the_size_the_empty_days_and_what_will_be_made() {
        assert_eq!(
            check_lines(&checked()),
            [
                "하루 기록 3일 · 그 밖의 기록 71건 · 추정 입력 약 9.1만 토큰 · gpt-6.2-astra",
                "하루 기록이 없는 대화 날 6일: 11월 13일, 11월 14일, 11월 15일, 11월 16일, 11월 17일 외 1일",
                "완료 전 종합으로 만듭니다",
            ]
        );
        let mut c = checked();
        c.missing.clear();
        c.completed_on = Some("2026-12-01".into());
        c.current = true;
        c.tokens = 4_200;
        assert_eq!(
            check_lines(&c),
            [
                "하루 기록 3일 · 그 밖의 기록 71건 · 추정 입력 약 4200 토큰 · gpt-6.2-astra",
                "완료(12월 1일) 뒤 종합으로 만듭니다",
                "지난 기록과 입력이 같아 모델을 부르지 않습니다",
            ]
        );
    }
}
