//! Keeps every AI chat session file of a registered project as a zstd copy in the
//! owner's document store, so the originals survive the tools deleting them.
//!
//! This module only works on files. A kept copy is replaced only by a copy proven to
//! continue it (the old content is the start of the new content). Any other change
//! keeps the old copy beside the new one, and nothing here ever deletes a kept file.
//! (The one removal is a set-aside copy made in the same step, when the replace it was
//! made for fails and the kept file is therefore still in place.)
//! `index.jsonl` records what was kept from where, and saves reading unchanged sources
//! again. Losing it loses no content.
use chrono::{SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    io::{self, Read, Write},
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};

mod sources;
use sources::{discover, Found};

const COMPRESSION_LEVEL: i32 = 9;
/// Readable part of a project's folder name. A hash of the whole path follows it.
const GROUP_NAME_CHARS: usize = 60;

/// One registered project with a folder. `root` is `repo_key(repo_path)`.
pub struct OriginalsProject {
    pub id: String,
    pub title: String,
    pub root: String,
    /// Whether its chats are captured. A project that is not captured still owns the
    /// sessions in its folder, so a project above it does not take them.
    pub capture: bool,
    pub claude: bool,
}

pub struct ToolHomes<'a> {
    pub codex: &'a Path,
    pub claude: Option<&'a Path>,
}

#[derive(Debug, Default, Clone, Serialize)]
pub struct OriginalsReport {
    pub archived: usize,
    pub unchanged: usize,
    pub failed: usize,
}

#[derive(Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
struct Entry {
    file: String,
    tool: String,
    session: String,
    cwd: String,
    client: String,
    started: String,
    source: String,
    source_size: u64,
    source_modified: u64,
    archived_at: String,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProjectFile {
    schema_version: u32,
    project_id: String,
    title: String,
    repo_key: String,
}

/// Copies new and changed session files of the captured `projects` into `vault`, an
/// existing document store root whose marker the caller has checked.
pub fn archive(
    projects: &[OriginalsProject],
    homes: &ToolHomes,
    vault: &Path,
) -> Result<OriginalsReport, String> {
    if !vault.is_dir() {
        return Err("문서 저장소 폴더를 찾지 못했습니다.".into());
    }
    let discovery = discover(projects, homes);
    let mut report = OriginalsReport {
        failed: discovery.failed,
        ..Default::default()
    };
    for (index, project) in projects.iter().enumerate() {
        let files: Vec<&Found> = discovery
            .found
            .iter()
            .filter(|found| found.project == index)
            .collect();
        if files.is_empty() {
            continue;
        }
        match archive_project(project, &files, vault) {
            Ok(done) => {
                report.archived += done.archived;
                report.unchanged += done.unchanged;
                report.failed += done.failed;
            }
            Err(_) => report.failed += files.len(),
        }
    }
    Ok(report)
}

/// The project's folder name: the readable start of its path and a hash of all of it,
/// so two paths never share a folder and a long path stays a valid name.
pub fn group_name(root: &str) -> String {
    let readable: String = root
        .chars()
        .take(GROUP_NAME_CHARS)
        .map(|c| {
            if c.is_ascii_lowercase() || c.is_ascii_digit() {
                c
            } else {
                '-'
            }
        })
        .collect();
    // FNV-1a, 64 bit. The name must not change between builds, so no std hasher.
    let hash = root.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3)
    });
    format!("{readable}-{hash:016x}")
}

fn other(message: &str) -> io::Error {
    io::Error::other(message.to_owned())
}

/// Whether something is at `path`. Only "not found" means nothing is: a lookup that
/// fails for another reason is an error, never an empty place to write to.
fn exists(path: &Path) -> io::Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e),
    }
}

fn is_link(path: &Path) -> io::Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(meta) => Ok(meta.file_type().is_symlink()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e),
    }
}

/// Where whole files are written before they are moved into place. The folder is made
/// when the first file is, so a run that changes nothing writes nothing.
struct Temp(PathBuf);
impl Temp {
    fn file(&self) -> io::Result<PathBuf> {
        fs::create_dir_all(&self.0)?;
        Ok(self.0.join(uuid::Uuid::new_v4().to_string()))
    }
}

