//! When a diary record happened and which real commits back it.
//!
//! Two kinds of time are kept apart from the editable display copy:
//! - `entry_time`: the time of day a person typed for a hand-written record.
//! - `entry_evidence`: facts that came with a record from its source — a Git
//!   commit's time, an automatic development record's begin receipt, confirmed
//!   completion and owned commits. Users cannot edit these.
//!
//! A time nobody recorded stays unknown (`None`). Nothing here fills in "now".
use crate::storage::{database_error, validate_date, validate_id, validate_time, Storage};
use chrono::{DateTime, FixedOffset, NaiveDate, TimeZone};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::time::{Duration, Instant};

/// Korea Standard Time has no daylight saving: a fixed +09:00.
const KST_SECONDS: i32 = 9 * 3600;
pub const UNKNOWN: &str = "미상";
pub const MAX_COMMITS: usize = 200;
/// The line `development-diary.mjs finish` writes with the owned commits.
pub const OWNED_COMMITS_LINE: &str = "작업 커밋 · ";

pub fn kst() -> FixedOffset {
    FixedOffset::east_opt(KST_SECONDS).expect("valid fixed offset")
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RecordCommit {
    pub hash: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub committed_at: Option<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub subject: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub url: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EntryTimeline {
    /// `HH:MM` in KST on the record's date, typed by a person. Only a
    /// hand-written record has one; a sourced record never shows or takes it.
    pub manual_time: Option<String>,
    /// The record came from a source (a collected Git commit or an automatic
    /// development record). Its source time is the reference, even after edits.
    pub sourced: bool,
    /// When the source says it happened (a collected Git commit's time).
    pub occurred_at: Option<String>,
    /// The automatic development record this entry displays.
    pub work_id: Option<String>,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    /// Work status code of an automatic record; empty otherwise.
    pub status: String,
    pub commits: Vec<RecordCommit>,
    /// The display copy no longer equals the last automatic delivery.
    pub edited: bool,
}

impl EntryTimeline {
    pub fn is_automatic(&self) -> bool {
        self.work_id.is_some()
    }
    /// A typed time of day belongs only to a hand-written record.
    pub fn takes_manual_time(&self) -> bool {
        !self.sourced && self.work_id.is_none()
    }
    /// The confirmed completion moment, shown only while the status is a
    /// completed one. A first completion stays stored if the status changes.
    pub fn shown_completion(&self) -> Option<&str> {
        self.completed_at
            .as_deref()
            .filter(|_| completes(&self.status))
    }
}

/// An RFC 3339 instant with an explicit offset (`Z` or `+09:00`).
pub fn parse_instant(value: &str) -> Result<DateTime<FixedOffset>, String> {
    if value.len() > 40 || value.contains('\0') {
        return Err("시각 형식을 확인해 주세요.".into());
    }
    DateTime::parse_from_rfc3339(value)
        .map_err(|_| "시각은 시간대가 있는 RFC 3339 형식이어야 합니다.".into())
}

/// `2026-11-19 14:05` in KST, 24-hour, to the minute. Unknown otherwise.
pub fn kst_minute(value: Option<&str>) -> String {
    value
        .and_then(|v| parse_instant(v).ok())
        .map(|t| t.with_timezone(&kst()).format("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_else(|| UNKNOWN.into())
}

/// The KST calendar date of an instant.
pub fn kst_date(value: &str) -> Option<String> {
    parse_instant(value)
        .ok()
        .map(|t| t.with_timezone(&kst()).format("%Y-%m-%d").to_string())
}

/// The instant a person meant by a date and a KST time of day.
pub fn manual_instant(date: &str, time: &str) -> Result<String, String> {
    validate_date(date)?;
    let time = validate_time(time)?;
    let date = NaiveDate::parse_from_str(date, "%Y-%m-%d").map_err(|e| e.to_string())?;
    let local = kst()
        .from_local_datetime(&date.and_time(time))
        .single()
        .ok_or("시각을 확인해 주세요.")?;
    Ok(local.to_rfc3339())
}

/// The reference time of a record that is not an automatic one: a sourced
/// record's own source time (unknown stays unknown, a typed time never
/// replaces it), otherwise the time a person typed. A record with only a date
/// is unknown; its save time is never used.
pub fn record_time(date: &str, timeline: &EntryTimeline) -> String {
    if timeline.sourced {
        return kst_minute(timeline.occurred_at.as_deref());
    }
    match &timeline.manual_time {
        Some(time) => manual_instant(date, time)
            .map(|v| kst_minute(Some(&v)))
            .unwrap_or_else(|_| UNKNOWN.into()),
        None => kst_minute(timeline.occurred_at.as_deref()),
    }
}

/// The line that follows a record's title: the begin receipt of an automatic
/// record, else its source or typed time.
pub fn reference_line(date: &str, timeline: &EntryTimeline) -> String {
    if timeline.is_automatic() {
        format!("작업 시작 · {}", kst_minute(timeline.started_at.as_deref()))
    } else {
        format!("기록 시각 · {}", record_time(date, timeline))
    }
}

/// The completion or current-status line of an automatic record. A completed
/// status also names which kind of completion it is.
pub fn completion_line(timeline: &EntryTimeline) -> Option<String> {
    if !timeline.is_automatic() {
        return None;
    }
    Some(if completes(&timeline.status) {
        format!(
            "완료 · {} · {}",
            kst_minute(timeline.shown_completion()),
            status_label(&timeline.status)
        )
    } else {
        format!("현재 상태 · {}", status_label(&timeline.status))
    })
}

pub fn status_label(code: &str) -> &'static str {
    match code {
        "in_progress" => "진행 중",
        "verification_pending" => "검증 대기",
        "verified" => "확인한 범위 검증 완료",
        "completed" => "작업 정리 완료",
        "merged" => "병합 완료",
        _ => UNKNOWN,
    }
}

/// Statuses that may carry a confirmed completion time.
pub fn completes(status: &str) -> bool {
    matches!(status, "verified" | "completed" | "merged")
}

pub fn valid_hash(hash: &str) -> bool {
    matches!(hash.len(), 40 | 64)
        && hash
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

pub fn short_hash(hash: &str) -> &str {
    &hash[..hash.len().min(7)]
}

fn valid_commit_url(url: &str, hash: &str) -> bool {
    let Ok(parsed) = url::Url::parse(url) else {
        return false;
    };
    parsed.scheme() == "https"
        && parsed.host_str().is_some()
        && parsed.username().is_empty()
        && parsed.password().is_none()
        && parsed.query().is_none()
        && parsed.fragment().is_none()
        && parsed.path().ends_with(&format!("/commit/{hash}"))
}

pub fn validate_commits(commits: &[RecordCommit]) -> Result<(), String> {
    if commits.len() > MAX_COMMITS {
        return Err(format!("관련 커밋은 최대 {MAX_COMMITS}개입니다."));
    }
    let mut seen = std::collections::HashSet::new();
    for commit in commits {
        if !valid_hash(&commit.hash) || !seen.insert(commit.hash.as_str()) {
            return Err("관련 커밋 해시를 확인해 주세요.".into());
        }
        if let Some(at) = &commit.committed_at {
            parse_instant(at)?;
        }
        if commit.subject.chars().count() > 200 || commit.subject.chars().any(char::is_control) {
            return Err("커밋 제목은 200자 이하 한 줄이어야 합니다.".into());
        }
        if !commit.url.is_empty() && !valid_commit_url(&commit.url, &commit.hash) {
            return Err("커밋 링크는 해당 커밋을 가리키는 https 주소여야 합니다.".into());
        }
    }
    Ok(())
}

/// Owned commit hashes from the line `finish` adds to a record's detail.
pub fn owned_commits_in(detail: &str) -> Vec<RecordCommit> {
    let mut commits: Vec<RecordCommit> = Vec::new();
    for line in detail.lines() {
        let Some(list) = line.trim().strip_prefix(OWNED_COMMITS_LINE) else {
            continue;
        };
        for hash in list.split(',').map(str::trim) {
            if valid_hash(hash) && !commits.iter().any(|c| c.hash == hash) {
                commits.push(RecordCommit {
                    hash: hash.into(),
                    committed_at: None,
                    subject: String::new(),
                    url: String::new(),
                });
            }
        }
    }
    commits.truncate(MAX_COMMITS);
    commits
}

pub(crate) fn migrate_v7(c: &Connection) -> Result<(), String> {
    c.execute_batch(
        "CREATE TABLE entry_time(
            entry_id TEXT PRIMARY KEY NOT NULL REFERENCES diary_entries(id) ON DELETE CASCADE,
            time TEXT NOT NULL);
         CREATE TABLE entry_evidence(
            entry_id TEXT PRIMARY KEY NOT NULL REFERENCES diary_entries(id) ON DELETE CASCADE,
            work_id TEXT, occurred_at TEXT, started_at TEXT, completed_at TEXT,
            status TEXT NOT NULL DEFAULT '', commits TEXT NOT NULL DEFAULT '[]');
         CREATE INDEX entry_evidence_work ON entry_evidence(work_id);",
    )
    .map_err(database_error)?;
    restore_collected_commits(c)?;
    restore_delivered_records(c)?;
    c.execute_batch("PRAGMA user_version=7;")
        .map_err(database_error)
}

/// Collected Git records keep their hash in `source_ref` and the commit message
/// in the immutable `source_body`: both are exact. Their time was never stored,
/// so it stays unknown until the next collection fills it from Git.
fn restore_collected_commits(c: &Connection) -> Result<(), String> {
    let rows: Vec<(String, String, String)> = {
        let mut q = c
            .prepare("SELECT id,source_ref,source_body FROM diary_entries WHERE source_ref IS NOT NULL AND source_ref NOT LIKE 'space-work:%'")
            .map_err(database_error)?;
        let rows = q
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .map_err(database_error)?
            .collect::<Result<_, _>>()
            .map_err(database_error)?;
        rows
    };
    for (id, source_ref, source_body) in rows {
        let Some((_, hash)) = source_ref.rsplit_once('#') else {
            continue;
        };
        if !valid_hash(hash) {
            continue;
        }
        let commit = RecordCommit {
            hash: hash.into(),
            committed_at: None,
            subject: commit_subject(&source_body),
            url: String::new(),
        };
        c.execute(
            "INSERT OR IGNORE INTO entry_evidence(entry_id,commits) VALUES(?1,?2)",
            params![id, commits_json(&[commit])?],
        )
        .map_err(database_error)?;
    }
    Ok(())
}

/// Delivered automatic records keep their last accepted payload. Its work ID,
/// status and the owned-commit line are exact; start, completion and commit
/// times were not part of earlier payloads and stay unknown.
fn restore_delivered_records(c: &Connection) -> Result<(), String> {
    let rows: Vec<(String, String, String)> = {
        let mut q = c
            .prepare("SELECT work_id,payload,entry_id FROM project_record_receipts")
            .map_err(database_error)?;
        let rows = q
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .map_err(database_error)?
            .collect::<Result<_, _>>()
            .map_err(database_error)?;
        rows
    };
    for (work_id, payload, entry_id) in rows {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&payload) else {
            continue;
        };
        let status = value["status"].as_str().unwrap_or_default();
        let status = if status_label(status) == UNKNOWN {
            ""
        } else {
            status
        };
        let commits = owned_commits_in(value["detail"].as_str().unwrap_or_default());
        // Only a row that still exists; a user's deletion stays deleted.
        c.execute(
            "INSERT OR IGNORE INTO entry_evidence(entry_id,work_id,status,commits)
             SELECT id,?2,?3,?4 FROM diary_entries WHERE id=?1",
            params![entry_id, work_id, status, commits_json(&commits)?],
        )
        .map_err(database_error)?;
    }
    Ok(())
}

pub(crate) fn commit_subject(message: &str) -> String {
    message
        .lines()
        .next()
        .unwrap_or_default()
        .chars()
        .filter(|c| !c.is_control())
        .take(200)
        .collect()
}

pub(crate) fn commits_json(commits: &[RecordCommit]) -> Result<String, String> {
    serde_json::to_string(commits).map_err(|e| e.to_string())
}

/// Read one record's times and evidence. A missing row means all unknown.
pub(crate) fn read_timeline(c: &Connection, id: &str) -> Result<EntryTimeline, String> {
    let manual_time: Option<String> = c
        .query_row("SELECT time FROM entry_time WHERE entry_id=?1", [id], |r| {
            r.get(0)
        })
        .optional()
        .map_err(database_error)?;
    let evidence: Option<(Option<String>, Option<String>, Option<String>, Option<String>, String, String)> = c
        .query_row(
            "SELECT work_id,occurred_at,started_at,completed_at,status,commits FROM entry_evidence WHERE entry_id=?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)),
        )
        .optional()
        .map_err(database_error)?;
    // A collected record keeps its source reference; evidence marks the rest.
    let collected: bool = c
        .query_row(
            "SELECT source_ref IS NOT NULL FROM diary_entries WHERE id=?1",
            [id],
            |r| r.get(0),
        )
        .optional()
        .map_err(database_error)?
        .unwrap_or(false);
    let sourced = collected || evidence.is_some();
    let mut timeline = EntryTimeline {
        // A typed time is never shown for, or applied to, a sourced record.
        manual_time: if sourced { None } else { manual_time },
        sourced,
        ..Default::default()
    };
    if let Some((work_id, occurred_at, started_at, completed_at, status, commits)) = evidence {
        timeline.work_id = work_id;
        timeline.occurred_at = occurred_at;
        timeline.started_at = started_at;
        timeline.completed_at = completed_at;
        timeline.status = status;
        // A damaged list is shown as no commits rather than failing the record.
        timeline.commits = serde_json::from_str(&commits).unwrap_or_default();
    }
    if let Some(work_id) = &timeline.work_id {
        timeline.edited = edited_since_delivery(c, id, work_id)?;
    }
    Ok(timeline)
}

fn edited_since_delivery(c: &Connection, id: &str, work_id: &str) -> Result<bool, String> {
    let snapshot: Option<String> = c
        .query_row(
            "SELECT managed_snapshot FROM project_record_receipts WHERE work_id=?1 AND entry_id=?2",
            params![work_id, id],
            |r| r.get(0),
        )
        .optional()
        .map_err(database_error)?;
    let Some(snapshot) = snapshot else {
        return Ok(false);
    };
    let Ok(managed) = serde_json::from_str::<serde_json::Value>(&snapshot) else {
        return Ok(false);
    };
    let current: Option<serde_json::Value> = c
        .query_row(
            "SELECT project_id,date,kind,title,body,source_url FROM diary_entries WHERE id=?1",
            [id],
            |r| {
                Ok(serde_json::json!({
                    "project_id": r.get::<_, Option<String>>(0)?,
                    "date": r.get::<_, String>(1)?,
                    "kind": r.get::<_, String>(2)?,
                    "title": r.get::<_, String>(3)?,
                    "body": r.get::<_, String>(4)?,
                    "source_url": r.get::<_, String>(5)?,
                }))
            },
        )
        .optional()
        .map_err(database_error)?;
    Ok(current.is_some_and(|current| current != managed))
}

/// Store, keep or clear a person's time of day for a record.
pub(crate) fn write_manual_time(
    c: &Connection,
    id: &str,
    change: &TimeChange,
) -> Result<(), String> {
    match change {
        TimeChange::Keep => Ok(()),
        TimeChange::Clear => c
            .execute("DELETE FROM entry_time WHERE entry_id=?1", [id])
            .map(|_| ())
            .map_err(database_error),
        TimeChange::Set(time) => {
            validate_time(time)?;
            c.execute(
                "INSERT INTO entry_time(entry_id,time) VALUES(?1,?2) ON CONFLICT(entry_id) DO UPDATE SET time=excluded.time",
                params![id, time],
            )
            .map(|_| ())
            .map_err(database_error)
        }
    }
}

/// What saving a record does with its typed time of day.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TimeChange {
    Keep,
    Clear,
    Set(String),
}

impl TimeChange {
    /// From an editor field: blank clears, `HH:MM` sets.
    pub fn from_field(value: &str) -> Result<Self, String> {
        let value = value.trim();
        if value.is_empty() {
            return Ok(Self::Clear);
        }
        validate_time(value).map_err(|_| "시각은 24시간제 HH:mm으로 입력해 주세요.")?;
        Ok(Self::Set(value.into()))
    }
}

/// What a begin receipt says about a work: when it began, who began it and
/// the commit it began from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BeginReceipt {
    pub at: String,
    pub owner: Option<String>,
    pub base: Option<String>,
}

