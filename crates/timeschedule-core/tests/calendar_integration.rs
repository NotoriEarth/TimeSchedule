use timeschedule_core::{
    calendar::*,
    diary::{EntryInput, ProjectInput},
    storage::{Storage, TaskInput},
};

fn entry(date: &str, title: &str) -> EntryInput {
    EntryInput {
        id: None,
        project_id: None,
        date: date.into(),
        kind: "development".into(),
        title: title.into(),
        body: "원문은 상세에서만".into(),
        source_url: String::new(),
    }
}

#[test]
fn category_order_controls_calendar_without_losing_sources() {
    let store = Storage::open(":memory:").unwrap();
    let defaults = store.list_categories().unwrap();
    assert_eq!(
        defaults.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(),
        ["일정", "기록"]
    );
    let custom = store.save_category(None, "학습", "#42A0C0").unwrap();
    let record = store
        .save_calendar_entry(
            entry("2026-11-03", "학습 내용"),
            EntryDisplay {
                category_id: custom.id.clone(),
                short_title: "학습".into(),
            },
            None,
        )
        .unwrap();
    let task = store
        .save_task(TaskInput {
            id: None,
            title: "미완료 일정".into(),
            date: Some("2026-11-03".into()),
            completed: false,
        })
        .unwrap();
    let before = store.calendar_days("2026-11-03", "2026-11-03").unwrap();
    assert_eq!(before[0].items[0].id, task.id);
    let ids = store
        .list_categories()
        .unwrap()
        .iter()
        .map(|c| c.id.clone())
        .collect::<Vec<_>>();
    store
        .reorder_categories(
            &ids,
            &[
                custom.id.clone(),
                SCHEDULE_CATEGORY.into(),
                RECORD_CATEGORY.into(),
            ],
        )
        .unwrap();
    let after = store.calendar_days("2026-11-03", "2026-11-03").unwrap();
    assert_eq!(after[0].items[0].id, record.entry.id);
    assert_eq!(after[0].items[0].short_title, "학습");
    assert!(
        store.reorder_categories(&ids, &ids).is_err(),
        "stale order must not overwrite newer order"
    );
    assert!(store.save_category(None, "bad", "red<script>").is_err());
    assert_eq!(
        store.get_entry(&record.entry.id).unwrap().body,
        "원문은 상세에서만"
    );
}

#[test]
fn calendar_counts_and_pages_are_independent_of_200_entry_list() {
    let store = Storage::open(":memory:").unwrap();
    for i in 0..205 {
        store
            .save_entry(entry("2026-11-03", &format!("기록 {i}")))
            .unwrap();
    }
    store.save_entry(entry("2026-12-01", "다른 달")).unwrap();
    store
        .save_task(TaskInput {
            id: None,
            title: "완료".into(),
            date: Some("2026-11-03".into()),
            completed: true,
        })
        .unwrap();
    store
        .save_task(TaskInput {
            id: None,
            title: "날짜 미정".into(),
            date: None,
            completed: false,
        })
        .unwrap();
    let days = store.calendar_days("2026-11-03", "2026-11-03").unwrap();
    assert_eq!(days.len(), 1);
    assert_eq!(
        (days[0].total, days[0].completed, days[0].items.len()),
        (206, 1, 16)
    );
    let mut ids = std::collections::HashSet::new();
    for offset in [0, 50, 100, 150, 200] {
        let page = store.calendar_page("2026-11-03", offset).unwrap();
        assert_eq!(page.total, 206);
        for item in page.items {
            assert!(ids.insert((item.source, item.id)));
        }
    }
    assert_eq!(ids.len(), 206);
    assert_eq!(
        store.calendar_days("2026-12-01", "2026-12-01").unwrap()[0].items[0].title,
        "다른 달"
    );
}

#[test]
fn record_edit_is_atomic_and_preserves_alias_on_external_update() {
    let store = Storage::open(":memory:").unwrap();
    let original = store
        .save_calendar_entry(
            entry("2026-11-03", "전체 제목"),
            EntryDisplay {
                category_id: RECORD_CATEGORY.into(),
                short_title: "짧은 제목".into(),
            },
            None,
        )
        .unwrap();
    let mut changed = entry("2026-11-04", "수정 제목");
    changed.id = Some(original.entry.id.clone());
    let saved = store
        .save_calendar_entry(changed.clone(), original.display.clone(), Some(&original))
        .unwrap();
    assert!(store
        .save_calendar_entry(changed, original.display.clone(), Some(&original))
        .is_err());
    let mut external = entry("2026-11-04", "외부 제목");
    external.id = Some(saved.entry.id.clone());
    store.save_entry(external).unwrap();
    assert_eq!(
        store.entry_display(&saved.entry.id).unwrap().short_title,
        "짧은 제목"
    );
    let mut invalid = entry("2026-11-05", "저장 안 될 제목");
    invalid.id = Some(saved.entry.id.clone());
    assert!(store
        .save_calendar_entry(
            invalid,
            EntryDisplay {
                category_id: "missing".into(),
                short_title: String::new()
            },
            None
        )
        .is_err());
    assert_eq!(store.get_entry(&saved.entry.id).unwrap().title, "외부 제목");
}

