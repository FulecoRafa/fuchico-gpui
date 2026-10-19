//! cm-vim `actions`.

use fuchico_helix::{Effect, Scroll, ScrollAlign};

use crate::editor::Editor;
use crate::keymap::ActionKind;
use crate::pos::{self, INF, Pos, before, chars, clen, first_non_ws, offset};
use crate::state::{Args, Register};

impl Editor {
    /// `actions[name](cm, actionArgs, vim)`.
    pub(crate) fn run_action(&mut self, kind: ActionKind, args: &mut Args) {
        use ActionKind as A;
        match kind {
            A::JumpListWalk => self.act_jump_list_walk(args),
            A::Scroll => self.act_scroll(args),
            A::ScrollToCursor => {
                let line = self.head().line;
                let align = match args.position {
                    Some("center") => ScrollAlign::Center,
                    Some("bottom") => ScrollAlign::Bottom,
                    _ => ScrollAlign::Top,
                };
                let pos = self.index_from_pos(Pos::new(line, 0));
                self.scroll = Some(Scroll::To { pos, align });
            }
            A::ReplayMacro => {
                let mut name = args
                    .selected_character
                    .as_deref()
                    .and_then(|s| s.chars().next());
                let mut repeat = if args.repeat == 0 { 1 } else { args.repeat };
                if name == Some('@') {
                    name = self.g.macro_mode_state.latest_register;
                } else {
                    self.g.macro_mode_state.latest_register = name;
                }
                while repeat > 0 {
                    repeat -= 1;
                    self.execute_macro_register(name);
                }
            }
            A::EnterMacroRecordMode => {
                let name = args
                    .selected_character
                    .as_deref()
                    .and_then(|s| s.chars().next());
                if crate::state::RegisterController::is_valid_register(name) {
                    self.enter_macro_record_mode(name.unwrap());
                }
            }
            A::ToggleOverwrite => {
                if !self.overwrite {
                    self.overwrite = true;
                    self.signal_mode("replace", "");
                } else {
                    self.overwrite = false;
                    self.signal_mode("insert", "");
                }
            }
            A::EnterInsertMode => self.enter_insert_mode(args),
            A::ToggleVisualMode => self.act_toggle_visual_mode(args),
            A::ReselectLastSelection => self.act_reselect_last_selection(),
            A::JoinLines => self.act_join_lines(args),
            A::NewLineAndEnterInsertMode => {
                self.vim.insert_mode = true;
                let mut at = self.head();
                if at.line == self.first_line() && !args.after {
                    self.replace_range("\n", Pos::new(0, 0), None);
                    self.set_cursor(Pos::new(0, 0));
                } else {
                    at.line = if args.after { at.line } else { at.line - 1 };
                    at.ch = self.line_length(at.line);
                    self.set_cursor(at);
                    self.cmd_newline_and_indent(false);
                }
                let a = Args {
                    repeat: args.repeat,
                    ..Args::default()
                };
                self.enter_insert_mode(&a);
            }
            A::Paste => self.act_paste(args),
            A::Undo => {
                let repeat = args.repeat;
                self.operation(false, |ed| {
                    for _ in 0..repeat {
                        ed.run_history_command(false);
                    }
                    let c = ed.clip_cursor_to_content(ed.get_cursor("start"));
                    ed.set_cursor(c);
                });
            }
            A::Redo => {
                for _ in 0..args.repeat {
                    self.run_history_command(true);
                }
            }
            A::SetRegister => {
                self.vim.input_state.register_name = args
                    .selected_character
                    .as_deref()
                    .and_then(|s| s.chars().next());
            }
            A::InsertRegister => {
                let name = args
                    .selected_character
                    .as_deref()
                    .and_then(|s| s.chars().next());
                let text = self.register_text(name);
                if !text.is_empty() {
                    self.replace_selection(&text);
                }
            }
            A::OneNormalCommand => {
                self.exit_insert_mode(true);
                self.vim.insert_mode_return = true;
                self.one_normal_pending = true;
            }
            A::SetMark => {
                if let Some(c) = args
                    .selected_character
                    .as_deref()
                    .and_then(|s| s.chars().next())
                {
                    let h = self.head();
                    self.update_mark(c, h);
                }
            }
            A::Replace => self.act_replace(args),
            A::IncrementNumberToken => self.act_increment(args),
            A::RepeatLastEdit => {
                let Some(last) = self.vim.last_edit_input_state.as_mut() else {
                    return;
                };
                let mut repeat = args.repeat;
                if repeat != 0 && args.repeat_is_explicit {
                    last.repeat_override = Some(repeat);
                } else if let Some(o) = last.repeat_override.filter(|o| *o != 0) {
                    repeat = o;
                }
                self.repeat_last_edit(repeat, false);
            }
            A::Indent => {
                if args.indent_right {
                    self.cmd_indent_more();
                } else {
                    self.cmd_indent_less();
                }
            }
            A::ExitInsertMode => self.exit_insert_mode(false),
        }
    }

