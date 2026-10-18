//! Undo history (cm-helix `historyField`): whole-document checkpoints,
//! newest first, with insert sessions recorded as a "temp" pending
//! checkpoint that is committed on `Escape`.

use crate::selection::Selection;
use crate::text::Text;

#[derive(Debug, Clone)]
pub struct Checkpoint {
    pub doc: Text,
    pub selection: Selection,
}

#[derive(Debug, Clone)]
pub struct History {
    /// Newest first.
    pub checkpoints: Vec<Checkpoint>,
    /// Index of the checkpoint currently restored; -1 = at head.
    pub cursor: isize,
    pub pending: Option<Checkpoint>,
    /// State before the first undo (target of redoing back to head).
    pub head: Option<Checkpoint>,
}

impl Default for History {
    fn default() -> Self {
        History {
            checkpoints: Vec::new(),
            cursor: -1,
            pending: None,
            head: None,
        }
    }
}

impl History {
    fn truncated(&self) -> Vec<Checkpoint> {
        if self.cursor == -1 {
            self.checkpoints.clone()
        } else {
            self.checkpoints[(self.cursor + 1) as usize..].to_vec()
        }
    }

    /// `{type: "add"}`.
    pub fn add(&mut self, state: Checkpoint, temp: bool) {
        if temp {
            self.pending = Some(state);
            return;
        }
        let mut checkpoints = vec![state];
        checkpoints.extend(self.truncated());
        self.checkpoints = checkpoints;
        self.cursor = -1;
        self.pending = None;
        self.head = None;
    }

    /// `{type: "commit"}` (leaving insert mode).
    pub fn commit(&mut self, current_doc: &Text) {
        let Some(pending) = self.pending.take() else {
            return;
        };
        if pending.doc == *current_doc {
            return;
        }
        let mut checkpoints = vec![pending];
        checkpoints.extend(self.truncated());
        self.checkpoints = checkpoints;
        self.cursor = -1;
        self.head = None;
    }

    /// `{type: "move"}`.
    pub fn move_by(&mut self, offset: isize, head: Option<Checkpoint>) {
        self.cursor += offset;
        self.pending = None;
        if self.head.is_none() {
            self.head = head;
        }
    }

    /// Target of `u` with `count`.
    pub fn undo_target(&self, count: usize) -> Option<&Checkpoint> {
        let next = self.cursor + count as isize;
        if next < 0 {
            return None;
        }
        self.checkpoints.get(next as usize)
    }

    /// Target of `U` with `count`.
    pub fn redo_target(&self, count: usize) -> Option<&Checkpoint> {
        let next = self.cursor - count as isize;
        if next == -1 {
            self.head.as_ref()
        } else if next < 0 {
            None
        } else {
            self.checkpoints.get(next as usize)
        }
    }

    pub fn can_undo(&self) -> bool {
        self.undo_target(1).is_some()
    }
    pub fn can_redo(&self) -> bool {
        self.redo_target(1).is_some()
    }
}
