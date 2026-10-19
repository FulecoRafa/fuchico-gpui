//! Insert mode bookkeeping, `.` repeat and macros: `exitInsertMode`,
//! `onChange`, `onCursorActivity`, `handleExternalSelection`,
//! `repeatLastEdit`, `repeatInsertModeChanges`, `executeMacroRegister`
//! and the `log*` helpers.

use crate::editor::Editor;
use crate::keymap::split_keys;
use crate::pos::{self, Pos, before, clen, offset};
use crate::state::{InsertChange, Register};

impl Editor {
    /// `exitInsertMode(cm, keepCursor)`.
    pub(crate) fn exit_insert_mode(&mut self, keep_cursor: bool) {
        let playing = self.g.macro_mode_state.is_playing;
        if !playing {
            self.change_listener = false;
            if let Some(m) = self.vim.insert_end.take() {
                self.clear_marker(m);
            }
        }
        if !playing && let Some(n) = self.vim.insert_mode_repeat.filter(|n| *n > 1) {
            self.repeat_last_edit(n - 1, true);
            if let Some(l) = self.vim.last_edit_input_state.as_mut() {
                l.repeat_override = Some(n);
            }
        }
        self.vim.insert_mode_repeat = None;
        self.vim.insert_mode = false;
        if !keep_cursor {
            let c = self.head();
            self.set_cursor(Pos::new(c.line, c.ch - 1));
        }
        self.overwrite = false;
        let text: String = self
            .g
            .macro_mode_state
            .last_insert_mode_changes
            .changes
            .iter()
            .map(|c| match c {
                InsertChange::Text(t) | InsertChange::Overwrite(t) => t.clone(),
                InsertChange::TextOffset(t, o) => format!("{t},{o}"),
                InsertChange::Key(_) => String::new(),
            })
            .collect();
        self.g
            .register_controller
            .get_register(Some('.'))
            .set_text(&text, false, false);
        self.signal_mode("normal", "");
        if self.g.macro_mode_state.is_recording {
            self.log_insert_mode_change();
        }
    }

    // ---- macros ----------------------------------------------------------------

    /// `macroModeState.enterMacroRecordMode`.
    pub(crate) fn enter_macro_record_mode(&mut self, name: char) {
        let reg = self.g.register_controller.get_register(Some(name));
        reg.clear();
        let m = &mut self.g.macro_mode_state;
        m.latest_register = Some(name);
        m.is_recording = true;
        self.vim.status.clear();
    }

    /// `macroModeState.exitMacroRecordMode`.
    pub(crate) fn exit_macro_record_mode(&mut self) {
        self.g.macro_mode_state.is_recording = false;
    }

    /// `executeMacroRegister`.
    pub(crate) fn execute_macro_register(&mut self, name: Option<char>) {
        let register: Register = self.g.register_controller.get_register(name).clone();
        if name == Some(':') {
            if let Some(cmd) = register.key_buffer.first().filter(|s| !s.is_empty()) {
                let cmd = cmd.clone();
                self.process_ex_command(&cmd);
            }
            self.g.macro_mode_state.is_playing = false;
            return;
        }
        let mut imc = 0;
        self.g.macro_mode_state.is_playing = true;
        self.g.macro_mode_state.replay_search_queries = register.search_queries.clone();
        for text in &register.key_buffer {
            for key in split_keys(text) {
                self.handle_key_str(&key, "macro");
                if self.vim.insert_mode {
                    let changes = register
                        .insert_mode_changes
                        .get(imc)
                        .map(|c| c.changes.clone())
                        .unwrap_or_default();
                    imc += 1;
                    self.g.macro_mode_state.last_insert_mode_changes.changes = changes.clone();
                    self.repeat_insert_mode_changes(&changes, 1);
                    self.exit_insert_mode(false);
                }
            }
        }
        self.g.macro_mode_state.is_playing = false;
    }

    fn recording_register(&mut self) -> Option<&mut Register> {
        let name = self.g.macro_mode_state.latest_register;
        Some(self.g.register_controller.get_register(name))
    }

    /// `logKey`.
    pub(crate) fn log_key(&mut self, key: &str) {
        if self.g.macro_mode_state.is_playing {
            return;
        }
        if let Some(r) = self.recording_register() {
            r.push_text(key, false);
        }
    }

    /// `logInsertModeChange`.
    pub(crate) fn log_insert_mode_change(&mut self) {
        if self.g.macro_mode_state.is_playing {
            return;
        }
        let c = self.g.macro_mode_state.last_insert_mode_changes.clone();
        if let Some(r) = self.recording_register() {
            r.push_insert_mode_changes(&c);
        }
    }

