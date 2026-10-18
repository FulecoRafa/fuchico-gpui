//! Normal/select/insert command implementations (cm-helix
//! `helixCommandBindings` and the helpers they call).

use crate::change::{Change, ChangeSet, diff};
use crate::editor::{Editor, Tr};
use crate::effect::{Effect, External, Scroll, ScrollAlign, SearchKind};
use crate::history::Checkpoint;
use crate::jsnum::{js_number, js_number_to_string, js_trim, js_trim_start};
use crate::keymap::Cmd;
use crate::markup::parse_prefix;
use crate::mode::{Expecting, Minor, Mode, ModeType};
use crate::search::{SearchQuery, escape_regex};
use crate::selection::{Range, Selection, internal_to_cm};
use crate::text::{
    CharCategory, Text, categorize, categorize_long, char_len, count_column, find_column,
    first_non_ws, grapheme_count, move_by_chars, move_by_group_with, next_cluster_break,
};

pub(crate) fn ncb(doc: &Text, pos: usize, forward: bool) -> usize {
    next_cluster_break(doc, pos, forward)
}

pub(crate) fn exec(ed: &mut Editor, cmd: Cmd) {
    let mode = ed.mode;
    match cmd {
        // ---- insert ---------------------------------------------------------
        Cmd::InsBackspace => ed.delete_char(false),
        Cmd::InsDelete => ed.delete_char(true),
        Cmd::InsEnter => ed.insert_newline_and_indent(),
        Cmd::InsLeft => ed.cursor_char(false),
        Cmd::InsRight => ed.cursor_char(true),
        Cmd::InsUp => ed.cursor_line(false),
        Cmd::InsDown => ed.cursor_line(true),
        Cmd::InsHome => ed.cursor_line_boundary(false),
        Cmd::InsEnd => ed.cursor_line_boundary(true),
        Cmd::InsEscape => {
            if mode.expecting.is_some() {
                ed.dispatch(Tr::mode(Mode::INSERT));
                return;
            }
            ed.history.commit(&ed.doc);
            let sel = ed.map_sel(|r| {
                if r.is_empty() {
                    internal_to_cm(r, &ed.doc)
                } else {
                    r
                }
            });
            ed.dispatch(Tr::sel(sel).with_mode(Mode::NORMAL));
        }
        Cmd::InsRegister => ed.dispatch(Tr::mode(Mode {
            expecting: Some(Expecting::InsertRegister),
            ..Mode::INSERT
        })),
        Cmd::InsIndent => ed.indent_more(),
        Cmd::InsDedent => ed.indent_less(),
        Cmd::InsDeleteGroupBackward => ed.delete_group(false),
        Cmd::InsDeleteGroupForward => ed.delete_group(true),
        Cmd::InsDeleteLineStart => ed.delete_to_line_boundary(false),
        Cmd::InsDeleteLineEnd => ed.delete_to_line_boundary(true),

        // ---- normal ---------------------------------------------------------
        Cmd::Escape => {
            if !mode.is_plain_normal() {
                ed.dispatch(Tr::mode(Mode::NORMAL));
            }
        }
        Cmd::Search => ed.open_search(SearchKind::Forward),
        Cmd::SelectRegex => ed.open_search(SearchKind::Selection),
        Cmd::SplitRegex => ed.open_search(SearchKind::Split),
        Cmd::KeepRegex => ed.open_search(SearchKind::Keep),
        Cmd::RemoveRegex => ed.open_search(SearchKind::Remove),
        Cmd::SplitLines => split_lines(ed),
        Cmd::KeepPrimary => {
            if ed.sel.len() > 1 {
                let s = ed.sel.as_single();
                ed.dispatch(Tr::sel(s));
            }
        }
        Cmd::RotatePrev => rotate(ed, false),
        Cmd::RotateNext => rotate(ed, true),
        Cmd::Count(d) => {
            let d = d as usize;
            let next = match mode.count {
                Some(c) => c.saturating_mul(10).saturating_add(d),
                None => d,
            };
            if next != 0 {
                ed.dispatch(Tr::mode(Mode {
                    count: Some(next),
                    ..mode
                }));
            }
        }
        Cmd::CommandLine => {
            ed.dispatch(Tr::mode(mode.reset()));
            ed.open_command_prompt();
        }
        Cmd::Yank => {
            let msg = yank(ed, None);
            ed.message(msg);
        }
        Cmd::Append => {
            let sel = ed.map_sel(|r| Range::cursor(r.to()));
            ed.dispatch(Tr::sel(sel).with_mode(Mode::INSERT));
        }
        Cmd::AppendLineEnd => cursor_to_line_end(ed, true),
        Cmd::InsertLineStart => {
            let sel = ed.map_sel(|r| {
                let line = ed.doc.line_at(r.from());
                Range::cursor(line.from + first_non_ws(&ed.doc.line_text(&line)))
            });
            ed.dispatch(Tr::sel(sel).with_mode(Mode::INSERT));
        }
        Cmd::Insert => {
            let sel = ed.map_sel(|r| Range::cursor(r.from()));
            ed.dispatch(Tr::sel(sel).with_mode(Mode::INSERT));
        }
        Cmd::CopyDown => copy_selection_to_line(ed, true),
        Cmd::CopyUp => copy_selection_to_line(ed, false),
        Cmd::Change => remove_text(ed, true, true),
        Cmd::Delete => remove_text(ed, true, false),
        Cmd::ChangeNoYank => remove_text(ed, false, true),
        Cmd::DeleteNoYank => remove_text(ed, false, false),
        Cmd::PasteBefore | Cmd::PasteAfter => match ed.read_register(mode.register) {
            Ok(y) => ed.paste(y, cmd == Cmd::PasteBefore, mode.count(), true, true),
            Err(e) => ed.error(e),
        },
        Cmd::ReplaceWithYank => match ed.read_register(mode.register) {
            Ok(y) => replace_with_yanked(ed, y, mode.count()),
            Err(e) => ed.error(e),
        },
        Cmd::ReplaceChar => ed.dispatch(Tr::mode(Mode {
            expecting: Some(Expecting::Replace),
            ..mode
        })),
        Cmd::WordNext => move_by_group(ed, true, categorize),
        Cmd::WordPrev => move_by_group(ed, false, categorize),
        Cmd::LongWordNext => move_by_group(ed, true, categorize_long),
        Cmd::LongWordPrev => move_by_group(ed, false, categorize_long),
        Cmd::ToggleSelect => {
            let m = if mode.ty == ModeType::Normal {
                Mode::SELECT
            } else {
                Mode::NORMAL
            };
            ed.dispatch(Tr::mode(m));
        }
        Cmd::GotoMinor => set_minor(ed, Minor::Goto),
        Cmd::SpaceMinor => set_minor(ed, Minor::Space),
        Cmd::MatchMinor => set_minor(ed, Minor::Match),
        Cmd::ViewMinor => set_minor(ed, Minor::View),
        Cmd::LeftBracketMinor => set_minor(ed, Minor::LeftBracket),
        Cmd::RightBracketMinor => set_minor(ed, Minor::RightBracket),
        Cmd::Left => move_by_char(ed, false),
        Cmd::Right => move_by_char(ed, true),
        Cmd::Down => move_by_line(ed, true),
        Cmd::Up => move_by_line(ed, false),
        Cmd::SelectAll => {
            let len = ed.doc.len();
            ed.dispatch(Tr::sel(Selection::single(0, len)));
        }
        Cmd::OpenBelow => open_line(ed, true),
        Cmd::OpenAbove => open_line(ed, false),
        Cmd::Find { inclusive, forward } => ed.dispatch(Tr::mode(Mode {
            ty: mode.ty,
            minor: Some(Minor::Normal),
            count: mode.count,
            register: None,
            expecting: Some(Expecting::Find { inclusive, forward }),
        })),
        Cmd::Undo => undo(ed),
        Cmd::Redo => redo(ed),
        Cmd::ExtendLine => extend_line(ed),
        Cmd::SearchNext => search_next(ed),
        Cmd::SearchPrev => search_prev(ed),
        Cmd::Increment => change_number(ed, true),
        Cmd::Decrement => change_number(ed, false),
        Cmd::HalfPageDown => half_page(ed, true),
        Cmd::HalfPageUp => half_page(ed, false),
        Cmd::Collapse => {
            let doc = &ed.doc;
            let sel = ed.map_sel(|r| crate::selection::cm_to_internal(r, doc));
            let sel = crate::selection::map_sel(&sel, |r| Range::cursor(r.head));
            let sel = crate::selection::map_sel(&sel, |r| internal_to_cm(r, doc));
            ed.dispatch(Tr::sel(sel).scrolled());
        }
        Cmd::Flip => {
            let sel = ed.map_sel(|r| {
                if r.is_atomic() {
                    r
                } else {
                    Range::new(r.head, r.anchor)
                }
            });
            ed.dispatch(Tr::sel(sel).scrolled());
        }
        Cmd::EnsureForward => {
            let sel = ed.map_sel(|r| {
                if r.is_atomic() {
                    r
                } else {
                    Range::new(r.from(), r.to())
                }
            });
            ed.dispatch(Tr::sel(sel));
        }
        Cmd::ExpandSyntax => expand_syntax(ed),
        Cmd::ShrinkSyntax => {
            if ed.syntax.cursor > 0 {
                let s = ed.syntax.selections[ed.syntax.cursor - 1].clone();
                ed.dispatch(Tr {
                    selection: Some(s),
                    syntax: true,
                    ..Tr::default()
                });
                ed.syntax.cursor -= 1;
            }
        }
        Cmd::SiblingNext => sibling(ed, true),
        Cmd::SiblingPrev => sibling(ed, false),
        Cmd::ToggleComment => {
            ed.toggle_comment();
            ed.dispatch(Tr::mode(Mode::NORMAL));
        }
        Cmd::Indent => ed.indent_more(),
        Cmd::Dedent => ed.indent_less(),
        Cmd::Lower => change_case(ed, Some(false)),
        Cmd::Upper => change_case(ed, Some(true)),
        Cmd::SwitchCase => change_case(ed, None),
        Cmd::StarSearch => {
            let mut seen: Vec<String> = Vec::new();
            for t in ed.selection_texts() {
                let e = escape_regex(&t);
                if !seen.contains(&e) {
                    seen.push(e);
                }
            }
            let search = seen.join("|");
            ed.yank_to('/', vec![search.clone()]);
            ed.dispatch(Tr::default());
            ed.message(format!("register '/' set to '{search}'"));
        }
        Cmd::Trim => trim(ed),
        Cmd::LineStart => cursor_to_line_start(ed),
        Cmd::LineEnd => cursor_to_line_end(ed, false),
        Cmd::Join => join(ed),
        Cmd::SelectRegister => ed.dispatch(Tr::mode(Mode {
            expecting: Some(Expecting::Register),
            ..mode
        })),
        Cmd::Nop => {}

        // ---- goto -----------------------------------------------------------
        Cmd::DocStart => {
            let sel = if mode.ty == ModeType::Normal {
                Selection::single(0, 0)
            } else {
                let anchor = ed.internal(ed.sel.main()).anchor;
                Selection::single(anchor, 0)
            };
            let sel = crate::selection::map_sel(&sel, |r| ed.to_cm(r));
            ed.dispatch(Tr::sel(sel).scrolled());
            let m = ed.reset_mode();
            ed.dispatch(Tr::mode(m));
        }
        Cmd::LastLine => {
            let doc = &ed.doc;
            let start = ed.sel.ranges()[0].from();
            let last = doc.line(doc.lines());
            let line = if !last.is_empty() || doc.lines() == 1 {
                last
            } else {
                doc.line(doc.lines() - 1)
            };
            let r = if mode.ty == ModeType::Normal {
                Range::cursor(line.from)
            } else {
                Range::new(start, line.from)
            };
            let r = ed.to_cm(r);
            let m = ed.reset_mode();
            ed.dispatch(
                Tr::sel(Selection::single(r.anchor, r.head))
                    .with_mode(m)
                    .scrolled(),
            );
        }
        Cmd::GotoDown | Cmd::GotoUp => {
            move_by_line(ed, cmd == Cmd::GotoDown);
            let m = Mode::reset_for(mode.ty);
            ed.dispatch(Tr::mode(m));
        }
        Cmd::FirstNonBlank => {
            let normal = mode.ty == ModeType::Normal;
            let doc = &ed.doc;
            let sel = ed.map_sel(|r| {
                let s = crate::selection::cm_to_internal(r, doc);
                let line = doc.line_at(s.head);
                let pos = line.from + first_non_ws(&doc.line_text(&line));
                let next = if normal {
                    Range::cursor(pos)
                } else {
                    Range::new(s.anchor, pos)
                };
                internal_to_cm(next, doc)
            });
            let m = ed.reset_mode();
            ed.dispatch(Tr::sel(sel).with_mode(m).scrolled());
        }
        Cmd::BufferNext | Cmd::BufferPrevious => {
            let (enabled, ext) = if cmd == Cmd::BufferNext {
                (ed.config.externals.buffer_next, External::BufferNext)
            } else {
                (
                    ed.config.externals.buffer_previous,
                    External::BufferPrevious,
                )
            };
            if enabled {
                ed.emit(Effect::External(ext));
            }
            let m = ed.reset_mode();
            ed.dispatch(Tr::mode(m));
        }
        Cmd::WindowTop | Cmd::WindowCenter | Cmd::WindowBottom => {
            if let Some((first, last)) = ed.host.visible_lines() {
                let n = match cmd {
                    Cmd::WindowTop => first,
                    Cmd::WindowCenter => (first + last) / 2,
                    _ => last,
                }
                .clamp(1, ed.doc.lines());
                let pos = ed.doc.line(n).from;
                let r = if ed.is_select() {
                    Range::new(ed.internal(ed.sel.main()).anchor, pos)
                } else {
                    Range::cursor(pos)
                };
                let r = ed.to_cm(r);
                ed.dispatch(Tr::sel(Selection::single(r.anchor, r.head)).scrolled());
            }
            let m = ed.reset_mode();
            ed.dispatch(Tr::mode(m));
        }

        // ---- match ----------------------------------------------------------
        Cmd::SurroundAdd => set_match_expecting(ed, Expecting::Surround),
        Cmd::SelectAround => set_match_expecting(ed, Expecting::Delimiters { inclusive: true }),
        Cmd::SelectInside => set_match_expecting(ed, Expecting::Delimiters { inclusive: false }),
        Cmd::SurroundDelete => set_match_expecting(ed, Expecting::DeleteSurround),
        Cmd::SurroundReplace => set_match_expecting(ed, Expecting::ReplaceSurround(None)),
        Cmd::MatchBracket => ed.match_bracket(),

        // ---- space ----------------------------------------------------------
        Cmd::ClipboardYank => {
            let msg = yank(ed, Some('+'));
            ed.message(msg);
        }
        Cmd::ClipboardPasteAfter | Cmd::ClipboardPasteBefore => {
            ed.dispatch(Tr::mode(Mode::NORMAL));
            match ed.read_register(Some('+')) {
                Ok(y) => ed.paste(
                    y,
                    cmd == Cmd::ClipboardPasteBefore,
                    mode.count(),
                    true,
                    true,
                ),
                Err(e) => ed.error(e),
            }
        }
        Cmd::ClipboardReplace => {
            ed.dispatch(Tr::mode(Mode::NORMAL));
            match ed.read_register(Some('+')) {
                Ok(y) => {
                    let text = y.and_then(|v| v.into_iter().next()).unwrap_or_default();
                    ed.replace_selection(&text);
                }
                Err(e) => ed.error(e),
            }
        }
        Cmd::FilePicker | Cmd::BufferPicker => {
            let (enabled, ext) = if cmd == Cmd::FilePicker {
                (ed.config.externals.file_picker, External::FilePicker)
            } else {
                (ed.config.externals.buffer_picker, External::BufferPicker)
            };
            if enabled {
                ed.emit(Effect::External(ext));
            }
            let m = ed.reset_mode();
            ed.dispatch(Tr::mode(m));
        }
        Cmd::GlobalSearch => {
            if ed.config.externals.global_search {
                ed.open_search(SearchKind::Global);
            }
            let m = ed.reset_mode();
            ed.dispatch(Tr::mode(m));
        }
        Cmd::SpaceComment => {
            ed.dispatch(Tr::mode(Mode::NORMAL));
            ed.toggle_comment();
        }

        // ---- brackets -------------------------------------------------------
        Cmd::InsertLineAbove => insert_line(ed, false),
        Cmd::InsertLineBelow => insert_line(ed, true),

        // ---- view (extension) -------------------------------------------------
        Cmd::AlignCenter | Cmd::AlignTop | Cmd::AlignBottom => {
            let align = match cmd {
                Cmd::AlignCenter => ScrollAlign::Center,
                Cmd::AlignTop => ScrollAlign::Top,
                _ => ScrollAlign::Bottom,
            };
            let pos = ed.sel.main().head;
            ed.emit(Effect::Scroll(Scroll::To { pos, align }));
            let m = ed.reset_mode();
            ed.dispatch(Tr::mode(m));
        }
        Cmd::ScrollDown | Cmd::ScrollUp => {
            let n = mode.count() as isize;
            ed.emit(Effect::Scroll(Scroll::Lines(if cmd == Cmd::ScrollDown {
                n
            } else {
                -n
            })));
            let m = ed.reset_mode();
            ed.dispatch(Tr::mode(m));
        }
    }
}

