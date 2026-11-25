use crate::editor::{Form, Kind};
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock};
use timeschedule_core::{
    calendar::{CalendarDay, Category},
    diary::{EntrySummary, Project, SearchHit},
    evidence::{evidence_draft, Evidence},
    storage::{Note, Storage, Task},
};

#[derive(Clone, Debug)]
pub struct Scope {
    pub from: String,
    pub to: String,
    pub event_from: String,
    pub event_to: String,
    pub widget_from: String,
    pub widget_to: String,
    pub today: String,
    pub project: Option<String>,
}
#[derive(Clone, Debug, Default)]
pub struct Snapshot {
    pub days: Vec<CalendarDay>,
    pub widget_days: Vec<CalendarDay>,
    pub categories: Vec<Category>,
    pub projects: Vec<Project>,
    pub reviewable_chats: Vec<timeschedule_core::chat_experiences::ChatChoice>,
    pub chat_capture_project: Option<String>,
    pub portfolio: Option<timeschedule_core::portfolio::PortfolioPanel>,
    pub today: CalendarDay,
    pub tasks: Vec<Task>,
    pub notes: Vec<Note>,
    pub entries: Vec<EntrySummary>,
    /// Every record of each listed date, including ones not loaded yet.
    pub entry_totals: HashMap<String, u32>,
}
/// A page of list rows and the full count of each date they cover.
pub fn entry_page(
    store: &Storage,
    scope: &Scope,
    offset: u32,
) -> Result<(Vec<EntrySummary>, HashMap<String, u32>), String> {
    let rows = store.list_entries(&scope.from, &scope.to, scope.project.as_deref(), offset)?;
    // Rows come newest date first: the last row has the earliest date.
    let totals = match (rows.last(), rows.first()) {
        (Some(first), Some(last)) => {
            store.entry_day_totals(&first.date, &last.date, scope.project.as_deref())?
        }
        _ => HashMap::new(),
    };
    Ok((rows, totals))
}
/// Before records are shown: a past automatic record gains its begin time
/// and owned commit times from the begin receipt in the record state folder
/// this app trusts, and from Git, only when it is opened; a failure leaves it
/// unknown. One opening spends at most a short time in total, each Git call
/// bounded by what is left. Each record's progress is kept for the session:
/// commits Git did not confirm are not checked again, a record cut short
/// continues past them when next opened, and a finished one costs only a
/// database read.
pub fn complete_evidence(store: &Storage, data_dir: &Path, ids: &[String]) {
    use timeschedule_core::timeline::BackfillProgress;
    static PROGRESS: OnceLock<Mutex<HashMap<String, BackfillProgress>>> = OnceLock::new();
    const BUDGET: std::time::Duration = std::time::Duration::from_secs(2);
    if data_dir.as_os_str().is_empty() || ids.is_empty() {
        return;
    }
    let Ok(state_dir) = crate::record_sync::registered_state_dir(data_dir) else {
        return;
    };
    let all = PROGRESS.get_or_init(Default::default);
    let deadline = std::time::Instant::now() + BUDGET;
    for id in ids {
        if std::time::Instant::now() >= deadline {
            break;
        }
        let mut progress = all
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(id)
            .cloned()
            .unwrap_or_default();
        if store
            .backfill_automatic_evidence(id, &state_dir, &mut progress, deadline)
            .is_ok()
        {
            all.lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert(id.clone(), progress);
        }
    }
}
pub fn snapshot(store: &Storage, scope: &Scope) -> Result<Snapshot, String> {
    let days = store.calendar_days(&scope.event_from, &scope.event_to)?;
    let widget_days = if scope.event_from == scope.widget_from && scope.event_to == scope.widget_to
    {
        days.clone()
    } else {
        store.calendar_days(&scope.widget_from, &scope.widget_to)?
    };
    let today = if let Some(day) = days
        .iter()
        .chain(&widget_days)
        .find(|d| d.date == scope.today)
    {
        day.clone()
    } else if (scope.today >= scope.event_from && scope.today <= scope.event_to)
        || (scope.today >= scope.widget_from && scope.today <= scope.widget_to)
    {
        CalendarDay {
            date: scope.today.clone(),
            ..Default::default()
        }
    } else {
        store
            .calendar_days(&scope.today, &scope.today)?
            .pop()
            .unwrap_or_default()
    };
    let (entries, entry_totals) = entry_page(store, scope, 0)?;
    Ok(Snapshot {
        days,
        widget_days,
        today,
        categories: store.list_categories()?,
        projects: store.list_projects()?,
        reviewable_chats: scope
            .project
            .as_deref()
            .map(|p| store.reviewable_chats(p))
            .transpose()?
            .unwrap_or_default(),
        chat_capture_project: match &scope.project {
            Some(id) if store.chat_capture_enabled(id)? => Some(id.clone()),
            _ => None,
        },
        portfolio: scope
            .project
            .as_deref()
            .map(|id| store.portfolio_panel(id))
            .transpose()?,
        tasks: store.list_tasks()?,
        notes: store.list_note_headers()?,
        entries,
        entry_totals,
    })
}
pub fn hydrate_form(store: &Storage, mut form: Form) -> Result<Form, String> {
    if let Some(id) = form.id.clone() {
        match form.kind {
            Kind::Entry => {
                let value = store.calendar_entry(&id)?;
                form = Form::entry(value.entry.clone());
                form.category_id = value.display.category_id.clone();
                form.short_title = value.display.short_title.clone();
                form.time = value.timeline.manual_time.clone().unwrap_or_default();
                form.entry_version = Some(value);
            }
            Kind::Task => {
                let value = store.task_edit(&id)?;
                form = Form::task(value.task.clone());
                form.body = value.details.body.clone();
                form.all_day = value.details.all_day;
                form.start = value
                    .details
                    .start_time
                    .clone()
                    .unwrap_or_else(|| "09:00".into());
                form.end = value
                    .details
                    .end_time
                    .clone()
                    .unwrap_or_else(|| "10:00".into());
                form.task_version = Some(value);
            }
            Kind::Project => form.project_short_name = store.project_short_name(&id)?,
            Kind::Event => form = Form::event(store.get_event(&id)?),
            Kind::Note => {}
        }
    }
    form.hydrated = true;
    Ok(form)
}
pub fn fetch_form(store: &Storage, hit: &SearchHit) -> Result<Form, String> {
    match hit.kind.as_str() {
        "entry" => store.get_entry(&hit.id).map(Form::entry),
        "note" => store.get_note(&hit.id).map(Form::note),
        "task" => store
            .list_tasks()?
            .into_iter()
            .find(|x| x.id == hit.id)
            .map(Form::task)
            .ok_or("할 일을 찾을 수 없습니다.".into()),
        "project" => store
            .list_projects()?
            .into_iter()
            .find(|x| x.id == hit.id)
            .map(Form::project)
            .ok_or("프로젝트를 찾을 수 없습니다.".into()),
        "event" => {
            let date = hit.date.as_deref().ok_or("일정 날짜가 없습니다.")?;
            store
                .list_events(date, date)?
                .into_iter()
                .find(|x| x.id == hit.id)
                .map(Form::event)
                .ok_or("일정을 찾을 수 없습니다.".into())
        }
        _ => Err("지원되지 않는 검색 결과입니다.".into()),
    }
}
pub fn draft(
    store: &Storage,
    scope: &Scope,
    selected: String,
    project_mode: bool,
    portfolio: bool,
) -> Result<Form, String> {
    let summaries = store.list_entries(&scope.from, &scope.to, scope.project.as_deref(), 0)?;
    let eligible: Vec<_> = summaries
        .iter()
        .filter(|e| e.kind != "summary" && e.kind != "portfolio")
        .collect();
    let mut records = Vec::new();
    for e in eligible.iter().take(100) {
        let full = store.get_entry(&e.id)?;
        records.push(Evidence {
            id: full.id,
            title: full.title,
            date: full.date,
            body: full.body,
            source_url: full.source_url,
            source_ref: full.source_ref,
        });
    }
    let project = if project_mode {
        store
            .list_projects()?
            .into_iter()
            .find(|p| Some(&p.id) == scope.project.as_ref())
    } else {
        None
    };
    if !project_mode {
        for e in store.list_events(&scope.from, &scope.to)? {
            records.push(Evidence {
                id: e.id,
                title: e.title,
                date: e.date,
                body: e.notes,
                source_url: String::new(),
                source_ref: None,
            });
        }
        for t in store.list_tasks()?.into_iter().filter(|t| {
            t.date
                .as_ref()
                .is_some_and(|d| d >= &scope.from && d <= &scope.to)
        }) {
            records.push(Evidence {
                id: t.id,
                title: t.title,
                date: t.date.unwrap(),
                body: if t.completed {
                    "완료 표시됨"
                } else {
                    "미완료"
                }
                .into(),
                source_url: String::new(),
                source_ref: None,
            });
        }
    }
    let scope_title = if project_mode {
        project
            .as_ref()
            .map(|p| p.title.clone())
            .unwrap_or_else(|| "전체 프로젝트".into())
    } else {
        format!("{} ~ {}", scope.from, scope.to)
    };
    let truncated = summaries.len() == 200 || eligible.len() > 100 || records.len() > 100;
    let mut body = evidence_draft(
        &scope_title,
        &records.into_iter().take(100).collect::<Vec<_>>(),
        portfolio,
        truncated,
    );
    if let Some(p) = project {
        if !p.description.is_empty() {
            body = format!("프로젝트 설명\n{}\n\n{body}", p.description);
        }
    }
    if body.chars().count() > 99_000 {
        body = body.chars().take(99_000).collect::<String>()
            + "\n[길이 제한으로 발췌되었습니다. 원문을 확인하세요.]";
    }
    let mut form = Form::new(Kind::Entry, selected, scope.project.clone());
    form.entry_kind = if portfolio { "portfolio" } else { "summary" }.into();
    form.title = format!(
        "{scope_title} · {}",
        if portfolio {
            "포트폴리오 초안"
        } else {
            "근거 요약"
        }
    )
    .chars()
    .take(200)
    .collect();
    form.body = body;
    Ok(form)
}
pub(crate) fn codex_home() -> Result<std::path::PathBuf, String> {
    std::env::var_os("CODEX_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| {
            std::env::var_os("USERPROFILE").map(|p| std::path::PathBuf::from(p).join(".codex"))
        })
        .ok_or_else(|| "Codex 사용자 폴더를 찾을 수 없습니다.".into())
}
/// What one chat scan did: the diary excerpts, the originals kept in the document store,
/// and the daily records shown in the diary. Each part has its own result, so one that
/// could not start does not hide what the others did.
#[derive(Debug, Clone)]
pub struct ChatScan {
    pub excerpts: Result<timeschedule_core::chat_history::ScanReport, String>,
    pub originals: Result<timeschedule_core::chat_originals::OriginalsReport, String>,
    pub daily: Result<timeschedule_core::daily_record::diary::DiaryReport, String>,
}

