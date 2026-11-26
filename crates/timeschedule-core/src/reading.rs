//! Reading what the app keeps, for the `record` commands that only read: the diary
//! records of projects, the evidence of a daily record, the dialogue of a day, and the
//! lines of a kept original. Nothing here writes. The database is read through
//! `Storage::open_read_only`, and the document store by the rules of the day transcript.
//!
//! Every answer is a page. Its text is held to a budget of characters, and `next` says
//! what to ask for to read on; a page always holds at least one item.
use crate::storage::validate_text;
use chrono::NaiveDate;
use serde::Serialize;
use serde_json::Value;
use std::{collections::BTreeMap, path::Path};

mod dialogue;
mod records;
pub use dialogue::{read_day, read_original, search_days, store_counts};

/// Characters of text a page holds when the caller names no budget. Claude Code cuts a
/// command's output at 30,000 characters; a page this size leaves room for the rest.
pub const DEFAULT_BUDGET: usize = 20_000;
/// Rows of the database read for one page of a list, before the budget is applied.
const PAGE_ROWS: u32 = 200;
/// Days one search of the dialogue reads. A day takes about a second to read.
pub const MAX_SEARCH_DAYS: i64 = 31;
/// What one line of a kept original is read up to, as the day transcript does.
const LINE_LIMIT: u64 = 8 * 1024 * 1024;
/// Characters of a search excerpt before and after what was found.
const BEFORE: usize = 60;
const AFTER: usize = 140;
/// Warnings given in one answer; `warningsTotal` says how many there were.
const WARNINGS: usize = 20;
/// Excerpts given for one record.
const EXCERPTS: usize = 3;

/// `work_id`, `started_at`, `completed_at`, `status` and `commits` of an agent record.
type AgentRow = (
    Option<String>,
    Option<String>,
    Option<String>,
    String,
    String,
);

/// Where a diary record came from, read from its `source_ref`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Origin {
    /// 하루 정리: a daily record shown in the diary.
    Daily,
    /// 자동 개발 기록: a record an agent delivered when it finished a task.
    Agent,
    /// 확인한 경험: an experience a person confirmed from a chat review.
    Experience,
    /// 채팅 발췌: the start and end of one chat session.
    Chat,
    /// 직접 쓴 기록.
    Manual,
    Other,
}

impl Origin {
    pub const ALL: [Origin; 6] = [
        Self::Daily,
        Self::Agent,
        Self::Experience,
        Self::Chat,
        Self::Manual,
        Self::Other,
    ];
    /// What is read when no origin is named: all but the chat excerpts. They are most
    /// of the text, and `transcript` reads the same dialogue whole and by speaker.
    pub const DEFAULT: [Origin; 5] = [
        Self::Daily,
        Self::Agent,
        Self::Experience,
        Self::Manual,
        Self::Other,
    ];
    pub fn key(self) -> &'static str {
        match self {
            Self::Daily => "daily",
            Self::Agent => "agent",
            Self::Experience => "experience",
            Self::Chat => "chat",
            Self::Manual => "manual",
            Self::Other => "other",
        }
    }
    pub fn parse(text: &str) -> Result<Self, String> {
        Self::ALL
            .into_iter()
            .find(|o| o.key() == text)
            .ok_or_else(|| {
                let keys: Vec<&str> = Self::ALL.iter().map(|o| o.key()).collect();
                format!("기록 출처는 {} 중 하나여야 합니다: {text}", keys.join(", "))
            })
    }
    fn of(key: &str) -> Self {
        Self::parse(key).unwrap_or(Self::Other)
    }
}

/// The origin of `e`, a row of `diary_entries`, as SQL. The prefixes are those the code
/// that writes each kind of record gives its `source_ref`.
const ORIGIN: &str = "CASE
    WHEN e.source_ref IS NULL THEN 'manual'
    WHEN e.source_ref LIKE 'daily-record:%' THEN 'daily'
    WHEN e.source_ref LIKE 'space-work:%' THEN 'agent'
    WHEN e.source_ref LIKE 'codex-chat:%' OR e.source_ref LIKE 'claude-code-chat:%' THEN 'chat'
    WHEN e.source_ref LIKE 'chat-experience:%' OR e.source_ref LIKE 'manual-review:%' THEN 'experience'
    ELSE 'other' END";