fn set_minor(ed: &mut Editor, minor: Minor) {
    let m = Mode {
        minor: Some(minor),
        ..ed.mode
    };
    ed.dispatch(Tr::mode(m));
}

fn set_match_expecting(ed: &mut Editor, exp: Expecting) {
    let m = Mode {
        ty: ed.mode.ty,
        minor: Some(Minor::Match),
        count: None,
        register: None,
        expecting: Some(exp),
    };
    ed.dispatch(Tr::mode(m));
}

fn rotate(ed: &mut Editor, forward: bool) {
    let n = ed.sel.len() as isize;
    if n == 1 {
        return;
    }
    let mut main = (ed.sel.main_index() as isize + if forward { 1 } else { -1 }) % n;
    if main < 0 {
        main += n;
    }
    let s = Selection::create(ed.sel.ranges().to_vec(), main as usize);
    ed.dispatch(Tr::sel(s));
}

fn split_lines(ed: &mut Editor) {
    let doc = &ed.doc;
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for r in ed.sel.ranges() {
        let start = doc.line_at(r.from()).number;
        let end_line = doc.line_at(r.to());
        let end = if end_line.from == r.to() {
            end_line.number - 1
        } else {
            end_line.number
        };
        for l in start..=end {
            if seen.insert(l) {
                let line = doc.line(l);
                out.push(Range::new(line.from, line.to));
            }
        }
    }
    if out.is_empty() {
        return;
    }
    ed.dispatch(Tr::sel(Selection::create(out, 0)));
}

