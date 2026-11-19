use chrono::{DateTime, Local};
use serde_json::Value;
use std::{
    collections::{HashSet, VecDeque},
    io::{BufRead, Read},
    path::Path,
};

pub(super) struct Header {
    pub id: String,
    pub cwd: String,
    pub date: String,
}
pub(super) struct Transcript {
    pub id: String,
    pub date: String,
    pub title: String,
    pub body: String,
}

// Skip large protocol blobs without allocating their full line. Only completed,
// user-visible chat events enter a diary record.
const LINE_LIMIT: u64 = 8 * 1024 * 1024;
fn line(input: &mut impl BufRead, buf: &mut Vec<u8>) -> Result<bool, String> {
    buf.clear();
    input
        .take(LINE_LIMIT + 1)
        .read_until(b'\n', buf)
        .map_err(|e| e.to_string())?;
    if buf.len() as u64 > LINE_LIMIT {
        if buf.last() != Some(&b'\n') {
            input.skip_until(b'\n').map_err(|e| e.to_string())?;
        }
        buf.clear();
        return Ok(true);
    }
    Ok(buf.last() == Some(&b'\n'))
}

pub(super) fn header(input: &mut impl BufRead) -> Result<Option<Header>, String> {
    let mut buf = Vec::new();
    if !line(input, &mut buf)? || buf.is_empty() {
        return Ok(None);
    }
    let meta: Value = serde_json::from_slice(&buf).map_err(|e| e.to_string())?;
    let p = &meta["payload"];
    if meta["type"] != "session_meta"
        || p["originator"] != "Codex Desktop"
        || p["source"].is_object()
    {
        return Ok(None);
    }
    let id = p["id"]
        .as_str()
        .and_then(|s| uuid::Uuid::parse_str(s).ok())
        .ok_or("세션 ID 누락")?
        .to_string();
    let cwd = p["cwd"]
        .as_str()
        .filter(|s| Path::new(s).is_absolute())
        .ok_or("세션 폴더 누락")?
        .to_owned();
    let date = DateTime::parse_from_rfc3339(p["timestamp"].as_str().ok_or("시각 누락")?)
        .map_err(|e| e.to_string())?
        .with_timezone(&Local)
        .date_naive()
        .to_string();
    Ok(Some(Header { id, cwd, date }))
}

fn clip(s: &str, bytes: usize) -> String {
    if s.len() <= bytes {
        return s.into();
    }
    let boundary = s.floor_char_boundary(bytes);
    format!("{}… (이하 원문 참조)", &s[..boundary])
}

// Keep the start and recent turns when a transcript exceeds the diary editor's limit.
#[derive(Default)]
struct Excerpts {
    first: String,
    last: VecDeque<String>,
    last_bytes: usize,
    omitted: bool,
}
impl Excerpts {
    fn add(&mut self, block: String) {
        if self.last.is_empty() && !self.omitted && self.first.len() + block.len() <= 40_000 {
            self.first.push_str(&block);
            return;
        }
        self.last_bytes += block.len();
        self.last.push_back(block);
        while self.last_bytes > 48_000 {
            self.last_bytes -= self.last.pop_front().unwrap().len();
            self.omitted = true;
        }
    }
    fn finish(self) -> String {
        let mut s = self.first;
        if self.omitted {
            s.push_str("\n(중간 대화는 원문 참조)\n");
        }
        for b in self.last {
            s.push_str(&b);
        }
        s
    }
}

pub(super) fn read(
    input: &mut impl BufRead,
    header: Header,
    path: &Path,
    index_title: &str,
) -> Result<Option<Transcript>, String> {
    let mut excerpts = Excerpts::default();
    let mut first_request = None;
    let mut ids = HashSet::new();
    let mut buf = Vec::new();
    let mut n = 1;
    while line(input, &mut buf)? {
        n += 1;
        let raw = std::str::from_utf8(&buf).map_err(|e| e.to_string())?;
        if !raw.contains("\"item_completed\"") {
            continue;
        }
        let row: Value = serde_json::from_str(raw).map_err(|e| e.to_string())?;
        let p = &row["payload"];
        if row["type"] != "event_msg"
            || p["type"] != "item_completed"
            || p["thread_id"].as_str() != Some(&header.id)
        {
            continue;
        }
        let item = &p["item"];
        let Some(id) = item["id"].as_str() else {
            continue;
        };
        let (label, text) = match item["type"].as_str() {
            Some(kind @ ("UserMessage" | "AgentMessage")) => {
                let text: String = item["content"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|b| b["text"].as_str())
                    .collect();
                if text.trim().is_empty() {
                    continue;
                }
                if kind == "UserMessage" && first_request.is_none() {
                    first_request = Some(
                        text.lines()
                            .find(|l| !l.trim().is_empty())
                            .unwrap_or("")
                            .trim()
                            .chars()
                            .take(100)
                            .collect::<String>(),
                    );
                }
                (
                    if kind == "UserMessage" {
                        "사용자"
                    } else {
                        "AI"
                    },
                    text,
                )
            }
            Some("CommandExecution") => {
                let Some(evidence) = super::command::evidence(item) else {
                    continue;
                };
                ("검사 출력", evidence)
            }
            _ => continue,
        };
        if ids.insert(id.to_owned()) {
            let mut block = format!("\n## {label} · 원문 {n}행\n");
            for line in clip(&text, 16_000).split_inclusive('\n') {
                // Quoted transcript headings are content, never provenance for a new block.
                if line.starts_with("## ") && line.contains(" · 원문 ") {
                    block.push_str("> ");
                }
                block.push_str(line);
            }
            block.push('\n');
            excerpts.add(block);
        }
    }
    let Some(fallback) = first_request else {
        return Ok(None);
    };
    let name = if index_title.trim().is_empty() {
        fallback.as_str()
    } else {
        index_title.trim()
    };
    let title = format!(
        "Codex 채팅 · {}",
        name.chars()
            .filter(|c| !c.is_control())
            .take(150)
            .collect::<String>()
    );
    let body = format!(
        "Windows Codex · {}\n원문: {}\n{}",
        header.id,
        path.display(),
        excerpts.finish()
    );
    Ok(Some(Transcript {
        id: header.id,
        date: header.date,
        title,
        body,
    }))
}
