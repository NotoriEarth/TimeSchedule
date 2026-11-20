//! Claude Code CLI projects/<project>/<session>.jsonl, excluding subagent trees.
use super::{
    command,
    transcript::{line, ChatText, Header, Transcript},
    Provider,
};
use chrono::{DateTime, Local};
use serde_json::Value;
use std::{
    collections::HashMap,
    fs,
    io::{self, BufRead},
    path::{Path, PathBuf},
};

pub(super) fn files(root: &Path) -> io::Result<Vec<PathBuf>> {
    let dirs = match root.read_dir() {
        Ok(d) => d,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(vec![]),
        Err(e) => return Err(e),
    };
    let mut files = Vec::new();
    for dir in dirs {
        let dir = dir?;
        if !dir.file_type()?.is_dir() {
            continue;
        }
        for child in fs::read_dir(dir.path())? {
            let child = child?;
            let path = child.path();
            if child.file_type()?.is_file()
                && path.extension().is_some_and(|e| e == "jsonl")
                && path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .is_some_and(|s| uuid::Uuid::parse_str(s).is_ok())
            {
                files.push(path);
            }
        }
    }
    Ok(files)
}

fn visible(row: &Value) -> bool {
    matches!(row["type"].as_str(), Some("user" | "assistant"))
        && row["isSidechain"] != true
        && row["isMeta"] != true
        && row["isCompactSummary"] != true
        && row["entrypoint"].as_str().is_none_or(|s| s == "cli")
        && row["message"]["role"] == row["type"]
}
pub(super) fn header(input: &mut impl BufRead) -> Result<Option<Header>, String> {
    let mut buf = Vec::new();
    while line(input, &mut buf)? {
        if buf.is_empty() {
            continue;
        }
        let row: Value = serde_json::from_slice(&buf).map_err(|e| e.to_string())?;
        if !visible(&row) {
            continue;
        }
        let (Some(id), Some(cwd), Some(ts)) = (
            row["sessionId"].as_str(),
            row["cwd"].as_str(),
            row["timestamp"].as_str(),
        ) else {
            continue;
        };
        if uuid::Uuid::parse_str(id).is_err() || !Path::new(cwd).is_absolute() {
            continue;
        }
        let date = DateTime::parse_from_rfc3339(ts)
            .map_err(|e| e.to_string())?
            .with_timezone(&Local)
            .date_naive()
            .to_string();
        return Ok(Some(Header {
            id: id.into(),
            cwd: cwd.into(),
            date,
        }));
    }
    Ok(None)
}

// Keep only the flags needed to interpret the corresponding result, never the shell command.
struct ShellCall {
    reports_exit: bool,
    git_work_tree: bool,
}
impl ShellCall {
    fn new(cmd: &str) -> Self {
        let cmd = cmd.trim().trim_end_matches(';').trim_end();
        let before = cmd.strip_suffix("; echo \"exit=$?\"");
        let probe = before
            .unwrap_or(cmd)
            .rsplit(';')
            .next()
            .unwrap_or("")
            .trim();
        Self {
            reports_exit: before.is_some(),
            git_work_tree: probe == "git rev-parse --is-inside-work-tree",
        }
    }
    fn evidence(&self, block: &Value, result: &Value) -> Option<String> {
        let content = block["content"].as_str()?;
        let last = content.lines().last().unwrap_or("").trim();
        let echoed = self
            .reports_exit
            .then(|| last.strip_prefix("exit=")?.parse::<i32>().ok())
            .flatten();
        let failed = block["is_error"] == true;
        let code = echoed
            .map(i64::from)
            .or_else(|| result["exitCode"].as_i64())
            .or_else(|| {
                if failed {
                    content
                        .lines()
                        .find_map(|l| l.trim().strip_prefix("Exit code ")?.parse::<i64>().ok())
                } else {
                    None
                }
            });
        let mut evidence = command::evidence(content, code, failed, self.git_work_tree)?;
        if echoed.is_some() {
            evidence.push('\n');
            evidence.push_str(last);
        }
        Some(evidence)
    }
}

pub(super) fn read(
    input: &mut impl BufRead,
    header: Header,
    path: &Path,
) -> Result<Option<Transcript>, String> {
    let mut chat = ChatText::default();
    let mut shells = HashMap::new();
    let mut buf = Vec::new();
    let mut n = 0;
    while line(input, &mut buf)? {
        n += 1;
        if buf.is_empty() {
            continue;
        }
        let row: Value = serde_json::from_slice(&buf).map_err(|e| e.to_string())?;
        if !visible(&row) || row["sessionId"].as_str() != Some(&header.id) {
            continue;
        }
        let Some(id) = row["uuid"].as_str() else {
            continue;
        };
        let user = row["type"] == "user";
        let label = if user { "사용자" } else { "AI" };
        let content = &row["message"]["content"];
        // Tool results are user-role messages too. Attribute only human text to the user.
        let human_text = !user
            || (row["toolUseResult"].is_null()
                && row["origin"]["kind"]
                    .as_str()
                    .is_none_or(|kind| kind == "human")
                && row["turnOrigin"]
                    .as_str()
                    .is_none_or(|kind| kind == "human"));
        if let Some(text) = content.as_str() {
            if human_text
                && !text.starts_with("<local-command-")
                && !text.starts_with("<command-name>")
            {
                chat.add(id, label, n, text);
            }
            continue;
        }
        for (i, b) in content.as_array().into_iter().flatten().enumerate() {
            match b["type"].as_str() {
                Some("text") if human_text => {
                    if let Some(text) = b["text"].as_str() {
                        chat.add(&format!("{id}:{i}"), label, n, text);
                    }
                }
                Some("tool_use") if !user && b["name"] == "Bash" => {
                    if let (Some(key), Some(cmd)) =
                        (b["id"].as_str(), b["input"]["command"].as_str())
                    {
                        shells.insert(key.to_owned(), ShellCall::new(cmd));
                    }
                }
                Some("tool_result") if user => {
                    if let Some(key) = b["tool_use_id"].as_str() {
                        if let Some(evidence) = shells
                            .get(key)
                            .and_then(|call| call.evidence(b, &row["toolUseResult"]))
                        {
                            chat.add(&format!("shell:{key}"), "검사 출력", n, &evidence);
                        }
                    }
                }
                _ => {}
            }
        }
    }
    Ok(chat.finish(header, path, "", Provider::Claude))
}
