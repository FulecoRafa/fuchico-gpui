//! Key dispatch: the plugin's `handleKey`, `vimKeyFromEvent`,
//! `multiSelectHandleKey`, `findKey`, `commandDispatcher` (`matchCommand`,
//! `processCommand`, `processMotion` … `evalInput`, `recordLastEdit`) and
//! `doKeyToKey`.

use fuchico_helix::{Effect, Key, KeyEvent, Selection};

use crate::editor::Editor;
use crate::keymap::{CmdType, Command, Context, Match, MotionKind, command_match, last_char, split_keys};
use crate::motions::{MotionResult, WordOpts};
use crate::pos::{self, INF, Pos, before};
use crate::prompt::{Prompt, PromptMode};
use crate::search::escape_regex_js;
use crate::state::{Args, InputState, LastSel};
use crate::util::SelMode;

/// `vimKeyFromEvent` for a host key event (`None` = ignored key).
pub fn vim_key_from_event(ev: &KeyEvent) -> Option<String> {
    let key: String = match &ev.key {
        Key::Char(' ') => "Space".into(),
        Key::Char(c) => c.to_string(),
        Key::Escape => "Esc".into(),
        Key::Enter => "CR".into(),
        Key::Backspace => "BS".into(),
        Key::Delete => "Del".into(),
        Key::Tab => "Tab".into(),
        Key::ArrowUp => "Up".into(),
        Key::ArrowDown => "Down".into(),
        Key::ArrowLeft => "Left".into(),
        Key::ArrowRight => "Right".into(),
        Key::Home => "Home".into(),
        Key::End => "End".into(),
        Key::PageUp => "PageUp".into(),
        Key::PageDown => "PageDown".into(),
        Key::F(n) => format!("F{n}"),
        Key::Other(s) => match s.as_str() {
            "Shift" | "Alt" | "Command" | "Control" | "CapsLock" | "AltGraph" | "Dead"
            | "Unidentified" | "Meta" | "" => return None,
            "Insert" => "Ins".into(),
            "Return" | "Enter" => "CR".into(),
            "Escape" => "Esc".into(),
            _ => s.strip_prefix("Numpad").unwrap_or(s).to_string(),
        },
    };
    let m = ev.modifiers;
    let mut name = String::new();
    if m.ctrl {
        name.push_str("C-");
    }
    if m.alt {
        name.push_str("A-");
    }
    if m.meta {
        name.push_str("M-");
    }
    if (!name.is_empty() || key.chars().count() > 1) && m.shift {
        name.push_str("S-");
    }
    name.push_str(&key);
    if name.chars().count() > 1 {
        name = format!("<{name}>");
    }
    Some(name)
}

/// `CodeMirror.keyName(e)`-style name (`Ctrl-Backspace`, `ArrowLeft`).
pub(crate) fn cm_key_name(ev: &KeyEvent) -> String {
    let base = match &ev.key {
        Key::Char(c) => c.to_string(),
        Key::Escape => "Escape".into(),
        Key::Enter => "Enter".into(),
        Key::Backspace => "Backspace".into(),
        Key::Delete => "Delete".into(),
        Key::Tab => "Tab".into(),
        Key::ArrowUp => "ArrowUp".into(),
        Key::ArrowDown => "ArrowDown".into(),
        Key::ArrowLeft => "ArrowLeft".into(),
        Key::ArrowRight => "ArrowRight".into(),
        Key::Home => "Home".into(),
        Key::End => "End".into(),
        Key::PageUp => "PageUp".into(),
        Key::PageDown => "PageDown".into(),
        Key::F(n) => format!("F{n}"),
        Key::Other(s) => s.clone(),
    };
    let m = ev.modifiers;
    let mut name = String::new();
    if m.alt {
        name.push_str("Alt-");
    }
    if m.ctrl {
        name.push_str("Ctrl-");
    }
    if m.meta {
        name.push_str("Cmd-");
    }
    if m.shift && base.chars().count() > 1 {
        name.push_str("Shift-");
    }
    name.push_str(&base);
    name
}

/// `vimToCmKeyMap` (lower-case vim key name -> CodeMirror key name).
pub(crate) fn vim_to_cm_key(lower: &str) -> Option<&'static str> {
    Some(match lower {
        "left" | "arrowleft" => "ArrowLeft",
        "right" | "arrowright" => "ArrowRight",
        "up" | "arrowup" => "ArrowUp",
        "down" | "arrowdown" => "ArrowDown",
        "end" => "End",
        "home" => "Home",
        "cr" | "enter" => "Enter",
        "return" => "Return",
        "bs" | "backspace" => "Backspace",
        "del" | "delete" => "Delete",
        "esc" | "escape" => "Escape",
        "ins" | "insert" => "Insert",
        "space" | " " => " ",
        _ => return None,
    })
}