    /// The text of a register as `getRegister(name).toString()` (`"+`
    /// reads the host clipboard).
    pub(crate) fn register_text(&mut self, name: Option<char>) -> String {
        if name == Some('+') {
            return self.host.clipboard_read().unwrap_or_default();
        }
        self.g.register_controller.get_register(name).text()
    }

    fn act_jump_list_walk(&mut self, args: &Args) {
        if self.vim.visual_mode {
            return;
        }
        let repeat = if args.repeat == 0 { 1 } else { args.repeat };
        let mark = self.jump_move(if args.forward { repeat } else { -repeat });
        let p = mark
            .and_then(|m| self.find_marker(m))
            .unwrap_or_else(|| self.head());
        self.set_cursor(p);
    }

    /// `<C-e>` / `<C-y>`: scroll the view by lines; keep the cursor inside
    /// the visible range (approximated from the host's visible lines).
    fn act_scroll(&mut self, args: &Args) {
        if self.vim.visual_mode {
            return;
        }
        let repeat = if args.repeat == 0 { 1 } else { args.repeat };
        let delta = if args.forward { repeat } else { -repeat };
        self.scroll = Some(Scroll::Lines(delta as isize));
        if let Some((a, b)) = self.host.visible_lines() {
            let last = self.last_line();
            let top = (a as i64 - 1 + delta).clamp(0, last);
            let bottom = (b as i64 - 1 + delta).clamp(0, last);
            let mut cur = self.head();
            if args.forward && cur.line < top {
                cur.line = top;
                self.set_cursor(cur);
            } else if !args.forward && cur.line > bottom {
                cur.line = bottom;
                self.set_cursor(cur);
            }
        }
    }

    /// `actions.enterInsertMode`.
    pub(crate) fn enter_insert_mode(&mut self, args: &Args) {
        self.vim.insert_mode = true;
        self.vim.insert_mode_repeat = Some(if args.repeat == 0 { 1 } else { args.repeat });
        let (sa, sh) = (self.vim.sel_anchor, self.vim.sel_head);
        let mut head = args.head.unwrap_or_else(|| self.head());
        let mut height = self.sel.len() as i64;
        match args.insert_at {
            Some("eol") => head = Pos::new(head.line, self.line_length(head.line)),
            Some("bol") => head = Pos::new(head.line, 0),
            Some("charAfter") => head = offset(head, 0, 1),
            Some("firstNonBlank") => head = Pos::new(head.line, self.first_non_ws_of(head.line)),
            Some("startOfSelectedArea") => {
                if !self.vim.visual_mode {
                    return;
                }
                if !self.vim.visual_block {
                    head = if sh.line < sa.line { sh } else { Pos::new(sa.line, 0) };
                } else {
                    head = Pos::new(sh.line.min(sa.line), sh.ch.min(sa.ch));
                    height = (sh.line - sa.line).abs() + 1;
                }
            }
            Some("endOfSelectedArea") => {
                if !self.vim.visual_mode {
                    return;
                }
                if !self.vim.visual_block {
                    head = if sh.line >= sa.line {
                        offset(sh, 0, 1)
                    } else {
                        Pos::new(sa.line, 0)
                    };
                } else {
                    head = Pos::new(sh.line.min(sa.line), sh.ch.max(sa.ch) + 1);
                    height = (sh.line - sa.line).abs() + 1;
                }
            }
            Some("inplace") => {
                if self.vim.visual_mode {
                    return;
                }
            }
            Some("lastEdit") => {
                head = self.pos_from_index(self.last_change_end_offset);
            }
            _ => {}
        }
        if args.replace {
            self.overwrite = true;
            self.signal_mode("replace", "");
        } else {
            self.overwrite = false;
            self.signal_mode("insert", "");
        }
        if !self.g.macro_mode_state.is_playing {
            self.change_listener = true;
            self.change_listener_ever = true;
            if let Some(m) = self.vim.insert_end.take() {
                self.clear_marker(m);
            }
            let m = self.set_bookmark(head, true);
            self.vim.insert_end = Some(m);
        }
        if self.vim.visual_mode {
            self.exit_visual_mode(true);
        }
        self.select_for_insert(head, height);
    }

