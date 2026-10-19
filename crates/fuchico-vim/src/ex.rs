//! Ex commands: cm-vim's `ExCommandDispatcher` (`processCommand`,
//! `parseInput_`, `parseLineSpec_`, `matchCommand_`, `map` / `unmap`),
//! `exCommands`, `doReplace` and `vimApi.mapclear`.

use std::collections::HashMap;
use std::sync::LazyLock;

use fuchico_helix::{Change, ChangeSet, Effect};
use regex::Regex;

use crate::editor::Editor;
use crate::keymap::{ActionKind, CmdType, Command, Context, MotionKind};
use crate::pos::{INF, Pos};
use crate::prompt::{Prompt, PromptMode};
use crate::search::{Query, SearchCursor, split_by_separator, translate_regex_replace, unescape_regex_replace};
use crate::searchops::Scope;
use crate::state::{Args, OptionValue, RegisterController};

/// What an entry of the Ex command map does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExKind {
    /// One of `exCommands` (including `defineEx` commands such as `version`).
    Builtin,
    /// `:map :foo :bar` (Ex to Ex).
    ExToEx(String),
    /// `:map :foo keys` (Ex to keys).
    ExToKey(String),
}

/// An entry of `exCommandDispatcher.commandMap_`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExCommand {
    pub name: String,
    pub short_name: Option<String>,
    pub kind: ExKind,
    pub user: bool,
    pub possibly_async: bool,
    pub exclude_from_history: bool,
}

/// `defaultExCommandMap` (+ `defineEx("version", "ve")` and Fuchico's
/// `:quit` / `:wq` / `:xit`).
pub(crate) const DEFAULT_EX_COMMANDS: &[(&str, Option<&str>)] = &[
    ("colorscheme", Some("colo")),
    ("map", None),
    ("imap", Some("im")),
    ("nmap", Some("nm")),
    ("vmap", Some("vm")),
    ("omap", Some("om")),
    ("noremap", Some("no")),
    ("nnoremap", Some("nn")),
    ("vnoremap", Some("vn")),
    ("inoremap", Some("ino")),
    ("onoremap", Some("ono")),
    ("unmap", None),
    ("mapclear", Some("mapc")),
    ("nmapclear", Some("nmapc")),
    ("vmapclear", Some("vmapc")),
    ("imapclear", Some("imapc")),
    ("omapclear", Some("omapc")),
    ("write", Some("w")),
    ("undo", Some("u")),
    ("redo", Some("red")),
    ("set", Some("se")),
    ("setlocal", Some("setl")),
    ("setglobal", Some("setg")),
    ("sort", Some("sor")),
    ("substitute", Some("s")),
    ("startinsert", Some("start")),
    ("nohlsearch", Some("noh")),
    ("yank", Some("y")),
    ("delmarks", Some("delm")),
    ("marks", None),
    ("registers", Some("reg")),
    ("vglobal", Some("v")),
    ("delete", Some("d")),
    ("join", Some("j")),
    ("normal", Some("norm")),
    ("global", Some("g")),
    ("version", Some("ve")),
    // Fuchico extras (not in cm-vim; `Config::app_commands`)
    ("quit", Some("q")),
    ("wq", None),
    ("xit", Some("x")),
];

/// Fuchico's extra commands (gated by `Config::app_commands`).
pub(crate) const EXTRA_EX_COMMANDS: &[&str] = &["quit", "wq", "xit"];

pub(crate) fn default_ex_command_map() -> HashMap<String, ExCommand> {
    let mut m = HashMap::new();
    for &(name, short) in DEFAULT_EX_COMMANDS {
        let key = short.unwrap_or(name).to_string();
        m.insert(
            key,
            ExCommand {
                name: name.to_string(),
                short_name: short.map(String::from),
                kind: ExKind::Builtin,
                user: false,
                possibly_async: name == "substitute",
                exclude_from_history: name == "marks" || name == "registers",
            },
        );
    }
    m
}

/// CodeMirror's `StringStream` (positions may go negative after
/// `backUp` at end of input, like the original).
pub(crate) struct Stream {
    s: Vec<char>,
    pos: i64,
}

impl Stream {
    pub fn new(s: &str) -> Stream {
        Stream { s: s.chars().collect(), pos: 0 }
    }
    fn len(&self) -> i64 {
        self.s.len() as i64
    }
    pub fn eol(&self) -> bool {
        self.pos >= self.len()
    }
    fn char_at(&self, i: i64) -> Option<char> {
        if i < 0 { None } else { self.s.get(i as usize).copied() }
    }
    pub fn next(&mut self) -> Option<char> {
        if self.pos < self.len() {
            let c = self.char_at(self.pos);
            self.pos += 1;
            c
        } else {
            None
        }
    }
    pub fn back_up(&mut self, n: i64) {
        self.pos -= n;
    }
    pub fn eat(&mut self, c: char) -> bool {
        if self.char_at(self.pos) == Some(c) {
            self.pos += 1;
            true
        } else {
            false
        }
    }
    pub fn eat_while(&mut self, c: char) -> bool {
        let start = self.pos;
        while self.eat(c) {}
        self.pos > start
    }
    pub fn eat_space(&mut self) -> bool {
        let start = self.pos;
        while self.char_at(self.pos).is_some_and(|c| c.is_whitespace()) {
            self.pos += 1;
        }
        self.pos > start
    }
    /// `string.slice(pos)` (negative = from the end).
    pub fn rest(&self) -> String {
        let n = self.len();
        let start = if self.pos < 0 { (n + self.pos).max(0) } else { self.pos.min(n) };
        self.s[start as usize..].iter().collect()
    }
    /// `match(re)` (anchored at the current position, consumes the match).
    pub fn match_re(&mut self, re: &Regex) -> Option<Vec<Option<String>>> {
        let rest = self.rest();
        let caps = re.captures(&rest)?;
        let m0 = caps.get(0)?;
        if m0.start() > 0 {
            return None;
        }
        let groups = (0..caps.len()).map(|i| caps.get(i).map(|m| m.as_str().to_string())).collect();
        // consume (from the clamped start, like `pos += match[0].length`)
        self.pos += m0.as_str().chars().count() as i64;
        Some(groups)
    }
    pub fn peek_matches(&self, f: impl Fn(char) -> bool) -> bool {
        self.char_at(self.pos.max(0)).is_some_and(f) && self.pos >= 0
    }
    pub fn pos(&self) -> i64 {
        self.pos
    }
}

