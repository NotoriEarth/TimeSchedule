use super::*;
use crate::{
    calendar::EntryDisplay,
    diary::{Entry, EntryInput, ProjectInput},
    project_records::repo_key,
    timeline::TimeChange,
};
use serde_json::json;
use std::path::PathBuf;

/// A store with one captured project, its folder, and a document store.
struct Fixture {
    root: PathBuf,
    store: Storage,
    project: String,
    /// The `repoKey` of the project's folder.
    key: String,
    vault: PathBuf,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn setup() -> Fixture {
    let root = std::env::temp_dir().join(format!("ts-daily-diary-{}", uuid::Uuid::new_v4()));
    let repo = root.join("repo");
    fs::create_dir_all(&repo).unwrap();
    let store = Storage::open(":memory:").unwrap();
    let project = store
        .save_project(ProjectInput {
            id: None,
            title: "작업".into(),
            description: String::new(),
            repo_path: repo.to_string_lossy().into_owned(),
        })
        .unwrap()
        .id;
    store.set_chat_capture(&project, true).unwrap();
    let vault = root.join("vault");
    fs::create_dir_all(vault.join("daily")).unwrap();
    Fixture {
        key: repo_key(&repo.to_string_lossy()),
        root,
        store,
        project,
        vault,
    }
}

/// A record file of schema 1, as made before prompt version 4: the app still shows
/// its `summary` sentences and `improvements`.
fn record(project: &str, key: &str, date: &str, revision: u32, summary: &str) -> String {
    json!({
        "schemaVersion": 1,
        "project": {"id": project, "title": "작업", "repoKey": key},
        "date": date, "revision": revision, "generatedAt": "2026-11-24T10:00:00Z",
        "model": "gpt-6.1-terra", "effort": "medium", "promptVersion": 3,
        "input": {"sessions": 1, "agentRuns": 0, "blocks": 2, "chars": 100,
                  "fingerprint": format!("fp-{revision}-{summary}"), "latestArchived": null, "warnings": []},
        "usage": {"inputTokens": 1, "cachedInputTokens": 0, "outputTokens": 1,
                  "reasoningOutputTokens": null, "seconds": 1.0},
        "checks": {"withoutEvidence": 0, "quotesNotFound": 0,
                   "proposedByUnsupported": 0, "decidedByUnsupported": 0},
        "summary": [{"text": summary, "evidence": []}],
        "development": [], "troubleshooting": [], "improvements": [], "decisions": [],
        "sources": []
    })
    .to_string()
}

impl Fixture {
    /// Where the records of the folder `key` are kept.
    fn records(&self, key: &str) -> PathBuf {
        let dir = self.vault.join("daily").join(group_name(key));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Makes `revision` the latest record of the day in the project's folder.
    fn write(&self, date: &str, revision: u32, summary: &str) {
        fs::write(
            self.records(&self.key).join(format!("{date}.json")),
            record(&self.project, &self.key, date, revision, summary),
        )
        .unwrap();
    }

    fn scan(&self) -> DiaryReport {
        self.store.import_daily_records(&self.vault).unwrap()
    }

    /// The entries the diary shows for the day.
    fn shown(&self, date: &str) -> Vec<Entry> {
        self.store
            .list_entries(date, date, None, 0)
            .unwrap()
            .into_iter()
            .map(|e| self.store.get_entry(&e.id).unwrap())
            .collect()
    }

    /// Saves the entry the way the diary editor of the app does.
    fn edit(&self, id: &str, change: impl FnOnce(&mut EntryInput)) {
        let entry = self.store.get_entry(id).unwrap();
        let mut input = EntryInput {
            id: Some(entry.id),
            project_id: entry.project_id,
            date: entry.date,
            kind: entry.kind,
            title: entry.title,
            body: entry.body,
            source_url: entry.source_url,
        };
        change(&mut input);
        self.store
            .save_calendar_entry_timed(input, EntryDisplay::default(), TimeChange::Keep, None)
            .unwrap();
    }
}

#[test]
fn a_record_becomes_one_diary_entry_and_a_newer_one_replaces_its_text() {
    let f = setup();
    f.write("2026-11-20", 1, "첫 판의 요약");
    // Kept revisions, temporary and lock files are not records to show.
    let records = f.records(&f.key);
    fs::write(records.join("2026-11-20.r1.json"), "not a record").unwrap();
    fs::write(records.join(".2026-11-20.lock"), "").unwrap();
    fs::write(records.join("usage.jsonl"), "{}\n").unwrap();
    let report = f.scan();
    assert_eq!((report.added, report.failed), (1, 0));
    let entries = f.shown("2026-11-20");
    assert_eq!(entries.len(), 1);
    assert_eq!(
        (entries[0].kind.as_str(), entries[0].title.as_str()),
        ("development", "하루 정리 · 2026-11-20")
    );
    // Only what a person reads: the revision, model and time stay in the file.
    assert_eq!(entries[0].body, "## 오늘 한 일\n- 첫 판의 요약\n");
    // Nothing new: nothing changes.
    assert_eq!(f.scan().unchanged, 1);
    // A newer record replaces the text of an entry nobody edited.
    f.write("2026-11-20", 2, "둘째 판의 요약");
    assert_eq!(f.scan().updated, 1);
    let entries = f.shown("2026-11-20");
    assert_eq!(entries.len(), 1);
    assert!(entries[0].body.contains("둘째 판의 요약"));
    // The owner changed the title only: the text is still the program's.
    f.edit(&entries[0].id, |e| e.title = "내가 붙인 제목".into());
    f.write("2026-11-20", 3, "셋째 판의 요약");
    assert_eq!(f.scan().updated, 1);
    let entries = f.shown("2026-11-20");
    assert_eq!(entries[0].title, "내가 붙인 제목");
    assert!(entries[0].body.contains("셋째 판의 요약"));
}

/// Makes the entry and its receipt look as an older program left them.
fn shown_by_an_older_program(f: &Fixture, id: &str, text: &str) {
    let c = f.store.connection().unwrap();
    c.execute(
        "UPDATE diary_entries SET body=?2, source_body=?2 WHERE id=?1",
        params![id, text],
    )
    .unwrap();
    c.execute("UPDATE daily_record_entries SET shown_body=?1", [text])
        .unwrap();
}

#[test]
fn the_same_record_shown_another_way_takes_the_new_text_unless_the_owner_changed_it() {
    let f = setup();
    f.write("2026-11-20", 1, "첫 판의 요약");
    f.scan();
    let id = f.shown("2026-11-20").remove(0).id;
    let old = "- 요약: 첫 판의 요약 ([1] 사람 · S1 3행)\n하루 기록 판 1 · gpt-6.1-terra · …\n";
    shown_by_an_older_program(&f, &id, old);
    assert_eq!(f.scan().updated, 1);
    assert_eq!(
        f.store.get_entry(&id).unwrap().body,
        "## 오늘 한 일\n- 첫 판의 요약\n"
    );
    assert_eq!(f.scan().unchanged, 1);
    // The owner edited the old text: it stays, and the scan has nothing new each time.
    shown_by_an_older_program(&f, &id, old);
    f.edit(&id, |e| e.body = "직접 고친 글".into());
    for _ in 0..2 {
        let report = f.scan();
        assert_eq!((report.unchanged, report.updated, report.kept), (1, 0, 0));
    }
    assert_eq!(f.store.get_entry(&id).unwrap().body, "직접 고친 글");
}

#[test]
fn a_record_of_schema_1_shows_its_sentences_and_improvements_under_the_new_headings() {
    let f = setup();
    let mut old: serde_json::Value =
        serde_json::from_str(&record(&f.project, &f.key, "2026-11-20", 1, "첫 문장")).unwrap();
    old["summary"] =
        json!([{"text": "첫 문장", "evidence": []}, {"text": "둘째 문장", "evidence": []}]);
    old["improvements"] = json!([{"text": "나중에 할 개선", "evidence": []}]);
    fs::write(f.records(&f.key).join("2026-11-20.json"), old.to_string()).unwrap();
    assert_eq!(f.scan().added, 1);
    assert_eq!(
        f.shown("2026-11-20")[0].body,
        "## 오늘 한 일\n- 첫 문장\n- 둘째 문장\n\n## 남은 일\n- 나중에 할 개선\n"
    );
}

#[test]
fn the_owners_edit_and_deletion_survive_a_newer_record() {
    let f = setup();
    f.write("2026-11-20", 1, "첫 판");
    f.scan();
    let first = f.shown("2026-11-20").remove(0);
    f.edit(&first.id, |e| e.body = "직접 고친 글".into());
    f.write("2026-11-20", 2, "둘째 판");
    let report = f.scan();
    assert_eq!((report.kept, report.updated), (1, 0));
    let kept = f.store.get_entry(&first.id).unwrap();
    assert_eq!(kept.body, "직접 고친 글");
    // Nothing of the owner's entry is touched, the collected source included.
    assert_eq!(kept.source_body, first.body);
    // The same record again is nothing new.
    assert_eq!(f.scan().unchanged, 1);
    // Deleted, the entry is not made again by this or a later record.
    f.store.delete_entry(&first.id).unwrap();
    f.write("2026-11-20", 3, "셋째 판");
    let report = f.scan();
    assert_eq!((report.added, report.kept), (0, 1));
    assert!(f.shown("2026-11-20").is_empty());
}

#[test]
fn text_the_owner_put_back_to_an_older_revision_stays() {
    let f = setup();
    f.write("2026-11-20", 1, "첫 판");
    f.scan();
    let first = f.shown("2026-11-20").remove(0);
    f.write("2026-11-20", 2, "둘째 판");
    f.scan();
    // The owner prefers the first text, and the store has the first record again.
    f.edit(&first.id, |e| e.body = first.body.clone());
    f.write("2026-11-20", 1, "첫 판");
    let restored = f.scan();
    f.write("2026-11-20", 3, "셋째 판");
    let third = f.scan();
    assert_eq!(f.store.get_entry(&first.id).unwrap().body, first.body);
    assert_eq!((restored.older, restored.kept, restored.updated), (1, 0, 0));
    assert_eq!((third.kept, third.updated), (1, 0));
}

#[test]
fn an_entry_moved_to_another_project_or_day_is_left_alone() {
    let f = setup();
    f.write("2026-11-20", 1, "스무날 첫 판");
    f.write("2026-11-21", 1, "스무하루 첫 판");
    f.scan();
    let other = f
        .store
        .save_project(ProjectInput {
            id: None,
            title: "다른 일".into(),
            description: String::new(),
            repo_path: String::new(),
        })
        .unwrap()
        .id;
    let to_project = f.shown("2026-11-20").remove(0);
    let to_day = f.shown("2026-11-21").remove(0);
    f.edit(&to_project.id, |e| e.project_id = Some(other));
    f.edit(&to_day.id, |e| e.date = "2026-12-01".into());
    f.write("2026-11-20", 2, "스무날 둘째 판");
    f.write("2026-11-21", 2, "스무하루 둘째 판");
    let report = f.scan();
    for moved in [to_project, to_day] {
        let now = f.store.get_entry(&moved.id).unwrap();
        assert_eq!(
            (now.body, now.source_body),
            (moved.body.clone(), moved.body)
        );
    }
    assert_eq!((report.kept, report.updated, report.added), (2, 0, 0));
}

#[test]
fn a_record_made_for_another_folder_is_not_shown() {
    let f = setup();
    fs::write(
        f.records(&f.key).join("2026-11-20.json"),
        record(
            &f.project,
            "c:/elsewhere",
            "2026-11-20",
            1,
            "다른 폴더의 기록",
        ),
    )
    .unwrap();
    let report = f.scan();
    assert_eq!(
        (report.other_folder, report.added, report.failed),
        (1, 0, 0)
    );
    assert!(f.shown("2026-11-20").is_empty());
}

#[test]
fn the_records_of_a_new_folder_never_replace_those_of_the_old_one() {
    let f = setup();
    f.write("2026-11-20", 3, "예전 폴더의 셋째 판");
    f.scan();
    let old = f.shown("2026-11-20").remove(0);
    // The project moves to another folder and its chats are captured there.
    let repo = f.root.join("moved");
    fs::create_dir_all(&repo).unwrap();
    f.store
        .save_project(ProjectInput {
            id: Some(f.project.clone()),
            title: "작업".into(),
            description: String::new(),
            repo_path: repo.to_string_lossy().into_owned(),
        })
        .unwrap();
    f.store.set_chat_capture(&f.project, true).unwrap();
    let key = repo_key(&repo.to_string_lossy());
    let latest = f.records(&key).join("2026-11-20.json");
    fs::write(
        &latest,
        record(&f.project, &key, "2026-11-20", 1, "새 폴더의 첫 판"),
    )
    .unwrap();
    let report = f.scan();
    assert_eq!(f.store.get_entry(&old.id).unwrap().body, old.body);
    assert_eq!((report.added, report.updated), (1, 0));
    let entries = f.shown("2026-11-20");
    assert_eq!(entries.len(), 2);
    assert!(entries.iter().any(|e| e.body.contains("새 폴더의 첫 판")));
    // Another record with a revision already seen is not a newer one.
    fs::write(
        &latest,
        record(
            &f.project,
            &key,
            "2026-11-20",
            1,
            "같은 판 번호의 다른 기록",
        ),
    )
    .unwrap();
    let report = f.scan();
    assert_eq!((report.older, report.updated), (1, 0));
    assert!(f
        .shown("2026-11-20")
        .iter()
        .all(|e| !e.body.contains("같은 판 번호의 다른 기록")));
}

#[test]
fn a_record_of_another_project_or_day_or_unreadable_is_not_shown() {
    let f = setup();
    let records = f.records(&f.key);
    fs::write(
        records.join("2026-11-20.json"),
        record("someone-else", &f.key, "2026-11-20", 1, "남의 기록"),
    )
    .unwrap();
    fs::write(
        records.join("2026-11-21.json"),
        record(&f.project, &f.key, "2026-11-22", 1, "날짜가 다른 기록"),
    )
    .unwrap();
    fs::write(records.join("2026-11-23.json"), "{ broken").unwrap();
    f.write("2026-11-24", 1, "제대로 된 기록");
    let report = f.scan();
    assert_eq!((report.added, report.failed), (1, 3));
    assert!(f.shown("2026-11-20").is_empty());
    assert_eq!(f.shown("2026-11-24").len(), 1);
}

#[test]
fn only_a_missing_record_folder_is_nothing_to_show() {
    let f = setup();
    assert_eq!(f.scan(), DiaryReport::default());
    // Something that is not a folder where the project's records belong.
    fs::write(f.vault.join("daily").join(group_name(&f.key)), "").unwrap();
    assert_eq!(f.scan().failed, 1);
}

#[test]
fn a_project_whose_chats_are_not_captured_shows_no_record() {
    let f = setup();
    f.write("2026-11-20", 1, "요약");
    f.store.set_chat_capture(&f.project, false).unwrap();
    assert_eq!(f.scan(), DiaryReport::default());
}

#[test]
fn opening_a_schema_15_database_backs_it_up_and_adds_the_receipts() {
    let root = std::env::temp_dir().join(format!("ts-daily-diary-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&root).unwrap();
    let path = root.join("schedule.db");
    {
        let mut old = Connection::open(&path).unwrap();
        crate::storage::migrate_to(&mut old, 15).unwrap();
        let tables: i64 = old
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE name='daily_record_entries'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(tables, 0);
    }
    {
        let store = Storage::open(&path).unwrap();
        let c = store.connection().unwrap();
        let version: i64 = c
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version, 18);
        c.query_row("SELECT count(*) FROM daily_record_entries", [], |r| {
            r.get::<_, i64>(0)
        })
        .unwrap();
    }
    let backups: Vec<PathBuf> = fs::read_dir(&root)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.to_string_lossy().contains(".pre-v18-"))
        .collect();
    assert_eq!(backups.len(), 1);
    let backup = Connection::open(&backups[0]).unwrap();
    let version: i64 = backup
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap();
    assert_eq!(version, 15);
    drop(backup);
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn what_decides_an_update_is_the_text_the_program_showed_not_the_source() {
    let f = setup();
    f.write("2026-11-20", 1, "첫 판");
    f.scan();
    let first = f.shown("2026-11-20").remove(0);
    // The collected source no longer equals the text, but the text is still the
    // program's own: the next record replaces it.
    f.store
        .connection()
        .unwrap()
        .execute(
            "UPDATE diary_entries SET source_body='다른 원문', kind='note' WHERE id=?1",
            [&first.id],
        )
        .unwrap();
    f.write("2026-11-20", 2, "둘째 판");
    assert_eq!(f.scan().updated, 1);
    let now = f.store.get_entry(&first.id).unwrap();
    assert!(now.body.contains("둘째 판"));
    // The kind the owner chose stays.
    assert_eq!(now.kind, "note");
    // Edited, kept, then put back to what was shown: it is the program's text again.
    f.edit(&first.id, |e| e.body = "잠시 고친 글".into());
    f.write("2026-11-20", 3, "셋째 판");
    assert_eq!(f.scan().kept, 1);
    f.edit(&first.id, |e| e.body = now.body.clone());
    f.write("2026-11-20", 4, "넷째 판");
    assert_eq!(f.scan().updated, 1);
    assert!(f
        .store
        .get_entry(&first.id)
        .unwrap()
        .body
        .contains("넷째 판"));
}

#[test]
fn a_record_too_long_for_the_diary_shows_its_start_and_where_the_rest_is() {
    let f = setup();
    f.write("2026-11-20", 1, &"가".repeat(BODY_LIMIT + 10));
    let report = f.scan();
    assert_eq!((report.added, report.failed), (1, 0));
    let body = &f.shown("2026-11-20")[0].body;
    assert_eq!(body.chars().count(), BODY_LIMIT);
    assert!(
        body.ends_with("2026-11-20.json에 있습니다.\n"),
        "{}",
        &body[body.len() - 200..]
    );
}

#[test]
fn capture_turned_off_after_the_projects_were_listed_writes_nothing() {
    let f = setup();
    f.write("2026-11-20", 1, "첫 판");
    let projects = f.store.originals_projects().unwrap();
    let project = projects.iter().find(|p| p.id == f.project).unwrap();
    // Between listing the projects and writing, the owner turns capture off.
    f.store.set_chat_capture(&f.project, false).unwrap();
    let path = f.records(&f.key).join("2026-11-20.json");
    assert!(matches!(
        f.store.show_record(project, "2026-11-20", &path).unwrap(),
        Shown::OtherFolder
    ));
    assert!(f.shown("2026-11-20").is_empty());
}
