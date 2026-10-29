pub struct History<T> {
    undo: Vec<T>,
    redo: Vec<T>,
}
impl<T> Default for History<T> {
    fn default() -> Self {
        Self {
            undo: vec![],
            redo: vec![],
        }
    }
}
impl<T> History<T> {
    pub fn record(&mut self, previous: T) {
        self.undo.push(previous);
        if self.undo.len() > 32 {
            self.undo.remove(0);
        }
        self.redo.clear();
    }
    pub fn undo(&mut self, current: T) -> Option<T> {
        let previous = self.undo.pop()?;
        self.redo.push(current);
        Some(previous)
    }
    pub fn redo(&mut self, current: T) -> Option<T> {
        let next = self.redo.pop()?;
        self.undo.push(current);
        Some(next)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unicode_undo_redo_and_new_edit_drops_redo() {
        let mut h = History::default();
        h.record("");
        h.record("만년");
        assert_eq!(h.undo("만년 다이어리"), Some("만년"));
        assert_eq!(h.redo("만년"), Some("만년 다이어리"));
        assert_eq!(h.undo("만년 다이어리"), Some("만년"));
        h.record("만년");
        assert_eq!(h.redo("만년 계획"), None);
    }
    #[test]
    fn history_is_bounded() {
        let mut h = History::default();
        for n in 0..100 {
            h.record(n);
        }
        let mut count = 0;
        while h.undo(101).is_some() {
            count += 1;
        }
        assert_eq!(count, 32);
    }
}
