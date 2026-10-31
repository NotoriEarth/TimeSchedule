//! Deterministic synthetic data only. Refuses to overwrite a database.
//! cargo run -p timeschedule-core --example seed_benchmark -- --size small --output DIR
//! Use scripts/tests/benchmark-fixtures.tests.ps1 to generate paired fixtures and SHA256 metadata.
use chrono::{Duration, NaiveDate};
use rusqlite::{params, types::ValueRef, Connection, OpenFlags};
use serde_json::{json, Value};
use std::{fs, io::Write, path::Path};
use timeschedule_core::storage::Storage;

const UPDATED: &str = "2026-10-29T00:00:00Z";
const ANCHOR: &str = "2026-10-29";
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn id(kind: u8, index: usize) -> String {
    format!("{kind:08x}-0000-4000-8000-{index:012x}")
}

#[derive(Clone, Copy)]
struct Counts {
    events: usize,
    entries: usize,
    projects: usize,
    tasks: usize,
    notes: usize,
}
fn counts(size: &str) -> Result<Counts> {
    match size {
        "small" => Ok(Counts {
            events: 1000,
            entries: 2000,
            projects: 10,
            tasks: 100,
            notes: 20,
        }),
        "large" => Ok(Counts {
            events: 10000,
            entries: 20000,
            projects: 50,
            tasks: 1000,
            notes: 100,
        }),
        _ => Err("--size must be small or large".into()),
    }
}
fn main() {
    if let Err(error) = run() {
        eprintln!("benchmark fixture: {error}");
        std::process::exit(1);
    }
}
fn run() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 4 || args[0] != "--size" || args[2] != "--output" {
        return Err("usage: seed_benchmark --size small|large --output NEW_DIRECTORY".into());
    }
    let size = &args[1];
    let count = counts(size)?;
    let directory = Path::new(&args[3]);
    fs::create_dir_all(directory)?;
    let database = directory.join("schedule.db");
    for name in [
        "schedule.db",
        "schedule.db-wal",
        "schedule.db-shm",
        "logical.jsonl",
        "metadata.json",
    ] {
        if directory.join(name).exists() {
            return Err(format!("refusing to overwrite {}", directory.join(name).display()).into());
        }
    }
    // Atomic create prevents a concurrent generator from claiming the same target.
    drop(
        fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&database)?,
    );
    drop(Storage::open(&database)?);
    let mut connection = Connection::open(&database)?;
    connection.execute_batch("PRAGMA foreign_keys=ON; PRAGMA synchronous=FULL;")?;
    let transaction = connection.transaction()?;
    let anchor = NaiveDate::parse_from_str(ANCHOR, "%Y-%m-%d")?;
    let date = |index: usize| (anchor + Duration::days((index % 365) as i64 - 182)).to_string();
    for i in 0..count.projects {
        transaction.execute(
            "INSERT INTO projects VALUES (?1,?2,?3,'',?4)",
            params![
                id(1, i),
                format!("프로젝트 {i:04}"),
                format!("합성 프로젝트 설명 {i:04}. 개발 기록과 트러블슈팅을 정리합니다."),
                UPDATED
            ],
        )?;
    }
    for i in 0..count.events {
        transaction.execute(
            "INSERT INTO events VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
            params![
                id(2, i),
                format!("일정 {i:06} 학습 및 개발"),
                date(i),
                if i % 3 == 0 { None } else { Some("09:00") },
                if i % 3 == 0 { None } else { Some("10:00") },
                i % 3 == 0,
                format!(
                    "일정 메모 {i:06}. SQL 학습과 개발 계획 확인. https://example.com/event/{i}"
                ),
                ["sage", "blue", "amber", "rose"][i % 4],
                i % 5 == 0,
                UPDATED
            ],
        )?;
    }
    let long_body = format!(
        "장문검증 {}",
        "가".repeat(100000 - "장문검증 ".chars().count())
    );
    let kinds = [
        "note",
        "development",
        "troubleshooting",
        "commit",
        "summary",
        "portfolio",
    ];
    for i in 0..count.entries {
        let body = if i == 0 {
            long_body.clone()
        } else {
            format!("개발 기록 {i:06}\n문제: 테스트 입력을 점검했습니다.\n원인: 합성 데이터의 경계 조건입니다.\n해결: 날짜와 저장 결과를 검증했습니다.\n배운 내용: 재현 가능한 근거를 남깁니다.\n참고 https://example.com/entry/{i}\n{}", "학습 내용과 작업 내역을 기록합니다. ".repeat(6))
        };
        transaction.execute(
            "INSERT INTO diary_entries VALUES (?1,?2,?3,?4,?5,?6,?7,NULL,'',?8)",
            params![
                id(3, i),
                id(1, i % count.projects),
                if i == 0 { ANCHOR.to_string() } else { date(i) },
                kinds[i % 6],
                if i == 0 {
                    "장문검증 기록".to_string()
                } else {
                    format!("개발내역 {i:06}")
                },
                body,
                format!("https://example.com/entry/{i}"),
                UPDATED
            ],
        )?;
    }
    for i in 0..count.tasks {
        transaction.execute(
            "INSERT INTO tasks VALUES (?1,?2,?3,?4,?5)",
            params![
                id(4, i),
                format!("할 일 {i:06}"),
                if i % 4 == 0 { None } else { Some(date(i)) },
                i % 5 == 0,
                UPDATED
            ],
        )?;
    }
    for i in 0..count.notes {
        transaction.execute(
            "INSERT INTO notes VALUES (?1,?2,?3,?4)",
            params![
                id(5, i),
                format!("자유 메모 {i:06}"),
                format!(
                    "메모검색 {i:06}\n{}",
                    "학습 노트와 자유 필기입니다.\n".repeat(if i == 0 { 500 } else { 10 })
                ),
                UPDATED
            ],
        )?;
    }
    transaction.commit()?;
    connection.execute_batch("INSERT INTO diary_search(diary_search) VALUES('optimize'); PRAGMA wal_checkpoint(TRUNCATE);")?;
    drop(connection);

    // Reopen through the application's actual storage API, exercising schema and search compatibility.
    let storage = Storage::open(&database)?;
    if storage.search_diary("장문검증", 50)?.len() != 1
        || storage.search_diary("메모검색", 50)?.is_empty()
    {
        return Err("Korean FTS search did not return generated records".into());
    }
    let entry = storage.get_entry(&id(3, 0))?;
    if entry.body.chars().count() != 100000 || storage.list_projects()?.len() != count.projects {
        return Err("storage reopen data mismatch".into());
    }
    drop(storage);
    let connection = Connection::open_with_flags(&database, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let integrity: String = connection.query_row("PRAGMA integrity_check", [], |r| r.get(0))?;
    if integrity != "ok" {
        return Err(format!("integrity_check: {integrity}").into());
    }
    if connection.prepare("PRAGMA foreign_key_check")?.exists([])? {
        return Err("foreign key check failed".into());
    }
    let expected = [
        ("events", count.events),
        ("diary_entries", count.entries),
        ("projects", count.projects),
        ("tasks", count.tasks),
        ("notes", count.notes),
    ];
    for (table, expected_count) in expected {
        let actual: usize =
            connection.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))?;
        if actual != expected_count {
            return Err(format!("{table}: {actual} != {expected_count}").into());
        }
    }
    let fts_count: usize =
        connection.query_row("SELECT count(*) FROM diary_search", [], |r| r.get(0))?;
    if fts_count != count.events + count.entries + count.projects + count.tasks + count.notes {
        return Err("FTS count mismatch".into());
    }
    let mut logical = std::io::BufWriter::new(
        fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(directory.join("logical.jsonl"))?,
    );
    for table in ["events", "diary_entries", "projects", "tasks", "notes"] {
        let mut statement = connection.prepare(&format!("SELECT * FROM {table} ORDER BY id"))?;
        let columns = statement.column_count();
        let mut rows = statement.query([])?;
        while let Some(row) = rows.next()? {
            let values = (0..columns)
                .map(|i| -> Result<Value> {
                    Ok(match row.get_ref(i)? {
                        ValueRef::Null => Value::Null,
                        ValueRef::Integer(v) => json!(v),
                        ValueRef::Text(v) => json!(std::str::from_utf8(v)?),
                        _ => return Err("unexpected fixture value type".into()),
                    })
                })
                .collect::<Result<Vec<_>>>()?;
            serde_json::to_writer(&mut logical, &json!({"table":table,"row":values}))?;
            logical.write_all(b"\n")?;
        }
    }
    logical.flush()?;
    drop(logical);
    drop(connection);
    // Verify the database can be copied without any WAL sidecar.
    if directory.join("schedule.db-wal").exists()
        && fs::metadata(directory.join("schedule.db-wal"))?.len() != 0
    {
        return Err("nonempty WAL remains after closing all connections".into());
    }
    let metadata = json!({"fixtureVersion":1,"size":size,"anchorDate":ANCHOR,"updatedAt":UPDATED,"schemaVersion":2,"events":count.events,"entries":count.entries,"projects":count.projects,"tasks":count.tasks,"notes":count.notes,"dateDistribution":"365 days centered on 2026-10-29; first entry on anchor","longestEntryCharacters":100000,"logicalFile":"logical.jsonl","databaseFile":"schedule.db","integrityCheck":"ok","foreignKeyCheck":"ok","ftsRows":fts_count,"koreanSearchVerified":true,"walCheckpointed":true,"hashNote":"SHA256 added by benchmark-fixtures.tests.ps1 after deterministic regeneration check"});
    fs::write(
        directory.join("metadata.json"),
        serde_json::to_vec_pretty(&metadata)?,
    )?;
    println!("{}", serde_json::to_string(&metadata)?);
    Ok(())
}
