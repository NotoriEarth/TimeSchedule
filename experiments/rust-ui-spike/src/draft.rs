#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CloseIntent {
    Editor,
    Application,
}

#[derive(Default)]
pub struct Draft {
    pub saved: String,
    pub pending: Option<CloseIntent>,
}

impl Draft {
    pub fn request_close(&mut self, current: &str, intent: CloseIntent) -> bool {
        if current == self.saved {
            return true;
        }
        self.pending = Some(intent);
        false
    }
    pub fn keep_editing(&mut self) {
        self.pending = None;
    }
    pub fn discard(&mut self) -> Option<CloseIntent> {
        self.pending.take()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dirty_close_requires_confirmation_and_preserves_intent() {
        let mut draft = Draft::default();
        assert!(!draft.request_close("만년 다이어리", CloseIntent::Application));
        assert_eq!(draft.pending, Some(CloseIntent::Application));
        assert_eq!(draft.discard(), Some(CloseIntent::Application));
        assert_eq!(draft.pending, None);
    }
    #[test]
    fn continue_editing_clears_close_intent() {
        let mut draft = Draft::default();
        draft.request_close("한글", CloseIntent::Editor);
        draft.keep_editing();
        assert_eq!(draft.pending, None);
        assert_eq!(draft.discard(), None);
    }
    #[test]
    fn unchanged_content_can_close() {
        let mut draft = Draft {
            saved: "기존 메모".into(),
            pending: None,
        };
        assert!(draft.request_close("기존 메모", CloseIntent::Editor));
    }
}
