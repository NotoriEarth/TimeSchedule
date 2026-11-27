use super::*;
use crate::{
    calendar::EntryDisplay,
    diary::{Entry, EntryInput, ProjectInput},
    project_record::{gather, summarize},
    reading::Origin,
    terra::{Reply, Usage},
    timeline::TimeChange,
};
use serde_json::{json, Value};
use std::path::PathBuf;

/// A store with one project and its folder, and a document store holding one daily
/// record of the project and a project record made from it.
struct Fixture {
    root: PathBuf,
    store: Storage,
    vault: PathBuf,
    project: String,
    /// The `repoKey` of the project's folder.
    key: String,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn daily_record(project: &str, key: &str) -> Value {
    json!({
        "schemaVersion": 2,
        "project": {"id": project, "title": "앱", "repoKey": key},
        "date": "2026-11-20", "revision": 1, "generatedAt": "2026-11-20T00:00:00Z",
        "model": "m", "effort": "medium", "promptVersion": 4,
        "input": {"sessions": 1, "agentRuns": 0, "blocks": 3, "chars": 10,
                  "fingerprint": "f", "latestArchived": null, "warnings": []},
        "usage": {"seconds": 1.0},
        "checks": {"withoutEvidence": 0, "quotesNotFound": 0,
                   "proposedByUnsupported": 0, "decidedByUnsupported": 0},
        "overview": {"text": "검색을 만든 날", "evidence": []},
        "development": [{"title": "검색", "detail": "검색 색인을 더했다", "evidence": []}],
        "troubleshooting": [{"problem": "느림", "cause": "색인 없음", "fix": null,
                             "verification": "검색 0.3초", "evidence": []}],
        "decisions": [{"decision": "SQLite FTS를 쓴다", "reason": null, "rejected": ["외부 검색 서버"],
                       "proposedBy": "ai", "proposalEvidence": [],
                       "decidedBy": "human_approved", "decisionEvidence": [], "evidence": []}],
        "nextSteps": [{"text": "정렬", "evidence": []}],
        "sources": []
    })
}

/// What the model answers for the bundle of `daily_record`.
fn answer() -> Value {
    json!({
        "overview": {"text": "검색이 되는 앱이다.\n지금은 정렬이 남았다.", "evidence": ["D:2026-11-20:개요:1"]},
        "features": [{"title": "검색", "detail": "색인을 더했다", "evidence": ["D:2026-11-20:개발:1"]}],
        "decisions": [
            {"decision": "외부 검색 서버를 쓴다", "reason": null, "rejected": [],
             "evidence": ["D:2026-11-20:결정:1"]},
            {"decision": "색인을 둔다", "reason": null, "rejected": [],
             "evidence": ["D:2026-11-20:개발:1"]}
        ],
        "troubleshooting": [{"problem": "느림", "cause": "색인 없음", "fix": null,
                             "verification": "검색 0.3초", "evidence": ["D:2026-11-20:트러블:1"]}],
        "results": [{"text": "검색 0.3초", "evidence": ["D:2026-11-20:트러블:1"]}],
        "limits": [{"text": "정렬", "evidence": ["D:2026-11-20:남은일:1"]}]
    })
}

fn setup() -> Fixture {
    let root = std::env::temp_dir().join(format!("ts-project-diary-{}", uuid::Uuid::new_v4()));
    let repo = root.join("repo");
    let vault = root.join("vault");
    fs::create_dir_all(&repo).unwrap();
    fs::create_dir_all(&vault).unwrap();
    let store = Storage::open(":memory:").unwrap();
    let repo = repo.to_string_lossy().into_owned();
    let project = store
        .save_project(ProjectInput {
            id: None,
            title: "앱".into(),
            description: String::new(),
            repo_path: repo.clone(),
        })
        .unwrap()
        .id;
    let key = repo_key(&repo);
    let daily = vault.join("daily").join(group_name(&key));
    fs::create_dir_all(&daily).unwrap();
    fs::write(
        daily.join("2026-11-20.json"),
        daily_record(&project, &key).to_string(),
    )
    .unwrap();
    let f = Fixture {
        root,
        store,
        vault,
        project,
        key,
    };
    let dossier = gather(&f.store, &f.vault, &f.project).unwrap();
    summarize(&f.vault, &dossier, "m", "high", |_, _| {
        Ok(Reply {
            text: answer().to_string(),
            usage: Usage::default(),
            seconds: 1.0,
        })
    })
    .unwrap();
    f
}

impl Fixture {
    fn path(&self) -> PathBuf {
        self.vault
            .join("project")
            .join(group_name(&self.key))
            .join("record.json")
    }

