//! Shows each kept daily record in the diary, as one entry per project, folder and day.
//!
//! The record file in the document store is the source. The diary entry is its display.
//! A receipt per project, folder and day remembers what the program last showed: the
//! entry, the text it put there, and the revision it last saw. A newer record replaces
//! the text only while the entry is still in that project and day with exactly that
//! text. An entry the owner edited, moved or deleted is not touched and not made again.
//! The receipt has no reference to the entry, so it outlives a deleted one, like
//! `codex_chat_sessions` does for chat excerpts.
//!
//! Revisions count within one folder (`repoKey`), so the folder is part of the receipt:
//! after the project moves to another folder, the records made there start their own
//! entries and never replace what was shown from the old folder.
use super::record::DailyRecord;
use crate::project_records::repo_key;
use crate::{
    chat_originals::{group_name, is_link, OriginalsProject},
    storage::{database_error, timestamp, validate_date, validate_text, Storage},
    vault_folder::{is_linked, Folder},
};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::Serialize;
use std::path::Path;

/// Schema 16. Tests that build an older database by dropping later tables leave this
/// one in place, so it is made only when missing.
pub(crate) fn migrate(c: &Connection) -> Result<(), String> {
    c.execute_batch(
        "CREATE TABLE IF NOT EXISTS daily_record_entries(
            project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
            repo_key TEXT NOT NULL, date TEXT NOT NULL, entry_id TEXT NOT NULL,
            revision INTEGER NOT NULL, fingerprint TEXT NOT NULL, shown_body TEXT NOT NULL,
            PRIMARY KEY(project_id, repo_key, date));
        PRAGMA user_version=16;",
    )
    .map_err(database_error)
}

#[derive(Debug, Default, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DiaryReport {
    /// Days that got a diary entry.
    pub added: usize,
    /// Entries whose text was replaced by a newer record.
    pub updated: usize,
    /// Newer records whose entry the owner had edited, moved or deleted, so nothing of
    /// it was touched.
    pub kept: usize,
    pub unchanged: usize,
    /// Records whose revision is not above the one already seen for the day, such as a
    /// restored backup. Not shown.
    pub older: usize,
    /// Records in the project's folder that were made for another folder. Not shown.
    pub other_folder: usize,
    /// Record files and record folders that could not be read or shown.
    pub failed: usize,
}

/// The diary text of a record. Its revision, model and time stay in the record file.
pub fn entry_body(record: &DailyRecord) -> String {
    let text = record.render();
    if text.is_empty() {
        "이날 하루 기록에 남길 내용이 없었습니다.\n".into()
    } else {
        text
    }
}

/// The diary's limit for an entry's text.
pub(crate) const BODY_LIMIT: usize = 100_000;

/// The text cut to the diary's limit, with a line saying where the whole record is:
/// `what` names the record and `file` is its place in the document store. A record is
/// never too long to show: the record file keeps all of it.
pub(crate) fn fitted(body: String, what: &str, file: &str) -> String {
    if body.chars().count() <= BODY_LIMIT {
        return body;
    }
    let note = format!(
        "\n… {what}이 길어 다이어리에는 앞부분만 보입니다. 전체는 문서 저장소의 {file}에 있습니다.\n"
    );
    let keep = BODY_LIMIT - note.chars().count();
    let cut: String = body.chars().take(keep).collect();
    cut + &note
}

pub fn entry_title(date: &str) -> String {
    format!("하루 정리 · {date}")
}

/// The day of a latest record file, `<YYYY-MM-DD>.json`. Kept revisions
/// (`<date>.r<N>.json`), temporary and lock files are not latest records.
fn day_of(name: &str) -> Option<&str> {
    let date = name.strip_suffix(".json")?;
    validate_date(date).ok().map(|()| date)
}

enum Shown {
    Added,
    Updated,
    Kept,
    Unchanged,
    Older,
    OtherFolder,
}

impl Storage {
    /// Shows the daily records kept in `vault` for every project whose chats are
    /// captured. Each record file is read and shown on its own: one that fails is
    /// counted and the rest go on.
    pub fn import_daily_records(&self, vault: &Path) -> Result<DiaryReport, String> {
        let mut report = DiaryReport::default();
        let daily = vault.join("daily");
        if is_link(&daily).map_err(|e| e.to_string())? {
            return Err("링크인 daily 폴더의 하루 기록은 읽지 않습니다.".into());
        }
        // The store is opened once and every project's records are listed and read
        // through folders held open from it, so no record is read from where a link put
        // on the way meanwhile points.
        let Some(store) = Folder::store(vault).map_err(|e| e.to_string())? else {
            return Ok(report);
        };
        for project in self.originals_projects()?.into_iter().filter(|p| p.capture) {
            let folder = group_name(&project.root);
            let names = match store.below(&["daily", &folder]) {
                Ok(Some(dir)) => dir.names().map(|names| (dir, names)),
                // No records for this project yet.
                Ok(None) => continue,
                Err(e) => Err(e),
            };
            let Ok((dir, names)) = names else {
                report.failed += 1;
                continue;
            };
            for name in names {
                let Some(date) = day_of(&name) else {
                    continue;
                };
                match self.show_record(&project, date, &dir, &name) {
                    Ok(Shown::Added) => report.added += 1,
                    Ok(Shown::Updated) => report.updated += 1,
                    Ok(Shown::Kept) => report.kept += 1,
                    Ok(Shown::Unchanged) => report.unchanged += 1,
                    Ok(Shown::Older) => report.older += 1,
                    Ok(Shown::OtherFolder) => report.other_folder += 1,
                    Err(_) => report.failed += 1,
                }
            }
        }
        Ok(report)
    }

