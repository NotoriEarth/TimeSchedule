//! Local, opt-in project history from local Codex Desktop and Claude Code session files.
use crate::{
    chat_originals::OriginalsProject,
    project_records::repo_key,
    storage::{database_error, timestamp, Storage},
};
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use std::{
    collections::HashMap,
    fs,
    io::{self, BufRead, BufReader, Seek},
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};

mod claude;
mod codex;
pub(crate) mod command;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Provider {
    Codex,
    Claude,
}
impl Provider {
    fn session_key(self, id: &str) -> String {
        match self {
            Self::Codex => id.to_owned(),
            Self::Claude => format!("claude:{id}"),
        }
    }
    fn source_ref(self, project: &str, key: &str) -> String {
        match self {
            Self::Codex => format!("codex-chat:{project}:{key}"),
            Self::Claude => format!(
                "claude-code-chat:{project}:{}",
                key.trim_start_matches("claude:")
            ),
        }
    }
}

pub(crate) mod transcript;
use transcript::Transcript;

pub(crate) fn migrate(c: &Connection) -> Result<(), String> {
    c.execute_batch("CREATE TABLE codex_chat_projects(
        project_id TEXT PRIMARY KEY REFERENCES projects(id) ON DELETE CASCADE, repo_key TEXT NOT NULL);
        CREATE TABLE codex_chat_sessions(
        project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE, session_id TEXT NOT NULL,
        entry_id TEXT NOT NULL, file_size INTEGER NOT NULL, modified INTEGER NOT NULL,
        index_title TEXT NOT NULL, generated_title TEXT NOT NULL,
        PRIMARY KEY(project_id,session_id));
        PRAGMA user_version=9;").map_err(database_error)
}

#[derive(Debug, Default, Clone, Serialize)]
pub struct ScanReport {
    pub saved: usize,
    pub unchanged: usize,
    pub failed: usize,
}