    /// `logSearchQuery`.
    pub(crate) fn log_search_query(&mut self, q: &str) {
        if self.g.macro_mode_state.is_playing {
            return;
        }
        if let Some(r) = self.recording_register() {
            r.push_search_query(q);
        }
    }

    // ---- change / cursor listeners ---------------------------------------------

    /// vim's insert-mode `onChange` (texts of one operation's changes).
    pub(crate) fn vim_on_change(&mut self, texts: &[String]) {
        if self.g.macro_mode_state.is_playing {
            return;
        }
        let sel_count = self.sel.len() as i64;
        let overwrite = self.overwrite;
        let insert_end = self.vim.insert_end.and_then(|m| self.find_marker(m));
        let cursor = self.head();
        let last = &mut self.g.macro_mode_state.last_insert_mode_changes;
        for text in texts {
            last.expect_cursor_activity_for_change = true;
            if last.ignore_count > 1 {
                last.ignore_count -= 1;
                continue;
            }
            if sel_count > 1 {
                last.ignore_count = sel_count;
            }
            if last.maybe_reset {
                last.changes.clear();
                last.maybe_reset = false;
            }
            if text.is_empty() {
                continue;
            }
            if overwrite && !text.contains('\n') {
                last.changes.push(InsertChange::Overwrite(text.clone()));
                continue;
            }
            let len = clen(text);
            if len > 1
                && let Some(ie) = insert_end
                && ie.line == cursor.line
            {
                let off = ie.ch - cursor.ch;
                if off > 0 && off < len {
                    last.changes.push(InsertChange::TextOffset(text.clone(), off));
                    continue;
                }
            }
            last.changes.push(InsertChange::Text(text.clone()));
        }
    }

    /// `onCursorActivity`.
    pub(crate) fn on_cursor_activity(&mut self) {
        if self.vim.insert_mode {
            if self.g.macro_mode_state.is_playing {
                return;
            }
            let last = &mut self.g.macro_mode_state.last_insert_mode_changes;
            if last.expect_cursor_activity_for_change {
                last.expect_cursor_activity_for_change = false;
            } else {
                last.maybe_reset = true;
                if let Some(m) = self.vim.insert_end.take() {
                    self.clear_marker(m);
                }
                let c = self.head();
                let m = self.set_bookmark(c, true);
                self.vim.insert_end = Some(m);
            }
        } else if !self.is_vim_op() {
            self.handle_external_selection();
        }
    }

    /// `handleExternalSelection`.
    pub(crate) fn handle_external_selection(&mut self) {
        let mut anchor = self.get_cursor("anchor");
        let mut head = self.head();
        if self.vim.visual_mode && !self.something_selected() {
            self.exit_visual_mode(false);
        } else if !self.vim.visual_mode && !self.vim.insert_mode && self.something_selected() {
            self.vim.visual_mode = true;
            self.vim.visual_line = false;
            self.signal_mode("visual", "");
        }
        if self.vim.visual_mode {
            let head_off = if !before(head, anchor) { -1 } else { 0 };
            let anchor_off = if before(head, anchor) { -1 } else { 0 };
            head = offset(head, 0, head_off);
            anchor = offset(anchor, 0, anchor_off);
            self.vim.sel_anchor = anchor;
            self.vim.sel_head = head;
            self.update_mark('<', pos::min(head, anchor));
            self.update_mark('>', pos::max(head, anchor));
        } else if !self.vim.insert_mode {
            self.vim.last_h_pos = self.head().ch;
        }
    }

    /// `onKeyEventTargetKeyDown`: Backspace / Delete in insert mode are
    /// recorded as keys.
    pub(crate) fn record_insert_key(&mut self, name: &str) {
        if !self.change_listener {
            return;
        }
        if name.contains("Delete") || name.contains("Backspace") {
            let last = &mut self.g.macro_mode_state.last_insert_mode_changes;
            if last.maybe_reset {
                last.changes.clear();
                last.maybe_reset = false;
            }
            last.changes.push(InsertChange::Key(name.to_string()));
        }
    }

    // ---- repeat ----------------------------------------------------------------