impl ChatScan {
    /// A part that could not start, or excerpt files, originals or daily records that
    /// could not be read, kept or shown.
    pub fn needs_attention(&self) -> bool {
        self.excerpts.as_ref().map_or(true, |r| r.failed > 0)
            || self.originals.as_ref().map_or(true, |o| o.failed > 0)
            || self.daily.as_ref().map_or(true, |d| d.failed > 0)
    }

    /// What went wrong in each part, for the app's error line. One cause that stopped
    /// several parts, such as a document store that cannot be found, is said once.
    pub fn problems(&self) -> Vec<String> {
        fn problem<T>(
            part: &Result<T, String>,
            failed: impl Fn(&T) -> usize,
            say: impl Fn(usize) -> String,
        ) -> Option<String> {
            match part {
                Ok(report) => Some(failed(report)).filter(|n| *n > 0).map(say),
                Err(why) => Some(why.clone()),
            }
        }
        let parts = [
            problem(
                &self.originals,
                |o| o.failed,
                |n| format!("채팅 원문 {n}개를 보관하지 못했습니다. 다음 확인 때 재시도합니다."),
            ),
            problem(
                &self.daily,
                |d| d.failed,
                |n| {
                    format!("하루 기록 {n}개를 다이어리에 반영하지 못했습니다. 다음 확인 때 재시도합니다.")
                },
            ),
            problem(
                &self.excerpts,
                |r| r.failed,
                |n| format!("로컬 채팅 파일 {n}개를 읽지 못했습니다. 다음 확인 때 재시도합니다."),
            ),
        ];
        let mut problems = Vec::new();
        for problem in parts.into_iter().flatten() {
            if !problems.contains(&problem) {
                problems.push(problem);
            }
        }
        problems
    }

    /// What the scan put in the diary. Empty when the diary did not change.
    pub fn notices(&self) -> Vec<String> {
        let mut notices = Vec::new();
        let saved = self.excerpts.as_ref().map_or(0, |r| r.saved);
        if saved > 0 {
            notices.push(format!(
                "로컬 채팅 {saved}건을 프로젝트 이력에 반영했습니다."
            ));
        }
        let shown = self.daily.as_ref().map_or(0, |d| d.added + d.updated);
        if shown > 0 {
            notices.push(format!("하루 기록 {shown}건을 다이어리에 반영했습니다."));
        }
        notices
    }
}

pub fn scan_chats(store: &Storage, data_dir: &std::path::Path, reread: bool) -> ChatScan {
    let claude_home = claude_home();
    scan_from(store, codex_home(), claude_home.as_deref(), reread, || {
        crate::record_original::originals_vault(data_dir)
    })
}

/// The scan with its places given: the Codex home or why there is none, the Claude Code
/// home, and how to find the document store. The store is looked for only when a
/// project's chats are captured.
fn scan_from(
    store: &Storage,
    codex: Result<std::path::PathBuf, String>,
    claude: Option<&std::path::Path>,
    reread: bool,
    vault: impl FnOnce() -> Result<std::path::PathBuf, String>,
) -> ChatScan {
    // Keeping originals, showing daily records and the excerpt scan do not stop each other.
    let projects = store.originals_projects();
    let captured = projects
        .as_ref()
        .map_or(true, |projects| projects.iter().any(|p| p.capture));
    let vault = captured.then(vault);
    let originals = (|| {
        let projects = projects?;
        let Some(vault) = &vault else {
            return Ok(Default::default());
        };
        let homes = timeschedule_core::chat_originals::ToolHomes {
            codex: codex.as_ref()?,
            claude,
        };
        timeschedule_core::chat_originals::archive(&projects, &homes, vault.as_ref()?)
    })();
    // Records the record command made since the last scan become diary entries. They
    // are read from the document store, so they need no home folder of an AI app.
    let daily = match &vault {
        Some(vault) => vault
            .as_ref()
            .map_err(String::clone)
            .and_then(|vault| store.import_daily_records(vault)),
        None => Ok(Default::default()),
    };
    let excerpts = codex
        .as_ref()
        .map_err(String::clone)
        .and_then(|home| store.scan_local_chats(home, claude, reread));
    ChatScan {
        excerpts,
        originals,
        daily,
    }
}

/// A day of a project and the input its record would be made from. The automatic mode
/// does not ask the model again for an input that failed after asking it.
pub type DailyInput = (String, chrono::NaiveDate, String);