impl Storage {
    pub fn chat_capture_enabled(&self, project: &str) -> Result<bool, String> {
        let c = self.connection()?;
        let item = c.query_row(
            "SELECT p.repo_path,w.repo_key FROM projects p JOIN codex_chat_projects w ON w.project_id=p.id WHERE p.id=?1",
            [project],
            |r| Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?)),
        ).optional().map_err(database_error)?;
        Ok(item.is_some_and(|(path, key)| repo_key(&path) == key))
    }

    pub fn set_chat_capture(&self, project: &str, enabled: bool) -> Result<(), String> {
        self.set_capture_sources(project, enabled, true)
    }

    pub fn set_capture_sources(
        &self,
        project: &str,
        enabled: bool,
        include_claude: bool,
    ) -> Result<(), String> {
        let c = self.connection()?;
        if enabled {
            let path: String = c
                .query_row(
                    "SELECT repo_path FROM projects WHERE id=?1",
                    [project],
                    |r| r.get(0),
                )
                .map_err(database_error)?;
            if !Path::new(&path).is_absolute() || !Path::new(&path).is_dir() {
                return Err("먼저 프로젝트에 로컬 저장소 폴더를 연결하세요.".into());
            }
            c.execute(
                "INSERT INTO codex_chat_projects(project_id,repo_key,include_claude) VALUES(?1,?2,?3)
                 ON CONFLICT(project_id) DO UPDATE SET repo_key=excluded.repo_key,include_claude=excluded.include_claude",
                params![project, repo_key(&path),include_claude],
            )
            .map_err(database_error)?;
        } else {
            c.execute(
                "DELETE FROM codex_chat_projects WHERE project_id=?1",
                [project],
            )
            .map_err(database_error)?;
        }
        Ok(())
    }

    /// Every project with a repository folder, one per folder, and whether its chats
    /// are captured. A project that is not captured still owns the sessions in its
    /// folder, so a project above it does not keep them.
    pub fn originals_projects(&self) -> Result<Vec<OriginalsProject>, String> {
        let c = self.connection()?;
        let mut q = c.prepare(
            "SELECT p.id,p.title,p.repo_path,w.repo_key,COALESCE(w.include_claude,0) FROM projects p LEFT JOIN codex_chat_projects w ON w.project_id=p.id ORDER BY w.project_id IS NULL"
        ).map_err(database_error)?;
        let rows = q
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, Option<String>>(3)?,
                    r.get::<_, bool>(4)?,
                ))
            })
            .map_err(database_error)?;
        let mut projects: Vec<OriginalsProject> = Vec::new();
        for row in rows {
            let (id, title, path, enabled_key, claude) = row.map_err(database_error)?;
            if !Path::new(&path).is_absolute() {
                continue;
            }
            let root = repo_key(&path);
            // Capture was switched on for the folder the project had then.
            let capture = enabled_key.as_deref() == Some(root.as_str());
            let project = OriginalsProject {
                id,
                title,
                root,
                capture,
                claude: capture && claude,
            };
            match projects.iter_mut().find(|p| p.root == project.root) {
                Some(same) if capture && same.capture => {
                    return Err(
                        "같은 저장소에 채팅 수집 프로젝트가 두 개 연결되어 있습니다.".into(),
                    );
                }
                // Of two projects on one folder, the captured one stands for it.
                Some(same) if capture => *same = project,
                Some(_) => {}
                None => projects.push(project),
            }
        }
        Ok(projects)
    }

    pub fn scan_codex_chats(&self, home: &Path, reread: bool) -> Result<ScanReport, String> {
        self.scan_local_chats(home, None, reread)
    }

    pub fn scan_local_chats(
        &self,
        home: &Path,
        claude_home: Option<&Path>,
        reread: bool,
    ) -> Result<ScanReport, String> {
        let mut report = ScanReport::default();
        let enabled: HashMap<String, (String, bool)> = self
            .originals_projects()?
            .into_iter()
            .filter(|p| p.capture)
            .map(|p| (p.root, (p.id, p.claude)))
            .collect();
        if enabled.is_empty() {
            return Ok(report);
        }
        let codex = read_titles(&home.join("session_index.jsonl"))
            .and_then(|titles| Ok((rollouts(&home.join("sessions"), 0)?, titles)));
        let mut scan = |provider, files: io::Result<(Vec<PathBuf>, HashMap<String, String>)>| {
            let Ok((files, titles)) = files else {
                report.failed += 1;
                return;
            };
            for file in files {
                match self.read_chat(provider, &file, &enabled, &titles, reread) {
                    Ok(Some(true)) => report.saved += 1,
                    Ok(Some(false)) => report.unchanged += 1,
                    Ok(None) => {}
                    Err(_) => report.failed += 1,
                }
            }
        };
        scan(Provider::Codex, codex);
        if let Some(dir) = claude_home.filter(|_| enabled.values().any(|(_, claude)| *claude)) {
            scan(
                Provider::Claude,
                claude::files(&dir.join("projects")).map(|files| (files, HashMap::new())),
            );
        }
        Ok(report)
    }

    fn read_chat(
        &self,
        provider: Provider,
        file: &Path,
        enabled: &HashMap<String, (String, bool)>,
        titles: &HashMap<String, String>,
        reread: bool,
    ) -> Result<Option<bool>, String> {
        let handle = fs::File::open(file).map_err(|e| e.to_string())?;
        let meta = handle.metadata().map_err(|e| e.to_string())?;
        let size = meta.len() as i64;
        let modified = meta
            .modified()
            .and_then(|t| t.duration_since(UNIX_EPOCH).map_err(io::Error::other))
            .map_err(|e| e.to_string())?
            .as_nanos() as i64;
        let mut reader = BufReader::new(handle);
        let header = match provider {
            Provider::Codex => codex::header(&mut reader)?,
            Provider::Claude => claude::header(&mut reader)?,
        };
        let Some(header) = header else {
            return Ok(None);
        };
        let key = repo_key(&header.cwd);
        let Some((project, include_claude)) = enabled.get(&key) else {
            return Ok(None);
        };
        let stem = file.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        let matches = match provider {
            Provider::Codex => stem.ends_with(&format!("-{}", header.id)),
            Provider::Claude => *include_claude && stem == header.id,
        };
        if !matches {
            return Ok(None);
        }
        let index_title = titles.get(&header.id).map(String::as_str).unwrap_or("");
        if !reread && self.connection()?.query_row(
            "SELECT EXISTS(SELECT 1 FROM codex_chat_sessions WHERE project_id=?1 AND session_id=?2
             AND file_size=?3 AND modified=?4 AND index_title=?5)",
            params![project,provider.session_key(&header.id),size,modified,index_title], |r| r.get::<_,bool>(0)
        ).map_err(database_error)? {
            return Ok(Some(false));
        }
        let chat = match provider {
            Provider::Codex => codex::read(&mut reader, header, file, index_title)?,
            Provider::Claude => {
                reader.rewind().map_err(|e| e.to_string())?;
                claude::read(&mut reader, header, file)?
            }
        };
        let Some(chat) = chat else {
            return Ok(None);
        };
        // An actively written file is scanned again if it grew during the read.
        self.store_chat(provider, project, &key, chat, size, modified, index_title)
            .map(Some)
    }

    fn store_chat(
        &self,
        provider: Provider,
        project: &str,
        key: &str,
        chat: Transcript,
        size: i64,
        modified: i64,
        index_title: &str,
    ) -> Result<bool, String> {
        let mut c = self.connection()?;
        let tx = c
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(database_error)?;
        // The user may have disabled capture or selected a different repository during I/O.
        let path: Option<String> = tx
            .query_row(
                "SELECT p.repo_path FROM codex_chat_projects w JOIN projects p ON p.id=w.project_id
             WHERE w.project_id=?1 AND w.repo_key=?2 AND (?3=0 OR w.include_claude)",
                params![project, key, provider == Provider::Claude],
                |r| r.get(0),
            )
            .optional()
            .map_err(database_error)?;
        if !path.is_some_and(|p| repo_key(&p) == key) {
            return Ok(false);
        }
        let previous: Option<(String,String)> = tx.query_row(
            "SELECT entry_id,generated_title FROM codex_chat_sessions WHERE project_id=?1 AND session_id=?2",
            params![project,chat.id], |r|Ok((r.get(0)?,r.get(1)?)),
        ).optional().map_err(database_error)?;
        let (entry, changed) = if let Some((entry, previous_title)) = previous {
            // Keep an edited title/body and a user's deletion. source_body tracks the current original.
            let changed = tx
                .execute(
                    "UPDATE diary_entries SET
                title=CASE WHEN title=?2 THEN ?3 ELSE title END,
                body=CASE WHEN body=source_body THEN ?4 ELSE body END, source_body=?4, updated_at=?5
                WHERE id=?1 AND (source_body<>?4 OR (title=?2 AND title<>?3))",
                    params![entry, previous_title, chat.title, chat.body, timestamp()],
                )
                .map_err(database_error)?;
            (entry, changed > 0)
        } else {
            let entry = uuid::Uuid::new_v4().to_string();
            tx.execute(
                "INSERT INTO diary_entries(id,project_id,date,kind,title,body,source_ref,source_body,updated_at)
                 VALUES(?1,?2,?3,'note',?4,?5,?6,?5,?7)",
                params![entry,project,chat.date,chat.title,chat.body,provider.source_ref(project,&chat.id),timestamp()],
            ).map_err(database_error)?;
            (entry, true)
        };
        tx.execute(
            "INSERT INTO codex_chat_sessions(project_id,session_id,entry_id,file_size,modified,index_title,generated_title)
             VALUES(?1,?2,?3,?4,?5,?6,?7) ON CONFLICT(project_id,session_id) DO UPDATE SET
             file_size=excluded.file_size,modified=excluded.modified,index_title=excluded.index_title,generated_title=excluded.generated_title",
            params![project,chat.id,entry,size,modified,index_title,chat.title],
        ).map_err(database_error)?;
        tx.commit().map_err(database_error)?;
        Ok(changed)
    }
}