/// The vim key a synthesized CodeMirror key event maps back to.
fn cm_to_vim_key(cm: &str) -> &'static str {
    match cm {
        "ArrowLeft" => "<Left>",
        "ArrowRight" => "<Right>",
        "ArrowUp" => "<Up>",
        "ArrowDown" => "<Down>",
        "End" => "<End>",
        "Home" => "<Home>",
        "Enter" | "Return" => "<CR>",
        "Backspace" => "<BS>",
        "Delete" => "<Del>",
        "Escape" => "<Esc>",
        "Insert" => "<Ins>",
        _ => "<Space>",
    }
}

/// The last `-`-separated part of a lower-cased `<…>` key.
fn lower_key(key: &str) -> String {
    let inner = if key.len() >= 2 { &key[1..key.len() - 1] } else { "" };
    let lower = inner.to_lowercase();
    lower.rsplit('-').next().unwrap_or("").to_string()
}

pub(crate) enum MatchResult {
    None,
    Partial { expect_literal: bool },
    Clear,
    Full(Box<Command>),
}

/// What `findKey` returns.
enum Found {
    /// `undefined`.
    Unhandled,
    /// A no-op function (`return true`).
    Noop,
    Command(Box<Command>),
}

fn is_count(keys: &str) -> bool {
    let mut c = keys.chars();
    matches!(c.next(), Some('1'..='9')) && c.all(|c| c.is_ascii_digit())
}

impl Editor {
    // ---- plugin -------------------------------------------------------------

    /// The vim plugin's `handleKey` plus CodeMirror's default handling of
    /// keys vim did not take (typing in insert mode).
    pub(crate) fn plugin_handle_key(&mut self, ev: &KeyEvent) {
        self.check_insert_timeout();
        if self.prompt.is_some() {
            self.prompt_key(ev);
            return;
        }
        let Some(key) = vim_key_from_event(ev) else {
            self.emit(Effect::Unhandled);
            return;
        };
        if key == "<Esc>" && !self.vim.insert_mode && !self.vim.visual_mode && self.highlight.is_some()
        {
            self.clear_search_highlight();
        }
        if self.vim.insert_mode {
            let name = cm_key_name(ev);
            self.record_insert_key(&name);
        }
        self.vim.status.push_str(&key);
        let mut result = self.multi_select_handle_key(&key, "user");
        if !result && self.vim.insert_mode && self.overwrite {
            match &ev.key {
                Key::Char(c) if *c != '\n' && !ev.modifiers.ctrl && !ev.modifiers.meta => {
                    result = true;
                    let s = c.to_string();
                    self.operation(false, |ed| ed.overwrite_selection(&s));
                }
                Key::Backspace => {
                    result = true;
                    self.operation(false, |ed| ed.cmd_cursor_char(false));
                }
                _ => {}
            }
        }
        if !result {
            self.default_key(ev);
        }
    }

    /// CodeMirror's own handling of a key vim did not take.
    fn default_key(&mut self, ev: &KeyEvent) {
        if !self.vim.insert_mode {
            self.emit(Effect::Unhandled);
            return;
        }
        let m = ev.modifiers;
        match &ev.key {
            Key::Char(c) if !m.ctrl && !m.meta => {
                let s = c.to_string();
                self.type_text(&s);
            }
            Key::Enter if !m.ctrl && !m.meta && !m.alt => {
                self.operation(false, |ed| ed.cmd_newline_and_indent(true));
            }
            Key::Backspace
            | Key::Delete
            | Key::ArrowLeft
            | Key::ArrowRight
            | Key::ArrowUp
            | Key::ArrowDown
            | Key::Home
            | Key::End => {
                let name = cm_key_name(ev);
                self.operation(false, |ed| ed.send_cm_key(&name));
            }
            _ => self.emit(Effect::Unhandled),
        }
    }

    /// The `insertModeEscKeysTimeout` timer, checked lazily on the next key.
    fn check_insert_timeout(&mut self) {
        if let Some(t) = self.last_partial_ms {
            let timeout = self.get_option_number("insertModeEscKeysTimeout") as f64;
            if self.now_ms() - t >= timeout {
                self.last_partial_ms = None;
                if self.vim.insert_mode && !self.vim.input_state.key_buffer.is_empty() {
                    self.clear_input_state();
                }
            }
        }
    }