/// cm-helix `yank` (returns the status message).
fn yank(ed: &mut Editor, register: Option<char>) -> String {
    let reg = register
        .or(ed.mode.register)
        .unwrap_or(ed.config.default_register);
    let texts = ed.selection_texts();
    let total = texts.len();
    ed.yank_to(reg, texts);
    ed.dispatch(Tr::mode(Mode::NORMAL));
    if total == 1 {
        format!("yanked 1 selection to register {reg}")
    } else {
        format!("yanked {total} selections to register {reg}")
    }
}

/// cm-helix `yanksForSelection`.
pub(crate) fn yanks_for_selection(n: usize, yanked: Vec<String>) -> Vec<String> {
    use std::cmp::Ordering;
    match n.cmp(&yanked.len()) {
        Ordering::Equal => yanked,
        Ordering::Less => yanked.into_iter().take(n).collect(),
        Ordering::Greater => {
            let last = yanked.last().cloned().unwrap_or_default();
            let mut out = yanked;
            while out.len() < n {
                out.push(last.clone());
            }
            out
        }
    }
}

impl Editor {
    /// cm-helix `paste`.
    pub(crate) fn paste(
        &mut self,
        yanked: Option<Vec<String>>,
        before: bool,
        count: usize,
        reset: bool,
        select: bool,
    ) {
        let yanked = yanked.unwrap_or_else(|| vec![String::new()]);
        let yanks = yanks_for_selection(self.sel.len(), yanked);
        let mut changes = Vec::new();
        let mut ranges = Vec::new();
        let mut offset = 0;
        for (y, r) in yanks.iter().zip(self.sel.ranges()) {
            let pos = if before { r.from() } else { r.to() };
            changes.push(Change::insert(pos, y.repeat(count)));
            let len = char_len(y);
            let anchor = pos + offset;
            ranges.push(if select {
                Range::new(anchor, anchor + len)
            } else {
                Range::cursor(anchor + len)
            });
            offset += len;
        }
        let mut tr = Tr::changes(ChangeSet::new(changes))
            .with_sel(Selection::create(ranges, self.sel.main_index()));
        if reset {
            tr = tr.with_mode(Mode::NORMAL);
        }
        self.dispatch(tr);
    }