    /// `repeatLastEdit(cm, vim, repeat, repeatForInsert)`.
    pub(crate) fn repeat_last_edit(&mut self, repeat: i64, for_insert: bool) {
        self.g.macro_mode_state.is_playing = true;
        let last_action = self.vim.last_edit_action_command.clone();
        // `vim.inputState = vim.lastEditInputState` (shared object: the
        // first command clears vim.inputState, later ones see a fresh one;
        // evalInput writes its arg mutations back, see `replaying_last_edit`)
        let cached = std::mem::replace(
            &mut self.vim.input_state,
            self.vim.last_edit_input_state.clone().unwrap_or_default(),
        );
        let interlace = last_action.as_ref().is_some_and(|a| a.interlace_insert_repeat);
        // cleared by clear_input_state once the shared state is replaced
        self.replaying_last_edit = true;
        let run_cmd = |ed: &mut Editor| match &last_action {
            Some(a) => ed.process_action(a),
            None => ed.eval_input(),
        };
        let repeat_insert = |ed: &mut Editor, n: i64| {
            let changes = ed.g.macro_mode_state.last_insert_mode_changes.changes.clone();
            if !changes.is_empty() {
                let n = if ed.vim.last_edit_action_command.is_none() { 1 } else { n };
                ed.repeat_insert_mode_changes(&changes, n);
            }
        };
        if interlace {
            for _ in 0..repeat {
                run_cmd(self);
                repeat_insert(self, 1);
            }
        } else {
            if !for_insert {
                run_cmd(self);
            }
            repeat_insert(self, repeat);
        }
        self.replaying_last_edit = false;
        self.vim.input_state = cached;
        if self.vim.insert_mode && !for_insert {
            self.exit_insert_mode(false);
        }
        self.g.macro_mode_state.is_playing = false;
    }

    /// `repeatInsertModeChanges(cm, changes, repeat)`.
    pub(crate) fn repeat_insert_mode_changes(&mut self, changes: &[InsertChange], repeat: i64) {
        let head = self.head();
        let vb = self.g.macro_mode_state.last_insert_mode_changes.visual_block;
        let mut repeat = repeat;
        if vb != 0 {
            self.select_for_insert(head, vb + 1);
            repeat = self.sel.len() as i64;
            self.set_cursor(head);
        }
        for i in 0..repeat {
            if vb != 0 {
                self.set_cursor(offset(head, i, 0));
            }
            for change in changes {
                match change {
                    InsertChange::Key(name) => self.send_cm_key(name),
                    InsertChange::Text(t) => self.replace_selection(t),
                    InsertChange::Overwrite(t) => self.replace_change(t, 0),
                    InsertChange::TextOffset(t, o) => self.replace_change(t, *o),
                }
            }
        }
        if vb != 0 {
            self.set_cursor(offset(head, 0, 1));
        }
    }

    fn replace_change(&mut self, text: &str, off: i64) {
        let start = self.head();
        let end = offset(start, 0, clen(text) - off);
        self.replace_range(text, start, Some(if off != 0 { start } else { end }));
        self.set_cursor(end);
    }

    /// `sendCmKey(cm, key)`: run the CodeMirror command bound to `key` in
    /// insert mode (the default keymap).
    pub(crate) fn send_cm_key(&mut self, key: &str) {
        let (mods, base) = match key.rfind('-') {
            Some(i) if i + 1 < key.len() => (&key[..i + 1], &key[i + 1..]),
            _ => ("", key),
        };
        let group = mods.contains("Ctrl-") || mods.contains("Alt-");
        let line = mods.contains("Meta-") || mods.contains("Cmd-");
        match base {
            "Backspace" => {
                if line {
                    self.cmd_delete_to_line_boundary(false)
                } else if group {
                    self.cmd_delete_group(false)
                } else {
                    self.cmd_delete_char(false)
                }
            }
            "Delete" => {
                if line {
                    self.cmd_delete_to_line_boundary(true)
                } else if group {
                    self.cmd_delete_group(true)
                } else {
                    self.cmd_delete_char(true)
                }
            }
            "ArrowLeft" | "Left" => {
                if group {
                    self.cmd_cursor_group(false)
                } else {
                    self.cmd_cursor_char(false)
                }
            }
            "ArrowRight" | "Right" => {
                if group {
                    self.cmd_cursor_group(true)
                } else {
                    self.cmd_cursor_char(true)
                }
            }
            "ArrowUp" | "Up" => self.cmd_cursor_line(false),
            "ArrowDown" | "Down" => self.cmd_cursor_line(true),
            "Home" => self.cmd_cursor_line_boundary(false),
            "End" => self.cmd_cursor_line_boundary(true),
            "Enter" | "Return" => self.cmd_newline_and_indent(true),
            "Insert" => {
                self.overwrite = !self.overwrite;
            }
            "Escape" => {}
            " " | "Space" => self.replace_selection(" "),
            _ => {}
        }
    }
}
