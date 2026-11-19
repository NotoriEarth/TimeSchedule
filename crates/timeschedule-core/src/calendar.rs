//! Shared, body-free calendar projection. Source IDs remain the original IDs.
use crate::{
    diary::{entry_row, validate_url, Entry, EntryInput},
    storage::*,
    timeline::{EntryTimeline, TimeChange},
};
use chrono::NaiveDate;
use rusqlite::{params, Connection, OptionalExtension, Row, TransactionBehavior};
use std::collections::{BTreeMap, HashSet};
mod category_lifecycle;
pub use category_lifecycle::CategoryDeletePreview;

pub const SCHEDULE_CATEGORY: &str = "00000000-0000-4000-8000-000000000001";
pub const RECORD_CATEGORY: &str = "00000000-0000-4000-8000-000000000002";
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Category {
    pub id: String,
    pub name: String,
    pub color: String,
    pub position: u32,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CalendarSource {
    Event,
    Task,
    Entry,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CalendarItem {
    pub id: String,
    pub source: CalendarSource,
    pub date: String,
    pub title: String,
    pub project_id: Option<String>,
    pub project_title: String,
    pub project_short_name: String,
    pub short_title: String,
    pub category_id: String,
    pub start_time: Option<String>,
    pub end_time: Option<String>,
    pub all_day: bool,
    pub completed: bool,
    /// Records this line stands for: the same project's records of one day
    /// share a line, headed by the most recently changed one. 1 otherwise.
    pub count: u32,
}
#[derive(Debug, Clone, Default)]
pub struct CalendarDay {
    pub date: String,
    /// Calendar lines of the day: one project's records share one line.
    /// Layout, the 16-line preview and paging use this.
    pub total: u32,
    pub completed: u32,
    /// Every schedule, task and record the lines stand for, each counted once,
    /// including those outside the preview. Shown to people as the day's total.
    pub represented: u32,
    pub items: Vec<CalendarItem>,
}
#[derive(Debug, Clone, Default)]
pub struct CalendarPage {
    pub items: Vec<CalendarItem>,
    pub total: u32,
    pub completed: u32,
    pub has_more: bool,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryDisplay {
    pub category_id: String,
    pub short_title: String,
}
impl Default for EntryDisplay {
    fn default() -> Self {
        Self {
            category_id: RECORD_CATEGORY.into(),
            short_title: String::new(),
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryEdit {
    pub entry: Entry,
    pub display: EntryDisplay,
    /// Typed time and source evidence; part of the version a save compares.
    pub timeline: EntryTimeline,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskDetails {
    pub body: String,
    pub all_day: bool,
    pub start_time: Option<String>,
    pub end_time: Option<String>,
}
impl Default for TaskDetails {
    fn default() -> Self {
        Self {
            body: String::new(),
            all_day: true,
            start_time: None,
            end_time: None,
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskEdit {
    pub task: Task,
    pub details: TaskDetails,
}

pub(crate) fn migrate(c: &Connection) -> Result<(), String> {
    c.execute_batch("CREATE TABLE calendar_categories(id TEXT PRIMARY KEY NOT NULL,name TEXT NOT NULL,color TEXT NOT NULL,position INTEGER NOT NULL);
        CREATE TABLE entry_display(entry_id TEXT PRIMARY KEY REFERENCES diary_entries(id) ON DELETE CASCADE,category_id TEXT NOT NULL REFERENCES calendar_categories(id),short_title TEXT NOT NULL DEFAULT '');
        CREATE TABLE project_display(project_id TEXT PRIMARY KEY REFERENCES projects(id) ON DELETE CASCADE,short_name TEXT NOT NULL DEFAULT '');
        CREATE TABLE task_details(task_id TEXT PRIMARY KEY REFERENCES tasks(id) ON DELETE CASCADE,body TEXT NOT NULL DEFAULT '',all_day INTEGER NOT NULL DEFAULT 1,start_time TEXT,end_time TEXT);
        CREATE INDEX tasks_date ON tasks(date);
        PRAGMA user_version=4;").map_err(database_error)?;
    c.execute(
        "INSERT INTO calendar_categories VALUES(?1,'일정','#80B6A5',0),(?2,'기록','#82A8D8',1)",
        params![SCHEDULE_CATEGORY, RECORD_CATEGORY],
    )
    .map_err(database_error)?;
    crate::search::task_details_v4(c)
}

fn line(value: &str, label: &str, max: usize) -> Result<String, String> {
    let value = value.trim();
    validate_text(value, label, max)?;
    if value.chars().any(char::is_control) {
        return Err(format!("{label}은 한 줄로 입력해 주세요."));
    }
    Ok(value.into())
}
fn categories(c: &Connection) -> Result<Vec<Category>, String> {
    let mut q = c
        .prepare("SELECT id,name,color,position FROM calendar_categories ORDER BY position,id")
        .map_err(database_error)?;
    let rows = q
        .query_map([], |r| {
            Ok(Category {
                id: r.get(0)?,
                name: r.get(1)?,
                color: r.get(2)?,
                position: r.get(3)?,
            })
        })
        .map_err(database_error)?
        .collect::<Result<_, _>>()
        .map_err(database_error)?;
    Ok(rows)
}
fn display(c: &Connection, id: &str) -> Result<EntryDisplay, String> {
    c.query_row(
        "SELECT category_id,short_title FROM entry_display WHERE entry_id=?1",
        [id],
        |r| {
            Ok(EntryDisplay {
                category_id: r.get(0)?,
                short_title: r.get(1)?,
            })
        },
    )
    .optional()
    .map(|v| v.unwrap_or_default())
    .map_err(database_error)
}
fn entry_edit(c: &Connection, id: &str) -> Result<EntryEdit, String> {
    let entry=c.query_row("SELECT id,project_id,date,kind,title,body,source_url,source_ref,source_body,updated_at FROM diary_entries WHERE id=?1",[id],entry_row).map_err(database_error)?;
    Ok(EntryEdit {
        entry,
        display: display(c, id)?,
        timeline: crate::timeline::read_timeline(c, id)?,
    })
}
fn task_edit(c: &Connection, id: &str) -> Result<TaskEdit, String> {
    let task = c
        .query_row(
            "SELECT id,title,date,completed,updated_at FROM tasks WHERE id=?1",
            [id],
            task_from_row,
        )
        .map_err(database_error)?;
    let details = c
        .query_row(
            "SELECT body,all_day,start_time,end_time FROM task_details WHERE task_id=?1",
            [id],
            |r| {
                Ok(TaskDetails {
                    body: r.get(0)?,
                    all_day: r.get(1)?,
                    start_time: r.get(2)?,
                    end_time: r.get(3)?,
                })
            },
        )
        .optional()
        .map_err(database_error)?
        .unwrap_or_default();
    Ok(TaskEdit { task, details })
}
const CONFLICT: &str = "다른 창에서 수정된 항목입니다. 닫고 다시 열어 최신 내용을 확인해 주세요.";

impl Storage {
    pub fn save_calendar_project(
        &self,
        input: crate::diary::ProjectInput,
        short_name: &str,
    ) -> Result<crate::diary::Project, String> {
        let short_name = line(short_name, "프로젝트 표시 이름", 24)?;
        let title = validated_title(&input.title)?;
        validate_text(&input.description, "프로젝트 설명", 20000)?;
        validate_text(&input.repo_path, "저장소 경로", 2000)?;
        let updating = input.id.is_some();
        let project = crate::diary::Project {
            id: record_id(input.id)?,
            title,
            description: input.description,
            repo_path: input.repo_path.trim().into(),
            updated_at: timestamp(),
        };
        let mut c = self.connection()?;
        let tx = c
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(database_error)?;
        let changed=tx.execute(if updating{"UPDATE projects SET title=?2,description=?3,repo_path=?4,updated_at=?5 WHERE id=?1"}else{"INSERT INTO projects VALUES(?1,?2,?3,?4,?5)"},params![project.id,project.title,project.description,project.repo_path,project.updated_at]).map_err(database_error)?;
        require_changed(changed, "프로젝트")?;
        tx.execute("INSERT INTO project_display VALUES(?1,?2) ON CONFLICT(project_id) DO UPDATE SET short_name=excluded.short_name",params![project.id,short_name]).map_err(database_error)?;
        tx.commit().map_err(database_error)?;
        Ok(project)
    }
    pub fn list_categories(&self) -> Result<Vec<Category>, String> {
        categories(&*self.connection()?)
    }
    pub fn save_category(
        &self,
        id: Option<&str>,
        name: &str,
        color: &str,
    ) -> Result<Category, String> {
        let mut c = self.connection()?;
        let tx = c
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(database_error)?;
        let value = category_lifecycle::write_category(&tx, id, name, color)?;
        tx.commit().map_err(database_error)?;
        Ok(value)
    }
    pub fn reorder_categories(&self, expected: &[String], order: &[String]) -> Result<(), String> {
        let mut c = self.connection()?;
        let tx = c
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(database_error)?;
        let current: Vec<_> = categories(&tx)?.into_iter().map(|v| v.id).collect();
        if current != expected {
            return Err("카테고리가 변경되었습니다. 목록을 다시 열어 주세요.".into());
        }
        if order.len() != current.len()
            || order.iter().collect::<HashSet<_>>() != current.iter().collect::<HashSet<_>>()
        {
            return Err("카테고리 순서를 확인해 주세요.".into());
        }
        for (position, id) in order.iter().enumerate() {
            tx.execute(
                "UPDATE calendar_categories SET position=?2 WHERE id=?1",
                params![id, position],
            )
            .map_err(database_error)?;
        }
        tx.commit().map_err(database_error)
    }
    pub fn project_short_name(&self, id: &str) -> Result<String, String> {
        validate_id(id)?;
        self.connection()?
            .query_row(
                "SELECT short_name FROM project_display WHERE project_id=?1",
                [id],
                |r| r.get(0),
            )
            .optional()
            .map(|v| v.unwrap_or_default())
            .map_err(database_error)
    }
    pub fn save_project_short_name(&self, id: &str, value: &str) -> Result<(), String> {
        validate_id(id)?;
        let value = line(value, "프로젝트 표시 이름", 24)?;
        self.connection()?.execute("INSERT INTO project_display VALUES(?1,?2) ON CONFLICT(project_id) DO UPDATE SET short_name=excluded.short_name",params![id,value]).map_err(database_error)?;
        Ok(())
    }
    pub fn entry_display(&self, id: &str) -> Result<EntryDisplay, String> {
        validate_id(id)?;
        display(&*self.connection()?, id)
    }
    pub fn calendar_entry(&self, id: &str) -> Result<EntryEdit, String> {
        validate_id(id)?;
        entry_edit(&*self.connection()?, id)
    }
    pub fn task_edit(&self, id: &str) -> Result<TaskEdit, String> {
        validate_id(id)?;
        task_edit(&*self.connection()?, id)
    }
    /// Saves a record and keeps whatever time of day it already had.
    pub fn save_calendar_entry(
        &self,
        input: EntryInput,
        meta: EntryDisplay,
        expected: Option<&EntryEdit>,
    ) -> Result<EntryEdit, String> {
        self.save_calendar_entry_timed(input, meta, TimeChange::Keep, expected)
    }
    /// Saves a record with the time of day a person typed, or none (unknown).
    pub fn save_calendar_entry_timed(
        &self,
        input: EntryInput,
        mut meta: EntryDisplay,
        time: TimeChange,
        expected: Option<&EntryEdit>,
    ) -> Result<EntryEdit, String> {
        let title = validated_title(&input.title)?;
        validate_date(&input.date)?;
        validate_text(&input.body, "기록 본문", 100000)?;
        validate_url(&input.source_url)?;
        if ![
            "note",
            "design",
            "development",
            "troubleshooting",
            "commit",
            "summary",
            "portfolio",
        ]
        .contains(&input.kind.as_str())
        {
            return Err("기록 종류를 확인해 주세요.".into());
        }
        if let Some(id) = &input.project_id {
            validate_id(id)?;
        }
        meta.short_title = line(&meta.short_title, "짧은 작업명", 80)?;
        let updating = input.id.is_some();
        let id = record_id(input.id)?;
        let mut c = self.connection()?;
        let tx = c
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(database_error)?;
        if meta.category_id == SCHEDULE_CATEGORY
            || !categories(&tx)?.iter().any(|v| v.id == meta.category_id)
        {
            return Err("기록의 카테고리를 선택해 주세요.".into());
        }
        let previous = if updating {
            Some(entry_edit(&tx, &id)?)
        } else {
            None
        };
        if expected.is_some() && expected != previous.as_ref() {
            return Err(CONFLICT.into());
        }
        let updated = next_event_timestamp(previous.as_ref().map(|v| v.entry.updated_at.as_str()));
        tx.execute(if updating {"UPDATE diary_entries SET project_id=?2,date=?3,kind=?4,title=?5,body=?6,source_url=?7,updated_at=?8 WHERE id=?1"}else{"INSERT INTO diary_entries(id,project_id,date,kind,title,body,source_url,updated_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)"},params![id,input.project_id,input.date,input.kind,title,input.body,input.source_url,updated]).map_err(database_error)?;
        tx.execute("INSERT INTO entry_display VALUES(?1,?2,?3) ON CONFLICT(entry_id) DO UPDATE SET category_id=excluded.category_id,short_title=excluded.short_title",params![id,meta.category_id,meta.short_title]).map_err(database_error)?;
        // A typed time belongs only to a hand-written record. Editing a collected
        // Git or automatic record keeps its source time as the reference.
        let manual = previous
            .as_ref()
            .map_or(true, |p| p.timeline.takes_manual_time());
        if manual {
            crate::timeline::write_manual_time(&tx, &id, &time)?;
        }
        let saved = entry_edit(&tx, &id)?;
        tx.commit().map_err(database_error)?;
        Ok(saved)
    }
    pub fn save_calendar_task(
        &self,
        input: TaskInput,
        mut details: TaskDetails,
        expected: Option<&TaskEdit>,
    ) -> Result<TaskEdit, String> {
        let title = validated_title(&input.title)?;
        if let Some(date) = &input.date {
            validate_date(date)?;
        }
        validate_text(&details.body, "일정 내용", 20000)?;
        if details.all_day {
            details.start_time = None;
            details.end_time = None;
        } else {
            let start = validate_time(
                details
                    .start_time
                    .as_deref()
                    .ok_or("시작 시간을 입력해 주세요.")?,
            )?;
            let end = validate_time(
                details
                    .end_time
                    .as_deref()
                    .ok_or("종료 시간을 입력해 주세요.")?,
            )?;
            if end <= start {
                return Err("종료 시간은 시작 시간 이후여야 합니다.".into());
            }
            if input.date.is_none() {
                return Err("시간을 지정하려면 날짜를 입력해 주세요.".into());
            }
        }
        let updating = input.id.is_some();
        let id = record_id(input.id)?;
        let mut c = self.connection()?;
        let tx = c
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(database_error)?;
        let previous = if updating {
            Some(task_edit(&tx, &id)?)
        } else {
            None
        };
        if expected.is_some() && expected != previous.as_ref() {
            return Err(CONFLICT.into());
        }
        let updated = next_event_timestamp(previous.as_ref().map(|v| v.task.updated_at.as_str()));
        tx.execute(
            if updating {
                "UPDATE tasks SET title=?2,date=?3,completed=?4,updated_at=?5 WHERE id=?1"
            } else {
                "INSERT INTO tasks VALUES(?1,?2,?3,?4,?5)"
            },
            params![id, title, input.date, input.completed, updated],
        )
        .map_err(database_error)?;
        tx.execute("INSERT INTO task_details VALUES(?1,?2,?3,?4,?5) ON CONFLICT(task_id) DO UPDATE SET body=excluded.body,all_day=excluded.all_day,start_time=excluded.start_time,end_time=excluded.end_time",params![id,details.body,details.all_day,details.start_time,details.end_time]).map_err(database_error)?;
        let saved = task_edit(&tx, &id)?;
        tx.commit().map_err(database_error)?;
        Ok(saved)
    }
    pub fn set_calendar_completed(
        &self,
        source: CalendarSource,
        id: &str,
        completed: bool,
    ) -> Result<(), String> {
        match source {
            CalendarSource::Event => self.set_event_completed(id, completed),
            CalendarSource::Entry => Err("기록에는 완료 상태가 없습니다.".into()),
            CalendarSource::Task => {
                validate_id(id)?;
                let mut c = self.connection()?;
                let tx = c
                    .transaction_with_behavior(TransactionBehavior::Immediate)
                    .map_err(database_error)?;
                let previous: String = tx
                    .query_row("SELECT updated_at FROM tasks WHERE id=?1", [id], |r| {
                        r.get(0)
                    })
                    .map_err(database_error)?;
                tx.execute(
                    "UPDATE tasks SET completed=?2,updated_at=?3 WHERE id=?1",
                    params![id, completed, next_event_timestamp(Some(&previous))],
                )
                .map_err(database_error)?;
                tx.commit().map_err(database_error)
            }
        }
    }
}

// The union selects headers only; original bodies stay in source tables until opened.
// One project's records of a day become one head: the latest, with their count.
fn headers() -> String {
    format!("WITH heads AS (
 SELECT e.id,0 AS source,e.date,e.title,NULL AS project_id,'' AS project_title,'' AS project_short_name,'' AS short_title,'{SCHEDULE_CATEGORY}' AS category_id,e.start_time,e.end_time,e.all_day,e.completed,e.updated_at,1 AS grouped FROM events e WHERE e.date BETWEEN ?1 AND ?2
 UNION ALL SELECT t.id,1,t.date,t.title,NULL,'','','','{SCHEDULE_CATEGORY}',d.start_time,d.end_time,COALESCE(d.all_day,1),t.completed,t.updated_at,1 FROM tasks t LEFT JOIN task_details d ON d.task_id=t.id WHERE t.date BETWEEN ?1 AND ?2
 UNION ALL SELECT id,2,date,title,project_id,project_title,project_short_name,short_title,category_id,NULL,NULL,1,0,updated_at,grouped FROM (
  SELECT e.id,e.date,e.title,e.project_id,COALESCE(p.title,'') AS project_title,COALESCE(pv.short_name,'') AS project_short_name,COALESCE(v.short_title,'') AS short_title,COALESCE(v.category_id,'{RECORD_CATEGORY}') AS category_id,e.updated_at,
   COUNT(*) OVER day_project AS grouped,ROW_NUMBER() OVER (day_project ORDER BY e.updated_at DESC,e.id) AS place
  FROM diary_entries e LEFT JOIN projects p ON p.id=e.project_id LEFT JOIN project_display pv ON pv.project_id=p.id LEFT JOIN entry_display v ON v.entry_id=e.id WHERE e.date BETWEEN ?1 AND ?2
  WINDOW day_project AS (PARTITION BY e.date,COALESCE(e.project_id,e.id))
 ) WHERE place=1
), ordered AS (SELECT heads.*,c.position FROM heads JOIN calendar_categories c ON c.id=heads.category_id)")
}
const COLUMNS:&str="id,source,date,title,project_id,project_title,project_short_name,short_title,category_id,start_time,end_time,all_day,completed,grouped";
const ORDER:&str="completed,position,all_day DESC,start_time,CASE WHEN source=2 THEN updated_at END DESC,title COLLATE NOCASE,source,id";
fn item(r: &Row<'_>) -> rusqlite::Result<CalendarItem> {
    Ok(CalendarItem {
        id: r.get(0)?,
        source: match r.get::<_, i32>(1)? {
            0 => CalendarSource::Event,
            1 => CalendarSource::Task,
            _ => CalendarSource::Entry,
        },
        date: r.get(2)?,
        title: r.get(3)?,
        project_id: r.get(4)?,
        project_title: r.get(5)?,
        project_short_name: r.get(6)?,
        short_title: r.get(7)?,
        category_id: r.get(8)?,
        start_time: r.get(9)?,
        end_time: r.get(10)?,
        all_day: r.get(11)?,
        completed: r.get(12)?,
        count: r.get(13)?,
    })
}
fn counts(c: &Connection, from: &str, to: &str) -> Result<BTreeMap<String, CalendarDay>, String> {
    // Each row is one calendar line; `members` is how many items that line stands for.
    let mut q=c.prepare("SELECT date,count(*),sum(completed),sum(members) FROM (SELECT date,completed,1 AS members FROM events WHERE date BETWEEN ?1 AND ?2 UNION ALL SELECT date,completed,1 FROM tasks WHERE date BETWEEN ?1 AND ?2 UNION ALL SELECT date,0,count(*) FROM diary_entries WHERE date BETWEEN ?1 AND ?2 GROUP BY date,COALESCE(project_id,id)) GROUP BY date").map_err(database_error)?;
    let rows = q
        .query_map(params![from, to], |r| {
            let date: String = r.get(0)?;
            Ok((
                date.clone(),
                CalendarDay {
                    date,
                    total: r.get(1)?,
                    completed: r.get(2)?,
                    represented: r.get(3)?,
                    items: vec![],
                },
            ))
        })
        .map_err(database_error)?
        .collect::<Result<_, _>>()
        .map_err(database_error)?;
    Ok(rows)
}
impl Storage {
    pub fn calendar_days(&self, from: &str, to: &str) -> Result<Vec<CalendarDay>, String> {
        validate_date(from)?;
        validate_date(to)?;
        let days = (NaiveDate::parse_from_str(to, "%Y-%m-%d").unwrap()
            - NaiveDate::parse_from_str(from, "%Y-%m-%d").unwrap())
        .num_days();
        if !(0..=365).contains(&days) {
            return Err("달력 조회는 최대 366일입니다.".into());
        }
        let mut c = self.connection()?;
        let tx = c.transaction().map_err(database_error)?;
        let mut days = counts(&tx, from, to)?;
        {
            let sql=format!("{}, ranked AS (SELECT *,ROW_NUMBER() OVER(PARTITION BY date ORDER BY {ORDER}) AS rank FROM ordered WHERE completed=0) SELECT {COLUMNS} FROM ranked WHERE rank<=16 ORDER BY date,rank",headers());
            let mut q = tx.prepare(&sql).map_err(database_error)?;
            for value in q
                .query_map(params![from, to], item)
                .map_err(database_error)?
            {
                let value = value.map_err(database_error)?;
                if let Some(day) = days.get_mut(&value.date) {
                    day.items.push(value);
                }
            }
        }
        tx.commit().map_err(database_error)?;
        Ok(days.into_values().collect())
    }
    pub fn calendar_page(&self, date: &str, offset: u32) -> Result<CalendarPage, String> {
        self.calendar_page_inner(date, offset, None)
    }
    pub fn calendar_section(
        &self,
        date: &str,
        offset: u32,
        completed: bool,
    ) -> Result<CalendarPage, String> {
        self.calendar_page_inner(date, offset, Some(completed))
    }
    fn calendar_page_inner(
        &self,
        date: &str,
        offset: u32,
        completed: Option<bool>,
    ) -> Result<CalendarPage, String> {
        validate_date(date)?;
        let mut c = self.connection()?;
        let tx = c.transaction().map_err(database_error)?;
        let count = counts(&tx, date, date)?.remove(date).unwrap_or_default();
        let items = {
            let sql = format!(
                "{} SELECT {COLUMNS} FROM ordered WHERE (?4 IS NULL OR completed=?4) ORDER BY {ORDER} LIMIT 50 OFFSET ?3",
                headers()
            );
            let mut q = tx.prepare(&sql).map_err(database_error)?;
            let rows = q
                .query_map(params![date, date, offset, completed], item)
                .map_err(database_error)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(database_error)?;
            rows
        };
        let section_total = match completed {
            None => count.total,
            Some(true) => count.completed,
            Some(false) => count.total - count.completed,
        };
        let has_more = (offset as u64 + items.len() as u64) < section_total as u64;
        tx.commit().map_err(database_error)?;
        Ok(CalendarPage {
            items,
            total: count.total,
            completed: count.completed,
            has_more,
        })
    }
}
