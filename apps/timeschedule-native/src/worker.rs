use crate::editor::{Form, Kind};
use std::sync::Arc;
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
    pub today: CalendarDay,
    pub tasks: Vec<Task>,
    pub notes: Vec<Note>,
    pub entries: Vec<EntrySummary>,
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
    Ok(Snapshot {
        days,
        widget_days,
        today,
        categories: store.list_categories()?,
        projects: store.list_projects()?,
        tasks: store.list_tasks()?,
        notes: store.list_note_headers()?,
        entries: store.list_entries(&scope.from, &scope.to, scope.project.as_deref(), 0)?,
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