/// What one daily record job did, over the captured projects.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct DailyMade {
    /// The days a new record was kept for, one per project and day.
    pub saved: Vec<chrono::NaiveDate>,
    /// A dry run: the days that would ask the model, oldest first.
    pub due: Vec<chrono::NaiveDate>,
    /// The folder (its key) a `Which::Past` job listed its days in.
    pub root: Option<String>,
    /// Days whose record was already made from the same input: no model was asked.
    pub unchanged: usize,
    /// Days without dialogue, or of a project with no kept chats: nothing to make.
    pub empty: usize,
    /// Days another run was making at the time.
    pub busy: usize,
    /// Days left alone because their input failed before.
    pub skipped: usize,
    /// Projects whose originals this job could not all keep, by title. None of their
    /// days was made: the kept originals may be older than the chats.
    pub unkept: Vec<String>,
    /// The job stopped before its last day: the way of making changed, or a project's
    /// capture was turned off.
    pub stopped: bool,
    /// Why a day could not be made, one line each.
    pub failed: Vec<String>,
    /// The inputs the model was asked about and that failed.
    pub failed_inputs: Vec<DailyInput>,
    /// Diary entries the new records became or replaced.
    pub shown: usize,
    /// Chat files that could not be read before it was known whose they are.
    pub unplaced: usize,
}

/// The finished days the automatic mode looks at: the seven before `today`, oldest
/// first. Today is still going on, so it is not one of them. `today` is the date in
/// Korea, as the day transcript counts days.
pub fn finished_days(today: chrono::NaiveDate) -> Vec<chrono::NaiveDate> {
    (1..=7)
        .rev()
        .map(|back| today - chrono::Days::new(back))
        .collect()
}

/// What a job did with one day.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Made {
    /// The model was asked and a new record was kept.
    Saved,
    /// The latest record was made from the same input: the model was not asked.
    Unchanged,
    /// Another run is making this day.
    Busy,
    /// A dry run: this day would ask the model.
    Due,
}

impl From<timeschedule_core::daily_record::record::Outcome> for Made {
    fn from(outcome: timeschedule_core::daily_record::record::Outcome) -> Self {
        use timeschedule_core::daily_record::record::Outcome;
        match outcome {
            Outcome::Saved { .. } => Self::Saved,
            Outcome::Unchanged { .. } => Self::Unchanged,
            Outcome::Busy { .. } => Self::Busy,
        }
    }
}

/// The days a daily record job looks at.
#[derive(Debug, Clone, Copy)]
pub enum Which<'a> {
    /// Every captured project, these days: the selected day's button and the automatic mode.
    Days(&'a [chrono::NaiveDate]),
    /// One project (its ID), every day before `before` on which its kept sessions have rows.
    Past {
        project: &'a str,
        before: chrono::NaiveDate,
    },
    /// One project (its ID), these days: the days a check of the past found due in the
    /// folder `root` (`DailyMade::root` of that check). A project whose folder is another
    /// one by now makes nothing: the days were counted in the old folder's chats.
    ProjectDays {
        project: &'a str,
        days: &'a [chrono::NaiveDate],
        root: &'a str,
    },
}

/// How a job asks the model and decides what to leave alone.
pub struct DailyRun<'a, M, K, G>
where
    M: Fn(
        &std::path::Path,
        &timeschedule_core::daily_record::DayTranscript,
    ) -> Result<Made, crate::record_cli::DailyError>,
    K: Fn(&timeschedule_core::daily_record::DayTranscript) -> String,
    G: Fn() -> bool,
{
    /// Makes the record of a transcript.
    pub make: M,
    /// The input a transcript would be sent as (`DailyInput`'s last part).
    pub key: K,
    /// Inputs not to send again.
    pub skip: &'a std::collections::HashSet<DailyInput>,
    /// Whether the job may go on to its next day.
    pub keep_going: G,
}

/// Keeps the newest originals of the captured projects, as the chat scan does.
pub fn keep_originals(
    projects: &[timeschedule_core::chat_originals::OriginalsProject],
    vault: &std::path::Path,
) -> Result<timeschedule_core::chat_originals::OriginalsReport, String> {
    let codex = codex_home()?;
    let claude = claude_home();
    timeschedule_core::chat_originals::archive(
        projects,
        &timeschedule_core::chat_originals::ToolHomes {
            codex: &codex,
            claude: claude.as_deref(),
        },
        vault,
    )
}

