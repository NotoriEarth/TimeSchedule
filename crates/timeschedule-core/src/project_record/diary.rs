//! Shows each project's kept project record in the diary, as one entry per project and
//! folder, the way `daily_record::diary` shows the daily records.
//!
//! The record file (`project/<folder>/<project ID>/record.json`) is the source and the entry is its
//! display. A receipt per project and folder remembers the entry, the date and the text
//! the program last put there, and the revision it last saw. A newer record replaces the
//! text, and moves the entry to the day that record was made, only while the entry is
//! still in that project on that day with exactly that text. An entry the owner edited,
//! moved or deleted is not touched and not made again: the receipt has no reference to
//! the entry, so it outlives a deleted one.
use super::ProjectRecord;
use crate::{
    chat_originals::{group_name, is_link},
    daily_record::{
        diary::{fitted, DiaryReport, BODY_LIMIT},
        one_line,
        record::read_plain,
    },
    project_records::repo_key,
    storage::{database_error, timestamp, validate_text, Storage},
    timeline::kst_date,
};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use std::{fmt::Write as _, fs, io, path::Path};

/// Schema 18. Tests that build an older database by dropping later tables leave this
/// one in place, so it is made only when missing.
pub(crate) fn migrate(c: &Connection) -> Result<(), String> {
    c.execute_batch(
        "CREATE TABLE IF NOT EXISTS project_record_entries(
            project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
            repo_key TEXT NOT NULL, entry_id TEXT NOT NULL,
            revision INTEGER NOT NULL, fingerprint TEXT NOT NULL,
            shown_date TEXT NOT NULL, shown_body TEXT NOT NULL,
            PRIMARY KEY(project_id, repo_key));
        PRAGMA user_version=18;",
    )
    .map_err(database_error)
}

/// The diary's limit for a title, in characters.
const TITLE_LIMIT: usize = 200;

/// "프로젝트 기록 · <name>", the name cut so the title fits the diary's limit and the
/// entry can be saved again from the editor.
pub fn entry_title(project: &str) -> String {
    let prefix = "프로젝트 기록 · ";
    let room = TITLE_LIMIT - prefix.chars().count();
    let name: String = project.chars().take(room).collect();
    format!("{prefix}{name}")
}

/// Text the model wrote, on one line, with its square brackets turned round: `[제안 … ·
/// …]` is the mark the program puts on a daily record's decision, and no text of the
/// model may look like it.
fn said(text: &str) -> String {
    one_line(text).replace('[', "(").replace(']', ")")
}

/// The diary text of a record: what a person reads. The keys it cites, its checks, its
/// revision and model stay in the record file.
pub fn entry_body(record: &ProjectRecord) -> String {
    let mut out = match &record.completed_on {
        Some(day) => format!("완료 · {day}\n"),
        None => "완료 전 종합\n".to_owned(),
    };
    let section = |out: &mut String, title: &str| {
        writeln!(out, "\n## {title}").unwrap();
    };
    if let Some(overview) = &record.overview {
        section(&mut out, "개요");
        writeln!(out, "{}", said(&overview.text)).unwrap();
    }
    if !record.features.is_empty() {
        section(&mut out, "기능");
        for f in &record.features {
            writeln!(out, "- {}: {}", said(&f.title), said(&f.detail)).unwrap();
        }
    }
    if !record.decisions.is_empty() {
        section(&mut out, "결정");
        for d in &record.decisions {
            writeln!(out, "- {}", said(&d.decision)).unwrap();
            let reason = d.reason.as_deref().map(said);
            writeln!(
                out,
                "  이유: {}",
                reason.as_deref().unwrap_or("이유 기록 없음")
            )
            .unwrap();
            if !d.rejected.is_empty() {
                let rejected: Vec<String> = d.rejected.iter().map(|r| said(r)).collect();
                writeln!(out, "  버린 대안: {}", rejected.join("; ")).unwrap();
            }
            if d.daily.is_empty() {
                writeln!(out, "  확인: 제안 알 수 없음 · 확인 기록 없음").unwrap();
                continue;
            }
            // Only these lines carry who proposed and who decided: they are the daily
            // records' own decisions, copied.
            writeln!(out, "  하루 기록의 결정:").unwrap();
            for daily in &d.daily {
                write!(
                    out,
                    "  - {} [제안 {} · {}] {}",
                    daily.date,
                    daily.proposed_by.label(),
                    daily.decided_by.label(),
                    said(&daily.decision)
                )
                .unwrap();
                if !daily.rejected.is_empty() {
                    let rejected: Vec<String> = daily.rejected.iter().map(|r| said(r)).collect();
                    write!(out, " (버린 대안: {})", rejected.join("; ")).unwrap();
                }
                out.push('\n');
            }
        }
    }
    if !record.troubleshooting.is_empty() {
        section(&mut out, "트러블슈팅");
        for t in &record.troubleshooting {
            writeln!(out, "- {}", said(&t.problem)).unwrap();
            for (label, value) in [
                ("원인", &t.cause),
                ("해결", &t.fix),
                ("확인", &t.verification),
            ] {
                if let Some(value) = value {
                    writeln!(out, "  {label}: {}", said(value)).unwrap();
                }
            }
        }
    }
    for (title, items) in [("결과", &record.results), ("한계", &record.limits)] {
        if !items.is_empty() {
            section(&mut out, title);
            for item in items {
                writeln!(out, "- {}", said(&item.text)).unwrap();
            }
        }
    }
    out
}

