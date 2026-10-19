//! Undo history modelled on `@codemirror/commands` 6.10 `history()`
//! (the CodeMirror 6 history cm-vim's `u` / `<C-r>` drive).
//!
//! Every transaction is added like `HistoryBranch.addChanges` /
//! `addSelection`: a doc change joins the top event when the top event has
//! changes, the user event is joinable (`input.type*` / `delete*` or none),
//! and either no selection was recorded after it, the change is adjacent and
//! within `newGroupDelay` (500ms), or the user event is
//! `input.type.compose` (which cm-vim uses for every change after the first
//! of one command). A selection-only transaction records the previous
//! selection on the top event, which ends the group.

use fuchico_helix::{Change, ChangeSet, Selection, Text};

const NEW_GROUP_DELAY: f64 = 500.0;
const MIN_DEPTH: usize = 100;

#[derive(Debug, Clone)]
struct Event {
    /// Change sets that revert the event, in application order (`None` for
    /// a selection-only event).
    undo: Option<Vec<ChangeSet>>,
    /// Ranges the event touched, in the current document (for
    /// `isAdjacent`).
    ranges: Vec<(usize, usize)>,
    start_sel: Option<Selection>,
    sels_after: Vec<Selection>,
}

/// The undo/redo branches.
#[derive(Debug, Clone, Default)]
pub struct UndoHistory {
    done: Vec<Event>,
    undone: Vec<Event>,
    prev_time: f64,
    prev_user_event: Option<String>,
    /// Do not record transactions (external `set_text`).
    pub suspend: bool,
}

/// `ChangeSet.invert(doc)`.
pub(crate) fn invert(changes: &ChangeSet, old: &Text) -> ChangeSet {
    let mut out = Vec::new();
    let mut delta: isize = 0;
    for c in changes.changes() {
        let from = (c.from as isize + delta) as usize;
        let ins = c.insert.chars().count();
        out.push(Change::new(from, from + ins, old.slice(c.from, c.to)));
        delta += ins as isize - (c.to - c.from) as isize;
    }
    ChangeSet::new(out)
}

/// Changed ranges of `changes` in the new document.
fn new_ranges(changes: &ChangeSet) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut delta: isize = 0;
    for c in changes.changes() {
        let from = (c.from as isize + delta) as usize;
        let ins = c.insert.chars().count();
        out.push((from, from + ins));
        delta += ins as isize - (c.to - c.from) as isize;
    }
    out
}

fn is_adjacent(ranges: &[(usize, usize)], changes: &ChangeSet) -> bool {
    changes
        .changes()
        .iter()
        .any(|c| ranges.iter().any(|&(f, t)| c.to >= f && c.from <= t))
}

fn joinable(user_event: Option<&str>) -> bool {
    match user_event {
        None => true,
        Some(u) => {
            let ok = |p: &str| u == p || u.starts_with(&format!("{p}."));
            ok("input.type") || ok("delete")
        }
    }
}

fn is_select(user_event: Option<&str>) -> bool {
    user_event.is_some_and(|u| u == "select" || u.starts_with("select."))
}

fn push_branch(branch: &mut Vec<Event>, e: Event) {
    branch.push(e);
    if branch.len() > MIN_DEPTH + 20 {
        let n = branch.len() - MIN_DEPTH;
        branch.drain(..n);
    }
}

/// `addSelection(branch, selection)`.
fn add_selection_to(branch: &mut Vec<Event>, sel: &Selection) {
    match branch.last_mut() {
        None => branch.push(Event {
            undo: None,
            ranges: Vec::new(),
            start_sel: None,
            sels_after: vec![sel.clone()],
        }),
        Some(last) => {
            if last.sels_after.last().is_some_and(|s| s.same(sel)) {
                return;
            }
            if last.sels_after.len() >= 200 {
                last.sels_after.remove(0);
            }
            last.sels_after.push(sel.clone());
        }
    }
}

/// What an undo/redo transaction applies.
#[allow(dead_code)]
pub(crate) struct Popped {
    pub steps: Vec<ChangeSet>,
    pub selection: Option<Selection>,
    /// Selection recorded on the opposite branch.
    pub other_sel: Selection,
    pub rest_side: bool,
}