/// Which records a list or a search reads. Only records of a project are read: the
/// diary's other records and the calendar are not this command's to show.
#[derive(Debug, Clone)]
pub struct Filter<'a> {
    /// A project ID, as `Storage::project_of` gives it.
    pub project: Option<&'a str>,
    pub from: Option<NaiveDate>,
    pub to: Option<NaiveDate>,
    pub origins: &'a [Origin],
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordRow {
    pub id: String,
    pub project: String,
    pub date: String,
    pub kind: String,
    pub origin: Origin,
    pub title: String,
    /// Characters of the body.
    pub chars: usize,
    pub updated_at: String,
    /// The start of the body on one line. A list has it; a search has excerpts.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    /// How many times the words are in the body. A search has it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub matches: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub excerpts: Option<Vec<String>>,
}

/// Where to go on from: the same command with `--offset`.
#[derive(Debug, Clone, Serialize)]
pub struct Offset {
    pub offset: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct Listing {
    pub total: usize,
    pub offset: usize,
    pub records: Vec<RecordRow>,
    pub next: Option<Offset>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectRow {
    pub id: String,
    pub title: String,
    pub folder: String,
    /// Whether the project's chats are collected.
    pub capture: bool,
    /// Records of each origin.
    pub records: BTreeMap<&'static str, usize>,
    pub first_date: Option<String>,
    pub last_date: Option<String>,
    /// Files listed in the kept originals of the project's folder, and days of daily
    /// records. 0 when the folder is not there; `None` when the document store was
    /// not read, or the folder could not be read.
    pub originals: Option<usize>,
    pub daily_days: Option<usize>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DailyRef {
    /// The record file, relative to the document store.
    pub file: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentRef {
    pub work_id: Option<String>,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    pub status: Option<String>,
    /// The commits the record owns, as the diary keeps them.
    pub commits: Value,
    /// The original `record.md`. The caller finds it through the store's catalog.
    pub document: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub document_error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordView {
    pub id: String,
    pub project: String,
    pub date: String,
    pub kind: String,
    pub origin: Origin,
    pub title: String,
    pub updated_at: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub source_url: String,
    pub chars: usize,
    pub offset: usize,
    pub body: String,
    pub next: Option<Offset>,
    /// For a daily record: whether a person changed the diary text the app last put in.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub edited: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub daily: Option<DailyRef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent: Option<AgentRef>,
}

/// A cited block of a daily record: where its words are in the kept original.
#[derive(Debug, Clone, Serialize)]
pub struct BlockRef {
    pub block: usize,
    pub speaker: &'static str,
    pub at: String,
    pub file: String,
    pub line: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct EvidenceView {
    pub id: String,
    pub file: String,
    pub date: String,
    pub revision: u32,
    pub total: usize,
    pub offset: usize,
    /// The record's items in the order the diary shows them, each with the blocks it
    /// cites by number.
    pub items: Vec<Value>,
    /// The blocks the items of this page cite.
    pub blocks: Vec<BlockRef>,
    pub next: Option<Offset>,
}

#[derive(Debug, Clone, Serialize)]
pub struct BlockView {
    pub date: String,
    pub number: usize,
    /// RFC 3339, Korean time.
    pub at: String,
    pub speaker: &'static str,
    pub source: String,
    pub file: String,
    pub line: usize,
    /// The day transcript cut the text and ended it with `…`.
    pub clipped: bool,
    pub text: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProjectName {
    pub id: String,
    pub title: String,
}

/// Which blocks of a day to give.
#[derive(Debug, Clone)]
pub enum Pick<'a> {
    All,
    /// The block at a line of a kept file, with `context` blocks on each side. A line
    /// no block starts at means the last block of that file before it.
    Around {
        file: &'a str,
        line: usize,
        context: usize,
    },
    Matching(&'a str),
}

#[derive(Debug, Clone, Serialize)]
pub struct DayView {
    pub project: ProjectName,
    pub date: String,
    /// The block `Pick::Around` found.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub focus: Option<usize>,
    pub total: usize,
    pub offset: usize,
    pub blocks: Vec<BlockView>,
    pub warnings: Vec<String>,
    #[serde(rename = "warningsTotal")]
    pub warnings_total: usize,
    pub next: Option<Offset>,
}

/// Where a search of several days goes on from: the same command with `--from` and
/// `--offset`, which counts the blocks of that day already given.
#[derive(Debug, Clone, Serialize)]
pub struct DayOffset {
    pub from: String,
    pub offset: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchView {
    pub project: ProjectName,
    pub from: String,
    pub to: String,
    pub days_read: usize,
    pub blocks: Vec<BlockView>,
    pub warnings: Vec<String>,
    pub warnings_total: usize,
    pub next: Option<DayOffset>,
}

#[derive(Debug, Clone, Serialize)]
pub struct LineView {
    pub line: usize,
    /// Characters of the line, as far as it was read.
    pub chars: usize,
    pub offset: usize,
    pub text: String,
    /// The line goes on past `text`.
    pub clipped: bool,
    /// The line is longer than 8 MiB; only its first 8 MiB were read.
    pub long: bool,
    /// Bytes of the line that are not UTF-8 text were shown as `�`.
    pub lossy: bool,
}

/// Where to go on from: the same command with `--line`, `--offset` and `--lines`, which
/// is how many of the lines asked for are left.
#[derive(Debug, Clone, Serialize)]
pub struct LineOffset {
    pub line: usize,
    #[serde(skip_serializing_if = "is_zero")]
    pub offset: usize,
    pub lines: usize,
}

fn is_zero(n: &usize) -> bool {
    *n == 0
}

#[derive(Debug, Clone, Serialize)]
pub struct OriginalView {
    pub project: ProjectName,
    pub file: String,
    /// Earlier contents of the file kept beside it (`<name>.prev-<time>`), relative to
    /// the project's originals folder. The file was replaced by content that did not
    /// go on from them, so a line cited before then may now hold other text. `None`
    /// when the folder could not be listed: then nothing says there are none.
    pub earlier: Option<Vec<String>>,
    pub lines: Vec<LineView>,
    pub next: Option<LineOffset>,
}

/// The first of `items` whose JSON fits in `budget` characters together, and always the
/// first one. The second value says whether any was left out.
pub fn fit<T: Serialize>(items: impl IntoIterator<Item = T>, budget: usize) -> (Vec<T>, bool) {
    let mut used = 0;
    let mut kept = Vec::new();
    for item in items {
        let size = serde_json::to_string(&item).map_or(0, |s| s.chars().count());
        if !kept.is_empty() && used + size > budget {
            return (kept, true);
        }
        used += size;
        kept.push(item);
    }
    (kept, false)
}

/// Characters of the JSON of `value`.
fn chars_of(value: &impl Serialize) -> usize {
    serde_json::to_string(value).map_or(0, |s| s.chars().count())
}

/// The first `WARNINGS` of `warnings`, and how many there were.
fn capped(mut warnings: Vec<String>) -> (Vec<String>, usize) {
    let total = warnings.len();
    warnings.truncate(WARNINGS);
    (warnings, total)
}

/// The text for finding words without regard to case: each letter in lower case where
/// that is one letter, or with `every_letter` false only ASCII letters, as SQLite's
/// `lower()` does.
fn folded(text: &str, every_letter: bool) -> Vec<char> {
    text.chars()
        .map(|c| {
            if !every_letter {
                return c.to_ascii_lowercase();
            }
            let mut lower = c.to_lowercase();
            match (lower.next(), lower.next()) {
                (Some(one), None) => one,
                _ => c,
            }
        })
        .collect()
}

/// Where `needle` starts in `hay`, without overlaps, as character positions.
fn found_at(hay: &[char], needle: &[char]) -> Vec<usize> {
    let mut at = Vec::new();
    if needle.is_empty() || needle.len() > hay.len() {
        return at;
    }
    let mut i = 0;
    while i + needle.len() <= hay.len() {
        if hay[i..i + needle.len()] == *needle {
            at.push(i);
            i += needle.len();
        } else {
            i += 1;
        }
    }
    at
}

fn contains(text: &str, query: &str) -> bool {
    !found_at(&folded(text, true), &folded(query, true)).is_empty()
}

/// The words in `query` trimmed, as the app's search takes them.
fn query_words(query: &str) -> Result<&str, String> {
    let query = query.trim();
    if query.is_empty() {
        return Err("검색어가 비어 있습니다.".into());
    }
    validate_text(query, "검색어", 200)?;
    Ok(query)
}

/// Excerpts of `body` around the first places `query` is found, each on one line, and
/// how many places there are. `every_letter` is as for `folded`.
fn excerpts(body: &str, query: &str, every_letter: bool) -> (usize, Vec<String>) {
    let chars: Vec<char> = body.chars().collect();
    let needle = folded(query, every_letter);
    let at = found_at(&folded(body, every_letter), &needle);
    let shown = at
        .iter()
        .take(EXCERPTS)
        .map(|&start| {
            let from = start.saturating_sub(BEFORE);
            let to = (start + needle.len() + AFTER).min(chars.len());
            let text: String = chars[from..to].iter().collect();
            let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
            format!(
                "{}{flat}{}",
                if from > 0 { "…" } else { "" },
                if to < chars.len() { "…" } else { "" }
            )
        })
        .collect();
    (at.len(), shown)
}

#[cfg(test)]
mod tests {
    use super::records::daily_place;
    use super::*;
    use crate::storage::Storage;

    #[test]
    fn words_are_found_without_regard_to_case_and_without_overlaps() {
        assert_eq!(found_at(&folded("aAa", true), &folded("aa", true)), vec![0]);
        assert!(contains("UNIQUE constraint", "unique"));
        assert!(!contains("하루 정리", "정리 하루"));
        // As SQLite's lower(): only ASCII letters.
        assert_eq!(excerpts("é", "É", false).0, 0);
        assert_eq!(excerpts("é", "É", true).0, 1);
        let (n, shown) = excerpts("앞\n\n오류가 났다 그리고 또 오류", "오류", true);
        assert_eq!(n, 2);
        assert_eq!(shown[0], "앞 오류가 났다 그리고 또 오류");
    }

    #[test]
    fn a_read_only_database_writes_no_temporary_file() {
        let path = std::env::temp_dir().join(format!("ts-read-only-{}.db", uuid::Uuid::new_v4()));
        drop(Storage::open(&path).unwrap());
        let store = Storage::open_read_only(&path).unwrap();
        let c = store.connection().unwrap();
        let read = |pragma: &str| -> i64 { c.query_row(pragma, [], |r| r.get(0)).unwrap() };
        // 2 is MEMORY.
        assert_eq!(read("PRAGMA temp_store"), 2);
        assert_eq!(read("PRAGMA query_only"), 1);
        drop(c);
        drop(store);
        for suffix in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{}{suffix}", path.display()));
        }
    }

    #[test]
    fn a_page_always_holds_its_first_item() {
        let (kept, cut) = fit(["가".repeat(50), "나".into()], 10);
        assert_eq!(kept.len(), 1);
        assert!(cut);
        let (kept, cut) = fit(["a", "b"], 100);
        assert_eq!(kept, ["a", "b"]);
        assert!(!cut);
    }

    #[test]
    fn only_a_daily_reference_of_the_written_shape_names_a_file() {
        assert_eq!(
            daily_place("daily-record:p:c--work-0123:2026-11-20"),
            Some(("p", "c--work-0123", "2026-11-20"))
        );
        assert_eq!(daily_place("daily-record:p:../x:2026-11-20"), None);
        assert_eq!(daily_place("daily-record:p:c--work:2026-13-01"), None);
        assert_eq!(daily_place("space-work:x"), None);
    }
}