    fn act_toggle_visual_mode(&mut self, args: &Args) {
        let repeat = args.repeat;
        let anchor = self.head();
        if !self.vim.visual_mode {
            self.vim.visual_mode = true;
            self.vim.visual_line = args.linewise;
            self.vim.visual_block = args.blockwise;
            let head = self.clip_cursor_to_content(Pos::new(anchor.line, anchor.ch + repeat - 1));
            self.vim.sel_anchor = anchor;
            self.vim.sel_head = head;
            let sub = self.visual_sub_mode();
            self.signal_mode("visual", sub);
            self.update_cm_selection();
            self.update_mark('<', pos::min(anchor, head));
            self.update_mark('>', pos::max(anchor, head));
        } else if self.vim.visual_line != args.linewise || self.vim.visual_block != args.blockwise {
            self.vim.visual_line = args.linewise;
            self.vim.visual_block = args.blockwise;
            let sub = self.visual_sub_mode();
            self.signal_mode("visual", sub);
            self.update_cm_selection();
        } else {
            self.exit_visual_mode(true);
        }
    }

    fn act_reselect_last_selection(&mut self) {
        let last = self.vim.last_selection;
        if self.vim.visual_mode {
            self.update_last_selection();
        }
        let Some(last) = last else { return };
        let (Some(anchor), Some(head)) =
            (self.find_marker(last.anchor_mark), self.find_marker(last.head_mark))
        else {
            return;
        };
        self.vim.sel_anchor = anchor;
        self.vim.sel_head = head;
        self.vim.visual_mode = true;
        self.vim.visual_line = last.visual_line;
        self.vim.visual_block = last.visual_block;
        self.update_cm_selection();
        self.update_mark('<', pos::min(anchor, head));
        self.update_mark('>', pos::max(anchor, head));
        let sub = self.visual_sub_mode();
        self.signal_mode("visual", sub);
    }

    fn act_join_lines(&mut self, args: &Args) {
        let (cur_start, cur_end);
        if self.vim.visual_mode {
            let mut s = self.get_cursor("anchor");
            let mut e = self.get_cursor("head");
            if before(e, s) {
                std::mem::swap(&mut s, &mut e);
            }
            e.ch = self.line_length(e.line) - 1;
            cur_start = s;
            cur_end = e;
        } else {
            let repeat = args.repeat.max(2);
            cur_start = self.head();
            cur_end = self.clip_cursor_to_content(Pos::new(cur_start.line + repeat - 1, INF));
        }
        let mut final_ch = 0;
        for _ in cur_start.line..cur_end.line {
            final_ch = self.line_length(cur_start.line);
            let mut text = "";
            let mut next_start = 0;
            if !args.keep_spaces {
                let next = self.get_line(cur_start.line + 1);
                let s = first_non_ws_index(&next);
                if s == -1 {
                    next_start = clen(&next);
                } else {
                    next_start = s;
                    text = " ";
                }
            }
            self.replace_range(
                text,
                Pos::new(cur_start.line, final_ch),
                Some(Pos::new(cur_start.line + 1, next_start)),
            );
        }
        let fin = self.clip_cursor_to_content(Pos::new(cur_start.line, final_ch));
        if self.vim.visual_mode {
            self.exit_visual_mode(false);
        }
        self.set_cursor(fin);
    }

