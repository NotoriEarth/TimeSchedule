//! Review local, attributed excerpts before adding them to project history.
use crate::storage::{database_error, timestamp, Storage};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use std::fmt;

mod extract;
mod model;
mod terra;
pub use model::MODEL;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatChoice {
    pub id: String,
    pub title: String,
}
impl fmt::Display for ChatChoice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.title)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Kind {
    Design,
    Troubleshooting,
}
impl Kind {
    fn key(self) -> &'static str {
        match self {
            Self::Design => "design",
            Self::Troubleshooting => "troubleshooting",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Design => "설계",
            Self::Troubleshooting => "트러블슈팅",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub kind: Kind,
    pub title: String,
    pub body: String,
}

#[derive(Debug, Clone)]
pub struct Review {
    project: String,
    chat: ChatChoice,
    candidates: Vec<Candidate>,
    source: String,
}
impl Review {
    pub fn project(&self) -> &str {
        &self.project
    }
    pub fn chat(&self) -> &ChatChoice {
        &self.chat
    }
    pub fn candidates(&self) -> &[Candidate] {
        &self.candidates
    }
}
#[derive(Default, Debug, Clone)]
pub struct Saved {
    pub added: usize,
    pub kept: usize,
}

pub(crate) fn migrate(c: &Connection) -> Result<(), String> {
    c.execute_batch(
        "CREATE TABLE chat_experience_receipts(
        project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
        chat_id TEXT NOT NULL, kind TEXT NOT NULL, entry_id TEXT NOT NULL,
        PRIMARY KEY(project_id,chat_id,kind)); PRAGMA user_version=11;",
    )
    .map_err(database_error)
}

fn read_source(
    c: &Connection,
    project: &str,
    id: &str,
) -> Result<(ChatChoice, String, String), String> {
    let (chat,body,date)=c.query_row("SELECT e.id,e.title,e.source_body,e.date FROM codex_chat_sessions s JOIN diary_entries e ON e.id=s.entry_id AND e.project_id=s.project_id WHERE s.project_id=?1 AND e.id=?2",
        params![project,id],|r|Ok((ChatChoice{id:r.get(0)?,title:r.get(1)?},r.get::<_,String>(2)?,r.get::<_,String>(3)?))).optional().map_err(database_error)?.ok_or("이 프로젝트에서 수집한 채팅을 다시 선택하세요.")?;
    Ok((chat, body, date))
}

impl Storage {
    pub fn reviewable_chats(&self, project: &str) -> Result<Vec<ChatChoice>, String> {
        let c = self.connection()?;
        let mut q=c.prepare("SELECT e.id,e.title FROM codex_chat_sessions s JOIN diary_entries e ON e.id=s.entry_id AND e.project_id=s.project_id WHERE s.project_id=?1 ORDER BY e.date DESC,e.updated_at DESC,e.id").map_err(database_error)?;
        let rows = q
            .query_map([project], |r| {
                Ok(ChatChoice {
                    id: r.get(0)?,
                    title: r.get(1)?,
                })
            })
            .map_err(database_error)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(database_error)
    }

    pub fn review_chat(&self, project: &str, id: &str) -> Result<Review, String> {
        let (chat, source, _) = read_source(&*self.connection()?, project, id)?;
        let candidates = extract::candidates(&chat, &source);
        Ok(Review {
            project: project.into(),
            chat,
            candidates,
            source,
        })
    }
    pub fn review_chat_with_model(&self, project: &str, id: &str) -> Result<Review, String> {
        self.review_chat_with_inference(project, id, terra::infer)
    }

    pub fn review_chat_with_inference(
        &self,
        project: &str,
        id: &str,
        infer: impl FnOnce(&serde_json::Value) -> Result<String, String>,
    ) -> Result<Review, String> {
        let (chat, source, _) = read_source(&*self.connection()?, project, id)?;
        let candidates = model::review(&chat, &source, infer)?;
        Ok(Review {
            project: project.into(),
            chat,
            candidates,
            source,
        })
    }

    pub fn confirm_chat_candidates(
        &self,
        review: &Review,
        selected: &[Kind],
    ) -> Result<Saved, String> {
        if selected.is_empty() {
            return Err("저장할 후보를 선택하세요.".into());
        }
        let mut c = self.connection()?;
        let tx = c
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(database_error)?;
        let (chat, source, date) = read_source(&tx, &review.project, &review.chat.id)?;
        if chat != review.chat || source != review.source {
            return Err("채팅이 변경되었습니다. 후보를 다시 읽고 확인하세요.".into());
        }
        let mut saved = Saved::default();
        for &kind in selected {
            let item = review
                .candidates
                .iter()
                .find(|x| x.kind == kind)
                .ok_or("검토 목록에 없는 후보입니다.")?;
            let key = params![review.project, review.chat.id, kind.key()];
            let known:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM chat_experience_receipts WHERE project_id=?1 AND chat_id=?2 AND kind=?3)",key,|r|r.get(0)).map_err(database_error)?;
            if known {
                saved.kept += 1;
                continue;
            }
            // Earlier manually reviewed experiences cite their source diary ID on a provenance line.
            let marker = format!(" · 기록 {}", review.chat.id);
            let mut legacy=tx.prepare("SELECT id,body FROM diary_entries WHERE project_id=?1 AND kind=?2 AND instr(body,?3)>0").map_err(database_error)?;
            let rows = legacy
                .query_map(params![review.project, kind.key(), marker], |r| {
                    Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
                })
                .map_err(database_error)?;
            let mut matches = Vec::new();
            for row in rows {
                let (id, body) = row.map_err(database_error)?;
                if body.lines().any(|l| {
                    l.starts_with("Windows Codex 채팅 「") && l.trim_end().ends_with(&marker)
                }) {
                    matches.push(id);
                }
            }
            let entry = match matches.as_slice() {
                [id] => {
                    saved.kept += 1;
                    id.clone()
                }
                [] => {
                    let id = uuid::Uuid::new_v4().to_string();
                    tx.execute("INSERT INTO diary_entries(id,project_id,date,kind,title,body,source_ref,source_body,updated_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?6,?8)",
                        params![id,review.project,date,kind.key(),item.title,item.body,format!("chat-experience:{}:{}:{}",review.project,review.chat.id,kind.key()),timestamp()]).map_err(database_error)?;
                    saved.added += 1;
                    id
                }
                _ => return Err(
                    "이 채팅을 인용한 같은 종류의 기존 경험이 여러 건입니다. 이력을 확인하세요."
                        .into(),
                ),
            };
            tx.execute("INSERT INTO chat_experience_receipts(project_id,chat_id,kind,entry_id) VALUES(?1,?2,?3,?4)",params![review.project,review.chat.id,kind.key(),entry]).map_err(database_error)?;
        }
        tx.commit().map_err(database_error)?;
        Ok(saved)
    }
}

#[cfg(test)]
mod tests;
