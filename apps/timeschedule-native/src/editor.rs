use iced::widget::text_editor;
use timeschedule_core::{
    diary::{Entry, EntryInput, Project, ProjectInput},
    storage::{CalendarEvent, EventInput, Note, NoteInput, Task, TaskInput},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Event,
    Entry,
    Project,
    Note,
    Task,
}
impl Kind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Event => "일정",
            Self::Entry => "기록",
            Self::Project => "프로젝트",
            Self::Note => "메모",
            Self::Task => "할 일",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Form {
    pub kind: Kind,
    pub id: Option<String>,
    pub title: String,
    pub date: String,
    pub body: String,
    pub project_id: Option<String>,
    pub entry_kind: String,
    pub source_url: String,
    pub repo_path: String,
    pub all_day: bool,
    pub start: String,
    pub end: String,
    pub color: String,
    pub completed: bool,
    pub event_version: Option<String>,
}
impl Form {
    pub fn new(kind: Kind, date: String, project_id: Option<String>) -> Self {
        Self {
            kind,
            id: None,
            title: String::new(),
            date,
            body: String::new(),
            project_id,
            entry_kind: "development".into(),
            source_url: String::new(),
            repo_path: String::new(),
            all_day: true,
            start: "09:00".into(),
            end: "10:00".into(),
            color: "sage".into(),
            completed: false,
            event_version: None,
        }
    }
    pub fn event(e: CalendarEvent) -> Self {
        Self {
            id: Some(e.id),
            title: e.title,
            date: e.date,
            body: e.notes,
            all_day: e.all_day,
            start: e.start_time.unwrap_or_else(|| "09:00".into()),
            end: e.end_time.unwrap_or_else(|| "10:00".into()),
            color: e.color,
            completed: e.completed,
            event_version: Some(e.updated_at),
            ..Self::new(Kind::Event, String::new(), None)
        }
    }
    pub fn entry(e: Entry) -> Self {
        Self {
            id: Some(e.id),
            title: e.title,
            date: e.date,
            body: e.body,
            project_id: e.project_id,
            entry_kind: e.kind,
            source_url: e.source_url,
            ..Self::new(Kind::Entry, String::new(), None)
        }
    }
    pub fn project(p: Project) -> Self {
        Self {
            id: Some(p.id),
            title: p.title,
            body: p.description,
            repo_path: p.repo_path,
            ..Self::new(Kind::Project, String::new(), None)
        }
    }
    pub fn note(n: Note) -> Self {
        Self {
            id: Some(n.id),
            title: n.title,
            body: n.body,
            ..Self::new(Kind::Note, String::new(), None)
        }
    }
    pub fn task(t: Task) -> Self {
        Self {
            id: Some(t.id),
            title: t.title,
            date: t.date.unwrap_or_default(),
            completed: t.completed,
            ..Self::new(Kind::Task, String::new(), None)
        }
    }
    pub fn save(&self, store: &timeschedule_core::storage::Storage) -> Result<(), String> {
        match self.kind {
            Kind::Event => store
                .save_event_if_unchanged(
                    EventInput {
                        id: self.id.clone(),
                        title: self.title.clone(),
                        date: self.date.clone(),
                        start_time: if self.all_day {
                            None
                        } else {
                            Some(self.start.clone())
                        },
                        end_time: if self.all_day {
                            None
                        } else {
                            Some(self.end.clone())
                        },
                        all_day: self.all_day,
                        notes: self.body.clone(),
                        color: self.color.clone(),
                        completed: self.completed,
                    },
                    self.event_version.as_deref(),
                )
                .map(|_| ()),
            Kind::Entry => store
                .save_entry(EntryInput {
                    id: self.id.clone(),
                    project_id: self.project_id.clone(),
                    date: self.date.clone(),
                    kind: self.entry_kind.clone(),
                    title: self.title.clone(),
                    body: self.body.clone(),
                    source_url: self.source_url.clone(),
                })
                .map(|_| ()),
            Kind::Project => store
                .save_project(ProjectInput {
                    id: self.id.clone(),
                    title: self.title.clone(),
                    description: self.body.clone(),
                    repo_path: self.repo_path.clone(),
                })
                .map(|_| ()),
            Kind::Note => store
                .save_note(NoteInput {
                    id: self.id.clone(),
                    title: self.title.clone(),
                    body: self.body.clone(),
                })
                .map(|_| ()),
            Kind::Task => store
                .save_task(TaskInput {
                    id: self.id.clone(),
                    title: self.title.clone(),
                    date: if self.date.trim().is_empty() {
                        None
                    } else {
                        Some(self.date.clone())
                    },
                    completed: self.completed,
                })
                .map(|_| ()),
        }
    }
    pub fn delete(&self, store: &timeschedule_core::storage::Storage) -> Result<(), String> {
        let id = self.id.as_deref().ok_or("저장되지 않은 항목입니다.")?;
        match self.kind {
            Kind::Event => store.delete_event(id),
            Kind::Entry => store.delete_entry(id),
            Kind::Project => store.delete_project(id),
            Kind::Note => store.delete_note(id),
            Kind::Task => store.delete_task(id),
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseIntent {
    Editor,
    Manager,
    Quick,
    Application,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Confirmation {
    Discard(CloseIntent),
    Delete,
}

pub struct Editor {
    pub generation: u64,
    pub form: Form,
    initial: Form,
    pub content: text_editor::Content,
    pub saving: bool,
    pub composing: bool,
    pub confirmation: Option<Confirmation>,
    pub error: String,
    undo: Vec<(String, text_editor::Cursor)>,
    redo: Vec<(String, text_editor::Cursor)>,
}
impl Editor {
    pub fn new(form: Form, generation: u64) -> Self {
        Self {
            content: text_editor::Content::with_text(&form.body),
            initial: form.clone(),
            form,
            generation,
            saving: false,
            composing: false,
            confirmation: None,
            error: String::new(),
            undo: vec![],
            redo: vec![],
        }
    }
    pub fn dirty(&self) -> bool {
        self.form != self.initial
    }
    pub fn request_close(&mut self, intent: CloseIntent) -> bool {
        if self.saving {
            return false;
        }
        if self.composing {
            self.error = "한글 조합을 완료한 뒤 다시 닫아 주세요. 작성 내용은 유지됩니다.".into();
            return false;
        }
        if self.dirty() {
            self.confirmation = Some(Confirmation::Discard(intent));
            return false;
        }
        true
    }
    pub fn body_action(&mut self, action: text_editor::Action) {
        if self.saving || self.confirmation.is_some() {
            return;
        }
        let before = (self.content.text(), self.content.cursor());
        let is_edit = action.is_edit();
        self.content.perform(action);
        if is_edit && self.content.text() != before.0 {
            let limit = if self.form.kind == Kind::Entry {
                100_000
            } else {
                20_000
            };
            let value = self.content.text();
            if value.chars().count() > limit {
                self.content = text_editor::Content::with_text(&before.0);
                self.content.move_to(before.1);
                self.error = format!("본문은 {limit}자 이내로 입력해 주세요.");
                return;
            }
            self.undo.push(before);
            if self.undo.len() > 32 {
                self.undo.remove(0);
            }
            self.redo.clear();
            self.form.body = value;
        }
    }
    pub fn history(&mut self, redo: bool) {
        if self.saving || self.composing || self.confirmation.is_some() {
            return;
        }
        let current = (self.content.text(), self.content.cursor());
        let target = if redo {
            let x = self.redo.pop();
            if x.is_some() {
                self.undo.push(current);
            }
            x
        } else {
            let x = self.undo.pop();
            if x.is_some() {
                self.redo.push(current);
            }
            x
        };
        if let Some((text, cursor)) = target {
            self.content = text_editor::Content::with_text(&text);
            self.content.move_to(cursor);
            self.form.body = text;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dirty_close_preserves_intent() {
        let mut e = Editor::new(Form::new(Kind::Note, "2026-10-29".into(), None), 1);
        e.form.title = "한글".into();
        assert!(!e.request_close(CloseIntent::Application));
        assert_eq!(
            e.confirmation,
            Some(Confirmation::Discard(CloseIntent::Application))
        );
    }
    #[test]
    fn saving_and_composition_prevent_close() {
        for composing in [false, true] {
            let mut e = Editor::new(Form::new(Kind::Note, String::new(), None), 1);
            e.composing = composing;
            e.saving = !composing;
            assert!(!e.request_close(CloseIntent::Editor));
            assert_eq!(e.confirmation, None);
        }
    }
    #[test]
    fn unchanged_form_can_close() {
        let mut e = Editor::new(Form::new(Kind::Note, String::new(), None), 1);
        assert!(e.request_close(CloseIntent::Editor));
    }
    #[test]
    fn body_history_restores_korean_and_preserves_metadata() {
        let mut e = Editor::new(Form::new(Kind::Entry, String::new(), None), 1);
        e.form.title = "제목".into();
        e.body_action(text_editor::Action::Edit(text_editor::Edit::Paste(
            std::sync::Arc::new("만년 다이어리".into()),
        )));
        let typed = e.form.body.clone();
        e.history(false);
        assert_eq!(e.form.body, "");
        e.history(true);
        assert_eq!(e.form.body, typed);
        assert_eq!(e.form.title, "제목");
    }
}
