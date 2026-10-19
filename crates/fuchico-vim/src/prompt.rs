//! Prompts: the `/` `?` search prompt, the `:` command line and the
//! `:s///c` confirmation, as cm-vim drives them through `openDialog`
//! (`onKeyDown` / `onKeyUp` / `onClose`), plus the virtual prompt used
//! while a mapping is being replayed.

use fuchico_helix::{Completions, Effect, Key, KeyEvent, PromptKind, Scroll, ScrollAlign, SearchKind};

use crate::dispatch::vim_key_from_event;
use crate::editor::Editor;
use crate::ex::{ExParams, ReplaceState, Stream};
use crate::searchops::option_names;

#[derive(Debug, Clone)]
pub(crate) enum PromptMode {
    Search {
        forward: bool,
        original_query: Option<String>,
        to_jumplist: bool,
        last_query: String,
    },
    Ex,
    ReplaceConfirm(Box<ReplaceState>),
}

#[derive(Debug, Clone)]
pub(crate) struct Prompt {
    pub mode: PromptMode,
    pub value: String,
    pub prefix: String,
}

fn split_words(s: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut rest = s;
    while let Some(i) = rest.find(' ') {
        out.push(&rest[..i]);
        rest = rest[i..].trim_start_matches(' ');
    }
    out.push(rest);
    out
}

impl Editor {
    fn slot(&mut self, virt: bool) -> &mut Option<Prompt> {
        if virt { &mut self.virtual_prompt } else { &mut self.prompt }
    }

    /// The `close(newVal)` passed to prompt handlers.
    pub(crate) fn prompt_close_with(&mut self, virt: bool, value: Option<String>) {
        match value {
            Some(v) => {
                if let Some(p) = self.slot(virt).as_mut() {
                    p.value = v;
                }
            }
            None => {
                *self.slot(virt) = None;
            }
        }
    }

    /// A key typed into the open (real) prompt.
    pub(crate) fn prompt_key(&mut self, ev: &KeyEvent) {
        let Some(key) = vim_key_from_event(ev) else {
            return;
        };
        let stopped = self.prompt_keydown(false, &key);
        if self.prompt.is_none() {
            return;
        }
        if !stopped {
            match key.as_str() {
                "<CR>" => {
                    if let Some(p) = self.prompt.take() {
                        self.operation(true, |ed| ed.prompt_on_close(p));
                    }
                    return;
                }
                "<Esc>" => {
                    self.prompt = None;
                    return;
                }
                "<BS>" => {
                    if let Some(p) = self.prompt.as_mut() {
                        p.value.pop();
                    }
                }
                _ => {
                    let typed = match &ev.key {
                        Key::Char(c) if !ev.modifiers.ctrl && !ev.modifiers.meta => Some(*c),
                        _ => None,
                    };
                    if let (Some(c), Some(p)) = (typed, self.prompt.as_mut()) {
                        p.value.push(c);
                    }
                }
            }
        }
        if self.prompt.is_some() {
            self.prompt_keyup(false, &key);
        }
    }

    /// `onKeyDown(e, value, close)`; `true` = the event was stopped (no
    /// default editing of the input).
    pub(crate) fn prompt_keydown(&mut self, virt: bool, key: &str) -> bool {
        let Some(p) = self.slot(virt).as_ref() else {
            return false;
        };
        let value = p.value.clone();
        match p.mode.clone() {
            PromptMode::Search { original_query, .. } => {
                if key == "<Esc>" || key == "<C-c>" || key == "<C-[>" || (key == "<BS>" && value.is_empty()) {
                    self.g.search_history.push_input(&value);
                    self.g.search_history.reset();
                    let oq = original_query.unwrap_or_default();
                    let _ = self.update_search_query(&oq, false, false);
                    self.clear_search_highlight();
                    self.clear_input_state();
                    self.prompt_close_with(virt, None);
                    return true;
                } else if key == "<C-u>" {
                    self.prompt_close_with(virt, Some(String::new()));
                }
                key == "<Up>" || key == "<Down>" || key == "<C-u>"
            }
            PromptMode::Ex => {
                let mut stop = false;
                if key == "<Esc>" || key == "<C-c>" || key == "<C-[>" || (key == "<BS>" && value.is_empty()) {
                    self.g.ex_history.push_input(&value);
                    self.g.ex_history.reset();
                    self.clear_input_state();
                    self.clear_search_highlight();
                    self.prompt_close_with(virt, None);
                    stop = true;
                }
                if key == "<Up>" || key == "<Down>" {
                    let up = key == "<Up>";
                    let v = self.g.ex_history.next_match(&value, up).unwrap_or_default();
                    self.prompt_close_with(virt, Some(v));
                    stop = true;
                } else if key == "<C-u>" {
                    self.prompt_close_with(virt, Some(String::new()));
                    stop = true;
                } else if !key.is_empty() && key != "<Left>" && key != "<Right>" {
                    self.g.ex_history.reset();
                }
                stop
            }
            PromptMode::ReplaceConfirm(_) => {
                self.replace_confirm_key(virt, key);
                true
            }
        }
    }