    fn act_paste(&mut self, args: &Args) {
        let reg = self.g.register_controller.get_register(args.register_name).clone();
        let text = if args.register_name == Some('+') {
            match self.host.clipboard_read() {
                Ok(t) => t,
                Err(e) => {
                    self.show_error(&e);
                    return;
                }
            }
        } else {
            reg.text()
        };
        self.continue_paste(args, text, &reg);
    }

    /// `actions.continuePaste`.
    fn continue_paste(&mut self, args: &Args, text: String, register: &Register) {
        let mut cur = self.head();
        if text.is_empty() {
            return;
        }
        let mut text = text;
        if args.match_indent {
            let tab = self.config.tab_size as i64;
            let ws_len = |s: &str| -> i64 {
                s.chars().filter(|c| *c == '\t').count() as i64 * tab
                    + s.chars().filter(|c| *c == ' ').count() as i64
            };
            let current = self.get_line(cur.line);
            let indent = ws_len(leading_ws(&current));
            let chomped = text.strip_suffix('\n').unwrap_or(&text).to_string();
            let was_chomped = chomped.len() != text.len();
            let first_indent = ws_len(leading_ws(&text));
            let tabs = self.config.indent_is_tab();
            let mut out = String::new();
            for (i, line) in chomped.split('\n').enumerate() {
                if i > 0 {
                    out.push('\n');
                }
                let ws = leading_ws(line);
                let new_indent = indent + (ws_len(ws) - first_indent);
                if new_indent > 0 {
                    if tabs {
                        out.push_str(&"\t".repeat((new_indent / tab) as usize));
                    } else {
                        out.push_str(&" ".repeat(new_indent as usize));
                    }
                }
                out.push_str(&line[ws.len()..]);
            }
            if was_chomped {
                out.push('\n');
            }
            text = out;
        }
        if args.repeat > 1 {
            text = text.repeat(args.repeat as usize);
        }
        let linewise = register.linewise;
        let blockwise = register.blockwise;
        let mut text_lines: Option<Vec<String>> = None;
        if blockwise {
            let mut lines: Vec<String> = text.split('\n').map(String::from).collect();
            if linewise {
                lines.pop();
            }
            for l in lines.iter_mut() {
                if l.is_empty() {
                    *l = " ".into();
                }
            }
            text_lines = Some(lines);
            cur.ch += i64::from(args.after);
            cur.ch = cur.ch.min(self.line_length(cur.line));
        } else if linewise {
            if self.vim.visual_mode {
                let body: String = {
                    let c = chars(&text);
                    c[..c.len().saturating_sub(1)].iter().collect()
                };
                text = if self.vim.visual_line {
                    body
                } else {
                    format!("\n{body}\n")
                };
            } else if args.after {
                let c = chars(&text);
                let body: String = c[..c.len().saturating_sub(1)].iter().collect();
                text = format!("\n{body}");
                cur.ch = self.line_length(cur.line);
            } else {
                cur.ch = 0;
            }
        } else {
            cur.ch += i64::from(args.after);
        }
        let mut cur_pos_final;
        if self.vim.visual_mode {
            self.vim.last_pasted_text = Some(text.clone());
            let (sel_start, _sel_end) = self.get_selected_area_range();
            let sel_end = _sel_end;
            let selected = self.get_selection();
            let n = self.sel.len();
            let empties = vec![String::new(); n];
            let last_end = self
                .vim
                .last_selection
                .and_then(|l| self.find_marker(l.head_mark));
            self.g.register_controller.unnamed().set_text(&selected, false, false);
            if let Some(lines) = text_lines.as_ref().filter(|_| blockwise) {
                self.replace_selections(&empties);
                let end = Pos::new(sel_start.line + lines.len() as i64 - 1, sel_start.ch);
                self.set_cursor(sel_start);
                self.select_block(end);
                self.replace_selections(lines);
                cur_pos_final = sel_start;
            } else if self.vim.visual_block {
                self.replace_selections(&empties);
                self.set_cursor(sel_start);
                self.replace_range(&text, sel_start, Some(sel_start));
                cur_pos_final = sel_start;
            } else {
                self.replace_range(&text, sel_start, Some(sel_end));
                let i = self.index_from_pos(sel_start) + clen(&text) as usize;
                cur_pos_final = self.pos_from_index(i.saturating_sub(1));
            }
            if let Some(p) = last_end {
                let m = self.set_bookmark(p, false);
                if let Some(l) = self.vim.last_selection.as_mut() {
                    l.head_mark = m;
                }
            }
            if linewise {
                cur_pos_final.ch = 0;
            }
        } else if let Some(lines) = text_lines.as_ref().filter(|_| blockwise) {
            self.set_cursor(cur);
            for i in 0..lines.len() as i64 {
                let line = cur.line + i;
                if line > self.last_line() {
                    self.replace_range("\n", Pos::new(line, 0), None);
                }
                let last_ch = self.line_length(line);
                if last_ch < cur.ch {
                    self.extend_line_to_column(line, cur.ch);
                }
            }
            self.set_cursor(cur);
            self.select_block(Pos::new(cur.line + lines.len() as i64 - 1, cur.ch));
            self.replace_selections(lines);
            cur_pos_final = cur;
        } else {
            self.replace_range(&text, cur, None);
            if linewise {
                let line = if args.after { cur.line + 1 } else { cur.line };
                cur_pos_final = Pos::new(line, first_non_ws(&self.get_line(line)));
            } else {
                cur_pos_final = cur;
                if !text.contains('\n') {
                    cur_pos_final.ch += clen(&text) - i64::from(args.after);
                }
            }
        }
        if self.vim.visual_mode {
            self.exit_visual_mode(false);
        }
        self.set_cursor(cur_pos_final);
    }