fn archive_project(
    project: &OriginalsProject,
    files: &[&Found],
    vault: &Path,
) -> Result<OriginalsReport, String> {
    let text = |e: io::Error| e.to_string();
    let originals = vault.join("originals");
    let dir = originals.join(group_name(&project.root));
    let temp = Temp(dir.join(".tmp"));
    for folder in [&originals, &dir, &temp.0] {
        if is_link(folder).map_err(text)? {
            return Err("링크 폴더에는 보관하지 않습니다.".into());
        }
    }
    fs::create_dir_all(&dir).map_err(text)?;
    // One process at a time per project folder. The lock ends with the process.
    let lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(dir.join(".lock"))
        .map_err(text)?;
    match lock.try_lock() {
        Ok(()) => {}
        // Another process is keeping this project right now.
        Err(fs::TryLockError::WouldBlock) => return Ok(OriginalsReport::default()),
        Err(fs::TryLockError::Error(e)) => return Err(e.to_string()),
    }
    let claimed = claimed(&dir, project)?;
    // Unfinished copies of a run that was killed. Only this module writes there, and
    // the folder is this project's.
    if exists(&temp.0).map_err(text)? {
        fs::remove_dir_all(&temp.0).map_err(text)?;
    }
    if !claimed {
        let file = ProjectFile {
            schema_version: 1,
            project_id: project.id.clone(),
            title: project.title.clone(),
            repo_key: project.root.clone(),
        };
        write_new(&dir.join("project.json"), &temp, |out| {
            serde_json::to_writer_pretty(out, &file).map_err(io::Error::other)
        })
        .map_err(text)?;
    }
    let index_path = dir.join("index.jsonl");
    let mut index = read_index(&index_path)?;
    let mut report = OriginalsReport::default();
    let mut changed = false;
    for found in files {
        match keep(found, &dir, &temp, &mut index) {
            Ok(true) => {
                report.archived += 1;
                changed = true;
            }
            Ok(false) => report.unchanged += 1,
            Err(_) => report.failed += 1,
        }
    }
    if changed && save_index(&index_path, &temp, &index).is_err() {
        // The copies exist but nothing records them: the next run checks them again.
        report.failed += report.archived;
        report.archived = 0;
    }
    let _ = fs::remove_dir(&temp.0);
    Ok(report)
}

/// Whether the folder already says it is this project's. A folder of another
/// `repoKey` is an error and is left as it is.
fn claimed(dir: &Path, project: &OriginalsProject) -> Result<bool, String> {
    match fs::read(dir.join("project.json")) {
        Ok(bytes) => {
            let existing: ProjectFile =
                serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
            if existing.repo_key != project.root {
                return Err("다른 프로젝트의 보관 폴더입니다.".into());
            }
            Ok(true)
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e.to_string()),
    }
}

fn read_index(path: &Path) -> Result<BTreeMap<String, Entry>, String> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(BTreeMap::new()),
        Err(e) => return Err(e.to_string()),
    };
    let mut index = BTreeMap::new();
    for row in text.lines().filter(|l| !l.trim().is_empty()) {
        // A damaged index is left alone for the owner to look at.
        let entry: Entry = serde_json::from_str(row).map_err(|e| e.to_string())?;
        index.insert(entry.file.clone(), entry);
    }
    Ok(index)
}

fn save_index(path: &Path, temp: &Temp, index: &BTreeMap<String, Entry>) -> io::Result<()> {
    write_new(path, temp, |out| {
        for entry in index.values() {
            serde_json::to_writer(&mut *out, entry).map_err(io::Error::other)?;
            out.write_all(b"\n")?;
        }
        Ok(())
    })
}