    /// `onKeyUp(e, value, close)`; `key` is empty for a plain input change.
    pub(crate) fn prompt_keyup(&mut self, virt: bool, key: &str) {
        let Some(p) = self.slot(virt).as_ref() else {
            return;
        };
        let mut value = p.value.clone();
        match p.mode.clone() {
            PromptMode::Search { forward, .. } => {
                if key == "<Up>" || key == "<Down>" {
                    let up = key == "<Up>";
                    value = self.g.search_history.next_match(&value, up).unwrap_or_default();
                    self.prompt_close_with(virt, Some(value.clone()));
                } else if !key.is_empty() && key != "<Left>" && key != "<Right>" {
                    self.g.search_history.reset();
                }
                if let Some(Prompt {
                    mode: PromptMode::Search { last_query, .. },
                    ..
                }) = self.slot(virt).as_mut()
                {
                    *last_query = value.clone();
                }
                self.search_prompt_on_change(forward, &value);
            }
            PromptMode::Ex => {
                let mut params = ExParams::default();
                let mut stream = Stream::new(&value);
                if self.parse_input(&mut stream, &mut params).is_err() {
                    return;
                }
                if params.command_name.as_deref() != Some("s") {
                    self.clear_search_highlight();
                    return;
                }
                let Some(cmd) = self.match_ex_command("s") else {
                    return;
                };
                Self::parse_command_args(&mut stream, &mut params, &cmd);
                let Some(arg) = params.arg_string.clone() else {
                    return;
                };
                if arg.is_empty() {
                    return;
                }
                let rest: String = arg.chars().skip(1).collect();
                if let Ok(Some(q)) = self.parse_query(&rest, true, true) {
                    self.highlight_search_matches(&q);
                }
            }
            PromptMode::ReplaceConfirm(_) => {}
        }
    }

    /// processSearch's `onChange`.
    fn search_prompt_on_change(&mut self, forward: bool, value: &str) {
        match self.update_search_query(value, true, true) {
            Ok(Some(q)) => {
                if let Some(p) = self.find_next(!forward, &q, 1) {
                    let pos = self.index_from_pos(p);
                    self.scroll = Some(Scroll::To {
                        pos,
                        align: ScrollAlign::Nearest,
                    });
                }
            }
            _ => self.clear_search_highlight(),
        }
    }

    /// `onClose(value)` (Enter).
    pub(crate) fn prompt_on_close(&mut self, p: Prompt) {
        match p.mode {
            PromptMode::Search { to_jumplist, .. } => {
                self.handle_search_query(&p.value, true, true, to_jumplist);
                if self.g.macro_mode_state.is_recording {
                    self.log_search_query(&p.value);
                }
            }
            PromptMode::Ex => {
                self.g.ex_history.push_input(&p.value);
                self.g.ex_history.reset();
                self.process_ex_command(&p.value);
                self.clear_input_state();
                self.clear_search_highlight();
            }
            PromptMode::ReplaceConfirm(_) => {}
        }
    }

    // ---- public prompt API ---------------------------------------------------

    /// The open prompt: [`PromptKind::Command`] for `:`,
    /// [`PromptKind::Search`] for `/` and `?` (see [`Editor::prompt_prefix`]).
    /// `None` while the `:s///c` confirmation is shown (see
    /// [`Editor::confirm_prompt`]).
    pub fn prompt_kind(&self) -> Option<PromptKind> {
        match &self.prompt.as_ref()?.mode {
            PromptMode::Ex => Some(PromptKind::Command),
            PromptMode::Search { .. } => Some(PromptKind::Search(SearchKind::Forward)),
            PromptMode::ReplaceConfirm(_) => None,
        }
    }

    /// The prompt's prefix (`:`, `/`, `?`, or the confirmation text).
    pub fn prompt_prefix(&self) -> Option<&str> {
        self.prompt.as_ref().map(|p| p.prefix.as_str())
    }

    /// The prompt's current text (`'<,'>` when `:` is opened in visual mode).
    pub fn prompt_value(&self) -> Option<&str> {
        let p = self.prompt.as_ref()?;
        match &p.mode {
            PromptMode::ReplaceConfirm(_) => None,
            _ => Some(p.value.as_str()),
        }
    }