    fn extend_line_to_column(&mut self, line: i64, column: i64) {
        let end = self.line_length(line);
        let spaces = " ".repeat((column - end).max(0) as usize);
        self.set_cursor(Pos::new(line, end));
        let c = self.head();
        self.replace_range(&spaces, c, None);
    }

    fn act_replace(&mut self, args: &Args) {
        let with = args.selected_character.clone().unwrap_or_default();
        let mut cur_start = self.head();
        let cur_end;
        let selections = self.list_selections();
        if self.vim.visual_mode {
            cur_start = self.get_cursor("start");
            cur_end = self.get_cursor("end");
        } else {
            let len = self.line_len(cur_start.line);
            let to = (cur_start.ch + args.repeat).min(len);
            cur_end = Pos::new(cur_start.line, to);
        }
        if with == "\n" {
            if !self.vim.visual_mode {
                self.replace_range("", cur_start, Some(cur_end));
            }
            self.cmd_newline_and_indent(false);
            return;
        }
        let range = self.get_range(cur_start, cur_end);
        let replaced: String = range
            .chars()
            .map(|c| if c == '\n' { "\n".to_string() } else { with.clone() })
            .collect();
        if self.vim.visual_block {
            let spaces = " ".repeat(self.config.tab_size);
            // getSelection() joins the block's lines with "\n"; split
            // back, that is one replacement per selection
            let per: Vec<String> = self
                .get_selections()
                .iter()
                .map(|s| {
                    s.replace('\t', &spaces)
                        .chars()
                        .map(|c| if c == '\n' { "\n".to_string() } else { with.clone() })
                        .collect()
                })
                .collect();
            self.replace_selections(&per);
        } else {
            self.replace_range(&replaced, cur_start, Some(cur_end));
        }
        if self.vim.visual_mode {
            let s0 = selections[0];
            let start = if before(s0.0, s0.1) { s0.0 } else { s0.1 };
            self.set_cursor(start);
            self.exit_visual_mode(false);
        } else {
            self.set_cursor(offset(cur_end, 0, -1));
        }
    }