/// One daily record job, in order: keep the newest originals of the projects whose
/// chats are captured, make the record of each day `which` names from them, and show the
/// new records in the diary. The projects are listed once, so the job keeps and makes the
/// same ones. A project whose originals were not all kept is left for a later job; a
/// project with no kept chats and a day without dialogue have nothing to make; a day
/// whose record was made from the same input is not sent again (`summarize` checks the
/// fingerprint). Before each day the job checks that it may go on and that the
/// project's chats are still captured. What was done stays in the result when a later
/// step fails.
///
/// `Which::Past` takes its days from the kept originals once they are kept (`session_days`),
/// so the days are those of the originals the job then reads. A project that `Past` or
/// `ProjectDays` names and that is unknown or not captured makes nothing and is said in
/// `failed`. A dry `run.make` that answers `Made::Due` makes no record: the day goes to
/// `due`.
pub fn daily_job<M, K, G>(
    store: &Storage,
    vault: impl FnOnce() -> Result<std::path::PathBuf, String>,
    keep: impl FnOnce(
        &[timeschedule_core::chat_originals::OriginalsProject],
        &std::path::Path,
    ) -> Result<timeschedule_core::chat_originals::OriginalsReport, String>,
    which: Which<'_>,
    run: DailyRun<'_, M, K, G>,
) -> Result<DailyMade, String>
where
    M: Fn(
        &std::path::Path,
        &timeschedule_core::daily_record::DayTranscript,
    ) -> Result<Made, crate::record_cli::DailyError>,
    K: Fn(&timeschedule_core::daily_record::DayTranscript) -> String,
    G: Fn() -> bool,
{
    use timeschedule_core::{
        chat_originals::group_name,
        daily_record::{day_transcript, session_days},
    };
    let all = store.originals_projects()?;
    let mut made = DailyMade::default();
    let named = match which {
        Which::Days(_) => None,
        Which::Past { project, .. } | Which::ProjectDays { project, .. } => Some(project),
    };
    let projects: Vec<_> = match named {
        None => all.iter().filter(|p| p.capture).collect(),
        Some(id) => match all.iter().find(|p| p.id == id) {
            Some(p) if p.capture => {
                match which {
                    // The days are those of this folder's chats and of no other.
                    Which::ProjectDays { root, .. } if p.root != root => {
                        made.failed.push(format!(
                            "{}: 프로젝트의 폴더가 지난 대화를 확인한 때와 달라 만들지 않았습니다. 다시 확인해 주세요.",
                            p.title
                        ));
                        return Ok(made);
                    }
                    Which::Past { .. } => made.root = Some(p.root.clone()),
                    _ => {}
                }
                vec![p]
            }
            Some(p) => {
                made.failed.push(format!(
                    "{}: 채팅 자동 수집이 꺼져 있어 하루 정리를 만들지 않았습니다.",
                    p.title
                ));
                return Ok(made);
            }
            None => {
                made.failed
                    .push("알 수 없는 프로젝트라 하루 정리를 만들지 않았습니다.".into());
                return Ok(made);
            }
        },
    };
    if projects.is_empty() {
        return Ok(made);
    }
    let folders: std::collections::HashMap<String, String> = store
        .list_projects()?
        .into_iter()
        .map(|p| (p.id, p.repo_path))
        .collect();
    let vault = vault()?;
    // Every project is given, captured or not: one that is not still owns its folder.
    let kept = match keep(&all, &vault) {
        Ok(kept) => kept,
        Err(e) => {
            // Nothing is made from originals that could not be kept; records an
            // earlier job kept but could not show are still shown.
            made.failed.push(format!("원문 보관 실패: {e}"));
            made.unkept = projects.iter().map(|p| p.title.clone()).collect();
            show(store, &vault, &mut made);
            return Ok(made);
        }
    };
    made.unplaced = kept.unplaced;
    'projects: for project in projects {
        let Some(folder) = folders.get(&project.id) else {
            continue;
        };
        if kept.incomplete.contains(&project.id) {
            made.unkept.push(project.title.clone());
            continue;
        }
        let originals = vault.join("originals").join(group_name(&project.root));
        match std::fs::symlink_metadata(&originals) {
            Ok(_) => {}
            // Capture is on but no chat was found yet: nothing to make.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                made.empty += match which {
                    Which::Days(days) | Which::ProjectDays { days, .. } => days.len(),
                    Which::Past { .. } => 0,
                };
                continue;
            }
            Err(e) => {
                made.failed.push(format!(
                    "{}: 보관 폴더를 확인하지 못했습니다 ({e})",
                    project.title
                ));
                continue;
            }
        }
        let dates: Vec<chrono::NaiveDate> = match which {
            Which::Days(days) | Which::ProjectDays { days, .. } => days.to_vec(),
            Which::Past { before, .. } => {
                if !(run.keep_going)() {
                    made.stopped = true;
                    break 'projects;
                }
                match session_days(&vault, folder, &run.keep_going) {
                    Ok(listed) => {
                        if listed.stopped {
                            made.stopped = true;
                            break 'projects;
                        }
                        made.failed.extend(
                            listed
                                .unread
                                .iter()
                                .map(|line| format!("{}: {line}", project.title)),
                        );
                        listed
                            .days
                            .into_iter()
                            .filter(|day| *day < before)
                            .collect()
                    }
                    Err(e) => {
                        made.failed.push(format!("{}: {e}", project.title));
                        continue;
                    }
                }
            }
        };
        for date in &dates {
            // Still captured, and for the same folder: after a move the records of the
            // old folder would not be shown.
            let captured = store.originals_projects().map(|now| {
                now.iter()
                    .any(|p| p.capture && p.id == project.id && p.root == project.root)
            });
            match captured {
                Ok(true) if (run.keep_going)() => {}
                Ok(_) => {
                    made.stopped = true;
                    break 'projects;
                }
                Err(e) => {
                    made.failed.push(format!("{}: {e}", project.title));
                    made.stopped = true;
                    break 'projects;
                }
            }
            let transcript = match day_transcript(&vault, folder, *date) {
                Ok(transcript) => transcript,
                Err(e) => {
                    made.failed.push(format!("{} {date}: {e}", project.title));
                    continue;
                }
            };
            // A folder moved from another project keeps that project's ID: the diary
            // would refuse the record after the model was asked.
            if transcript.project_id != project.id {
                made.failed.push(format!(
                    "{}: 보관 폴더가 다른 프로젝트의 것이라 남은 하루 정리를 만들지 않았습니다.",
                    project.title
                ));
                continue 'projects;
            }
            if transcript.blocks.is_empty() {
                // Warnings with no dialogue mean files that could not be read, not a
                // day without dialogue.
                match transcript.warnings.first() {
                    Some(warning) => made.failed.push(format!(
                        "{} {date}: 원문을 읽지 못했습니다 ({warning})",
                        project.title
                    )),
                    None => made.empty += 1,
                }
                continue;
            }
            let input = (project.id.clone(), *date, (run.key)(&transcript));
            if run.skip.contains(&input) {
                made.skipped += 1;
                continue;
            }
            match (run.make)(&vault, &transcript) {
                Ok(Made::Saved) => made.saved.push(*date),
                Ok(Made::Unchanged) => made.unchanged += 1,
                Ok(Made::Busy) => made.busy += 1,
                Ok(Made::Due) => made.due.push(*date),
                Err(e) => {
                    made.failed
                        .push(format!("{} {date}: {}", project.title, e.message));
                    if e.asked {
                        made.failed_inputs.push(input);
                    }
                }
            }
        }
    }
    // Also when nothing new was made: a record kept by an earlier job whose diary
    // step failed is shown now.
    show(store, &vault, &mut made);
    Ok(made)
}

/// Shows the records kept in `vault` in the diary, and says in `made` what came of it.
fn show(store: &Storage, vault: &std::path::Path, made: &mut DailyMade) {
    match store.import_daily_records(vault) {
        Ok(report) => {
            made.shown = report.added + report.updated;
            if report.failed > 0 {
                made.failed.push(format!(
                    "하루 기록 {}개를 다이어리에 반영하지 못했습니다.",
                    report.failed
                ));
            }
        }
        Err(e) => made.failed.push(format!("다이어리 반영 실패: {e}")),
    }
}

fn claude_home() -> Option<std::path::PathBuf> {
    std::env::var_os("CLAUDE_CONFIG_DIR")
        .map(std::path::PathBuf::from)
        .or_else(|| {
            std::env::var_os("USERPROFILE").map(|p| std::path::PathBuf::from(p).join(".claude"))
        })
}