static RE_NUMBER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(\d+)").unwrap());
static RE_OFFSET: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^([+-])?(\d+)").unwrap());
static RE_CMD_NAME: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^([A-Za-z0-9_]+|!!|@@|[!#&*<=>@~])").unwrap());
static RE_REST: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^.*").unwrap());
static RE_SORT_OPTS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^([dinuox]+)?\s*(/.+/)?\s*").unwrap());

/// `exCommandArgs` / `ExParams`.
#[derive(Debug, Clone, Default)]
pub(crate) struct ExParams {
    pub input: String,
    pub line: Option<i64>,
    pub line_end: Option<i64>,
    pub selection_line: Option<i64>,
    pub selection_line_end: Option<i64>,
    pub command_name: Option<String>,
    pub arg_string: Option<String>,
    pub args: Option<Vec<String>>,
    /// The `:global` chain to continue when the command is done.
    pub callback: Option<usize>,
}

/// A running `:global` command chain (`nextCommand`).
#[derive(Debug, Clone, Default)]
pub struct GlobalChain {
    handles: Vec<usize>,
    index: usize,
    cmd: String,
    pending: usize,
    running: bool,
}

/// `doReplace`'s closure state.
#[derive(Debug, Clone)]
pub struct ReplaceState {
    confirm: bool,
    global: bool,
    line_start: i64,
    line_end: i64,
    cursor: SearchCursor,
    query: Query,
    replace_with: String,
    callback: Option<usize>,
    done: bool,
    matches: usize,
    last_pos: Option<Pos>,
    modified_line: Option<i64>,
    joined: bool,
}

fn js_truthy(n: Option<i64>) -> Option<i64> {
    n.filter(|&x| x != 0)
}

fn parse_i64(s: &str) -> i64 {
    s.parse::<i64>().unwrap_or(INF)
}

/// JavaScript `parseInt(s)` (leading integer, `None` = NaN).
fn parse_int_js(s: &str) -> Option<i64> {
    let t = s.trim_start();
    let (neg, digits) = match t.strip_prefix('-') {
        Some(r) => (true, r),
        None => (false, t.strip_prefix('+').unwrap_or(t)),
    };
    let d: String = digits.chars().take_while(|c| c.is_ascii_digit()).collect();
    if d.is_empty() {
        return None;
    }
    let v = parse_i64(&d);
    Some(if neg { -v } else { v })
}

/// `parseInt(s, radix)` as a float (leading valid digits).
fn parse_radix(s: &str, radix: u32) -> f64 {
    let (neg, rest) = match s.strip_prefix('-') {
        Some(r) => (true, r),
        None => (false, s),
    };
    let rest = if radix == 16 {
        rest.strip_prefix("0x").or_else(|| rest.strip_prefix("0X")).unwrap_or(rest)
    } else {
        rest
    };
    let mut v = 0f64;
    let mut any = false;
    for c in rest.chars() {
        match c.to_digit(radix) {
            Some(d) => {
                v = v * radix as f64 + d as f64;
                any = true;
            }
            None => break,
        }
    }
    if !any {
        return f64::NAN;
    }
    if neg { -v } else { v }
}

/// `new RegExp(src).source`: escape unescaped `/` outside classes and
/// line terminators. `Err` = the `SyntaxError` message.
fn normalize_regex_source(src: &str) -> Result<String, String> {
    if let Err(e) = Query::new(src, false) {
        return Err(format!("SyntaxError: Invalid regular expression: /{src}/: {e}"));
    }
    let mut out = String::new();
    let mut escaped = false;
    let mut in_class = false;
    for c in src.chars() {
        if escaped {
            out.push(c);
            escaped = false;
            continue;
        }
        match c {
            '\\' => {
                escaped = true;
                out.push(c);
            }
            '[' => {
                in_class = true;
                out.push(c);
            }
            ']' => {
                in_class = false;
                out.push(c);
            }
            '/' if !in_class => out.push_str("\\/"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            _ => out.push(c),
        }
    }
    Ok(out)
}

/// doReplace's `replaceWith.replace(/\$(\d{1,3}|[$&])/g, …)`.
fn expand_do_replace(rep: &str, groups: &[Option<String>]) -> String {
    let c: Vec<char> = rep.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < c.len() {
        if c[i] == '$' && i + 1 < c.len() {
            let n = c[i + 1];
            if n == '$' {
                out.push('$');
                i += 2;
                continue;
            }
            if n == '&' {
                out.push_str(groups.first().and_then(|g| g.as_deref()).unwrap_or(""));
                i += 2;
                continue;
            }
            if n.is_ascii_digit() {
                let mut j = i + 1;
                while j < c.len() && j < i + 4 && c[j].is_ascii_digit() {
                    j += 1;
                }
                let x: String = c[i + 1..j].iter().collect();
                let mut x1 = x.clone();
                while !x1.is_empty() && x1.parse::<usize>().unwrap_or(usize::MAX) >= groups.len() {
                    x1.pop();
                }
                if !x1.is_empty() {
                    let g = x1.parse::<usize>().unwrap();
                    out.push_str(groups[g].as_deref().unwrap_or(""));
                    out.push_str(&x[x1.len()..]);
                } else {
                    out.push('$');
                    out.push_str(&x);
                }
                i = j;
                continue;
            }
        }
        out.push(c[i]);
        i += 1;
    }
    out
}

fn context_from_name(s: &str) -> Option<Context> {
    match s {
        "normal" => Some(Context::Normal),
        "visual" => Some(Context::Visual),
        "insert" => Some(Context::Insert),
        "operatorPending" => Some(Context::OperatorPending),
        _ => None,
    }
}

impl Editor {
    // ---- dispatcher -------------------------------------------------------------

    /// `exCommandDispatcher.processCommand(cm, input)`.
    pub(crate) fn process_ex_command(&mut self, input: &str) {
        self.process_ex_command_cb(input, None);
    }

    pub(crate) fn process_ex_command_cb(&mut self, input: &str, callback: Option<usize>) {
        self.operation(true, |ed| ed.process_ex_inner(input, callback));
    }

    fn process_ex_inner(&mut self, input: &str, callback: Option<usize>) {
        let prev = self.g.register_controller.get_register(Some(':')).text();
        self.g
            .register_controller
            .get_register(Some(':'))
            .set_text(input, false, false);
        let mut params = ExParams {
            input: input.to_string(),
            callback,
            ..ExParams::default()
        };
        let mut stream = Stream::new(input);
        if let Err(e) = self.parse_input(&mut stream, &mut params) {
            self.show_confirm(&e);
            return;
        }
        if self.vim.visual_mode {
            self.exit_visual_mode(true);
        }
        let mut command: Option<ExCommand> = None;
        let mut command_name: Option<String> = None;
        match params.command_name.clone().filter(|s| !s.is_empty()) {
            None => {
                if params.line.is_some() {
                    command_name = Some("move".into());
                }
            }
            Some(cn) => {
                if let Some(cmd) = self.match_ex_command(&cn) {
                    command_name = Some(cmd.name.clone());
                    if cmd.exclude_from_history {
                        self.g
                            .register_controller
                            .get_register(Some(':'))
                            .set_text(&prev, false, false);
                    }
                    Self::parse_command_args(&mut stream, &mut params, &cmd);
                    match &cmd.kind {
                        ExKind::ExToKey(keys) => {
                            let ident = format!(":{}", cmd.name);
                            self.do_key_to_key(keys, Some(ident), true);
                            return;
                        }
                        ExKind::ExToEx(to) => {
                            self.process_ex_command(to);
                            return;
                        }
                        ExKind::Builtin => {}
                    }
                    command = Some(cmd);
                }
            }
        }
        let name = command_name.filter(|n| {
            self.config.app_commands || !EXTRA_EX_COMMANDS.contains(&n.as_str())
        });
        let Some(name) = name else {
            self.show_confirm(&format!("Not an editor command \":{input}\""));
            return;
        };
        match self.run_ex_command(&name, &mut params) {
            Err(e) => self.show_confirm(&e),
            Ok(()) => {
                let is_async = command.as_ref().is_some_and(|c| c.possibly_async);
                if !is_async && let Some(cb) = params.callback {
                    self.ex_callback(cb);
                }
            }
        }
    }

    /// `parseInput_`. `Err` = the thrown error's text.
    pub(crate) fn parse_input(&mut self, stream: &mut Stream, p: &mut ExParams) -> Result<(), String> {
        stream.eat_while(':');
        if stream.eat('%') {
            p.line = Some(self.first_line());
            p.line_end = Some(self.last_line());
        } else {
            p.line = self.parse_line_spec(stream)?;
            if p.line.is_some() && stream.eat(',') {
                p.line_end = self.parse_line_spec(stream)?;
            }
        }
        if p.line.is_none() {
            if self.vim.visual_mode {
                p.selection_line = self.get_mark_pos('<').map(|m| m.line);
                p.selection_line_end = self.get_mark_pos('>').map(|m| m.line);
            } else {
                p.selection_line = Some(self.get_cursor("head").line);
            }
        } else {
            p.selection_line = p.line;
            p.selection_line_end = p.line_end;
        }
        p.command_name = match stream.match_re(&RE_CMD_NAME) {
            Some(g) => g[1].clone(),
            None => stream.match_re(&RE_REST).and_then(|g| g[0].clone()).or(Some(String::new())),
        };
        Ok(())
    }

    fn parse_line_spec(&mut self, stream: &mut Stream) -> Result<Option<i64>, String> {
        if let Some(g) = stream.match_re(&RE_NUMBER) {
            return Ok(Some(parse_i64(g[1].as_deref().unwrap_or("0")) - 1));
        }
        let c = stream.next();
        match c {
            Some('.') => {
                let l = self.get_cursor("head").line;
                Ok(Some(Self::parse_line_spec_offset(stream, l)))
            }
            Some('$') => {
                let l = self.last_line();
                Ok(Some(Self::parse_line_spec_offset(stream, l)))
            }
            Some('\'') => {
                let name = stream.next();
                let mp = name.and_then(|n| self.get_mark_pos(n));
                let Some(mp) = mp else {
                    return Err("Error: Mark not set".into());
                };
                Ok(Some(Self::parse_line_spec_offset(stream, mp.line)))
            }
            Some('-') | Some('+') => {
                stream.back_up(1);
                let l = self.get_cursor("head").line;
                Ok(Some(Self::parse_line_spec_offset(stream, l)))
            }
            _ => {
                stream.back_up(1);
                Ok(None)
            }
        }
    }

    fn parse_line_spec_offset(stream: &mut Stream, line: i64) -> i64 {
        if let Some(g) = stream.match_re(&RE_OFFSET) {
            let off = parse_i64(g[2].as_deref().unwrap_or("0"));
            if g[1].as_deref() == Some("-") {
                return line - off;
            }
            return line + off;
        }
        line
    }

    /// `parseCommandArgs_`.
    pub(crate) fn parse_command_args(stream: &mut Stream, p: &mut ExParams, _cmd: &ExCommand) {
        if stream.eol() {
            return;
        }
        p.arg_string = stream.match_re(&RE_REST).and_then(|g| g[0].clone());
        let args: Vec<String> = p
            .arg_string
            .as_deref()
            .unwrap_or("")
            .trim()
            .split(char::is_whitespace)
            .filter(|s| !s.is_empty())
            .map(String::from)
            .collect();
        if !args.is_empty() {
            p.args = Some(args);
        }
    }

    /// `matchCommand_(commandName)`.
    pub(crate) fn match_ex_command(&self, name: &str) -> Option<ExCommand> {
        let chars: Vec<char> = name.chars().collect();
        for i in (1..=chars.len()).rev() {
            let prefix: String = chars[..i].iter().collect();
            if let Some(c) = self.g.ex_command_map.get(&prefix)
                && c.name.starts_with(name)
            {
                return Some(c.clone());
            }
        }
        None
    }

    /// All Ex command names (for completion).
    pub(crate) fn ex_command_names(&self) -> Vec<String> {
        self.g
            .ex_command_map
            .values()
            .filter(|c| self.config.app_commands || !EXTRA_EX_COMMANDS.contains(&c.name.as_str()))
            .map(|c| c.name.clone())
            .collect()
    }

    /// `params.callback()` of a `:global` chain (`nextCommand`).
    fn ex_callback(&mut self, id: usize) {
        let Some(c) = self.ex_chains.get_mut(&id) else {
            return;
        };
        c.pending += 1;
        if c.running {
            return;
        }
        c.running = true;
        loop {
            let Some(c) = self.ex_chains.get_mut(&id) else {
                return;
            };
            if c.pending == 0 {
                c.running = false;
                return;
            }
            c.pending -= 1;
            if c.index < c.handles.len() {
                let h = c.handles[c.index];
                c.index += 1;
                let cmd = c.cmd.clone();
                match self.find_marker(h) {
                    None => {
                        if let Some(c) = self.ex_chains.get_mut(&id) {
                            c.pending += 1;
                        }
                    }
                    Some(p) => {
                        let command = format!("{}{}", p.line + 1, cmd);
                        self.process_ex_command_cb(&command, Some(id));
                    }
                }
            } else {
                let handles = std::mem::take(&mut c.handles);
                self.ex_chains.remove(&id);
                for h in handles {
                    self.clear_marker(h);
                }
                return;
            }
        }
    }

    // ---- mappings ---------------------------------------------------------------

    /// `exCommandDispatcher.map(lhs, rhs, ctx, noremap)`.
    pub(crate) fn ex_map(&mut self, lhs: &str, rhs: &str, ctx: Option<Context>, noremap: bool) -> Result<(), String> {
        if lhs != ":" && lhs.starts_with(':') {
            if ctx.is_some() {
                return Err("Error: Mode not supported for ex mappings".into());
            }
            let name = lhs[1..].to_string();
            let kind = if rhs != ":" && rhs.starts_with(':') {
                ExKind::ExToEx(rhs[1..].to_string())
            } else {
                ExKind::ExToKey(rhs.to_string())
            };
            self.g.ex_command_map.insert(
                name.clone(),
                ExCommand {
                    name,
                    short_name: None,
                    kind,
                    user: true,
                    possibly_async: false,
                    exclude_from_history: false,
                },
            );
        } else {
            let mut c = Command::new(lhs, CmdType::KeyToKey);
            c.to_keys = Some(rhs.to_string());
            c.noremap = Some(noremap);
            c.context = ctx;
            c.user = true;
            self.g.keymap.insert(0, c);
        }
        Ok(())
    }

    /// `exCommandDispatcher.unmap(lhs, ctx)`.
    pub(crate) fn ex_unmap(&mut self, lhs: &str, ctx: Option<Context>) -> Result<bool, String> {
        if lhs != ":" && lhs.starts_with(':') {
            if ctx.is_some() {
                return Err("Error: Mode not supported for ex mappings".into());
            }
            let name = &lhs[1..];
            if self.g.ex_command_map.get(name).is_some_and(|c| c.user) {
                self.g.ex_command_map.remove(name);
                return Ok(true);
            }
            return Ok(false);
        }
        if let Some(i) = self.g.keymap.iter().position(|c| c.keys == lhs && c.context == ctx) {
            self.g.keymap.remove(i);
            return Ok(true);
        }
        Ok(false)
    }

    /// `vimApi.mapclear(ctx)`.
    pub(crate) fn mapclear(&mut self, ctx: Option<Context>) {
        let n = self.g.keymap.len().saturating_sub(self.g.default_keymap_length);
        let user: Vec<Command> = self.g.keymap.drain(..n).collect();
        if let Some(ctx) = ctx {
            for m in user.into_iter().rev() {
                if m.context == Some(ctx) {
                    continue;
                }
                if m.context.is_some() {
                    self.g.keymap.insert(0, m);
                } else {
                    for c in [Context::Normal, Context::Insert, Context::Visual] {
                        if c != ctx {
                            let mut nm = m.clone();
                            nm.context = Some(c);
                            self.g.keymap.insert(0, nm);
                        }
                    }
                }
            }
        }
    }

    /// `showConfirm(cm, text, true)` (a notification that stays open until
    /// the next key; Enter dismisses it).
    pub(crate) fn show_confirm_long(&mut self, text: &str) {
        self.show_confirm(&format!("{text}\nPress ENTER or type command to continue"));
        self.notification_open = true;
    }

    // ---- commands -----------------------------------------------------------------

    fn run_ex_command(&mut self, name: &str, p: &mut ExParams) -> Result<(), String> {
        let ctx_of = |n: &str| match n.chars().next() {
            Some('i') => Some(Context::Insert),
            Some('n') if !n.starts_with("noremap") => Some(Context::Normal),
            Some('v') => Some(Context::Visual),
            Some('o') => Some(Context::OperatorPending),
            _ => None,
        };
        match name {
            "colorscheme" => {
                match p.args.as_ref().and_then(|a| a.first()) {
                    None => {
                        let t = self.config.theme.clone().unwrap_or_else(|| "default".into());
                        self.show_confirm(&t);
                    }
                    Some(t) => {
                        self.config.theme = Some(t.clone());
                        self.emit(Effect::ChangeTheme(t.clone()));
                    }
                }
                Ok(())
            }
            "map" | "imap" | "nmap" | "vmap" | "omap" | "noremap" | "inoremap" | "nnoremap"
            | "vnoremap" | "onoremap" => {
                let noremap = name.contains("noremap");
                let ctx = ctx_of(name);
                let args = p.args.clone().unwrap_or_default();
                if args.len() < 2 {
                    self.show_confirm(&format!("Invalid mapping: {}", p.input));
                    return Ok(());
                }
                self.ex_map(&args[0], &args[1], ctx, noremap)
            }
            "unmap" => {
                let ok = match p.args.as_ref().and_then(|a| a.first()) {
                    Some(lhs) => self.ex_unmap(&lhs.clone(), None)?,
                    None => false,
                };
                if !ok {
                    self.show_confirm(&format!("No such mapping: {}", p.input));
                }
                Ok(())
            }
            "mapclear" => {
                self.mapclear(None);
                Ok(())
            }
            "imapclear" | "nmapclear" | "vmapclear" | "omapclear" => {
                self.mapclear(ctx_of(name));
                Ok(())
            }
            "move" => {
                let mut c = Command::new("", CmdType::Motion);
                c.motion = Some(MotionKind::MoveToLineOrEdgeOfDocument);
                c.motion_args = Some(Args {
                    forward: false,
                    explicit_repeat: true,
                    linewise: true,
                    ..Args::default()
                });
                c.repeat_override = Some(p.line.unwrap_or(0) + 1);
                self.process_command(&c);
                Ok(())
            }
            "set" => self.ex_set(p, Scope::Any),
            "setlocal" => self.ex_set(p, Scope::Local),
            "setglobal" => self.ex_set(p, Scope::Global),
            "registers" => {
                self.ex_registers(p);
                Ok(())
            }
            "marks" => {
                self.ex_marks(p);
                Ok(())
            }
            "sort" => self.ex_sort(p),
            "global" | "vglobal" => self.ex_global(p),
            "normal" => {
                self.ex_normal(p);
                Ok(())
            }
            "substitute" => self.ex_substitute(p),
            "startinsert" => {
                let k = if p.arg_string.as_deref() == Some("!") { "A" } else { "i" };
                self.do_key_to_key(k, None, true);
                Ok(())
            }
            "redo" => {
                self.run_history_command(true);
                Ok(())
            }
            "undo" => {
                self.run_history_command(false);
                Ok(())
            }
            "write" => {
                self.emit(Effect::Save);
                Ok(())
            }
            "quit" => {
                self.emit(Effect::Quit);
                Ok(())
            }
            "wq" | "xit" => {
                self.emit(Effect::Save);
                self.emit(Effect::Quit);
                Ok(())
            }
            "nohlsearch" => {
                self.clear_search_highlight();
                Ok(())
            }
            "yank" => {
                let line = self.get_cursor("head").line;
                let text = self.get_line(line);
                self.g
                    .register_controller
                    .push_text(Some('0'), "yank", &text, true, true);
                Ok(())
            }
            "delete" => {
                let line = p.selection_line.unwrap_or_else(|| self.get_cursor("head").line);
                let end = p.selection_line_end.unwrap_or(line);
                let mut args = Args {
                    linewise: true,
                    ..Args::default()
                };
                self.op_delete(&mut args, &[(Pos::new(line, 0), Pos::new(end + 1, 0))]);
                Ok(())
            }
            "join" => {
                let line = p.selection_line.unwrap_or_else(|| self.get_cursor("head").line);
                let end = p.selection_line_end.unwrap_or(line);
                self.set_cursor(Pos::new(line, 0));
                let mut args = Args {
                    repeat: end - line,
                    ..Args::default()
                };
                self.run_action(ActionKind::JoinLines, &mut args);
                Ok(())
            }
            "delmarks" => {
                self.ex_delmarks(p);
                Ok(())
            }
            "version" => {
                self.show_confirm("Codemirror-vim version: 6.3.0");
                Ok(())
            }
            _ => Ok(()),
        }
    }

    fn ex_set(&mut self, p: &mut ExParams, scope: Scope) -> Result<(), String> {
        let Some(arg0) = p.args.as_ref().and_then(|a| a.first()).cloned() else {
            self.show_confirm(&format!("Invalid mapping: {}", p.input));
            return Ok(());
        };
        let mut expr = arg0.split('=');
        let mut name = expr.next().unwrap_or("").to_string();
        let rest: Vec<&str> = expr.collect();
        let mut value: Option<OptionValue> = if rest.is_empty() {
            None
        } else {
            Some(OptionValue::Str(rest.join("=")))
        };
        let mut force_get = false;
        let mut force_toggle = false;
        if name.ends_with('?') {
            if matches!(&value, Some(OptionValue::Str(s)) if !s.is_empty()) {
                return Err(format!(
                    "Error: Trailing characters: {}",
                    p.arg_string.as_deref().unwrap_or("")
                ));
            }
            name.pop();
            force_get = true;
        } else if name.ends_with('!') {
            name.pop();
            force_toggle = true;
        }
        if value.is_none() && name.starts_with("no") {
            name = name[2..].to_string();
            value = Some(OptionValue::Bool(false));
        }
        let is_bool = matches!(
            crate::searchops::option_def(&name),
            Some((_, crate::searchops::OptType::Bool))
        );
        if is_bool {
            if force_toggle {
                let cur = matches!(self.get_option(&name, scope), Ok(Some(OptionValue::Bool(true))));
                value = Some(OptionValue::Bool(!cur));
            } else if value.is_none() {
                value = Some(OptionValue::Bool(true));
            }
        }
        if (!is_bool && value.is_none()) || force_get {
            match self.get_option(&name, scope) {
                Err(e) => self.show_confirm(&e),
                Ok(Some(OptionValue::Bool(b))) => {
                    self.show_confirm(&format!(" {}{name}", if b { "" } else { "no" }))
                }
                Ok(Some(OptionValue::Number(n))) => self.show_confirm(&format!("  {name}={n}")),
                Ok(Some(OptionValue::Str(s))) => self.show_confirm(&format!("  {name}={s}")),
                Ok(None) => self.show_confirm(&format!("  {name}=undefined")),
            }
        } else if let Err(e) = self.set_option(&name, value, scope) {
            self.show_confirm(&e);
        }
        Ok(())
    }

    fn ex_registers(&mut self, p: &ExParams) {
        let mut info = String::from("----------Registers----------\n\n");
        let regs = &self.g.register_controller.registers;
        match &p.args {
            None => {
                let mut names: Vec<char> = regs.keys().copied().collect();
                names.sort_by_key(|c| (!c.is_ascii_digit(), *c));
                for n in names {
                    let t = regs[&n].text();
                    if !t.is_empty() {
                        info.push_str(&format!("\"{n}    {t}\n"));
                    }
                }
            }
            Some(args) => {
                for n in args.concat().chars() {
                    if !RegisterController::is_valid_register(Some(n)) {
                        continue;
                    }
                    let t = regs.get(&n).map(|r| r.text()).unwrap_or_default();
                    info.push_str(&format!("\"{n}    {t}\n"));
                }
            }
        }
        self.show_confirm_long(&info);
    }

    fn ex_marks(&mut self, p: &ExParams) {
        let mut info = String::from("-----------Marks-----------\nmark\tline\tcol\n\n");
        let names: Vec<char> = match &p.args {
            None => {
                let mut v: Vec<char> = self.vim.marks.keys().copied().collect();
                v.sort();
                v
            }
            Some(a) => a.concat().chars().collect(),
        };
        for n in names {
            let Some(&id) = self.vim.marks.get(&n) else { continue };
            if let Some(m) = self.find_marker(id) {
                info.push_str(&format!("{n}\t{}\t{}\n", m.line, m.ch));
            }
        }
        self.show_confirm_long(&info);
    }

    fn ex_sort(&mut self, p: &ExParams) -> Result<(), String> {
        let mut reverse = false;
        let mut ignore_case = false;
        let mut unique = false;
        let mut number: Option<&str> = None;
        let mut pattern: Option<Query> = None;
        let arg = p.arg_string.clone().unwrap_or_default();
        let err: Option<&str> = (|| {
            if arg.is_empty() {
                return None;
            }
            let mut s = Stream::new(&arg);
            if s.eat('!') {
                reverse = true;
            }
            if s.eol() {
                return None;
            }
            if !s.eat_space() {
                return Some("Invalid arguments");
            }
            let opts = s.match_re(&RE_SORT_OPTS);
            let Some(opts) = opts.filter(|_| s.eol()) else {
                return Some("Invalid arguments");
            };
            if let Some(o) = &opts[1] {
                ignore_case = o.contains('i');
                unique = o.contains('u');
                let decimal = o.contains('d') || o.contains('n');
                let hex = o.contains('x');
                let octal = o.contains('o');
                if decimal as u8 + hex as u8 + octal as u8 > 1 {
                    return Some("Invalid arguments");
                }
                number = if decimal {
                    Some("decimal")
                } else if hex {
                    Some("hex")
                } else if octal {
                    Some("octal")
                } else {
                    None
                };
            }
            if let Some(pat) = &opts[2] {
                let src: String = pat.chars().skip(1).take(pat.chars().count() - 2).collect();
                pattern = Query::new(&src, ignore_case).ok();
                if pattern.is_none() {
                    return Some("Invalid arguments");
                }
            }
            None
        })();
        if let Some(e) = err {
            self.show_confirm(&format!("{e}: {arg}"));
            return Ok(());
        }
        let line_start = js_truthy(p.line).unwrap_or(self.first_line());
        let line_end = js_truthy(p.line_end)
            .or(js_truthy(p.line))
            .unwrap_or(self.last_line());
        if line_start == line_end {
            return Ok(());
        }
        let cur_start = Pos::new(line_start, 0);
        let cur_end = Pos::new(line_end, self.line_length(line_end));
        let text: Vec<String> = self
            .get_range(cur_start, cur_end)
            .split('\n')
            .map(String::from)
            .collect();
        let number_re: Option<Regex> = match number {
            Some("decimal") => Some(Regex::new(r"(-?)(\d+)").unwrap()),
            Some("hex") => Some(Regex::new(r"(?i)(-?)(?:0x)?([0-9a-f]+)").unwrap()),
            Some("octal") => Some(Regex::new(r"([0-7]+)").unwrap()),
            _ => None,
        };
        let radix = match number {
            Some("hex") => 16,
            Some("octal") => 8,
            _ => 10,
        };
        // (line, sort key for pattern sorts)
        let mut num_part: Vec<(String, String)> = Vec::new();
        let mut text_part: Vec<String> = Vec::new();
        if number.is_some() || pattern.is_some() {
            for t in &text {
                let m = pattern.as_ref().and_then(|q| q.exec(t)).and_then(|m| m.groups[0].clone());
                if let Some(m0) = m.filter(|m| !m.is_empty()) {
                    num_part.push((t.clone(), m0));
                } else if number_re.as_ref().is_some_and(|r| r.is_match(t)) {
                    let key: String = t.chars().take(1).collect();
                    num_part.push((t.clone(), key));
                } else {
                    text_part.push(t.clone());
                }
            }
        } else {
            text_part = text.clone();
        }
        let compare = |a: &str, b: &str| -> std::cmp::Ordering {
            let (mut a, mut b) = (a.to_string(), b.to_string());
            if reverse {
                std::mem::swap(&mut a, &mut b);
            }
            if ignore_case {
                a = a.to_lowercase();
                b = b.to_lowercase();
            }
            let num = |s: &str| -> Option<f64> {
                let r = number_re.as_ref()?;
                let c = r.captures(s)?;
                let digits = if radix == 8 {
                    c.get(1).map(|m| m.as_str().to_string()).unwrap_or_default()
                } else {
                    format!(
                        "{}{}",
                        c.get(1).map(|m| m.as_str()).unwrap_or(""),
                        c.get(2).map(|m| m.as_str()).unwrap_or("")
                    )
                };
                Some(parse_radix(&digits.to_lowercase(), radix))
            };
            match (num(&a), num(&b)) {
                (Some(x), Some(y)) => x.partial_cmp(&y).unwrap_or(std::cmp::Ordering::Equal),
                _ => a.cmp(&b),
            }
        };
        if pattern.is_some() {
            num_part.sort_by(|x, y| {
                let (mut a, mut b) = (x.1.clone(), y.1.clone());
                if reverse {
                    std::mem::swap(&mut a, &mut b);
                }
                if ignore_case {
                    a = a.to_lowercase();
                    b = b.to_lowercase();
                }
                a.cmp(&b)
            });
        } else {
            num_part.sort_by(|x, y| compare(&x.0, &y.0));
            if number.is_none() {
                text_part.sort_by(|a, b| compare(a, b));
            }
        }
        let nums: Vec<String> = num_part.into_iter().map(|x| x.0).collect();
        let mut out: Vec<String> = if !reverse {
            text_part.into_iter().chain(nums).collect()
        } else {
            nums.into_iter().chain(text_part).collect()
        };
        if unique {
            out.dedup();
        }
        self.replace_range(&out.join("\n"), cur_start, Some(cur_end));
        Ok(())
    }

    fn ex_normal(&mut self, p: &ExParams) {
        let mut noremap = false;
        let mut arg = p.arg_string.clone().unwrap_or_default();
        if arg.starts_with('!') {
            arg = arg[1..].to_string();
            noremap = true;
        }
        let arg = arg.trim_start().to_string();
        if arg.is_empty() {
            self.show_confirm("Argument is required.");
            return;
        }
        // cm-vim passes `params.argString.trimStart()` (the `!` included)
        let keys = p.arg_string.clone().unwrap_or_default().trim_start().to_string();
        match p.line {
            Some(line) => {
                let end = p.line_end.unwrap_or(line);
                let mut i = line;
                while i <= end {
                    self.set_cursor(Pos::new(i, 0));
                    self.do_key_to_key(&keys, None, noremap);
                    if self.vim.insert_mode {
                        self.exit_insert_mode(true);
                    }
                    i += 1;
                }
            }
            None => {
                self.do_key_to_key(&keys, None, noremap);
                if self.vim.insert_mode {
                    self.exit_insert_mode(true);
                }
            }
        }
    }

    fn ex_global(&mut self, p: &ExParams) -> Result<(), String> {
        let Some(mut arg) = p.arg_string.clone().filter(|s| !s.is_empty()) else {
            self.show_confirm("Regular Expression missing from global");
            return Ok(());
        };
        let cn = p.command_name.clone().unwrap_or_default();
        let mut inverted = cn.starts_with('v');
        if arg.starts_with('!') && cn.starts_with('g') {
            inverted = true;
            arg = arg[1..].to_string();
        }
        let line_start = p.line.unwrap_or(self.first_line());
        let line_end = js_truthy(p.line_end)
            .or(js_truthy(p.line))
            .unwrap_or(self.last_line());
        let mut regex_part = arg.clone();
        let mut cmd = String::new();
        if let Some(tokens) = split_by_separator(&arg, '/')
            && !tokens.is_empty()
        {
            regex_part = tokens[0].clone();
            cmd = tokens[1..].join("/");
        }
        if !regex_part.is_empty() && self.update_search_query(&regex_part, true, true).is_err() {
            self.show_confirm(&format!("Invalid regex: {regex_part}"));
            return Ok(());
        }
        let Some(query) = self.g.search_query.clone() else {
            return Err("TypeError: Cannot read properties of undefined (reading 'test')".into());
        };
        let mut lines = Vec::new();
        let mut handles = Vec::new();
        let last = self.last_line();
        for i in line_start.max(0)..=line_end.min(last) {
            let line = self.get_line(i);
            if query.test(&line) != inverted {
                if cmd.is_empty() {
                    lines.push(line);
                } else {
                    handles.push(self.line_handle(i));
                }
            }
        }
        if cmd.is_empty() {
            self.show_confirm(&lines.join("\n"));
            return Ok(());
        }
        let id = self.next_chain;
        self.next_chain += 1;
        self.ex_chains.insert(
            id,
            GlobalChain {
                handles,
                cmd,
                ..GlobalChain::default()
            },
        );
        self.ex_callback(id);
        Ok(())
    }

    fn ex_substitute(&mut self, p: &ExParams) -> Result<(), String> {
        let arg = p.arg_string.clone().unwrap_or_default();
        let tokens: Option<Vec<String>> = match arg.chars().next() {
            Some(sep) => split_by_separator(&arg, sep),
            None => Some(Vec::new()),
        };
        let pcre = self.get_option_bool("pcre");
        let mut regex_part = String::new();
        let mut replace_part: Option<String> = None;
        let mut trailing: Option<Vec<String>> = None;
        let mut confirm = false;
        let mut global = false;
        let mut count: Option<i64> = None;
        match tokens.filter(|t| !t.is_empty()) {
            Some(t) => {
                regex_part = t[0].clone();
                if pcre && !regex_part.is_empty() {
                    regex_part = normalize_regex_source(&regex_part)?;
                }
                if let Some(rp) = t.get(1) {
                    static AMP: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"([^\\])&").unwrap());
                    let rp = if pcre {
                        unescape_regex_replace(&AMP.replace_all(rp, "${1}$$&"))
                    } else {
                        translate_regex_replace(rp)
                    };
                    self.g.last_substitute_replace_part = Some(rp.clone());
                    replace_part = Some(rp);
                }
                trailing = Some(match t.get(2).filter(|s| !s.is_empty()) {
                    Some(s) => s.split(' ').map(String::from).collect(),
                    None => Vec::new(),
                });
            }
            None => {
                if !arg.is_empty() {
                    self.show_confirm("Substitutions should be of the form :s/pattern/replace/");
                    return Ok(());
                }
            }
        }
        if let Some(tr) = &trailing {
            let flags = tr.first().cloned().unwrap_or_default();
            count = tr.get(1).and_then(|s| parse_int_js(s));
            if !flags.is_empty() {
                confirm = flags.contains('c');
                global = flags.contains('g');
                regex_part = if pcre {
                    format!("{regex_part}/{flags}")
                } else {
                    format!("{}/{flags}", regex_part.replace('/', "\\/"))
                };
            }
        }
        if !regex_part.is_empty() && self.update_search_query(&regex_part, true, true).is_err() {
            self.show_confirm(&format!("Invalid regex: {regex_part}"));
            return Ok(());
        }
        let replace_part = replace_part
            .filter(|s| !s.is_empty())
            .or(self.g.last_substitute_replace_part.clone());
        let Some(replace_with) = replace_part else {
            self.show_confirm("No previous substitute regular expression");
            return Ok(());
        };
        let Some(query) = self.g.search_query.clone() else {
            return Err("TypeError: Cannot read properties of undefined (reading 'source')".into());
        };
        let mut line_start = p.line.unwrap_or_else(|| self.get_cursor("head").line);
        let mut line_end = js_truthy(p.line_end).unwrap_or(line_start);
        if line_start == self.first_line() && line_end == self.last_line() {
            line_end = INF;
        }
        if let Some(c) = count.filter(|&c| c != 0) {
            line_start = line_end;
            line_end = line_start + c - 1;
        }
        let start = self.clip_cursor_to_content(Pos::new(line_start, 0));
        let cursor = SearchCursor::new(&query, self.index_from_pos(start));
        self.do_replace(ReplaceState {
            confirm,
            global,
            line_start,
            line_end,
            cursor,
            query,
            replace_with,
            callback: p.callback,
            done: false,
            matches: 0,
            last_pos: None,
            modified_line: None,
            joined: false,
        });
        Ok(())
    }

    fn ex_delmarks(&mut self, p: &ExParams) {
        let arg = p.arg_string.clone().unwrap_or_default();
        let trimmed = arg.trim().to_string();
        if trimmed.is_empty() {
            self.show_confirm("Argument required");
            return;
        }
        let substring = |n: i64| -> String { arg.chars().skip(n.max(0) as usize).collect() };
        let mut s = Stream::new(&trimmed);
        while !s.eol() {
            s.eat_space();
            let count = s.pos();
            if !s.peek_matches(|c| c.is_ascii_alphabetic()) {
                self.show_confirm(&format!("Invalid argument: {}", substring(count)));
                return;
            }
            let sym = s.next().unwrap();
            if s.eat('-') {
                if !s.peek_matches(|c| c.is_ascii_alphabetic()) {
                    self.show_confirm(&format!("Invalid argument: {}", substring(count)));
                    return;
                }
                let finish = s.next().unwrap();
                if sym.is_ascii_lowercase() == finish.is_ascii_lowercase() {
                    if sym >= finish {
                        self.show_confirm(&format!("Invalid argument: {}", substring(count)));
                        return;
                    }
                    for c in sym..=finish {
                        if let Some(id) = self.vim.marks.remove(&c) {
                            self.clear_marker(id);
                        }
                    }
                } else {
                    self.show_confirm(&format!("Invalid argument: {sym}-"));
                    return;
                }
            } else if let Some(id) = self.vim.marks.remove(&sym) {
                self.clear_marker(id);
            }
        }
    }

    // ---- doReplace ------------------------------------------------------------------

    fn pcre_suffix(&self, vim_caps: bool) -> &'static str {
        if !self.get_option_bool("pcre") {
            ""
        } else if vim_caps {
            " (set nopcre to use Vim regexps)"
        } else {
            " (set nopcre to use vim regexps)"
        }
    }

    fn do_replace(&mut self, mut st: ReplaceState) {
        self.vim.ex_mode = true;
        self.rs_next(&mut st);
        if st.done {
            let msg = format!("No matches for {}{}", st.query.display(), self.pcre_suffix(false));
            self.show_confirm(&msg);
            return;
        }
        if !st.confirm {
            self.rs_replace_all(&mut st);
            if let Some(cb) = st.callback {
                self.ex_callback(cb);
            }
            return;
        }
        let prefix = format!("replace with {} (y/n/a/q/l)", st.replace_with);
        self.show_prompt(Prompt {
            mode: PromptMode::ReplaceConfirm(Box::new(st)),
            value: String::new(),
            prefix,
        });
    }

    fn rs_replace_all(&mut self, st: &mut ReplaceState) {
        self.operation(false, |ed| {
            while !st.done {
                ed.rs_replace(st);
                ed.rs_next(st);
            }
            ed.rs_stop(st);
        });
    }

    fn rs_replace(&mut self, st: &mut ReplaceState) {
        let Some(m) = st.cursor.last.clone() else {
            return;
        };
        let new_text = expand_do_replace(&st.replace_with, &m.groups);
        let unmodified = self.pos_from_index(m.to).line;
        self.dispatch_change(ChangeSet::new(vec![Change::new(m.from, m.to, &new_text)]), None);
        let to = m.from + new_text.chars().count();
        st.cursor.set_last_to(to);
        let modified = self.pos_from_index(to).line;
        st.line_end += modified - unmodified;
        st.modified_line = Some(modified);
        st.joined = modified < unmodified;
    }

    fn rs_find_next_valid(&mut self, st: &mut ReplaceState) -> bool {
        let last_match_to = st.last_pos.and(st.cursor.to());
        let mut m = st.cursor.find(&self.doc, false);
        if m.as_deref() == Some("") && last_match_to.is_some() && st.cursor.from() == last_match_to {
            m = st.cursor.find(&self.doc, false);
        }
        if m.is_some() {
            st.matches += 1;
        }
        m.is_some()
    }

    fn rs_next(&mut self, st: &mut ReplaceState) {
        while self.rs_find_next_valid(st) {
            let from = self.pos_from_index(st.cursor.from().unwrap_or(0));
            if !(from.line >= st.line_start && from.line <= st.line_end) {
                break;
            }
            if !st.global && Some(from.line) == st.modified_line && !st.joined {
                continue;
            }
            let to = self.pos_from_index(st.cursor.to().unwrap_or(0));
            self.want_scroll_into_view = true;
            self.set_selection_pos(from, to);
            st.last_pos = Some(from);
            st.done = false;
            return;
        }
        st.done = true;
    }

    /// `stop()` (the dialog, if any, is closed by the caller first).
    fn rs_stop(&mut self, st: &mut ReplaceState) {
        if let Some(lp) = st.last_pos {
            self.set_cursor(lp);
            self.vim.ex_mode = false;
            self.vim.last_h_pos = lp.ch;
            self.vim.last_hs_pos = lp.ch;
        }
        if let Some(cb) = st.callback {
            self.ex_callback(cb);
        } else if st.done {
            let found = if st.matches > 0 {
                format!("Found {} matches", st.matches)
            } else {
                "No matches found".to_string()
            };
            let msg = format!("{found} for pattern: {}{}", st.query.display(), self.pcre_suffix(true));
            self.show_confirm(&msg);
        }
    }

    /// The confirmation prompt's `onKeyDown` (every key is swallowed).
    pub(crate) fn replace_confirm_key(&mut self, virt: bool, key: &str) {
        let slot = if virt { &mut self.virtual_prompt } else { &mut self.prompt };
        let Some(p) = slot.take() else { return };
        let Prompt {
            mode: PromptMode::ReplaceConfirm(mut st),
            value,
            prefix,
        } = p
        else {
            *slot = Some(p);
            return;
        };
        let mut closed = false;
        match key {
            "y" => {
                self.rs_replace(&mut st);
                self.rs_next(&mut st);
            }
            "n" => self.rs_next(&mut st),
            "a" => {
                let saved = st.callback.take();
                self.rs_replace_all(&mut st);
                st.callback = saved;
            }
            "l" | "q" | "<Esc>" | "<C-c>" | "<C-[>" => {
                if key == "l" {
                    self.rs_replace(&mut st);
                }
                closed = true;
                self.rs_stop(&mut st);
            }
            _ => {}
        }
        if st.done {
            closed = true;
            self.rs_stop(&mut st);
        }
        if !closed {
            let slot = if virt { &mut self.virtual_prompt } else { &mut self.prompt };
            if slot.is_none() {
                *slot = Some(Prompt {
                    mode: PromptMode::ReplaceConfirm(st),
                    value,
                    prefix,
                });
            }
        }
    }
}

/// Context names used by `:map` (for docs / completion).
#[allow(dead_code)]
pub(crate) fn context_name_to_ctx(s: &str) -> Option<Context> {
    context_from_name(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stream_negative_pos() {
        let mut s = Stream::new("");
        assert_eq!(s.next(), None);
        s.back_up(1);
        assert_eq!(s.rest(), "");
        let mut s = Stream::new(":");
        s.eat_while(':');
        s.next();
        s.back_up(1);
        assert_eq!(s.rest(), ":");
    }

    #[test]
    fn do_replace_groups() {
        let g = vec![Some("ab".to_string()), Some("a".to_string()), None];
        assert_eq!(expand_do_replace("[$1|$2|$&|$$|$9|$10]", &g), "[a||ab|$|$9|a0]");
    }

    #[test]
    fn regex_source() {
        assert_eq!(normalize_regex_source("a/b[/]").unwrap(), "a\\/b[/]");
        assert!(normalize_regex_source("(").is_err());
    }

    #[test]
    fn radix() {
        assert_eq!(parse_radix("-ff", 16), -255.0);
        assert_eq!(parse_radix("0x10", 16), 16.0);
        assert_eq!(parse_radix("17", 8), 15.0);
    }
}
