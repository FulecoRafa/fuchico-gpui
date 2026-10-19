//! The cm-vim command table (`defaultKeymap`) and key matching
//! (`commandMatches` / `commandMatch` / `lastChar`).

use crate::state::Args;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Context {
    Normal,
    Visual,
    Insert,
    OperatorPending,
}

impl Context {
    pub fn name(self) -> &'static str {
        match self {
            Context::Normal => "normal",
            Context::Visual => "visual",
            Context::Insert => "insert",
            Context::OperatorPending => "operatorPending",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CmdType {
    KeyToKey,
    Motion,
    Operator,
    OperatorMotion,
    Action,
    Search,
    Ex,
    KeyToEx,
    Idle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MotionKind {
    MoveToTopLine,
    MoveToMiddleLine,
    MoveToBottomLine,
    ExpandToLine,
    FindNext,
    FindAndSelectNextInclusive,
    GoToMark,
    MoveToOtherHighlightedEnd,
    JumpToMark,
    MoveByCharacters,
    MoveByLines,
    MoveByDisplayLines,
    MoveByPage,
    MoveByParagraph,
    MoveBySentence,
    MoveByScroll,
    MoveByWords,
    MoveTillCharacter,
    MoveToCharacter,
    MoveToSymbol,
    MoveToColumn,
    MoveToEol,
    MoveToFirstNonWhiteSpaceCharacter,
    MoveToMatchedSymbol,
    MoveToStartOfLine,
    MoveToLineOrEdgeOfDocument,
    MoveToStartOfDisplayLine,
    MoveToEndOfDisplayLine,
    TextObjectManipulation,
    RepeatLastCharacterSearch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OperatorKind {
    Change,
    Delete,
    Indent,
    IndentAuto,
    HardWrap,
    ChangeCase,
    Yank,
    Rot13,
}

impl OperatorKind {
    pub fn name(self) -> &'static str {
        match self {
            OperatorKind::Change => "change",
            OperatorKind::Delete => "delete",
            OperatorKind::Indent => "indent",
            OperatorKind::IndentAuto => "indentAuto",
            OperatorKind::HardWrap => "hardWrap",
            OperatorKind::ChangeCase => "changeCase",
            OperatorKind::Yank => "yank",
            OperatorKind::Rot13 => "rot13",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ActionKind {
    JumpListWalk,
    Scroll,
    ScrollToCursor,
    ReplayMacro,
    EnterMacroRecordMode,
    ToggleOverwrite,
    EnterInsertMode,
    ToggleVisualMode,
    ReselectLastSelection,
    JoinLines,
    NewLineAndEnterInsertMode,
    Paste,
    Undo,
    Redo,
    SetRegister,
    InsertRegister,
    OneNormalCommand,
    SetMark,
    Replace,
    IncrementNumberToken,
    RepeatLastEdit,
    Indent,
    ExitInsertMode,
}

/// One keymap entry (a cm-vim `vimKey` object).
#[derive(Debug, Clone, PartialEq)]
pub struct Command {
    pub keys: String,
    pub ty: CmdType,
    pub context: Option<Context>,
    pub motion: Option<MotionKind>,
    pub motion_args: Option<Args>,
    pub operator: Option<OperatorKind>,
    pub operator_args: Option<Args>,
    pub operator_motion_args: Option<Args>,
    pub action: Option<ActionKind>,
    pub action_args: Option<Args>,
    pub search_args: Option<Args>,
    pub to_keys: Option<String>,
    /// `noremap` (`None` = undefined).
    pub noremap: Option<bool>,
    pub ex_input: Option<String>,
    pub is_edit: bool,
    pub interlace_insert_repeat: bool,
    pub exit_visual_block: bool,
    pub repeat_override: Option<i64>,
    /// Added by `:map` and friends.
    pub user: bool,
}

impl Command {
    pub fn new(keys: &str, ty: CmdType) -> Command {
        Command {
            keys: keys.to_string(),
            ty,
            context: None,
            motion: None,
            motion_args: None,
            operator: None,
            operator_args: None,
            operator_motion_args: None,
            action: None,
            action_args: None,
            search_args: None,
            to_keys: None,
            noremap: None,
            ex_input: None,
            is_edit: false,
            interlace_insert_repeat: false,
            exit_visual_block: false,
            repeat_override: None,
            user: false,
        }
    }
    fn ctx(mut self, c: Context) -> Command {
        self.context = Some(c);
        self
    }
    fn edit(mut self) -> Command {
        self.is_edit = true;
        self
    }
}

fn d() -> Args {
    Args::default()
}

fn k2k(keys: &str, to: &str) -> Command {
    let mut c = Command::new(keys, CmdType::KeyToKey);
    c.to_keys = Some(to.to_string());
    c
}

fn mo(keys: &str, m: MotionKind, args: Option<Args>) -> Command {
    let mut c = Command::new(keys, CmdType::Motion);
    c.motion = Some(m);
    c.motion_args = args;
    c
}

fn op(keys: &str, o: OperatorKind, args: Option<Args>) -> Command {
    let mut c = Command::new(keys, CmdType::Operator);
    c.operator = Some(o);
    c.operator_args = args;
    c
}

fn opm(keys: &str, o: OperatorKind, m: MotionKind, margs: Option<Args>) -> Command {
    let mut c = Command::new(keys, CmdType::OperatorMotion);
    c.operator = Some(o);
    c.motion = Some(m);
    c.motion_args = margs;
    c
}

fn act(keys: &str, a: ActionKind, args: Option<Args>) -> Command {
    let mut c = Command::new(keys, CmdType::Action);
    c.action = Some(a);
    c.action_args = args;
    c
}

fn search(keys: &str, args: Args) -> Command {
    let mut c = Command::new(keys, CmdType::Search);
    c.search_args = Some(args);
    c
}

/// cm-vim 6.3.0 `defaultKeymap`, in order.
pub fn default_keymap() -> Vec<Command> {
    use ActionKind as A;
    use Context::*;
    use MotionKind as M;
    use OperatorKind as O;
    let fwd = || Args { forward: true, ..d() };
    let back = d;
    vec![
        k2k("<Left>", "h"),
        k2k("<Right>", "l"),
        k2k("<Up>", "k"),
        k2k("<Down>", "j"),
        k2k("g<Up>", "gk"),
        k2k("g<Down>", "gj"),
        k2k("<Space>", "l"),
        k2k("<BS>", "h"),
        k2k("<Del>", "x"),
        k2k("<C-Space>", "W"),
        k2k("<C-BS>", "B"),
        k2k("<S-Space>", "w"),
        k2k("<S-BS>", "b"),
        k2k("<C-n>", "j"),
        k2k("<C-p>", "k"),
        k2k("<C-[>", "<Esc>"),
        k2k("<C-c>", "<Esc>"),
        k2k("<C-[>", "<Esc>").ctx(Insert),
        k2k("<C-c>", "<Esc>").ctx(Insert),
        k2k("<C-Esc>", "<Esc>"),
        k2k("<C-Esc>", "<Esc>").ctx(Insert),
        k2k("s", "cl").ctx(Normal),
        k2k("s", "c").ctx(Visual),
        k2k("S", "cc").ctx(Normal),
        k2k("S", "VdO").ctx(Visual),
        k2k("<Home>", "0"),
        k2k("<End>", "$"),
        k2k("<PageUp>", "<C-b>"),
        k2k("<PageDown>", "<C-f>"),
        k2k("<CR>", "j^").ctx(Normal),
        k2k("<Ins>", "i").ctx(Normal),
        act("<Ins>", A::ToggleOverwrite, None).ctx(Insert),
        mo("H", M::MoveToTopLine, Some(Args { linewise: true, to_jumplist: true, ..d() })),
        mo("M", M::MoveToMiddleLine, Some(Args { linewise: true, to_jumplist: true, ..d() })),
        mo("L", M::MoveToBottomLine, Some(Args { linewise: true, to_jumplist: true, ..d() })),
        mo("h", M::MoveByCharacters, Some(back())),
        mo("l", M::MoveByCharacters, Some(fwd())),
        mo("j", M::MoveByLines, Some(Args { forward: true, linewise: true, ..d() })),
        mo("k", M::MoveByLines, Some(Args { linewise: true, ..d() })),
        mo("gj", M::MoveByDisplayLines, Some(fwd())),
        mo("gk", M::MoveByDisplayLines, Some(back())),
        mo("w", M::MoveByWords, Some(fwd())),
        mo("W", M::MoveByWords, Some(Args { forward: true, big_word: true, ..d() })),
        mo("e", M::MoveByWords, Some(Args { forward: true, word_end: true, inclusive: true, ..d() })),
        mo(
            "E",
            M::MoveByWords,
            Some(Args { forward: true, word_end: true, big_word: true, inclusive: true, ..d() }),
        ),
        mo("b", M::MoveByWords, Some(back())),
        mo("B", M::MoveByWords, Some(Args { big_word: true, ..d() })),
        mo("ge", M::MoveByWords, Some(Args { word_end: true, inclusive: true, ..d() })),
        mo(
            "gE",
            M::MoveByWords,
            Some(Args { word_end: true, big_word: true, inclusive: true, ..d() }),
        ),
        mo("{", M::MoveByParagraph, Some(Args { to_jumplist: true, ..d() })),
        mo("}", M::MoveByParagraph, Some(Args { forward: true, to_jumplist: true, ..d() })),
        mo("(", M::MoveBySentence, Some(back())),
        mo(")", M::MoveBySentence, Some(fwd())),
        mo("<C-f>", M::MoveByPage, Some(fwd())),
        mo("<C-b>", M::MoveByPage, Some(back())),
        mo("<C-d>", M::MoveByScroll, Some(Args { forward: true, explicit_repeat: true, ..d() })),
        mo("<C-u>", M::MoveByScroll, Some(Args { explicit_repeat: true, ..d() })),
        mo(
            "gg",
            M::MoveToLineOrEdgeOfDocument,
            Some(Args { explicit_repeat: true, linewise: true, to_jumplist: true, ..d() }),
        ),
        mo(
            "G",
            M::MoveToLineOrEdgeOfDocument,
            Some(Args {
                forward: true,
                explicit_repeat: true,
                linewise: true,
                to_jumplist: true,
                ..d()
            }),
        ),
        mo("g$", M::MoveToEndOfDisplayLine, None),
        mo("g^", M::MoveToStartOfDisplayLine, None),
        mo("g0", M::MoveToStartOfDisplayLine, None),
        mo("0", M::MoveToStartOfLine, None),
        mo("^", M::MoveToFirstNonWhiteSpaceCharacter, None),
        mo("+", M::MoveByLines, Some(Args { forward: true, to_first_char: true, ..d() })),
        mo("-", M::MoveByLines, Some(Args { to_first_char: true, ..d() })),
        mo(
            "_",
            M::MoveByLines,
            Some(Args { forward: true, to_first_char: true, repeat_offset: -1, ..d() }),
        ),
        mo("$", M::MoveToEol, Some(Args { inclusive: true, ..d() })),
        mo("%", M::MoveToMatchedSymbol, Some(Args { inclusive: true, to_jumplist: true, ..d() })),
        mo("f<character>", M::MoveToCharacter, Some(Args { forward: true, inclusive: true, ..d() })),
        mo("F<character>", M::MoveToCharacter, Some(back())),
        mo("t<character>", M::MoveTillCharacter, Some(Args { forward: true, inclusive: true, ..d() })),
        mo("T<character>", M::MoveTillCharacter, Some(back())),
        mo(";", M::RepeatLastCharacterSearch, Some(fwd())),
        mo(",", M::RepeatLastCharacterSearch, Some(back())),
        mo("'<register>", M::GoToMark, Some(Args { to_jumplist: true, linewise: true, ..d() })),
        mo("`<register>", M::GoToMark, Some(Args { to_jumplist: true, ..d() })),
        mo("]`", M::JumpToMark, Some(fwd())),
        mo("[`", M::JumpToMark, Some(back())),
        mo("]'", M::JumpToMark, Some(Args { forward: true, linewise: true, ..d() })),
        mo("['", M::JumpToMark, Some(Args { linewise: true, ..d() })),
        act(
            "]p",
            A::Paste,
            Some(Args { after: true, is_edit: true, match_indent: true, ..d() }),
        )
        .edit(),
        act("[p", A::Paste, Some(Args { is_edit: true, match_indent: true, ..d() })).edit(),
        mo("]<character>", M::MoveToSymbol, Some(Args { forward: true, to_jumplist: true, ..d() })),
        mo("[<character>", M::MoveToSymbol, Some(Args { to_jumplist: true, ..d() })),
        mo("|", M::MoveToColumn, None),
        mo("o", M::MoveToOtherHighlightedEnd, None).ctx(Visual),
        mo("O", M::MoveToOtherHighlightedEnd, Some(Args { same_line: true, ..d() })).ctx(Visual),
        op("d", O::Delete, None),
        op("y", O::Yank, None),
        op("c", O::Change, None),
        op("=", O::IndentAuto, None),
        op(">", O::Indent, Some(Args { indent_right: true, ..d() })),
        op("<", O::Indent, Some(Args { indent_right: false, ..d() })),
        op("g~", O::ChangeCase, None),
        op("gu", O::ChangeCase, Some(Args { to_lower: Some(true), ..d() })).edit(),
        op("gU", O::ChangeCase, Some(Args { to_lower: Some(false), ..d() })).edit(),
        mo("n", M::FindNext, Some(Args { forward: true, to_jumplist: true, ..d() })),
        mo("N", M::FindNext, Some(Args { to_jumplist: true, ..d() })),
        mo("gn", M::FindAndSelectNextInclusive, Some(fwd())),
        mo("gN", M::FindAndSelectNextInclusive, Some(back())),
        op("gq", O::HardWrap, None),
        op("gw", O::HardWrap, Some(Args { keep_cursor: true, ..d() })),
        op("g?", O::Rot13, None),
        {
            let mut c = opm("x", O::Delete, M::MoveByCharacters, Some(fwd()));
            c.operator_motion_args = Some(Args { visual_line: false, ..d() });
            c
        },
        {
            let mut c = opm("X", O::Delete, M::MoveByCharacters, Some(back()));
            c.operator_motion_args = Some(Args { visual_line: true, ..d() });
            c
        },
        opm("D", O::Delete, M::MoveToEol, Some(Args { inclusive: true, ..d() })).ctx(Normal),
        op("D", O::Delete, Some(Args { linewise: true, ..d() })).ctx(Visual),
        opm("Y", O::Yank, M::ExpandToLine, Some(Args { linewise: true, ..d() })).ctx(Normal),
        op("Y", O::Yank, Some(Args { linewise: true, ..d() })).ctx(Visual),
        opm("C", O::Change, M::MoveToEol, Some(Args { inclusive: true, ..d() })).ctx(Normal),
        op("C", O::Change, Some(Args { linewise: true, ..d() })).ctx(Visual),
        {
            let mut c = opm("~", O::ChangeCase, M::MoveByCharacters, Some(fwd()));
            c.operator_args = Some(Args { should_move_cursor: true, ..d() });
            c.ctx(Normal)
        },
        op("~", O::ChangeCase, None).ctx(Visual),
        opm("<C-u>", O::Delete, M::MoveToStartOfLine, None).ctx(Insert),
        opm("<C-w>", O::Delete, M::MoveByWords, Some(back())).ctx(Insert),
        Command::new("<C-w>", CmdType::Idle).ctx(Normal),
        act("<C-i>", A::JumpListWalk, Some(fwd())),
        act("<C-o>", A::JumpListWalk, Some(back())),
        act("<C-e>", A::Scroll, Some(Args { forward: true, linewise: true, ..d() })),
        act("<C-y>", A::Scroll, Some(Args { linewise: true, ..d() })),
        act("a", A::EnterInsertMode, Some(Args { insert_at: Some("charAfter"), ..d() }))
            .edit()
            .ctx(Normal),
        act("A", A::EnterInsertMode, Some(Args { insert_at: Some("eol"), ..d() }))
            .edit()
            .ctx(Normal),
        act("A", A::EnterInsertMode, Some(Args { insert_at: Some("endOfSelectedArea"), ..d() }))
            .edit()
            .ctx(Visual),
        act("i", A::EnterInsertMode, Some(Args { insert_at: Some("inplace"), ..d() }))
            .edit()
            .ctx(Normal),
        act("gi", A::EnterInsertMode, Some(Args { insert_at: Some("lastEdit"), ..d() }))
            .edit()
            .ctx(Normal),
        act("I", A::EnterInsertMode, Some(Args { insert_at: Some("firstNonBlank"), ..d() }))
            .edit()
            .ctx(Normal),
        act("gI", A::EnterInsertMode, Some(Args { insert_at: Some("bol"), ..d() }))
            .edit()
            .ctx(Normal),
        act(
            "I",
            A::EnterInsertMode,
            Some(Args { insert_at: Some("startOfSelectedArea"), ..d() }),
        )
        .edit()
        .ctx(Visual),
        {
            let mut c = act("o", A::NewLineAndEnterInsertMode, Some(Args { after: true, ..d() }))
                .edit()
                .ctx(Normal);
            c.interlace_insert_repeat = true;
            c
        },
        {
            let mut c = act("O", A::NewLineAndEnterInsertMode, Some(Args { after: false, ..d() }))
                .edit()
                .ctx(Normal);
            c.interlace_insert_repeat = true;
            c
        },
        act("v", A::ToggleVisualMode, None),
        act("V", A::ToggleVisualMode, Some(Args { linewise: true, ..d() })),
        act("<C-v>", A::ToggleVisualMode, Some(Args { blockwise: true, ..d() })),
        act("<C-q>", A::ToggleVisualMode, Some(Args { blockwise: true, ..d() })),
        act("gv", A::ReselectLastSelection, None),
        act("J", A::JoinLines, None).edit(),
        act("gJ", A::JoinLines, Some(Args { keep_spaces: true, ..d() })).edit(),
        act("p", A::Paste, Some(Args { after: true, is_edit: true, ..d() })).edit(),
        act("P", A::Paste, Some(Args { after: false, is_edit: true, ..d() })).edit(),
        act("r<character>", A::Replace, None).edit(),
        act("@<register>", A::ReplayMacro, None),
        act("q<register>", A::EnterMacroRecordMode, None),
        act("R", A::EnterInsertMode, Some(Args { replace: true, ..d() })).edit().ctx(Normal),
        {
            let mut c = op("R", O::Change, Some(Args { linewise: true, full_line: true, ..d() }))
                .ctx(Visual);
            c.exit_visual_block = true;
            c
        },
        act("u", A::Undo, None).ctx(Normal),
        op("u", O::ChangeCase, Some(Args { to_lower: Some(true), ..d() })).ctx(Visual).edit(),
        op("U", O::ChangeCase, Some(Args { to_lower: Some(false), ..d() })).ctx(Visual).edit(),
        act("<C-r>", A::Redo, None),
        act("m<register>", A::SetMark, None),
        act("\"<register>", A::SetRegister, None),
        act("<C-r><register>", A::InsertRegister, None).ctx(Insert).edit(),
        act("<C-o>", A::OneNormalCommand, None).ctx(Insert),
        act("zz", A::ScrollToCursor, Some(Args { position: Some("center"), ..d() })),
        {
            let mut c = act("z.", A::ScrollToCursor, Some(Args { position: Some("center"), ..d() }));
            c.motion = Some(M::MoveToFirstNonWhiteSpaceCharacter);
            c
        },
        act("zt", A::ScrollToCursor, Some(Args { position: Some("top"), ..d() })),
        {
            let mut c = act("z<CR>", A::ScrollToCursor, Some(Args { position: Some("top"), ..d() }));
            c.motion = Some(M::MoveToFirstNonWhiteSpaceCharacter);
            c
        },
        act("zb", A::ScrollToCursor, Some(Args { position: Some("bottom"), ..d() })),
        {
            let mut c = act("z-", A::ScrollToCursor, Some(Args { position: Some("bottom"), ..d() }));
            c.motion = Some(M::MoveToFirstNonWhiteSpaceCharacter);
            c
        },
        act(".", A::RepeatLastEdit, None),
        act("<C-a>", A::IncrementNumberToken, Some(Args { increase: true, ..d() })).edit(),
        act("<C-x>", A::IncrementNumberToken, Some(Args { increase: false, ..d() })).edit(),
        act("<C-t>", A::Indent, Some(Args { indent_right: true, ..d() })).ctx(Insert),
        act("<C-d>", A::Indent, Some(Args { indent_right: false, ..d() })).ctx(Insert),
        mo("a<register>", M::TextObjectManipulation, None),
        mo("i<register>", M::TextObjectManipulation, Some(Args { text_object_inner: true, ..d() })),
        search("/", Args { forward: true, query_src: Some("prompt"), to_jumplist: true, ..d() }),
        search("?", Args { query_src: Some("prompt"), to_jumplist: true, ..d() }),
        search(
            "*",
            Args {
                forward: true,
                query_src: Some("wordUnderCursor"),
                whole_word_only: true,
                to_jumplist: true,
                ..d()
            },
        ),
        search(
            "#",
            Args {
                query_src: Some("wordUnderCursor"),
                whole_word_only: true,
                to_jumplist: true,
                ..d()
            },
        ),
        search(
            "g*",
            Args { forward: true, query_src: Some("wordUnderCursor"), to_jumplist: true, ..d() },
        ),
        search("g#", Args { query_src: Some("wordUnderCursor"), to_jumplist: true, ..d() }),
        Command::new(":", CmdType::Ex),
    ]
}

fn clen(s: &str) -> usize {
    s.chars().count()
}

fn cslice(s: &str, n: usize) -> &str {
    match s.char_indices().nth(n) {
        Some((i, _)) => &s[..i],
        None => s,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Match {
    Partial,
    Full,
}

/// `commandMatch(pressed, mapped)`.
pub fn command_match(pressed: &str, mapped: &str) -> Option<Match> {
    let is_char = mapped.ends_with("<character>");
    let is_reg = mapped.ends_with("<register>");
    if is_char || is_reg {
        let prefix_len = clen(mapped) - if is_char { 11 } else { 10 };
        let pressed_prefix = cslice(pressed, prefix_len);
        let mapped_prefix = cslice(mapped, prefix_len);
        if pressed_prefix == mapped_prefix && clen(pressed) > prefix_len {
            Some(Match::Full)
        } else if mapped_prefix.starts_with(pressed_prefix) {
            Some(Match::Partial)
        } else {
            None
        }
    } else if pressed == mapped {
        Some(Match::Full)
    } else if mapped.starts_with(pressed) {
        Some(Match::Partial)
    } else {
        None
    }
}

/// `lastChar(keys)`.
pub fn last_char(keys: &str) -> String {
    // /^.*(<[^>]+>)$/
    let sel = if keys.ends_with('>') {
        let body = keys.strip_suffix('>').unwrap_or(keys);
        match body.rfind('<') {
            Some(i) if !keys[i + 1..keys.len() - 1].contains('>') && keys.len() - i > 2 => {
                keys[i..].to_string()
            }
            _ => keys.chars().last().map(String::from).unwrap_or_default(),
        }
    } else {
        keys.chars().last().map(String::from).unwrap_or_default()
    };
    if clen(&sel) > 1 {
        match sel.as_str() {
            "<CR>" | "<S-CR>" => "\n".into(),
            "<Space>" | "<S-Space>" => " ".into(),
            _ => String::new(),
        }
    } else {
        sel
    }
}

/// Split a key string like `"d3w<C-r>a"` into vim keys
/// (`/<(?:[CSMA]-)*\w+>|./gi`).
pub fn split_keys(keys: &str) -> Vec<String> {
    let chars: Vec<char> = keys.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '<' {
            // (?:[CSMA]-)*
            let mut j = i + 1;
            while j + 1 < chars.len()
                && matches!(chars[j].to_ascii_uppercase(), 'C' | 'S' | 'M' | 'A')
                && chars[j + 1] == '-'
            {
                j += 2;
            }
            let start = j;
            while j < chars.len() && (chars[j].is_alphanumeric() || chars[j] == '_') {
                j += 1;
            }
            if j > start && j < chars.len() && chars[j] == '>' {
                out.push(chars[i..=j].iter().collect());
                i = j + 1;
                continue;
            }
            // backtrack: modifiers might have eaten a word char ("<C-a>" ok,
            // "<S>" -> word "S")
            let mut j = i + 1;
            while j < chars.len() && (chars[j].is_alphanumeric() || chars[j] == '_') {
                j += 1;
            }
            if j > i + 1 && j < chars.len() && chars[j] == '>' {
                out.push(chars[i..=j].iter().collect());
                i = j + 1;
                continue;
            }
        }
        out.push(chars[i].to_string());
        i += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matching() {
        assert_eq!(command_match("f", "f<character>"), Some(Match::Partial));
        assert_eq!(command_match("fa", "f<character>"), Some(Match::Full));
        assert_eq!(command_match("g", "gg"), Some(Match::Partial));
        assert_eq!(command_match("gg", "gg"), Some(Match::Full));
        assert_eq!(command_match("x", "gg"), None);
        assert_eq!(last_char("f<Space>"), " ");
        assert_eq!(last_char("fa"), "a");
        assert_eq!(last_char("f<C-a>"), "");
        assert_eq!(split_keys("d3w<C-r>a<Esc><"), vec!["d", "3", "w", "<C-r>", "a", "<Esc>", "<"]);
        assert_eq!(split_keys("<lt>"), vec!["<lt>"]);
    }
}
