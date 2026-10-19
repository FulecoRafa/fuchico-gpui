//! cm-vim `operators` and the adapter's `hardWrap`.

use crate::editor::Editor;
use crate::keymap::OperatorKind;
use crate::pos::{self, INF, Pos, clen, is_upper, is_ws, is_ws_string, offset};
use crate::state::Args;

impl Editor {
    /// `operators[name](cm, operatorArgs, ranges, oldAnchor, newHead)`.
    /// `ranges` are `(anchor, head)` pairs.
    pub(crate) fn run_operator(
        &mut self,
        kind: OperatorKind,
        args: &mut Args,
        ranges: &[(Pos, Pos)],
        old_anchor: Pos,
        new_head: Pos,
    ) -> Option<Pos> {
        match kind {
            OperatorKind::Change => {
                self.op_change(args, ranges);
                None
            }
            OperatorKind::Delete => Some(self.op_delete(args, ranges)),
            OperatorKind::Indent => Some(self.op_indent(args, ranges)),
            OperatorKind::IndentAuto => {
                // no language indentation service for Markdown: a no-op
                let a = ranges[0].0;
                Some(Pos::new(a.line, self.first_non_ws_of(a.line)))
            }
            OperatorKind::HardWrap => {
                let from = ranges[0].0.line;
                let mut to = ranges[0].1.line;
                if args.linewise {
                    to -= 1;
                }
                let mut end_row = self.hard_wrap(from, to);
                if end_row > from && args.linewise {
                    end_row -= 1;
                }
                Some(if args.keep_cursor {
                    old_anchor
                } else {
                    Pos::new(end_row, 0)
                })
            }
            OperatorKind::ChangeCase | OperatorKind::Rot13 => {
                let sels = self.get_selections();
                let swapped: Vec<String> = sels
                    .iter()
                    .map(|s| {
                        if kind == OperatorKind::Rot13 {
                            rot13(s)
                        } else {
                            match args.to_lower {
                                Some(true) => s.to_lowercase(),
                                Some(false) => s.to_uppercase(),
                                None => swap_case(s),
                            }
                        }
                    })
                    .collect();
                self.replace_selections(&swapped);
                Some(if args.should_move_cursor {
                    new_head
                } else if !self.vim.visual_mode
                    && args.linewise
                    && ranges[0].0.line + 1 == ranges[0].1.line
                {
                    Pos::new(old_anchor.line, self.first_non_ws_of(old_anchor.line))
                } else if args.linewise {
                    old_anchor
                } else {
                    pos::min(ranges[0].0, ranges[0].1)
                })
            }
            OperatorKind::Yank => {
                let text = self.get_selection();
                let end = if self.vim.visual_mode {
                    pos::min_of(&[self.vim.sel_anchor, self.vim.sel_head, ranges[0].1, ranges[0].0])
                } else {
                    old_anchor
                };
                let vb = self.vim.visual_block;
                self.g
                    .register_controller
                    .push_text(args.register_name, "yank", &text, args.linewise, vb);
                Some(end)
            }
        }
    }

    fn op_change(&mut self, args: &mut Args, ranges: &[(Pos, Pos)]) {
        let (mut anchor, mut head) = ranges[0];
        let text;
        let final_head;
        if !self.vim.visual_mode {
            let mut t = self.get_range(anchor, head);
            let last = self.vim.last_edit_input_state.as_ref();
            if last.is_some_and(|l| l.motion == Some(crate::keymap::MotionKind::MoveByWords))
                && !is_ws_string(&t)
            {
                let trimmed = t.trim_end_matches(is_ws).to_string();
                let m = clen(&t) - clen(&trimmed);
                if m > 0
                    && last
                        .and_then(|l| l.motion_args.as_ref())
                        .is_some_and(|a| a.forward)
                {
                    head = offset(head, 0, -m);
                    t = trimmed;
                }
            }
            if args.linewise {
                anchor = Pos::new(anchor.line, self.first_non_ws_of(anchor.line));
                if head.line > anchor.line {
                    head = Pos::new(head.line - 1, INF);
                }
            }
            self.replace_range("", anchor, Some(head));
            text = t;
            final_head = anchor;
        } else if args.full_line {
            head.ch = INF;
            head.line -= 1;
            self.set_selection_pos(anchor, head);
            text = self.get_selection();
            self.replace_selection("");
            final_head = anchor;
        } else {
            text = self.get_selection();
            let rep = vec![String::new(); ranges.len()];
            self.replace_selections(&rep);
            final_head = pos::min(ranges[0].1, ranges[0].0);
        }
        self.g.register_controller.push_text(
            args.register_name,
            "change",
            &text,
            args.linewise,
            ranges.len() > 1,
        );
        let a = Args {
            head: Some(final_head),
            ..Args::default()
        };
        self.enter_insert_mode(&a);
    }

