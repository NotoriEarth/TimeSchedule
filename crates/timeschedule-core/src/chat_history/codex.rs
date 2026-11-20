use super::transcript::{line, ChatText, Header, Transcript};
use chrono::{DateTime, Local};
use serde_json::Value;
use std::{io::BufRead, path::Path};
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

pub(super) fn read(
    input: &mut impl BufRead,
    header: Header,
    path: &Path,
    index_title: &str,
) -> Result<Option<Transcript>, String> {
    let mut chat = ChatText::default();
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
                let Some(evidence) = super::command::evidence(
                    item["aggregated_output"].as_str().unwrap_or(""),
                    item["exit_code"].as_i64(),
                    false,
                    false,
                ) else {
                    continue;
                };
                ("검사 출력", evidence)
            }
            _ => continue,
        };
        chat.add(id, label, n, &text);
    }
    Ok(chat.finish(header, path, index_title, super::Provider::Codex))
}