    /// cm-helix `findText`.
    pub(crate) fn find_text(&mut self, c: char, inclusive: bool, forward: bool) {
        let mode = self.mode;
        let select = mode.ty == ModeType::Select;
        let count = if mode.ty == ModeType::Insert {
            1
        } else {
            mode.count()
        };
        let chars: Vec<char> = self.doc.to_string().chars().collect();
        let doc = &self.doc;
        let sel = self.map_sel(|r| {
            let s = crate::selection::cm_to_internal(r, doc);
            let mut raw = s.head as isize;
            for _ in 0..count {
                if !forward && raw == 0 {
                    raw = -1;
                    break;
                }
                raw = if forward {
                    let from = (raw + 1) as usize;
                    chars
                        .iter()
                        .skip(from)
                        .position(|ch| *ch == c)
                        .map(|i| (i + from) as isize)
                        .unwrap_or(-1)
                } else {
                    let upto = ((raw - 1) as usize).min(chars.len().saturating_sub(1));
                    if chars.is_empty() {
                        -1
                    } else {
                        chars[..=upto]
                            .iter()
                            .rposition(|ch| *ch == c)
                            .map(|i| i as isize)
                            .unwrap_or(-1)
                    }
                };
                if raw < 0 {
                    break;
                }
            }
            if raw < 0 {
                return r;
            }
            let idx = if inclusive {
                raw
            } else if forward {
                raw - 1
            } else {
                raw + 1
            } as usize;
            let next = if select {
                Range::new(s.anchor, idx)
            } else {
                Range::new(s.head, idx)
            };
            internal_to_cm(next, doc)
        });
        let m = Mode::reset_for(mode.ty);
        self.dispatch(Tr::sel(sel).with_mode(m));
    }

