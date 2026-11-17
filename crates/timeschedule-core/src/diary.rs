use crate::storage::{
    database_error, record_id, require_changed, timestamp, validate_date, validate_id,
    validate_text, validated_title, Storage,
};
use rusqlite::{params, Row};
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

fn project_row(row: &Row<'_>) -> rusqlite::Result<Project> {
    Ok(Project {
        id: row.get(0)?,
        title: row.get(1)?,
        description: row.get(2)?,
        repo_path: row.get(3)?,
        updated_at: row.get(4)?,
    })
}
pub(crate) fn entry_row(row: &Row<'_>) -> rusqlite::Result<Entry> {
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
pub fn validate_url(url: &str) -> Result<(), String> {
    validate_text(url, "출처 링크", 2000)?;
    if !url.is_empty() {
        let parsed = url::Url::parse(url).map_err(|_| "출처 링크의 주소를 확인해 주세요.")?;
        if !matches!(parsed.scheme(), "http" | "https") || parsed.host_str().is_none() {
            return Err("출처 링크는 http 또는 https 주소로 입력해 주세요.".into());
        }
    }
    Ok(())
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
    /// The records one calendar line stands for, latest change first.
    pub fn project_day_entries(&self, date: &str, project_id: &str) -> Result<Vec<Entry>, String> {
        validate_date(date)?;
        validate_id(project_id)?;
        let c = self.connection()?;
        let mut q=c.prepare("SELECT id,project_id,date,kind,title,body,source_url,source_ref,source_body,updated_at FROM diary_entries WHERE date=?1 AND project_id=?2 ORDER BY updated_at DESC,id LIMIT 50").map_err(database_error)?;
        let rows = q
            .query_map(params![date, project_id], entry_row)
            .map_err(database_error)?
            .collect::<Result<_, _>>()
            .map_err(database_error)?;
        Ok(rows)
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
    git_with_timeout(path, args, std::time::Duration::from_secs(30))
}

fn git_with_timeout(
    path: &Path,
    args: &[&str],
    timeout: std::time::Duration,
) -> Result<String, String> {
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
    let output = run_bounded_command(&mut command, timeout, 8 * 1024 * 1024, 64 * 1024)?;
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

fn run_bounded_command(
    command: &mut Command,
    timeout: std::time::Duration,
    stdout_limit: usize,
    stderr_limit: usize,
) -> Result<std::process::Output, String> {
    use std::{
        process::Stdio,
        time::{Duration, Instant},
    };
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Git 실행 실패. Git 설치와 저장소 경로를 확인해 주세요. {e}"))?;
    let mut stdout = child.stdout.take().ok_or("Git stdout 파이프가 없습니다.")?;
    let mut stderr = child.stderr.take().ok_or("Git stderr 파이프가 없습니다.")?;
    let started = Instant::now();
    let mut out = Vec::new();
    let mut err = Vec::new();
    let result = (|| {
        configure_pipe(&stdout)?;
        configure_pipe(&stderr)?;
        loop {
            read_available(&mut stdout, &mut out, stdout_limit, "표준 출력")?;
            read_available(&mut stderr, &mut err, stderr_limit, "오류 출력")?;
            if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
                // Drain bytes written between the first read and exit. No reader
                // threads are joined: inherited pipe handles cannot stall exit.
                read_available(&mut stdout, &mut out, stdout_limit, "표준 출력")?;
                read_available(&mut stderr, &mut err, stderr_limit, "오류 출력")?;
                return Ok(std::process::Output {
                    status,
                    stdout: out,
                    stderr: err,
                });
            }
            if started.elapsed() >= timeout {
                return Err(format!(
                    "Git 실행 제한 시간({}ms)을 초과했습니다.",
                    timeout.as_millis()
                ));
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    })();
    if result.is_err() {
        match child.try_wait().map_err(|e| e.to_string())? {
            Some(_) => {}
            None => {
                // Terminate only the child we spawned, then reap it. Never kill
                // unrelated Git processes or alter the repository to interrupt it.
                if let Err(kill_error) = child.kill() {
                    if child.try_wait().map_err(|e| e.to_string())?.is_none() {
                        return Err(format!(
                            "{}; Git 종료 실패: {kill_error}",
                            result.unwrap_err()
                        ));
                    }
                }
                child
                    .wait()
                    .map_err(|e| format!("Git 종료 대기 실패: {e}"))?;
            }
        }
    }
    result
}

#[cfg(windows)]
fn configure_pipe<T>(_: &T) -> Result<(), String> {
    Ok(())
}

#[cfg(windows)]
fn read_available<T: std::io::Read + std::os::windows::io::AsRawHandle>(
    pipe: &mut T,
    buffer: &mut Vec<u8>,
    limit: usize,
    label: &str,
) -> Result<(), String> {
    #[link(name = "kernel32")]
    extern "system" {
        fn PeekNamedPipe(
            handle: *mut std::ffi::c_void,
            buffer: *mut std::ffi::c_void,
            size: u32,
            read: *mut u32,
            available: *mut u32,
            left: *mut u32,
        ) -> i32;
    }
    let mut chunk = [0u8; 8192];
    loop {
        let mut available = 0;
        if unsafe {
            PeekNamedPipe(
                pipe.as_raw_handle(),
                std::ptr::null_mut(),
                0,
                std::ptr::null_mut(),
                &mut available,
                std::ptr::null_mut(),
            )
        } == 0
        {
            let error = std::io::Error::last_os_error();
            if matches!(error.raw_os_error(), Some(109) | Some(232)) {
                return Ok(());
            }
            return Err(format!("Git {label} 읽기 실패: {error}"));
        }
        if available == 0 {
            return Ok(());
        }
        if available as usize > limit.saturating_sub(buffer.len()) {
            return Err(format!("Git {label} 제한({limit} bytes)을 초과했습니다."));
        }
        let size = (available as usize).min(chunk.len());
        let count = pipe.read(&mut chunk[..size]).map_err(|e| e.to_string())?;
        if count == 0 {
            return Ok(());
        }
        buffer.extend_from_slice(&chunk[..count]);
    }
}

#[cfg(unix)]
fn configure_pipe<T: std::os::fd::AsRawFd>(pipe: &T) -> Result<(), String> {
    extern "C" {
        fn fcntl(fd: i32, command: i32, ...) -> i32;
    }
    #[cfg(any(
        target_os = "macos",
        target_os = "ios",
        target_os = "freebsd",
        target_os = "openbsd",
        target_os = "netbsd"
    ))]
    const NONBLOCK: i32 = 4;
    #[cfg(not(any(
        target_os = "macos",
        target_os = "ios",
        target_os = "freebsd",
        target_os = "openbsd",
        target_os = "netbsd"
    )))]
    const NONBLOCK: i32 = 2048;
    let flags = unsafe { fcntl(pipe.as_raw_fd(), 3) };
    if flags < 0 || unsafe { fcntl(pipe.as_raw_fd(), 4, flags | NONBLOCK) } < 0 {
        Err(std::io::Error::last_os_error().to_string())
    } else {
        Ok(())
    }
}
#[cfg(unix)]
fn read_available<T: std::io::Read>(
    pipe: &mut T,
    buffer: &mut Vec<u8>,
    limit: usize,
    label: &str,
) -> Result<(), String> {
    let mut chunk = [0u8; 8192];
    loop {
        match pipe.read(&mut chunk) {
            Ok(0) => return Ok(()),
            Ok(count) => {
                if count > limit.saturating_sub(buffer.len()) {
                    return Err(format!("Git {label} 제한({limit} bytes)을 초과했습니다."));
                }
                buffer.extend_from_slice(&chunk[..count]);
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => return Ok(()),
            Err(error) => return Err(error.to_string()),
        }
    }
}
fn read_git(path: &str) -> Result<(String, String, Vec<GitCommit>), String> {
    if path.trim().is_empty() {
        return Err("프로젝트에 로컬 Git 저장소 경로를 먼저 저장해 주세요.".into());
    }
    let path = Path::new(path);
    let started = std::time::Instant::now();
    let remaining = || {
        std::time::Duration::from_secs(30)
            .checked_sub(started.elapsed())
            .ok_or_else(|| "Git 수집 제한 시간(30초)을 초과했습니다.".to_string())
    };
    let root = git(path, &["rev-parse", "--show-toplevel"])?;
    let root = std::fs::canonicalize(root.trim())
        .map_err(|e| e.to_string())?
        .to_string_lossy()
        .into_owned();
    let raw_remote = match git_with_timeout(path, &["remote", "get-url", "origin"], remaining()?) {
        Ok(remote) => remote,
        // Missing origin is ordinary and optional. Timeout, capture overflow,
        // launch/read failures must abort import rather than hide the limit.
        Err(error) if error.starts_with("Git:") => String::new(),
        Err(error) => return Err(error),
    };
    let remote = remote_web_url(raw_remote.trim());
    let log = git_with_timeout(
        path,
        &[
            "-c",
            "i18n.logOutputEncoding=UTF-8",
            "log",
            "--no-show-signature",
            "--max-count=500",
            "--format=%H%x00%cs%x00%B%x00",
        ],
        remaining()?,
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

    fn fixture_command(mode: &str) -> Command {
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--ignored",
                "--exact",
                "diary::tests::bounded_process_fixture",
                "--nocapture",
            ])
            .env("TIMESCHEDULE_PROCESS_FIXTURE", mode);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
        }
        command
    }
    #[test]
    #[ignore = "child process fixture invoked only by bounded-runner tests"]
    fn bounded_process_fixture() {
        use std::io::Write;
        match std::env::var("TIMESCHEDULE_PROCESS_FIXTURE").as_deref() {
            Ok("slow") => std::thread::sleep(std::time::Duration::from_secs(2)),
            Ok("stdout") => {
                std::io::stdout()
                    .write_all(&vec![b'x'; 256 * 1024])
                    .unwrap();
            }
            Ok("stderr") => {
                std::io::stderr()
                    .write_all(&vec![b'x'; 128 * 1024])
                    .unwrap();
            }
            Ok("unicode") => {
                eprintln!("한글 오류 메시지");
                std::process::exit(7);
            }
            _ => panic!("fixture requires explicit mode"),
        }
    }
    #[test]
    fn child_wall_time_is_bounded() {
        let start = std::time::Instant::now();
        let result = run_bounded_command(
            &mut fixture_command("slow"),
            std::time::Duration::from_millis(150),
            1024,
            1024,
        );
        assert!(result.is_err());
        assert!(start.elapsed() < std::time::Duration::from_secs(2));
    }
    #[test]
    fn child_stdout_and_stderr_are_bounded() {
        for mode in ["stdout", "stderr"] {
            assert!(
                run_bounded_command(
                    &mut fixture_command(mode),
                    std::time::Duration::from_secs(3),
                    4096,
                    2048
                )
                .is_err(),
                "{mode} overflow must fail"
            );
        }
    }
    #[test]
    fn unicode_failure_output_remains_valid() {
        let result = run_bounded_command(
            &mut fixture_command("unicode"),
            std::time::Duration::from_secs(3),
            4096,
            4096,
        )
        .unwrap();
        assert!(!result.status.success());
        assert!(String::from_utf8_lossy(&result.stderr).contains("한글 오류 메시지"));
    }
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
