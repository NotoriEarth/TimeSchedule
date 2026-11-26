//! The diary records of projects, read from the database: lists, searches, one
//! record, and the evidence of a daily record from its file in the document store.
use super::*;
use crate::{
    chat_originals::{group_name, is_link},
    daily_record::record::{DailyRecord, Evidence, Quoted},
    diary::list_summary,
    project_records::repo_key,
    storage::{database_error, validate_id, Storage},
};
use rusqlite::{types::ToSql, OptionalExtension};
use serde_json::json;
use std::collections::BTreeMap;

fn date_text(date: Option<NaiveDate>) -> Option<String> {
    date.map(|d| d.to_string())
}

impl Storage {
    /// The project `project` names: its ID, or the absolute path of its folder.
    pub fn project_of(&self, project: &str) -> Result<crate::diary::Project, String> {
        let projects = self.list_projects()?;
        let mut found: Vec<crate::diary::Project> = if Path::new(project).is_absolute() {
            let key = repo_key(project);
            projects
                .into_iter()
                .filter(|p| !p.repo_path.is_empty() && repo_key(&p.repo_path) == key)
                .collect()
        } else {
            validate_id(project)?;
            projects.into_iter().filter(|p| p.id == project).collect()
        };
        // Two projects can be given one folder. Which one is meant is not guessed.
        if found.len() > 1 {
            let ids: Vec<&str> = found.iter().map(|p| p.id.as_str()).collect();
            return Err(format!(
                "이 폴더의 프로젝트가 {}개입니다. --project에 프로젝트 ID를 지정하세요: {}",
                found.len(),
                ids.join(", ")
            ));
        }
        found
            .pop()
            .ok_or_else(|| format!("프로젝트를 찾지 못했습니다: {project}"))
    }