    /// cm-helix `replaceWithChar`.
    pub(crate) fn replace_with_char(&mut self, c: char) {
        let main = self.sel.main();
        let len = grapheme_count(&self.range_text(main));
        let insert: String = std::iter::repeat_n(c, len).collect();
        self.dispatch(
            Tr::changes(ChangeSet::new(vec![Change::new(
                main.from(),
                main.to(),
                insert,
            )]))
            .with_mode(Mode::NORMAL),
        );
    }

    /// cm-helix `surround`.
    pub(crate) fn surround(&mut self, c: char) {
        let (open, close) = crate::textobj::pair_for(c);
        let tr = self.change_by_range(|_, r| {
            let (a, h) = if r.is_forward() || r.is_empty() {
                (r.anchor, r.head + 2)
            } else {
                (r.anchor + 2, r.head)
            };
            (
                vec![
                    Change::insert(r.from(), open.to_string()),
                    Change::insert(r.to(), close.to_string()),
                ],
                Range::new(a, h),
            )
        });
        self.dispatch(tr.with_mode(Mode::NORMAL));
    }
}

fn remove_text(ed: &mut Editor, yank: bool, edit: bool) {
    if yank {
        let texts = ed.selection_texts();
        let reg = ed.config.default_register;
        ed.yank_to(reg, texts);
    }
    let changes: Vec<Change> = ed
        .sel
        .ranges()
        .iter()
        .map(|r| Change::delete(r.from(), r.to()))
        .collect();
    let mut tr = Tr::changes(ChangeSet::new(changes));
    if edit {
        tr = tr.with_mode(Mode::INSERT);
    }
    ed.dispatch(tr);
    if !edit && ed.sel.ranges().iter().any(|r| r.is_empty()) {
        let sel = ed.map_sel(|r| ed.to_cm(r));
        ed.dispatch(Tr::sel(sel));
    }
}

fn replace_with_yanked(ed: &mut Editor, yanked: Option<Vec<String>>, count: usize) {
    let Some(yanked) = yanked else {
        return;
    };
    let yanks = yanks_for_selection(ed.sel.len(), yanked);
    let reps: Vec<String> = if count == 1 {
        yanks
    } else {
        yanks.iter().map(|y| y.repeat(count)).collect()
    };
    let mut i = 0;
    let tr = ed.change_by_range(|_, r| {
        let insert = reps[i].clone();
        i += 1;
        if insert.is_empty() {
            return (Vec::new(), r);
        }
        let len = char_len(&insert);
        (
            vec![Change::new(r.from(), r.to(), insert)],
            Range::new(r.from(), r.from() + len),
        )
    });
    ed.dispatch(tr.with_mode(Mode::NORMAL));
}

fn move_by_char(ed: &mut Editor, forward: bool) {
    let mode = ed.mode;
    let count = mode.count();
    let select = mode.ty == ModeType::Select;
    let doc = &ed.doc;
    let next = ed.map_sel(|r| {
        let initial = crate::selection::cm_to_internal(r, doc);
        let moved = move_by_chars(doc, initial.head, forward, count);
        if select {
            internal_to_cm(Range::new(initial.anchor, moved), doc)
        } else {
            internal_to_cm(Range::cursor(moved), doc)
        }
    });
    if ed.sel.same(&next) {
        return;
    }
    ed.dispatch(Tr::sel(next).with_mode(mode.without_count()).scrolled());
}

fn move_by_line(ed: &mut Editor, forward: bool) {
    let mode = ed.mode;
    let count = mode.count();
    let select = mode.ty == ModeType::Select;
    let tab = ed.config.tab_size;
    let doc = &ed.doc;
    let host = &mut ed.host;
    let next = crate::selection::map_sel(&ed.sel, |r| {
        let initial = crate::selection::cm_to_internal(r, doc);
        let mut head = initial.head;
        let mut goal = initial.goal;
        for _ in 0..count {
            if !select {
                let line = doc.line_at(head).number;
                if (forward && line == doc.lines()) || (!forward && line == 1) {
                    break;
                }
            }
            let (p, g) = host.move_vertically(doc, head, goal, forward, 1, tab);
            if select && p == head {
                break;
            }
            head = p;
            goal = g;
        }
        if select {
            internal_to_cm(Range::with_goal(initial.anchor, head, goal), doc)
        } else {
            internal_to_cm(Range::with_goal(head, head, goal), doc)
        }
    });
    if ed.sel.same(&next) {
        return;
    }
    ed.dispatch(Tr::sel(next).with_mode(mode.without_count()).scrolled());
}

fn half_page(ed: &mut Editor, forward: bool) {
    let select = ed.is_select();
    let tab = ed.config.tab_size;
    let half = ed.host.half_page_lines();
    let doc = &ed.doc;
    let host = &mut ed.host;
    let next = crate::selection::map_sel(&ed.sel, |r| {
        let s = crate::selection::cm_to_internal(r, doc);
        let dist = host.lines_to_boundary(doc, s.head, forward);
        let height = half.min(dist);
        if height < 1 {
            return r;
        }
        let (p, g) = host.move_vertically(doc, s.head, s.goal, forward, height, tab);
        if select {
            internal_to_cm(Range::with_goal(s.anchor, p, g), doc)
        } else {
            internal_to_cm(Range::with_goal(p, p, g), doc)
        }
    });
    if ed.sel.same(&next) {
        return;
    }
    ed.dispatch(Tr::sel(next).scrolled());
}