const RECEIPT_BYTES: u64 = 16 * 1024;
const RECEIPT_SCAN: usize = 5000;
/// Commits checked with Git for one record in one pass; the rest wait for
/// the next opening.
const BACKFILL_COMMITS: usize = 50;
/// The longest a single Git call of a check may run, within the pass's deadline.
const GIT_CHECK_LIMIT: Duration = Duration::from_secs(2);

/// How far completing one record's evidence got, kept by the caller between
/// openings. Commits Git did not confirm are remembered only here — never
/// written as confirmed — so the next opening continues with the commits
/// after them instead of checking the same ones again. A pass cut short by
/// its deadline leaves the unchecked commits for the next opening.
#[derive(Debug, Default, Clone)]
pub struct BackfillProgress {
    /// The record's stored commit list this progress belongs to.
    commits: Option<String>,
    refused: HashSet<String>,
    finished: bool,
}

impl BackfillProgress {
    /// Every commit without a time was checked, or nothing more can be.
    pub fn finished(&self) -> bool {
        self.finished
    }
    /// Commits Git checked and did not confirm as this work's.
    pub fn refused(&self) -> &HashSet<String> {
        &self.refused
    }
}

/// What a Git check of one commit found.
enum Check {
    Owned(RecordCommit),
    Refused,
    /// Out of time, Git could not run, or the repository could not be opened:
    /// not checked, try again later.
    Interrupted,
}