    /// `multiSelectHandleKey(cm, key, origin)`.
    pub(crate) fn multi_select_handle_key(&mut self, key: &str, origin: &str) -> bool {
        if self.notification_open {
            self.notification_open = false;
            self.message = None;
            if key == "<CR>" {
                self.clear_input_state();
                return true;
            }
        }
        let was_multi = self.sel.len() > 1;
        if key == "<Esc>"
            && !self.vim.insert_mode
            && !self.vim.visual_mode
            && was_multi
            && self.vim.status == "<Esc>"
        {
            self.clear_input_state();
            // CodeMirror's Escape: simplifySelection
            let main = self.sel.main();
            self.operation(false, |ed| {
                ed.dispatch_tr(None, Some(Selection::single(main.anchor, main.head)), Some("select"))
            });
            return true;
        }
        let handled = self.handle_key_str(key, origin);
        if handled && !self.vim.visual_mode && !self.vim.insert_mode && self.something_selected() {
            self.handle_external_selection();
        }
        handled
    }

    /// `vimApi.handleKey(cm, key, origin)`.
    pub(crate) fn handle_key_str(&mut self, key: &str, origin: &str) -> bool {
        match self.find_key(key, origin) {
            Found::Unhandled => false,
            Found::Noop => true,
            Found::Command(cmd) => {
                self.operation(true, |ed| {
                    if cmd.ty == CmdType::KeyToKey {
                        let ident = command_ident(&cmd);
                        let to = cmd.to_keys.clone().unwrap_or_default();
                        ed.do_key_to_key(&to, Some(ident), cmd.noremap != Some(false));
                    } else {
                        ed.process_command(&cmd);
                    }
                });
                true
            }
        }
    }

    fn handle_esc(&mut self, key: &str) -> bool {
        if key != "<Esc>" {
            return false;
        }
        if self.vim.visual_mode {
            self.exit_visual_mode(true);
        } else if self.vim.insert_mode {
            self.exit_insert_mode(false);
        } else {
            return false;
        }
        self.clear_input_state();
        true
    }

    /// `findKey(cm, key, origin)`.
    fn find_key(&mut self, key: &str, origin: &str) -> Found {
        let res = if self.vim.insert_mode {
            self.find_key_insert(key)
        } else {
            self.find_key_normal(key, origin)
        };
        match res {
            Err(false) => {
                if !self.vim.insert_mode && key.chars().count() == 1 {
                    Found::Noop
                } else {
                    Found::Unhandled
                }
            }
            Err(true) => Found::Noop,
            Ok(cmd) => Found::Command(cmd),
        }
    }

    /// `handleKeyInsertMode` (`Err(bool)` = `return bool`).
    fn find_key_insert(&mut self, key: &str) -> Result<Box<Command>, bool> {
        if self.handle_esc(key) {
            return Err(true);
        }
        self.vim.input_state.key_buffer.push(key.to_string());
        let keys = self.vim.input_state.key_buffer.concat();
        let keys_are_chars = key.chars().count() == 1;
        let m = self.match_command(&keys, Context::Insert);
        let command = match m {
            MatchResult::None => {
                self.clear_input_state();
                return Err(false);
            }
            MatchResult::Partial { expect_literal } => {
                if expect_literal {
                    self.vim.expect_literal_next = true;
                }
                self.last_partial_ms = if keys_are_chars { Some(self.now_ms()) } else { None };
                if keys_are_chars {
                    let sels = self.list_selections();
                    let overwrite = self.overwrite;
                    let texts: Vec<String> = sels
                        .iter()
                        .map(|&(a, h)| {
                            let from = pos::min(a, h);
                            let to = pos::max(a, h);
                            let to = if overwrite { pos::offset(to, 0, 1) } else { to };
                            self.get_range(from, to)
                        })
                        .collect();
                    let is = &mut self.vim.input_state;
                    let recreate = is.change_queue.as_ref().is_none_or(|q| q.removed.len() != sels.len());
                    if recreate {
                        is.change_queue = Some(Default::default());
                    }
                    let q = is.change_queue.as_mut().unwrap();
                    q.inserted.push_str(key);
                    q.removed.resize(sels.len(), String::new());
                    for (i, t) in texts.into_iter().enumerate() {
                        q.removed[i].push_str(&t);
                    }
                }
                return Err(!keys_are_chars);
            }
            MatchResult::Clear => None,
            MatchResult::Full(c) => {
                self.vim.input_state.key_buffer.clear();
                Some(c)
            }
        };
        self.vim.expect_literal_next = false;
        self.last_partial_ms = None;
        let change_queue = self.vim.input_state.change_queue.clone();
        if command.is_some()
            && let Some(q) = change_queue
        {
            let sels = self.list_selections();
            let n = pos::clen(&q.inserted);
            self.operation(false, |ed| {
                for (i, &(_, here)) in sels.iter().enumerate() {
                    let removed = q.removed.get(i).cloned().unwrap_or_default();
                    ed.replace_range(&removed, pos::offset(here, 0, -n), Some(here));
                }
            });
            self.g.macro_mode_state.last_insert_mode_changes.changes.pop();
        }
        match command {
            Some(c) => Ok(c),
            None => {
                self.clear_input_state();
                Err(false)
            }
        }
    }

