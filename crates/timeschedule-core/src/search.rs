//! Global search over events, tasks, notes, projects and diary entries.
//!
//! `diary_search` is one FTS5 trigram index. Since schema 6, `search_order` holds each
//! index row's sort key (`updated_at`, `id`) so results can be ordered newest-first
//! without reading the large text of every match. Both use `source rowid * 5 + offset`
//! as their rowid, kept in step by the triggers defined here.
//!
//! All search SQL lives in this module. The step functions of past schema versions are
//! frozen: a database at version N runs every later step in order, so editing an old
//! step would change what existing databases become. Change the index with a new step.
use crate::diary::SearchHit;
use crate::storage::{database_error, validate_text, Storage};
use rusqlite::{params, Connection, OptionalExtension};

/// Rows a short query checks newest-first before falling back to a full scan.
const RECENT_WINDOW: u32 = 1000;
const COLUMNS: &str = "s.id,s.kind,s.title,substr(s.body,1,160),s.date,s.project_id";

/// Schema 2 (frozen). Trigrams index literal Korean substrings of three or more
/// characters; shorter queries scan.
pub(crate) fn create_v2(connection: &Connection) -> Result<(), String> {
    connection.execute_batch("CREATE VIRTUAL TABLE diary_search USING fts5(kind UNINDEXED, id UNINDEXED, title, body, date UNINDEXED, project_id UNINDEXED, updated_at UNINDEXED, tokenize='trigram');").map_err(database_error)?;
    for (offset, (table, kind, body, date, project)) in [
        ("events", "event", "notes", "date", "NULL"),
        ("tasks", "task", "''", "date", "NULL"),
        ("notes", "note", "body", "NULL", "NULL"),
        ("projects", "project", "description", "NULL", "id"),
        ("diary_entries", "entry", "body", "date", "project_id"),
    ]
    .into_iter()
    .enumerate()
    {
        let expr = |field: &str| {
            if field == "NULL" || field == "''" {
                field.to_string()
            } else {
                format!("new.{field}")
            }
        };
        let values = format!(
            "new.rowid*5+{offset}, '{kind}', new.id, new.title, {}, {}, {}, new.updated_at",
            expr(body),
            expr(date),
            expr(project)
        );
        connection.execute_batch(&format!(
            "INSERT INTO diary_search(rowid,kind,id,title,body,date,project_id,updated_at) SELECT rowid*5+{offset},'{kind}',id,title,{body},{date},{project},updated_at FROM {table};
             CREATE TRIGGER {table}_search_insert AFTER INSERT ON {table} BEGIN INSERT INTO diary_search(rowid,kind,id,title,body,date,project_id,updated_at) VALUES ({values}); END;
             CREATE TRIGGER {table}_search_delete AFTER DELETE ON {table} BEGIN DELETE FROM diary_search WHERE rowid=old.rowid*5+{offset}; END;
             CREATE TRIGGER {table}_search_update AFTER UPDATE ON {table} BEGIN DELETE FROM diary_search WHERE rowid=old.rowid*5+{offset}; INSERT INTO diary_search(rowid,kind,id,title,body,date,project_id,updated_at) VALUES ({values}); END;"
        )).map_err(database_error)?;
    }
    Ok(())
}

