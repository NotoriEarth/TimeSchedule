//! Claude Code session files. Every row is one message, and names its session.
//!
//! Tool results come back as `user` rows, and the app writes `user` rows of its own
//! (slash-command output, interruptions, task notifications). So text in a `user` row
//! is a person's only when `origin.kind` is `human` and no other mark of the row says
//! otherwise. On 2026-11-24 the 12 main sessions kept on this PC (Claude Code 2.1.325
//! to 2.1.330) had 119 such rows; the 20 plain rows with neither origin field were all
//! written by the app. An instruction another session sent has `turnOrigin`
//! `host_synthetic`.
//!
//! A person's pick in a question dialog comes back as the result of `AskUserQuestion`.
//! The picks are in the row's `toolUseResult.answers`, a table from each question to
//! the answer picked; the text of the result is the tool's. The 4 such rows kept on
//! 2026-11-24 each held one result, and every answer was a string.
//!
//! The app puts `<system-reminder>…</system-reminder>` parts before a person's text. In
//! the same 12 sessions the 6 typed rows that had any each held them as a text block
//! of their own ahead of the person's block (9 parts, none after or inside the person's
//! text), and no typed row that was one string had the tag. An instruction another
//! session sent is one string that starts with them (34 rows).
//!
//! A row has a `uuid`, and a tool result a `tool_use_id`. A row that is no message of
//! a person's here still tells against a copy of it that looks like one, so the reader
//! hands on every `uuid` and every result ID it saw.
use super::{one_line, plain_lines, without_blank_edges, Item, Parsed, Role, Row, Rows};
use crate::chat_history::command;
use serde_json::Value;
use std::collections::HashMap;

const SHELLS: [&str; 2] = ["Bash", "PowerShell"];
const QUESTION: &str = "AskUserQuestion";
/// How rows the app writes without either origin field start: a slash command and its
/// output. A person does not start a message this way.
const APP_START: [&str; 2] = ["<local-command-", "<command-name>"];
/// The whole text of the rows the app writes when a person stops it. A person can
/// start a message with these words, so only the whole text is the app's. The first
/// was the one such row kept on 2026-11-24; the second is the app's text for stopping
/// a tool, not seen in a file here.
const APP_WHOLE: [&str; 2] = [
    "[Request interrupted by user]",
    "[Request interrupted by user for tool use]",
];

/// What one of a row's two origin fields says about who sent it.
#[derive(PartialEq)]
enum Mark {
    Absent,
    Human,
    Other,
}

fn mark(field: &Value, value: Option<&str>) -> Mark {
    match value {
        Some("human") => Mark::Human,
        _ if field.is_null() => Mark::Absent,
        _ => Mark::Other,
    }
}

/// The text of a message: its string, or its text blocks that are not blank.
fn text_blocks(content: &Value) -> Vec<&str> {
    match content {
        Value::String(s) => vec![s],
        Value::Array(parts) => parts
            .iter()
            .filter(|p| p["type"] == "text")
            .filter_map(|p| p["text"].as_str())
            .filter(|t| !t.trim().is_empty())
            .collect(),
        _ => Vec::new(),
    }
}

fn texts(content: &Value, between: &str) -> String {
    text_blocks(content).join(between)
}

/// The text after the complete `<system-reminder>…</system-reminder>` parts it starts
/// with. A tag anywhere else, or one that is not closed, stays.
fn after_reminders(text: &str) -> &str {
    let mut rest = text;
    while let Some((_, after)) = rest
        .trim_start()
        .strip_prefix("<system-reminder>")
        .and_then(|inside| inside.split_once("</system-reminder>"))
    {
        rest = after;
    }
    rest
}