enum Shown {
    Added,
    Updated,
    Kept,
    Unchanged,
    Older,
    OtherFolder,
}

/// A project as the projects table has it now: its ID and its folder's key.
struct Holder {
    id: String,
    key: String,
}

impl Storage {
    /// Shows the project record kept in `vault` of every project with a local folder.
    /// Each record is read and shown on its own: one that fails is counted and the rest
    /// go on. Each project's record is in its own folder, so two projects given one
    /// folder each show their own.
    pub fn import_project_records(&self, vault: &Path) -> Result<DiaryReport, String> {
        self.import_records_of(vault, None)
    }

    /// Shows the project record of the project `id` only, so what is reported is that
    /// project's.
    pub fn import_project_record(&self, vault: &Path, id: &str) -> Result<DiaryReport, String> {
        self.import_records_of(vault, Some(id))
    }

    fn import_records_of(&self, vault: &Path, only: Option<&str>) -> Result<DiaryReport, String> {
        let mut report = DiaryReport::default();
        let root = vault.join("project");
        if is_link(&root).map_err(|e| e.to_string())? {
            return Err("링크인 project 폴더의 프로젝트 기록은 읽지 않습니다.".into());
        }
        let holders = self.holders()?;
        for holder in holders.iter().filter(|h| only.is_none_or(|id| h.id == id)) {
            let folder = group_name(&holder.key);
            let bytes = match read_record(vault, &folder, &holder.id) {
                Ok(Some(bytes)) => bytes,
                Ok(None) => continue,
                Err(_) => {
                    report.failed += 1;
                    continue;
                }
            };
            let Ok(record) = serde_json::from_slice::<ProjectRecord>(&bytes) else {
                report.failed += 1;
                continue;
            };
            // A file copied in from another project's folder is not this project's.
            if record.project.id != holder.id {
                report.failed += 1;
                continue;
            }
            match self.show_project_record(holder, &folder, &record, (vault, &bytes)) {
                Ok(Shown::Added) => report.added += 1,
                Ok(Shown::Updated) => report.updated += 1,
                Ok(Shown::Kept) => report.kept += 1,
                Ok(Shown::Unchanged) => report.unchanged += 1,
                Ok(Shown::Older) => report.older += 1,
                Ok(Shown::OtherFolder) => report.other_folder += 1,
                Err(_) => report.failed += 1,
            }
        }
        Ok(report)
    }

    /// The projects with a local folder.
    fn holders(&self) -> Result<Vec<Holder>, String> {
        let c = self.connection()?;
        let mut q = c
            .prepare("SELECT id, repo_path FROM projects ORDER BY id")
            .map_err(database_error)?;
        let rows = q
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
            .map_err(database_error)?;
        let mut holders = Vec::new();
        for row in rows {
            let (id, path) = row.map_err(database_error)?;
            if Path::new(&path).is_absolute() {
                holders.push(Holder {
                    id,
                    key: repo_key(&path),
                });
            }
        }
        Ok(holders)
    }

