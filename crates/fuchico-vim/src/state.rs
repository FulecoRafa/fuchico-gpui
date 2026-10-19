//! Vim state: cm-vim `vimState`, `InputState`, registers, history
//! controllers, macro state and the global state shared between editors.

use std::collections::HashMap;

use crate::keymap::{Command, default_keymap};
use crate::pos::{Pos, is_latin_word, is_upper};
use crate::search::Query;

/// Motion / operator / action / search arguments. cm-vim uses loose JS
/// objects; every field any command uses lives here (`false`/`None` =
/// `undefined`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Args {
    pub forward: bool,
    pub linewise: bool,
    pub to_jumplist: bool,
    pub word_end: bool,
    pub big_word: bool,
    pub inclusive: bool,
    pub explicit_repeat: bool,
    pub no_repeat: bool,
    pub to_first_char: bool,
    pub repeat_offset: i64,
    pub same_line: bool,
    pub text_object_inner: bool,
    pub after: bool,
    pub is_edit: bool,
    pub match_indent: bool,
    pub visual_line: bool,
    pub indent_right: bool,
    pub to_lower: Option<bool>,
    pub keep_cursor: bool,
    pub should_move_cursor: bool,
    pub insert_at: Option<&'static str>,
    pub replace: bool,
    pub blockwise: bool,
    pub keep_spaces: bool,
    pub position: Option<&'static str>,
    pub increase: bool,
    pub backtrack: bool,
    pub full_line: bool,
    // filled in at run time
    pub repeat: i64,
    pub repeat_is_explicit: bool,
    pub register_name: Option<char>,
    pub selected_character: Option<String>,
    pub last_sel: Option<LastSel>,
    /// `getSearchCursor` / `*` helpers: `wholeWordOnly`.
    pub whole_word_only: bool,
    /// Search commands: `prompt` / `wordUnderCursor`.
    pub query_src: Option<&'static str>,
    /// `motionArgs.head` (`findNext` from the `*` cursor).
    pub head: Option<Pos>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LastSel {
    pub anchor: Pos,
    pub head: Pos,
    pub visual_block: bool,
    pub visual_line: bool,
}