    /// Every project, with how many records of each origin it has.
    pub fn read_projects(&self) -> Result<Vec<ProjectRow>, String> {
        let c = self.connection()?;
        // The projects and their counts are read in one transaction, so they agree.
        let c = c.unchecked_transaction().map_err(database_error)?;
        let projects: Vec<(String, String, String)> = c
            .prepare("SELECT id,title,repo_path FROM projects ORDER BY updated_at DESC,id")
            .map_err(database_error)?
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .map_err(database_error)?
            .collect::<Result<_, _>>()
            .map_err(database_error)?;
        // The app collects a project's chats while it is registered for the folder it
        // has now (`Storage::chat_capture_enabled`).
        let capture: Vec<(String, String)> = c
            .prepare("SELECT project_id,repo_key FROM codex_chat_projects")
            .map_err(database_error)?
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .map_err(database_error)?
            .collect::<Result<_, _>>()
            .map_err(database_error)?;
        let mut q = c
            .prepare(&format!(
                "SELECT e.project_id,{ORIGIN},count(*),min(e.date),max(e.date) FROM diary_entries e
                 WHERE e.project_id IS NOT NULL GROUP BY 1,2"
            ))
            .map_err(database_error)?;
        let counts: Vec<(String, String, usize, String, String)> = q
            .query_map([], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
            })
            .map_err(database_error)?
            .collect::<Result<_, _>>()
            .map_err(database_error)?;
        Ok(projects
            .into_iter()
            .map(|(id, title, repo_path)| crate::diary::Project {
                id,
                title,
                description: String::new(),
                repo_path,
                updated_at: String::new(),
            })
            .map(|p| {
                let mine = counts.iter().filter(|c| c.0 == p.id);
                let records = mine.clone().map(|c| (Origin::of(&c.1).key(), c.2)).fold(
                    BTreeMap::new(),
                    |mut all, (key, n)| {
                        *all.entry(key).or_insert(0) += n;
                        all
                    },
                );
                ProjectRow {
                    capture: !p.repo_path.is_empty()
                        && capture
                            .iter()
                            .any(|(id, key)| *id == p.id && repo_key(&p.repo_path) == *key),
                    first_date: mine.clone().map(|c| c.3.clone()).min(),
                    last_date: mine.map(|c| c.4.clone()).max(),
                    records,
                    id: p.id,
                    title: p.title,
                    folder: p.repo_path,
                    originals: None,
                    daily_days: None,
                }
            })
            .collect())
    }

    /// A page of the records `filter` names, newest first. With `query`, only those
    /// whose title or body holds those words, with excerpts of where; the words are
    /// found as the app's search finds them.
    pub fn read_records(
        &self,
        filter: &Filter,
        query: Option<&str>,
        offset: usize,
        budget: usize,
    ) -> Result<Listing, String> {
        if let Some(project) = filter.project {
            validate_id(project)?;
        }
        if let (Some(from), Some(to)) = (filter.from, filter.to) {
            if from > to {
                return Err("시작 날짜가 종료 날짜보다 늦습니다.".into());
            }
        }
        let query = query.map(query_words).transpose()?;
        let origins = format!(
            ",{},",
            filter
                .origins
                .iter()
                .map(|o| o.key())
                .collect::<Vec<_>>()
                .join(",")
        );
        let (from, to) = (date_text(filter.from), date_text(filter.to));
        // As in `Storage::search_diary`: three letters or more use the trigram index,
        // fewer compare text, folding case only where SQLite can.
        let matching = match query {
            None => "",
            Some(q) if q.chars().count() >= 3 => {
                "AND id IN (SELECT id FROM diary_search WHERE diary_search MATCH :q AND kind='entry')"
            }
            Some(q) if q.bytes().any(|b| b.is_ascii_alphabetic()) => {
                "AND (instr(lower(title),lower(:q))>0 OR instr(lower(body),lower(:q))>0)"
            }
            Some(_) => "AND (instr(title,:q)>0 OR instr(body,:q)>0)",
        };
        let phrase = query.map(|q| {
            if q.chars().count() >= 3 {
                format!("\"{}\"", q.replace('"', "\"\""))
            } else {
                q.to_owned()
            }
        });
        let base = format!(
            "WITH base AS (SELECT e.id AS id,e.project_id AS project,e.date AS date,e.kind AS kind,
                e.title AS title,e.body AS body,e.updated_at AS updated_at,{ORIGIN} AS origin
                FROM diary_entries e WHERE e.project_id IS NOT NULL
                AND (:project IS NULL OR e.project_id=:project)
                AND (:from IS NULL OR e.date>=:from) AND (:to IS NULL OR e.date<=:to))
             SELECT {{}} FROM base WHERE instr(:origins, ','||origin||',')>0 {matching}"
        );
        let mut params: Vec<(&str, &dyn ToSql)> = vec![
            (":project", &filter.project),
            (":from", &from),
            (":to", &to),
            (":origins", &origins),
        ];
        if let Some(phrase) = &phrase {
            params.push((":q", phrase));
        }
        let c = self.connection()?;
        // The count and the page are read in one transaction, so they agree.
        let c = c.unchecked_transaction().map_err(database_error)?;
        let total: usize = c
            .query_row(&base.replace("{}", "count(*)"), params.as_slice(), |r| {
                r.get(0)
            })
            .map_err(database_error)?;
        let columns = if query.is_some() {
            "id,project,date,kind,origin,title,length(body),updated_at,body,0"
        } else {
            "id,project,date,kind,origin,title,length(body),updated_at,substr(body,1,960),length(body)>960"
        };
        let page = base.replace("{}", columns)
            + " ORDER BY date DESC,updated_at DESC,id LIMIT :limit OFFSET :offset";
        let offset_sql = i64::try_from(offset).map_err(|_| "--offset이 너무 큽니다.")?;
        params.push((":limit", &PAGE_ROWS));
        params.push((":offset", &offset_sql));
        let mut q = c.prepare(&page).map_err(database_error)?;
        let rows: Vec<(RecordRow, String, bool)> = q
            .query_map(params.as_slice(), |r| {
                Ok((
                    RecordRow {
                        id: r.get(0)?,
                        project: r.get(1)?,
                        date: r.get(2)?,
                        kind: r.get(3)?,
                        origin: Origin::of(&r.get::<_, String>(4)?),
                        title: r.get(5)?,
                        chars: r.get(6)?,
                        updated_at: r.get(7)?,
                        summary: None,
                        matches: None,
                        excerpts: None,
                    },
                    r.get(8)?,
                    r.get(9)?,
                ))
            })
            .map_err(database_error)?
            .collect::<Result<_, _>>()
            .map_err(database_error)?;
        let records = rows.into_iter().map(|(mut row, text, more)| {
            match query {
                Some(q) => {
                    // Found as the query found it: the trigram index folds the case of
                    // every letter, the comparison only that of ASCII letters.
                    let (matches, shown) = excerpts(&text, q, q.chars().count() >= 3);
                    row.matches = Some(matches);
                    row.excerpts = Some(shown);
                }
                None => row.summary = Some(list_summary(&text, more)),
            }
            row
        });
        let (records, _) = fit(records, budget);
        let read = offset + records.len();
        Ok(Listing {
            total,
            offset,
            next: (read < total).then_some(Offset { offset: read }),
            records,
        })
    }

    /// One record of a project, its body from `offset` characters on, up to `budget`.
    ///
    /// `document` finds the original `record.md` of an agent record's work: its path,
    /// or why it was not found.
    pub fn read_record(
        &self,
        id: &str,
        offset: usize,
        budget: usize,
        document: &dyn Fn(&str) -> Result<String, String>,
    ) -> Result<RecordView, String> {
        validate_id(id)?;
        let c = self.connection()?;
        // The body and what is said of its source are read in one transaction, so a
        // record the app shows anew in between is not taken for one a person changed.
        let c = c.unchecked_transaction().map_err(database_error)?;
        let row = c
            .query_row(
                &format!(
                    "SELECT e.id,e.project_id,e.date,e.kind,e.title,e.updated_at,e.source_url,e.body,
                     e.source_ref,{ORIGIN} FROM diary_entries e WHERE e.id=?1 AND e.project_id IS NOT NULL"
                ),
                [id],
                |r| {
                    Ok((
                        RecordView {
                            id: r.get(0)?,
                            project: r.get(1)?,
                            date: r.get(2)?,
                            kind: r.get(3)?,
                            title: r.get(4)?,
                            updated_at: r.get(5)?,
                            source_url: r.get(6)?,
                            origin: Origin::of(&r.get::<_, String>(9)?),
                            chars: 0,
                            offset,
                            body: String::new(),
                            next: None,
                            edited: None,
                            daily: None,
                            agent: None,
                        },
                        r.get::<_, String>(7)?,
                        r.get::<_, Option<String>>(8)?,
                    ))
                },
            )
            .optional()
            .map_err(database_error)?;
        let (mut view, body, source_ref) =
            row.ok_or_else(|| format!("프로젝트의 기록 중에 이 ID가 없습니다: {id}"))?;
        let chars: Vec<char> = body.chars().collect();
        view.chars = chars.len();
        if offset > chars.len() || (offset == chars.len() && offset > 0) {
            return Err(format!(
                "--offset이 본문 길이({}자)를 넘습니다.",
                chars.len()
            ));
        }
        // What is said of the record's source is given once, on its first page.
        let mut said = 0;
        if offset == 0 {
            self.describe(&c, &mut view, &body, source_ref.as_deref(), document)?;
            if view.edited.is_some() || view.daily.is_some() || view.agent.is_some() {
                said = chars_of(&(&view.edited, &view.daily, &view.agent));
            }
        }
        let room = budget.saturating_sub(said).max(1000);
        let end = (offset + room).min(chars.len());
        view.body = chars[offset..end].iter().collect();
        view.next = (end < chars.len()).then_some(Offset { offset: end });
        Ok(view)
    }

    /// Adds to `view` what the record's source says of it: for a daily record, its
    /// file and whether a person changed the text; for an agent record, its work.
    fn describe(
        &self,
        c: &rusqlite::Connection,
        view: &mut RecordView,
        body: &str,
        source_ref: Option<&str>,
        document: &dyn Fn(&str) -> Result<String, String>,
    ) -> Result<(), String> {
        let id = view.id.as_str();
        match view.origin {
            Origin::Daily => {
                let shown: Option<String> = c
                    .query_row(
                        "SELECT shown_body FROM daily_record_entries WHERE entry_id=?1",
                        [id],
                        |r| r.get(0),
                    )
                    .optional()
                    .map_err(database_error)?;
                view.edited = shown.map(|text| text != body);
                view.daily = source_ref
                    .and_then(daily_place)
                    .map(|(_, group, date)| DailyRef {
                        file: format!("daily/{group}/{date}.json"),
                    });
            }
            Origin::Agent => {
                let evidence: Option<AgentRow> = c
                    .query_row(
                        "SELECT work_id,started_at,completed_at,status,commits FROM entry_evidence WHERE entry_id=?1",
                        [id],
                        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
                    )
                    .optional()
                    .map_err(database_error)?;
                let work = source_ref
                    .and_then(|r| r.strip_prefix("space-work:"))
                    .map(str::to_owned);
                let (work_id, started_at, completed_at, status, commits) = match evidence {
                    Some((w, s, d, status, commits)) => (
                        w.or(work),
                        s,
                        d,
                        Some(status).filter(|s| !s.is_empty()),
                        serde_json::from_str(&commits).unwrap_or(Value::String(commits)),
                    ),
                    None => (work, None, None, None, json!([])),
                };
                let found = work_id.as_deref().map(document);
                view.agent = Some(AgentRef {
                    work_id,
                    started_at,
                    completed_at,
                    status,
                    commits,
                    document: found.clone().and_then(Result::ok),
                    document_error: found.and_then(Result::err),
                });
            }
            _ => {}
        }
        Ok(())
    }

    /// The project, `source_ref` and origin of a record of a project.
    fn record_ref(&self, id: &str) -> Result<(String, Option<String>, String), String> {
        validate_id(id)?;
        self.connection()?
            .query_row(
                &format!(
                    "SELECT e.project_id,e.source_ref,{ORIGIN} FROM diary_entries e WHERE e.id=?1 AND e.project_id IS NOT NULL"
                ),
                [id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()
            .map_err(database_error)?
            .ok_or_else(|| format!("프로젝트의 기록 중에 이 ID가 없습니다: {id}"))
    }

    /// The items of the daily record behind the record `id`, with where the blocks each
    /// item cites are in the kept originals. The record file is read from `vault`.
    pub fn read_evidence(
        &self,
        vault: &Path,
        id: &str,
        offset: usize,
        budget: usize,
    ) -> Result<EvidenceView, String> {
        let (owner, source_ref, origin) = self.record_ref(id)?;
        let place = (Origin::of(&origin) == Origin::Daily)
            .then_some(source_ref.as_deref())
            .flatten()
            .and_then(daily_place)
            .ok_or("하루 정리 기록이 아니어서 근거가 없습니다. --evidence는 하루 정리 기록에만 씁니다.")?;
        let (project, group, date) = place;
        // An entry a person moved to another project keeps the reference of the day it
        // was made for: that day's evidence is another project's.
        if owner != project {
            return Err("이 기록은 다른 프로젝트로 옮겨져 있어 근거를 주지 않습니다. 근거는 처음 만든 프로젝트의 하루 기록에 있습니다.".into());
        }
        let record = read_daily(vault, group, date)?;
        // The file in the place must be that project's record of that day, as the code
        // that shows records checks.
        if record.project.id != project
            || record.date != date
            || group_name(&record.project.repo_key) != group
        {
            return Err(format!(
                "daily/{group}/{date}.json이 이 기록의 프로젝트나 날짜의 것이 아니어서 읽지 않았습니다."
            ));
        }
        let items = daily_items(&record);
        let total = items.len();
        if offset > 0 && offset >= total {
            return Err(format!("--offset이 항목 수({total}개)를 넘습니다."));
        }
        // An item takes room for itself and for the blocks it cites that the page does
        // not hold yet.
        let mut cited: BTreeMap<usize, &Evidence> = BTreeMap::new();
        let mut page = Vec::new();
        let mut used = 0;
        for (item, evidence) in &items[offset..] {
            // An item can cite one block twice, as reason and as proposal: it is given once.
            let new: BTreeMap<usize, &Evidence> = evidence
                .iter()
                .copied()
                .filter(|e| !cited.contains_key(&e.block))
                .map(|e| (e.block, e))
                .collect();
            let cost =
                chars_of(item) + new.values().map(|e| chars_of(&block_ref(e))).sum::<usize>();
            if !page.is_empty() && used + cost > budget {
                break;
            }
            used += cost;
            cited.extend(new);
            page.push(item.clone());
        }
        let read = offset + page.len();
        Ok(EvidenceView {
            id: id.to_owned(),
            file: format!("daily/{group}/{date}.json"),
            date: date.to_owned(),
            revision: record.revision,
            total,
            offset,
            items: page,
            blocks: cited.into_values().map(block_ref).collect(),
            next: (read < total).then_some(Offset { offset: read }),
        })
    }
}

fn block_ref(e: &Evidence) -> BlockRef {
    BlockRef {
        block: e.block,
        speaker: e.speaker.label(),
        at: e.at.clone(),
        file: e.file.clone(),
        line: e.line,
    }
}

/// The project ID, store folder and date a daily record's `source_ref` names:
/// `daily-record:<project>:<folder>:<date>`.
pub(super) fn daily_place(source_ref: &str) -> Option<(&str, &str, &str)> {
    let mut parts = source_ref.strip_prefix("daily-record:")?.splitn(3, ':');
    let (project, group, date) = (parts.next()?, parts.next()?, parts.next()?);
    let plain = |s: &str| {
        !s.is_empty()
            && s.bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    };
    (plain(group) && crate::storage::validate_date(date).is_ok()).then_some((project, group, date))
}

/// The latest daily record of `date` in the store folder `group`. No step of the way may
/// be a link, as for the code that shows these records.
fn read_daily(vault: &Path, group: &str, date: &str) -> Result<DailyRecord, String> {
    let mut path = vault.to_path_buf();
    for part in ["daily", group, &format!("{date}.json")] {
        path.push(part);
        if is_link(&path).map_err(|e| e.to_string())? {
            return Err("링크를 거치는 하루 기록은 읽지 않습니다.".into());
        }
    }
    let bytes = std::fs::read(&path)
        .map_err(|e| format!("하루 기록 파일을 읽지 못했습니다(daily/{group}/{date}.json): {e}"))?;
    serde_json::from_slice(&bytes).map_err(|e| format!("하루 기록 파일 형식 오류: {e}"))
}

/// The items of a daily record in the order the diary shows them, each with the blocks
/// it cites.
fn daily_items(record: &DailyRecord) -> Vec<(Value, Vec<&Evidence>)> {
    fn tagged(mut item: Value, cited: Vec<&Evidence>) -> (Value, Vec<&Evidence>) {
        let mut numbers: Vec<usize> = cited.iter().map(|e| e.block).collect();
        numbers.sort_unstable();
        numbers.dedup();
        item["blocks"] = json!(numbers);
        (item, cited)
    }
    let quotes = |quoted: &[Quoted]| -> Value {
        quoted
            .iter()
            .map(|q| json!({"block": q.at.block, "quote": q.quote}))
            .collect()
    };
    let mut items = Vec::new();
    if let Some(o) = &record.overview {
        items.push(tagged(
            json!({"section": "overview", "text": o.text}),
            o.evidence.iter().collect(),
        ));
    }
    for s in &record.summary {
        items.push(tagged(
            json!({"section": "summary", "text": s.text}),
            s.evidence.iter().collect(),
        ));
    }
    for d in &record.development {
        items.push(tagged(
            json!({"section": "development", "title": d.title, "detail": d.detail}),
            d.evidence.iter().collect(),
        ));
    }
    for t in &record.troubleshooting {
        items.push(tagged(
            json!({"section": "troubleshooting", "problem": t.problem, "cause": t.cause,
                "fix": t.fix, "verification": t.verification}),
            t.evidence.iter().collect(),
        ));
    }
    for d in &record.decisions {
        let cited = d
            .evidence
            .iter()
            .chain(d.proposal_evidence.iter().map(|q| &q.at))
            .chain(d.decision_evidence.iter().map(|q| &q.at))
            .collect();
        items.push(tagged(
            json!({"section": "decision", "decision": d.decision, "reason": d.reason,
                "rejected": d.rejected, "proposedBy": d.proposed_by.label(),
                "decidedBy": d.decided_by.label(), "proposal": quotes(&d.proposal_evidence),
                "decided": quotes(&d.decision_evidence)}),
            cited,
        ));
    }
    for n in &record.next_steps {
        items.push(tagged(
            json!({"section": "nextStep", "text": n.text}),
            n.evidence.iter().collect(),
        ));
    }
    items
}