/// A person's text without the reminders the app put before it. The app writes them as
/// text blocks of their own ahead of the person's block, so only such blocks go, and
/// never the last block of the message, whatever kind that block is: a reminder before
/// an image alone goes too. A tag in the person's own block, or in a message that is
/// one string, is what the person wrote.
fn typed_text(content: &Value) -> String {
    let Value::Array(parts) = content else {
        return texts(content, "\n");
    };
    let last = parts.len().saturating_sub(1);
    let app = |(at, text): &(usize, &str)| *at < last && after_reminders(text).trim().is_empty();
    let blocks = parts
        .iter()
        .enumerate()
        .filter(|(_, part)| part["type"] == "text")
        .filter_map(|(at, part)| Some((at, part["text"].as_str()?)))
        .filter(|(_, text)| !text.trim().is_empty());
    let own: Vec<&str> = blocks.skip_while(app).map(|(_, text)| text).collect();
    own.join("\n")
}

fn result_text(block: &Value) -> String {
    match &block["content"] {
        Value::String(s) => s.clone(),
        Value::Array(parts) => parts
            .iter()
            .filter_map(|p| p["text"].as_str())
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

/// The answers a person picked in a question dialog, each under its question, in the
/// order asked. `None` when the row holds no picked answer.
fn picked(result: &Value) -> Option<String> {
    let answers = result["answers"].as_object()?;
    let asked = result["questions"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|q| q["question"].as_str());
    // Answers to questions the row does not list come after, in the table's order.
    let mut order: Vec<&str> = asked.filter(|q| answers.contains_key(*q)).collect();
    for question in answers.keys() {
        if !order.contains(&question.as_str()) {
            order.push(question);
        }
    }
    let pairs: Vec<String> = order
        .into_iter()
        .filter_map(|question| {
            let answer = plain_lines(answers[question].as_str()?);
            let answer = without_blank_edges(&answer);
            (!answer.is_empty())
                .then(|| format!("질문: {}\n고른 답: {answer}", one_line(question).trim()))
        })
        .collect();
    (!pairs.is_empty()).then(|| pairs.join("\n\n"))
}

struct Reader<'a> {
    session: &'a str,
    subagent: bool,
    /// The tool each `tool_use` block called, by the block's ID.
    tools: HashMap<String, String>,
    /// Whether any row has an origin field, and whether a row without one holds text
    /// that the app is not known to write.
    marked: bool,
    bare: bool,
    parsed: Parsed,
}

pub(super) fn read<R: std::io::BufRead>(
    rows: &mut Rows<R>,
    session: &str,
    subagent: bool,
) -> Result<Parsed, String> {
    let mut reader = Reader {
        session,
        subagent,
        tools: HashMap::new(),
        marked: false,
        bare: false,
        parsed: Parsed {
            by_program: subagent,
            ..Parsed::default()
        },
    };
    while let Some(row) = rows.next()? {
        let from = reader.parsed.items.len();
        reader.row(&row);
        // The row and each tool result in it stand against a copy taken for a person's,
        // unless the row gave a message of that key itself. This is so for a row the
        // reader left out whole, too.
        let content = row.value["message"]["content"].as_array();
        let results = content
            .into_iter()
            .flatten()
            .filter(|block| block["type"] == "tool_result")
            .filter_map(|block| Some(format!("result:{}", block["tool_use_id"].as_str()?)));
        let uuid = row.value["uuid"].as_str().map(str::to_owned);
        for key in uuid.into_iter().chain(results) {
            let taken = |item: &Item| item.key.as_ref() == Some(&key);
            if !reader.parsed.items[from..].iter().any(taken) {
                reader.parsed.items.push(Item::contrary(key, &row));
            }
        }
    }
    // A file from before Claude Code wrote who sent a message.
    reader.parsed.unmarked = !subagent && reader.bare && !reader.marked;
    Ok(reader.parsed)
}

impl Reader<'_> {
    /// Adds the messages of one row. A row it returns from early has none.
    fn row(&mut self, row: &Row) {
        let v = &row.value;
        let kind = v["type"].as_str();
        if !matches!(kind, Some("user" | "assistant")) || v["message"]["role"] != v["type"] {
            return;
        }
        match v["sessionId"].as_str() {
            Some(id) if id == self.session => {}
            // A row copied from another session.
            Some(_) => return,
            None => {
                self.parsed.unnamed += 1;
                return;
            }
        }
        if v["isMeta"] == true
            || v["isCompactSummary"] == true
            || (!self.subagent && v["isSidechain"] == true)
        {
            return;
        }
        let key = v["uuid"].as_str().map(str::to_owned);
        let content = &v["message"]["content"];
        let blocks = || content.as_array().into_iter().flatten();
        let item = |key, role, group, text| Item {
            key,
            role,
            line: row.line,
            at: row.at,
            group,
            text,
        };
        if kind == Some("assistant") {
            for block in blocks().filter(|b| b["type"] == "tool_use") {
                if let (Some(id), Some(name)) = (block["id"].as_str(), block["name"].as_str()) {
                    self.tools.insert(id.to_owned(), name.to_owned());
                }
            }
            let text = texts(content, "\n\n");
            if !text.trim().is_empty() {
                // Claude Code can write one answer as several rows. Each is kept where
                // it is; they share the answer's API message ID.
                let answer = v["message"]["id"].as_str().map(str::to_owned);
                self.parsed.items.push(item(key, Role::Ai, answer, text));
            }
            return;
        }
        let origin = mark(&v["origin"], v["origin"]["kind"].as_str());
        let turn = mark(&v["turnOrigin"], v["turnOrigin"].as_str());
        self.marked |= origin != Mark::Absent || turn != Mark::Absent;
        let no_other_sender = origin != Mark::Other && turn != Mark::Other;
        let results: Vec<(&str, &Value)> = blocks()
            .filter(|b| b["type"] == "tool_result")
            .filter_map(|b| Some((b["tool_use_id"].as_str()?, b)))
            .collect();
        let tool = |id: &str| self.tools.get(id).map(String::as_str);
        let questions = results
            .iter()
            .filter(|(id, _)| tool(id) == Some(QUESTION))
            .count();
        for (id, block) in &results {
            let failed = block["is_error"] == true;
            let message = match tool(id) {
                // `toolUseResult` describes the row, so the answers are this question's
                // only when the row answers no other. A question that failed or was
                // cancelled has no answer, whatever the row holds.
                Some(QUESTION) => picked(&v["toolUseResult"])
                    .filter(|_| !self.subagent && !failed && questions == 1 && no_other_sender)
                    .map(|answers| (Role::Picked, answers)),
                Some(name) if SHELLS.contains(&name) => {
                    let text = result_text(block);
                    // Here too `toolUseResult` is this result's only when the row has
                    // no other.
                    let reported = (results.len() == 1)
                        .then(|| v["toolUseResult"]["exitCode"].as_i64())
                        .flatten();
                    let code = reported.or_else(|| {
                        text.lines()
                            .filter(|_| failed)
                            .find_map(|l| l.trim().strip_prefix("Exit code ")?.parse::<i64>().ok())
                    });
                    (failed || code.is_some_and(|c| c != 0))
                        .then(|| command::diagnostics(&text, code, failed))
                        .flatten()
                        .map(|evidence| (Role::Check, evidence))
                }
                _ => None,
            };
            if let Some((role, text)) = message {
                let key = Some(format!("result:{id}"));
                self.parsed.items.push(item(key, role, None, text));
            }
        }
        let text = texts(content, "\n");
        if text.trim().is_empty() {
            return;
        }
        let (role, text) = if !self.subagent && origin == Mark::Human && turn != Mark::Other {
            (Role::Human, typed_text(content))
        } else if v["turnOrigin"] == "host_synthetic" {
            (Role::Delegated, after_reminders(&text).to_owned())
        } else {
            // Task notifications and other text the app adds. Plain text with no mark
            // at all may be a person's, from before the tool wrote who sent a message.
            let plain = origin == Mark::Absent
                && turn == Mark::Absent
                && results.is_empty()
                && v["toolUseResult"].is_null()
                && !APP_START.iter().any(|start| text.starts_with(start))
                && !APP_WHOLE.contains(&text.as_str());
            self.bare |= plain;
            if plain && row.on_day && !self.subagent {
                self.parsed.plain += 1;
            }
            return;
        };
        self.parsed.items.push(item(key, role, None, text));
    }
}