    pub(crate) fn op_delete(&mut self, args: &mut Args, ranges: &[(Pos, Pos)]) -> Pos {
        let text;
        let mut final_head;
        if !self.vim.visual_block {
            let (mut anchor, head) = ranges[0];
            if args.linewise
                && head.line != self.first_line()
                && anchor.line == self.last_line()
                && anchor.line == head.line - 1
            {
                if anchor.line == self.first_line() {
                    anchor.ch = 0;
                } else {
                    anchor = Pos::new(anchor.line - 1, self.line_length(anchor.line - 1));
                }
            }
            text = self.get_range(anchor, head);
            self.replace_range("", anchor, Some(head));
            final_head = anchor;
            if args.linewise {
                final_head = Pos::new(anchor.line, self.first_non_ws_of(anchor.line));
            }
        } else {
            text = self.get_selection();
            let rep = vec![String::new(); ranges.len()];
            self.replace_selections(&rep);
            final_head = pos::min(ranges[0].1, ranges[0].0);
        }
        let vb = self.vim.visual_block;
        self.g
            .register_controller
            .push_text(args.register_name, "delete", &text, args.linewise, vb);
        self.clip_cursor_to_content(final_head)
    }

    fn op_indent(&mut self, args: &mut Args, ranges: &[(Pos, Pos)]) -> Pos {
        let repeat = if self.vim.visual_mode {
            if args.repeat == 0 { 1 } else { args.repeat }
        } else {
            1
        };
        if self.vim.visual_block {
            let tab = self.config.tab_size;
            let indent = if self.config.indent_is_tab() {
                "\t".to_string()
            } else {
                " ".repeat(tab)
            };
            let ilen = clen(&indent);
            let mut cursor = ranges[0].0;
            for i in (0..ranges.len()).rev() {
                cursor = pos::min(ranges[i].0, ranges[i].1);
                if args.indent_right {
                    self.replace_range(&indent.repeat(repeat.max(0) as usize), cursor, Some(cursor));
                } else {
                    let text = self.line_chars(cursor.line);
                    let at = |k: i64| pos::char_at(&text, cursor.ch + k);
                    let mut end = 0i64;
                    for _ in 0..repeat {
                        match at(end) {
                            Some('\t') => end += 1,
                            Some(' ') => {
                                end += 1;
                                for _ in 1..ilen {
                                    if at(end) != Some(' ') {
                                        break;
                                    }
                                    end += 1;
                                }
                            }
                            _ => break,
                        }
                    }
                    self.replace_range("", cursor, Some(offset(cursor, 0, end)));
                }
            }
            return cursor;
        }
        for _ in 0..repeat {
            if args.indent_right {
                self.cmd_indent_more();
            } else {
                self.cmd_indent_less();
            }
        }
        let a = ranges[0].0;
        Pos::new(a.line, self.first_non_ws_of(a.line))
    }