    /// `handleKeyNonInsertMode`.
    fn find_key_normal(&mut self, key: &str, origin: &str) -> Result<Box<Command>, bool> {
        // handleMacroRecording
        if self.g.macro_mode_state.is_recording {
            if key == "q" {
                self.exit_macro_record_mode();
                self.clear_input_state();
                return Err(true);
            }
            if origin != "mapping" {
                self.log_key(key);
            }
        }
        if self.handle_esc(key) {
            return Err(true);
        }
        self.vim.input_state.key_buffer.push(key.to_string());
        let keys = self.vim.input_state.key_buffer.concat();
        if is_count(&keys) {
            return Err(true);
        }
        let digits: String = keys.chars().take_while(|c| c.is_ascii_digit()).collect();
        let rest: String = keys.chars().skip(digits.chars().count()).collect();
        let context = if self.vim.visual_mode { Context::Visual } else { Context::Normal };
        let mut main_key = if rest.is_empty() { digits.clone() } else { rest };
        if let Some(sc) = &self.vim.input_state.operator_shortcut
            && sc.chars().last().map(String::from).as_deref() == Some(main_key.as_str())
        {
            main_key = sc.clone();
        }
        match self.match_command(&main_key, context) {
            MatchResult::None => {
                self.clear_input_state();
                Err(false)
            }
            MatchResult::Partial { expect_literal } => {
                if expect_literal {
                    self.vim.expect_literal_next = true;
                }
                Err(true)
            }
            MatchResult::Clear => {
                self.clear_input_state();
                Err(true)
            }
            MatchResult::Full(cmd) => {
                self.vim.expect_literal_next = false;
                self.vim.input_state.key_buffer.clear();
                if !digits.is_empty() && digits != "0" {
                    self.vim.input_state.push_repeat_digit(&digits);
                }
                Ok(cmd)
            }
        }
    }

    /// `commandDispatcher.matchCommand(keys, keyMap, inputState, context)`.
    pub(crate) fn match_command(&mut self, keys: &str, context: Context) -> MatchResult {
        let context = if self.vim.input_state.operator.is_some() {
            Context::OperatorPending
        } else {
            context
        };
        let km = &self.g.keymap;
        let start = if self.noremap {
            km.len().saturating_sub(self.g.default_keymap_length)
        } else {
            0
        };
        let has_op = self.vim.input_state.operator.is_some();
        let mut partial: Vec<&Command> = Vec::new();
        let mut full: Option<&Command> = None;
        for cmd in &km[start..] {
            if (context == Context::Insert && cmd.context != Some(Context::Insert))
                || cmd.context.is_some_and(|c| c != context)
                || (has_op && cmd.ty == CmdType::Action)
            {
                continue;
            }
            match command_match(keys, &cmd.keys) {
                Some(Match::Partial) => partial.push(cmd),
                Some(Match::Full) if full.is_none() => full = Some(cmd),
                _ => {}
            }
        }
        let Some(best) = full else {
            if !partial.is_empty() {
                let expect_literal = partial.len() == 1 && partial[0].keys.ends_with("<character>");
                return MatchResult::Partial { expect_literal };
            }
            return MatchResult::None;
        };
        let best = Box::new(best.clone());
        if best.keys.ends_with("<character>") || best.keys.ends_with("<register>") {
            let ch = last_char(keys);
            if ch.is_empty() || ch.chars().count() > 1 {
                return MatchResult::Clear;
            }
            self.vim.input_state.selected_character = Some(ch);
        }
        MatchResult::Full(best)
    }

    // ---- commandDispatcher -----------------------------------------------------

