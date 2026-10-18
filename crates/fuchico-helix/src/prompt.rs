//! The `:` command line and the search prompts (cm-helix `CommandPanel`
//! `commandInput` / `searchInput` + `startSearch`). The host draws the
//! input; the engine keeps its state and previews results.

use crate::editor::{Editor, Tr};
use crate::effect::{Effect, External, Scroll, SearchKind};
use crate::search::SearchQuery;
use crate::selection::{Range, Selection};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptKind {
    Command,
    Search(SearchKind),
}

/// Autocomplete popup contents.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Completions {
    pub options: Vec<String>,
    pub help: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct Prompt {
    pub kind: PromptKind,
    /// The text typed so far (`None` until the first input for the command
    /// line; search prompts start with `""`).
    pub input: Option<String>,
    pub initial: Selection,
    pub history_entry: isize,
}

/// JS `value.split(/ +/)` (leading/trailing separators give empty fields).
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

fn is_number(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_digit())
}

impl Editor {
    pub(crate) fn open_command_prompt(&mut self) {
        let placeholder = self.registers.get(':').and_then(|v| v.first().cloned());
        self.prompt = Some(Prompt {
            kind: PromptKind::Command,
            input: None,
            initial: self.sel.clone(),
            history_entry: -1,
        });
        self.emit(Effect::OpenCommandLine { placeholder });
    }

    pub(crate) fn open_search(&mut self, kind: SearchKind) {
        let placeholder = self.registers.get('/').map(|v| v.join(","));
        self.prompt = Some(Prompt {
            kind: PromptKind::Search(kind),
            // cm-helix starts with `input = ""`, so accepting right away
            // yanks an empty pattern to `/`.
            input: Some(String::new()),
            initial: self.sel.clone(),
            history_entry: -1,
        });
        self.emit(Effect::OpenSearch { kind, placeholder });
    }

    /// The open prompt, if any.
    pub fn prompt_kind(&self) -> Option<PromptKind> {
        self.prompt.as_ref().map(|p| p.kind)
    }

    /// The prompt's text changed: preview (search) or jump to a line
    /// (`:<n>`).
    pub fn prompt_input(&mut self, value: &str) -> Vec<Effect> {
        self.step(|ed| ed.prompt_input_inner(value))
    }

    fn prompt_input_inner(&mut self, value: &str) {
        let Some(p) = self.prompt.as_mut() else {
            return;
        };
        match p.kind {
            PromptKind::Command => {
                p.input = Some(value.to_string());
                let words = split_words(value);
                let cmd = words[0];
                if is_number(cmd) {
                    let n: usize = cmd.parse().unwrap_or(0);
                    if n >= 1 && n <= self.doc.lines() {
                        let from = self.doc.line(n).from;
                        self.dispatch(Tr::sel(Selection::single(from, from)).centered());
                    }
                }
            }
            PromptKind::Search(kind) => {
                if p.input.as_deref() == Some(value) {
                    return;
                }
                p.input = Some(value.to_string());
                let initial = p.initial.clone();
                let query = SearchQuery::new(value);
                let multi = matches!(
                    kind,
                    SearchKind::Selection
                        | SearchKind::Split
                        | SearchKind::Keep
                        | SearchKind::Remove
                );
                if !query.is_valid() && !multi {
                    return;
                }
                if kind == SearchKind::Global {
                    return;
                }
                if multi {
                    if !query.is_valid() {
                        if value.is_empty() {
                            self.dispatch(Tr::sel(initial));
                        }
                        return;
                    }
                    let ranges = self.multi_search(kind, &query, &initial);
                    let sel = if ranges.is_empty() {
                        initial
                    } else {
                        Selection::create(ranges, 0)
                    };
                    self.dispatch(Tr::sel(sel));
                    return;
                }
                let m = query
                    .first_match(&self.doc, initial.main().to())
                    .or_else(|| query.first_match(&self.doc, 0));
                match m {
                    None => {
                        self.dispatch(Tr::sel(initial));
                        self.emit(Effect::Scroll(Scroll::RestorePromptStart));
                    }
                    Some((f, t)) => self.dispatch(Tr::sel(Selection::single(f, t)).centered()),
                }
            }
        }
    }

    fn multi_search(&self, kind: SearchKind, q: &SearchQuery, initial: &Selection) -> Vec<Range> {
        let mut out = Vec::new();
        for r in initial.ranges() {
            let ms = q.matches(&self.doc, r.from(), r.to());
            match kind {
                SearchKind::Selection => out.extend(ms.iter().map(|&(f, t)| Range::new(f, t))),
                SearchKind::Split => {
                    let mut start = r.from();
                    for (f, t) in ms {
                        if f > start {
                            out.push(Range::new(start, f));
                        }
                        start = t;
                    }
                    if r.to() > start {
                        out.push(Range::new(start, r.to()));
                    }
                }
                SearchKind::Keep if !ms.is_empty() => out.push(*r),
                SearchKind::Remove if ms.is_empty() => out.push(*r),
                _ => {}
            }
        }
        out
    }

    /// Close the prompt: `accept` = Enter, otherwise Escape / blur.
    pub fn prompt_close(&mut self, accept: bool) -> Vec<Effect> {
        self.step(|ed| ed.prompt_close_inner(accept))
    }