    /// The adapter's `hardWrap(cm, {from, to})`: returns the row after the
    /// last wrapped one.
    pub(crate) fn hard_wrap(&mut self, from: i64, to: i64) -> i64 {
        let mut max = self.get_option_number("textwidth");
        if max <= 0 {
            max = 80;
        }
        let mut row = from.min(to);
        let mut end_row = from.max(to);
        while row <= end_row {
            let line = self.get_line(row);
            let lc: Vec<char> = line.chars().collect();
            if lc.len() as i64 > max {
                if let Some((s, e)) = find_space(&lc, max, 5) {
                    let indentation: String = lc.iter().take_while(|c| is_ws(**c)).collect();
                    self.replace_range(
                        &format!("\n{indentation}"),
                        Pos::new(row, s),
                        Some(Pos::new(row, e)),
                    );
                }
                end_row += 1;
            } else if lc.iter().any(|c| !is_ws(*c)) && row != end_row {
                let next = self.get_line(row + 1);
                if !next.is_empty() && next.chars().any(|c| !is_ws(c)) {
                    let trimmed = line.trim_end_matches(is_ws);
                    let trimmed_next = next.trim_start_matches(is_ws);
                    let merged: Vec<char> =
                        format!("{trimmed} {trimmed_next}").chars().collect();
                    let tl = clen(trimmed);
                    let space = find_space(&merged, max, 5);
                    if space.is_some_and(|(s, _)| s > tl) || (merged.len() as i64) < max {
                        self.replace_range(
                            " ",
                            Pos::new(row, tl),
                            Some(Pos::new(row + 1, clen(&next) - clen(trimmed_next))),
                        );
                        row -= 1;
                        end_row -= 1;
                    } else if tl < lc.len() as i64 {
                        self.replace_range(
                            "",
                            Pos::new(row, tl),
                            Some(Pos::new(row, lc.len() as i64)),
                        );
                    }
                }
            }
            row += 1;
        }
        row
    }
}

/// `findSpace(line, max, min)` of `hardWrap`.
fn find_space(line: &[char], max: i64, min: i64) -> Option<(i64, i64)> {
    if (line.len() as i64) < max {
        return None;
    }
    let m = max as usize;
    let before = &line[..m.min(line.len())];
    let after = &line[m.min(line.len())..];
    // spaceAfter = /^(?:(\s+)|(\S+)(\s+))/
    let lead_ws = after.iter().take_while(|c| is_ws(**c)).count();
    let (after_g1, after_g23) = if lead_ws > 0 {
        (Some(lead_ws), None)
    } else {
        let ns = after.iter().take_while(|c| !is_ws(**c)).count();
        let ws = after[ns..].iter().take_while(|c| is_ws(**c)).count();
        if ns > 0 && ws > 0 { (None, Some((ns, ws))) } else { (None, None) }
    };
    // spaceBefore = /(?:(\s+)|(\s+)(\S+))$/
    let trail_ws = before.iter().rev().take_while(|c| is_ws(**c)).count();
    let (before_g1, before_g2) = if trail_ws > 0 {
        (Some(trail_ws), None)
    } else {
        let ns = before.iter().rev().take_while(|c| !is_ws(**c)).count();
        let ws = before[..before.len() - ns]
            .iter()
            .rev()
            .take_while(|c| is_ws(**c))
            .count();
        if ws > 0 {
            let index = before.len() - ns - ws;
            (None, Some((index as i64, ws as i64)))
        } else {
            (None, None)
        }
    };
    let mut start = 0i64;
    let mut end = 0i64;
    if let Some(n) = before_g1 {
        start = max - n as i64;
        end = max;
    }
    if let Some(n) = after_g1 {
        if start == 0 {
            start = max;
        }
        end = max + n as i64;
    }
    if start != 0 {
        return Some((start, end));
    }
    if let Some((index, len)) = before_g2
        && index > min
    {
        return Some((index, index + len));
    }
    if let Some((ns, ws)) = after_g23 {
        let start = max + ns as i64;
        return Some((start, start + ws as i64));
    }
    None
}

fn swap_case(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        if is_upper(c) {
            out.extend(c.to_lowercase());
        } else {
            out.extend(c.to_uppercase());
        }
    }
    out
}

fn rot13(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            'A'..='Z' => (b'A' + (c as u8 - b'A' + 13) % 26) as char,
            'a'..='z' => (b'a' + (c as u8 - b'a' + 13) % 26) as char,
            _ => c,
        })
        .collect()
}

#[allow(dead_code)]
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_space_rules() {
        let l: Vec<char> = "aaaa bbbb cccc".chars().collect();
        assert_eq!(find_space(&l, 7, 5), Some((9, 10)));
        let l: Vec<char> = "aaaaaa bbbb".chars().collect();
        assert_eq!(find_space(&l, 6, 5), Some((6, 7)));
        assert_eq!(rot13("Hello"), "Uryyb");
        assert_eq!(swap_case("aB"), "Ab");
    }
}
