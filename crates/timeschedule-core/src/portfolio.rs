use crate::storage::{database_error, validate_id, validate_text, Storage};
use crate::timeline::{status_label, RecordCommit};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use uuid::Uuid;

mod markdown;
mod writing;

pub(crate) fn migrate(c: &Connection) -> Result<(), String> {
    c.execute_batch(
        "CREATE VIEW project_experiences AS
         SELECT e.id,e.project_id,e.kind,e.title,e.date,e.source_url,
           r.revision,r.work_id,json_extract(r.payload,'$.summary') AS summary,
           COALESCE(e.body=json_extract(r.managed_snapshot,'$.body'),0) AS intact,
           CASE WHEN e.body=json_extract(r.managed_snapshot,'$.body') THEN
             COALESCE(NULLIF(json_extract(r.payload,'$.detail'),''),json_extract(r.payload,'$.summary'),e.body)
             ELSE e.body END AS experience_text, COALESCE(h.commits,'[]') AS commits,h.status
         FROM diary_entries e LEFT JOIN entry_evidence h ON h.entry_id=e.id
         LEFT JOIN project_record_receipts r ON r.work_id=h.work_id AND r.entry_id=e.id
         WHERE e.project_id IS NOT NULL AND
         (e.kind IN ('design','development','troubleshooting') OR
          (e.kind='summary' AND e.source_ref LIKE 'space-work:%' AND e.title LIKE '설계 경험 · %'));
         CREATE TABLE portfolio_answers(
           project_id TEXT PRIMARY KEY REFERENCES projects(id) ON DELETE CASCADE,
           answer TEXT NOT NULL);
         CREATE TABLE portfolio_editions(
           project_id TEXT PRIMARY KEY REFERENCES projects(id) ON DELETE CASCADE,
           revision INTEGER NOT NULL, folder TEXT NOT NULL, markdown TEXT NOT NULL, evidence TEXT NOT NULL,
           published INTEGER NOT NULL DEFAULT 0);
         PRAGMA user_version=8;"
    ).map_err(database_error)
}