fn read_titles(path: &Path) -> io::Result<HashMap<String, String>> {
    let mut titles = HashMap::new();
    let file = match fs::File::open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(titles),
        Err(e) => return Err(e),
    };
    for line in BufReader::new(file).lines() {
        if let Ok(row) = serde_json::from_str::<serde_json::Value>(&line?) {
            if let (Some(id), Some(name)) = (row["id"].as_str(), row["thread_name"].as_str()) {
                titles.insert(id.to_owned(), name.to_owned());
            }
        }
    }
    Ok(titles)
}

// Codex uses sessions/YYYY/MM/DD/rollout-*.jsonl. Never follow nested links.
fn rollouts(dir: &Path, depth: usize) -> io::Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    let children = match fs::read_dir(dir) {
        Ok(c) => c,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(files),
        Err(e) => return Err(e),
    };
    for child in children {
        let child = child?;
        let kind = child.file_type()?;
        let name = child.file_name();
        let name = name.to_string_lossy();
        if depth < 3
            && kind.is_dir()
            && name.len() == if depth == 0 { 4 } else { 2 }
            && name.bytes().all(|c| c.is_ascii_digit())
        {
            files.extend(rollouts(&child.path(), depth + 1)?);
        } else if depth == 3
            && kind.is_file()
            && name.starts_with("rollout-")
            && name.ends_with(".jsonl")
        {
            files.push(child.path());
        }
    }
    Ok(files)
}