/// One Git call within what is left of `deadline`, bounded by `GIT_CHECK_LIMIT`.
fn git_within(repo: &Path, args: &[&str], deadline: Instant) -> Result<String, Check> {
    let left = deadline
        .saturating_duration_since(Instant::now())
        .min(GIT_CHECK_LIMIT);
    if left.is_zero() {
        return Err(Check::Interrupted);
    }
    let began = Instant::now();
    crate::diary::git_with_timeout(repo, args, left).map_err(|error| {
        if began.elapsed() >= left || error.starts_with("Git 실행") {
            Check::Interrupted
        } else {
            Check::Refused
        }
    })
}

/// One Git call within what is left of `deadline`. Git answering no about a
/// repository it can still open is `Refused`. Running out of time, failing to
/// start Git, or a repository Git cannot open right now (an unplugged drive,
/// a dropped network share) is `Interrupted`, so the commit is checked again
/// on a later opening. Reachability is asked of Git itself after a failure,
/// not read from its (translated) messages.
fn bounded_git(repo: &Path, args: &[&str], deadline: Instant) -> Result<String, Check> {
    match git_within(repo, args, deadline) {
        Err(Check::Refused) => match git_within(repo, &["rev-parse", "--git-dir"], deadline) {
            Ok(_) => Err(Check::Refused),
            Err(_) => Err(Check::Interrupted),
        },
        other => other,
    }
}