    fn prompt_close_inner(&mut self, accept: bool) {
        let Some(p) = self.prompt.take() else {
            return;
        };
        match p.kind {
            PromptKind::Command => {
                let mut value = p.input.unwrap_or_default();
                if accept && !value.is_empty() {
                    self.yank_to(':', vec![value.clone()]);
                } else if accept {
                    value = self
                        .registers
                        .get(':')
                        .and_then(|v| v.first().cloned())
                        .unwrap_or_default();
                }
                let trimmed = value.trim_end();
                let words = split_words(trimmed);
                let cmd = words[0];
                if accept && !cmd.is_empty() {
                    if is_number(cmd) {
                        // Already moved by `prompt_input`; also handle a
                        // command line that was never previewed.
                        self.prompt = Some(Prompt { input: None, ..p });
                        self.prompt_input_inner(cmd);
                        self.prompt = None;
                        return;
                    }
                    let args: Vec<String> = words[1..].iter().map(|s| s.to_string()).collect();
                    let found = self.commands().into_iter().find(|c| c.matches(cmd));
                    let result = match found {
                        Some(info) => self.run_typable(&info, &args),
                        None => Some((format!("no such command: '{cmd}'"), true)),
                    };
                    if let Some((msg, err)) = result {
                        if err {
                            self.error(msg);
                        } else {
                            self.message(msg);
                        }
                    }
                } else if !accept && is_number(cmd) {
                    self.dispatch(Tr::sel(p.initial));
                    self.emit(Effect::Scroll(Scroll::RestorePromptStart));
                }
            }
            PromptKind::Search(kind) => {
                if !accept {
                    self.dispatch(Tr::sel(p.initial));
                    self.emit(Effect::Scroll(Scroll::RestorePromptStart));
                    return;
                }
                if let Some(input) = &p.input {
                    self.yank_to('/', vec![input.clone()]);
                    self.dispatch(Tr::default());
                }
                if kind == SearchKind::Global {
                    let q = match p.input.as_deref() {
                        Some(s) if !s.is_empty() => Some(s.to_string()),
                        _ => self.registers.get('/').map(|v| v.join(",")),
                    };
                    if let Some(q) = q.filter(|q| !q.is_empty())
                        && self.config.externals.global_search
                    {
                        self.emit(Effect::External(External::GlobalSearch(q)));
                    }
                }
            }
        }
    }

    /// Popup contents for the current prompt text (cm-helix `getPopup`).
    pub fn prompt_completions(&self, value: &str) -> Completions {
        match self.prompt_kind() {
            Some(PromptKind::Search(_)) => {
                let mut options: Vec<String> = self
                    .registers
                    .history('/')
                    .iter()
                    .filter(|e| e.starts_with(value))
                    .cloned()
                    .collect();
                options.sort();
                Completions {
                    options,
                    help: None,
                }
            }
            _ => {
                let words = split_words(value);
                let cmd = words[0];
                let commands = self.commands();
                let possible: Vec<_> = commands.iter().filter(|c| c.matches_prefix(cmd)).collect();
                let exact = possible.iter().find(|c| c.matches(cmd));
                let help = exact.map(|c| c.full_help());
                if words.len() > 1
                    && let Some(c) = exact
                    && let Some(options) = self.command_autocomplete(&c.name, &words[1..])
                {
                    return Completions { options, help };
                }
                Completions {
                    options: possible.iter().map(|c| c.name.clone()).collect(),
                    help,
                }
            }
        }
    }

    /// The prompt text after choosing `option` with Tab (cm-helix `accept`:
    /// the command line replaces its last word; search uses the option).
    pub fn prompt_accept_completion(&self, value: &str, option: &str) -> String {
        match self.prompt_kind() {
            Some(PromptKind::Command) => {
                let last = split_words(value).last().copied().unwrap_or("");
                format!("{}{option}", &value[..value.len() - last.len()])
            }
            _ => option.to_string(),
        }
    }

    /// ArrowUp (`older = true`) / ArrowDown in the prompt: the history entry
    /// to show, if any (`:` or `/` register history).
    pub fn prompt_history(&mut self, older: bool) -> Option<String> {
        let reg = match self.prompt_kind()? {
            PromptKind::Command => ':',
            PromptKind::Search(_) => '/',
        };
        let history = self.registers.history(reg).to_vec();
        let p = self.prompt.as_mut()?;
        let next = p.history_entry + if older { 1 } else { -1 };
        if next >= 0 && (next as usize) < history.len() {
            p.history_entry = next;
            return Some(history[history.len() - 1 - next as usize].clone());
        }
        None
    }

    /// `Ctrl-r <reg>` inside the command line: the text to append.
    pub fn prompt_read_register(&mut self, reg: char) -> Result<String, String> {
        Ok(self
            .read_register(Some(reg))?
            .and_then(|v| v.into_iter().next())
            .unwrap_or_default())
    }

    /// Run a command line as if typed after `:` and accepted (for the app's
    /// own palette). Records it in the `:` register.
    pub fn execute_command(&mut self, line: &str) -> Vec<Effect> {
        self.step(|ed| {
            ed.prompt = Some(Prompt {
                kind: PromptKind::Command,
                input: Some(line.to_string()),
                initial: ed.sel.clone(),
                history_entry: -1,
            });
            ed.prompt_close_inner(true);
        })
    }
}
