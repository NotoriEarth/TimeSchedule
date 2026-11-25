//! Finds the session files of both tools that belong to a registered project.
use super::{OriginalsProject, ToolHomes};
use crate::{chat_history::transcript::line, project_records::repo_key};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    fs,
    io::{self, BufReader},
    path::{Path, PathBuf},
};

/// Claude rows before the first one that names its session and folder.
const CLAUDE_HEADER_ROWS: usize = 200;

/// One file to keep, with the session it belongs to.
pub(super) struct Found {
    pub path: PathBuf,
    /// Kept name relative to the project's group folder, `.zst` included.
    pub file: String,
    /// A rollout Codex compressed itself. It is copied as it is, under the name its
    /// plain file had, so the two forms of one rollout are one kept file.
    pub packed: bool,
    pub tool: &'static str,
    pub session: String,
    pub cwd: String,
    pub client: String,
    pub started: String,
    pub project: usize,
}

#[derive(Default)]
pub(super) struct Discovery {
    pub found: Vec<Found>,
    /// Folders and headers that could not be read.
    pub failed: usize,
    /// Projects, by index, some of whose files could not be read: what is found of
    /// them is not all they have.
    pub incomplete: Vec<usize>,
    /// The part of `failed` that is known to be of a project in `incomplete`.
    pub placed: usize,
}

struct Header {
    session: String,
    cwd: String,
    client: String,
    started: String,
}

pub(super) fn discover(projects: &[OriginalsProject], homes: &ToolHomes) -> Discovery {
    let mut out = Discovery::default();
    codex(projects, homes.codex, &mut out);
    let wanted = projects.iter().any(|p| p.capture && p.claude);
    if let Some(home) = homes.claude.filter(|_| wanted) {
        claude(projects, home, &mut out);
    }
    out
}

/// The captured project whose folder is, or contains, `cwd`. The longest folder wins,
/// and when that project is not captured the session is nobody's.
fn owner(projects: &[OriginalsProject], cwd: &str) -> Option<usize> {
    let key = repo_key(cwd);
    projects
        .iter()
        .enumerate()
        .filter(|(_, p)| {
            key == p.root
                || key
                    .strip_prefix(&p.root)
                    .is_some_and(|rest| rest.starts_with('/'))
        })
        .max_by_key(|(_, p)| p.root.len())
        .filter(|(_, p)| p.capture)
        .map(|(i, _)| i)
}

/// The folders and files in `dir`. A missing folder is empty. A link is followed only
/// with `follow_links`, where the caller lists the target and goes no deeper through
/// links. Anywhere else a link counts as failed, so what it hides is not skipped quietly.
fn children(dir: &Path, follow_links: bool, failed: &mut usize) -> Vec<(PathBuf, String, bool)> {
    children_noting(dir, follow_links, failed, &mut Vec::new())
}

/// `children`, and the names counted as failed in `skipped`, so a caller that later
/// learns whose a name is can say that project lost it.
fn children_noting(
    dir: &Path,
    follow_links: bool,
    failed: &mut usize,
    skipped: &mut Vec<String>,
) -> Vec<(PathBuf, String, bool)> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return vec![],
        Err(_) => {
            *failed += 1;
            return vec![];
        }
    };
    let mut out = Vec::new();
    for entry in entries {
        let Ok(entry) = entry else {
            *failed += 1;
            continue;
        };
        let Ok(kind) = entry.file_type() else {
            *failed += 1;
            skipped.push(entry.file_name().to_string_lossy().into_owned());
            continue;
        };
        let is_dir = if kind.is_symlink() {
            match fs::metadata(entry.path()) {
                Ok(target) if follow_links => target.is_dir(),
                _ => {
                    *failed += 1;
                    skipped.push(entry.file_name().to_string_lossy().into_owned());
                    continue;
                }
            }
        } else if kind.is_dir() || kind.is_file() {
            kind.is_dir()
        } else {
            continue;
        };
        out.push((
            entry.path(),
            entry.file_name().to_string_lossy().into_owned(),
            is_dir,
        ));
    }
    out
}

fn is_rollout(name: &str) -> bool {
    name.starts_with("rollout-") && (name.ends_with(".jsonl") || name.ends_with(".jsonl.zst"))
}

/// Files below `dir`, with their path relative to the tool folder. With `bottom_only`
/// only files exactly `depth` folders down count (the date folders of Codex).
fn walk(
    dir: &Path,
    relative: &str,
    depth: usize,
    bottom_only: bool,
    accept: fn(&str) -> bool,
    out: &mut Vec<(PathBuf, String)>,
    failed: &mut usize,
) {
    for (path, name, is_dir) in children(dir, false, failed) {
        if is_dir && depth > 0 {
            let relative = format!("{relative}{name}/");
            walk(
                &path,
                &relative,
                depth - 1,
                bottom_only,
                accept,
                out,
                failed,
            );
        } else if !is_dir && (!bottom_only || depth == 0) && accept(&name) {
            out.push((path, format!("{relative}{name}")));
        }
    }
}

