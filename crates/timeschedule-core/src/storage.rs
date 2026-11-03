use chrono::{Datelike, NaiveDate, NaiveTime, SecondsFormat, Utc};
use rusqlite::{params, Connection, Row, TransactionBehavior};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;
use uuid::Uuid;

const SCHEMA_VERSION: i64 = 5;
const MAX_TITLE_LENGTH: usize = 200;
const MAX_TEXT_LENGTH: usize = 20_000;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CalendarEvent {
    pub id: String,
    pub title: String,
    pub date: String,
    pub start_time: Option<String>,
    pub end_time: Option<String>,
    pub all_day: bool,
    pub notes: String,
    pub color: String,
    pub completed: bool,
    pub updated_at: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EventInput {
    pub id: Option<String>,
    pub title: String,
    pub date: String,
    pub start_time: Option<String>,
    pub end_time: Option<String>,
    pub all_day: bool,
    pub notes: String,
    pub color: String,
    pub completed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Task {
    pub id: String,
    pub title: String,
    pub date: Option<String>,
    pub completed: bool,
    pub updated_at: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskInput {
    pub id: Option<String>,
    pub title: String,
    pub date: Option<String>,
    pub completed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Note {
    pub id: String,
    pub title: String,
    pub body: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteInput {
    pub id: Option<String>,
    pub title: String,
    pub body: String,
}

/// One SQLite connection shared by the manager and widget command handlers.
pub struct Storage {
    connection: Mutex<Connection>,
}

impl Storage {
    pub fn get_event(&self, id: &str) -> Result<CalendarEvent, String> {
        validate_id(id)?;
        self.connection()?.query_row(
            "SELECT id,title,date,start_time,end_time,all_day,notes,color,completed,updated_at FROM events WHERE id=?1",
            [id], event_from_row,
        ).map_err(|error| match error {
            rusqlite::Error::QueryReturnedNoRows => "Event not found.".into(),
            other => database_error(other),
        })
    }

    /// Bounded external queries never materialize full event notes or an unbounded result.
    pub fn list_event_headers_limited(
        &self,
        from: &str,
        to: &str,
        limit: usize,
    ) -> Result<(Vec<CalendarEvent>, bool), String> {
        validate_date(from)?;
        validate_date(to)?;
        let start = NaiveDate::parse_from_str(from, "%Y-%m-%d").map_err(|_| "Invalid date")?;
        let end = NaiveDate::parse_from_str(to, "%Y-%m-%d").map_err(|_| "Invalid date")?;
        if !(0..=365).contains(&(end - start).num_days()) || !(1..=100).contains(&limit) {
            return Err("Use a range of at most 366 days and a limit from 1 to 100.".into());
        }
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT id,title,date,start_time,end_time,all_day,'',color,completed,updated_at FROM events WHERE date>=?1 AND date<=?2 ORDER BY date,all_day DESC,start_time,id LIMIT ?3"
        ).map_err(database_error)?;
        let mut events = statement
            .query_map(params![from, to, limit + 1], event_from_row)
            .map_err(database_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(database_error)?;
        let truncated = events.len() > limit;
        events.truncate(limit);
        Ok((events, truncated))
    }
    /// The caller creates the application's data directory before opening this file.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, String> {
        let path = path.as_ref();
        let mut connection = Connection::open(path)
            .map_err(|error| format!("Could not open SQLite database: {error}"))?;
        connection
            .busy_timeout(Duration::from_secs(5))
            .map_err(database_error)?;
        // Reject unsupported schemas before changing the database's journal mode.
        let version: i64 = connection
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .map_err(database_error)?;
        if (1..SCHEMA_VERSION).contains(&version) && path != Path::new(":memory:") {
            let backup =
                path.with_extension(format!("pre-v{SCHEMA_VERSION}-{}.db", Uuid::new_v4()));
            connection
                .execute("VACUUM INTO ?1", [backup.to_string_lossy().as_ref()])
                .map_err(database_error)?;
        }
        migrate(&mut connection)?;
        connection
            .execute_batch(
                "PRAGMA foreign_keys = ON; PRAGMA journal_mode = WAL; PRAGMA synchronous = FULL;",
            )
            .map_err(database_error)?;
        Ok(Self {
            connection: Mutex::new(connection),
        })
    }

    pub fn list_events(&self, from: &str, to: &str) -> Result<Vec<CalendarEvent>, String> {
        self.events_in_range(from, to, false)
    }

    pub fn list_event_headers(&self, from: &str, to: &str) -> Result<Vec<CalendarEvent>, String> {
        self.events_in_range(from, to, true)
    }

    fn events_in_range(
        &self,
        from: &str,
        to: &str,
        headers: bool,
    ) -> Result<Vec<CalendarEvent>, String> {
        validate_date(from)?;
        validate_date(to)?;
        if from > to {
            return Err("The start date must not be after the end date.".into());
        }
        let connection = self.connection()?;
        let sql = format!("SELECT id, title, date, start_time, end_time, all_day, {}, color, completed, updated_at
             FROM events WHERE date >= ?1 AND date <= ?2
             ORDER BY date ASC, all_day DESC, start_time ASC, id ASC", if headers { "''" } else { "notes" });
        let mut statement = connection.prepare(&sql).map_err(database_error)?;
        let events = statement
            .query_map(params![from, to], event_from_row)
            .map_err(database_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(database_error)?;
        Ok(events)
    }

    pub fn save_event(&self, input: EventInput) -> Result<CalendarEvent, String> {
        self.save_event_if_unchanged(input, None)
    }

    /// The comparison and write share the SQLite lock/transaction, including other connections.
    pub fn save_event_if_unchanged(
        &self,
        mut input: EventInput,
        expected: Option<&str>,
    ) -> Result<CalendarEvent, String> {
        input.title = validated_title(&input.title)?;
        validate_date(&input.date)?;
        validate_text(&input.notes, "Event notes", MAX_TEXT_LENGTH)?;
        if !matches!(input.color.as_str(), "sage" | "blue" | "amber" | "rose") {
            return Err("Event color must be sage, blue, amber, or rose.".into());
        }
        if input.all_day {
            input.start_time = None;
            input.end_time = None;
        } else {
            let start = input
                .start_time
                .as_deref()
                .ok_or("A timed event needs a start time.")?;
            let end = input
                .end_time
                .as_deref()
                .ok_or("A timed event needs an end time.")?;
            let start = validate_time(start)?;
            let end = validate_time(end)?;
            if end <= start {
                return Err("The end time must be after the start time on the same date.".into());
            }
        }
        let updating = input.id.is_some();
        let mut connection = self.connection()?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(database_error)?;
        let previous: Option<String> = if let Some(id) = &input.id {
            validate_id(id)?;
            Some(
                transaction
                    .query_row("SELECT updated_at FROM events WHERE id=?1", [id], |row| {
                        row.get(0)
                    })
                    .map_err(|error| match error {
                        rusqlite::Error::QueryReturnedNoRows => "Event not found.".into(),
                        other => database_error(other),
                    })?,
            )
        } else {
            None
        };
        if expected.is_some() && expected != previous.as_deref() {
            return Err("Event conflict: the event changed. Reload its detail before saving; your draft was preserved.".into());
        }
        let event = CalendarEvent {
            id: record_id(input.id)?,
            title: input.title,
            date: input.date,
            start_time: input.start_time,
            end_time: input.end_time,
            all_day: input.all_day,
            notes: input.notes,
            color: input.color,
            completed: input.completed,
            updated_at: next_event_timestamp(previous.as_deref()),
        };
        let sql = if updating {
            "UPDATE events SET title = ?2, date = ?3, start_time = ?4, end_time = ?5,
             all_day = ?6, notes = ?7, color = ?8, completed = ?9, updated_at = ?10 WHERE id = ?1"
        } else {
            "INSERT INTO events (id, title, date, start_time, end_time, all_day, notes, color, completed, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)"
        };
        let changed = transaction
            .execute(
                sql,
                params![
                    event.id,
                    event.title,
                    event.date,
                    event.start_time,
                    event.end_time,
                    event.all_day,
                    event.notes,
                    event.color,
                    event.completed,
                    event.updated_at
                ],
            )
            .map_err(database_error)?;
        require_changed(changed, "Event")?;
        transaction.commit().map_err(database_error)?;
        Ok(event)
    }

    pub fn delete_event(&self, id: &str) -> Result<(), String> {
        self.delete("DELETE FROM events WHERE id = ?1", "Event", id)
    }

    /// Widget queries omit notes; update only completion so those headers can
    /// never overwrite the full event or a newer edit to its other fields.
    pub fn set_event_completed(&self, id: &str, completed: bool) -> Result<(), String> {
        validate_id(id)?;
        let mut connection = self.connection()?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(database_error)?;
        let previous: String = transaction
            .query_row("SELECT updated_at FROM events WHERE id=?1", [id], |row| {
                row.get(0)
            })
            .map_err(database_error)?;
        let changed = transaction
            .execute(
                "UPDATE events SET completed = ?2, updated_at = ?3 WHERE id = ?1",
                params![id, completed, next_event_timestamp(Some(&previous))],
            )
            .map_err(database_error)?;
        require_changed(changed, "Event")?;
        transaction.commit().map_err(database_error)
    }

    pub fn list_tasks(&self) -> Result<Vec<Task>, String> {
        let connection = self.connection()?;
        let mut statement = connection
            .prepare(
                "SELECT id, title, date, completed, updated_at FROM tasks
             ORDER BY completed ASC, date IS NULL ASC, date ASC, title COLLATE NOCASE ASC, id ASC",
            )
            .map_err(database_error)?;
        let tasks = statement
            .query_map([], task_from_row)
            .map_err(database_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(database_error)?;
        Ok(tasks)
    }

    pub fn save_task(&self, input: TaskInput) -> Result<Task, String> {
        let title = validated_title(&input.title)?;
        if let Some(date) = &input.date {
            validate_date(date)?;
        }
        let updating = input.id.is_some();
        let task = Task {
            id: record_id(input.id)?,
            title,
            date: input.date,
            completed: input.completed,
            updated_at: timestamp(),
        };
        let connection = self.connection()?;
        let sql = if updating {
            "UPDATE tasks SET title = ?2, date = ?3, completed = ?4, updated_at = ?5 WHERE id = ?1"
        } else {
            "INSERT INTO tasks (id, title, date, completed, updated_at) VALUES (?1, ?2, ?3, ?4, ?5)"
        };
        let changed = connection
            .execute(
                sql,
                params![
                    task.id,
                    task.title,
                    task.date,
                    task.completed,
                    task.updated_at
                ],
            )
            .map_err(database_error)?;
        require_changed(changed, "Task")?;
        Ok(task)
    }

    pub fn delete_task(&self, id: &str) -> Result<(), String> {
        self.delete("DELETE FROM tasks WHERE id = ?1", "Task", id)
    }

    pub fn list_notes(&self) -> Result<Vec<Note>, String> {
        let connection = self.connection()?;
        let mut statement = connection
            .prepare(
                "SELECT id, title, body, updated_at FROM notes ORDER BY updated_at DESC, id ASC",
            )
            .map_err(database_error)?;
        let notes = statement
            .query_map([], note_from_row)
            .map_err(database_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(database_error)?;
        Ok(notes)
    }

    pub fn list_note_headers(&self) -> Result<Vec<Note>, String> {
        let c = self.connection()?;
        let mut q = c
            .prepare("SELECT id,title,'' AS body,updated_at FROM notes ORDER BY updated_at DESC,id")
            .map_err(database_error)?;
        let rows = q
            .query_map([], note_from_row)
            .map_err(database_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(database_error)?;
        Ok(rows)
    }

    pub fn get_note(&self, id: &str) -> Result<Note, String> {
        validate_id(id)?;
        self.connection()?
            .query_row(
                "SELECT id,title,body,updated_at FROM notes WHERE id=?1",
                [id],
                note_from_row,
            )
            .map_err(database_error)
    }

    pub fn save_note(&self, input: NoteInput) -> Result<Note, String> {
        let title = validated_title(&input.title)?;
        validate_text(&input.body, "Note body", MAX_TEXT_LENGTH)?;
        let updating = input.id.is_some();
        let note = Note {
            id: record_id(input.id)?,
            title,
            body: input.body,
            updated_at: timestamp(),
        };
        let connection = self.connection()?;
        let sql = if updating {
            "UPDATE notes SET title = ?2, body = ?3, updated_at = ?4 WHERE id = ?1"
        } else {
            "INSERT INTO notes (id, title, body, updated_at) VALUES (?1, ?2, ?3, ?4)"
        };
        let changed = connection
            .execute(
                sql,
                params![note.id, note.title, note.body, note.updated_at],
            )
            .map_err(database_error)?;
        require_changed(changed, "Note")?;
        Ok(note)
    }

    pub fn delete_note(&self, id: &str) -> Result<(), String> {
        self.delete("DELETE FROM notes WHERE id = ?1", "Note", id)
    }

    pub(crate) fn delete(&self, sql: &str, kind: &str, id: &str) -> Result<(), String> {
        validate_id(id)?;
        let connection = self.connection()?;
        let changed = connection
            .execute(sql, params![id])
            .map_err(database_error)?;
        require_changed(changed, kind)
    }

    pub(crate) fn connection(&self) -> Result<MutexGuard<'_, Connection>, String> {
        self.connection
            .lock()
            .map_err(|_| "Database access was interrupted. Please restart the application.".into())
    }
}

fn migrate(connection: &mut Connection) -> Result<(), String> {
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(database_error)?;
    let version: i64 = transaction
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .map_err(database_error)?;
    if version > SCHEMA_VERSION {
        return Err(format!("This database uses schema version {version}, which is newer than this application supports ({SCHEMA_VERSION})."));
    }
    if version == 0 {
        transaction.execute_batch(
            "CREATE TABLE events (
                id TEXT PRIMARY KEY NOT NULL,
                title TEXT NOT NULL CHECK (length(trim(title)) BETWEEN 1 AND 200),
                date TEXT NOT NULL,
                start_time TEXT,
                end_time TEXT,
                all_day INTEGER NOT NULL CHECK (all_day IN (0, 1)),
                notes TEXT NOT NULL CHECK (length(notes) <= 20000),
                color TEXT NOT NULL CHECK (color IN ('sage', 'blue', 'amber', 'rose')),
                completed INTEGER NOT NULL CHECK (completed IN (0, 1)),
                updated_at TEXT NOT NULL,
                CHECK ((all_day = 1 AND start_time IS NULL AND end_time IS NULL)
                    OR (all_day = 0 AND start_time IS NOT NULL AND end_time IS NOT NULL AND end_time > start_time))
            );
            CREATE INDEX events_date ON events(date);
            CREATE TABLE tasks (
                id TEXT PRIMARY KEY NOT NULL,
                title TEXT NOT NULL CHECK (length(trim(title)) BETWEEN 1 AND 200),
                date TEXT,
                completed INTEGER NOT NULL CHECK (completed IN (0, 1)),
                updated_at TEXT NOT NULL
            );
            CREATE TABLE notes (
                id TEXT PRIMARY KEY NOT NULL,
                title TEXT NOT NULL CHECK (length(trim(title)) BETWEEN 1 AND 200),
                body TEXT NOT NULL CHECK (length(body) <= 20000),
                updated_at TEXT NOT NULL
            );
            PRAGMA user_version = 1;"
        ).map_err(database_error)?;
    }
    if version < 2 {
        transaction.execute_batch(
            "CREATE TABLE projects (
                id TEXT PRIMARY KEY NOT NULL, title TEXT NOT NULL, description TEXT NOT NULL,
                repo_path TEXT NOT NULL, updated_at TEXT NOT NULL
            );
            CREATE TABLE diary_entries (
                id TEXT PRIMARY KEY NOT NULL, project_id TEXT REFERENCES projects(id) ON DELETE SET NULL,
                date TEXT NOT NULL, kind TEXT NOT NULL, title TEXT NOT NULL, body TEXT NOT NULL,
                source_url TEXT NOT NULL DEFAULT '', source_ref TEXT,
                source_body TEXT NOT NULL DEFAULT '', updated_at TEXT NOT NULL,
                UNIQUE(source_ref)
            );
            CREATE INDEX diary_date ON diary_entries(date);
            CREATE INDEX diary_project_date ON diary_entries(project_id, date);
            PRAGMA user_version = 2;"
        ).map_err(database_error)?;
        crate::diary::create_search_index(&transaction)?;
    }
    if version < 3 {
        // Deliberately no FK: retaining this receipt also preserves a user's
        // deletion when an external delivery is retried later.
        transaction
            .execute_batch(
                "CREATE TABLE project_record_receipts (
                work_id TEXT PRIMARY KEY NOT NULL,
                revision INTEGER NOT NULL,
                payload TEXT NOT NULL,
                entry_id TEXT NOT NULL,
                managed_snapshot TEXT NOT NULL
            ); PRAGMA user_version = 3;",
            )
            .map_err(database_error)?;
    }
    if version < 4 {
        crate::calendar::migrate(&transaction)?;
    }
    if version < 5 {
        transaction.execute_batch("CREATE INDEX entry_display_category ON entry_display(category_id,entry_id); PRAGMA user_version=5;").map_err(database_error)?;
    }
    transaction.commit().map_err(database_error)
}

/// Require canonical, real calendar dates so text ordering equals date ordering.
pub fn validate_date(value: &str) -> Result<(), String> {
    let bytes = value.as_bytes();
    if bytes.len() != 10
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || !bytes
            .iter()
            .enumerate()
            .all(|(index, byte)| index == 4 || index == 7 || byte.is_ascii_digit())
    {
        return Err("Dates must use YYYY-MM-DD format.".into());
    }
    let date = NaiveDate::parse_from_str(value, "%Y-%m-%d")
        .map_err(|_| "Please enter a real calendar date in YYYY-MM-DD format.".to_string())?;
    if date.year() < 1 {
        return Err("The calendar year must be between 0001 and 9999.".into());
    }
    Ok(())
}

pub(crate) fn validate_time(value: &str) -> Result<NaiveTime, String> {
    let bytes = value.as_bytes();
    if bytes.len() != 5
        || bytes[2] != b':'
        || !bytes
            .iter()
            .enumerate()
            .all(|(index, byte)| index == 2 || byte.is_ascii_digit())
    {
        return Err("Times must use 24-hour HH:mm format.".into());
    }
    NaiveTime::parse_from_str(value, "%H:%M")
        .map_err(|_| "Please enter a valid 24-hour time in HH:mm format.".into())
}

pub(crate) fn validated_title(value: &str) -> Result<String, String> {
    let title = value.trim();
    if title.is_empty() {
        return Err("A title is required.".into());
    }
    validate_text(title, "Title", MAX_TITLE_LENGTH)?;
    Ok(title.to_string())
}

pub(crate) fn validate_text(value: &str, field: &str, maximum: usize) -> Result<(), String> {
    if value.chars().take(maximum + 1).count() > maximum {
        return Err(format!("{field} must be at most {maximum} characters."));
    }
    if value.contains('\0') {
        return Err(format!("{field} cannot contain null characters."));
    }
    Ok(())
}

pub(crate) fn validate_id(id: &str) -> Result<(), String> {
    Uuid::parse_str(id)
        .map(|_| ())
        .map_err(|_| "The record ID is invalid.".into())
}

pub(crate) fn record_id(id: Option<String>) -> Result<String, String> {
    match id {
        Some(id) => {
            validate_id(&id)?;
            Ok(id)
        }
        None => Ok(Uuid::new_v4().to_string()),
    }
}

pub(crate) fn timestamp() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

pub(crate) fn next_event_timestamp(previous: Option<&str>) -> String {
    let now = Utc::now();
    let next = previous
        .and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok())
        .map(|value| value.with_timezone(&Utc) + chrono::Duration::milliseconds(1));
    next.map_or(now, |next| now.max(next))
        .to_rfc3339_opts(SecondsFormat::Millis, true)
}

pub(crate) fn require_changed(changed: usize, kind: &str) -> Result<(), String> {
    if changed == 0 {
        Err(format!(
            "{kind} not found. It may have been deleted in another window."
        ))
    } else {
        Ok(())
    }
}

pub(crate) fn database_error(error: rusqlite::Error) -> String {
    format!("Database operation failed: {error}")
}

fn event_from_row(row: &Row<'_>) -> rusqlite::Result<CalendarEvent> {
    Ok(CalendarEvent {
        id: row.get(0)?,
        title: row.get(1)?,
        date: row.get(2)?,
        start_time: row.get(3)?,
        end_time: row.get(4)?,
        all_day: row.get(5)?,
        notes: row.get(6)?,
        color: row.get(7)?,
        completed: row.get(8)?,
        updated_at: row.get(9)?,
    })
}

pub(crate) fn task_from_row(row: &Row<'_>) -> rusqlite::Result<Task> {
    Ok(Task {
        id: row.get(0)?,
        title: row.get(1)?,
        date: row.get(2)?,
        completed: row.get(3)?,
        updated_at: row.get(4)?,
    })
}

fn note_from_row(row: &Row<'_>) -> rusqlite::Result<Note> {
    Ok(Note {
        id: row.get(0)?,
        title: row.get(1)?,
        body: row.get(2)?,
        updated_at: row.get(3)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guarded_event_save_rejects_stale_versions_and_retains_newer_content() {
        let store = Storage::open(":memory:").unwrap();
        let first = store.save_event(event_on("2026-11-02")).unwrap();
        let mut change = event_on("2026-11-02");
        change.id = Some(first.id.clone());
        change.notes = "new notes".into();
        let second = store
            .save_event_if_unchanged(change.clone(), Some(&first.updated_at))
            .unwrap();
        assert_ne!(first.updated_at, second.updated_at);
        change.title = "stale writer".into();
        assert!(store
            .save_event_if_unchanged(change, Some(&first.updated_at))
            .unwrap_err()
            .contains("conflict"));
        assert_eq!(store.get_event(&first.id).unwrap(), second);
        store.set_event_completed(&first.id, true).unwrap();
        assert_ne!(
            store.get_event(&first.id).unwrap().updated_at,
            second.updated_at
        );
    }

    #[test]
    fn bounded_event_headers_limit_in_database_and_reject_large_ranges() {
        let store = Storage::open(":memory:").unwrap();
        for _ in 0..4 {
            store.save_event(event_on("2026-11-02")).unwrap();
        }
        let (events, truncated) = store
            .list_event_headers_limited("2026-11-02", "2026-11-02", 2)
            .unwrap();
        assert_eq!(events.len(), 2);
        assert!(truncated);
        assert!(events.iter().all(|event| event.notes.is_empty()));
        assert!(store
            .list_event_headers_limited("2026-01-01", "2027-01-02", 10)
            .is_err());
        assert!(store
            .list_event_headers_limited("2026-11-02", "2026-11-01", 10)
            .is_err());
        assert!(store
            .list_event_headers_limited("2026-11-02", "2026-11-02", 101)
            .is_err());
    }

    #[test]
    fn concurrent_calendar_writers_allow_only_one_matching_revision() {
        use std::sync::{Arc, Barrier};
        let store = Arc::new(Storage::open(":memory:").unwrap());
        let event = store.save_event(event_on("2026-11-02")).unwrap();
        let barrier = Arc::new(Barrier::new(3));
        let threads: Vec<_> = (0..2)
            .map(|n| {
                let store = store.clone();
                let barrier = barrier.clone();
                let event = event.clone();
                std::thread::spawn(move || {
                    let mut input = event_on("2026-11-02");
                    input.id = Some(event.id);
                    input.title = format!("writer {n}");
                    barrier.wait();
                    store.save_event_if_unchanged(input, Some(&event.updated_at))
                })
            })
            .collect();
        barrier.wait();
        let results: Vec<_> = threads
            .into_iter()
            .map(|thread| thread.join().unwrap())
            .collect();
        assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
        assert!(results
            .iter()
            .filter_map(|result| result.as_ref().err())
            .all(|error| error.contains("conflict")));
    }
    use std::fs;
    use std::path::PathBuf;

    struct TestDatabase(PathBuf);

    impl TestDatabase {
        fn new() -> Self {
            let path =
                std::env::temp_dir().join(format!("timeschedule-test-{}.sqlite", Uuid::new_v4()));
            Self(path)
        }

        fn open(&self) -> Storage {
            Storage::open(&self.0).expect("open temporary SQLite database")
        }
    }

    impl Drop for TestDatabase {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.0);
            let _ = fs::remove_file(format!("{}-wal", self.0.display()));
            let _ = fs::remove_file(format!("{}-shm", self.0.display()));
        }
    }

    fn event_on(date: &str) -> EventInput {
        EventInput {
            id: None,
            title: "회의".into(),
            date: date.into(),
            start_time: Some("09:00".into()),
            end_time: Some("10:00".into()),
            all_day: false,
            notes: "메모\n두 번째 줄".into(),
            color: "sage".into(),
            completed: false,
        }
    }

    #[test]
    fn records_survive_closing_and_reopening_the_database() {
        let database = TestDatabase::new();
        let storage = database.open();
        let event = storage.save_event(event_on("2026-10-28")).unwrap();
        let task = storage
            .save_task(TaskInput {
                id: None,
                title: "자료 준비".into(),
                date: None,
                completed: false,
            })
            .unwrap();
        let note = storage
            .save_note(NoteInput {
                id: None,
                title: "계획".into(),
                body: "오늘 할 일".into(),
            })
            .unwrap();
        assert!(Uuid::parse_str(&event.id).is_ok());
        assert!(chrono::DateTime::parse_from_rfc3339(&event.updated_at).is_ok());
        assert!(event.updated_at.ends_with('Z'));
        drop(storage);

        let reopened = database.open();
        assert_eq!(
            reopened.list_events("2026-10-28", "2026-10-28").unwrap(),
            vec![event]
        );
        assert_eq!(reopened.list_tasks().unwrap(), vec![task]);
        assert_eq!(reopened.list_notes().unwrap(), vec![note]);
    }

    #[test]
    fn completing_a_header_preserves_notes_times_color_and_identity() {
        let database = TestDatabase::new();
        let storage = database.open();
        let original = storage.save_event(event_on("2026-10-28")).unwrap();
        let header = storage
            .list_event_headers(&original.date, &original.date)
            .unwrap()
            .remove(0);
        assert!(header.notes.is_empty());
        storage.set_event_completed(&header.id, true).unwrap();
        let mut actual = storage
            .list_events(&original.date, &original.date)
            .unwrap()
            .remove(0);
        assert!(actual.completed);
        actual.completed = original.completed;
        actual.updated_at = original.updated_at.clone();
        assert_eq!(actual, original);
        storage.set_event_completed(&header.id, false).unwrap();
        drop(storage);
        assert!(
            !database
                .open()
                .list_events(&original.date, &original.date)
                .unwrap()[0]
                .completed
        );
        assert!(database
            .open()
            .set_event_completed(&Uuid::new_v4().to_string(), true)
            .is_err());
    }

    #[test]
    fn event_ranges_include_both_boundaries_and_exclude_neighboring_dates() {
        let database = TestDatabase::new();
        let storage = database.open();
        for date in ["2026-09-30", "2026-10-01", "2026-10-31", "2026-11-01"] {
            storage.save_event(event_on(date)).unwrap();
        }
        let events = storage.list_events("2026-10-01", "2026-10-31").unwrap();
        assert_eq!(
            events
                .iter()
                .map(|event| event.date.as_str())
                .collect::<Vec<_>>(),
            vec!["2026-10-01", "2026-10-31"]
        );
        assert_eq!(
            storage
                .list_events("2026-10-31", "2026-10-31")
                .unwrap()
                .len(),
            1
        );
        assert!(storage.list_events("2026-11-01", "2026-10-01").is_err());
        assert!(storage.list_events("2026-02-30", "2026-10-01").is_err());
        assert!(storage.list_events("2026-10-01", "2026-13-01").is_err());
    }

    #[test]
    fn invalid_event_dates_times_titles_and_colors_are_not_written() {
        let database = TestDatabase::new();
        let storage = database.open();
        for date in [
            "2026-02-29",
            "2026-02-30",
            "2026-13-01",
            "2026-1-01",
            "0000-01-01",
            "2026-10-28junk",
        ] {
            assert!(
                storage.save_event(event_on(date)).is_err(),
                "accepted date {date}"
            );
        }
        for (start, end) in [
            ("9:00", "10:00"),
            ("24:00", "25:00"),
            ("09:60", "10:00"),
            ("10:00", "10:00"),
            ("11:00", "10:00"),
            ("23:00", "01:00"),
            ("09:00:00", "10:00"),
        ] {
            let mut input = event_on("2026-10-28");
            input.start_time = Some(start.into());
            input.end_time = Some(end.into());
            assert!(
                storage.save_event(input).is_err(),
                "accepted range {start}–{end}"
            );
        }
        let mut input = event_on("2026-10-28");
        input.start_time = None;
        assert!(storage.save_event(input).is_err());
        let mut input = event_on("2026-10-28");
        input.end_time = None;
        assert!(storage.save_event(input).is_err());
        let mut input = event_on("2026-10-28");
        input.title = " \t\n ".into();
        assert!(storage.save_event(input).is_err());
        let mut input = event_on("2026-10-28");
        input.color = "purple".into();
        assert!(storage.save_event(input).is_err());
        let mut input = event_on("2026-10-28");
        input.title = "가".repeat(201);
        assert!(storage.save_event(input).is_err());
        let mut input = event_on("2026-10-28");
        input.notes = "a".repeat(20_001);
        assert!(storage.save_event(input).is_err());
        assert!(storage
            .list_events("2026-01-01", "2026-12-31")
            .unwrap()
            .is_empty());
        assert!(storage.save_event(event_on("2028-02-29")).is_ok());
    }

    #[test]
    fn all_day_events_clear_times_and_preserve_text_as_data() {
        let database = TestDatabase::new();
        let storage = database.open();
        let mut input = event_on("2026-10-28");
        input.all_day = true;
        input.start_time = Some("invalid ignored time".into());
        input.end_time = Some("00:00".into());
        input.title = "  회의'); DROP TABLE events; --  ".into();
        let event = storage.save_event(input).unwrap();
        assert_eq!(event.start_time, None);
        assert_eq!(event.end_time, None);
        assert_eq!(event.title, "회의'); DROP TABLE events; --");
        assert_eq!(
            storage.list_events("2026-10-28", "2026-10-28").unwrap(),
            vec![event]
        );
        assert!(storage.save_event(event_on("2026-10-29")).is_ok());
    }

    #[test]
    fn event_updates_replace_existing_fields_and_delete_is_persistent() {
        let database = TestDatabase::new();
        let storage = database.open();
        let original = storage.save_event(event_on("2026-10-28")).unwrap();
        let mut input = event_on("2026-10-29");
        input.id = Some(original.id.clone());
        input.title = "바뀐 회의".into();
        input.completed = true;
        input.color = "blue".into();
        let updated = storage.save_event(input).unwrap();
        assert_eq!(updated.id, original.id);
        assert_eq!(updated.title, "바뀐 회의");
        assert_eq!(updated.color, "blue");
        assert!(updated.completed);
        assert!(storage
            .list_events("2026-10-28", "2026-10-28")
            .unwrap()
            .is_empty());
        assert_eq!(
            storage.list_events("2026-10-29", "2026-10-29").unwrap(),
            vec![updated]
        );
        storage.delete_event(&original.id).unwrap();
        assert!(storage.delete_event(&original.id).is_err());
        drop(storage);
        assert!(database
            .open()
            .list_events("2026-01-01", "2026-12-31")
            .unwrap()
            .is_empty());
    }

    #[test]
    fn missing_update_ids_never_create_records() {
        let database = TestDatabase::new();
        let storage = database.open();
        let missing_id = Uuid::new_v4().to_string();
        let mut event = event_on("2026-10-28");
        event.id = Some(missing_id.clone());
        assert!(storage.save_event(event).unwrap_err().contains("not found"));
        assert!(storage
            .save_task(TaskInput {
                id: Some(missing_id.clone()),
                title: "작업".into(),
                date: None,
                completed: false,
            })
            .unwrap_err()
            .contains("not found"));
        assert!(storage
            .save_note(NoteInput {
                id: Some(missing_id.clone()),
                title: "메모".into(),
                body: String::new(),
            })
            .unwrap_err()
            .contains("not found"));
        assert!(storage.delete_event(&missing_id).is_err());
        assert!(storage.delete_task(&missing_id).is_err());
        assert!(storage.delete_note(&missing_id).is_err());
        assert!(storage
            .list_events("2026-01-01", "2026-12-31")
            .unwrap()
            .is_empty());
        assert!(storage.list_tasks().unwrap().is_empty());
        assert!(storage.list_notes().unwrap().is_empty());
    }

    #[test]
    fn a_rejected_edit_preserves_the_existing_record() {
        let database = TestDatabase::new();
        let storage = database.open();
        let original = storage.save_event(event_on("2026-10-28")).unwrap();
        let mut invalid = event_on("2026-10-29");
        invalid.id = Some(original.id.clone());
        invalid.end_time = Some("08:00".into());
        assert!(storage.save_event(invalid).is_err());
        drop(storage);
        assert_eq!(
            database
                .open()
                .list_events("2026-10-01", "2026-10-31")
                .unwrap(),
            vec![original]
        );
    }

    #[test]
    fn the_event_json_contract_accepts_omitted_ids_and_uses_camel_case() {
        let database = TestDatabase::new();
        let storage = database.open();
        let input: EventInput = serde_json::from_value(serde_json::json!({
            "title": "회의", "date": "2026-10-28", "startTime": "09:00", "endTime": "10:00",
            "allDay": false, "notes": "본문", "color": "rose", "completed": true
        }))
        .unwrap();
        let output = serde_json::to_value(storage.save_event(input).unwrap()).unwrap();
        assert_eq!(output["startTime"], "09:00");
        assert_eq!(output["endTime"], "10:00");
        assert_eq!(output["allDay"], false);
        assert_eq!(output["completed"], true);
        assert!(output["updatedAt"].is_string());
        assert!(output.get("start_time").is_none());
    }

    #[test]
    fn task_and_note_updates_validate_inputs_and_keep_record_identity() {
        let database = TestDatabase::new();
        let storage = database.open();
        assert!(storage
            .save_task(TaskInput {
                id: None,
                title: "작업".into(),
                date: Some("2026-02-29".into()),
                completed: false,
            })
            .is_err());
        assert!(storage
            .save_task(TaskInput {
                id: None,
                title: " ".into(),
                date: None,
                completed: false,
            })
            .is_err());
        assert!(storage
            .save_note(NoteInput {
                id: None,
                title: " ".into(),
                body: String::new(),
            })
            .is_err());
        assert!(storage
            .save_note(NoteInput {
                id: None,
                title: "메모".into(),
                body: "a".repeat(20_001),
            })
            .is_err());
        let task = storage
            .save_task(TaskInput {
                id: None,
                title: "작업".into(),
                date: Some("2026-10-28".into()),
                completed: false,
            })
            .unwrap();
        let task = storage
            .save_task(TaskInput {
                id: Some(task.id),
                title: "완료".into(),
                date: None,
                completed: true,
            })
            .unwrap();
        assert_eq!(task.title, "완료");
        assert_eq!(task.date, None);
        assert!(task.completed);
        assert_eq!(storage.list_tasks().unwrap(), vec![task.clone()]);
        let note = storage
            .save_note(NoteInput {
                id: None,
                title: "메모".into(),
                body: "첫 내용".into(),
            })
            .unwrap();
        let note = storage
            .save_note(NoteInput {
                id: Some(note.id),
                title: "수정".into(),
                body: "바뀐 내용\n' ".into(),
            })
            .unwrap();
        assert_eq!(note.title, "수정");
        assert_eq!(note.body, "바뀐 내용\n' ");
        assert_eq!(storage.list_notes().unwrap(), vec![note.clone()]);
        storage.delete_task(&task.id).unwrap();
        storage.delete_note(&note.id).unwrap();
        drop(storage);
        let reopened = database.open();
        assert!(reopened.list_tasks().unwrap().is_empty());
        assert!(reopened.list_notes().unwrap().is_empty());
    }

    #[test]
    fn a_newer_database_schema_is_rejected_without_modifying_it() {
        let database = TestDatabase::new();
        let connection = Connection::open(&database.0).unwrap();
        connection.execute_batch("PRAGMA user_version = 99; CREATE TABLE future_data (value TEXT); INSERT INTO future_data VALUES ('keep me');").unwrap();
        drop(connection);
        assert!(Storage::open(&database.0).is_err());
        let connection = Connection::open(&database.0).unwrap();
        assert_eq!(
            connection
                .query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
                .unwrap(),
            99
        );
        assert_eq!(
            connection
                .query_row("SELECT value FROM future_data", [], |row| row
                    .get::<_, String>(0))
                .unwrap(),
            "keep me"
        );
    }
}