    /// `commandDispatcher.processCommand`.
    pub(crate) fn process_command(&mut self, cmd: &Command) {
        self.vim.input_state.repeat_override = cmd.repeat_override;
        match cmd.ty {
            CmdType::Motion => self.process_motion(cmd),
            CmdType::Operator => self.process_operator(cmd),
            CmdType::OperatorMotion => self.process_operator_motion(cmd),
            CmdType::Action => self.process_action(cmd),
            CmdType::Search => self.process_search(cmd),
            CmdType::Ex | CmdType::KeyToEx => self.process_ex(cmd),
            CmdType::KeyToKey | CmdType::Idle => {}
        }
    }

    pub(crate) fn process_motion(&mut self, cmd: &Command) {
        self.vim.input_state.motion = cmd.motion;
        self.vim.input_state.motion_args = Some(cmd.motion_args.clone().unwrap_or_default());
        self.eval_input();
    }

    pub(crate) fn process_operator(&mut self, cmd: &Command) {
        if let Some(op) = self.vim.input_state.operator {
            if Some(op) == cmd.operator {
                self.vim.input_state.motion = Some(MotionKind::ExpandToLine);
                self.vim.input_state.motion_args = Some(Args {
                    linewise: true,
                    repeat: 1,
                    ..Args::default()
                });
                self.eval_input();
                return;
            }
            self.clear_input_state();
        }
        self.vim.input_state.operator = cmd.operator;
        self.vim.input_state.operator_args = Some(cmd.operator_args.clone().unwrap_or_default());
        if cmd.keys.chars().count() > 1 {
            self.vim.input_state.operator_shortcut = Some(cmd.keys.clone());
        }
        if cmd.exit_visual_block {
            self.vim.visual_block = false;
            self.update_cm_selection();
        }
        if self.vim.visual_mode {
            self.eval_input();
        }
    }

    fn process_operator_motion(&mut self, cmd: &Command) {
        let visual = self.vim.visual_mode;
        if let Some(a) = &cmd.operator_motion_args
            && visual
            && a.visual_line
        {
            self.vim.visual_line = true;
        }
        self.process_operator(cmd);
        if !visual {
            self.process_motion(cmd);
        }
    }

    /// `commandDispatcher.processAction`.
    pub(crate) fn process_action(&mut self, cmd: &Command) {
        let repeat = self.vim.input_state.get_repeat();
        let explicit = repeat != 0;
        let mut args = cmd.action_args.clone().unwrap_or_default();
        if let Some(c) = &self.vim.input_state.selected_character {
            args.selected_character = Some(c.clone());
        }
        if cmd.operator.is_some() {
            self.process_operator(cmd);
        }
        if cmd.motion.is_some() {
            self.process_motion(cmd);
        }
        if cmd.motion.is_some() || cmd.operator.is_some() {
            self.eval_input();
        }
        args.repeat = if repeat != 0 { repeat } else { 1 };
        args.repeat_is_explicit = explicit;
        args.register_name = self.vim.input_state.register_name;
        let is = self.vim.input_state.clone();
        self.clear_input_state();
        self.vim.last_motion = None;
        if cmd.is_edit {
            self.record_last_edit(is, Some(cmd.clone()));
        }
        if let Some(a) = cmd.action {
            self.run_action(a, &mut args);
        }
    }

    /// `commandDispatcher.recordLastEdit`.
    pub(crate) fn record_last_edit(&mut self, is: InputState, cmd: Option<Command>) {
        if self.g.macro_mode_state.is_playing {
            return;
        }
        self.vim.last_edit_input_state = Some(is);
        self.vim.last_edit_action_command = cmd;
        let vb = if self.vim.visual_block {
            self.vim.sel_head.line - self.vim.sel_anchor.line
        } else {
            0
        };
        let l = &mut self.g.macro_mode_state.last_insert_mode_changes;
        l.changes.clear();
        l.expect_cursor_activity_for_change = false;
        l.visual_block = vb;
    }