/// Schema 4 (frozen). Task bodies come from `task_details`; both old clients updating a
/// task and native detail edits retain the full search text.
pub(crate) fn task_details_v4(c: &Connection) -> Result<(), String> {
    c.execute_batch("DROP TRIGGER tasks_search_update;
        CREATE TRIGGER tasks_search_update AFTER UPDATE ON tasks BEGIN
          DELETE FROM diary_search WHERE rowid=old.rowid*5+1;
          INSERT INTO diary_search(rowid,kind,id,title,body,date,project_id,updated_at) VALUES(new.rowid*5+1,'task',new.id,new.title,COALESCE((SELECT body FROM task_details WHERE task_id=new.id),''),new.date,NULL,new.updated_at);
        END;
        CREATE TRIGGER task_details_search_insert AFTER INSERT ON task_details BEGIN
          UPDATE diary_search SET body=new.body WHERE kind='task' AND id=new.task_id;
        END;
        CREATE TRIGGER task_details_search_update AFTER UPDATE ON task_details BEGIN
          UPDATE diary_search SET body=new.body WHERE kind='task' AND id=new.task_id;
        END;").map_err(database_error)
}

/// Schema 6: adds `search_order` and replaces every search trigger. Task detail edits
/// now address their index row by rowid; the v4 triggers filtered on unindexed FTS
/// columns and so read the whole index on every save.
pub(crate) fn migrate_v6(c: &Connection) -> Result<(), String> {
    let mut sql = String::from(
        "CREATE TABLE search_order(rowid INTEGER PRIMARY KEY, updated_at TEXT NOT NULL, id TEXT NOT NULL);
         INSERT INTO search_order SELECT rowid,updated_at,id FROM diary_search;
         CREATE INDEX search_order_recent ON search_order(updated_at DESC,id);
         DROP TRIGGER task_details_search_insert; DROP TRIGGER task_details_search_update;",
    );
    for (offset, (table, kind, body, date, project)) in [
        ("events", "event", "new.notes", "new.date", "NULL"),
        (
            "tasks",
            "task",
            "COALESCE((SELECT body FROM task_details WHERE task_id=new.id),'')",
            "new.date",
            "NULL",
        ),
        ("notes", "note", "new.body", "NULL", "NULL"),
        ("projects", "project", "new.description", "NULL", "new.id"),
        (
            "diary_entries",
            "entry",
            "new.body",
            "new.date",
            "new.project_id",
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let add = format!(
            "INSERT INTO diary_search(rowid,kind,id,title,body,date,project_id,updated_at) VALUES(new.rowid*5+{offset},'{kind}',new.id,new.title,{body},{date},{project},new.updated_at);
             INSERT INTO search_order VALUES(new.rowid*5+{offset},new.updated_at,new.id);"
        );
        let remove = format!(
            "DELETE FROM diary_search WHERE rowid=old.rowid*5+{offset};
             DELETE FROM search_order WHERE rowid=old.rowid*5+{offset};"
        );
        sql += &format!(
            "DROP TRIGGER {table}_search_insert; DROP TRIGGER {table}_search_delete; DROP TRIGGER {table}_search_update;
             CREATE TRIGGER {table}_search_insert AFTER INSERT ON {table} BEGIN {add} END;
             CREATE TRIGGER {table}_search_delete AFTER DELETE ON {table} BEGIN {remove} END;
             CREATE TRIGGER {table}_search_update AFTER UPDATE ON {table} BEGIN {remove} {add} END;"
        );
    }
    for event in ["insert", "update"] {
        sql += &format!(
            "CREATE TRIGGER task_details_search_{event} AFTER {event} ON task_details BEGIN
               UPDATE diary_search SET body=new.body WHERE rowid=(SELECT rowid*5+1 FROM tasks WHERE id=new.task_id);
             END;"
        );
    }
    sql += "PRAGMA user_version=6;";
    c.execute_batch(&sql).map_err(database_error)
}

fn hits(
    c: &Connection,
    sql: &str,
    params: impl rusqlite::Params,
) -> Result<Vec<SearchHit>, String> {
    let mut q = c.prepare_cached(sql).map_err(database_error)?;
    let rows = q
        .query_map(params, |r| {
            Ok(SearchHit {
                id: r.get(0)?,
                kind: r.get(1)?,
                title: r.get(2)?,
                excerpt: r.get(3)?,
                date: r.get(4)?,
                project_id: r.get(5)?,
            })
        })
        .map_err(database_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(database_error)?;
    Ok(rows)
}

impl Storage {
    /// Newest-updated first (ties by id), at most `limit` (1..=50) hits with a short
    /// excerpt; full bodies stay in the database.
    pub fn search_diary(&self, query: &str, limit: u32) -> Result<Vec<SearchHit>, String> {
        let query = query.trim();
        if query.is_empty() {
            return Ok(vec![]);
        }
        validate_text(query, "검색어", 200)?;
        let limit = limit.clamp(1, 50);
        let c = self.search_connection()?;
        if query.chars().count() >= 3 {
            // The trigram index yields matching rowids without reading text, the order
            // table sorts them, and only the returned rows' text is read.
            let phrase = format!("\"{}\"", query.replace('"', "\"\""));
            return hits(
                &c,
                &format!(
                    "WITH found AS (SELECT o.rowid AS rid,o.updated_at,o.id FROM search_order o WHERE o.rowid IN (SELECT rowid FROM diary_search WHERE diary_search MATCH ?1) ORDER BY o.updated_at DESC,o.id LIMIT ?2)
                     SELECT {COLUMNS} FROM found f CROSS JOIN diary_search s ON s.rowid=f.rid ORDER BY f.updated_at DESC,f.id"
                ),
                params![phrase, limit],
            );
        }
        // Shorter than a trigram: no index applies, so compare substrings. SQLite's
        // lower() folds only ASCII letters, so it is applied only when there are some.
        let contains = if query.bytes().any(|b| b.is_ascii_alphabetic()) {
            "(instr(lower(s.title),lower(?1))>0 OR instr(lower(s.body),lower(?1))>0)"
        } else {
            "(instr(s.title,?1)>0 OR instr(s.body,?1)>0)"
        };
        let scan = format!("SELECT {COLUMNS} FROM diary_search s CROSS JOIN search_order o ON o.rowid=s.rowid WHERE {contains} ORDER BY o.updated_at DESC,o.id LIMIT ?2");
        // The first row after the window; none means one scan covers every row.
        let Some((after_at, after_id)) = c
            .query_row(
                "SELECT updated_at,id FROM search_order ORDER BY updated_at DESC,id LIMIT 1 OFFSET ?1",
                [RECENT_WINDOW],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
            )
            .optional()
            .map_err(database_error)?
        else {
            return hits(&c, &scan, params![query, limit]);
        };
        // Common words match among the newest rows: walk them in order and stop at
        // `limit`. The window is a prefix of the order, so every row outside it sorts
        // after every row inside, and a full page of matches inside is the answer.
        let recent = hits(
            &c,
            &format!("SELECT {COLUMNS} FROM search_order o INDEXED BY search_order_recent CROSS JOIN diary_search s ON s.rowid=o.rowid WHERE o.updated_at>=?3 AND (o.updated_at>?3 OR o.id<?4) AND {contains} ORDER BY o.updated_at DESC,o.id LIMIT ?2"),
            params![query, limit, after_at, after_id],
        )?;
        if recent.len() == limit as usize {
            return Ok(recent);
        }
        // Rare words: a sequential scan reads the text in storage order, faster than
        // visiting every row by recency.
        hits(&c, &scan, params![query, limit])
    }
}

#[cfg(test)]
mod tests {
    use crate::calendar::TaskDetails;
    use crate::storage::{migrate_to, NoteInput, Storage, TaskInput};
    use rusqlite::{params, Connection};
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
    use std::sync::{mpsc, Arc};
    use std::time::Duration;
    use uuid::Uuid;

    struct TempDb(PathBuf);
    impl TempDb {
        fn new() -> Self {
            Self(std::env::temp_dir().join(format!("timeschedule-search-{}.db", Uuid::new_v4())))
        }
    }
    impl Drop for TempDb {
        fn drop(&mut self) {
            let dir = self.0.parent().unwrap().to_path_buf();
            let stem = self.0.file_stem().unwrap().to_string_lossy().to_string();
            for entry in std::fs::read_dir(dir).unwrap().flatten() {
                if entry.file_name().to_string_lossy().starts_with(&stem) {
                    let _ = std::fs::remove_file(entry.path());
                }
            }
        }
    }

    fn id(kind: u32, n: usize) -> String {
        format!("{kind:08x}-0000-4000-8000-{n:012x}")
    }
    fn stamp(n: usize) -> String {
        format!("2026-01-01T00:00:{:02}.{:03}Z", n / 1000 % 60, n % 1000)
    }

    /// Bulk rows through the app's own triggers, in one transaction.
    fn entries(
        store: &Storage,
        range: std::ops::Range<usize>,
        body: &str,
        at: fn(usize) -> String,
    ) {
        let mut c = store.connection().unwrap();
        let tx = c.transaction().unwrap();
        for n in range {
            tx.execute(
                "INSERT INTO diary_entries(id,project_id,date,kind,title,body,source_url,updated_at) VALUES(?1,NULL,'2026-11-01','development',?2,?3,'',?4)",
                params![id(5, n), format!("기록 {n}"), body, at(n)],
            )
            .unwrap();
        }
        tx.commit().unwrap();
    }

    /// SQLite VM work (in units of 10 instructions) that `f` spends on either connection.
    fn work<T>(store: &Storage, f: impl FnOnce() -> T) -> (u64, T) {
        let counter = Arc::new(AtomicU64::new(0));
        let install = |c: &Connection| {
            let k = Arc::clone(&counter);
            c.progress_handler(
                10,
                Some(move || {
                    k.fetch_add(1, Relaxed);
                    false
                }),
            );
        };
        install(&store.connection().unwrap());
        install(&store.search_connection().unwrap());
        let value = f();
        store
            .connection()
            .unwrap()
            .progress_handler(10, None::<fn() -> bool>);
        store
            .search_connection()
            .unwrap()
            .progress_handler(10, None::<fn() -> bool>);
        (counter.load(Relaxed), value)
    }

    /// Database pages the search connection reads from the file (page-cache misses)
    /// during `f`, with a cache too small to hide re-reads. Long text spills into
    /// overflow pages, so this counts how much record text a search reads.
    fn pages_read<T>(store: &Storage, f: impl FnOnce() -> T) -> (i32, T) {
        let status = |reset: i32| {
            let c = store.search_connection().unwrap();
            let (mut current, mut highest) = (0, 0);
            // SAFETY: a live connection handle and valid out-pointers.
            let code = unsafe {
                rusqlite::ffi::sqlite3_db_status(
                    c.handle(),
                    rusqlite::ffi::SQLITE_DBSTATUS_CACHE_MISS,
                    &mut current,
                    &mut highest,
                    reset,
                )
            };
            assert_eq!(code, rusqlite::ffi::SQLITE_OK);
            current
        };
        store
            .search_connection()
            .unwrap()
            .execute_batch("PRAGMA cache_size=8")
            .unwrap();
        status(1);
        let value = f();
        (status(0), value)
    }

    fn task(
        store: &Storage,
        body: &str,
        previous: Option<&crate::calendar::TaskEdit>,
    ) -> crate::calendar::TaskEdit {
        store
            .save_calendar_task(
                TaskInput {
                    id: previous.map(|p| p.task.id.clone()),
                    title: "측정 할 일".into(),
                    date: Some("2026-11-02".into()),
                    completed: false,
                },
                TaskDetails {
                    body: body.into(),
                    ..TaskDetails::default()
                },
                previous,
            )
            .unwrap()
    }

    #[test]
    fn saving_task_details_does_not_scan_the_whole_search_index() {
        let store = Storage::open(":memory:").unwrap();
        let first = task(&store, "처음본문", None);
        entries(&store, 0..100, "무관한 본문", stamp);
        let (small, second) = work(&store, || task(&store, "둘째본문", Some(&first)));
        entries(&store, 100..4000, "무관한 본문", stamp);
        let (large, _) = work(&store, || task(&store, "셋째본문", Some(&second)));
        assert!(
            large < small * 2,
            "work grew with index size: {small} -> {large}"
        );
        let hits = store.search_diary("셋째본문", 50).unwrap();
        assert_eq!((hits.len(), hits[0].kind.as_str()), (1, "task"));
        assert!(store.search_diary("둘째본문", 50).unwrap().is_empty());
    }

    #[test]
    fn indexed_search_reads_only_the_text_it_returns() {
        let db = TempDb::new();
        let store = Storage::open(&db.0).unwrap();
        // ~18 KB per body: every record spills into several overflow pages.
        let body = "주간 회의록 정리 내용입니다 ".repeat(400);
        let fill = |range: std::ops::Range<usize>| {
            let mut c = store.connection().unwrap();
            let tx = c.transaction().unwrap();
            for n in range {
                tx.execute(
                    "INSERT INTO diary_entries(id,project_id,date,kind,title,body,source_url,updated_at) VALUES(?1,NULL,'2026-11-01','development',?2,?3,'',?4)",
                    params![id(5, n), format!("기록 {n}"), body, stamp(n)],
                )
                .unwrap();
            }
            tx.commit().unwrap();
        };
        fill(0..100);
        let (small, hits) = pages_read(&store, || store.search_diary("회의록", 50).unwrap());
        assert_eq!(hits.len(), 50);
        fill(100..1000);
        let (large, hits) = pages_read(&store, || store.search_diary("회의록", 50).unwrap());
        assert!(
            large < small * 2,
            "pages read grew with match count: {small} -> {large}"
        );
        // Newest first: the last inserted rows carry the latest timestamps.
        assert_eq!(hits[0].title, "기록 999");
        assert_eq!(hits[49].title, "기록 950");
    }

    #[test]
    fn common_short_queries_stop_at_the_newest_matches() {
        let store = Storage::open(":memory:").unwrap();
        entries(&store, 0..200, "주간 회의 정리", stamp);
        let (small, _) = work(&store, || store.search_diary("회의", 50).unwrap());
        entries(&store, 200..4000, "주간 회의 정리", stamp);
        let (large, hits) = work(&store, || store.search_diary("회의", 50).unwrap());
        assert!(
            large < small * 3,
            "short query scanned everything: {small} -> {large}"
        );
        assert_eq!(hits[0].title, "기록 3999");
        assert_eq!(hits.len(), 50);
    }

    #[test]
    fn a_window_covering_every_row_is_not_scanned_twice() {
        let store = Storage::open(":memory:").unwrap();
        let window = super::RECENT_WINDOW as usize;
        entries(&store, 0..window - 1, "무관한 본문", stamp);
        let (under, _) = work(&store, || store.search_diary("견적", 50).unwrap());
        entries(&store, window - 1..window, "무관한 본문", stamp);
        let (full, _) = work(&store, || store.search_diary("견적", 50).unwrap());
        assert!(full < under * 3 / 2, "{under} -> {full}");
    }

    #[test]
    fn rows_tied_across_the_window_edge_keep_id_order() {
        let store = Storage::open(":memory:").unwrap();
        let same = |_| "2026-03-03T00:00:00.000Z".to_string();
        entries(&store, 0..980, "무관한 본문", same);
        entries(&store, 980..1200, "견적 요청", same);
        let ids: Vec<_> = store
            .search_diary("견적", 50)
            .unwrap()
            .into_iter()
            .map(|h| h.id)
            .collect();
        assert_eq!(ids, (980..1030).map(|n| id(5, n)).collect::<Vec<_>>());
        entries(&store, 1200..1300, "견적 요청", same);
        let ids: Vec<_> = store
            .search_diary("견적", 50)
            .unwrap()
            .into_iter()
            .map(|h| h.id)
            .collect();
        assert_eq!(ids, (980..1030).map(|n| id(5, n)).collect::<Vec<_>>());
        // A full page inside the window (ids 900..999 of 0..999) returns early, still in
        // id order, although tied rows continue past the window edge.
        let store = Storage::open(":memory:").unwrap();
        entries(&store, 0..900, "무관한 본문", same);
        entries(&store, 900..1300, "견적 요청", same);
        let (early, hits) = work(&store, || store.search_diary("견적", 50).unwrap());
        let ids: Vec<_> = hits.into_iter().map(|h| h.id).collect();
        assert_eq!(ids, (900..950).map(|n| id(5, n)).collect::<Vec<_>>());
        let (full, _) = work(&store, || store.search_diary("꿹뷁", 50).unwrap());
        assert!(
            early < full,
            "early page did not stop: {early} vs full {full}"
        );
    }

    #[test]
    fn short_queries_still_find_old_matches_in_recency_order() {
        let store = Storage::open(":memory:").unwrap();
        // Three old matches, then far more newer rows than any recent window.
        entries(&store, 0..3, "견적 요청", stamp);
        entries(&store, 3..2600, "무관한 본문", stamp);
        let hits = store.search_diary("견적", 50).unwrap();
        let titles: Vec<_> = hits.iter().map(|h| h.title.as_str()).collect();
        assert_eq!(titles, ["기록 2", "기록 1", "기록 0"]);
    }

    #[test]
    fn ties_order_by_id_and_all_kinds_are_searchable() {
        let store = Storage::open(":memory:").unwrap();
        let same = |_| "2026-02-02T00:00:00.000Z".to_string();
        entries(&store, 0..3, "동일시각 회의록", same);
        store
            .save_note(NoteInput {
                id: None,
                title: "메모 회의록".into(),
                body: "Rust PR 검토".into(),
            })
            .unwrap();
        let hits = store.search_diary("동일시각", 50).unwrap();
        let ids: Vec<_> = hits.iter().map(|h| h.id.clone()).collect();
        assert_eq!(ids, [id(5, 0), id(5, 1), id(5, 2)]);
        let short = store.search_diary("동일", 50).unwrap();
        assert_eq!(short.iter().map(|h| h.id.clone()).collect::<Vec<_>>(), ids);
        // ASCII stays case-insensitive on both paths; the note is the newest row.
        assert_eq!(store.search_diary("pr", 50).unwrap()[0].kind, "note");
        assert_eq!(store.search_diary("rust", 50).unwrap()[0].kind, "note");
        assert_eq!(store.search_diary("회의록", 50).unwrap()[0].kind, "note");
    }

    #[test]
    fn search_index_follows_records_after_rowids_are_renumbered() {
        let db = TempDb::new();
        let store = Storage::open(&db.0).unwrap();
        let mut notes = vec![];
        for n in 0..6 {
            notes.push(
                store
                    .save_note(NoteInput {
                        id: None,
                        title: format!("메모번호{n}번"),
                        body: String::new(),
                    })
                    .unwrap(),
            );
        }
        for note in &notes[..3] {
            store.delete_note(&note.id).unwrap();
        }
        // Index rows are addressed by `source rowid * 5 + offset`; this guards that a
        // VACUUM after deletions keeps that mapping valid (it passed before schema 6 too).
        store.connection().unwrap().execute_batch("VACUUM").unwrap();
        store
            .save_note(NoteInput {
                id: Some(notes[3].id.clone()),
                title: "바뀐제목입니다".into(),
                body: String::new(),
            })
            .unwrap();
        assert_eq!(store.search_diary("바뀐제목", 50).unwrap().len(), 1);
        assert!(store.search_diary("메모번호3번", 50).unwrap().is_empty());
        for (n, note) in notes.iter().enumerate().skip(4) {
            let hits = store.search_diary(&format!("메모번호{n}번"), 50).unwrap();
            assert_eq!(hits.len(), 1, "note {n} vanished from search");
            assert_eq!(hits[0].id, note.id);
        }
    }

    #[test]
    fn version_5_databases_are_indexed_for_the_new_search() {
        let db = TempDb::new();
        let mut raw = Connection::open(&db.0).unwrap();
        migrate_to(&mut raw, 5).unwrap();
        raw.execute_batch(&format!(
            "INSERT INTO events VALUES('{}','행사제목','2026-11-01',NULL,NULL,1,'행사본문내용','sage',0,'2026-01-01T00:00:01.000Z');
             INSERT INTO tasks VALUES('{}','할일제목','2026-11-02',0,'2026-01-01T00:00:02.000Z');
             INSERT INTO task_details VALUES('{}','할일상세본문',1,NULL,NULL);
             INSERT INTO notes VALUES('{}','메모제목','메모본문내용','2026-01-01T00:00:03.000Z');
             INSERT INTO projects VALUES('{}','프로젝트제목','프로젝트설명','','2026-01-01T00:00:04.000Z');
             INSERT INTO diary_entries(id,project_id,date,kind,title,body,source_url,updated_at) VALUES('{}',NULL,'2026-11-03','note','기록제목','기록본문내용','','2026-01-01T00:00:05.000Z');",
            id(1, 1), id(2, 1), id(2, 1), id(3, 1), id(4, 1), id(5, 1)
        ))
        .unwrap();
        drop(raw);
        let store = Storage::open(&db.0).unwrap();
        for (query, kind) in [
            ("행사본문", "event"),
            ("할일상세", "task"),
            ("메모본문", "note"),
            ("프로젝트설명", "project"),
            ("기록본문", "entry"),
        ] {
            let hits = store.search_diary(query, 50).unwrap();
            assert_eq!(hits.len(), 1, "{query}");
            assert_eq!(hits[0].kind, kind);
        }
        let kinds: Vec<_> = store
            .search_diary("본문", 50)
            .unwrap()
            .into_iter()
            .map(|h| h.kind)
            .collect();
        assert_eq!(kinds, ["entry", "note", "task", "event"]);
        let c = store.connection().unwrap();
        let version: i64 = c
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version, 7);
        drop(c);
        let stem = db.0.file_stem().unwrap().to_string_lossy().to_string();
        let backups = std::fs::read_dir(db.0.parent().unwrap())
            .unwrap()
            .flatten()
            .filter(|e| {
                let name = e.file_name().to_string_lossy().to_string();
                name.starts_with(&stem) && name.contains(".pre-v7-")
            })
            .count();
        assert_eq!(backups, 1);
    }

    #[test]
    fn search_does_not_wait_for_the_shared_write_connection() {
        let db = TempDb::new();
        let store = Arc::new(Storage::open(&db.0).unwrap());
        store
            .save_note(NoteInput {
                id: None,
                title: "대기없는검색".into(),
                body: String::new(),
            })
            .unwrap();
        let held = store.connection().unwrap();
        let searching = Arc::clone(&store);
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(searching.search_diary("대기없는검색", 50).map(|h| h.len()));
        });
        let result = rx.recv_timeout(Duration::from_secs(5));
        drop(held);
        assert_eq!(
            result.expect("search waited for the write connection"),
            Ok(1)
        );
        // A write committed on the main connection is visible to the next search.
        store
            .save_note(NoteInput {
                id: None,
                title: "방금저장한메모".into(),
                body: String::new(),
            })
            .unwrap();
        assert_eq!(store.search_diary("방금저장", 50).unwrap().len(), 1);
    }
}