#[test]
fn legacy_task_supports_shared_schedule_body_time_and_completion() {
    let store = Storage::open(":memory:").unwrap();
    let task = store
        .save_task(TaskInput {
            id: None,
            title: "기존 할 일".into(),
            date: Some("2026-11-03".into()),
            completed: false,
        })
        .unwrap();
    let original = store.task_edit(&task.id).unwrap();
    let details = TaskDetails {
        body: "견적 내용을 비교".into(),
        all_day: false,
        start_time: Some("13:00".into()),
        end_time: Some("14:00".into()),
    };
    let edited = store
        .save_calendar_task(
            TaskInput {
                id: Some(task.id.clone()),
                title: task.title,
                date: task.date,
                completed: false,
            },
            details,
            Some(&original),
        )
        .unwrap();
    assert_eq!(
        store.calendar_days("2026-11-03", "2026-11-03").unwrap()[0].items[0]
            .start_time
            .as_deref(),
        Some("13:00")
    );
    store
        .set_calendar_completed(CalendarSource::Task, &task.id, true)
        .unwrap();
    assert_eq!(
        store.task_edit(&task.id).unwrap().details.body,
        "견적 내용을 비교"
    );
    assert!(store
        .save_calendar_task(
            TaskInput {
                id: Some(task.id),
                title: "낡은 수정".into(),
                date: None,
                completed: false
            },
            edited.details.clone(),
            Some(&edited)
        )
        .is_err());
    assert_eq!(store.search_diary("견적 내용을", 50).unwrap().len(), 1);
}

#[test]
fn project_short_name_is_shared_without_replacing_original_name() {
    let store = Storage::open(":memory:").unwrap();
    let project = store
        .save_project(ProjectInput {
            id: None,
            title: "TimeSchedule 원래 이름".into(),
            description: String::new(),
            repo_path: String::new(),
        })
        .unwrap();
    store.save_project_short_name(&project.id, "TS").unwrap();
    let mut input = entry("2026-11-03", "작업");
    input.project_id = Some(project.id.clone());
    store.save_entry(input).unwrap();
    let day = store.calendar_days("2026-11-03", "2026-11-03").unwrap();
    assert_eq!(day[0].items[0].project_short_name, "TS");
    assert_eq!(
        store.list_projects().unwrap()[0].title,
        "TimeSchedule 원래 이름"
    );
}

#[test]
fn migration_keeps_backup_and_category_settings_survive_restart() {
    let dir = std::env::temp_dir().join(format!("timeschedule-calendar-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("schedule.db");
    let id = {
        let s = Storage::open(&path).unwrap();
        s.save_entry(entry("2026-11-03", "이전 원문")).unwrap().id
    };
    {
        let c = rusqlite::Connection::open(&path).unwrap();
        c.execute_batch("DROP TABLE entry_display; DROP TABLE project_display; DROP TABLE task_details; DROP TABLE calendar_categories; DROP INDEX tasks_date; PRAGMA user_version=3;").unwrap();
    }
    let custom = {
        let s = Storage::open(&path).unwrap();
        assert_eq!(s.get_entry(&id).unwrap().body, "원문은 상세에서만");
        let c = s.save_category(None, "학습", "#123456").unwrap();
        let ids = s
            .list_categories()
            .unwrap()
            .into_iter()
            .map(|c| c.id)
            .collect::<Vec<_>>();
        s.reorder_categories(
            &ids,
            &[
                c.id.clone(),
                RECORD_CATEGORY.into(),
                SCHEDULE_CATEGORY.into(),
            ],
        )
        .unwrap();
        c.id
    };
    {
        let s = Storage::open(&path).unwrap();
        assert_eq!(s.list_categories().unwrap()[0].id, custom);
    }
    let backups = std::fs::read_dir(&dir)
        .unwrap()
        .map(|p| p.unwrap().path())
        .filter(|p| p.to_string_lossy().contains("pre-v4-"))
        .collect::<Vec<_>>();
    assert_eq!(backups.len(), 1);
    {
        let c = rusqlite::Connection::open(&backups[0]).unwrap();
        assert_eq!(
            c.query_row::<i64, _, _>("PRAGMA user_version", [], |r| r.get(0))
                .unwrap(),
            3
        );
        assert_eq!(
            c.query_row::<String, _, _>(
                "SELECT title FROM diary_entries WHERE id=?1",
                [&id],
                |r| r.get(0)
            )
            .unwrap(),
            "이전 원문"
        );
    }
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn invalid_project_alias_cannot_partially_create_a_project() {
    let store = Storage::open(":memory:").unwrap();
    let input = ProjectInput {
        id: None,
        title: "프로젝트".into(),
        description: String::new(),
        repo_path: String::new(),
    };
    assert!(store.save_calendar_project(input, "여러\n줄").is_err());
    assert!(store.list_projects().unwrap().is_empty());
}

#[test]
fn completed_section_opens_directly_after_many_pending_records() {
    let s = Storage::open(":memory:").unwrap();
    for i in 0..55 {
        s.save_entry(entry("2026-11-03", &format!("기록{i}")))
            .unwrap();
    }
    s.save_task(TaskInput {
        id: None,
        title: "완료 일정".into(),
        date: Some("2026-11-03".into()),
        completed: true,
    })
    .unwrap();
    let page = s.calendar_section("2026-11-03", 0, true).unwrap();
    assert_eq!(page.items.len(), 1);
    assert!(page.items[0].completed);
    assert!(!page.has_more);
    assert_eq!(
        s.calendar_section("2026-11-03", 0, false)
            .unwrap()
            .items
            .len(),
        50
    );
}