    /// `commandDispatcher.processSearch`.
    fn process_search(&mut self, cmd: &Command) {
        let sargs = cmd.search_args.clone().unwrap_or_default();
        let forward = sargs.forward;
        self.g.search_is_reversed = !forward;
        let original_query = self.g.search_query.as_ref().map(|q| q.source.clone());
        match sargs.query_src {
            Some("prompt") => {
                if self.g.macro_mode_state.is_playing {
                    let q = if self.g.macro_mode_state.replay_search_queries.is_empty() {
                        String::new()
                    } else {
                        self.g.macro_mode_state.replay_search_queries.remove(0)
                    };
                    self.handle_search_query(&q, true, false, sargs.to_jumplist);
                } else {
                    let prefix = if forward { "/" } else { "?" };
                    self.show_prompt(Prompt {
                        mode: PromptMode::Search {
                            forward,
                            original_query,
                            to_jumplist: sargs.to_jumplist,
                            last_query: String::new(),
                        },
                        value: String::new(),
                        prefix: prefix.to_string(),
                    });
                }
            }
            Some("wordUnderCursor") => {
                let mut keyword = true;
                let mut word =
                    self.expand_word_under_cursor(WordOpts { no_symbol: true, ..WordOpts::default() }, None);
                if word.is_none() {
                    word = self.expand_word_under_cursor(WordOpts::default(), None);
                    keyword = false;
                }
                let Some(word) = word else {
                    self.show_confirm("No word under cursor");
                    self.clear_input_state();
                    return;
                };
                let text = pos::substr(&self.get_line(word.start.line), word.start.ch, word.end.ch);
                let query = if keyword && sargs.whole_word_only {
                    format!("\\b{text}\\b")
                } else {
                    escape_regex_js(&text)
                };
                let cur = self.head();
                self.vim.jump_list.cached_cursor = Some(cur);
                self.set_cursor(word.start);
                self.handle_search_query(&query, true, false, sargs.to_jumplist);
            }
            _ => {}
        }
    }

    /// processSearch's `handleQuery`.
    pub(crate) fn handle_search_query(&mut self, query: &str, ic: bool, sc: bool, to_jumplist: bool) {
        self.g.search_history.push_input(query);
        self.g.search_history.reset();
        if self.update_search_query(query, ic, sc).is_err() {
            self.show_confirm(&format!("Invalid regex: {query}"));
            self.clear_input_state();
            return;
        }
        let mut c = Command::new("", CmdType::Motion);
        c.motion = Some(MotionKind::FindNext);
        c.motion_args = Some(Args {
            forward: true,
            to_jumplist,
            ..Args::default()
        });
        self.process_motion(&c);
    }

    /// `commandDispatcher.processEx`.
    fn process_ex(&mut self, cmd: &Command) {
        if cmd.ty == CmdType::KeyToEx {
            let input = cmd.ex_input.clone().unwrap_or_default();
            self.process_ex_command(&input);
            return;
        }
        let value = if self.vim.visual_mode { "'<,'>".to_string() } else { String::new() };
        self.show_prompt(Prompt {
            mode: PromptMode::Ex,
            value,
            prefix: ":".into(),
        });
    }