    /// Shows `record`, read from the store at `read.0` as the bytes `read.1`, in the diary.
    fn show_project_record(
        &self,
        holder: &Holder,
        folder: &str,
        record: &ProjectRecord,
        read: (&Path, &[u8]),
    ) -> Result<Shown, String> {
        // A file copied in from the records of another folder is not of this lineage.
        if record.project.repo_key != holder.key {
            return Ok(Shown::OtherFolder);
        }
        let date =
            kst_date(&record.generated_at).ok_or("프로젝트 기록의 만든 시각을 읽지 못했습니다.")?;
        let body = fitted(
            entry_body(record),
            "프로젝트 기록",
            &format!("project/{folder}/{}/record.json", holder.id),
        );
        validate_text(&body, "기록 본문", BODY_LIMIT)?;
        let revision = i64::from(record.revision);
        let fingerprint = &record.input.fingerprint;
        let mut c = self.connection()?;
        let tx = c
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(database_error)?;
        // The owner may have moved the project to another folder or deleted it since
        // the projects were listed: the write goes by what holds now.
        let path: Option<String> = tx
            .query_row(
                "SELECT repo_path FROM projects WHERE id=?1",
                [&holder.id],
                |r| r.get(0),
            )
            .optional()
            .map_err(database_error)?;
        if !path.is_some_and(|path| Path::new(&path).is_absolute() && repo_key(&path) == holder.key)
        {
            return Ok(Shown::OtherFolder);
        }
        // The file may have been replaced since it was read (a run kept a newer record
        // and showed it meanwhile): the older read writes nothing, and the next showing
        // reads the file that is there.
        match read_record(read.0, folder, &holder.id) {
            Ok(Some(now)) if now == read.1 => {}
            Ok(_) => return Ok(Shown::Unchanged),
            Err(e) => return Err(e),
        }
        let receipt: Option<(String, i64, String, String, String)> = tx
            .query_row(
                "SELECT entry_id,revision,fingerprint,shown_date,shown_body FROM project_record_entries
                 WHERE project_id=?1 AND repo_key=?2",
                params![holder.id, holder.key],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )
            .optional()
            .map_err(database_error)?;
        let shown = match receipt {
            None => {
                let entry = uuid::Uuid::new_v4().to_string();
                let source_ref = format!("project-record:{}:{folder}", holder.id);
                tx.execute(
                    "INSERT INTO diary_entries(id,project_id,date,kind,title,body,source_ref,source_body,updated_at)
                     VALUES(?1,?2,?3,'portfolio',?4,?5,?6,?5,?7)",
                    params![
                        entry,
                        holder.id,
                        date,
                        entry_title(&record.project.title),
                        body,
                        source_ref,
                        timestamp()
                    ],
                )
                .map_err(database_error)?;
                tx.execute(
                    "INSERT INTO project_record_entries(project_id,repo_key,entry_id,revision,fingerprint,shown_date,shown_body)
                     VALUES(?1,?2,?3,?4,?5,?6,?7)",
                    params![holder.id, holder.key, entry, revision, fingerprint, date, body],
                )
                .map_err(database_error)?;
                Shown::Added
            }
            Some((entry, seen, kept, shown_date, last_shown))
                if revision == seen && &kept == fingerprint =>
            {
                // The same input. A newer program may show it differently, or the file
                // was made again from the same input on another day: the entry takes
                // the new text and day under the same rule as a newer record. An entry
                // the owner changed keeps its own.
                if (last_shown == body && shown_date == date)
                    || !replace(
                        &tx,
                        &entry,
                        &holder.id,
                        (&shown_date, &last_shown),
                        (&date, &body),
                    )?
                {
                    return Ok(Shown::Unchanged);
                }
                tx.execute(
                    "UPDATE project_record_entries SET shown_date=?3, shown_body=?4 WHERE project_id=?1 AND repo_key=?2",
                    params![holder.id, holder.key, date, body],
                )
                .map_err(database_error)?;
                Shown::Updated
            }
            Some((_, seen, ..)) if revision <= seen => return Ok(Shown::Older),
            Some((entry, _, _, shown_date, last_shown)) => {
                let replaced = replace(
                    &tx,
                    &entry,
                    &holder.id,
                    (&shown_date, &last_shown),
                    (&date, &body),
                )?;
                // The record was seen either way, so the next scan has nothing new.
                let (now_date, now_body) = if replaced {
                    (&date, &body)
                } else {
                    (&shown_date, &last_shown)
                };
                tx.execute(
                    "UPDATE project_record_entries SET revision=?3, fingerprint=?4, shown_date=?5, shown_body=?6
                     WHERE project_id=?1 AND repo_key=?2",
                    params![holder.id, holder.key, revision, fingerprint, now_date, now_body],
                )
                .map_err(database_error)?;
                if replaced {
                    Shown::Updated
                } else {
                    Shown::Kept
                }
            }
        };
        tx.commit().map_err(database_error)?;
        Ok(shown)
    }
}

/// The latest record file of the project `id` in the store `vault`: `None` when there
/// is none. It is found the way it is made (`record_dir`: the ID checked, no folder a
/// link), and the file is opened without following a link.
fn read_record(vault: &Path, folder: &str, id: &str) -> Result<Option<Vec<u8>>, String> {
    let dir = super::record_dir(vault, folder, id)?;
    // The folder's group (`project/<folder>`) first, then the project's own folder.
    for at in [dir.parent().ok_or("프로젝트 기록 폴더가 없습니다.")?, &dir] {
        match fs::symlink_metadata(at) {
            // No record for this project yet.
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e.to_string()),
            Ok(meta) if !meta.is_dir() => {
                return Err(format!("{}이 폴더가 아닙니다.", at.display()))
            }
            Ok(_) => {}
        }
    }
    let path = dir.join("record.json");
    match read_plain(&path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

/// Puts the text and date `now` in the entry only while it is where the program put it,
/// in the project on the day `shown` with the text `shown`. An entry the owner edited or
/// moved matches no row, and neither does a deleted one.
fn replace(
    tx: &Connection,
    entry: &str,
    project: &str,
    shown: (&str, &str),
    now: (&str, &str),
) -> Result<bool, String> {
    let rows = tx
        .execute(
            "UPDATE diary_entries SET date=?5, body=?6, source_body=?6, updated_at=?7
             WHERE id=?1 AND project_id=?2 AND date=?3 AND body=?4",
            params![entry, project, shown.0, shown.1, now.0, now.1, timestamp()],
        )
        .map_err(database_error)?;
    Ok(rows > 0)
}

#[cfg(test)]
mod tests;