    /// `replace with X (y/n/a/q/l)` while `:s///c` waits for a key (feed
    /// keys with [`Editor::handle_key`]).
    pub fn confirm_prompt(&self) -> Option<&str> {
        let p = self.prompt.as_ref()?;
        match &p.mode {
            PromptMode::ReplaceConfirm(_) => Some(p.prefix.as_str()),
            _ => None,
        }
    }

    /// The prompt's text field changed (incremental search highlight,
    /// `:s` pattern highlight).
    pub fn prompt_input(&mut self, value: &str) -> Vec<Effect> {
        let snap = self.snapshot();
        if let Some(p) = self.prompt.as_mut()
            && !matches!(p.mode, PromptMode::ReplaceConfirm(_))
        {
            p.value = value.to_string();
            self.prompt_keyup(false, "");
        }
        self.finish(snap)
    }

    /// Close the prompt: `accept` = Enter, otherwise Escape.
    pub fn prompt_close(&mut self, accept: bool) -> Vec<Effect> {
        let key = if accept { Key::Enter } else { Key::Escape };
        let snap = self.snapshot();
        if self.prompt.is_some() {
            self.prompt_key(&KeyEvent::new(key, Default::default()));
        }
        self.finish(snap)
    }

    /// History navigation (Up = `older`): the new prompt text.
    pub fn prompt_history(&mut self, older: bool) -> Option<String> {
        self.prompt_kind()?;
        let key = if older { "<Up>" } else { "<Down>" };
        self.prompt_keydown(false, key);
        if self.prompt.is_some() {
            self.prompt_keyup(false, key);
        }
        self.prompt.as_ref().map(|p| p.value.clone())
    }

    /// Completion candidates for the prompt text.
    pub fn prompt_completions(&self, value: &str) -> Completions {
        match self.prompt_kind() {
            Some(PromptKind::Search(_)) => {
                let mut options: Vec<String> = Vec::new();
                for h in self.g.search_history.buffer.iter().rev() {
                    if h.starts_with(value) && h != value && !options.contains(h) {
                        options.push(h.clone());
                    }
                }
                Completions { options, help: None }
            }
            Some(PromptKind::Command) => {
                let words = split_words(value);
                if words.len() <= 1 {
                    let trimmed = value.trim_start_matches(':');
                    let start = trimmed.find(|c: char| c.is_ascii_alphabetic()).unwrap_or(trimmed.len());
                    let name = &trimmed[start..];
                    let mut options: Vec<String> = self
                        .ex_command_names()
                        .into_iter()
                        .filter(|n| n.starts_with(name) && n != name)
                        .collect();
                    options.sort();
                    options.dedup();
                    let help = if name.is_empty() {
                        None
                    } else {
                        self.match_ex_command(name).map(|c| format!(":{}", c.name))
                    };
                    Completions { options, help }
                } else {
                    let first = words[0].trim_start_matches(':');
                    let start = first.find(|c: char| c.is_ascii_alphabetic()).unwrap_or(first.len());
                    let cmd = self.match_ex_command(&first[start..]);
                    let last = *words.last().unwrap();
                    let options = match cmd.as_ref().map(|c| c.name.as_str()) {
                        Some("set" | "setlocal" | "setglobal") => option_names()
                            .into_iter()
                            .map(String::from)
                            .filter(|o| o.starts_with(last) && o != last)
                            .collect(),
                        Some("colorscheme") => self
                            .config
                            .themes
                            .iter()
                            .filter(|t| t.starts_with(last) && *t != last)
                            .cloned()
                            .collect(),
                        _ => Vec::new(),
                    };
                    Completions { options, help: None }
                }
            }
            _ => Completions::default(),
        }
    }

    /// The prompt text after accepting `option` (replaces the last word of
    /// a command line, keeping a range prefix such as `%`).
    pub fn prompt_accept_completion(&self, value: &str, option: &str) -> String {
        match self.prompt_kind() {
            Some(PromptKind::Command) => {
                let words = split_words(value);
                let last = words.last().copied().unwrap_or("");
                let head = &value[..value.len() - last.len()];
                if words.len() <= 1 {
                    let lead = last.len() - last.trim_start_matches(|c: char| !c.is_ascii_alphabetic()).len();
                    return format!("{head}{}{option}", &last[..lead]);
                }
                format!("{head}{option}")
            }
            _ => option.to_string(),
        }
    }

    /// Run an Ex command line (without the `:`), as if typed at the `:`
    /// prompt and accepted.
    pub fn execute_command(&mut self, line: &str) -> Vec<Effect> {
        let snap = self.snapshot();
        self.operation(true, |ed| {
            ed.prompt_on_close(Prompt {
                mode: PromptMode::Ex,
                value: line.to_string(),
                prefix: ":".into(),
            })
        });
        self.finish(snap)
    }
}