    /// `commandDispatcher.evalInput`.
    pub(crate) fn eval_input(&mut self) {
        let is = self.vim.input_state.clone();
        let motion = is.motion;
        let mut motion_args = is.motion_args.clone().unwrap_or(Args {
            repeat: 1,
            ..Args::default()
        });
        let operator = is.operator;
        let mut operator_args = is.operator_args.clone().unwrap_or_default();
        let register_name = is.register_name;
        let orig_head = if self.vim.visual_mode {
            self.clip_cursor_to_content(self.vim.sel_head)
        } else {
            self.get_cursor("head")
        };
        let orig_anchor = if self.vim.visual_mode {
            self.clip_cursor_to_content(self.vim.sel_anchor)
        } else {
            self.get_cursor("anchor")
        };
        let old_head = orig_head;
        let old_anchor = orig_anchor;
        let mut new_head: Option<Pos> = None;
        let mut new_anchor: Option<Pos> = None;
        let playing = self.g.macro_mode_state.is_playing;
        if operator.is_some() {
            self.record_last_edit(is.clone(), None);
        }
        let mut repeat = match is.repeat_override {
            Some(r) => r,
            None => is.get_repeat(),
        };
        if repeat > 0 && motion_args.explicit_repeat {
            motion_args.repeat_is_explicit = true;
        } else if motion_args.no_repeat || (!motion_args.explicit_repeat && repeat == 0) {
            repeat = 1;
            motion_args.repeat_is_explicit = false;
        }
        if let Some(c) = &is.selected_character {
            motion_args.selected_character = Some(c.clone());
            operator_args.selected_character = Some(c.clone());
        }
        motion_args.repeat = repeat;
        let replaying = self.replaying_last_edit;
        self.clear_input_state();

        // `vim.lastEditInputState` shares its arg objects with this run:
        // write the mutations back (lastSel, repeat …).
        let write_back = |ed: &mut Editor, ma: &Args, oa: &Args| {
            if ((operator.is_some() && !playing) || replaying)
                && let Some(l) = ed.vim.last_edit_input_state.as_mut()
            {
                if l.motion_args.is_some() {
                    l.motion_args = Some(ma.clone());
                }
                if l.operator_args.is_some() {
                    l.operator_args = Some(oa.clone());
                }
            }
        };

        if let Some(m) = motion {
            let result = self.run_motion(m, orig_head, &mut motion_args, &is);
            self.vim.last_motion = Some(m);
            let Some(result) = result else {
                write_back(self, &motion_args, &operator_args);
                return;
            };
            if motion_args.to_jumplist {
                let from = self.vim.jump_list.cached_cursor.take().unwrap_or(orig_head);
                self.record_jump_position(from, result.head());
            }
            match result {
                MotionResult::Range(a, h) => {
                    new_anchor = Some(a);
                    new_head = Some(h);
                }
                MotionResult::Head(h) => new_head = Some(h),
            }
            if self.vim.visual_mode {
                let mut h = new_head.unwrap();
                if !(self.vim.visual_block && h.ch >= INF) {
                    h = self.clip_cursor_to_content(h);
                }
                new_head = Some(h);
                let a = match new_anchor {
                    Some(a) => self.clip_cursor_to_content(a),
                    None => old_anchor,
                };
                new_anchor = Some(a);
                self.vim.sel_anchor = a;
                self.vim.sel_head = h;
                self.update_cm_selection();
                self.update_mark('<', if before(a, h) { a } else { h });
                self.update_mark('>', if before(a, h) { h } else { a });
            } else if operator.is_none() {
                let h = self.clip_cursor_to_content(new_head.unwrap());
                new_head = Some(h);
                self.set_cursor(h);
            }
        }
        if let Some(op) = operator {
            if let Some(ls) = operator_args.last_sel {
                new_anchor = Some(old_anchor);
                let lo = (ls.head.line - ls.anchor.line).abs();
                let co = (ls.head.ch - ls.anchor.ch).abs();
                let h = if ls.visual_line {
                    Pos::new(old_anchor.line + lo, old_anchor.ch)
                } else if ls.visual_block {
                    Pos::new(old_anchor.line + lo, old_anchor.ch + co)
                } else if ls.head.line == ls.anchor.line {
                    Pos::new(old_anchor.line, old_anchor.ch + co)
                } else {
                    Pos::new(old_anchor.line + lo, old_anchor.ch)
                };
                new_head = Some(h);
                self.vim.visual_mode = true;
                self.vim.visual_line = ls.visual_line;
                self.vim.visual_block = ls.visual_block;
                self.vim.sel_anchor = old_anchor;
                self.vim.sel_head = h;
                self.update_cm_selection();
            } else if self.vim.visual_mode {
                operator_args.last_sel = Some(LastSel {
                    anchor: self.vim.sel_anchor,
                    head: self.vim.sel_head,
                    visual_block: self.vim.visual_block,
                    visual_line: self.vim.visual_line,
                });
            }
            let linewise;
            let mut cm_sel;
            if self.vim.visual_mode {
                let (sh, sa) = (self.vim.sel_head, self.vim.sel_anchor);
                let start = pos::min(sh, sa);
                let end = pos::max(sh, sa);
                linewise = self.vim.visual_line || operator_args.linewise;
                let mode = if self.vim.visual_block {
                    SelMode::Block
                } else if linewise {
                    SelMode::Line
                } else {
                    SelMode::Char
                };
                cm_sel = self.make_cm_selection(start, end, mode, false);
                if linewise {
                    if mode == SelMode::Block {
                        for r in cm_sel.ranges.iter_mut() {
                            r.1.ch = self.line_length(r.1.line);
                        }
                    } else if mode == SelMode::Line {
                        let h = cm_sel.ranges[0].1;
                        cm_sel.ranges[0].1 = Pos::new(h.line + 1, 0);
                    }
                }
            } else {
                let mut start = new_anchor.unwrap_or(old_anchor);
                let mut end = new_head.unwrap_or(old_head);
                if before(end, start) {
                    std::mem::swap(&mut start, &mut end);
                }
                linewise = motion_args.linewise || operator_args.linewise;
                if linewise {
                    start.ch = 0;
                    end.ch = 0;
                    end.line += 1;
                } else if motion_args.forward {
                    self.clip_to_line(start, &mut end);
                }
                let exclusive = !motion_args.inclusive || linewise;
                cm_sel = self.make_cm_selection(start, end, SelMode::Char, exclusive);
            }
            self.set_selections(&cm_sel.ranges, cm_sel.primary);
            self.vim.last_motion = None;
            operator_args.repeat = repeat;
            operator_args.register_name = register_name;
            operator_args.linewise = linewise;
            let mv = self.run_operator(
                op,
                &mut operator_args,
                &cm_sel.ranges,
                old_anchor,
                new_head.unwrap_or(old_head),
            );
            if self.vim.visual_mode {
                self.exit_visual_mode(mv.is_some());
            }
            if let Some(p) = mv {
                self.set_cursor(p);
            }
        }
        write_back(self, &motion_args, &operator_args);
    }

