use crate::storage::{
    database_error, record_id, require_changed, timestamp, validate_date, validate_id,
    validate_text, validated_title, Storage,
};
use rusqlite::{params, Connection, Row};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::process::Command;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub id: String,
    pub title: String,
    pub description: String,
    pub repo_path: String,
    pub updated_at: String,
}
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectInput {
    pub id: Option<String>,
    pub title: String,
    pub description: String,
    pub repo_path: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub id: String,
    pub project_id: Option<String>,
    pub date: String,
    pub kind: String,
    pub title: String,
    pub body: String,
    pub source_url: String,
    pub source_ref: Option<String>,
    pub source_body: String,
    pub updated_at: String,
}
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EntryInput {
    pub id: Option<String>,
    pub project_id: Option<String>,
    pub date: String,
    pub kind: String,
    pub title: String,
    pub body: String,
    pub source_url: String,
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EntrySummary {
    pub id: String,
    pub project_id: Option<String>,
    pub date: String,
    pub kind: String,
    pub title: String,
    pub updated_at: String,
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchHit {
    pub id: String,
    pub kind: String,
    pub title: String,
    pub excerpt: String,
    pub date: Option<String>,
    pub project_id: Option<String>,
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportResult {
    pub imported: usize,
    pub scanned: usize,
    pub limited: bool,
}
#[derive(Debug)]
pub struct GitCommit {
    hash: String,
    date: String,
    message: String,
}

/// Trigrams index literal Korean substrings of three or more characters. Short queries
/// fall back to a bounded-result scan; neither path returns full bodies to the WebView.
pub(crate) fn create_search_index(connection: &Connection) -> Result<(), String> {
    connection.execute_batch("CREATE VIRTUAL TABLE diary_search USING fts5(kind UNINDEXED, id UNINDEXED, title, body, date UNINDEXED, project_id UNINDEXED, updated_at UNINDEXED, tokenize='trigram');").map_err(database_error)?;
    for (offset, (table, kind, body, date, project)) in [
        ("events", "event", "notes", "date", "NULL"),
        ("tasks", "task", "''", "date", "NULL"),
        ("notes", "note", "body", "NULL", "NULL"),
        ("projects", "project", "description", "NULL", "id"),
        ("diary_entries", "entry", "body", "date", "project_id"),
    ]
    .into_iter()
    .enumerate()
    {
        let expr = |field: &str| {
            if field == "NULL" || field == "''" {
                field.to_string()
            } else {
                format!("new.{field}")
            }
        };
        let values = format!(
            "new.rowid*5+{offset}, '{kind}', new.id, new.title, {}, {}, {}, new.updated_at",
            expr(body),
            expr(date),
            expr(project)
        );
        connection.execute_batch(&format!(
            "INSERT INTO diary_search(rowid,kind,id,title,body,date,project_id,updated_at) SELECT rowid*5+{offset},'{kind}',id,title,{body},{date},{project},updated_at FROM {table};
             CREATE TRIGGER {table}_search_insert AFTER INSERT ON {table} BEGIN INSERT INTO diary_search(rowid,kind,id,title,body,date,project_id,updated_at) VALUES ({values}); END;
             CREATE TRIGGER {table}_search_delete AFTER DELETE ON {table} BEGIN DELETE FROM diary_search WHERE rowid=old.rowid*5+{offset}; END;
             CREATE TRIGGER {table}_search_update AFTER UPDATE ON {table} BEGIN DELETE FROM diary_search WHERE rowid=old.rowid*5+{offset}; INSERT INTO diary_search(rowid,kind,id,title,body,date,project_id,updated_at) VALUES ({values}); END;"
        )).map_err(database_error)?;
    }
    Ok(())
}

fn project_row(row: &Row<'_>) -> rusqlite::Result<Project> {
    Ok(Project {
        id: row.get(0)?,
        title: row.get(1)?,
        description: row.get(2)?,
        repo_path: row.get(3)?,
        updated_at: row.get(4)?,
    })
}
fn entry_row(row: &Row<'_>) -> rusqlite::Result<Entry> {
    Ok(Entry {
        id: row.get(0)?,
        project_id: row.get(1)?,
        date: row.get(2)?,
        kind: row.get(3)?,
        title: row.get(4)?,
        body: row.get(5)?,
        source_url: row.get(6)?,
        source_ref: row.get(7)?,
        source_body: row.get(8)?,
        updated_at: row.get(9)?,
    })
}
fn range(from: &str, to: &str) -> Result<(), String> {
    validate_date(from)?;
    validate_date(to)?;
    if from > to {
        return Err("시작 날짜가 종료 날짜보다 늦습니다.".into());
    }
    Ok(())
}
fn validate_url(url: &str) -> Result<(), String> {
    validate_text(url, "출처 링크", 2000)?;
    if !url.is_empty() {
        let parsed = tauri::Url::parse(url).map_err(|_| "출처 링크의 주소를 확인해 주세요.")?;
        if !matches!(parsed.scheme(), "http" | "https") || parsed.host_str().is_none() {
            return Err("출처 링크는 http 또는 https 주소로 입력해 주세요.".into());
        }
    }
    Ok(())
}

pub fn open_source_url(url: &str) -> Result<(), String> {
    validate_url(url)?;
    if url.is_empty() {
        return Err("출처 링크가 없습니다.".into());
    }
    #[cfg(windows)]
    {
        use std::ptr::null;
        #[link(name = "shell32")]
        extern "system" {
            fn ShellExecuteW(
                hwnd: isize,
                operation: *const u16,
                file: *const u16,
                parameters: *const u16,
                directory: *const u16,
                show: i32,
            ) -> isize;
        }
        let operation: Vec<u16> = "open\0".encode_utf16().collect();
        let address: Vec<u16> = url.encode_utf16().chain(Some(0)).collect();
        // Only a validated web URL reaches the OS's default browser, never a shell command.
        let result =
            unsafe { ShellExecuteW(0, operation.as_ptr(), address.as_ptr(), null(), null(), 1) };
        if result <= 32 {
            return Err(format!("기본 브라우저를 열지 못했습니다 ({result})."));
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        Err("이 데스크톱 빌드에서는 외부 링크 열기를 지원하지 않습니다.".into())
    }
}

impl Storage {
    pub fn list_projects(&self) -> Result<Vec<Project>, String> {
        let c = self.connection()?;
        let mut q = c.prepare("SELECT id,title,description,repo_path,updated_at FROM projects ORDER BY updated_at DESC,id").map_err(database_error)?;
        let rows = q
            .query_map([], project_row)
            .map_err(database_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(database_error)?;
        Ok(rows)
    }
    pub fn save_project(&self, input: ProjectInput) -> Result<Project, String> {
        let title = validated_title(&input.title)?;
        validate_text(&input.description, "프로젝트 설명", 20000)?;
        validate_text(&input.repo_path, "저장소 경로", 2000)?;
        let updating = input.id.is_some();
        let p = Project {
            id: record_id(input.id)?,
            title,
            description: input.description,
            repo_path: input.repo_path.trim().into(),
            updated_at: timestamp(),
        };
        let c = self.connection()?;
        let changed = c.execute(if updating { "UPDATE projects SET title=?2,description=?3,repo_path=?4,updated_at=?5 WHERE id=?1" } else { "INSERT INTO projects VALUES (?1,?2,?3,?4,?5)" }, params![p.id,p.title,p.description,p.repo_path,p.updated_at]).map_err(database_error)?;
        require_changed(changed, "프로젝트")?;
        Ok(p)
    }
    pub fn delete_project(&self, id: &str) -> Result<(), String> {
        self.delete("DELETE FROM projects WHERE id=?1", "프로젝트", id)
    }
    pub fn list_entries(
        &self,
        from: &str,
        to: &str,
        project_id: Option<&str>,
        offset: u32,
    ) -> Result<Vec<EntrySummary>, String> {
        range(from, to)?;
        if let Some(id) = project_id {
            validate_id(id)?;
        }
        let c = self.connection()?;
        let mut q=c.prepare("SELECT id,project_id,date,kind,title,updated_at FROM diary_entries WHERE date>=?1 AND date<=?2 AND (?3 IS NULL OR project_id=?3) ORDER BY date DESC,updated_at DESC,id LIMIT 200 OFFSET ?4").map_err(database_error)?;
        let rows = q
            .query_map(params![from, to, project_id, offset], |r| {
                Ok(EntrySummary {
                    id: r.get(0)?,
                    project_id: r.get(1)?,
                    date: r.get(2)?,
                    kind: r.get(3)?,
                    title: r.get(4)?,
                    updated_at: r.get(5)?,
                })
            })
            .map_err(database_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(database_error)?;
        Ok(rows)
    }
    pub fn get_entry(&self, id: &str) -> Result<Entry, String> {
        validate_id(id)?;
        self.connection()?.query_row("SELECT id,project_id,date,kind,title,body,source_url,source_ref,source_body,updated_at FROM diary_entries WHERE id=?1", [id], entry_row).map_err(database_error)
    }
    pub fn save_entry(&self, input: EntryInput) -> Result<Entry, String> {
        let title = validated_title(&input.title)?;
        validate_date(&input.date)?;
        validate_text(&input.body, "기록 본문", 100000)?;
        validate_url(&input.source_url)?;
        if ![
            "note",
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
        let updating = input.id.is_some();
        let id = record_id(input.id)?;
        {
            let c = self.connection()?;
            let changed=c.execute(if updating {
                "UPDATE diary_entries SET project_id=?2,date=?3,kind=?4,title=?5,body=?6,source_url=?7,updated_at=?8 WHERE id=?1"
            } else { "INSERT INTO diary_entries(id,project_id,date,kind,title,body,source_url,updated_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)" }, params![id,input.project_id,input.date,input.kind,title,input.body,input.source_url,timestamp()]).map_err(database_error)?;
            require_changed(changed, "기록")?;
        }
        self.get_entry(&id)
    }
    pub fn delete_entry(&self, id: &str) -> Result<(), String> {
        self.delete("DELETE FROM diary_entries WHERE id=?1", "기록", id)
    }
    pub fn search_diary(&self, query: &str, limit: u32) -> Result<Vec<SearchHit>, String> {
        let query = query.trim();
        if query.is_empty() {
            return Ok(vec![]);
        }
        validate_text(query, "검색어", 200)?;
        let indexed = query.chars().count() >= 3;
        let needle = if indexed {
            format!("\"{}\"", query.replace('"', "\"\""))
        } else {
            query.to_string()
        };
        let c = self.connection()?;
        let sql=format!("SELECT id,kind,title,substr(body,1,160),date,project_id FROM diary_search WHERE {} ORDER BY updated_at DESC,id LIMIT ?2", if indexed { "diary_search MATCH ?1" } else { "instr(lower(title),lower(?1))>0 OR instr(lower(body),lower(?1))>0" });
        let mut q = c.prepare(&sql).map_err(database_error)?;
        let rows = q
            .query_map(params![needle, limit.clamp(1, 50)], |r| {
                Ok(SearchHit {
                    id: r.get(0)?,
                    kind: r.get(1)?,
                    title: r.get(2)?,
                    excerpt: r.get(3)?,
                    date: r.get(4)?,
                    project_id: r.get(5)?,
                })
            })
            .map_err(database_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(database_error)?;
        Ok(rows)
    }
    pub fn import_git(&self, project_id: &str) -> Result<ImportResult, String> {
        validate_id(project_id)?;
        let path: String = self
            .connection()?
            .query_row(
                "SELECT repo_path FROM projects WHERE id=?1",
                [project_id],
                |r| r.get(0),
            )
            .map_err(database_error)?;
        let (root, remote, commits) = read_git(&path)?;
        let imported = self.store_commits(project_id, &root, &remote, &commits)?;
        Ok(ImportResult {
            imported,
            scanned: commits.len(),
            limited: commits.len() == 500,
        })
    }
    fn store_commits(
        &self,
        project_id: &str,
        root: &str,
        remote: &str,
        commits: &[GitCommit],
    ) -> Result<usize, String> {
        let mut c = self.connection()?;
        let tx = c.transaction().map_err(database_error)?;
        let mut added = 0;
        // INSERT OR IGNORE only suppresses duplicates; all user edits and immutable source text survive.
        for commit in commits {
            validate_date(&commit.date)?;
            validate_text(&commit.message, "Git 원본 커밋 메시지", 100000)?;
            let title: String = commit
                .message
                .lines()
                .next()
                .unwrap_or("Git 커밋")
                .chars()
                .take(200)
                .collect();
            let body: String = commit.message.chars().take(100000).collect();
            let source_ref = format!("{root}#{}", commit.hash);
            let url = if remote.is_empty() {
                String::new()
            } else {
                format!("{remote}/commit/{}", commit.hash)
            };
            added+=tx.execute("INSERT INTO diary_entries(id,project_id,date,kind,title,body,source_url,source_ref,source_body,updated_at) VALUES(?1,?2,?3,'commit',?4,?5,?6,?7,?5,?8) ON CONFLICT(source_ref) DO NOTHING",params![record_id(None)?,project_id,commit.date,title,body,url,source_ref,timestamp()]).map_err(database_error)?;
        }
        tx.commit().map_err(database_error)?;
        Ok(added)
    }
}

fn git(path: &Path, args: &[&str]) -> Result<String, String> {
    let mut command = Command::new("git");
    command
        .arg("--no-pager")
        .arg("--no-replace-objects")
        .arg("-C")
        .arg(path)
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let output = command
        .output()
        .map_err(|e| format!("Git 실행 실패. Git 설치와 저장소 경로를 확인해 주세요. {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "Git: {}",
            String::from_utf8_lossy(&output.stderr)
                .chars()
                .take(2000)
                .collect::<String>()
        ));
    }
    if output.stdout.len() > 8 * 1024 * 1024 {
        return Err("Git 출력이 8MB를 넘습니다. 저장소를 확인해 주세요.".into());
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}
fn read_git(path: &str) -> Result<(String, String, Vec<GitCommit>), String> {
    if path.trim().is_empty() {
        return Err("프로젝트에 로컬 Git 저장소 경로를 먼저 저장해 주세요.".into());
    }
    let path = Path::new(path);
    let root = git(path, &["rev-parse", "--show-toplevel"])?;
    let root = std::fs::canonicalize(root.trim())
        .map_err(|e| e.to_string())?
        .to_string_lossy()
        .into_owned();
    let raw_remote = git(path, &["remote", "get-url", "origin"]).unwrap_or_default();
    let remote = remote_web_url(raw_remote.trim());
    let log = git(
        path,
        &[
            "-c",
            "i18n.logOutputEncoding=UTF-8",
            "log",
            "--no-show-signature",
            "--max-count=500",
            "--format=%H%x00%cs%x00%B%x00",
        ],
    )?;
    let fields: Vec<_> = log.split('\0').collect();
    let mut commits = vec![];
    for fields in fields.chunks(3) {
        if fields.len() < 3 {
            continue;
        }
        let hash = fields[0].trim();
        if !(hash.len() == 40 || hash.len() == 64) || !hash.bytes().all(|c| c.is_ascii_hexdigit()) {
            return Err("Git 커밋 정보를 해석하지 못했습니다.".into());
        }
        commits.push(GitCommit {
            hash: hash.into(),
            date: fields[1].into(),
            message: fields[2].into(),
        });
    }
    Ok((root, remote, commits))
}
fn remote_web_url(remote: &str) -> String {
    let remote = remote.trim_end_matches('/').trim_end_matches(".git");
    let url = if let Some(rest) = remote.strip_prefix("git@") {
        rest.split_once(':')
            .map(|(host, path)| format!("https://{host}/{path}"))
            .unwrap_or_default()
    } else {
        remote.to_string()
    };
    // Do not expose embedded credentials or local paths as clickable links.
    if (url.starts_with("https://") || url.starts_with("http://")) && !url.contains('@') {
        url
    } else {
        String::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::{NoteInput, Storage};
    use uuid::Uuid;

    fn project() -> ProjectInput {
        ProjectInput {
            id: None,
            title: "만년 다이어리".into(),
            description: "한국어 프로젝트".into(),
            repo_path: String::new(),
        }
    }
    fn entry(project_id: Option<String>) -> EntryInput {
        EntryInput {
            id: None,
            project_id,
            date: "2026-12-31".into(),
            kind: "troubleshooting".into(),
            title: "검색 수정".into(),
            body: "한국어 본문 100% _ 해결\n다음 줄".into(),
            source_url: String::new(),
        }
    }
    #[test]
    fn diary_persists_and_rejected_edit_keeps_original() {
        let path = std::env::temp_dir().join(format!("diary-{}.db", Uuid::new_v4()));
        let storage = Storage::open(&path).unwrap();
        let p = storage.save_project(project()).unwrap();
        let original = storage.save_entry(entry(Some(p.id.clone()))).unwrap();
        let mut invalid = entry(Some(p.id.clone()));
        invalid.id = Some(original.id.clone());
        invalid.date = "2026-02-30".into();
        assert!(storage.save_entry(invalid).is_err());
        drop(storage);
        let storage = Storage::open(&path).unwrap();
        assert_eq!(storage.get_entry(&original.id).unwrap(), original);
        assert_eq!(storage.list_projects().unwrap()[0].title, p.title);
        storage.delete_project(&p.id).unwrap();
        assert_eq!(storage.get_entry(&original.id).unwrap().project_id, None);
        storage.delete_entry(&original.id).unwrap();
        assert!(storage.get_entry(&original.id).is_err());
        drop(storage);
        let _ = std::fs::remove_file(path);
    }
    #[test]
    fn search_is_literal_korean_bounded_and_returns_navigation() {
        let storage = Storage::open(":memory:").unwrap();
        let p = storage.save_project(project()).unwrap();
        let e = storage.save_entry(entry(Some(p.id.clone()))).unwrap();
        storage
            .save_note(NoteInput {
                id: None,
                title: "옛 메모".into(),
                body: "한국어 이전 자료".into(),
            })
            .unwrap();
        assert_eq!(storage.search_diary("한국어", 1000).unwrap().len(), 3);
        for query in ["100%", "_"] {
            let results = storage.search_diary(query, 10).unwrap();
            assert_eq!(results.len(), 1);
            assert_eq!(results[0].id, e.id);
        }
        assert!(storage.search_diary("  ", 10).unwrap().is_empty());
        assert_eq!(storage.search_diary("한국어", 1).unwrap().len(), 1);
        let summaries = storage
            .list_entries("2026-12-01", "2027-01-01", None, 0)
            .unwrap();
        let json = serde_json::to_value(&summaries).unwrap();
        assert!(json[0].get("body").is_none());
    }
    #[test]
    fn import_is_idempotent_preserves_user_edits_and_source() {
        let storage = Storage::open(":memory:").unwrap();
        let p = storage.save_project(project()).unwrap();
        let commits = vec![GitCommit {
            hash: "a".repeat(40),
            date: "2026-12-31".into(),
            message: "원본 커밋\n본문".into(),
        }];
        assert_eq!(
            storage
                .store_commits(&p.id, "C:/repo with spaces", "", &commits)
                .unwrap(),
            1
        );
        let list = storage
            .list_entries("2026-12-01", "2027-01-01", Some(&p.id), 0)
            .unwrap();
        let original = storage.get_entry(&list[0].id).unwrap();
        let mut edit = entry(Some(p.id.clone()));
        edit.id = Some(original.id.clone());
        edit.body = "내 해석".into();
        edit.kind = "commit".into();
        storage.save_entry(edit).unwrap();
        assert_eq!(
            storage
                .store_commits(&p.id, "C:/repo with spaces", "", &commits)
                .unwrap(),
            0
        );
        let saved = storage.get_entry(&original.id).unwrap();
        assert_eq!(saved.body, "내 해석");
        assert_eq!(saved.source_body, "원본 커밋\n본문");
    }
    #[test]
    fn git_reads_real_repository_with_spaces_and_rejects_non_repo() {
        let path = std::env::temp_dir().join(format!("diary git {}", Uuid::new_v4()));
        std::fs::create_dir_all(&path).unwrap();
        assert!(read_git(path.to_str().unwrap()).is_err());
        for args in [
            vec!["init"],
            vec![
                "-c",
                "user.name=Diary Test",
                "-c",
                "user.email=test@example.invalid",
                "commit",
                "--allow-empty",
                "-m",
                "한국어 커밋",
            ],
        ] {
            assert!(git(&path, &args).is_ok());
        }
        let (_, remote, commits) = read_git(path.to_str().unwrap()).unwrap();
        assert_eq!(remote, "");
        assert_eq!(commits.len(), 1);
        assert!(commits[0].message.contains("한국어"));
        assert!(path.starts_with(std::env::temp_dir()));
        std::fs::remove_dir_all(path).unwrap();
    }

    #[test]
    fn moving_imported_entry_does_not_change_import_identity() {
        let storage = Storage::open(":memory:").unwrap();
        let p = storage.save_project(project()).unwrap();
        let commits = vec![GitCommit {
            hash: "b".repeat(40),
            date: "2026-12-31".into(),
            message: "원문\n".into(),
        }];
        storage.store_commits(&p.id, "repo", "", &commits).unwrap();
        let id = storage
            .list_entries("2026-12-31", "2026-12-31", None, 0)
            .unwrap()[0]
            .id
            .clone();
        let mut edited = entry(None);
        edited.id = Some(id.clone());
        edited.body = "분류를 바꾼 필기".into();
        storage.save_entry(edited).unwrap();
        assert_eq!(
            storage.store_commits(&p.id, "repo", "", &commits).unwrap(),
            0
        );
        assert_eq!(storage.get_entry(&id).unwrap().source_body, "원문\n");
        assert_eq!(storage.get_entry(&id).unwrap().body, "분류를 바꾼 필기");
        assert_eq!(
            storage
                .list_entries("2026-12-31", "2026-12-31", None, 0)
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn oversized_commit_rolls_back_batch_without_silent_source_truncation() {
        let storage = Storage::open(":memory:").unwrap();
        let p = storage.save_project(project()).unwrap();
        let commits = vec![
            GitCommit {
                hash: "a".repeat(40),
                date: "2026-12-31".into(),
                message: "정상 메시지".into(),
            },
            GitCommit {
                hash: "b".repeat(40),
                date: "2026-12-31".into(),
                message: "가".repeat(100001),
            },
        ];
        assert!(storage.store_commits(&p.id, "repo", "", &commits).is_err());
        assert!(storage
            .list_entries("2026-12-31", "2026-12-31", None, 0)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn search_index_tracks_edits_deletes_and_limits_large_matches() {
        let storage = Storage::open(":memory:").unwrap();
        for _ in 0..65 {
            storage.save_entry(entry(None)).unwrap();
        }
        assert_eq!(storage.search_diary("한국어", 10000).unwrap().len(), 50);
        let mut input = entry(None);
        input.title = "수정전제목 a_b 100%".into();
        let saved = storage.save_entry(input.clone()).unwrap();
        assert_eq!(storage.search_diary("a_b", 50).unwrap().len(), 1);
        input.id = Some(saved.id.clone());
        input.title = "수정후제목".into();
        input.body = "새로운본문".into();
        storage.save_entry(input.clone()).unwrap();
        assert!(storage.search_diary("수정전제목", 50).unwrap().is_empty());
        assert_eq!(storage.search_diary("수정후제목", 50).unwrap().len(), 1);
        input.source_url = "javascript:alert(1)".into();
        assert!(storage.save_entry(input).is_err());
        storage.delete_entry(&saved.id).unwrap();
        assert!(storage.search_diary("수정후제목", 50).unwrap().is_empty());
    }

    #[test]
    fn v1_migration_preserves_old_records_and_indexes_them() {
        let path = std::env::temp_dir().join(format!("diary-v1-{}.db", Uuid::new_v4()));
        let connection = rusqlite::Connection::open(&path).unwrap();
        connection.execute_batch("CREATE TABLE events (id TEXT PRIMARY KEY,title TEXT,date TEXT,start_time TEXT,end_time TEXT,all_day INTEGER,notes TEXT,color TEXT,completed INTEGER,updated_at TEXT);
            CREATE TABLE tasks (id TEXT PRIMARY KEY,title TEXT,date TEXT,completed INTEGER,updated_at TEXT);
            CREATE TABLE notes (id TEXT PRIMARY KEY,title TEXT,body TEXT,updated_at TEXT);
            INSERT INTO notes VALUES ('12345678-1234-1234-1234-123456789abc','기존메모','한국어 원문 100% _','2026-10-29'); PRAGMA user_version=1;").unwrap();
        drop(connection);
        let storage = Storage::open(&path).unwrap();
        assert_eq!(storage.list_notes().unwrap()[0].body, "한국어 원문 100% _");
        assert_eq!(storage.search_diary("한국어", 50).unwrap()[0].kind, "note");
        storage.save_project(project()).unwrap();
        drop(storage);
        let reopened = Storage::open(&path).unwrap();
        assert_eq!(reopened.list_projects().unwrap().len(), 1);
        assert_eq!(reopened.list_notes().unwrap().len(), 1);
        drop(reopened);
        let _ = std::fs::remove_file(path);
    }
}