fn small_json(path: &Path) -> Option<serde_json::Value> {
    use std::io::Read;
    let meta = std::fs::symlink_metadata(path).ok()?;
    if !meta.is_file() || meta.len() > RECEIPT_BYTES {
        return None;
    }
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .ok()?
        .take(RECEIPT_BYTES + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    let text = String::from_utf8(bytes).ok()?;
    serde_json::from_str(text.trim_start_matches('\u{feff}')).ok()
}

/// The begin receipt `development-diary.mjs begin` wrote for this work inside
/// the record state folder the app trusts, or none.
pub fn begin_receipt(state_dir: &Path, work_id: &str) -> Option<BeginReceipt> {
    let starts = std::fs::read_dir(state_dir.join("diary-starts")).ok()?;
    for entry in starts.flatten().take(RECEIPT_SCAN) {
        // `DirEntry::file_type` does not follow links: a linked folder is skipped.
        if !entry.file_type().is_ok_and(|t| t.is_dir()) {
            continue;
        }
        let dir = entry.path();
        let Some(work) = small_json(&dir.join("work.json")) else {
            continue;
        };
        if work["workId"].as_str() != Some(work_id) {
            continue;
        }
        let start = small_json(&dir.join("start.json"))?;
        let at = start["at"].as_str().filter(|v| parse_instant(v).is_ok())?;
        return Some(BeginReceipt {
            at: at.into(),
            owner: start["owner"]
                .as_str()
                .map(str::to_lowercase)
                .filter(|o| !o.is_empty() && !o.chars().any(char::is_control)),
            base: start["base"]
                .as_str()
                .filter(|b| valid_hash(b))
                .map(Into::into),
        });
    }
    None
}

/// A commit's real time and subject when Git confirms this work owns it: a
/// single-parent commit after the work's base, with exactly this work's
/// `Diary-Work-Id` trailer, by the author the begin receipt names.
fn verify_owned(
    repo: &Path,
    hash: &str,
    work_id: &str,
    base: &str,
    owner: &str,
    deadline: Instant,
) -> Check {
    if !valid_hash(hash) || hash == base {
        return Check::Refused;
    }
    let target = format!("{hash}^{{commit}}");
    let out = match bounded_git(
        repo,
        &[
            "-c",
            "i18n.logOutputEncoding=UTF-8",
            "show",
            "-s",
            "--no-show-signature",
            "--format=%P%x00%ae%x00%cI%x00%s%x00%(trailers:key=Diary-Work-Id,valueonly)",
            &target,
        ],
        deadline,
    ) {
        Ok(out) => out,
        Err(stop) => return stop,
    };
    let parts: Vec<&str> = out.split('\0').collect();
    let [parents, email, committed_at, subject, ids] = parts.as_slice() else {
        return Check::Refused;
    };
    let tagged: Vec<&str> = ids
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    if parents.trim().contains(' ') || tagged != [work_id] || email.trim().to_lowercase() != owner {
        return Check::Refused;
    }
    let committed_at = committed_at.trim();
    if parse_instant(committed_at).is_err() {
        return Check::Refused;
    }
    if let Err(stop) = bounded_git(repo, &["merge-base", "--is-ancestor", base, hash], deadline) {
        return stop;
    }
    Check::Owned(RecordCommit {
        hash: hash.into(),
        committed_at: Some(committed_at.into()),
        subject: commit_subject(subject),
        url: String::new(),
    })
}

impl Storage {
    /// Fill what a past automatic record is missing — its begin time, and the
    /// time and subject of owned commits it already names — from the begin
    /// receipt in the trusted record state folder and from Git. Only empty
    /// evidence is filled. The display copy, original, status, completion,
    /// receipts and Space link are never touched; anything without evidence
    /// stays unknown. True when something was filled.
    ///
    /// One pass checks at most `BACKFILL_COMMITS` commits and stops at
    /// `deadline`, each Git call bounded by what is left. `progress` carries
    /// what was checked to the next pass, so commits after unconfirmed ones
    /// are still reached; a finished record costs only a database read.
    pub fn backfill_automatic_evidence(
        &self,
        id: &str,
        state_dir: &Path,
        progress: &mut BackfillProgress,
        deadline: Instant,
    ) -> Result<bool, String> {
        validate_id(id)?;
        type Row = (Option<String>, Option<String>, Option<String>, String);
        let (work_id, started_at, completed_at, raw_commits, payload) = {
            let guard = self.connection()?;
            let c: &Connection = &guard;
            let row: Option<Row> = c
                .query_row(
                    "SELECT work_id,started_at,completed_at,commits FROM entry_evidence WHERE entry_id=?1",
                    [id],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
                )
                .optional()
                .map_err(database_error)?;
            let Some((Some(work_id), started_at, completed_at, raw_commits)) = row else {
                progress.finished = true;
                return Ok(false);
            };
            let payload: Option<String> = c
                .query_row(
                    "SELECT payload FROM project_record_receipts WHERE work_id=?1 AND entry_id=?2",
                    params![work_id, id],
                    |r| r.get(0),
                )
                .optional()
                .map_err(database_error)?;
            (work_id, started_at, completed_at, raw_commits, payload)
        };
        if progress.commits.as_deref() != Some(raw_commits.as_str()) {
            // A first look, or the list changed since (a newer delivery).
            *progress = BackfillProgress {
                commits: Some(raw_commits.clone()),
                ..Default::default()
            };
        } else if progress.finished {
            return Ok(false);
        }
        // The lock is released: files and Git are read without holding it.
        let payload: serde_json::Value = payload
            .and_then(|p| serde_json::from_str(&p).ok())
            .unwrap_or_default();
        let mut commits: Vec<RecordCommit> = serde_json::from_str(&raw_commits).unwrap_or_default();
        if commits.is_empty() {
            commits = owned_commits_in(payload["detail"].as_str().unwrap_or_default());
        }
        let wants_start = started_at.is_none();
        let pending: Vec<usize> = (0..commits.len())
            .filter(|&i| {
                commits[i].committed_at.is_none() && !progress.refused.contains(&commits[i].hash)
            })
            .collect();
        if !wants_start && pending.is_empty() {
            progress.finished = true;
            return Ok(false);
        }
        let Some(receipt) = begin_receipt(state_dir, &work_id) else {
            progress.finished = true;
            return Ok(false);
        };
        let mut start = None;
        if wants_start {
            // A receipt later than a confirmed completion is not this work's start.
            let begun = parse_instant(&receipt.at).ok();
            let done = completed_at.as_deref().and_then(|v| parse_instant(v).ok());
            if !matches!((begun, done), (Some(b), Some(d)) if b > d) {
                start = Some(receipt.at.clone());
            }
        }
        let mut filled = false;
        // Without the receipt's author and base, or the repository, nothing
        // more can be checked: the pass is finished.
        let mut unfinished = false;
        let repo = payload["repoPath"]
            .as_str()
            .map(Path::new)
            .filter(|p| p.is_absolute());
        if let (false, Some(owner), Some(base), Some(repo)) =
            (pending.is_empty(), &receipt.owner, &receipt.base, repo)
        {
            unfinished = pending.len() > BACKFILL_COMMITS;
            for &i in pending.iter().take(BACKFILL_COMMITS) {
                let commit = &mut commits[i];
                match verify_owned(repo, &commit.hash, &work_id, base, owner, deadline) {
                    Check::Owned(found) => {
                        commit.committed_at = found.committed_at;
                        if commit.subject.is_empty() {
                            commit.subject = found.subject;
                        }
                        filled = true;
                    }
                    // Remembered for this progress only; stays unknown in the record.
                    Check::Refused => {
                        progress.refused.insert(commit.hash.clone());
                    }
                    Check::Interrupted => {
                        unfinished = true;
                        break;
                    }
                }
            }
        }
        progress.finished = !unfinished;
        if start.is_none() && !filled {
            return Ok(false);
        }
        let written = commits_json(&commits)?;
        let guard = self.connection()?;
        let c: &Connection = &guard;
        // Written only if nothing changed meanwhile; a known start stays.
        let changed = c
            .execute(
                "UPDATE entry_evidence SET started_at=COALESCE(started_at,?3),commits=?4
                 WHERE entry_id=?1 AND work_id=?2 AND commits=?5",
                params![id, work_id, start, written, raw_commits],
            )
            .map_err(database_error)?;
        if changed > 0 {
            progress.commits = Some(written);
        } else {
            // Changed meanwhile: the next opening looks again from the start.
            *progress = BackfillProgress::default();
        }
        Ok(changed > 0)
    }

    pub fn entry_timeline(&self, id: &str) -> Result<EntryTimeline, String> {
        validate_id(id)?;
        let guard = self.connection()?;
        let connection: &Connection = &guard;
        let timeline = read_timeline(connection, id)?;
        Ok(timeline)
    }

    /// Timelines of several records read under one lock, keyed by record ID.
    pub fn entry_timelines(
        &self,
        ids: &[String],
    ) -> Result<HashMap<String, EntryTimeline>, String> {
        for id in ids {
            validate_id(id)?;
        }
        let guard = self.connection()?;
        let c: &Connection = &guard;
        let mut timelines = HashMap::with_capacity(ids.len());
        for id in ids {
            timelines.insert(id.clone(), read_timeline(c, id)?);
        }
        Ok(timelines)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kst_display_is_24_hour_minutes_and_unknown_is_never_now() {
        assert_eq!(kst_minute(Some("2026-11-19T05:07:59Z")), "2026-11-19 14:07");
        assert_eq!(
            kst_minute(Some("2026-11-19T23:30:00+09:00")),
            "2026-11-19 23:30"
        );
        // 16:30 UTC is already the next day in Korea.
        assert_eq!(kst_minute(Some("2026-12-31T16:30:00Z")), "2027-01-01 01:30");
        assert_eq!(
            kst_date("2026-12-31T16:30:00Z").as_deref(),
            Some("2027-01-01")
        );
        assert_eq!(kst_minute(None), UNKNOWN);
        assert_eq!(kst_minute(Some("2026-11-19 14:00")), UNKNOWN);
        assert_eq!(kst_minute(Some("yesterday")), UNKNOWN);
    }

    #[test]
    fn manual_time_is_the_typed_kst_time_on_the_record_date() {
        assert_eq!(
            manual_instant("2026-11-19", "09:05").unwrap(),
            "2026-11-19T09:05:00+09:00"
        );
        assert!(manual_instant("2026-02-30", "09:05").is_err());
        assert!(manual_instant("2026-11-19", "24:00").is_err());
        let timeline = EntryTimeline {
            manual_time: Some("23:59".into()),
            occurred_at: Some("2026-01-01T00:00:00Z".into()),
            ..Default::default()
        };
        assert_eq!(record_time("2026-11-19", &timeline), "2026-11-19 23:59");
        assert_eq!(
            record_time("2026-11-19", &EntryTimeline::default()),
            UNKNOWN
        );
        assert_eq!(TimeChange::from_field(" ").unwrap(), TimeChange::Clear);
        assert_eq!(
            TimeChange::from_field("07:30").unwrap(),
            TimeChange::Set("07:30".into())
        );
        for bad in ["7:30", "7시", "25:00", "07:60"] {
            assert!(TimeChange::from_field(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn commits_are_validated_and_owned_line_is_parsed_exactly() {
        let a = "a".repeat(40);
        let b = "b".repeat(64);
        let detail = format!("요약\n\n{OWNED_COMMITS_LINE}{a}, {b}, nothex, {a}\n");
        let commits = owned_commits_in(&detail);
        assert_eq!(
            commits.iter().map(|c| c.hash.as_str()).collect::<Vec<_>>(),
            vec![a.as_str(), b.as_str()]
        );
        assert!(commits.iter().all(|c| c.committed_at.is_none()));
        assert!(validate_commits(&commits).is_ok());
        let mut bad = commits.clone();
        bad.push(bad[0].clone());
        assert!(validate_commits(&bad).is_err(), "duplicate");
        let mut upper = commits.clone();
        upper[0].hash = "A".repeat(40);
        assert!(validate_commits(&upper).is_err());
        let mut url = commits.clone();
        url[0].url = format!("https://github.com/o/r/commit/{a}");
        assert!(validate_commits(&url).is_ok());
        for wrong in [
            format!("http://github.com/o/r/commit/{a}"),
            format!("https://user:pw@github.com/o/r/commit/{a}"),
            format!("https://github.com/o/r/commit/{b}"),
        ] {
            url[0].url = wrong.clone();
            assert!(validate_commits(&url).is_err(), "{wrong}");
        }
        let mut time = commits;
        time[0].committed_at = Some("2026-11-19 10:00".into());
        assert!(validate_commits(&time).is_err());
    }

    #[test]
    fn sourced_record_keeps_its_source_time_and_completion_needs_a_completed_status() {
        let git = EntryTimeline {
            sourced: true,
            manual_time: Some("07:00".into()),
            occurred_at: Some("2026-11-19T01:00:00Z".into()),
            ..Default::default()
        };
        assert_eq!(record_time("2026-11-19", &git), "2026-11-19 10:00");
        let unknown = EntryTimeline {
            sourced: true,
            manual_time: Some("07:00".into()),
            ..Default::default()
        };
        assert_eq!(
            record_time("2026-11-19", &unknown),
            UNKNOWN,
            "never the typed time"
        );
        assert!(!unknown.takes_manual_time());
        assert!(EntryTimeline::default().takes_manual_time());
        let mut auto = EntryTimeline {
            sourced: true,
            work_id: Some("w".into()),
            started_at: Some("2026-11-19T00:05:00Z".into()),
            completed_at: Some("2026-11-19T09:30:00+09:00".into()),
            status: "merged".into(),
            ..Default::default()
        };
        assert_eq!(
            reference_line("2026-11-19", &auto),
            "작업 시작 · 2026-11-19 09:05"
        );
        assert_eq!(
            completion_line(&auto).unwrap(),
            "완료 · 2026-11-19 09:30 · 병합 완료"
        );
        // A first completion stays stored, but a status that is not complete
        // shows as the current status.
        auto.status = "verification_pending".into();
        assert_eq!(auto.shown_completion(), None);
        assert_eq!(completion_line(&auto).unwrap(), "현재 상태 · 검증 대기");
        auto.status = "verified".into();
        auto.completed_at = None;
        assert_eq!(
            completion_line(&auto).unwrap(),
            "완료 · 미상 · 확인한 범위 검증 완료"
        );
        assert_eq!(completion_line(&git), None);
    }

    #[test]
    fn past_automatic_record_gains_begin_and_owned_commit_times_only_from_evidence() {
        use crate::project_records::ProjectRecord;
        let dir = std::env::temp_dir().join(format!("ts backfill {}", uuid::Uuid::new_v4()));
        let repo = dir.join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        let work = "6f1c2d3e-4a5b-4c6d-8e7f-0a1b2c3d4e5f";
        let run = |args: &[&str]| crate::diary::git(&repo, args).unwrap();
        run(&["init", "-q"]);
        let commit = |subject: &str, email: &str, id: &str| {
            let user = format!("user.email={email}");
            let trailer = format!("Diary-Work-Id: {id}");
            run(&[
                "-c",
                "user.name=Record Test",
                "-c",
                user.as_str(),
                "commit",
                "--allow-empty",
                "-q",
                "-m",
                subject,
                "-m",
                trailer.as_str(),
            ]);
            run(&["rev-parse", "HEAD"]).trim().to_string()
        };
        let base = commit("시작 전", "me@example.invalid", "none");
        let mine = commit("내 커밋", "me@example.invalid", work);
        let theirs = commit("남의 커밋", "other@example.invalid", work);
        let other_work = commit(
            "다른 작업",
            "me@example.invalid",
            "00000000-0000-4000-8000-0000000000aa",
        );

        let store = Storage::open(":memory:").unwrap();
        let record = ProjectRecord {
            schema_version: 2,
            work_id: work.into(),
            revision: 1,
            project_title: "TimeSchedule".into(),
            repo_path: repo.to_string_lossy().into(),
            date: "2026-11-19".into(),
            kind: "development".into(),
            title: "이전 형식 기록".into(),
            summary: "요약".into(),
            space_url: String::new(),
            status: "verified".into(),
            delivery_state: "local".into(),
            detail: format!("요약\n\n{OWNED_COMMITS_LINE}{mine}, {theirs}, {other_work}"),
            started_at: None,
            completed_at: None,
            commits: vec![],
        };
        // A v2 delivery arriving for the first time keeps the exact hashes.
        let id = store
            .apply_project_record(&record)
            .unwrap()
            .entry_id
            .unwrap();
        let first = store.entry_timeline(&id).unwrap();
        assert_eq!(first.commits.len(), 3);
        assert!(first.commits.iter().all(|c| c.committed_at.is_none()));
        let entry = store.get_entry(&id).unwrap();

        // Without a trusted begin receipt nothing is filled.
        let state = dir.join("state");
        std::fs::create_dir_all(&state).unwrap();
        assert!(!open_once(&store, &id, &state));
        assert_eq!(store.entry_timeline(&id).unwrap(), first);

        let receipt = state.join("diary-starts").join("x");
        std::fs::create_dir_all(&receipt).unwrap();
        std::fs::write(
            receipt.join("work.json"),
            format!(r#"{{"workId":"{work}"}}"#),
        )
        .unwrap();
        std::fs::write(
            receipt.join("start.json"),
            format!(r#"{{"branch":"codex/x","owner":"Me@Example.invalid","base":"{base}","at":"2026-11-19T00:05:00.000Z"}}"#),
        )
        .unwrap();
        assert!(open_once(&store, &id, &state));
        let filled = store.entry_timeline(&id).unwrap();
        assert_eq!(
            filled.started_at.as_deref(),
            Some("2026-11-19T00:05:00.000Z")
        );
        let by = |hash: &str| {
            filled
                .commits
                .iter()
                .find(|c| c.hash == hash)
                .unwrap()
                .clone()
        };
        let own = by(mine.as_str());
        assert_eq!(
            own.committed_at.as_deref(),
            Some(run(&["show", "-s", "--format=%cI", mine.as_str()]).trim())
        );
        assert_eq!(own.subject, "내 커밋");
        for foreign in [theirs.as_str(), other_work.as_str()] {
            assert_eq!(
                by(foreign).committed_at,
                None,
                "another author or work stays unknown"
            );
        }
        // Status, completion, the display copy and the receipt are untouched.
        assert_eq!(
            (filled.status.as_str(), filled.completed_at.as_deref()),
            ("verified", None)
        );
        assert!(!filled.edited);
        assert_eq!(store.get_entry(&id).unwrap(), entry);
        assert_eq!(
            store.apply_project_record(&record).unwrap().outcome,
            "unchanged"
        );
        // Nothing more can be confirmed: a second open does nothing.
        assert!(!open_once(&store, &id, &state));
        assert_eq!(store.entry_timeline(&id).unwrap(), filled);
        drop(store);
        let _ = std::fs::remove_dir_all(dir);
    }

    /// One opening with its own progress and a generous deadline.
    fn open_once(store: &Storage, id: &str, state: &Path) -> bool {
        store
            .backfill_automatic_evidence(
                id,
                state,
                &mut BackfillProgress::default(),
                Instant::now() + Duration::from_secs(120),
            )
            .unwrap()
    }

    #[test]
    fn owned_commit_after_fifty_unconfirmed_ones_is_reached_across_openings() {
        use crate::project_records::ProjectRecord;
        let dir = std::env::temp_dir().join(format!("ts backfill many {}", uuid::Uuid::new_v4()));
        let repo = dir.join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        let work = "7a2b3c4d-5e6f-4a7b-8c9d-0e1f2a3b4c5d";
        let run = |args: &[&str]| crate::diary::git(&repo, args).unwrap();
        run(&["init", "-q"]);
        let commit = |subject: &str| {
            let trailer = format!("Diary-Work-Id: {work}");
            run(&[
                "-c",
                "user.name=Record Test",
                "-c",
                "user.email=me@example.invalid",
                "commit",
                "--allow-empty",
                "-q",
                "-m",
                subject,
                "-m",
                trailer.as_str(),
            ]);
            run(&["rev-parse", "HEAD"]).trim().to_string()
        };
        let base = commit("시작 전");
        let mine = commit("51번째 커밋");
        // Fifty well-formed hashes Git cannot confirm come first.
        let missing: Vec<String> = (1..=50u32).map(|n| format!("{n:040x}")).collect();
        let listed = format!("{}, {mine}", missing.join(", "));
        let store = Storage::open(":memory:").unwrap();
        let record = ProjectRecord {
            schema_version: 2,
            work_id: work.into(),
            revision: 1,
            project_title: "TimeSchedule".into(),
            repo_path: repo.to_string_lossy().into(),
            date: "2026-11-19".into(),
            kind: "development".into(),
            title: "커밋이 많은 이전 기록".into(),
            summary: "요약".into(),
            space_url: String::new(),
            status: "verified".into(),
            delivery_state: "local".into(),
            detail: format!("요약\n\n{OWNED_COMMITS_LINE}{listed}"),
            started_at: None,
            completed_at: None,
            commits: vec![],
        };
        let id = store
            .apply_project_record(&record)
            .unwrap()
            .entry_id
            .unwrap();
        assert_eq!(store.entry_timeline(&id).unwrap().commits.len(), 51);
        let state = dir.join("state");
        let receipt = state.join("diary-starts").join("x");
        std::fs::create_dir_all(&receipt).unwrap();
        std::fs::write(
            receipt.join("work.json"),
            format!(r#"{{"workId":"{work}"}}"#),
        )
        .unwrap();
        std::fs::write(
            receipt.join("start.json"),
            format!(r#"{{"owner":"me@example.invalid","base":"{base}","at":"2026-11-19T00:05:00.000Z"}}"#),
        )
        .unwrap();
        let time_of = |hash: &str| {
            store
                .entry_timeline(&id)
                .unwrap()
                .commits
                .into_iter()
                .find(|c| c.hash == hash)
                .unwrap()
                .committed_at
        };
        let mut progress = BackfillProgress::default();

        // Out of time before any Git answer: the start needs no Git and is
        // kept, but no commit counts as checked and the pass is not finished.
        assert!(store
            .backfill_automatic_evidence(&id, &state, &mut progress, Instant::now())
            .unwrap());
        assert!(!progress.finished());
        assert!(progress.refused().is_empty());
        assert!(store.entry_timeline(&id).unwrap().started_at.is_some());
        let later = || Instant::now() + Duration::from_secs(120);

        // One pass checks the first fifty; none is confirmed or written as such.
        let began = Instant::now();
        assert!(!store
            .backfill_automatic_evidence(&id, &state, &mut progress, later())
            .unwrap());
        assert!(
            !progress.finished(),
            "one commit is left for the next opening"
        );
        assert_eq!(progress.refused().len(), 50);
        assert!(missing.iter().all(|h| time_of(h).is_none()));
        assert_eq!(time_of(&mine), None);
        assert!(began.elapsed() < Duration::from_secs(100));

        // The next opening continues after them and reaches the owned commit.
        assert!(store
            .backfill_automatic_evidence(&id, &state, &mut progress, later())
            .unwrap());
        assert!(progress.finished());
        assert_eq!(
            time_of(&mine).as_deref(),
            Some(run(&["show", "-s", "--format=%cI", mine.as_str()]).trim())
        );
        assert!(missing.iter().all(|h| time_of(h).is_none()));
        let done = store.entry_timeline(&id).unwrap();

        // Finished: a further opening reads only the database and changes nothing.
        assert!(!store
            .backfill_automatic_evidence(&id, &state, &mut progress, later())
            .unwrap());
        assert_eq!(store.entry_timeline(&id).unwrap(), done);
        drop(store);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn owned_commit_is_confirmed_after_its_repository_comes_back() {
        use crate::project_records::ProjectRecord;
        let dir = std::env::temp_dir().join(format!("ts backfill away {}", uuid::Uuid::new_v4()));
        let repo = dir.join("repo");
        let away = dir.join("repo-unplugged");
        std::fs::create_dir_all(&repo).unwrap();
        let work = "8b3c4d5e-6f7a-4b8c-9d0e-1f2a3b4c5d6e";
        let run = |args: &[&str]| crate::diary::git(&repo, args).unwrap();
        run(&["init", "-q"]);
        let commit = |subject: &str| {
            let trailer = format!("Diary-Work-Id: {work}");
            run(&[
                "-c",
                "user.name=Record Test",
                "-c",
                "user.email=me@example.invalid",
                "commit",
                "--allow-empty",
                "-q",
                "-m",
                subject,
                "-m",
                trailer.as_str(),
            ]);
            run(&["rev-parse", "HEAD"]).trim().to_string()
        };
        let base = commit("시작 전");
        let mine = commit("다시 연결된 저장소의 커밋");
        // A well-formed hash the repository does not have comes first.
        let missing = format!("{:040x}", 7u32);
        let store = Storage::open(":memory:").unwrap();
        let record = ProjectRecord {
            schema_version: 2,
            work_id: work.into(),
            revision: 1,
            project_title: "TimeSchedule".into(),
            repo_path: repo.to_string_lossy().into(),
            date: "2026-11-19".into(),
            kind: "development".into(),
            title: "외장 저장소 기록".into(),
            summary: "요약".into(),
            space_url: String::new(),
            status: "verified".into(),
            delivery_state: "local".into(),
            detail: format!("요약\n\n{OWNED_COMMITS_LINE}{missing}, {mine}"),
            started_at: None,
            completed_at: None,
            commits: vec![],
        };
        let id = store
            .apply_project_record(&record)
            .unwrap()
            .entry_id
            .unwrap();
        let state = dir.join("state");
        let receipt = state.join("diary-starts").join("x");
        std::fs::create_dir_all(&receipt).unwrap();
        std::fs::write(
            receipt.join("work.json"),
            format!(r#"{{"workId":"{work}"}}"#),
        )
        .unwrap();
        std::fs::write(
            receipt.join("start.json"),
            format!(r#"{{"owner":"me@example.invalid","base":"{base}","at":"2026-11-19T00:05:00.000Z"}}"#),
        )
        .unwrap();
        let time_of = |hash: &str| {
            store
                .entry_timeline(&id)
                .unwrap()
                .commits
                .into_iter()
                .find(|c| c.hash == hash)
                .unwrap()
                .committed_at
        };
        let later = || Instant::now() + Duration::from_secs(120);
        let mut progress = BackfillProgress::default();

        // The repository is unplugged: only the start (no Git needed) is kept;
        // no commit is taken as refused and the pass stays unfinished.
        std::fs::rename(&repo, &away).unwrap();
        assert!(store
            .backfill_automatic_evidence(&id, &state, &mut progress, later())
            .unwrap());
        assert!(!progress.finished());
        assert!(progress.refused().is_empty());
        assert!(store.entry_timeline(&id).unwrap().started_at.is_some());
        assert_eq!(time_of(&mine), None);

        // Plugged back in: the same progress confirms the real commit, and the
        // hash the reachable repository lacks is refused as before.
        std::fs::rename(&away, &repo).unwrap();
        assert!(store
            .backfill_automatic_evidence(&id, &state, &mut progress, later())
            .unwrap());
        assert!(progress.finished());
        assert_eq!(
            time_of(&mine).as_deref(),
            Some(run(&["show", "-s", "--format=%cI", mine.as_str()]).trim())
        );
        assert_eq!(time_of(&missing), None);
        assert_eq!(
            progress.refused().iter().collect::<Vec<_>>(),
            vec![&missing]
        );
        drop(store);
        let _ = std::fs::remove_dir_all(dir);
    }
}
