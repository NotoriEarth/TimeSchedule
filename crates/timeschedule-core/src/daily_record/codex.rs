//! Codex rollouts. The first row (`session_meta`) names the session and says who
//! started it. What the screen showed is in `event_msg` rows of type `item_completed`;
//! the same messages also appear in `response_item` rows, which this reader skips.
//!
//! A message belongs to the session only when its row names it (`thread_id`). A fork
//! starts with its parent's rows: on 2026-11-24 the 115 forks kept on this PC copied
//! them as `response_item` rows only, and a copied `item_completed` row would name the
//! parent.
//!
//! A person's message carries a `client_id`. On 2026-11-24 every one of 346 messages in
//! sessions opened from the Codex app had one, and none of 722 messages written by
//! `codex exec`, subagents or automations did (Codex CLI 0.164 to 0.169.1).
use super::{Item, Parsed, Role, Rows};
use crate::chat_history::command;
use serde_json::Value;

fn text(item: &Value) -> String {
    item["content"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|b| b["text"].as_str())
        .collect()
}

pub(super) fn read<R: std::io::BufRead>(
    rows: &mut Rows<R>,
    session: &str,
) -> Result<Parsed, String> {
    let Some(first) = rows.next()? else {
        return Ok(Parsed::default());
    };
    // Only the first row is this session's. A fork has its parent's `session_meta` after it.
    let meta = &first.value["payload"];
    if first.value["type"] != "session_meta" || meta["id"].as_str() != Some(session) {
        return Err("첫 행이 이 세션의 머리글이 아닙니다.".into());
    }
    let source = &meta["source"];
    let by_program = source.is_object() || source == "exec" || meta["originator"] == "codex_exec";
    // A session a person would have opened: from the app or the terminal, by the user.
    let opened_by_person = !by_program
        && matches!(source.as_str(), Some("vscode" | "cli"))
        && meta["thread_source"].as_str().is_none_or(|s| s == "user");
    let mut parsed = Parsed {
        by_program,
        ..Parsed::default()
    };
    // Older Codex versions wrote a person's message only as `user_message`, which says
    // nothing of who typed it. It is not read, but it must not vanish without a word.
    let (mut older, mut items_of_users) = (0, false);
    while let Some(row) = rows.next()? {
        let p = &row.value["payload"];
        if row.value["type"] == "event_msg" && p["type"] == "user_message" && row.on_day {
            older += 1;
        }
        if row.value["type"] != "event_msg" || p["type"] != "item_completed" {
            continue;
        }
        items_of_users |= p["item"]["type"] == "UserMessage";
        let item = &p["item"];
        let key = item["id"].as_str().map(str::to_owned);
        let named = match p["thread_id"].as_str() {
            Some(thread) => thread == session,
            None => {
                parsed.unnamed += 1;
                false
            }
        };
        // A row that names another session is one a fork copied from its parent.
        let message = match item["type"].as_str().filter(|_| named) {
            Some("UserMessage") => {
                if item["client_id"].as_str().is_some_and(|id| !id.is_empty()) {
                    Some((Role::Human, text(item)))
                } else {
                    parsed.unmarked |= opened_by_person;
                    Some((Role::Delegated, text(item)))
                }
            }
            Some("AgentMessage") => Some((Role::Ai, text(item))),
            Some("CommandExecution") => item["exit_code"]
                .as_i64()
                .filter(|code| *code != 0)
                .and_then(|code| {
                    let output = item["aggregated_output"].as_str().unwrap_or("");
                    command::diagnostics(output, Some(code), false)
                })
                .map(|evidence| (Role::Check, evidence)),
            _ => None,
        };
        match (message, key) {
            (Some((role, text)), key) => parsed.items.push(Item {
                key,
                role,
                line: row.line,
                at: row.at,
                group: None,
                text,
                answers: Vec::new(),
            }),
            // A row left out still tells against a copy of the item that looks like
            // a person's.
            (None, Some(key)) => parsed.items.push(Item::contrary(key, &row)),
            (None, None) => {}
        }
    }
    if !items_of_users {
        parsed.older = older;
    }
    Ok(parsed)
}