fn codex(projects: &[OriginalsProject], home: &Path, out: &mut Discovery) {
    let mut files = Vec::new();
    let sessions = home.join("sessions");
    walk(
        &sessions,
        "",
        3,
        true,
        is_rollout,
        &mut files,
        &mut out.failed,
    );
    let archived = home.join("archived_sessions");
    let prefix = "archived_sessions/";
    walk(
        &archived,
        prefix,
        0,
        true,
        is_rollout,
        &mut files,
        &mut out.failed,
    );
    // Codex compresses a rollout itself and then removes the plain file. Both forms
    // have one kept name. While the plain file exists it is the complete one, and the
    // compressed one is not opened.
    let mut chosen: BTreeMap<String, (PathBuf, bool)> = BTreeMap::new();
    for (path, relative) in files {
        let packed = relative.ends_with(".zst");
        let file = if packed {
            format!("codex/{relative}")
        } else {
            format!("codex/{relative}.zst")
        };
        if !packed || !chosen.contains_key(&file) {
            chosen.insert(file, (path, packed));
        }
    }
    for (file, (path, packed)) in chosen {
        match codex_header(&path) {
            Ok(Some(h)) => {
                if let Some(project) = owner(projects, &h.cwd) {
                    out.found.push(Found {
                        path,
                        file,
                        packed,
                        tool: "codex",
                        session: h.session,
                        cwd: h.cwd,
                        client: h.client,
                        started: h.started,
                        project,
                    });
                }
            }
            Ok(None) => {}
            Err(_) => out.failed += 1,
        }
    }
}

/// `None` when the file has no complete first line yet.
fn codex_header(path: &Path) -> Result<Option<Header>, String> {
    let file = fs::File::open(path).map_err(|e| e.to_string())?;
    let mut buf = Vec::new();
    let complete = if path.extension().is_some_and(|e| e == "zst") {
        let decoder = zstd::stream::read::Decoder::new(file).map_err(|e| e.to_string())?;
        line(&mut BufReader::new(decoder), &mut buf)?
    } else {
        line(&mut BufReader::new(file), &mut buf)?
    };
    if !complete {
        return Ok(None);
    }
    let meta: Value = serde_json::from_slice(&buf).map_err(|e| e.to_string())?;
    let payload = &meta["payload"];
    let text = |key: &str| payload[key].as_str().unwrap_or("").to_owned();
    let cwd = text("cwd");
    if meta["type"] != "session_meta" || text("id").is_empty() || !Path::new(&cwd).is_absolute() {
        return Err("세션 머리글을 확인할 수 없습니다.".into());
    }
    Ok(Some(Header {
        session: text("id"),
        cwd,
        client: text("originator"),
        started: text("timestamp"),
    }))
}

fn claude(projects: &[OriginalsProject], home: &Path, out: &mut Discovery) {
    // A Claude project folder may be a link, for example one moved to another drive.
    for (dir, dir_name, is_dir) in children(&home.join("projects"), true, &mut out.failed) {
        if !is_dir {
            continue;
        }
        let mut skipped = Vec::new();
        let entries = children_noting(&dir, false, &mut out.failed, &mut skipped);
        for (path, name, is_dir) in &entries {
            let Some(stem) = name.strip_suffix(".jsonl").filter(|_| !is_dir) else {
                continue;
            };
            if uuid::Uuid::parse_str(stem).is_err() {
                continue;
            }
            let header = match claude_header(path) {
                Ok(Some(h)) => h,
                Ok(None) => continue,
                Err(_) => {
                    out.failed += 1;
                    continue;
                }
            };
            let Some(project) = owner(projects, &header.cwd).filter(|i| projects[*i].claude) else {
                continue;
            };
            // Subagent transcripts and tool results sit in a folder named like the session.
            let mut side = Vec::new();
            // Windows names do not tell case apart.
            let named = |n: &str| n.eq_ignore_ascii_case(stem);
            if let Some((folder, folder_name, _)) = entries.iter().find(|(_, n, d)| *d && named(n))
            {
                // Kept under the folder's own name, as the source spells it.
                let relative = format!("{dir_name}/{folder_name}/");
                let all: fn(&str) -> bool = |_| true;
                let before = out.failed;
                walk(
                    folder,
                    &relative,
                    usize::MAX,
                    false,
                    all,
                    &mut side,
                    &mut out.failed,
                );
                if out.failed > before {
                    out.incomplete.push(project);
                    out.placed += out.failed - before;
                }
            } else if skipped.iter().any(|n| named(n)) {
                // The listing counted the session's folder as failed (a link, or a name
                // whose kind could not be read): this project lost its files.
                out.incomplete.push(project);
                out.placed += 1;
            }
            let mut push = |path: PathBuf, relative: String| {
                out.found.push(Found {
                    path,
                    // Every Claude file is compressed, so `out` and `out.zst` in one
                    // folder keep different names.
                    file: format!("claude/{relative}.zst"),
                    packed: false,
                    tool: "claude",
                    session: header.session.clone(),
                    cwd: header.cwd.clone(),
                    client: header.client.clone(),
                    started: header.started.clone(),
                    project,
                });
            };
            push(path.clone(), format!("{dir_name}/{name}"));
            for (path, relative) in side {
                push(path, relative);
            }
        }
    }
}

/// The first row that names both its session and an absolute folder. A file that ends
/// before such a row is too new to place and is left for a later run. One that has
/// none within the first rows is an error, so a change in the tool's format shows.
fn claude_header(path: &Path) -> Result<Option<Header>, String> {
    let mut input = BufReader::new(fs::File::open(path).map_err(|e| e.to_string())?);
    let mut buf = Vec::new();
    for _ in 0..CLAUDE_HEADER_ROWS {
        if !line(&mut input, &mut buf)? {
            return Ok(None);
        }
        let Ok(row) = serde_json::from_slice::<Value>(&buf) else {
            continue;
        };
        let text = |key: &str| row[key].as_str().unwrap_or("").to_owned();
        if !text("sessionId").is_empty() && Path::new(&text("cwd")).is_absolute() {
            return Ok(Some(Header {
                session: text("sessionId"),
                cwd: text("cwd"),
                client: text("entrypoint"),
                started: text("timestamp"),
            }));
        }
    }
    Err("세션 머리글을 확인할 수 없습니다.".into())
}