pub(crate) fn migrate_v10(c: &Connection) -> Result<(), String> {
    c.execute_batch("ALTER TABLE portfolio_editions RENAME TO portfolio_v9;
        CREATE TABLE portfolio_editions(
          project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
          revision INTEGER NOT NULL, folder TEXT NOT NULL, markdown TEXT NOT NULL, evidence TEXT NOT NULL,
          published INTEGER NOT NULL, selection TEXT, last_used INTEGER NOT NULL,
          PRIMARY KEY(project_id,revision));
        INSERT INTO portfolio_editions SELECT *,NULL,revision FROM portfolio_v9;
        DROP TABLE portfolio_v9;
        PRAGMA user_version=10;").map_err(database_error)
}

#[derive(Clone, Debug)]
pub struct ExperienceChoice {
    pub id: String,
    pub title: String,
    pub date: String,
    pub needs_answer: bool,
}

#[derive(Clone, Debug)]
pub struct PortfolioPanel {
    pub project_id: String,
    pub experiences: usize,
    pub choices: Vec<ExperienceChoice>,
    pub last_selection: Option<BTreeSet<String>>,
    /// Empty means the user explicitly deferred individual attribution.
    pub answer: Option<String>,
    pub needs_answer: bool,
    pub last_file: Option<PathBuf>,
}

fn answer(c: &Connection, project: &str) -> Result<Option<String>, String> {
    c.query_row(
        "SELECT answer FROM portfolio_answers WHERE project_id=?1",
        [project],
        |r| r.get(0),
    )
    .optional()
    .map_err(database_error)
}

impl Storage {
    pub fn portfolio_panel(&self, project: &str) -> Result<PortfolioPanel, String> {
        validate_id(project)?;
        let c = self.connection()?;
        let answer = answer(&c, project)?;
        let mut q = c
            .prepare(
                "SELECT x.id,x.title,x.date, CASE WHEN ?2 THEN x.experience_text ELSE '' END
            FROM project_experiences x JOIN diary_entries e ON x.id=e.id
            WHERE x.project_id=?1 ORDER BY x.date DESC,e.updated_at DESC,x.id",
            )
            .map_err(database_error)?;
        let choices = q
            .query_map(params![project, answer.is_none()], |r| {
                Ok(ExperienceChoice {
                    id: r.get(0)?,
                    title: r.get(1)?,
                    date: r.get(2)?,
                    needs_answer: answer.is_none() && markdown::needs_role(&r.get::<_, String>(3)?),
                })
            })
            .map_err(database_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(database_error)?;
        let experiences = choices.len();
        let needs_answer = choices.iter().any(|e| e.needs_answer);
        let last: Option<(i64,String,Option<String>)> = c.query_row(
            "SELECT revision,folder,selection FROM portfolio_editions WHERE project_id=?1 AND published=1 ORDER BY last_used DESC LIMIT 1",
            [project], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?)))
            .optional().map_err(database_error)?;
        let last_selection = last
            .as_ref()
            .and_then(|(_, _, s)| s.as_deref())
            .map(serde_json::from_str)
            .transpose()
            .map_err(|e| format!("저장한 경험 선택을 읽지 못했습니다. {e}"))?;
        let last_file = if let Some((revision, folder, _)) = last {
            validate_id(&folder)?;
            Some(
                Path::new("portfolios")
                    .join(project)
                    .join(format!("v{revision}-{folder}"))
                    .join("portfolio.md"),
            )
        } else {
            None
        };
        Ok(PortfolioPanel {
            project_id: project.into(),
            experiences,
            choices,
            last_selection,
            answer,
            needs_answer,
            last_file,
        })
    }

    pub fn save_portfolio_answer(
        &self,
        project: &str,
        expected: Option<&str>,
        value: &str,
    ) -> Result<(), String> {
        let value = value.trim();
        validate_text(value, "역할 답변", 1000)?;
        let mut c = self.connection()?;
        let tx = c
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(database_error)?;
        let current = answer(&tx, project)?;
        if current.as_deref() != expected && current.as_deref() != Some(value) {
            return Err("역할 답변이 변경되었습니다. 프로젝트를 다시 열어 확인하세요.".into());
        }
        tx.execute("INSERT INTO portfolio_answers VALUES(?1,?2) ON CONFLICT(project_id) DO UPDATE SET answer=excluded.answer",
            params![project,value]).map_err(database_error)?;
        tx.commit().map_err(database_error)
    }

    /// Reserve one edition for a consistent snapshot. Identical requests, including
    /// retry after a filesystem failure, reuse the reservation.
    pub fn export_portfolio(&self, project: &str, root: &Path) -> Result<PathBuf, String> {
        self.export_portfolio_selection(project, root, None)
    }

    pub fn export_portfolio_selection(
        &self,
        project: &str,
        root: &Path,
        selected: Option<&BTreeSet<String>>,
    ) -> Result<PathBuf, String> {
        self.export_edition(project, root, selected, None)
    }

    pub fn export_portfolio_with_writer(
        &self,
        project: &str,
        root: &Path,
        selected: Option<&BTreeSet<String>>,
        writer: impl Fn(&str) -> Result<String, String>,
    ) -> Result<PathBuf, String> {
        self.export_edition(project, root, selected, Some(&writer))
    }

    fn export_edition(
        &self,
        project: &str,
        root: &Path,
        selected: Option<&BTreeSet<String>>,
        writer: Option<&dyn Fn(&str) -> Result<String, String>>,
    ) -> Result<PathBuf, String> {
        if selected.is_some_and(BTreeSet::is_empty) {
            return Err("경험을 한 건 이상 선택하세요.".into());
        }
        validate_id(project)?;
        let selection = selected
            .map(serde_json::to_string)
            .transpose()
            .map_err(|e| e.to_string())?;
        let mut c = self.connection()?;
        let read = c.transaction().map_err(database_error)?;
        let snapshot = render(&read, project, selected)?;
        let key = writer.as_ref().map(|_| {
            serde_json::json!([crate::terra::MODEL, snapshot.role, snapshot.template]).to_string()
        });
        let lookup = |c: &Connection, source: Option<&str>| -> Result<Option<Edition>, String> {
            c.query_row(
                "SELECT revision,folder,markdown,published,carried_edit FROM portfolio_editions
                WHERE project_id=?1 AND selection IS ?2 AND evidence=?3 AND terra_source IS ?4
                AND (?4 IS NOT NULL OR markdown=?5) ORDER BY revision DESC LIMIT 1",
                params![
                    project,
                    selection,
                    snapshot.evidence,
                    source,
                    snapshot.template
                ],
                |r| {
                    Ok(Edition {
                        revision: r.get(0)?,
                        folder: r.get(1)?,
                        markdown: r.get(2)?,
                        published: r.get(3)?,
                        carried_edit: r.get(4)?,
                    })
                },
            )
            .optional()
            .map_err(database_error)
        };
        let cached = lookup(&read, key.as_deref())?;
        let previous = if writer.is_some() && cached.is_none() {
            // Keep edits within the same experience set and role, even as evidence changes.
            let prior = read
                .query_row(
                    "SELECT revision,folder,markdown,published,carried_edit,evidence
                FROM portfolio_editions WHERE project_id=?1 AND selection IS ?2 AND published=1
                AND terra_source IS NOT NULL AND json_extract(terra_source,'$[1]') IS ?3
                ORDER BY last_used DESC LIMIT 1",
                    params![project, selection, snapshot.role],
                    |r| {
                        Ok((
                            Edition {
                                revision: r.get(0)?,
                                folder: r.get(1)?,
                                markdown: r.get(2)?,
                                published: r.get(3)?,
                                carried_edit: r.get(4)?,
                            },
                            r.get::<_, String>(5)?,
                        ))
                    },
                )
                .optional()
                .map_err(database_error)?;
            let entries = |s: &str| {
                s.lines()
                    .filter(|l| l.starts_with("<a id=\"entry-"))
                    .map(str::to_owned)
                    .collect::<BTreeSet<_>>()
            };
            match prior {
                Some((edition, evidence)) if entries(&evidence) == entries(&snapshot.evidence) => {
                    Some(edition)
                }
                _ => lookup(&read, None)?,
            }
        } else {
            None
        };
        read.commit().map_err(database_error)?;
        drop(c);
        let edition = if let Some(existing) = cached {
            existing
        } else {
            let edited = || -> Result<Option<String>, String> {
                let Some(old) = previous.as_ref().filter(|e| e.published) else {
                    return Ok(None);
                };
                validate_id(&old.folder)?;
                let path = root
                    .join("portfolios")
                    .join(project)
                    .join(old.directory())
                    .join("portfolio.md");
                let body = std::fs::read_to_string(path)
                    .map_err(|e| format!("이전 편집본을 확인할 수 없습니다. {e}"))?;
                if body.replace("\r\n", "\n") != old.markdown.replace("\r\n", "\n") {
                    return Ok(Some(body));
                }
                Ok(old.carried_edit.clone().or_else(|| {
                    old.markdown
                        .split_once("\n\n## 백엔드 경험 요약\n\n")
                        .map(|(prefix, _)| prefix.to_owned())
                }))
            };
            let prior = edited()?;
            let markdown = match writer {
                Some(infer) => writing::compose(&snapshot, prior.as_deref(), infer)?,
                None => snapshot.template.clone(),
            };
            let mut c = self.connection()?;
            let tx = c
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(database_error)?;
            // A different request or a user edit may have completed during inference.
            if render(&tx, project, selected)? != snapshot || edited()? != prior {
                return Err(
                    "검토 중 경험·답변 또는 편집본이 바뀌었습니다. 다시 생성하세요.".into(),
                );
            }
            let edition = if let Some(existing) = lookup(&tx, key.as_deref())? {
                existing
            } else {
                let revision=tx.query_row("SELECT COALESCE(MAX(revision),0)+1 FROM portfolio_editions WHERE project_id=?1",[project],|r|r.get(0)).map_err(database_error)?;
                let folder = Uuid::new_v4().to_string();
                tx.execute("INSERT INTO portfolio_editions(project_id,revision,folder,markdown,evidence,published,selection,last_used,terra_source,carried_edit)
                    VALUES(?1,?2,?3,?4,?5,0,?6,0,?7,?8)",params![project,revision,folder,markdown,snapshot.evidence,selection,key,prior]).map_err(database_error)?;
                Edition {
                    revision,
                    folder,
                    markdown,
                    published: false,
                    carried_edit: prior,
                }
            };
            tx.commit().map_err(database_error)?;
            edition
        };
        validate_id(&edition.folder)?;
        let file = publish(
            root,
            project,
            &edition.directory(),
            &edition.markdown,
            &snapshot.evidence,
            edition.published,
        )?;
        self.connection()?.execute("UPDATE portfolio_editions SET published=1,last_used=(SELECT COALESCE(MAX(last_used),0)+1 FROM portfolio_editions WHERE project_id=?1) WHERE project_id=?1 AND folder=?2",
            params![project,edition.folder]).map_err(database_error)?;
        Ok(file)
    }
}
struct Edition {
    revision: i64,
    folder: String,
    markdown: String,
    published: bool,
    carried_edit: Option<String>,
}
impl Edition {
    fn directory(&self) -> String {
        format!("v{}-{}", self.revision, self.folder)
    }
}
#[derive(PartialEq)]
struct Snapshot {
    name: String,
    role: Option<String>,
    cases: Vec<Case>,
    template: String,
    evidence: String,
}
#[derive(serde::Serialize, PartialEq)]
struct Case {
    id: String,
    title: String,
    body: String,
    confirmed: Vec<ConfirmedQuote>,
}

#[derive(serde::Serialize, PartialEq)]
struct ConfirmedQuote {
    anchor: String,
    line: usize,
    role: String,
    quote: String,
}

fn render(
    c: &Connection,
    project: &str,
    selected: Option<&BTreeSet<String>>,
) -> Result<Snapshot, String> {
    let (name, description): (String, String) = c
        .query_row(
            "SELECT title,description FROM projects WHERE id=?1",
            [project],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(database_error)?;
    let role = answer(c, project)?;
    let mut doc = format!("# {} · 백엔드 개발 포트폴리오\n\n", markdown::plain(&name));
    let mut sources = String::from("# 포트폴리오 생성에 사용한 근거\n\n");
    if !description.trim().is_empty() {
        writeln!(
            doc,
            "{} [프로젝트 설명](evidence.md#project)\n",
            markdown::plain(&description)
        )
        .unwrap();
        markdown::source(&mut sources, "project", "프로젝트 설명", &description);
    }
    if let Some(role) = role.as_deref().filter(|s| !s.is_empty()) {
        writeln!(
            doc,
            "**프로젝트에서 맡은 역할** {} [사용자 답변](evidence.md#role)\n",
            markdown::plain(role)
        )
        .unwrap();
        markdown::source(&mut sources, "role", "사용자 답변", role);
    }
    let mut q = c
        .prepare(
            "SELECT id,kind,title,date,experience_text,source_url,revision,summary,intact,commits,status,work_id
             FROM project_experiences WHERE project_id=?1 ORDER BY date,id",
        )
        .map_err(database_error)?;
    let mut rows = q.query([project]).map_err(database_error)?;
    let mut cases = Vec::new();
    while let Some(r) = rows.next().map_err(database_error)? {
        let get = |idx| r.get::<_, String>(idx).map_err(database_error);
        let id = get(0)?;
        if selected.is_some_and(|ids| !ids.contains(&id)) {
            continue;
        }
        validate_id(&id)?;
        let (kind, title, date, body, url) = (get(1)?, get(2)?, get(3)?, get(4)?, get(5)?);
        let text = body.as_str();
        let revision: Option<i64> = r.get(6).map_err(database_error)?;
        let summary: Option<String> = r.get(7).map_err(database_error)?;
        let intact: bool = r.get(8).map_err(database_error)?;
        let commits: Vec<RecordCommit> =
            serde_json::from_str(&get(9)?).map_err(|e| e.to_string())?;
        let status: Option<String> = r.get(10).map_err(database_error)?;
        let work_id: Option<String> = r.get(11).map_err(database_error)?;
        if role.is_none() && markdown::needs_role(text) {
            return Err("프로젝트에서 맡은 역할을 입력하거나 개인 기여 보류를 선택하세요.".into());
        }
        let label = match kind.as_str() {
            "troubleshooting" => "문제 해결",
            "design" | "summary" => "설계",
            _ => "개발",
        };
        let anchor = format!("entry-{id}");
        let link = format!("[기록 근거](evidence.md#{anchor})");
        writeln!(
            doc,
            "## {}\n\n{date} · {label} 경험\n",
            markdown::plain(&title)
        )
        .unwrap();
        let blurb = summary
            .as_deref()
            .filter(|_| intact)
            .filter(|s| !s.trim().is_empty())
            .or_else(|| markdown::opening(text));
        if let Some(text) = blurb {
            writeln!(doc, "{} {link}\n", markdown::plain(text)).unwrap();
        } else {
            writeln!(doc, "{link}\n").unwrap();
        }
        for (label, value) in markdown::facts(text) {
            if Some(value) == blurb || (label == "개인 역할·기여" && role.as_deref() == Some(""))
            {
                continue;
            }
            writeln!(doc, "- **{label}** {} {link}", markdown::plain(value)).unwrap();
        }
        doc.push('\n');
        markdown::source(&mut sources, &anchor, &title, text);
        writeln!(sources, "기록일 {date} · 기록 ID `{id}`\n").unwrap();
        if let (Some(revision), Some(work)) = (revision, work_id) {
            writeln!(
                sources,
                "작업 `{work}` · 수신 판본 {revision} · {}\n",
                status_label(status.as_deref().unwrap_or(""))
            )
            .unwrap();
            if !intact {
                sources.push_str("다이어리에서 편집한 본문을 사용했습니다.\n\n");
            }
            if let Some(summary) = summary {
                sources.push_str("수신 요약\n\n");
                markdown::verbatim(&mut sources, &summary);
            }
        }
        if let Some(url) = markdown::web_link(&url) {
            writeln!(sources, "[기록 출처](<{url}>)\n").unwrap();
        }
        for commit in commits {
            if let Some(url) = markdown::web_link(&commit.url) {
                writeln!(
                    sources,
                    "- [커밋 {}](<{url}>) {}",
                    markdown::plain(&commit.hash),
                    markdown::plain(&commit.subject)
                )
                .unwrap();
            } else {
                writeln!(
                    sources,
                    "- 커밋 {} {}",
                    markdown::plain(&commit.hash),
                    markdown::plain(&commit.subject)
                )
                .unwrap();
            }
        }
        sources.push('\n');
        let mut query=c.prepare("SELECT rowid,line,role,quote FROM chat_experience_evidence WHERE entry_id=?1 ORDER BY line,rowid").map_err(database_error)?;
        let mut confirmed = Vec::new();
        let quotes = query
            .query_map([&id], |r| {
                Ok(ConfirmedQuote {
                    anchor: format!("quote-{}-{}", id, r.get::<_, i64>(0)?),
                    line: r.get(1)?,
                    role: r.get(2)?,
                    quote: r.get(3)?,
                })
            })
            .map_err(database_error)?;
        for quote in quotes {
            let quote = quote.map_err(database_error)?;
            // An edit removing a quotation takes precedence over the deduplication receipt.
            if body.contains(&format!(
                "{} ({} · 원문 {}행)",
                quote.quote, quote.role, quote.line
            )) {
                let heading = format!("{} · 원문 {}행", quote.role, quote.line);
                markdown::source(&mut sources, &quote.anchor, &heading, &quote.quote);
                confirmed.push(quote);
            }
        }
        cases.push(Case {
            id,
            title,
            body,
            confirmed,
        });
    }
    if selected.is_some_and(|ids| ids.len() != cases.len()) {
        return Err("선택한 경험이 변경되었습니다. 목록을 다시 확인하세요.".into());
    }
    if cases.is_empty() {
        return Err("이 프로젝트에 설계·개발·트러블슈팅 기록이 없습니다.".into());
    }
    Ok(Snapshot {
        name,
        role,
        cases,
        template: doc,
        evidence: sources,
    })
}

// Publish the pair together. A completed directory is never rewritten, so both
// the Markdown and its evidence can be edited without an export erasing them.
fn publish(
    root: &Path,
    project: &str,
    edition: &str,
    doc: &str,
    evidence: &str,
    published: bool,
) -> Result<PathBuf, String> {
    use std::fs;
    use std::io::Write;
    let parent = root.join("portfolios").join(project);
    fs::create_dir_all(&parent).map_err(|e| e.to_string())?;
    let dest = parent.join(edition);
    let file = dest.join("portfolio.md");
    if !published && !dest.try_exists().map_err(|e| e.to_string())? {
        let staging = parent.join(format!(".staging-{}", Uuid::new_v4()));
        fs::create_dir(&staging).map_err(|e| e.to_string())?;
        let attempt = (|| -> std::io::Result<()> {
            for (name, body) in [("portfolio.md", doc), ("evidence.md", evidence)] {
                let mut f = fs::File::create_new(staging.join(name))?;
                f.write_all(body.as_bytes())?;
                f.sync_all()?;
            }
            match fs::rename(&staging, &dest) {
                Err(_) if dest.is_dir() => Ok(()),
                result => result,
            }
        })();
        if staging.exists() {
            let _ = fs::remove_dir_all(&staging);
        }
        attempt.map_err(|e| e.to_string())?;
    }
    if !file.is_file() || !dest.join("evidence.md").is_file() {
        return Err(format!(
            "저장한 판본의 파일이 이동되거나 삭제되었습니다. 편집본을 확인하세요. {}",
            dest.display()
        ));
    }
    Ok(file)
}