/// `vim.lastSelection` (marks are bookmarks).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LastSelection {
    pub anchor_mark: usize,
    pub head_mark: usize,
    pub anchor: Pos,
    pub head: Pos,
    pub visual_mode: bool,
    pub visual_line: bool,
    pub visual_block: bool,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ChangeQueue {
    pub removed: Vec<String>,
    pub inserted: String,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct InputState {
    pub prefix_repeat: Vec<String>,
    pub motion_repeat: Vec<String>,
    pub operator: Option<crate::keymap::OperatorKind>,
    pub operator_args: Option<Args>,
    pub motion: Option<crate::keymap::MotionKind>,
    pub motion_args: Option<Args>,
    pub key_buffer: Vec<String>,
    pub register_name: Option<char>,
    pub change_queue: Option<ChangeQueue>,
    pub selected_character: Option<String>,
    pub repeat_override: Option<i64>,
    pub operator_shortcut: Option<String>,
}

impl InputState {
    pub fn push_repeat_digit(&mut self, n: &str) {
        if self.operator.is_none() {
            self.prefix_repeat.push(n.to_string());
        } else {
            self.motion_repeat.push(n.to_string());
        }
    }

    pub fn get_repeat(&self) -> i64 {
        let mut repeat = 0i64;
        if !self.prefix_repeat.is_empty() || !self.motion_repeat.is_empty() {
            repeat = 1;
            if !self.prefix_repeat.is_empty() {
                repeat = repeat.saturating_mul(parse_int(&self.prefix_repeat.concat()));
            }
            if !self.motion_repeat.is_empty() {
                repeat = repeat.saturating_mul(parse_int(&self.motion_repeat.concat()));
            }
        }
        repeat
    }
}

/// `parseInt(s, 10)` for digit strings (saturating instead of float).
pub fn parse_int(s: &str) -> i64 {
    let digits: String = s.chars().take_while(|c| c.is_ascii_digit()).collect();
    digits.parse::<i64>().unwrap_or(if digits.is_empty() { 0 } else { i64::MAX / 4 })
}

/// A recorded insert-mode change (cm-vim `lastInsertModeChanges.changes`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InsertChange {
    /// Plain inserted text (`replaceSelection`).
    Text(String),
    /// `[text]`: overwrite-mode text (`replaceSelection` after extending).
    Overwrite(String),
    /// `[text, offset]`: inserted text with the cursor `offset` chars in.
    TextOffset(String, i64),
    /// An `InsertModeKey` (Backspace, Delete, …) sent to CodeMirror.
    Key(String),
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct InsertModeChanges {
    pub changes: Vec<InsertChange>,
    pub expect_cursor_activity_for_change: bool,
    pub visual_block: i64,
    pub maybe_reset: bool,
    pub ignore_count: i64,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Register {
    pub key_buffer: Vec<String>,
    pub insert_mode_changes: Vec<InsertModeChanges>,
    pub search_queries: Vec<String>,
    pub linewise: bool,
    pub blockwise: bool,
}

impl Register {
    pub fn new(text: &str, linewise: bool, blockwise: bool) -> Register {
        Register {
            key_buffer: vec![text.to_string()],
            insert_mode_changes: Vec::new(),
            search_queries: Vec::new(),
            linewise,
            blockwise,
        }
    }
    pub fn set_text(&mut self, text: &str, linewise: bool, blockwise: bool) {
        self.key_buffer = vec![text.to_string()];
        self.linewise = linewise;
        self.blockwise = blockwise;
    }
    pub fn push_text(&mut self, text: &str, linewise: bool) {
        if linewise {
            if !self.linewise {
                self.key_buffer.push("\n".into());
            }
            self.linewise = true;
        }
        self.key_buffer.push(text.to_string());
    }
    pub fn push_insert_mode_changes(&mut self, c: &InsertModeChanges) {
        self.insert_mode_changes.push(InsertModeChanges {
            changes: c.changes.clone(),
            ..InsertModeChanges::default()
        });
    }
    pub fn push_search_query(&mut self, q: &str) {
        self.search_queries.push(q.to_string());
    }
    pub fn clear(&mut self) {
        self.key_buffer.clear();
        self.insert_mode_changes.clear();
        self.search_queries.clear();
        self.linewise = false;
    }
    pub fn text(&self) -> String {
        self.key_buffer.concat()
    }
}

/// The registers `getRegister` always accepts besides `/^\w$/`.
pub const VALID_REGISTERS: &[char] = &['-', '"', '.', ':', '_', '/', '+'];

#[derive(Debug, Clone, PartialEq)]
pub struct RegisterController {
    pub registers: HashMap<char, Register>,
    /// Pending writes to the system clipboard (`"+`), flushed to the host.
    pub clipboard_writes: Vec<String>,
}

impl Default for RegisterController {
    fn default() -> Self {
        let mut registers = HashMap::new();
        for c in ['"', '.', ':', '/', '+'] {
            registers.insert(c, Register::default());
        }
        RegisterController {
            registers,
            clipboard_writes: Vec::new(),
        }
    }
}

impl RegisterController {
    pub fn is_valid_register(name: Option<char>) -> bool {
        match name {
            Some(c) => VALID_REGISTERS.contains(&c) || is_latin_word(c),
            None => false,
        }
    }

    pub fn unnamed(&mut self) -> &mut Register {
        self.registers.entry('"').or_default()
    }

    /// `getRegister(name)` (creates it).
    pub fn get_register(&mut self, name: Option<char>) -> &mut Register {
        if !Self::is_valid_register(name) {
            return self.unnamed();
        }
        let c = name.unwrap().to_lowercase().next().unwrap_or('"');
        self.registers.entry(c).or_default()
    }

    pub fn push_text(
        &mut self,
        name: Option<char>,
        operator: &str,
        text: &str,
        linewise: bool,
        blockwise: bool,
    ) {
        if name == Some('_') {
            return;
        }
        let mut text = text.to_string();
        if linewise && !text.ends_with('\n') {
            text.push('\n');
        }
        if !Self::is_valid_register(name) {
            match operator {
                "yank" => {
                    self.registers
                        .insert('0', Register::new(&text, linewise, blockwise));
                }
                "delete" | "change" => {
                    if !text.contains('\n') {
                        self.registers.insert('-', Register::new(&text, linewise, false));
                    } else {
                        self.shift_numeric_registers();
                        self.registers.insert('1', Register::new(&text, linewise, false));
                    }
                }
                _ => {}
            }
            self.unnamed().set_text(&text, linewise, blockwise);
            return;
        }
        let name = name.unwrap();
        let append = is_upper(name);
        let reg = self.get_register(Some(name));
        if append {
            reg.push_text(&text, linewise);
        } else {
            reg.set_text(&text, linewise, blockwise);
        }
        let s = reg.text();
        if name == '+' {
            self.clipboard_writes.push(text.clone());
        }
        self.unnamed().set_text(&s, linewise, false);
    }

    fn shift_numeric_registers(&mut self) {
        for i in (2..=9).rev() {
            let prev = char::from_digit(i - 1, 10).unwrap();
            let r = self.registers.entry(prev).or_default().clone();
            self.registers.insert(char::from_digit(i, 10).unwrap(), r);
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HistoryController {
    pub buffer: Vec<String>,
    pub iterator: usize,
    pub initial_prefix: Option<String>,
}

impl HistoryController {
    pub fn next_match(&mut self, input: &str, up: bool) -> Option<String> {
        if self.initial_prefix.is_none() {
            self.initial_prefix = Some(input.to_string());
        }
        let prefix: Vec<char> = self.initial_prefix.clone().unwrap().chars().collect();
        let len = self.buffer.len() as i64;
        let dir: i64 = if up { -1 } else { 1 };
        let mut i = self.iterator as i64 + dir;
        while if up { i >= 0 } else { i < len } {
            let el: Vec<char> = self.buffer[i as usize].chars().collect();
            if el.len() >= prefix.len() && el[..prefix.len()] == prefix[..] {
                self.iterator = i as usize;
                return Some(self.buffer[i as usize].clone());
            }
            i += dir;
        }
        if i >= len {
            self.iterator = self.buffer.len();
            return self.initial_prefix.clone();
        }
        if i < 0 {
            return Some(input.to_string());
        }
        None
    }

    pub fn push_input(&mut self, input: &str) {
        if let Some(i) = self.buffer.iter().position(|x| x == input) {
            self.buffer.remove(i);
        }
        if !input.is_empty() {
            self.buffer.push(input.to_string());
        }
    }

    pub fn reset(&mut self) {
        self.initial_prefix = None;
        self.iterator = self.buffer.len();
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct MacroModeState {
    pub latest_register: Option<char>,
    pub is_playing: bool,
    pub is_recording: bool,
    pub replay_search_queries: Vec<String>,
    pub last_insert_mode_changes: InsertModeChanges,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LastCharacterSearch {
    pub increment: i64,
    pub forward: bool,
    pub selected_character: String,
}

impl Default for LastCharacterSearch {
    fn default() -> Self {
        LastCharacterSearch {
            increment: 0,
            forward: true,
            selected_character: String::new(),
        }
    }
}

/// A Vim option value (`:set`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OptionValue {
    Bool(bool),
    Number(i64),
    Str(String),
}

/// State cm-vim keeps in `vimGlobalState` and module globals. Share it
/// between editors with [`crate::Editor::take_globals`] /
/// [`crate::Editor::set_globals`] (cm-vim has exactly one per page).
#[derive(Debug, Clone)]
pub struct VimGlobals {
    pub search_query: Option<Query>,
    pub search_is_reversed: bool,
    pub last_substitute_replace_part: Option<String>,
    pub macro_mode_state: MacroModeState,
    pub last_character_search: LastCharacterSearch,
    pub register_controller: RegisterController,
    pub search_history: HistoryController,
    pub ex_history: HistoryController,
    /// The keymap; user mappings are prepended (`:map`).
    pub keymap: Vec<Command>,
    pub default_keymap_length: usize,
    /// Global option values.
    pub options: HashMap<String, OptionValue>,
    /// `exCommandDispatcher.commandMap_`.
    pub ex_command_map: HashMap<String, crate::ex::ExCommand>,
}

impl Default for VimGlobals {
    fn default() -> Self {
        let keymap = default_keymap();
        let mut options = HashMap::new();
        options.insert("pcre".to_string(), OptionValue::Bool(true));
        options.insert("insertModeEscKeysTimeout".to_string(), OptionValue::Number(200));
        options.insert("textwidth".to_string(), OptionValue::Number(80));
        VimGlobals {
            search_query: None,
            search_is_reversed: false,
            last_substitute_replace_part: None,
            macro_mode_state: MacroModeState::default(),
            last_character_search: LastCharacterSearch::default(),
            register_controller: RegisterController::default(),
            search_history: HistoryController::default(),
            ex_history: HistoryController::default(),
            default_keymap_length: keymap.len(),
            keymap,
            options,
            ex_command_map: crate::ex::default_ex_command_map(),
        }
    }
}

impl VimGlobals {
    /// Contexts are only used by `:map`/`:unmap`.
    pub fn user_mappings(&self) -> impl Iterator<Item = &Command> {
        self.keymap.iter().filter(|c| c.user)
    }
}

/// `createCircularJumpList` (marks are bookmark ids).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JumpList {
    pub pointer: i64,
    pub head: i64,
    pub tail: i64,
    pub buffer: Vec<Option<usize>>,
    pub cached_cursor: Option<Pos>,
}

pub const JUMP_LIST_SIZE: i64 = 100;

impl Default for JumpList {
    fn default() -> Self {
        JumpList {
            pointer: -1,
            head: 0,
            tail: 0,
            buffer: vec![None; JUMP_LIST_SIZE as usize],
            cached_cursor: None,
        }
    }
}

/// cm-vim `vimState`.
#[derive(Debug, Clone, Default)]
pub struct VimState {
    pub input_state: InputState,
    pub last_edit_input_state: Option<InputState>,
    pub last_edit_action_command: Option<Command>,
    pub last_h_pos: i64,
    pub last_hs_pos: i64,
    pub last_motion: Option<crate::keymap::MotionKind>,
    pub marks: HashMap<char, usize>,
    pub insert_mode: bool,
    pub insert_mode_return: bool,
    pub insert_mode_repeat: Option<i64>,
    pub visual_mode: bool,
    pub visual_line: bool,
    pub visual_block: bool,
    pub last_selection: Option<LastSelection>,
    pub last_pasted_text: Option<String>,
    pub sel_anchor: Pos,
    pub sel_head: Pos,
    pub options: HashMap<String, OptionValue>,
    pub expect_literal_next: bool,
    pub status: String,
    /// `vim.mode` as set by `vim-mode-change` (`normal`, `insert`,
    /// `replace`, `visual`, `visual line`, `visual block`).
    pub mode: &'static str,
    /// `vim.insertEnd` bookmark.
    pub insert_end: Option<usize>,
    /// `vim.searchState_` overlay active (highlight shown).
    pub search_overlay: bool,
    pub jump_list: JumpList,
    /// `vim.exMode` (a `:s///c` confirmation is running).
    pub ex_mode: bool,
}

impl VimState {
    pub fn new() -> VimState {
        VimState {
            last_h_pos: -1,
            last_hs_pos: -1,
            mode: "normal",
            ..VimState::default()
        }
    }
}