    // ---- key-to-key mappings -------------------------------------------------------

    /// `doKeyToKey(cm, keys, fromKey)`. `ident` identifies the mapping for
    /// the recursion guard (`None` = a fresh object, like `:normal`'s).
    pub(crate) fn do_key_to_key(&mut self, keys: &str, ident: Option<String>, noremap: bool) {
        let noremap_before = self.noremap;
        if let Some(id) = &ident
            && self.key_to_key_stack.iter().any(|s| s.as_ref() == Some(id))
        {
            return;
        }
        self.key_to_key_stack.push(ident);
        self.noremap = noremap;
        let mut rest: String = keys.to_string();
        while !rest.is_empty() {
            let key = split_keys(&rest).into_iter().next().unwrap_or_default();
            rest = rest[key.len()..].to_string();
            let was_insert = self.vim.insert_mode;
            if self.virtual_prompt.is_some() {
                self.send_key_to_prompt(&key);
                continue;
            }
            let result = self.handle_key_str(&key, "mapping");
            if !result && was_insert && self.vim.insert_mode {
                let mut text = key.clone();
                if key.starts_with('<') && key.len() > 1 {
                    let lower = lower_key(&key);
                    if lower == "lt" {
                        text = "<".into();
                    } else if lower == "space" {
                        text = " ".into();
                    } else if lower == "cr" {
                        text = "\n".into();
                    } else if let Some(cm) = vim_to_cm_key(&lower) {
                        self.send_cm_key(cm);
                        continue;
                    } else {
                        text = "<".into();
                        rest = format!("{}{}", &key[1..], rest);
                    }
                }
                self.replace_selection(&text);
            }
        }
        self.key_to_key_stack.pop();
        self.noremap = if self.key_to_key_stack.is_empty() { false } else { noremap_before };
        if self.key_to_key_stack.is_empty()
            && let Some(p) = self.virtual_prompt.take()
        {
            self.show_prompt(p);
        }
    }

    /// `sendKeyToPrompt(key)` (the virtual prompt of a mapping).
    fn send_key_to_prompt(&mut self, key: &str) {
        let mut key = key.to_string();
        if key.starts_with('<') && key.len() > 1 {
            let lower = lower_key(&key);
            if lower == "lt" {
                key = "<".into();
            } else if lower == "space" {
                key = " ".into();
            } else if lower == "cr" {
                key = "\n".into();
            } else if let Some(cm) = vim_to_cm_key(&lower) {
                let vk = cm_to_vim_key(cm);
                self.prompt_keydown(true, vk);
                if self.virtual_prompt.is_some() {
                    self.prompt_keyup(true, vk);
                }
                return;
            }
        }
        if key == "\n" {
            if let Some(p) = self.virtual_prompt.take() {
                self.prompt_on_close(p);
            }
        } else if let Some(p) = self.virtual_prompt.as_mut() {
            p.value.push_str(&key);
        }
    }

    /// `showPrompt(cm, options)`.
    pub(crate) fn show_prompt(&mut self, p: Prompt) {
        if !self.key_to_key_stack.is_empty() {
            self.virtual_prompt = Some(p);
            return;
        }
        let effect = match &p.mode {
            PromptMode::Ex => Effect::OpenCommandLine {
                placeholder: if p.value.is_empty() { None } else { Some(p.value.clone()) },
            },
            PromptMode::Search { .. } => Effect::OpenSearch {
                kind: fuchico_helix::SearchKind::Forward,
                placeholder: None,
            },
            PromptMode::ReplaceConfirm(_) => Effect::Message {
                text: p.prefix.clone(),
                error: false,
            },
        };
        // openDialog closes notifications
        self.notification_open = false;
        self.prompt = Some(p);
        self.emit(effect);
    }
}

/// A stable identity for a keymap entry (the recursion guard compares
/// command objects).
fn command_ident(c: &Command) -> String {
    format!("{}\u{0}{:?}\u{0}{}", c.keys, c.context, c.to_keys.as_deref().unwrap_or(""))
}
