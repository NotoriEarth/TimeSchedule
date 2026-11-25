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
    let claude_home = std::env::var_os("CLAUDE_CONFIG_DIR")
        .map(std::path::PathBuf::from)
        .or_else(|| {
            std::env::var_os("USERPROFILE").map(|p| std::path::PathBuf::from(p).join(".claude"))
        });
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