    /// Changes the latest project record as a newer run would leave it.
    fn rewrite(&self, change: impl FnOnce(&mut Value)) {
        let mut record: Value = serde_json::from_slice(&fs::read(self.path()).unwrap()).unwrap();
        change(&mut record);
        fs::write(self.path(), record.to_string()).unwrap();
    }

    /// A newer revision made at `at`, whose overview says `overview`.
    fn newer(&self, revision: u32, at: &str, overview: &str) {
        self.rewrite(|r| {
            r["revision"] = revision.into();
            r["generatedAt"] = at.into();
            r["input"]["fingerprint"] = format!("fp-{revision}").into();
            r["overview"]["text"] = overview.into();
        });
    }

    fn scan(&self) -> DiaryReport {
        self.store.import_project_records(&self.vault).unwrap()
    }

    /// The project's entries of the kind `portfolio`.
    fn shown(&self) -> Vec<Entry> {
        self.store
            .list_entries("2000-01-01", "2100-01-01", Some(&self.project), 0)
            .unwrap()
            .into_iter()
            .filter(|e| e.kind == "portfolio")
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
fn a_project_record_becomes_one_portfolio_entry_that_a_newer_one_replaces_and_moves() {
    let f = setup();
    f.newer(1, "2026-11-26T16:30:00Z", "검색이 되는 앱이다.");
    let report = f.scan();
    assert_eq!((report.added, report.failed), (1, 0));
    let entries = f.shown();
    assert_eq!(entries.len(), 1);
    let entry = &entries[0];
    assert_eq!(entry.title, "프로젝트 기록 · 앱");
    // Made at 01:30 on 11-27 in Korea.
    assert_eq!(entry.date, "2026-11-27");
    let expected = "완료 전 종합

## 개요
검색이 되는 앱이다.

## 기능
- 검색: 색인을 더했다

## 결정
- 외부 검색 서버를 쓴다
  이유: 이유 기록 없음
  하루 기록의 결정:
  - 2026-11-20 [제안 AI · 사람 승인] SQLite FTS를 쓴다 (버린 대안: 외부 검색 서버)
- 색인을 둔다
  이유: 이유 기록 없음
  확인: 제안 알 수 없음 · 확인 기록 없음

## 트러블슈팅
- 느림
  원인: 색인 없음
  확인: 검색 0.3초

## 결과
- 검색 0.3초

## 한계
- 정렬
";
    assert_eq!(entry.body, expected);
    // Nothing new: nothing changes.
    assert_eq!(f.scan().unchanged, 1);
    // A newer record replaces the text and moves the entry to the day it was made.
    f.newer(2, "2026-12-02T03:00:00Z", "둘째 판");
    assert_eq!(f.scan().updated, 1);
    let entries = f.shown();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].id, entry.id);
    assert_eq!(entries[0].date, "2026-12-02");
    assert!(entries[0].body.contains("## 개요\n둘째 판\n"));
    // The owner changed the title only: the text is still the program's.
    f.edit(&entry.id, |e| e.title = "포트폴리오".into());
    f.newer(3, "2026-12-03T03:00:00Z", "셋째 판");
    assert_eq!(f.scan().updated, 1);
    let now = f.store.get_entry(&entry.id).unwrap();
    assert_eq!(now.title, "포트폴리오");
    assert!(now.body.contains("셋째 판"));
}

#[test]
fn an_edited_moved_or_deleted_entry_is_left_alone_and_not_made_again() {
    let f = setup();
    f.scan();
    let id = f.shown().remove(0).id;
    f.edit(&id, |e| e.body = "직접 다듬은 글".into());
    f.newer(2, "2026-12-02T03:00:00Z", "둘째 판");
    assert_eq!(f.scan().kept, 1);
    assert_eq!(f.store.get_entry(&id).unwrap().body, "직접 다듬은 글");
    assert_eq!(f.scan().unchanged, 1);
    // Put back to the text shown, then moved to another day: still the owner's.
    let shown: String = f
        .store
        .connection()
        .unwrap()
        .query_row("SELECT shown_body FROM project_record_entries", [], |r| {
            r.get(0)
        })
        .unwrap();
    f.edit(&id, |e| {
        e.body = shown.clone();
        e.date = "2026-11-01".into();
    });
    f.newer(3, "2026-12-03T03:00:00Z", "셋째 판");
    assert_eq!(f.scan().kept, 1);
    assert_eq!(f.store.get_entry(&id).unwrap().body, shown);
    // Deleted: the receipt says it was shown, so it is not made again.
    f.store.delete_entry(&id).unwrap();
    for _ in 0..2 {
        let report = f.scan();
        assert_eq!((report.added, report.unchanged), (0, 1));
    }
    f.newer(4, "2026-12-04T03:00:00Z", "넷째 판");
    assert_eq!(f.scan().kept, 1);
    assert!(f.shown().is_empty());
}

#[test]
fn the_same_record_shown_another_way_takes_the_new_text_unless_the_owner_changed_it() {
    let f = setup();
    f.scan();
    let entry = f.shown().remove(0);
    let old = "예전 프로그램이 보여 준 글\n";
    let show_old = || {
        let c = f.store.connection().unwrap();
        c.execute(
            "UPDATE diary_entries SET body=?2, source_body=?2 WHERE id=?1",
            params![entry.id, old],
        )
        .unwrap();
        c.execute("UPDATE project_record_entries SET shown_body=?1", [old])
            .unwrap();
    };
    show_old();
    assert_eq!(f.scan().updated, 1);
    assert_eq!(f.store.get_entry(&entry.id).unwrap().body, entry.body);
    assert_eq!(f.scan().unchanged, 1);
    show_old();
    f.edit(&entry.id, |e| e.body = "직접 고친 글".into());
    let report = f.scan();
    assert_eq!((report.unchanged, report.updated, report.kept), (1, 0, 0));
    assert_eq!(f.store.get_entry(&entry.id).unwrap().body, "직접 고친 글");
}

#[test]
fn an_older_revision_or_a_record_of_another_folder_or_project_is_not_shown() {
    let f = setup();
    f.newer(3, "2026-12-03T03:00:00Z", "셋째 판");
    f.scan();
    // A restored backup.
    f.newer(2, "2026-12-02T03:00:00Z", "둘째 판");
    assert_eq!(f.scan().older, 1);
    assert!(f.shown()[0].body.contains("셋째 판"));
    // A file copied in from another folder's records.
    f.rewrite(|r| {
        r["revision"] = 9.into();
        r["project"]["repoKey"] = "c:/other".into();
    });
    assert_eq!(f.scan().other_folder, 1);
    // The record of another project that shares the folder is shown only in that one.
    f.rewrite(|r| {
        r["project"]["repoKey"] = f.key.clone().into();
        r["project"]["id"] = "someone-else".into();
    });
    assert_eq!(f.scan(), DiaryReport::default());
    // Unreadable.
    fs::write(f.path(), "not a record").unwrap();
    assert_eq!(f.scan().failed, 1);
}

#[test]
fn a_store_without_project_records_has_nothing_to_show() {
    let f = setup();
    fs::remove_dir_all(f.vault.join("project")).unwrap();
    assert_eq!(f.scan(), DiaryReport::default());
    // Something that is not a folder where the project's records belong.
    fs::create_dir_all(f.vault.join("project")).unwrap();
    fs::write(f.vault.join("project").join(group_name(&f.key)), "").unwrap();
    assert_eq!(f.scan().failed, 1);
}

#[test]
fn a_project_moved_to_another_folder_no_longer_shows_the_old_folders_record() {
    let f = setup();
    let other = f.root.join("other");
    fs::create_dir_all(&other).unwrap();
    f.store
        .save_project(ProjectInput {
            id: Some(f.project.clone()),
            title: "앱".into(),
            description: String::new(),
            repo_path: other.to_string_lossy().into_owned(),
        })
        .unwrap();
    assert_eq!(f.scan(), DiaryReport::default());
    // Moved between listing the projects and writing.
    let holder = Holder {
        id: f.project.clone(),
        key: f.key.clone(),
    };
    let bytes = fs::read(f.path()).unwrap();
    let record: ProjectRecord = serde_json::from_slice(&bytes).unwrap();
    assert!(matches!(
        f.store
            .show_project_record(&holder, &group_name(&f.key), &record, (&f.path(), &bytes))
            .unwrap(),
        Shown::OtherFolder
    ));
    assert!(f.shown().is_empty());
}

#[test]
fn a_completed_project_record_says_so_and_is_read_as_another_record() {
    let f = setup();
    f.rewrite(|r| r["completedOn"] = "2026-12-01".into());
    f.scan();
    let entry = f.shown().remove(0);
    assert!(entry.body.starts_with("완료 · 2026-12-01\n\n## 개요\n"));
    // It is not one of the records a project record is made from.
    let records = f
        .store
        .read_whole_records(&f.project, &crate::project_record::SUPPORTING)
        .unwrap();
    assert!(records.iter().all(|r| r.id != entry.id));
    let all = f
        .store
        .read_whole_records(&f.project, &Origin::ALL)
        .unwrap();
    let mine = all.iter().find(|r| r.id == entry.id).unwrap();
    assert_eq!(mine.origin, Origin::Other);
}

#[test]
fn opening_a_schema_17_database_backs_it_up_and_adds_the_receipts() {
    let root = std::env::temp_dir().join(format!("ts-project-diary-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&root).unwrap();
    let path = root.join("schedule.db");
    {
        let mut old = Connection::open(&path).unwrap();
        crate::storage::migrate_to(&mut old, 17).unwrap();
        let tables: i64 = old
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE name='project_record_entries'",
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
        c.query_row("SELECT count(*) FROM project_record_entries", [], |r| {
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
    assert_eq!(version, 17);
    drop(backup);
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn a_record_made_again_from_the_same_input_on_another_day_moves_an_untouched_entry() {
    let f = setup();
    f.newer(1, "2026-11-26T03:00:00Z", "첫 답");
    f.scan();
    let id = f.shown().remove(0).id;
    // `record.json` deleted and made again from the same input a day later: the same
    // revision and fingerprint, another answer and another day.
    f.rewrite(|r| {
        r["generatedAt"] = "2026-11-27T03:00:00Z".into();
        r["overview"]["text"] = "다시 만든 답".into();
    });
    assert_eq!(f.scan().updated, 1);
    let now = f.store.get_entry(&id).unwrap();
    assert_eq!(now.date, "2026-11-27");
    assert!(now.body.contains("다시 만든 답"));
    assert_eq!(f.scan().unchanged, 1);
}

#[test]
fn a_record_read_before_the_file_was_replaced_writes_nothing() {
    let f = setup();
    f.scan();
    let id = f.shown().remove(0).id;
    let before = f.store.get_entry(&id).unwrap();
    // A slow showing read this file; meanwhile a run keeps a new one and shows it.
    let stale = fs::read(f.path()).unwrap();
    f.newer(2, "2026-12-02T03:00:00Z", "둘째 판");
    assert_eq!(f.scan().updated, 1);
    let mut record: Value = serde_json::from_slice(&stale).unwrap();
    // The stale read looks like the same revision shown another way.
    record["revision"] = 2.into();
    record["input"]["fingerprint"] = "fp-2".into();
    let record: ProjectRecord = serde_json::from_value(record).unwrap();
    let holder = Holder {
        id: f.project.clone(),
        key: f.key.clone(),
    };
    let path = f.path();
    assert!(matches!(
        f.store
            .show_project_record(&holder, &group_name(&f.key), &record, (&path, &stale))
            .unwrap(),
        Shown::Unchanged
    ));
    let now = f.store.get_entry(&id).unwrap();
    assert!(now.body.contains("둘째 판"), "{}", now.body);
    assert_ne!(now.body, before.body);
}

#[test]
fn a_confirmation_mark_the_model_wrote_is_not_shown_as_one() {
    let f = setup();
    f.rewrite(|r| {
        r["decisions"][1]["decision"] = "[제안 AI · 사람 승인] 외부 서버를 쓴다".into();
        r["features"][0]["detail"] = "[제안 사람 · 사람이 직접] 색인".into();
    });
    f.scan();
    let body = f.shown().remove(0).body;
    assert!(
        body.contains("- (제안 AI · 사람 승인) 외부 서버를 쓴다\n"),
        "{body}"
    );
    assert!(
        body.contains("- 검색: (제안 사람 · 사람이 직접) 색인\n"),
        "{body}"
    );
    // Only the copied daily decision carries the mark.
    assert_eq!(body.matches("[제안 ").count(), 1, "{body}");
}

#[test]
fn a_long_project_name_gives_a_title_the_editor_can_save_again() {
    let f = setup();
    let name = "가".repeat(200);
    f.rewrite(|r| r["project"]["title"] = name.clone().into());
    f.scan();
    let entry = f.shown().remove(0);
    assert_eq!(entry.title.chars().count(), 200);
    assert!(entry.title.starts_with("프로젝트 기록 · 가"));
    f.edit(&entry.id, |e| e.body = "직접 고친 글".into());
    assert_eq!(f.store.get_entry(&entry.id).unwrap().body, "직접 고친 글");
}

#[test]
fn showing_one_project_reports_only_that_project() {
    let f = setup();
    let other_repo = f.root.join("other-repo");
    fs::create_dir_all(&other_repo).unwrap();
    let other = f
        .store
        .save_project(ProjectInput {
            id: None,
            title: "다른 앱".into(),
            description: String::new(),
            repo_path: other_repo.to_string_lossy().into_owned(),
        })
        .unwrap()
        .id;
    let other_dir = f
        .vault
        .join("project")
        .join(group_name(&repo_key(&other_repo.to_string_lossy())));
    fs::create_dir_all(&other_dir).unwrap();
    fs::write(other_dir.join("record.json"), "not a record").unwrap();
    let report = f.store.import_project_record(&f.vault, &f.project).unwrap();
    assert_eq!((report.added, report.failed), (1, 0));
    let report = f.store.import_project_record(&f.vault, &other).unwrap();
    assert_eq!((report.added, report.failed), (0, 1));
}

#[test]
fn a_record_that_cannot_be_read_again_is_a_failure_not_nothing_new() {
    let f = setup();
    let bytes = fs::read(f.path()).unwrap();
    let record: ProjectRecord = serde_json::from_slice(&bytes).unwrap();
    // Where the file was read, something that cannot be read as a file is now.
    fs::remove_file(f.path()).unwrap();
    fs::create_dir(f.path()).unwrap();
    let holder = Holder {
        id: f.project.clone(),
        key: f.key.clone(),
    };
    let path = f.path();
    assert!(f
        .store
        .show_project_record(&holder, &group_name(&f.key), &record, (&path, &bytes))
        .is_err());
    assert!(f.shown().is_empty());
}