fn move_by_group(ed: &mut Editor, forward: bool, cat: fn(&str) -> CharCategory) {
    let normal = ed.mode.ty == ModeType::Normal;
    let doc = &ed.doc;
    let ranges: Vec<Range> = ed
        .sel
        .ranges()
        .iter()
        .map(|&r| {
            let range_forward = r.is_forward();
            let (hc, ac) = if r.is_atomic() {
                (r, r)
            } else {
                (
                    Range::new(ncb(doc, r.head, !range_forward), r.head),
                    Range::new(r.anchor, ncb(doc, r.anchor, range_forward)),
                )
            };
            let mut na = if forward { hc.from() } else { hc.to() };
            let mut nh = move_by_group_with(doc, na, forward, cat);
            let old_end = if forward { hc.to() } else { hc.from() };
            if nh == old_end {
                na = nh;
                nh = move_by_group_with(doc, na, forward, cat);
            }
            let nr = Range::new(na, nh);
            if normal {
                return nr;
            }
            let nhc = if nr.is_atomic() {
                nr
            } else {
                Range::new(ncb(doc, nr.head, !forward), nr.head)
            };
            if nhc.to() < ac.from() {
                Range::new(ac.to(), nhc.from())
            } else {
                Range::new(ac.from(), nhc.to())
            }
        })
        .collect();
    let s = Selection::create(ranges, ed.sel.main_index());
    ed.dispatch(Tr::sel(s));
}

fn cursor_to_line_start(ed: &mut Editor) {
    let normal = ed.mode.ty == ModeType::Normal;
    let doc = &ed.doc;
    let s = ed.map_sel(|r| crate::selection::cm_to_internal(r, doc));
    let s = Selection::create(
        s.ranges()
            .iter()
            .map(|r| {
                let from = doc.line_at(r.head).from;
                if normal {
                    Range::cursor(from)
                } else {
                    Range::new(r.anchor, from)
                }
            })
            .collect(),
        s.main_index(),
    );
    let s = crate::selection::map_sel(&s, |r| internal_to_cm(r, doc));
    let m = ed.reset_mode();
    ed.dispatch(Tr::sel(s).with_mode(m).scrolled());
}

fn cursor_to_line_end(ed: &mut Editor, insert: bool) {
    let ty = if insert { ModeType::Insert } else { ed.mode.ty };
    let doc = &ed.doc;
    let s = ed.map_sel(|r| {
        let s = crate::selection::cm_to_internal(r, doc);
        let line = doc.line_at(s.head);
        let next = if line.is_empty() {
            s
        } else {
            let goal = if ty == ModeType::Insert {
                line.to
            } else {
                ncb(doc, line.to, false)
            };
            if ty == ModeType::Select {
                Range::with_goal(s.anchor, goal, s.goal)
            } else {
                Range::with_goal(goal, goal, s.goal)
            }
        };
        if insert {
            next
        } else {
            internal_to_cm(next, doc)
        }
    });
    let m = if insert {
        Mode::INSERT
    } else if ed.is_select() {
        Mode::SELECT
    } else {
        Mode::NORMAL
    };
    ed.dispatch(Tr::sel(s).with_mode(m));
}

fn copy_selection_to_line(ed: &mut Editor, down: bool) {
    let doc = &ed.doc;
    let tab = ed.config.tab_size;
    let main = ed.sel.main();
    let current = doc.line_at(main.head);
    if (down && current.number >= doc.lines()) || (!down && current.number <= 1) {
        return;
    }
    let text = doc.line_text(&current);
    let col = count_column(&text, tab, main.head - current.from);
    let anchor_col = count_column(&text, tab, main.anchor.saturating_sub(current.from));
    let lines: Box<dyn Iterator<Item = usize>> = if down {
        Box::new(current.number + 1..=doc.lines())
    } else {
        Box::new((1..current.number).rev())
    };
    for n in lines {
        let line = doc.line(n);
        let lt = doc.line_text(&line);
        let Some(h) = find_column(&lt, col, tab, true) else {
            continue;
        };
        let Some(a) = find_column(&lt, anchor_col, tab, true) else {
            continue;
        };
        let (nh, na) = (line.from + h, line.from + a);
        let r = if main.head < main.anchor {
            Range::new(nh, na)
        } else {
            Range::new(na, nh)
        };
        let s = ed.sel.add_range(r, true);
        ed.dispatch(Tr::sel(s));
        break;
    }
}

fn open_line(ed: &mut Editor, below: bool) {
    let doc = &ed.doc;
    let s = crate::selection::cm_to_internal(ed.sel.main(), doc);
    let markup = ed.config.continue_markup;
    let (from, insert, cursor) = if below {
        let line = doc.line_at(s.to());
        let cont = if markup {
            parse_prefix(&doc.line_text(&line)).continuation(true)
        } else {
            String::new()
        };
        let cursor = line.to + 1 + char_len(&cont);
        (line.to, format!("\n{cont}"), cursor)
    } else {
        let line = doc.line_at(s.from());
        let cont = if markup {
            parse_prefix(&doc.line_text(&line)).continuation(false)
        } else {
            String::new()
        };
        let cursor = line.from + char_len(&cont);
        (line.from, format!("{cont}\n"), cursor)
    };
    ed.dispatch(
        Tr::changes(ChangeSet::new(vec![Change::insert(from, insert)]))
            .with_sel(Selection::single(cursor, cursor))
            .with_mode(Mode::INSERT),
    );
}

fn insert_line(ed: &mut Editor, below: bool) {
    let mode = ed.mode;
    let count = if mode.ty == ModeType::Insert {
        1
    } else {
        mode.count()
    };
    let main = ed.sel.main();
    let line = ed.doc.line_at(if below { main.to() } else { main.from() });
    let at = if below { line.to } else { line.from };
    let m = Mode::reset_for(mode.ty);
    ed.dispatch(
        Tr::changes(ChangeSet::new(vec![Change::insert(at, "\n".repeat(count))])).with_mode(m),
    );
}