    fn act_increment(&mut self, args: &Args) {
        let cur = self.head();
        let line = self.get_line(cur.line);
        let re = regex::Regex::new(r"(?i)(-?)(?:(0x)([\da-f]+)|(0b|0|)(\d+))").expect("regex");
        let lchars = chars(&line);
        let mut found = None;
        let mut start = 0i64;
        let mut end = 0i64;
        for m in re.captures_iter(&line) {
            let whole = m.get(0).unwrap();
            start = line[..whole.start()].chars().count() as i64;
            end = start + whole.as_str().chars().count() as i64;
            found = Some(m);
            if cur.ch < end {
                break;
            }
        }
        let _ = lchars;
        if !args.backtrack && end <= cur.ch {
            return;
        }
        let Some(m) = found else { return };
        let sign = m.get(1).map_or("", |x| x.as_str());
        let base_str = m
            .get(2)
            .map(|x| x.as_str())
            .filter(|s| !s.is_empty())
            .or_else(|| m.get(4).map(|x| x.as_str()))
            .unwrap_or("");
        let digits = m
            .get(3)
            .map(|x| x.as_str())
            .filter(|s| !s.is_empty())
            .or_else(|| m.get(5).map(|x| x.as_str()))
            .unwrap_or("");
        let base: u32 = match base_str.to_ascii_lowercase().as_str() {
            "0b" => 2,
            "0" => 8,
            "0x" => 16,
            _ => 10,
        };
        let inc: i128 = if args.increase { 1 } else { -1 };
        let parsed = parse_int_radix(&format!("{sign}{digits}"), base);
        let number = parsed + inc * args.repeat as i128;
        let mut num_str = to_radix(number, base);
        let pad = if base_str.is_empty() {
            String::new()
        } else {
            let n = digits.len() as i64 - num_str.len() as i64 + sign.len() as i64;
            "0".repeat(n.max(0) as usize)
        };
        if let Some(rest) = num_str.strip_prefix('-') {
            num_str = format!("-{base_str}{pad}{rest}");
        } else {
            num_str = format!("{base_str}{pad}{num_str}");
        }
        self.replace_range(&num_str, Pos::new(cur.line, start), Some(Pos::new(cur.line, end)));
        self.set_cursor(Pos::new(cur.line, start + clen(&num_str) - 1));
    }

    /// Unbound helper kept for the host: emit a message.
    #[allow(dead_code)]
    pub(crate) fn notify(&mut self, text: &str) {
        self.emit(Effect::Message {
            text: text.into(),
            error: false,
        });
    }
}

fn leading_ws(s: &str) -> &str {
    let n = s.len() - s.trim_start().len();
    &s[..n]
}

/// `str.search(/\S/)` in chars.
fn first_non_ws_index(s: &str) -> i64 {
    s.chars().position(|c| !c.is_whitespace()).map_or(-1, |i| i as i64)
}

/// `parseInt(s, base)` (digits already validated by the regex; an octal
/// literal with 8/9 digits stops there like JS).
fn parse_int_radix(s: &str, base: u32) -> i128 {
    let (neg, digits) = match s.strip_prefix('-') {
        Some(r) => (true, r),
        None => (false, s),
    };
    let mut n: i128 = 0;
    for c in digits.chars() {
        match c.to_digit(base) {
            Some(d) => n = n.saturating_mul(base as i128).saturating_add(d as i128),
            None => break,
        }
    }
    if neg { -n } else { n }
}

/// `number.toString(base)`.
fn to_radix(n: i128, base: u32) -> String {
    if n == 0 {
        return "0".into();
    }
    let neg = n < 0;
    let mut v = n.unsigned_abs();
    let mut out = Vec::new();
    while v > 0 {
        let d = (v % base as u128) as u32;
        out.push(std::char::from_digit(d, base).unwrap());
        v /= base as u128;
    }
    if neg {
        out.push('-');
    }
    out.iter().rev().collect()
}
