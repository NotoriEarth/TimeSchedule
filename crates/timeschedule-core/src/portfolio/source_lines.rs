use super::markdown;
use crate::{chat_experiences::blocks, storage::database_error};
use rusqlite::{params, Connection, OptionalExtension};
use std::collections::{BTreeMap, BTreeSet};

#[derive(PartialEq)]
pub(super) struct LineLink {
    pub(super) line: usize,
    pub(super) role: String,
    anchor: String,
}

impl LineLink {
    pub(super) fn target(&self) -> String {
        format!("evidence.md#{}", self.anchor)
    }
    pub(super) fn markdown(&self) -> String {
        format!(
            "- [{} · 원문 {}행]({})",
            self.role,
            self.line,
            self.target()
        )
    }
}

fn citation(marker: &str) -> Option<(&str, usize)> {
    let (role, n) = marker.split_once(" · 원문 ")?;
    matches!(role, "사용자" | "AI" | "검사 출력")
        .then_some((role, n.strip_suffix('행')?.parse().ok()?))
}

fn references(body: &str) -> BTreeSet<(&str, usize)> {
    let mut found = BTreeSet::new();
    let mut fence = None;
    for line in body.lines().map(str::trim) {
        if let Some(mark) = line.chars().next().filter(|c| matches!(c, '~' | '\x60')) {
            let width = line.chars().take_while(|c| *c == mark).count();
            if width >= 3 {
                match fence {
                    None => fence = Some((mark, width)),
                    Some((m, w)) if m == mark && width >= w && width == line.len() => fence = None,
                    _ => {}
                }
                continue;
            }
        }
        if fence.is_some() {
            continue;
        }
        // Earlier reviewed experiences use a standalone attribution before a fenced excerpt.
        found.extend(citation(line));
        found.extend(
            line.split('(')
                .skip(1)
                .filter_map(|part| citation(part.split_once(')')?.0)),
        );
    }
    found
}

pub(super) fn render(
    c: &Connection,
    project: &str,
    entry: &str,
    body: &str,
    evidence: &mut String,
    written: &mut BTreeSet<String>,
) -> Result<Vec<LineLink>, String> {
    let cited = references(body);
    if cited.is_empty() {
        return Ok(Vec::new());
    }
    // Only resolve against the saved transcript linked by the confirmation receipt.
    // Free-text source paths and row numbers cannot select an arbitrary local file.
    let source = c
        .query_row(
            "SELECT s.entry_id,s.session_id,e.source_body FROM chat_experience_receipts r
         JOIN codex_chat_sessions s ON s.project_id=r.project_id AND s.entry_id=r.chat_id
         JOIN diary_entries e ON e.id=s.entry_id AND e.project_id=s.project_id
         WHERE r.project_id=?1 AND r.entry_id=?2 GROUP BY r.entry_id HAVING count(*)=1",
            params![project, entry],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                ))
            },
        )
        .optional()
        .map_err(database_error)?;
    let Some((chat, session, source)) = source else {
        return Ok(Vec::new());
    };
    // Manual imports used the original session UUID in portable Markdown links.
    // Other sources use the local chat identity, including when two tools share a UUID.
    let key = session
        .strip_prefix("manual:claude-desktop:")
        .and_then(|s| uuid::Uuid::parse_str(s).ok())
        .map(|id| id.to_string())
        .unwrap_or_else(|| format!("chat-{chat}"));
    let mut lines = BTreeMap::new();
    let mut roles_by_line = BTreeMap::<usize, BTreeSet<&str>>::new();
    for b in blocks(&source) {
        roles_by_line.entry(b.line).or_default().insert(b.role);
        if !b.text.is_empty() && cited.contains(&(b.role, b.line)) {
            lines
                .entry((b.line, b.role))
                .or_insert_with(Vec::new)
                .push(b.text);
        }
    }
    let mut result = Vec::new();
    for ((line, role), quotes) in lines {
        let mut anchor = format!("source-{key}-L{line}");
        if roles_by_line[&line].len() > 1 {
            anchor.push_str(match role {
                "사용자" => "-user",
                "AI" => "-ai",
                _ => "-output",
            });
        }
        if written.insert(anchor.clone()) {
            markdown::source(
                evidence,
                &anchor,
                &format!("{role} · 원문 {line}행"),
                &quotes.join("\n"),
            );
        }
        result.push(LineLink {
            line,
            role: role.into(),
            anchor,
        });
    }
    Ok(result)
}
