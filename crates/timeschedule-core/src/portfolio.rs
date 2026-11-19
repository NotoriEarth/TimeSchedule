use crate::storage::{database_error, validate_id, validate_text, Storage};
use crate::timeline::{status_label, RecordCommit};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use uuid::Uuid;

mod markdown;

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

#[derive(Clone, Debug)]
pub struct PortfolioPanel {
    pub project_id: String,
    pub experiences: usize,
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
        let experiences = c
            .query_row(
                "SELECT count(*) FROM project_experiences WHERE project_id=?1",
                [project],
                |r| r.get::<_, usize>(0),
            )
            .map_err(database_error)?;
        let mut needs_answer = false;
        if answer.is_none() && experiences > 0 {
            let mut q = c
                .prepare("SELECT experience_text FROM project_experiences WHERE project_id=?1")
                .map_err(database_error)?;
            let mut rows = q.query([project]).map_err(database_error)?;
            while let Some(row) = rows.next().map_err(database_error)? {
                let body: String = row.get(0).map_err(database_error)?;
                if markdown::needs_role(&body) {
                    needs_answer = true;
                    break;
                }
            }
        }
        let last: Option<(i64,String)> = c.query_row(
            "SELECT revision,folder FROM portfolio_editions WHERE project_id=?1 AND published=1",
            [project], |r| Ok((r.get(0)?,r.get(1)?)))
            .optional().map_err(database_error)?;
        let last_file = if let Some((revision, folder)) = last {
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
        validate_id(project)?;
        let (revision, folder, markdown, evidence, published) = {
            let mut c = self.connection()?;
            let tx = c
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(database_error)?;
            let (markdown, evidence) = render(&tx, project)?;
            let previous = tx.query_row("SELECT revision,folder,markdown=?2 AND evidence=?3,published FROM portfolio_editions WHERE project_id=?1",
                params![project,markdown,evidence], |r| Ok((r.get::<_,i64>(0)?,r.get::<_,String>(1)?,r.get::<_,bool>(2)?,r.get::<_,bool>(3)?)))
                .optional().map_err(database_error)?;
            let (revision, folder, published) = match previous {
                Some((revision, folder, true, published)) => (revision, folder, published),
                other => {
                    let revision = other.map_or(1, |(r, ..)| r + 1);
                    let folder = Uuid::new_v4().to_string();
                    tx.execute("INSERT INTO portfolio_editions VALUES(?1,?2,?3,?4,?5,0)
                      ON CONFLICT(project_id) DO UPDATE SET revision=excluded.revision,folder=excluded.folder,
                      markdown=excluded.markdown,evidence=excluded.evidence,published=0",
                      params![project,revision,folder,markdown,evidence]).map_err(database_error)?;
                    (revision, folder, false)
                }
            };
            tx.commit().map_err(database_error)?;
            (revision, folder, markdown, evidence, published)
        };
        validate_id(&folder)?;
        let file = publish(
            root,
            project,
            &format!("v{revision}-{folder}"),
            &markdown,
            &evidence,
            published,
        )?;
        if !published {
            self.connection()?
                .execute(
                    "UPDATE portfolio_editions SET published=1 WHERE project_id=?1 AND folder=?2",
                    params![project, folder],
                )
                .map_err(database_error)?;
        }
        Ok(file)
    }
}

fn render(c: &Connection, project: &str) -> Result<(String, String), String> {
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
    let mut count = 0;
    while let Some(r) = rows.next().map_err(database_error)? {
        let get = |idx| r.get::<_, String>(idx).map_err(database_error);
        let id = get(0)?;
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
        count += 1;
    }
    if count == 0 {
        return Err("이 프로젝트에 설계·개발·트러블슈팅 기록이 없습니다.".into());
    }
    Ok((doc, sources))
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