/// Writes a whole file in `temp`, then renames it to `path`, so a killed process
/// leaves no half file at `path`.
fn write_new(
    path: &Path,
    temp: &Temp,
    write: impl FnOnce(&mut fs::File) -> io::Result<()>,
) -> io::Result<()> {
    let temp = temp.file()?;
    let result = (|| {
        let mut file = fs::File::create_new(&temp)?;
        write(&mut file)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temp, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

/// The kept path of `file` under `dir`, with its folders made. No step may be a link.
fn place(dir: &Path, file: &str) -> io::Result<PathBuf> {
    let mut path = dir.to_path_buf();
    let mut parts = file.split('/').peekable();
    while let Some(part) = parts.next() {
        path.push(part);
        if is_link(&path)? {
            return Err(other("링크 경로에는 보관하지 않습니다."));
        }
        if parts.peek().is_some() && !exists(&path)? {
            fs::create_dir(&path)?;
        }
    }
    Ok(path)
}

fn read_full(input: &mut impl Read, buf: &mut [u8]) -> io::Result<usize> {
    let mut filled = 0;
    while filled < buf.len() {
        match input.read(&mut buf[filled..])? {
            0 => break,
            n => filled += n,
        }
    }
    Ok(filled)
}

/// Decodes `new` to its end, which proves it is a whole zstd file, and tells whether the
/// content of `old` is the start of it. An `old` that cannot be decoded is not.
fn continues(new: &Path, old: Option<&Path>) -> io::Result<bool> {
    let mut new = zstd::stream::read::Decoder::new(fs::File::open(new)?)?;
    let mut old = match old {
        Some(path) => Some(zstd::stream::read::Decoder::new(fs::File::open(path)?)?),
        None => None,
    };
    let mut same = true;
    let (mut a, mut b) = (vec![0u8; 1 << 16], vec![0u8; 1 << 16]);
    loop {
        let n = read_full(&mut new, &mut a)?;
        if n == 0 {
            break;
        }
        if let Some(input) = old.as_mut().filter(|_| same) {
            match read_full(input, &mut b[..n]) {
                Ok(m) if a[..m] == b[..m] => {
                    if m < n {
                        old = None;
                    }
                }
                _ => same = false,
            }
        }
    }
    // The new content ended. The old one must have ended too.
    if let Some(input) = old.as_mut().filter(|_| same) {
        same = matches!(input.read(&mut b[..1]), Ok(0));
    }
    Ok(same)
}

/// A name beside `target` that nothing has yet.
fn previous_name(target: &Path) -> io::Result<PathBuf> {
    let stamp = Utc::now().format("%Y%m%dT%H%M%SZ");
    for n in 1u32.. {
        let mut name = target.as_os_str().to_owned();
        name.push(if n == 1 {
            format!(".prev-{stamp}")
        } else {
            format!(".prev-{stamp}-{n}")
        });
        let path = PathBuf::from(name);
        if !exists(&path)? {
            return Ok(path);
        }
    }
    Err(other("이전 보관본의 이름을 정하지 못했습니다."))
}

/// Copies the kept `target` to a new name beside it and returns that name. `target`
/// itself stays where it is, so a run killed here leaves the kept copy in place, and
/// the copy appears under its name only once it is whole.
fn set_aside(target: &Path, temp: &Temp) -> io::Result<PathBuf> {
    let previous = previous_name(target)?;
    let staged = temp.file()?;
    let result = (|| {
        let mut to = fs::File::create_new(&staged)?;
        io::copy(&mut fs::File::open(target)?, &mut to)?;
        to.sync_all()?;
        drop(to);
        fs::rename(&staged, &previous)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&staged);
    }
    result.map(|()| previous)
}

/// `Ok(true)` when the file was copied, `Ok(false)` when the kept copy is current.
fn keep(
    found: &Found,
    dir: &Path,
    temp: &Temp,
    index: &mut BTreeMap<String, Entry>,
) -> io::Result<bool> {
    // Size and time come from the opened file, before it is read. Only that many bytes
    // are read: what a tool appends meanwhile is kept by the next run.
    let input = fs::File::open(&found.path)?;
    let meta = input.metadata()?;
    let size = meta.len();
    let modified = meta
        .modified()?
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos() as u64);
    let mut input = input.take(size);
    let source = found.path.to_string_lossy().into_owned();
    let target = place(dir, &found.file)?;
    let kept = exists(&target)?;
    let current = index.get(&found.file).is_some_and(|e| {
        e.source == source && e.source_size == size && e.source_modified == modified
    });
    if current && kept {
        return Ok(false);
    }
    let copy = temp.file()?;
    let result = (|| {
        let mut out = fs::File::create_new(&copy)?;
        if found.packed {
            io::copy(&mut input, &mut out)?;
        } else {
            zstd::stream::copy_encode(&mut input, &mut out, COMPRESSION_LEVEL)?;
        }
        if input.limit() != 0 {
            return Err(other("원본이 읽는 도중 짧아졌습니다."));
        }
        out.sync_all()?;
        drop(out);
        // Session files only grow. A copy that does not continue the kept one means the
        // tool rewrote the file, or it was read mid-write: both copies are kept.
        let aside = if continues(&copy, kept.then_some(target.as_path()))? {
            None
        } else {
            Some(set_aside(&target, temp)?)
        };
        fs::rename(&copy, &target).inspect_err(|_| {
            // The kept file was not replaced, so the copy just made of it is a duplicate.
            if let Some(aside) = &aside {
                let _ = fs::remove_file(aside);
            }
        })
    })();
    if result.is_err() {
        let _ = fs::remove_file(&copy);
    }
    result?;
    index.insert(
        found.file.clone(),
        Entry {
            file: found.file.clone(),
            tool: found.tool.into(),
            session: found.session.clone(),
            cwd: found.cwd.clone(),
            client: found.client.clone(),
            started: found.started.clone(),
            source,
            source_size: size,
            source_modified: modified,
            archived_at: Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true),
        },
    );
    Ok(true)
}