pub fn job<T: Send + 'static, M: Send + 'static>(
    store: Arc<Storage>,
    work: impl FnOnce(&Storage) -> Result<T, String> + Send + 'static,
    map: impl Fn(Result<T, String>) -> M + Send + 'static,
) -> iced::Task<M> {
    iced::Task::perform(
        async move {
            tokio::task::spawn_blocking(move || work(&store))
                .await
                .map_err(|e| format!("작업 실행 실패: {e}"))?
        },
        map,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    fn scope() -> Scope {
        Scope {
            from: "2026-10-01".into(),
            to: "2026-10-31".into(),
            event_from: "2026-10-01".into(),
            event_to: "2026-10-31".into(),
            widget_from: "2026-10-01".into(),
            widget_to: "2026-10-31".into(),
            today: "2026-10-29".into(),
            project: None,
        }
    }
    #[test]
    fn all_five_forms_round_trip_search_and_delete_without_losing_linked_records() {
        let store = Storage::open(":memory:").unwrap();
        let mut project = Form::new(Kind::Project, String::new(), None);
        project.title = "한글 프로젝트".into();
        project.body = "목적과 역할".into();
        project.save(&store).unwrap();
        let saved_project = store.list_projects().unwrap().remove(0);
        let mut entry = Form::new(
            Kind::Entry,
            "2026-10-29".into(),
            Some(saved_project.id.clone()),
        );
        entry.title = "한글 개발 기록".into();
        entry.body = "실제 원문\n둘째 줄".into();
        entry.source_url = "https://example.com/evidence".into();
        entry.save(&store).unwrap();
        let mut event = Form::new(Kind::Event, "2026-10-29".into(), None);
        event.title = "한글 일정".into();
        event.body = "회의 메모".into();
        event.all_day = false;
        event.start = "09:30".into();
        event.end = "10:00".into();
        event.color = "rose".into();
        event.save(&store).unwrap();
        let mut note = Form::new(Kind::Note, String::new(), None);
        note.title = "한글 메모".into();
        note.body = "노트 본문".into();
        note.save(&store).unwrap();
        let mut task = Form::new(Kind::Task, "2026-10-29".into(), None);
        task.title = "한글 할 일".into();
        task.completed = true;
        task.save(&store).unwrap();
        let all = store.search_diary("한글", 50).unwrap();
        assert_eq!(all.len(), 5);
        for kind in ["project", "entry", "event", "note", "task"] {
            assert!(all.iter().any(|h| h.kind == kind));
        }
        let stored_entry = store
            .get_entry(
                &store
                    .list_entries("2026-10-01", "2026-10-31", None, 0)
                    .unwrap()[0]
                    .id,
            )
            .unwrap();
        let entry_id = stored_entry.id.clone();
        let mut edited = Form::entry(stored_entry);
        edited.title = "수정된 기록".into();
        edited.save(&store).unwrap();
        assert_eq!(
            store.get_entry(&entry_id).unwrap().body,
            "실제 원문\n둘째 줄"
        );
        assert_eq!(
            store.get_entry(&entry_id).unwrap().source_url,
            "https://example.com/evidence"
        );
        Form::project(saved_project).delete(&store).unwrap();
        assert_eq!(store.get_entry(&entry_id).unwrap().project_id, None);
        Form::entry(store.get_entry(&entry_id).unwrap())
            .delete(&store)
            .unwrap();
        let saved_event = store
            .list_events("2026-10-29", "2026-10-29")
            .unwrap()
            .remove(0);
        assert_eq!(saved_event.start_time.as_deref(), Some("09:30"));
        assert_eq!(saved_event.color, "rose");
        Form::event(saved_event).delete(&store).unwrap();
        Form::note(store.list_notes().unwrap().remove(0))
            .delete(&store)
            .unwrap();
        Form::task(store.list_tasks().unwrap().remove(0))
            .delete(&store)
            .unwrap();
        assert!(store.search_diary("한글", 50).unwrap().is_empty());
    }
    #[test]
    fn snapshot_uses_lazy_headers_and_evidence_combines_week_sources() {
        let store = Storage::open(":memory:").unwrap();
        let mut entry = Form::new(Kind::Entry, "2026-10-29".into(), None);
        entry.title = "개발 근거".into();
        entry.body = "실패를 재현한 로그".into();
        entry.save(&store).unwrap();
        let mut summary = entry.clone();
        summary.title = "기존 초안".into();
        summary.entry_kind = "summary".into();
        summary.body = "RECURSIVE-DRAFT-MUST-NOT-APPEAR".into();
        summary.save(&store).unwrap();
        let mut event = Form::new(Kind::Event, "2026-10-29".into(), None);
        event.title = "검증 일정".into();
        event.body = "일정 원문".into();
        event.save(&store).unwrap();
        let mut note = Form::new(Kind::Note, String::new(), None);
        note.title = "메모".into();
        note.body = "본문은 지연 읽기".into();
        note.save(&store).unwrap();
        let mut task = Form::new(Kind::Task, "2026-10-29".into(), None);
        task.title = "주간 할 일".into();
        task.completed = true;
        task.save(&store).unwrap();
        let snapshot = snapshot(&store, &scope()).unwrap();
        assert!(snapshot
            .days
            .iter()
            .flat_map(|d| &d.items)
            .any(|i| i.title == "검증 일정"));
        assert_eq!(snapshot.days[0].total, snapshot.widget_days[0].total);
        assert!(snapshot.notes[0].body.is_empty());
        let generated = draft(&store, &scope(), "2026-10-29".into(), false, true).unwrap();
        assert!(generated.body.contains("실패를 재현한 로그"));
        assert!(generated.body.contains("검증 일정"));
        assert!(generated.body.contains("완료 표시됨"));
        assert!(!generated.body.contains("RECURSIVE-DRAFT-MUST-NOT-APPEAR"));
        assert_eq!(generated.entry_kind, "portfolio");
        assert_eq!(
            store
                .list_entries("2026-10-01", "2026-10-31", None, 0)
                .unwrap()
                .len(),
            2
        );
    }
    #[test]
    fn editor_saves_typed_time_and_unread_forms_keep_it() {
        let store = Storage::open(":memory:").unwrap();
        let mut entry = Form::new(Kind::Entry, "2026-10-29".into(), None);
        entry.title = "시각 기록".into();
        entry.time = "18:40".into();
        entry.save(&store).unwrap();
        let id = store
            .list_entries("2026-10-29", "2026-10-29", None, 0)
            .unwrap()[0]
            .id
            .clone();
        let hydrated = hydrate_form(&store, Form::entry(store.get_entry(&id).unwrap())).unwrap();
        assert_eq!(hydrated.time, "18:40");
        // A form built from the list without reading the record keeps the time.
        let mut unread = Form::entry(store.get_entry(&id).unwrap());
        unread.title = "제목만 수정".into();
        unread.save(&store).unwrap();
        assert_eq!(
            store.entry_timeline(&id).unwrap().manual_time.as_deref(),
            Some("18:40")
        );
        // Clearing the field in the editor makes the time unknown again.
        let mut cleared = hydrate_form(&store, Form::entry(store.get_entry(&id).unwrap())).unwrap();
        cleared.time.clear();
        cleared.save(&store).unwrap();
        assert_eq!(store.entry_timeline(&id).unwrap().manual_time, None);
        let mut bad = hydrate_form(&store, Form::entry(store.get_entry(&id).unwrap())).unwrap();
        bad.time = "오후 3시".into();
        assert!(bad.save(&store).is_err());
    }
    #[test]
    fn opening_a_past_automatic_record_fills_its_begin_from_the_registered_receipt() {
        let dir = crate::record_inbox::tests::TestDir::new();
        let state = dir.0.join("state");
        let receipt = state.join("diary-starts").join("r");
        std::fs::create_dir_all(&receipt).unwrap();
        let node = dir.0.join("node.exe");
        let script = dir.0.join("project-records.mjs");
        std::fs::write(&node, "").unwrap();
        std::fs::write(&script, "").unwrap();
        let store = Storage::open(":memory:").unwrap();
        let mut record = crate::record_inbox::tests::sample();
        record.work_id = uuid::Uuid::new_v4().to_string();
        let id = store
            .apply_project_record(&record)
            .unwrap()
            .entry_id
            .unwrap();
        let before = store.get_entry(&id).unwrap();
        std::fs::write(
            receipt.join("work.json"),
            format!(r#"{{"workId":"{}"}}"#, record.work_id),
        )
        .unwrap();
        std::fs::write(
            receipt.join("start.json"),
            r#"{"at":"2026-10-31T00:05:00Z","owner":"me@example.invalid"}"#,
        )
        .unwrap();
        // Without a registered record state folder nothing is read.
        complete_evidence(&store, &dir.0, std::slice::from_ref(&id));
        assert_eq!(store.entry_timeline(&id).unwrap().started_at, None);
        crate::record_sync::register(&dir.0, &node, &script, &state).unwrap();
        complete_evidence(&store, &dir.0, std::slice::from_ref(&id));
        let t = store.entry_timeline(&id).unwrap();
        assert_eq!(t.started_at.as_deref(), Some("2026-10-31T00:05:00Z"));
        assert_eq!(t.completed_at, None, "no completion is invented");
        assert_eq!(
            store.get_entry(&id).unwrap(),
            before,
            "the record itself is untouched"
        );
    }
    #[test]
    fn the_finished_days_are_the_seven_before_today() {
        let today = chrono::NaiveDate::from_ymd_opt(2026, 11, 25).unwrap();
        let days = finished_days(today);
        assert_eq!(days.len(), 7);
        assert_eq!(days[0].to_string(), "2026-11-18");
        assert_eq!(days[6].to_string(), "2026-11-24");
    }
    #[test]
    fn a_daily_job_keeps_first_and_makes_only_what_it_kept_whole() {
        use timeschedule_core::chat_originals::{group_name, OriginalsReport};
        let dir = crate::record_inbox::tests::TestDir::new();
        let repo = dir.0.join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        let store = Storage::open(":memory:").unwrap();
        let mut project = Form::new(Kind::Project, String::new(), None);
        project.title = "작업".into();
        project.repo_path = repo.to_string_lossy().into_owned();
        project.save(&store).unwrap();
        let days = finished_days(chrono::NaiveDate::from_ymd_opt(2026, 11, 25).unwrap());
        let skip = std::collections::HashSet::new();
        let run = |go: bool| DailyRun {
            make: |_: &std::path::Path, _: &_| -> Result<_, crate::record_cli::DailyError> {
                panic!("no model is asked")
            },
            key: |_: &_| String::new(),
            skip: &skip,
            keep_going: move || go,
        };
        let whole = |_: &[_], _: &std::path::Path| Ok(OriginalsReport::default());
        // No captured project: nothing is looked for, kept or made.
        let made = daily_job(
            &store,
            || panic!("the store was looked for"),
            |_: &[_], _: &std::path::Path| panic!("originals were kept"),
            Which::Days(&days),
            run(true),
        )
        .unwrap();
        assert_eq!(made, DailyMade::default());
        let id = store.list_projects().unwrap().remove(0).id;
        store.set_chat_capture(&id, true).unwrap();
        let vault = dir.0.join("vault");
        std::fs::create_dir_all(&vault).unwrap();
        // Originals not all kept (a failure or a lock): none of its days is made.
        let incomplete = |_: &[_], _: &std::path::Path| {
            Ok(OriginalsReport {
                locked: 1,
                incomplete: vec![id.clone()],
                ..Default::default()
            })
        };
        let made = daily_job(
            &store,
            || Ok(vault.clone()),
            incomplete,
            Which::Days(&days),
            run(true),
        )
        .unwrap();
        assert_eq!(made.unkept, ["작업"]);
        assert!(made.failed.is_empty() && made.empty == 0);
        // Kept whole but no chat found yet: nothing to make, and no error.
        let made = daily_job(
            &store,
            || Ok(vault.clone()),
            whole,
            Which::Days(&days),
            run(true),
        )
        .unwrap();
        assert_eq!((made.empty, made.failed.len()), (7, 0));
        // Kept chats whose list cannot be read: each day says why.
        let key = store.originals_projects().unwrap().remove(0).root;
        std::fs::create_dir_all(vault.join("originals").join(group_name(&key))).unwrap();
        let made = daily_job(
            &store,
            || Ok(vault.clone()),
            whole,
            Which::Days(&days[..2]),
            run(true),
        )
        .unwrap();
        assert_eq!(made.failed.len(), 2);
        assert!(
            made.failed[0].starts_with("작업 2026-11-18: "),
            "{:?}",
            made.failed
        );
        assert!(made.failed_inputs.is_empty(), "no model was asked");
        // A job told to stop makes nothing more.
        let made = daily_job(
            &store,
            || Ok(vault.clone()),
            whole,
            Which::Days(&days),
            run(false),
        )
        .unwrap();
        assert!(made.stopped && made.failed.is_empty());
        // A store or originals that cannot be reached stop the job with the reason.
        let missing = daily_job(
            &store,
            || Err("문서 저장소 없음".into()),
            whole,
            Which::Days(&days),
            run(true),
        );
        assert_eq!(missing.unwrap_err(), "문서 저장소 없음");
        let unkeepable = |_: &[_], _: &std::path::Path| Err("Codex 사용자 폴더 없음".to_string());
        // Originals that cannot be kept at all: nothing is made, and the job says why.
        let made = daily_job(
            &store,
            || Ok(vault.clone()),
            unkeepable,
            Which::Days(&days),
            run(true),
        )
        .unwrap();
        assert_eq!(made.failed, ["원문 보관 실패: Codex 사용자 폴더 없음"]);
        assert_eq!(made.unkept, ["작업"]);
    }
    /// Two captured projects, each with kept sessions, in a made-up document store.
    struct Past {
        _dir: crate::record_inbox::tests::TestDir,
        store: Storage,
        vault: std::path::PathBuf,
        /// `(ID, title, folder)` of the two projects.
        a: (String, String, String),
        b: (String, String, String),
    }
    fn keep_sessions(
        past: &Past,
        project: &(String, String, String),
        sessions: &[(&str, &[&str])],
    ) {
        use timeschedule_core::chat_originals::group_name;
        let root = past
            .store
            .originals_projects()
            .unwrap()
            .into_iter()
            .find(|p| p.id == project.0)
            .unwrap()
            .root;
        let dir = past.vault.join("originals").join(group_name(&root));
        std::fs::create_dir_all(dir.join("claude/C--x")).unwrap();
        std::fs::write(
            dir.join("project.json"),
            serde_json::json!({"schemaVersion":1,"projectId":project.0,"title":project.1,"repoKey":root})
                .to_string(),
        )
        .unwrap();
        let mut index = String::new();
        for (name, times) in sessions {
            let file = format!("claude/C--x/{name}.jsonl.zst");
            let body: String = times
                .iter()
                .map(|at| {
                    format!(
                        "{}\n",
                        serde_json::json!({"type":"user","timestamp":at,"cwd":"C:\\x",
                            "sessionId":name,"origin":{"kind":"human"},"turnOrigin":"human",
                            "message":{"role":"user","content":format!("{at}의 요청")}})
                    )
                })
                .collect();
            let bytes = if *name == "bad" {
                b"not a zstd file".to_vec()
            } else {
                zstd::encode_all(body.as_bytes(), 3).unwrap()
            };
            std::fs::write(dir.join(&file), bytes).unwrap();
            let modified = chrono::DateTime::parse_from_rfc3339(times.last().unwrap())
                .unwrap()
                .timestamp_nanos_opt()
                .unwrap();
            index.push_str(
                &serde_json::json!({"file":file,"tool":"claude","session":name,"cwd":"C:\\x",
                    "client":"claude-desktop","started":times[0],"source":"x","sourceSize":1,
                    "sourceModified":modified,"archivedAt":"2026-11-26T00:00:00Z"})
                .to_string(),
            );
            index.push('\n');
        }
        std::fs::write(dir.join("index.jsonl"), index).unwrap();
    }
    fn past() -> Past {
        let dir = crate::record_inbox::tests::TestDir::new();
        let store = Storage::open(":memory:").unwrap();
        let mut made = Vec::new();
        for title in ["가 프로젝트", "나 프로젝트"] {
            let repo = dir.0.join(format!("repo-{}", made.len()));
            std::fs::create_dir_all(&repo).unwrap();
            let mut project = Form::new(Kind::Project, String::new(), None);
            project.title = title.into();
            project.repo_path = repo.to_string_lossy().into_owned();
            project.save(&store).unwrap();
            let id = store
                .list_projects()
                .unwrap()
                .into_iter()
                .find(|p| p.title == title)
                .unwrap()
                .id;
            store.set_chat_capture(&id, true).unwrap();
            made.push((id, title.to_string(), repo.to_string_lossy().into_owned()));
        }
        let vault = dir.0.join("vault");
        std::fs::create_dir_all(&vault).unwrap();
        let b = made.pop().unwrap();
        let a = made.pop().unwrap();
        Past {
            _dir: dir,
            store,
            vault,
            a,
            b,
        }
    }
    /// A session on 11-10 and, past Korean midnight, 11-12, and another on 11-25.
    const A_SESSIONS: &[(&str, &[&str])] = &[
        ("s1", &["2026-11-10T01:00:00Z", "2026-11-11T16:00:00Z"]),
        ("s2", &["2026-11-25T01:00:00Z"]),
    ];
    fn day(text: &str) -> chrono::NaiveDate {
        chrono::NaiveDate::parse_from_str(text, "%Y-%m-%d").unwrap()
    }
    type Asked = std::cell::RefCell<Vec<(String, chrono::NaiveDate)>>;
    fn asking<'a>(
        asked: &'a Asked,
        skip: &'a std::collections::HashSet<DailyInput>,
        keep_going: impl Fn() -> bool + 'a,
    ) -> DailyRun<
        'a,
        impl Fn(
                &std::path::Path,
                &timeschedule_core::daily_record::DayTranscript,
            ) -> Result<Made, crate::record_cli::DailyError>
            + 'a,
        impl Fn(&timeschedule_core::daily_record::DayTranscript) -> String + 'a,
        impl Fn() -> bool + 'a,
    > {
        DailyRun {
            make: move |_: &std::path::Path, t: &timeschedule_core::daily_record::DayTranscript| {
                asked.borrow_mut().push((t.title.clone(), t.date));
                Ok(Made::Saved)
            },
            key: |_: &_| String::new(),
            skip,
            keep_going,
        }
    }
    fn whole(
        _: &[timeschedule_core::chat_originals::OriginalsProject],
        _: &std::path::Path,
    ) -> Result<timeschedule_core::chat_originals::OriginalsReport, String> {
        Ok(Default::default())
    }
    #[test]
    fn the_past_days_of_one_project_are_the_days_its_sessions_have_rows_before_today() {
        let past = past();
        keep_sessions(&past, &past.a, A_SESSIONS);
        keep_sessions(&past, &past.b, &[("s3", &["2026-11-10T01:00:00Z"])]);
        let (asked, skip) = (Asked::default(), Default::default());
        let made = daily_job(
            &past.store,
            || Ok(past.vault.clone()),
            whole,
            Which::Past {
                project: &past.a.0,
                before: day("2026-11-25"),
            },
            asking(&asked, &skip, || true),
        )
        .unwrap();
        // The days with rows, not the days between; not the day before which it stops,
        // and nothing of the other captured project.
        assert_eq!(made.saved, [day("2026-11-10"), day("2026-11-12")]);
        assert!(made.failed.is_empty(), "{:?}", made.failed);
        assert_eq!(
            *asked.borrow(),
            [
                ("가 프로젝트".to_string(), day("2026-11-10")),
                ("가 프로젝트".to_string(), day("2026-11-12")),
            ]
        );
        assert!(!made.stopped);
    }
    #[test]
    fn a_dry_run_lists_the_days_that_would_ask_the_model_and_writes_nothing() {
        use timeschedule_core::{
            daily_record::{day_transcript, record::summarize},
            terra::{Reply, Usage},
        };
        let past = past();
        keep_sessions(&past, &past.a, A_SESSIONS);
        // 11-10 was made already, from this very input.
        let model = timeschedule_core::terra::MODEL;
        let made_before = day_transcript(&past.vault, &past.a.2, day("2026-11-10")).unwrap();
        let answer = serde_json::json!({"overview": {"text": "하루", "evidence": [1]},
            "development": [], "troubleshooting": [], "nextSteps": [], "decisions": []});
        summarize(&past.vault, &made_before, model, "medium", |_, _| {
            Ok(Reply {
                text: answer.to_string(),
                usage: Usage::default(),
                seconds: 1.0,
            })
        })
        .unwrap();
        let folder = past.vault.join("daily").join(&made_before.folder);
        let written = |dir: &std::path::Path| {
            let mut names: Vec<_> = std::fs::read_dir(dir)
                .unwrap()
                .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
                .collect();
            names.sort();
            names
        };
        let before = written(&folder);
        let skip = Default::default();
        let made = daily_job(
            &past.store,
            || Ok(past.vault.clone()),
            whole,
            Which::Past {
                project: &past.a.0,
                before: day("2026-11-25"),
            },
            DailyRun {
                make: |vault: &std::path::Path, t: &_| {
                    crate::record_cli::would_summarize_day(vault, t, model)
                },
                key: |_: &_| String::new(),
                skip: &skip,
                keep_going: || true,
            },
        )
        .unwrap();
        assert_eq!(made.due, [day("2026-11-12")]);
        assert_eq!(made.unchanged, 1, "the day made before is left out");
        assert!(made.saved.is_empty() && made.failed_inputs.is_empty());
        assert_eq!(written(&folder), before, "a dry run writes no record");
        let usage = std::fs::read_to_string(folder.join("usage.jsonl")).unwrap();
        assert_eq!(usage.lines().count(), 1, "and asks the model nothing");
    }
    #[test]
    fn the_confirmed_days_are_made_and_no_others() {
        let past = past();
        keep_sessions(&past, &past.a, A_SESSIONS);
        // The other captured project has dialogue on the same day: it was not asked about.
        keep_sessions(&past, &past.b, A_SESSIONS);
        let (asked, skip) = (Asked::default(), Default::default());
        let days = [day("2026-11-12")];
        // The check says which folder it counted the days in.
        let checked = daily_job(
            &past.store,
            || Ok(past.vault.clone()),
            whole,
            Which::Past {
                project: &past.a.0,
                before: day("2026-11-25"),
            },
            asking(&asked, &skip, || true),
        )
        .unwrap();
        let root = root_of(&past, &past.a.0);
        assert_eq!(checked.root.as_deref(), Some(root.as_str()));
        asked.borrow_mut().clear();
        let made = daily_job(
            &past.store,
            || Ok(past.vault.clone()),
            whole,
            Which::ProjectDays {
                project: &past.a.0,
                days: &days,
                root: &root,
            },
            asking(&asked, &skip, || true),
        )
        .unwrap();
        assert_eq!(made.saved, days);
        assert_eq!(*asked.borrow(), [(past.a.1.clone(), day("2026-11-12"))]);
    }
    #[test]
    fn days_counted_in_another_folder_are_not_made() {
        let past = past();
        keep_sessions(&past, &past.a, A_SESSIONS);
        let (asked, skip) = (Asked::default(), Default::default());
        let days = [day("2026-11-12")];
        // The project's folder was another one when the days were counted.
        let made = daily_job(
            &past.store,
            || panic!("the store was looked for"),
            |_: &[_], _: &std::path::Path| panic!("originals were kept"),
            Which::ProjectDays {
                project: &past.a.0,
                days: &days,
                root: "c:/another/folder",
            },
            asking(&asked, &skip, || true),
        )
        .unwrap();
        assert!(asked.borrow().is_empty(), "the model was asked");
        assert!(made.saved.is_empty());
        assert_eq!(made.failed.len(), 1, "{:?}", made.failed);
        assert!(made.failed[0].contains("폴더가"), "{}", made.failed[0]);
    }
    /// The folder key of a project, as the job reads it.
    fn root_of(past: &Past, id: &str) -> String {
        past.store
            .originals_projects()
            .unwrap()
            .into_iter()
            .find(|p| p.id == id)
            .unwrap()
            .root
    }
    #[test]
    fn a_project_that_is_not_captured_or_not_known_makes_nothing_and_says_so() {
        let past = past();
        past.store.set_chat_capture(&past.b.0, false).unwrap();
        let (asked, skip) = (Asked::default(), Default::default());
        let days = [day("2026-11-12")];
        let root = root_of(&past, &past.b.0);
        for which in [
            Which::Past {
                project: &past.b.0,
                before: day("2026-11-25"),
            },
            Which::ProjectDays {
                project: &past.b.0,
                days: &days,
                root: &root,
            },
            Which::Past {
                project: "no-such-project",
                before: day("2026-11-25"),
            },
        ] {
            let made = daily_job(
                &past.store,
                || panic!("the store was looked for"),
                |_: &[_], _: &std::path::Path| panic!("originals were kept"),
                which,
                asking(&asked, &skip, || true),
            )
            .unwrap();
            assert_eq!(made.failed.len(), 1);
            assert!(made.saved.is_empty() && made.due.is_empty());
        }
        assert!(asked.borrow().is_empty());
    }
    #[test]
    fn a_file_that_cannot_be_read_is_said_and_the_other_days_are_still_made() {
        let past = past();
        keep_sessions(
            &past,
            &past.a,
            &[
                ("s1", &["2026-11-10T01:00:00Z"]),
                ("bad", &["2026-11-11T01:00:00Z"]),
            ],
        );
        let (asked, skip) = (Asked::default(), Default::default());
        let made = daily_job(
            &past.store,
            || Ok(past.vault.clone()),
            whole,
            Which::Past {
                project: &past.a.0,
                before: day("2026-11-25"),
            },
            asking(&asked, &skip, || true),
        )
        .unwrap();
        assert_eq!(made.saved, [day("2026-11-10")]);
        assert_eq!(made.failed.len(), 1, "{:?}", made.failed);
        assert!(
            made.failed[0].starts_with("가 프로젝트: claude/C--x/bad.jsonl.zst: "),
            "{:?}",
            made.failed
        );
    }
    #[test]
    fn the_stop_flag_ends_the_past_days_between_days() {
        let past = past();
        keep_sessions(&past, &past.a, A_SESSIONS);
        let (asked, skip) = (Asked::default(), Default::default());
        // Asked before the listing and before each day: once a day is made, it is told to stop.
        let made = daily_job(
            &past.store,
            || Ok(past.vault.clone()),
            whole,
            Which::Past {
                project: &past.a.0,
                before: day("2026-11-25"),
            },
            asking(&asked, &skip, || asked.borrow().is_empty()),
        )
        .unwrap();
        assert_eq!(made.saved, [day("2026-11-10")]);
        assert!(made.stopped);
    }
    #[test]
    fn a_stop_before_the_listing_makes_nothing() {
        let past = past();
        keep_sessions(&past, &past.a, A_SESSIONS);
        let (asked, skip) = (Asked::default(), Default::default());
        let made = daily_job(
            &past.store,
            || Ok(past.vault.clone()),
            whole,
            Which::Past {
                project: &past.a.0,
                before: day("2026-11-25"),
            },
            asking(&asked, &skip, || false),
        )
        .unwrap();
        assert!(made.stopped && made.saved.is_empty() && made.due.is_empty());
        assert!(asked.borrow().is_empty());
    }
    #[test]
    fn a_kept_folder_of_another_project_is_not_sent_to_the_model() {
        for which in [0, 1] {
            let past = past();
            keep_sessions(&past, &past.a, A_SESSIONS);
            // The folder was moved from another project: it still names that one.
            let root = past
                .store
                .originals_projects()
                .unwrap()
                .into_iter()
                .find(|p| p.id == past.a.0)
                .unwrap()
                .root;
            let meta = past
                .vault
                .join("originals")
                .join(timeschedule_core::chat_originals::group_name(&root))
                .join("project.json");
            std::fs::write(
                &meta,
                serde_json::json!({"schemaVersion":1,"projectId":past.b.0,
                    "title":"나 프로젝트","repoKey":root})
                .to_string(),
            )
            .unwrap();
            let (asked, skip) = (Asked::default(), Default::default());
            let days = [day("2026-11-10"), day("2026-11-12")];
            let counted_in = root_of(&past, &past.a.0);
            let made = daily_job(
                &past.store,
                || Ok(past.vault.clone()),
                whole,
                if which == 0 {
                    Which::Past {
                        project: &past.a.0,
                        before: day("2026-11-25"),
                    }
                } else {
                    Which::ProjectDays {
                        project: &past.a.0,
                        days: &days,
                        root: &counted_in,
                    }
                },
                asking(&asked, &skip, || true),
            )
            .unwrap();
            assert!(asked.borrow().is_empty(), "the model was asked");
            assert!(made.saved.is_empty() && made.due.is_empty());
            assert_eq!(made.failed.len(), 1, "{:?}", made.failed);
            assert!(made.failed[0].starts_with("가 프로젝트: "));
        }
    }
    #[test]
    fn a_scan_part_that_cannot_start_does_not_hide_the_others() {
        use timeschedule_core::chat_originals::group_name;
        let dir = crate::record_inbox::tests::TestDir::new();
        let repo = dir.0.join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        let store = Storage::open(":memory:").unwrap();
        let mut project = Form::new(Kind::Project, String::new(), None);
        project.title = "작업".into();
        project.repo_path = repo.to_string_lossy().into_owned();
        project.save(&store).unwrap();
        let id = store.list_projects().unwrap().remove(0).id;
        store.set_chat_capture(&id, true).unwrap();
        let key = store.originals_projects().unwrap().remove(0).root;
        let vault = dir.0.join("vault");
        let records = vault.join("daily").join(group_name(&key));
        std::fs::create_dir_all(&records).unwrap();
        let record = serde_json::json!({
            "schemaVersion": 1,
            "project": {"id": id, "title": "작업", "repoKey": key},
            "date": "2026-11-20", "revision": 1, "generatedAt": "2026-11-24T10:00:00Z",
            "model": "gpt-6.1-terra", "effort": "medium", "promptVersion": 3,
            "input": {"sessions": 1, "agentRuns": 0, "blocks": 2, "chars": 100,
                      "fingerprint": "fp", "latestArchived": null, "warnings": []},
            "usage": {"inputTokens": 1, "cachedInputTokens": 0, "outputTokens": 1,
                      "reasoningOutputTokens": null, "seconds": 1.0},
            "checks": {"withoutEvidence": 0, "quotesNotFound": 0,
                       "proposedByUnsupported": 0, "decidedByUnsupported": 0},
            "summary": [{"text": "요약", "evidence": []}],
            "development": [], "troubleshooting": [], "improvements": [], "decisions": [],
            "sources": []
        });
        std::fs::write(records.join("2026-11-20.json"), record.to_string()).unwrap();
        // No home folder of Codex: nothing to keep or excerpt, but the store is there.
        let no_home = "Codex 사용자 폴더를 찾을 수 없습니다.";
        let scan = scan_from(
            &store,
            Err(no_home.into()),
            None,
            true,
            || Ok(vault.clone()),
        );
        assert_eq!(scan.daily.as_ref().unwrap().added, 1);
        assert_eq!(scan.excerpts.as_ref().unwrap_err(), no_home);
        assert_eq!(scan.originals.as_ref().unwrap_err(), no_home);
        assert!(scan.needs_attention());
        assert_eq!(scan.problems(), [no_home]);
        assert_eq!(scan.notices(), ["하루 기록 1건을 다이어리에 반영했습니다."]);
        assert_eq!(
            store
                .list_entries("2026-11-20", "2026-11-20", None, 0)
                .unwrap()
                .len(),
            1
        );
    }
    #[test]
    fn fetch_search_result_reads_original_body() {
        let store = Storage::open(":memory:").unwrap();
        let mut note = Form::new(Kind::Note, String::new(), None);
        note.title = "검색 대상".into();
        note.body = "처음\n마지막".into();
        note.save(&store).unwrap();
        let hit = store.search_diary("검색", 50).unwrap().remove(0);
        let form = fetch_form(&store, &hit).unwrap();
        assert_eq!(form.kind, Kind::Note);
        assert_eq!(form.body, "처음\n마지막");
    }
    #[test]
    fn today_events_are_independent_of_both_calendar_months() {
        let store = Storage::open(":memory:").unwrap();
        let mut event = Form::new(Kind::Event, "2026-10-29".into(), None);
        event.title = "오늘 일정".into();
        event.body = "목록에 전달하지 않는 상세 본문".into();
        event.save(&store).unwrap();
        let mut scope = scope();
        scope.widget_from = "2026-12-01".into();
        scope.widget_to = "2026-12-31".into();
        scope.event_from = "2027-01-01".into();
        scope.event_to = "2027-01-31".into();
        let result = snapshot(&store, &scope).unwrap();
        assert!(result.widget_days.is_empty());
        assert!(result.days.is_empty());
        assert_eq!(result.today.total, 1);
        assert_eq!(result.today.items[0].title, "오늘 일정");
    }
}
