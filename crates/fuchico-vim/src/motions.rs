//! cm-vim `motions` and the text-finding helpers they use (`findWord`,
//! `moveToWord`, `findParagraph`, `findSentence`, `getSentence`,
//! `expandWordUnderCursor`, `selectCompanionObject`, `findSymbol` …).

use fuchico_helix::text::{count_column, find_column};

use crate::editor::Editor;
use crate::keymap::MotionKind;
use crate::pos::{
    self, INF, Pos, before, between, char_at, first_non_ws, index_of, is_lower_ascii, is_word_char,
    is_ws, last_index_of,
};
use crate::state::{Args, InputState};

/// A motion result: a new head, or `[anchor, head]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MotionResult {
    Head(Pos),
    Range(Pos, Pos),
}

impl MotionResult {
    pub fn head(self) -> Pos {
        match self {
            MotionResult::Head(p) | MotionResult::Range(_, p) => p,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct Word {
    pub from: i64,
    pub to: i64,
    pub line: i64,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct Span {
    pub start: Pos,
    pub end: Pos,
}

/// `wordCharTest` / `bigWordCharTest` (`None` = `""`).
fn word_test0(c: Option<char>) -> bool {
    c.is_some_and(is_word_char)
}
fn word_test1(c: Option<char>) -> bool {
    c.is_some_and(|c| !is_word_char(c) && !is_ws(c))
}
fn big_test(c: Option<char>) -> bool {
    c.is_some_and(|c| !is_ws(c))
}
fn ws_test(c: Option<char>) -> bool {
    c.is_some_and(is_ws)
}

/// `isWhiteSpaceString(line[i])`: `undefined` is not whitespace.
fn ws_str(c: Option<char>) -> bool {
    c.is_some_and(is_ws)
}

fn end_of_sentence(c: Option<char>) -> bool {
    matches!(c, Some('.' | '?' | '!'))
}

fn is_matchable_symbol(c: char) -> bool {
    "()[]{}".contains(c)
}

impl Editor {
    fn is_line(&self, n: i64) -> bool {
        n >= self.first_line() && n <= self.last_line()
    }

    /// `charCoords(pos).left` as a text column.
    pub(crate) fn col_of(&self, p: Pos) -> i64 {
        let text = self.get_line(p.line);
        let ch = p.ch.clamp(0, pos::clen(&text)) as usize;
        count_column(&text, self.config.tab_size, ch) as i64
    }

    /// `coordsChar` on line `line` at column `col`.
    fn ch_at_col(&self, line: i64, col: i64) -> i64 {
        let text = self.get_line(line);
        find_column(&text, col.max(0) as usize, self.config.tab_size, false).unwrap_or(0) as i64
    }

    /// `getHead(cm)`.
    pub(crate) fn get_head(&self) -> Pos {
        let mut cur = self.get_cursor("head");
        if pos::clen(&self.get_selection()) == 1 {
            cur = pos::min(cur, self.get_cursor("anchor"));
        }
        cur
    }

    /// Run a motion (`motions[name](cm, head, motionArgs, vim, inputState)`).
    pub(crate) fn run_motion(
        &mut self,
        kind: MotionKind,
        head: Pos,
        args: &mut Args,
        prev: &InputState,
    ) -> Option<MotionResult> {
        use MotionKind as M;
        use MotionResult::Head;
        match kind {
            M::MoveToTopLine => {
                let line = self.user_visible_lines().0 + args.repeat - 1;
                Some(Head(Pos::new(line, first_non_ws(&self.get_line(line)))))
            }
            M::MoveToMiddleLine => {
                let (t, b) = self.user_visible_lines();
                let line = (t + b) / 2;
                Some(Head(Pos::new(line, first_non_ws(&self.get_line(line)))))
            }
            M::MoveToBottomLine => {
                let line = self.user_visible_lines().1 - args.repeat + 1;
                Some(Head(Pos::new(line, first_non_ws(&self.get_line(line)))))
            }
            M::ExpandToLine => Some(Head(Pos::new(head.line + args.repeat - 1, INF))),
            M::FindNext => self.motion_find_next(args),
            M::FindAndSelectNextInclusive => self.motion_find_and_select_next(args, prev),
            M::GoToMark => {
                let c = args
                    .selected_character
                    .as_deref()
                    .and_then(|s| s.chars().next())
                    .unwrap_or('\0');
                let p = self.get_mark_pos(c)?;
                Some(Head(if args.linewise {
                    Pos::new(p.line, first_non_ws(&self.get_line(p.line)))
                } else {
                    p
                }))
            }
            M::MoveToOtherHighlightedEnd => {
                let (a, h) = (self.vim.sel_anchor, self.vim.sel_head);
                if self.vim.visual_block && args.same_line {
                    Some(MotionResult::Range(
                        self.clip_cursor_to_content(Pos::new(a.line, h.ch)),
                        self.clip_cursor_to_content(Pos::new(h.line, a.ch)),
                    ))
                } else {
                    Some(MotionResult::Range(h, a))
                }
            }
            M::JumpToMark => Some(Head(self.motion_jump_to_mark(head, args))),
            M::MoveByCharacters => {
                let ch = if args.forward {
                    head.ch + args.repeat
                } else {
                    head.ch - args.repeat
                };
                Some(Head(Pos::new(head.line, ch)))
            }
            M::MoveByLines => Some(Head(self.motion_move_by_lines(head, args))),
            M::MoveByDisplayLines => Some(Head(self.motion_move_by_display_lines(head, args))),
            M::MoveByPage => {
                let page = match self.host.visible_lines() {
                    Some((a, b)) if b > a => (b - a) as i64,
                    _ => (self.host.half_page_lines() as i64 * 2).max(1),
                };
                let n = page * args.repeat.max(1);
                let target = if args.forward { head.line + n } else { head.line - n };
                let target = target.clamp(0, self.last_line());
                let col = self.col_of(head);
                Some(Head(Pos::new(target, self.ch_at_col(target, col))))
            }
            M::MoveByParagraph => {
                let dir = if args.forward { 1 } else { -1 };
                Some(Head(self.find_paragraph(head, args.repeat, dir, false).start))
            }
            M::MoveBySentence => {
                let dir = if args.forward { 1 } else { -1 };
                Some(Head(self.find_sentence(head, args.repeat, dir)))
            }
            M::MoveByScroll => {
                if args.repeat == 0 {
                    args.repeat = self.host.half_page_lines().max(1) as i64;
                }
                let end = self.motion_move_by_display_lines(head, args);
                let delta = end.line - head.line;
                if delta != 0 {
                    self.scroll = Some(fuchico_helix::Scroll::Lines(delta as isize));
                }
                Some(Head(end))
            }
            M::MoveByWords => self
                .move_to_word(head, args.repeat, args.forward, args.word_end, args.big_word)
                .map(Head),
            M::MoveTillCharacter => {
                let cur = self.move_to_character(
                    args.repeat,
                    args.forward,
                    args.selected_character.as_deref(),
                    Some(head),
                );
                let inc = if args.forward { -1 } else { 1 };
                self.record_last_character_search(inc, args);
                let mut c = cur?;
                c.ch += inc;
                Some(Head(c))
            }
            M::MoveToCharacter => {
                self.record_last_character_search(0, args);
                Some(Head(
                    self.move_to_character(
                        args.repeat,
                        args.forward,
                        args.selected_character.as_deref(),
                        Some(head),
                    )
                    .unwrap_or(head),
                ))
            }
            M::MoveToSymbol => {
                let r = match args.selected_character.as_deref() {
                    Some(s) if !s.is_empty() => {
                        Some(self.find_symbol(args.repeat, args.forward, s.chars().next().unwrap()))
                    }
                    _ => None,
                };
                Some(Head(r.unwrap_or(head)))
            }
            M::MoveToColumn => {
                self.vim.last_h_pos = args.repeat - 1;
                self.vim.last_hs_pos = self.col_of(head);
                let line = self.head().line;
                Some(Head(self.clip_cursor_to_content(Pos::new(line, args.repeat - 1))))
            }
            M::MoveToEol => Some(Head(self.move_to_eol(head, args, false))),
            M::MoveToFirstNonWhiteSpaceCharacter => Some(Head(Pos::new(
                head.line,
                first_non_ws(&self.get_line(head.line)),
            ))),
            M::MoveToMatchedSymbol => self.motion_matched_symbol(head).map(Head),
            M::MoveToStartOfLine => Some(Head(Pos::new(head.line, 0))),
            M::MoveToLineOrEdgeOfDocument => {
                let mut n = if args.forward { self.last_line() } else { self.first_line() };
                if args.repeat_is_explicit {
                    n = args.repeat - 1;
                }
                Some(Head(Pos::new(n, first_non_ws(&self.get_line(n)))))
            }
            M::MoveToStartOfDisplayLine => {
                self.cmd_cursor_line_boundary(false);
                Some(Head(self.head()))
            }
            M::MoveToEndOfDisplayLine => {
                self.cmd_cursor_line_boundary(true);
                let cur = self.sel.main().head;
                if cur < self.doc.len() && self.doc.char_at(cur) != Some('\n') {
                    self.cmd_cursor_char(false);
                }
                Some(Head(self.head()))
            }
            M::TextObjectManipulation => self.motion_text_object(head, args),
            M::RepeatLastCharacterSearch => {
                let last = self.g.last_character_search.clone();
                let forward = args.forward == last.forward;
                let increment = i64::from(last.increment != 0) * if forward { -1 } else { 1 };
                self.move_h(-increment);
                args.inclusive = forward;
                let sc = if last.selected_character.is_empty() {
                    None
                } else {
                    Some(last.selected_character.as_str())
                };
                match self.move_to_character(args.repeat, forward, sc, None) {
                    None => {
                        self.move_h(increment);
                        Some(Head(head))
                    }
                    Some(mut c) => {
                        c.ch += increment;
                        Some(Head(c))
                    }
                }
            }
        }
    }

    fn record_last_character_search(&mut self, increment: i64, args: &Args) {
        let l = &mut self.g.last_character_search;
        l.increment = increment;
        l.forward = args.forward;
        l.selected_character = args.selected_character.clone().unwrap_or_default();
    }

    fn motion_jump_to_mark(&mut self, head: Pos, args: &Args) -> Pos {
        let mut best = head;
        let marks: Vec<(char, usize)> = self.vim.marks.iter().map(|(k, v)| (*k, *v)).collect();
        for _ in 0..args.repeat {
            let cursor = best;
            for (key, id) in &marks {
                if !is_lower_ascii(*key) {
                    continue;
                }
                let Some(mark) = self.find_marker(*id) else { continue };
                let wrong = if args.forward {
                    before(mark, cursor)
                } else {
                    before(cursor, mark)
                };
                if wrong {
                    continue;
                }
                if args.linewise && mark.line == cursor.line {
                    continue;
                }
                let equal = cursor == best;
                let btw = if args.forward {
                    between(cursor, mark, best)
                } else {
                    between(best, mark, cursor)
                };
                if equal || btw {
                    best = mark;
                }
            }
        }
        if args.linewise {
            best = Pos::new(best.line, first_non_ws(&self.get_line(best.line)));
        }
        best
    }

    fn motion_move_by_lines(&mut self, head: Pos, args: &Args) -> Pos {
        use MotionKind as M;
        let cur = head;
        let mut end_ch = cur.ch;
        match self.vim.last_motion {
            Some(
                M::MoveByLines
                | M::MoveByDisplayLines
                | M::MoveByScroll
                | M::MoveToColumn
                | M::MoveToEol,
            ) => end_ch = self.vim.last_h_pos,
            _ => self.vim.last_h_pos = end_ch,
        }
        let repeat = args.repeat + args.repeat_offset;
        let line = if args.forward { cur.line + repeat } else { cur.line - repeat };
        let first = self.first_line();
        let last = self.last_line();
        if line < first && cur.line == first {
            return Pos::new(head.line, 0);
        } else if line > last && cur.line == last {
            return self.move_to_eol(head, args, true);
        }
        if args.to_first_char {
            end_ch = first_non_ws(&self.get_line(line));
            self.vim.last_h_pos = end_ch;
        }
        self.vim.last_hs_pos = self.col_of(Pos::new(line, end_ch));
        Pos::new(line, end_ch)
    }

    fn motion_move_by_display_lines(&mut self, head: Pos, args: &Args) -> Pos {
        use MotionKind as M;
        match self.vim.last_motion {
            Some(
                M::MoveByDisplayLines
                | M::MoveByScroll
                | M::MoveByLines
                | M::MoveToColumn
                | M::MoveToEol,
            ) => {}
            _ => self.vim.last_hs_pos = self.col_of(head),
        }
        let amount = if args.forward { args.repeat } else { -args.repeat };
        let goal = self.vim.last_hs_pos;
        let (mut res, hit) = self.find_pos_v_line(head, amount, Some(goal));
        if hit {
            if args.forward {
                res = Pos::new(res.line, self.ch_at_col(res.line, goal));
            } else {
                res = Pos::new(self.first_line(), self.ch_at_col(self.first_line(), goal));
            }
        }
        self.vim.last_h_pos = res.ch;
        res
    }

    /// `moveToEol`.
    pub(crate) fn move_to_eol(&mut self, head: Pos, args: &Args, keep_h_pos: bool) -> Pos {
        let retval = Pos::new(head.line + args.repeat - 1, INF);
        let mut end = self.clip_pos(retval);
        end.ch -= 1;
        if !keep_h_pos {
            self.vim.last_h_pos = INF;
            self.vim.last_hs_pos = self.col_of(end);
        }
        retval
    }

    /// `moveToCharacter(cm, repeat, forward, character, head)`.
    fn move_to_character(
        &self,
        repeat: i64,
        forward: bool,
        character: Option<&str>,
        head: Option<Pos>,
    ) -> Option<Pos> {
        let character = character?;
        let c = character.chars().next()?;
        let cur = head.unwrap_or_else(|| self.head());
        let mut start = cur.ch;
        let mut idx = None;
        for _ in 0..repeat {
            let line = self.line_chars(cur.line);
            let i = if forward {
                index_of(&line, c, start + 1)
            } else {
                last_index_of_js(&line, c, start - 1)
            };
            if i == -1 {
                return None;
            }
            start = i;
            idx = Some(i);
        }
        idx.map(|i| Pos::new(self.head().line, i))
    }

    fn motion_matched_symbol(&self, head: Pos) -> Option<Pos> {
        let line = self.line_chars(head.line);
        let mut ch = head.ch.max(0);
        while (ch as usize) < line.len() {
            if is_matchable_symbol(line[ch as usize]) {
                break;
            }
            ch += 1;
        }
        if (ch as usize) < line.len() {
            let off = self.index_from_pos(Pos::new(head.line, ch));
            self.match_bracket_at(off).map(|o| self.pos_from_index(o))
        } else {
            Some(head)
        }
    }

    /// `findMatchingBracket` (CM6 `matchBrackets` over plain text).
    fn match_bracket_at(&self, offset: usize) -> Option<usize> {
        const BRACKETS: &str = "()[]{}";
        let c = self.doc.char_at(offset)?;
        let b = BRACKETS.find(c)?;
        let forward = b % 2 == 0;
        let mut depth = 0i64;
        let max = 10000usize;
        if forward {
            let end = (offset + max).min(self.doc.len());
            let text: Vec<char> = self.doc.slice(offset, end).chars().collect();
            for (i, ch) in text.iter().enumerate() {
                let Some(f) = BRACKETS.find(*ch) else { continue };
                if f % 2 == 0 {
                    depth += 1;
                } else if depth == 1 {
                    return Some(offset + i);
                } else {
                    depth -= 1;
                }
            }
        } else {
            let start = (offset + 1).saturating_sub(max);
            let text: Vec<char> = self.doc.slice(start, offset + 1).chars().collect();
            for i in (0..text.len()).rev() {
                let Some(f) = BRACKETS.find(text[i]) else { continue };
                if f % 2 == 1 {
                    depth += 1;
                } else if depth == 1 {
                    return Some(start + i);
                } else {
                    depth -= 1;
                }
            }
        }
        None
    }

    fn motion_find_next(&mut self, args: &Args) -> Option<MotionResult> {
        let query = self.g.search_query.clone()?;
        let mut prev = !args.forward;
        if self.g.search_is_reversed {
            prev = !prev;
        }
        self.highlight_search_matches(&query);
        let r = self.find_next(prev, &query, args.repeat);
        if r.is_none() {
            let pcre = if self.get_option_bool("pcre") {
                " (set nopcre to use Vim regexps)"
            } else {
                ""
            };
            self.show_confirm(&format!("No match found {}{}", query.display(), pcre));
        }
        r.map(MotionResult::Head)
    }

    fn motion_find_and_select_next(
        &mut self,
        args: &Args,
        prev_input: &InputState,
    ) -> Option<MotionResult> {
        let query = self.g.search_query.clone()?;
        let mut prev = !args.forward;
        if self.g.search_is_reversed {
            prev = !prev;
        }
        let (from, to_) = self.find_next_from_and_to_inclusive(prev, &query, args.repeat)?;
        if prev_input.operator.is_some() {
            return Some(MotionResult::Range(from, to_));
        }
        let to = Pos::new(to_.line, to_.ch - 1);
        if self.vim.visual_mode {
            if self.vim.visual_line || self.vim.visual_block {
                self.vim.visual_line = false;
                self.vim.visual_block = false;
                self.signal_mode("visual", "");
            }
            let anchor = self.vim.sel_anchor;
            return Some(if self.g.search_is_reversed {
                if args.forward {
                    MotionResult::Range(anchor, from)
                } else {
                    MotionResult::Range(anchor, to)
                }
            } else if args.forward {
                MotionResult::Range(anchor, to)
            } else {
                MotionResult::Range(anchor, from)
            });
        }
        self.vim.visual_mode = true;
        self.vim.visual_line = false;
        self.vim.visual_block = false;
        self.signal_mode("visual", "");
        Some(if prev {
            MotionResult::Range(to, from)
        } else {
            MotionResult::Range(from, to)
        })
    }

    fn motion_text_object(&mut self, head: Pos, args: &mut Args) -> Option<MotionResult> {
        let mut character = args
            .selected_character
            .as_deref()
            .and_then(|s| s.chars().next())
            .unwrap_or('\0');
        if character == 'b' {
            character = '(';
        } else if character == 'B' {
            character = '{';
        }
        let inclusive = !args.text_object_inner;
        let mut mv = false;
        let tmp: Option<Span> = match character {
            '(' | ')' | '{' | '}' | '[' | ']' | '<' | '>' => {
                mv = true;
                let mut t = self.select_companion_object(head, character, inclusive);
                if t.is_none() {
                    let start = self.index_from_pos(head);
                    let found = self
                        .doc
                        .slice(start, self.doc.len())
                        .chars()
                        .position(|c| c == character);
                    if let Some(i) = found {
                        let p = self.pos_from_index(start + i);
                        t = self.select_companion_object(p, character, inclusive);
                    }
                }
                t
            }
            '\'' | '"' | '`' => {
                mv = true;
                Some(self.find_beginning_and_end(head, character, inclusive))
            }
            'w' | 'W' => {
                let mut repeat = if args.repeat == 0 { 1 } else { args.repeat };
                let mut tmp: Option<Span> = None;
                while repeat > 0 {
                    repeat -= 1;
                    let opts = WordOpts {
                        inclusive,
                        inner_word: !inclusive,
                        big_word: character == 'W',
                        no_symbol: character == 'W',
                        multiline: true,
                    };
                    let repeated = self.expand_word_under_cursor(opts, tmp.map(|t| t.end));
                    if let Some(r) = repeated {
                        match &mut tmp {
                            None => tmp = Some(r),
                            Some(t) => t.end = r.end,
                        }
                    }
                }
                tmp
            }
            'p' => {
                let mut t = self.find_paragraph(head, args.repeat, 0, inclusive);
                args.linewise = true;
                if self.vim.visual_mode {
                    if !self.vim.visual_line {
                        self.vim.visual_line = true;
                    }
                } else {
                    // `vim.inputState.operatorArgs` is the fresh input state
                    // here (cleared by evalInput), so cm-vim never sets it.
                    t.end.line -= 1;
                }
                Some(t)
            }
            't' => Some(Span { start: head, end: head }),
            's' => {
                let mut h = head;
                let content = self.line_chars(h.line);
                if h.ch > 0 && end_of_sentence(char_at(&content, h.ch)) {
                    h.ch -= 1;
                }
                let end = self.get_sentence(h, args.repeat, 1, inclusive);
                let mut start = self.get_sentence(h, args.repeat, -1, inclusive);
                if ws_str(char_at(&self.line_chars(start.line), start.ch))
                    && ws_str(char_at(&self.line_chars(end.line), end.ch - 1))
                {
                    start = Pos::new(start.line, start.ch + 1);
                }
                Some(Span { start, end })
            }
            _ => None,
        };
        let tmp = tmp?;
        if !self.vim.visual_mode {
            Some(MotionResult::Range(tmp.start, tmp.end))
        } else {
            let (a, h) = self.expand_selection(tmp.start, tmp.end, mv);
            Some(MotionResult::Range(a, h))
        }
    }

    // ---- finders -----------------------------------------------------------------

    /// `findWord`.
    pub(crate) fn find_word(
        &self,
        cur: Pos,
        forward: bool,
        big_word: bool,
        empty_line_is_word: bool,
    ) -> Option<Word> {
        let mut line_num = cur.line;
        let mut pos = cur.ch;
        let mut line = self.line_chars(line_num);
        let dir: i64 = if forward { 1 } else { -1 };
        let tests: &[fn(Option<char>) -> bool] =
            if big_word { &[big_test] } else { &[word_test0, word_test1] };
        if empty_line_is_word && line.is_empty() {
            line_num += dir;
            line = self.line_chars(line_num);
            if !self.is_line(line_num) {
                return None;
            }
            pos = if forward { 0 } else { line.len() as i64 };
        }
        loop {
            if empty_line_is_word && line.is_empty() {
                return Some(Word {
                    from: 0,
                    to: 0,
                    line: line_num,
                });
            }
            let stop = if dir > 0 { line.len() as i64 } else { -1 };
            let mut word_start;
            let mut word_end;
            while pos != stop {
                let mut found_word = false;
                let mut i = 0;
                while i < tests.len() && !found_word {
                    if tests[i](char_at(&line, pos)) {
                        word_start = pos;
                        while pos != stop && tests[i](char_at(&line, pos)) {
                            pos += dir;
                        }
                        word_end = pos;
                        found_word = word_start != word_end;
                        if word_start == cur.ch && line_num == cur.line && word_end == word_start + dir
                        {
                            i += 1;
                            continue;
                        } else {
                            return Some(Word {
                                from: word_start.min(word_end + 1),
                                to: word_start.max(word_end),
                                line: line_num,
                            });
                        }
                    }
                    i += 1;
                }
                if !found_word {
                    pos += dir;
                }
            }
            line_num += dir;
            if !self.is_line(line_num) {
                return None;
            }
            line = self.line_chars(line_num);
            pos = if dir > 0 { 0 } else { line.len() as i64 };
        }
    }

    /// `moveToWord`.
    pub(crate) fn move_to_word(
        &self,
        cur: Pos,
        mut repeat: i64,
        forward: bool,
        word_end: bool,
        big_word: bool,
    ) -> Option<Pos> {
        let cur_start = cur;
        let mut cur = cur;
        let mut words: Vec<Word> = Vec::new();
        if (forward && !word_end) || (!forward && word_end) {
            repeat += 1;
        }
        let empty_line_is_word = !(forward && word_end);
        for _ in 0..repeat {
            match self.find_word(cur, forward, big_word, empty_line_is_word) {
                None => {
                    let eod = self.line_length(self.last_line());
                    words.push(if forward {
                        Word {
                            line: self.last_line(),
                            from: eod,
                            to: eod,
                        }
                    } else {
                        Word {
                            line: 0,
                            from: 0,
                            to: 0,
                        }
                    });
                    break;
                }
                Some(w) => {
                    words.push(w);
                    cur = Pos::new(w.line, if forward { w.to - 1 } else { w.from });
                }
            }
        }
        let short_circuit = words.len() as i64 != repeat;
        let first = *words.first()?;
        let mut last = words.pop();
        if forward && !word_end {
            if !short_circuit && (first.from != cur_start.ch || first.line != cur_start.line) {
                last = words.pop();
            }
            last.map(|w| Pos::new(w.line, w.from))
        } else if forward && word_end {
            last.map(|w| Pos::new(w.line, w.to - 1))
        } else if !forward && word_end {
            if !short_circuit && (first.to != cur_start.ch || first.line != cur_start.line) {
                last = words.pop();
            }
            last.map(|w| Pos::new(w.line, w.to))
        } else {
            last.map(|w| Pos::new(w.line, w.from))
        }
    }

    /// `expandWordUnderCursor`.
    pub(crate) fn expand_word_under_cursor(&self, o: WordOpts, cursor: Option<Pos>) -> Option<Span> {
        let cur = cursor.unwrap_or_else(|| self.get_head());
        let line = self.line_chars(cur.line);
        let mut end_line = line.clone();
        let start_line_number = cur.line;
        let mut end_line_number = start_line_number;
        let mut idx = cur.ch;
        let mut word_on_next_line: Option<Word> = None;
        let mut test: fn(Option<char>) -> bool = if o.no_symbol { word_test0 } else { big_test };
        if o.inner_word && ws_test(char_at(&line, idx)) {
            test = ws_test;
        } else {
            while !test(char_at(&line, idx)) {
                idx += 1;
                if idx >= line.len() as i64 {
                    if !o.multiline {
                        return None;
                    }
                    idx -= 1;
                    word_on_next_line = self.find_word(cur, true, o.big_word, true);
                    break;
                }
            }
            if o.big_word {
                test = big_test;
            } else {
                test = word_test0;
                if !test(char_at(&line, idx)) {
                    test = word_test1;
                }
            }
        }
        let mut end = idx;
        let mut start = idx;
        while test(char_at(&line, start)) && start >= 0 {
            start -= 1;
        }
        start += 1;
        if let Some(w) = word_on_next_line {
            end = w.to;
            end_line_number = w.line;
            end_line = self.line_chars(end_line_number);
            if end_line.is_empty() && end == 0 {
                end += 1;
            }
        } else {
            while test(char_at(&line, end)) && end < line.len() as i64 {
                end += 1;
            }
        }
        if o.inclusive {
            let word_end = end;
            let starts_with_space = cur.ch <= start && ws_test(char_at(&line, cur.ch));
            if !starts_with_space {
                while ws_test(char_at(&end_line, end)) && end < end_line.len() as i64 {
                    end += 1;
                }
            }
            if word_end == end || starts_with_space {
                let word_start = start;
                while ws_test(char_at(&line, start - 1)) && start > 0 {
                    start -= 1;
                }
                if start == 0 && !starts_with_space {
                    start = word_start;
                }
            }
        }
        Some(Span {
            start: Pos::new(start_line_number, start),
            end: Pos::new(end_line_number, end),
        })
    }

    /// `findParagraph`.
    pub(crate) fn find_paragraph(&self, head: Pos, mut repeat: i64, dir: i64, inclusive: bool) -> Span {
        let mut line = head.line;
        let min = self.first_line();
        let max = self.last_line();
        let is_empty = |i: i64| self.get_line(i).is_empty();
        let is_boundary = |i: i64, dir: i64, any: bool| {
            if any {
                is_empty(i) != is_empty(i + dir)
            } else {
                !is_empty(i) && is_empty(i + dir)
            }
        };
        let mut i = line;
        if dir != 0 {
            while min <= i && i <= max && repeat > 0 {
                if is_boundary(i, dir, false) {
                    repeat -= 1;
                }
                i += dir;
            }
            return Span {
                start: Pos::new(i, 0),
                end: head,
            };
        }
        let mut inclusive = inclusive;
        if self.vim.visual_line && is_boundary(line, 1, true) {
            let anchor = self.vim.sel_anchor;
            if is_boundary(anchor.line, -1, true) && (!inclusive || anchor.line != line) {
                line += 1;
            }
        }
        let mut start_state = is_empty(line);
        i = line;
        while i <= max && repeat != 0 {
            if is_boundary(i, 1, true) && (!inclusive || is_empty(i) != start_state) {
                repeat -= 1;
            }
            i += 1;
        }
        let end = Pos::new(i, 0);
        if i > max && !start_state {
            start_state = true;
        } else {
            inclusive = false;
        }
        i = line;
        while i > min {
            if (!inclusive || is_empty(i) == start_state || i == line) && is_boundary(i, -1, true) {
                break;
            }
            i -= 1;
        }
        Span {
            start: Pos::new(i, 0),
            end,
        }
    }

    /// `getSentence` (the `is`/`as` text objects).
    fn get_sentence(&self, cur: Pos, mut repeat: i64, dir: i64, inclusive: bool) -> Pos {
        // returns (ln, pos)
        let next_char = |line: &mut Option<Vec<char>>, pos: &mut i64, dir: i64| {
            if let Some(l) = line {
                if *pos + dir < 0 || *pos + dir >= l.len() as i64 {
                    *line = None;
                } else {
                    *pos += dir;
                }
            }
        };
        let forward = |ln: i64, pos: i64| -> (i64, i64) {
            let l = self.line_chars(ln);
            if l.is_empty() {
                return (ln, pos);
            }
            let mut line = Some(l);
            let mut p = pos;
            let mut last = p;
            next_char(&mut line, &mut p, dir);
            while let Some(l) = line.clone() {
                last = p;
                if end_of_sentence(char_at(&l, p)) {
                    if !inclusive {
                        return (ln, p + 1);
                    }
                    next_char(&mut line, &mut p, dir);
                    while let Some(l2) = line.clone() {
                        if ws_str(char_at(&l2, p)) {
                            last = p;
                            next_char(&mut line, &mut p, dir);
                        } else {
                            break;
                        }
                    }
                    return (ln, last + 1);
                }
                next_char(&mut line, &mut p, dir);
            }
            (ln, last + 1)
        };
        let reverse = |ln: i64, pos: i64| -> (i64, i64) {
            let orig = self.line_chars(ln);
            if orig.is_empty() {
                return (ln, pos);
            }
            let mut line = Some(orig.clone());
            let mut p = pos;
            let mut last = p;
            next_char(&mut line, &mut p, dir);
            while let Some(l) = line.clone() {
                let c = char_at(&l, p);
                if !ws_str(c) && !end_of_sentence(c) {
                    last = p;
                } else if end_of_sentence(c) {
                    if !inclusive {
                        return (ln, last);
                    } else if ws_str(char_at(&l, p + 1)) {
                        return (ln, p + 1);
                    } else {
                        return (ln, last);
                    }
                }
                next_char(&mut line, &mut p, dir);
            }
            if inclusive && ws_str(char_at(&orig, p)) {
                (ln, p)
            } else {
                (ln, last)
            }
        };
        let mut idx = (cur.line, cur.ch);
        while repeat > 0 {
            idx = if dir < 0 { reverse(idx.0, idx.1) } else { forward(idx.0, idx.1) };
            repeat -= 1;
        }
        Pos::new(idx.0, idx.1)
    }

    /// `findSentence` (`(` / `)`).
    fn find_sentence(&self, cur: Pos, mut repeat: i64, dir: i64) -> Pos {
        struct Idx {
            line: Option<Vec<char>>,
            ln: i64,
            pos: i64,
        }
        let next_char = |idx: &mut Idx| {
            let Some(l) = &idx.line else { return };
            if idx.pos + dir < 0 || idx.pos + dir >= l.len() as i64 {
                idx.ln += dir;
                if !self.is_line(idx.ln) {
                    idx.line = None;
                    return;
                }
                let nl = self.line_chars(idx.ln);
                idx.pos = if dir > 0 { 0 } else { nl.len() as i64 - 1 };
                idx.line = Some(nl);
            } else {
                idx.pos += dir;
            }
        };
        let forward = |ln: i64, pos: i64| -> (i64, i64) {
            let line = self.line_chars(ln);
            let mut stop = line.is_empty();
            let skip_empty = line.is_empty();
            let mut curr = Idx {
                line: Some(line),
                ln,
                pos,
            };
            let mut last = (ln, pos);
            next_char(&mut curr);
            while let Some(l) = curr.line.clone() {
                last = (curr.ln, curr.pos);
                let c = char_at(&l, curr.pos);
                if (l.is_empty() && !skip_empty) || (stop && !l.is_empty() && !ws_str(c)) {
                    return (curr.ln, curr.pos);
                } else if end_of_sentence(c)
                    && !stop
                    && (curr.pos == l.len() as i64 - 1 || ws_str(char_at(&l, curr.pos + 1)))
                {
                    stop = true;
                }
                next_char(&mut curr);
            }
            let line = self.line_chars(last.0);
            let mut p = 0;
            for i in (0..line.len()).rev() {
                if !is_ws(line[i]) {
                    p = i as i64;
                    break;
                }
            }
            (last.0, p)
        };
        let reverse = |ln: i64, pos: i64| -> (i64, i64) {
            let line = self.line_chars(ln);
            let mut skip_empty = line.is_empty();
            let mut curr = Idx {
                line: Some(line),
                ln,
                pos,
            };
            let mut last_ln = ln;
            let mut last_pos: Option<i64> = None;
            next_char(&mut curr);
            while let Some(l) = curr.line.clone() {
                let c = char_at(&l, curr.pos);
                if l.is_empty() && !skip_empty {
                    return match last_pos {
                        Some(p) => (last_ln, p),
                        None => (curr.ln, curr.pos),
                    };
                } else if end_of_sentence(c)
                    && last_pos.is_some()
                    && !(curr.ln == last_ln && Some(curr.pos + 1) == last_pos)
                {
                    return (last_ln, last_pos.unwrap());
                } else if !l.is_empty() && !ws_str(c) {
                    skip_empty = false;
                    last_ln = curr.ln;
                    last_pos = Some(curr.pos);
                }
                next_char(&mut curr);
            }
            let line = self.line_chars(last_ln);
            let mut p = 0;
            for (i, c) in line.iter().enumerate() {
                if !is_ws(*c) {
                    p = i as i64;
                    break;
                }
            }
            (last_ln, p)
        };
        let mut idx = (cur.line, cur.ch);
        while repeat > 0 {
            idx = if dir < 0 { reverse(idx.0, idx.1) } else { forward(idx.0, idx.1) };
            repeat -= 1;
        }
        Pos::new(idx.0, idx.1)
    }

    /// `scanForBracket(cm, where, dir, undefined, {bracketRegex})`.
    fn scan_for_bracket(&self, at: Pos, dir: i64, set: &[char]) -> Option<Pos> {
        let matching = |c: char| -> Option<&'static str> {
            Some(match c {
                '(' => ")>",
                ')' => "(<",
                '[' => "]>",
                ']' => "[<",
                '{' => "}>",
                '}' => "{<",
                '<' => ">>",
                '>' => "<<",
                _ => return None,
            })
        };
        let max_lines = 1000;
        let line_end = if dir > 0 {
            (at.line + max_lines).min(self.last_line() + 1)
        } else {
            (self.first_line() - 1).max(at.line - max_lines)
        };
        let mut stack: Vec<char> = Vec::new();
        let mut line_no = at.line;
        while line_no != line_end {
            let line = self.line_chars(line_no);
            if !line.is_empty() && line.len() <= 10000 {
                let mut p = if dir > 0 { 0 } else { line.len() as i64 - 1 };
                let end = if dir > 0 { line.len() as i64 } else { -1 };
                if line_no == at.line {
                    p = at.ch - if dir < 0 { 1 } else { 0 };
                }
                while if dir > 0 { p < end } else { p > end } {
                    if let Some(ch) = char_at(&line, p)
                        && set.contains(&ch)
                    {
                        let m = matching(ch);
                        if m.is_some_and(|m| m.ends_with('>') == (dir > 0)) {
                            stack.push(ch);
                        } else if stack.is_empty() {
                            return Some(Pos::new(line_no, p));
                        } else {
                            stack.pop();
                        }
                    }
                    p += dir;
                }
            }
            line_no += dir;
        }
        None
    }

    /// `selectCompanionObject`.
    fn select_companion_object(&self, head: Pos, symb: char, inclusive: bool) -> Option<Span> {
        let (set, open): (&[char], char) = match symb {
            '(' | ')' => (&['(', ')'], '('),
            '[' | ']' => (&['[', ']'], '['),
            '{' | '}' => (&['{', '}'], '{'),
            _ => (&['<', '>'], '<'),
        };
        let cur_char = char_at(&self.line_chars(head.line), head.ch);
        let off = i64::from(cur_char == Some(open));
        let at = Pos::new(head.line, head.ch + off);
        let s = self.scan_for_bracket(at, -1, set)?;
        let e = self.scan_for_bracket(at, 1, set)?;
        let (mut start, mut end) = (s, e);
        if (start.line == end.line && start.ch > end.ch) || start.line > end.line {
            std::mem::swap(&mut start, &mut end);
        }
        if inclusive {
            end.ch += 1;
        } else {
            start.ch += 1;
        }
        Some(Span { start, end })
    }

    /// `findBeginningAndEnd` (quotes).
    fn find_beginning_and_end(&self, head: Pos, symb: char, inclusive: bool) -> Span {
        let mut cur = head;
        let chars = self.line_chars(cur.line);
        let first = index_of(&chars, symb, 0);
        let mut start: Option<i64> = None;
        let mut end: Option<i64> = None;
        if cur.ch < first {
            cur.ch = first;
        } else if first < cur.ch && char_at(&chars, cur.ch) == Some(symb) {
            end = Some(cur.ch);
            cur.ch -= 1;
        }
        if char_at(&chars, cur.ch) == Some(symb) && end.is_none() {
            start = Some(cur.ch + 1);
        } else {
            let mut i = cur.ch;
            while i > -1 && start.is_none() {
                if char_at(&chars, i) == Some(symb) {
                    start = Some(i + 1);
                }
                i -= 1;
            }
        }
        if let Some(s) = start
            && end.is_none()
        {
            let mut i = s;
            while i < chars.len() as i64 && end.is_none() {
                if chars[i as usize] == symb {
                    end = Some(i);
                }
                i += 1;
            }
        }
        let (Some(mut s), Some(mut e)) = (start, end) else {
            return Span { start: cur, end: cur };
        };
        if inclusive {
            s -= 1;
            e += 1;
        }
        Span {
            start: Pos::new(cur.line, s),
            end: Pos::new(cur.line, e),
        }
    }

    /// `findSymbol` (`[(`, `])`, `[{`, `]m` …).
    fn find_symbol(&self, mut repeat: i64, forward: bool, symb: char) -> Pos {
        #[derive(Clone, Copy, PartialEq)]
        enum Mode {
            Bracket,
            Section,
            Comment,
            Method,
            Preprocess,
        }
        let mut cur = self.head();
        let inc: i64 = if forward { 1 } else { -1 };
        let end_line = if forward { self.line_count() } else { -1 };
        let mut line = cur.line;
        let mut line_text = self.line_chars(line);
        let mode = match symb {
            '(' | ')' | '{' | '}' => Mode::Bracket,
            '[' | ']' => Mode::Section,
            '*' | '/' => Mode::Comment,
            'm' | 'M' => Mode::Method,
            '#' => Mode::Preprocess,
            _ => return cur,
        };
        let mut index = cur.ch;
        let mut next_ch = char_at(&line_text, index);
        let mut last_ch: Option<char> = None;
        let mut symb = symb;
        let mut reverse_symb = if forward {
            match symb {
                ')' => Some('('),
                '}' => Some('{'),
                _ => None,
            }
        } else {
            match symb {
                '(' => Some(')'),
                '{' => Some('}'),
                _ => None,
            }
        };
        let mut depth = 0i64;
        let mut cur_move_through = false;
        match mode {
            Mode::Section => {
                cur_move_through = true;
                symb = if (if forward { ']' } else { '[' }) == symb { '{' } else { '}' };
            }
            Mode::Method => {
                symb = if symb == 'm' { '{' } else { '}' };
                reverse_symb = Some(if symb == '{' { '}' } else { '{' });
            }
            Mode::Preprocess => index = 0,
            _ => {}
        }
        while line != end_line && repeat != 0 {
            index += inc;
            next_ch = char_at(&line_text, index);
            if next_ch.is_none() {
                line += inc;
                line_text = self.line_chars(line);
                if inc > 0 {
                    index = 0;
                } else {
                    let n = line_text.len() as i64;
                    index = if n > 0 { n - 1 } else { 0 };
                }
                next_ch = char_at(&line_text, index);
            }
            let complete = match mode {
                Mode::Bracket => {
                    if next_ch == Some(symb) {
                        depth += 1;
                        depth >= 1
                    } else {
                        if next_ch.is_some() && next_ch == reverse_symb {
                            depth -= 1;
                        }
                        false
                    }
                }
                Mode::Section => index == 0 && next_ch == Some(symb),
                Mode::Comment => {
                    let found = last_ch == Some('*') && next_ch == Some('/');
                    last_ch = next_ch;
                    found
                }
                Mode::Method => next_ch == Some(symb),
                Mode::Preprocess => {
                    let mut done = false;
                    if next_ch == Some('#') {
                        let token: String = if line_text.first() == Some(&'#') {
                            line_text[1..].iter().take_while(|c| pos::is_latin_word(**c)).collect()
                        } else {
                            String::new()
                        };
                        if token == "endif" {
                            if forward && depth == 0 {
                                done = true;
                            } else {
                                depth += 1;
                            }
                        } else if token == "if" {
                            if !forward && depth == 0 {
                                done = true;
                            } else {
                                depth -= 1;
                            }
                        }
                        if !done && token == "else" && depth == 0 {
                            done = true;
                        }
                    }
                    done
                }
            };
            if complete {
                cur.line = line;
                cur.ch = index;
                repeat -= 1;
            }
        }
        if next_ch.is_some() || cur_move_through {
            return Pos::new(line, index);
        }
        cur
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct WordOpts {
    pub inclusive: bool,
    pub inner_word: bool,
    pub big_word: bool,
    pub no_symbol: bool,
    pub multiline: bool,
}

/// `line.lastIndexOf(c, from)`: a negative `from` searches only index 0.
fn last_index_of_js(line: &[char], c: char, from: i64) -> i64 {
    if from < 0 {
        return if line.first() == Some(&c) { 0 } else { -1 };
    }
    last_index_of(line, c, from)
}