fn undo(ed: &mut Editor) {
    let mode = ed.mode;
    let count = mode.count();
    let Some(target) = ed.history.undo_target(count).cloned() else {
        return;
    };
    let head = Checkpoint {
        doc: ed.doc.clone(),
        selection: ed.sel.clone(),
    };
    let changes = diff(&ed.doc, &target.doc);
    ed.dispatch(
        Tr::changes(changes)
            .with_sel(target.selection)
            .with_mode(mode.without_count())
            .scrolled(),
    );
    ed.history.move_by(count as isize, Some(head));
}

fn redo(ed: &mut Editor) {
    let mode = ed.mode;
    let count = mode.count();
    let Some(target) = ed.history.redo_target(count).cloned() else {
        return;
    };
    let changes = diff(&ed.doc, &target.doc);
    ed.dispatch(
        Tr::changes(changes)
            .with_sel(target.selection)
            .with_mode(mode.without_count())
            .scrolled(),
    );
    ed.history.move_by(-(count as isize), None);
}

fn extend_line(ed: &mut Editor) {
    let mode = ed.mode;
    let doc = &ed.doc;
    let len = doc.len();
    let sel = ed.map_sel(|r| {
        let start = doc.line_at(r.from());
        let mut end = doc.line_at(r.to());
        if !r.is_empty() && r.to() == end.from {
            end = doc.line(end.number - 1);
        }
        let ideal = Range::new(start.from, len.min(end.to + 1));
        let perfect = ideal.from() == r.from() && ideal.to() == r.to();
        if perfect || mode.count.is_some() {
            let n = (end.number + mode.count()).min(doc.lines());
            let next = doc.line(n);
            Range::new(start.from, len.min(next.to + 1))
        } else {
            ideal
        }
    });
    ed.dispatch(Tr::sel(sel).with_mode(mode.without_count()));
}

/// The search query from the `/` register (cm-helix `searchFacet`).
pub(crate) fn search_register_query(ed: &Editor) -> Option<SearchQuery> {
    let src = ed.registers.get('/').map(|v| v.join(","))?;
    if src.is_empty() {
        return None;
    }
    Some(SearchQuery::new(&src))
}

fn show_search_error(ed: &mut Editor, q: &SearchQuery) {
    let msg = q.error().unwrap_or_default().to_string();
    ed.error(format!("Invalid regex /{}/: {msg}", q.source));
}

fn search_next(ed: &mut Editor) {
    let mode = ed.mode;
    let query = search_register_query(ed);
    let Some(q) = query.filter(|q| q.is_valid()) else {
        if let Some(q) = search_register_query(ed) {
            show_search_error(ed, &q);
        }
        ed.dispatch(Tr::mode(mode.without_count()));
        return;
    };
    let from = ed.sel.main().to();
    let mut list = q.matches(&ed.doc, from, ed.doc.len());
    let mut idx = 0;
    let mut found = false;
    let mut wrapped = false;
    let mut m = None;
    for _ in 0..mode.count() {
        let mut next = list.get(idx).copied();
        idx += 1;
        found |= next.is_some();
        if next.is_none() {
            list = q.matches(&ed.doc, 0, ed.doc.len());
            idx = 0;
            wrapped = true;
            next = list.get(idx).copied();
            idx += 1;
            found |= next.is_some();
        }
        if !found {
            ed.error("No more matches");
            return;
        }
        m = next;
    }
    let Some((f, t)) = m else {
        return;
    };
    let r = Range::new(f, t);
    let sel = if mode.ty == ModeType::Select {
        ed.sel.add_range(r, true)
    } else {
        Selection::single(f, t)
    };
    ed.dispatch(Tr::sel(sel).scrolled());
    if wrapped {
        ed.message("Wrapped around document");
    }
}

/// cm-helix's `Ring` (including its `start` off-by-one when full).
struct Ring {
    items: Vec<Option<(usize, usize)>>,
    head: usize,
    length: usize,
    max: usize,
}

impl Ring {
    fn new(max: usize) -> Ring {
        Ring {
            items: vec![None; max],
            head: 0,
            length: 0,
            max,
        }
    }
    fn push(&mut self, item: (usize, usize)) {
        if self.max == 0 {
            return;
        }
        self.items[self.head] = Some(item);
        self.head = (self.head + 1) % self.max;
        self.length = (self.length + 1).min(self.max);
    }
    fn start(&self) -> usize {
        if self.length < self.max {
            0
        } else {
            (self.head + 1) % self.max
        }
    }
    fn first(&self) -> Option<(usize, usize)> {
        if self.max == 0 {
            return None;
        }
        self.items[self.start()]
    }
    fn iter(&self) -> impl Iterator<Item = Option<(usize, usize)>> + '_ {
        (0..self.length).map(move |i| self.items[(self.start() + i) % self.max])
    }
}

fn search_prev(ed: &mut Editor) {
    let mode = ed.mode;
    let Some(q) = search_register_query(ed).filter(|q| q.is_valid()) else {
        if let Some(q) = search_register_query(ed) {
            show_search_error(ed, &q);
        }
        ed.dispatch(Tr::mode(mode.without_count()));
        return;
    };
    let count = mode.count();
    let main_from = ed.sel.main().from();
    let all = q.matches(&ed.doc, 0, ed.doc.len());
    let split = all
        .iter()
        .position(|m| m.1 >= main_from)
        .unwrap_or(all.len());
    let mut before = Ring::new(count);
    for m in &all[..split] {
        before.push(*m);
    }
    let select = |ed: &mut Editor, m: Option<(usize, usize)>| {
        if let Some((f, t)) = m {
            ed.dispatch(Tr::sel(Selection::single(f, t)).centered());
        }
    };
    if before.length == count {
        let m = before.first();
        select(ed, m);
    } else {
        let mut after = Ring::new(count - before.length);
        for m in &all[split..] {
            after.push(*m);
        }
        let total = after.length + before.length;
        if total == 0 {
            ed.error("No more matches");
        } else if total == count {
            let m = after.first();
            select(ed, m);
            ed.message("Wrapped around document");
        } else {
            let items: Vec<Option<(usize, usize)>> = before.iter().chain(after.iter()).collect();
            let rem = count % total;
            let m = items[items.len() - rem - 1];
            select(ed, m);
            ed.message("Wrapped around document");
        }
    }
    let m = ed.mode.without_count();
    ed.dispatch(Tr::mode(m));
}