impl UndoHistory {
    /// Record a transaction. `old` is the document before `changes`.
    pub(crate) fn add(
        &mut self,
        old: &Text,
        changes: Option<&ChangeSet>,
        start_sel: &Selection,
        selection_set: bool,
        user_event: Option<&str>,
        time: f64,
    ) {
        if self.suspend {
            return;
        }
        match changes.filter(|c| !c.is_empty()) {
            Some(cs) => {
                let inv = invert(cs, old);
                let join = self.done.last().is_some_and(|last| {
                    last.undo.is_some()
                        && joinable(user_event)
                        && ((last.sels_after.is_empty()
                            && time - self.prev_time < NEW_GROUP_DELAY
                            && is_adjacent(&last.ranges, cs))
                            || user_event == Some("input.type.compose"))
                });
                if join {
                    let last = self.done.last_mut().unwrap();
                    let mut ranges: Vec<(usize, usize)> = last
                        .ranges
                        .iter()
                        .map(|&(f, t)| (cs.map_pos(f, -1), cs.map_pos(t, 1)))
                        .collect();
                    ranges.extend(new_ranges(cs));
                    last.ranges = ranges;
                    last.undo.as_mut().unwrap().insert(0, inv);
                    last.sels_after.clear();
                } else {
                    push_branch(
                        &mut self.done,
                        Event {
                            undo: Some(vec![inv]),
                            ranges: new_ranges(cs),
                            start_sel: Some(start_sel.clone()),
                            sels_after: Vec::new(),
                        },
                    );
                }
                self.undone.clear();
                self.prev_time = time;
                self.prev_user_event = user_event.map(String::from);
            }
            None if selection_set => {
                let skip = self.done.last().is_some_and(|last| {
                    !last.sels_after.is_empty()
                        && time - self.prev_time < NEW_GROUP_DELAY
                        && user_event.is_some()
                        && user_event == self.prev_user_event.as_deref()
                        && is_select(user_event)
                });
                if !skip {
                    add_selection_to(&mut self.done, start_sel);
                    self.prev_time = time;
                    self.prev_user_event = user_event.map(String::from);
                }
            }
            None => {}
        }
    }

    /// `HistoryState.pop`: the transaction undo (`redo = false`) or redo
    /// would dispatch, or `None` when there is nothing to do.
    pub(crate) fn pop(&mut self, redo: bool, current: &Selection) -> Option<Popped> {
        let branch = if redo { &mut self.undone } else { &mut self.done };
        let event = branch.last()?;
        let steps = event.undo.clone()?;
        let other_sel = event
            .sels_after
            .first()
            .cloned()
            .or_else(|| event.start_sel.clone())
            .unwrap_or_else(|| current.clone());
        let selection = event.start_sel.clone();
        branch.pop();
        Some(Popped {
            steps,
            selection,
            other_sel,
            rest_side: redo,
        })
    }

    /// Record the applied undo/redo transaction on the opposite branch.
    /// `applied` are the change sets applied (in order) with the document
    /// before each one.
    pub(crate) fn push_other(&mut self, redo: bool, applied: &[(ChangeSet, Text)], sel: Selection) {
        let mut undo = Vec::new();
        let mut ranges: Vec<(usize, usize)> = Vec::new();
        for (cs, old) in applied {
            undo.insert(0, invert(cs, old));
            ranges = ranges
                .iter()
                .map(|&(f, t)| (cs.map_pos(f, -1), cs.map_pos(t, 1)))
                .collect();
            ranges.extend(new_ranges(cs));
        }
        let other = if redo { &mut self.done } else { &mut self.undone };
        if undo.iter().all(|c| c.is_empty()) {
            add_selection_to(other, &sel);
        } else {
            push_branch(
                other,
                Event {
                    undo: Some(undo),
                    ranges,
                    start_sel: Some(sel),
                    sels_after: Vec::new(),
                },
            );
        }
    }

    pub fn clear(&mut self) {
        self.done.clear();
        self.undone.clear();
    }

    /// Number of undoable change events.
    #[allow(dead_code)]
    pub fn undo_depth(&self) -> usize {
        self.done.iter().filter(|e| e.undo.is_some()).count()
    }

    #[allow(dead_code)]
    pub fn redo_depth(&self) -> usize {
        self.undone.iter().filter(|e| e.undo.is_some()).count()
    }
}