    fn show_record(
        &self,
        project: &OriginalsProject,
        date: &str,
        dir: &Folder,
        name: &str,
    ) -> Result<Shown, String> {
        let bytes = dir.read(name).map_err(|e| {
            if is_linked(&e) {
                "링크인 하루 기록은 읽지 않습니다.".to_string()
            } else {
                e.to_string()
            }
        })?;
        let record: DailyRecord = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
        // The file must be the record of this project and this day.
        if record.project.id != project.id || record.date != date {
            return Err("하루 기록의 프로젝트나 날짜가 파일 위치와 다릅니다.".into());
        }
        // A file copied in from the records of another folder is not of this lineage.
        if record.project.repo_key != project.root {
            return Ok(Shown::OtherFolder);
        }
        let body = fitted(
            entry_body(&record),
            "하루 기록",
            &format!("daily/{}/{date}.json", group_name(&project.root)),
        );
        validate_text(&body, "기록 본문", BODY_LIMIT)?;
        let revision = i64::from(record.revision);
        let fingerprint = &record.input.fingerprint;
        let mut c = self.connection()?;
        let tx = c
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(database_error)?;
        // The owner may have turned capture off or moved the project to another folder
        // since the projects were listed: the write goes by what holds now.
        let current: Option<(String, String)> = tx
            .query_row(
                "SELECT p.repo_path, w.repo_key FROM projects p
                 JOIN codex_chat_projects w ON w.project_id=p.id WHERE p.id=?1",
                [&project.id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(database_error)?;
        if !current.is_some_and(|(path, key)| key == project.root && repo_key(&path) == key) {
            return Ok(Shown::OtherFolder);
        }
        let receipt: Option<(String, i64, String, String)> = tx
            .query_row(
                "SELECT entry_id,revision,fingerprint,shown_body FROM daily_record_entries
                 WHERE project_id=?1 AND repo_key=?2 AND date=?3",
                params![project.id, project.root, date],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .optional()
            .map_err(database_error)?;
        let shown = match receipt {
            None => {
                let entry = uuid::Uuid::new_v4().to_string();
                let source_ref = format!(
                    "daily-record:{}:{}:{date}",
                    project.id,
                    group_name(&project.root)
                );
                tx.execute(
                    "INSERT INTO diary_entries(id,project_id,date,kind,title,body,source_ref,source_body,updated_at)
                     VALUES(?1,?2,?3,'development',?4,?5,?6,?5,?7)",
                    params![entry, project.id, date, entry_title(date), body, source_ref, timestamp()],
                )
                .map_err(database_error)?;
                tx.execute(
                    "INSERT INTO daily_record_entries(project_id,repo_key,date,entry_id,revision,fingerprint,shown_body)
                     VALUES(?1,?2,?3,?4,?5,?6,?7)",
                    params![project.id, project.root, date, entry, revision, fingerprint, body],
                )
                .map_err(database_error)?;
                Shown::Added
            }
            Some((entry, seen, kept, last_shown)) if revision == seen && &kept == fingerprint => {
                // The same record. A newer program may show it differently: the entry
                // takes the new text under the same rule as a newer record. An entry
                // the owner changed keeps its text, and nothing is new for the scan.
                if last_shown == body || !replace(&tx, &entry, project, date, &last_shown, &body)? {
                    return Ok(Shown::Unchanged);
                }
                tx.execute(
                    "UPDATE daily_record_entries SET shown_body=?4
                     WHERE project_id=?1 AND repo_key=?2 AND date=?3",
                    params![project.id, project.root, date, body],
                )
                .map_err(database_error)?;
                Shown::Updated
            }
            Some((_, seen, ..)) if revision <= seen => return Ok(Shown::Older),
            Some((entry, _, _, last_shown)) => {
                let replaced = replace(&tx, &entry, project, date, &last_shown, &body)?;
                // The record was seen either way, so the next scan has nothing new.
                tx.execute(
                    "UPDATE daily_record_entries SET revision=?4, fingerprint=?5, shown_body=?6
                     WHERE project_id=?1 AND repo_key=?2 AND date=?3",
                    params![
                        project.id,
                        project.root,
                        date,
                        revision,
                        fingerprint,
                        if replaced { &body } else { &last_shown }
                    ],
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

/// Puts `body` in the entry only while it is where the program put it, with the text
/// the program last put there. An entry the owner edited or moved matches no row, and
/// neither does a deleted one.
fn replace(
    tx: &Connection,
    entry: &str,
    project: &OriginalsProject,
    date: &str,
    last_shown: &str,
    body: &str,
) -> Result<bool, String> {
    let rows = tx
        .execute(
            "UPDATE diary_entries SET body=?5, source_body=?5, updated_at=?6
             WHERE id=?1 AND project_id=?2 AND date=?3 AND body=?4",
            params![entry, project.id, date, last_shown, body, timestamp()],
        )
        .map_err(database_error)?;
    Ok(rows > 0)
}

#[cfg(test)]
mod tests;