fn change_number(ed: &mut Editor, increase: bool) {
    let tr = ed.change_by_range(|ed, r| {
        let s = ed.range_text(r);
        if !s.chars().any(|c| c.is_ascii_digit()) {
            return (Vec::new(), r);
        }
        let parsed = js_number(&s);
        let insert = js_number_to_string(parsed + if increase { 1.0 } else { -1.0 });
        let len = char_len(&insert);
        let range = if len == char_len(&s) {
            r
        } else {
            Range::new(r.from(), r.from() + len)
        };
        (vec![Change::new(r.from(), r.to(), insert)], range)
    });
    ed.dispatch(tr);
}

fn change_case(ed: &mut Editor, upper: Option<bool>) {
    let tr = ed.change_by_range(|ed, r| {
        let s = ed.range_text(r);
        let insert = match upper {
            None => s
                .chars()
                .map(|c| {
                    let up: String = c.to_uppercase().collect();
                    if up == c.to_string() {
                        c.to_lowercase().collect()
                    } else {
                        up
                    }
                })
                .collect::<String>(),
            Some(true) => s.to_uppercase(),
            Some(false) => s.to_lowercase(),
        };
        (vec![Change::new(r.from(), r.to(), insert)], r)
    });
    ed.dispatch(tr);
}

fn trim(ed: &mut Editor) {
    let sel = ed.map_sel(|r| {
        let selected = ed.range_text(r);
        let trimmed = js_trim(&selected);
        if trimmed == selected {
            return r;
        }
        let start_off = selected
            .find(trimmed)
            .map(|b| char_len(&selected[..b]))
            .unwrap_or(0);
        let end_off = char_len(&selected) - char_len(trimmed) - start_off;
        let anchor = if r.anchor == r.from() {
            r.anchor + start_off
        } else {
            r.anchor - end_off
        };
        let head = if r.head == r.to() {
            r.head - end_off
        } else {
            r.head + start_off
        };
        Range::new(anchor, head)
    });
    ed.dispatch(Tr::sel(sel));
}

fn join(ed: &mut Editor) {
    let doc = &ed.doc;
    let sel = ed.sel.main();
    let start = doc.line_at(sel.from());
    if start.number >= doc.lines() {
        return;
    }
    let mut end = doc.line_at(sel.to());
    let same_line = end.number == start.number;
    if same_line {
        end = doc.line(start.number + 1);
    }
    let mut content = String::new();
    let mut removed = 0;
    for n in start.number..=end.number {
        let mut line_content = doc.line_text(&start);
        if n > start.number {
            let lt = doc.line_text(&doc.line(n));
            line_content = js_trim_start(&lt).to_string();
            let mut trimmed = char_len(&lt) - char_len(&line_content);
            if !same_line && n == end.number && sel.to() - end.from < trimmed {
                trimmed = sel.to() - end.from;
            }
            removed += trimmed;
        }
        content.push_str(&line_content);
        if n != end.number {
            content.push(' ');
        }
    }
    let new_to = if same_line {
        sel.to()
    } else {
        sel.to() - removed
    };
    let range = if sel.anchor > sel.head {
        Range::new(new_to, sel.from())
    } else {
        Range::new(sel.from(), new_to)
    };
    let (from, to) = (start.from, end.to);
    ed.dispatch(
        Tr::changes(ChangeSet::new(vec![Change::new(from, to, content)]))
            .with_sel(Selection::from_range(range)),
    );
}

fn expand_syntax(ed: &mut Editor) {
    let h = &ed.syntax;
    if h.cursor < h.selections.len() {
        let s = if h.cursor == h.selections.len() - 1 {
            h.head.clone().expect("syntax history head")
        } else {
            h.selections[h.cursor + 1].clone()
        };
        ed.dispatch(Tr {
            selection: Some(s),
            syntax: true,
            ..Tr::default()
        });
        ed.syntax.cursor += 1;
        return;
    }
    // freeze
    ed.dispatch(Tr {
        syntax: true,
        ..Tr::default()
    });
    ed.syntax.frozen = true;
    // selectParentSyntax
    let prev = ed.sel.clone();
    let mut ranges = Vec::new();
    for r in prev.ranges() {
        ranges.push(match ed.host.syntax_parent(&ed.doc, r.from(), r.to()) {
            Some((f, t)) => Range::new(t, f),
            None => *r,
        });
    }
    let next = Selection::create(ranges, prev.main_index());
    if next.same(&prev) {
        // cm-helix never unfreezes here (selectParentSyntax does not dispatch)
        return;
    }
    ed.dispatch(Tr {
        selection: Some(next.clone()),
        syntax: true,
        scroll: Some(ScrollAlign::Nearest),
        ..Tr::default()
    });
    let h = &mut ed.syntax;
    h.selections.push(prev);
    h.cursor += 1;
    h.head = Some(next);
    h.frozen = false;
}

fn sibling(ed: &mut Editor, forward: bool) {
    let main = ed.sel.main();
    let s = match ed
        .host
        .syntax_sibling(&ed.doc, main.from(), main.to(), forward)
    {
        Some((f, t)) => Selection::single(f, t),
        None => Selection::single(0, ed.doc.len()),
    };
    ed.dispatch(Tr::sel(s).scrolled());
}
