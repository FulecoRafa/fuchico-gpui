//! Keybinding tables (cm-helix `helixCommandBindings`).
//!
//! Every binding names the table it lives in (the mode / pending "minor"
//! keymap), the key in CodeMirror notation, the command it runs, whether it
//! records an undo checkpoint, and whether it is a faithful port (`Parity`),
//! a deliberately reproduced cm-helix oddity (`Quirk`) or something cm-helix
//! does not have (`Extension`, only active with
//! [`crate::Config::extensions`]).

use std::sync::OnceLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KeymapTable {
    Insert,
    Normal,
    Goto,
    Match,
    Space,
    LeftBracket,
    RightBracket,
    /// `z` (extension).
    View,
}

impl KeymapTable {
    pub fn label(&self) -> &'static str {
        match self {
            KeymapTable::Insert => "insert",
            KeymapTable::Normal => "normal/select",
            KeymapTable::Goto => "goto (g)",
            KeymapTable::Match => "match (m)",
            KeymapTable::Space => "space",
            KeymapTable::LeftBracket => "left bracket ([)",
            KeymapTable::RightBracket => "right bracket (])",
            KeymapTable::View => "view (z)",
        }
    }
}

/// cm-helix `{checkpoint: true | "temp"}`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckpointKind {
    None,
    /// Adds an undo checkpoint (state before the command).
    Add,
    /// Starts an insert session: the checkpoint is kept pending and committed
    /// on `Escape` if the document changed.
    Temp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BindingStatus {
    Parity,
    Quirk,
    Extension,
}

/// Internal command identifiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Cmd {
    // insert
    InsBackspace,
    InsDelete,
    InsEnter,
    InsLeft,
    InsRight,
    InsUp,
    InsDown,
    InsHome,
    InsEnd,
    InsEscape,
    InsRegister,
    InsIndent,
    InsDedent,
    InsDeleteGroupBackward,
    InsDeleteGroupForward,
    InsDeleteLineStart,
    InsDeleteLineEnd,
    // normal
    Escape,
    Search,
    SelectRegex,
    SplitLines,
    KeepPrimary,
    RotatePrev,
    RotateNext,
    Count(u8),
    CommandLine,
    Yank,
    Append,
    AppendLineEnd,
    InsertLineStart,
    CopyDown,
    CopyUp,
    Change,
    Delete,
    ChangeNoYank,
    DeleteNoYank,
    PasteBefore,
    PasteAfter,
    ReplaceWithYank,
    ReplaceChar,
    WordNext,
    WordPrev,
    LongWordNext,
    LongWordPrev,
    ToggleSelect,
    GotoMinor,
    SpaceMinor,
    MatchMinor,
    ViewMinor,
    LeftBracketMinor,
    RightBracketMinor,
    Insert,
    Left,
    Down,
    Up,
    Right,
    SelectAll,
    OpenBelow,
    OpenAbove,
    Find { inclusive: bool, forward: bool },
    Undo,
    Redo,
    ExtendLine,
    SearchNext,
    SearchPrev,
    Increment,
    Decrement,
    HalfPageDown,
    HalfPageUp,
    Collapse,
    Flip,
    EnsureForward,
    ExpandSyntax,
    ShrinkSyntax,
    SiblingNext,
    SiblingPrev,
    ToggleComment,
    Indent,
    Dedent,
    Lower,
    Upper,
    SwitchCase,
    StarSearch,
    Trim,
    LineStart,
    LineEnd,
    Join,
    SelectRegister,
    Nop,
    SplitRegex,
    KeepRegex,
    RemoveRegex,
    // goto
    DocStart,
    LastLine,
    GotoDown,
    GotoUp,
    FirstNonBlank,
    BufferNext,
    BufferPrevious,
    WindowTop,
    WindowCenter,
    WindowBottom,
    // match
    SurroundAdd,
    MatchBracket,
    SelectAround,
    SelectInside,
    SurroundDelete,
    SurroundReplace,
    // space
    ClipboardYank,
    ClipboardPasteAfter,
    ClipboardPasteBefore,
    ClipboardReplace,
    FilePicker,
    BufferPicker,
    GlobalSearch,
    SpaceComment,
    // brackets
    InsertLineAbove,
    InsertLineBelow,
    // view
    AlignCenter,
    AlignTop,
    AlignBottom,
    ScrollDown,
    ScrollUp,
}

#[derive(Debug, Clone)]
pub struct Binding {
    pub table: KeymapTable,
    /// CodeMirror key name (`"Alt-s"`, `"Space"`, `"g"`).
    pub key: &'static str,
    /// Command name (cm-helix / Helix vocabulary).
    pub command: &'static str,
    pub checkpoint: CheckpointKind,
    pub status: BindingStatus,
    pub description: &'static str,
    pub(crate) cmd: Cmd,
}

/// Normal-table aliases (cm-helix string bindings).
pub(crate) fn normal_alias(key: &str) -> Option<&'static str> {
    Some(match key {
        "ArrowDown" => "j",
        "ArrowUp" => "k",
        "ArrowRight" => "l",
        "ArrowLeft" => "h",
        "PageDown" => "Ctrl-d",
        "PageUp" => "Ctrl-u",
        "Alt-o" => "Alt-ArrowUp",
        "Alt-i" => "Alt-ArrowDown",
        "Alt-n" => "Alt-ArrowRight",
        "Alt-p" => "Alt-ArrowLeft",
        _ => return None,
    })
}

/// Every binding, in table order.
pub fn bindings() -> &'static [Binding] {
    static B: OnceLock<Vec<Binding>> = OnceLock::new();
    B.get_or_init(build)
}

/// Look a key up in a table (aliases resolved, extensions filtered).
pub(crate) fn lookup(table: KeymapTable, key: &str, extensions: bool) -> Option<&'static Binding> {
    let key = if table == KeymapTable::Normal {
        normal_alias(key).unwrap_or(key)
    } else {
        key
    };
    bindings().iter().find(|b| {
        b.table == table
            && b.key == key
            && (extensions || b.status != BindingStatus::Extension)
            && !b.description.starts_with("alias")
    })
}

fn build() -> Vec<Binding> {
    use BindingStatus::*;
    use CheckpointKind as C;
    use KeymapTable::*;
    let mut v = Vec::new();
    let mut add = |table, key, command, cmd, checkpoint, status, description| {
        v.push(Binding {
            table,
            key,
            command,
            checkpoint,
            status,
            description,
            cmd,
        })
    };

    // ---- insert -------------------------------------------------------
    add(
        Insert,
        "Backspace",
        "delete_char_backward",
        Cmd::InsBackspace,
        C::None,
        Parity,
        "Delete previous char (by indent unit in leading whitespace)",
    );
    add(
        Insert,
        "Delete",
        "delete_char_forward",
        Cmd::InsDelete,
        C::None,
        Parity,
        "Delete next char",
    );
    add(
        Insert,
        "Enter",
        "insert_newline_and_indent",
        Cmd::InsEnter,
        C::None,
        Parity,
        "Newline keeping indentation (continues lists/quotes with continue_markup)",
    );
    add(
        Insert,
        "ArrowLeft",
        "cursor_char_left",
        Cmd::InsLeft,
        C::None,
        Parity,
        "Move left",
    );
    add(
        Insert,
        "ArrowRight",
        "cursor_char_right",
        Cmd::InsRight,
        C::None,
        Parity,
        "Move right",
    );
    add(
        Insert,
        "Escape",
        "normal_mode",
        Cmd::InsEscape,
        C::None,
        Parity,
        "Back to normal mode (commits the insert session to undo history)",
    );
    add(
        Insert,
        "Ctrl-r",
        "insert_register",
        Cmd::InsRegister,
        C::None,
        Parity,
        "Insert the contents of a register",
    );
    add(
        Insert,
        "Tab",
        "indent_more",
        Cmd::InsIndent,
        C::None,
        Parity,
        "Indent the selected lines",
    );
    add(
        Insert,
        "Shift-Tab",
        "indent_less",
        Cmd::InsDedent,
        C::None,
        Parity,
        "Dedent the selected lines",
    );
    // CodeMirror default keymap fallthroughs used in insert mode.
    add(
        Insert,
        "ArrowUp",
        "cursor_line_up",
        Cmd::InsUp,
        C::None,
        Parity,
        "Move up (CodeMirror default keymap)",
    );
    add(
        Insert,
        "ArrowDown",
        "cursor_line_down",
        Cmd::InsDown,
        C::None,
        Parity,
        "Move down (CodeMirror default keymap)",
    );
    add(
        Insert,
        "Home",
        "cursor_line_boundary_backward",
        Cmd::InsHome,
        C::None,
        Parity,
        "Smart home (CodeMirror default keymap)",
    );
    add(
        Insert,
        "End",
        "cursor_line_boundary_forward",
        Cmd::InsEnd,
        C::None,
        Parity,
        "Line end (CodeMirror default keymap)",
    );
    add(
        Insert,
        "Alt-Backspace",
        "delete_group_backward",
        Cmd::InsDeleteGroupBackward,
        C::None,
        Parity,
        "Delete word backward (CodeMirror default keymap, macOS)",
    );
    add(
        Insert,
        "Ctrl-Backspace",
        "delete_group_backward",
        Cmd::InsDeleteGroupBackward,
        C::None,
        Parity,
        "Delete word backward (CodeMirror default keymap)",
    );
    add(
        Insert,
        "Alt-Delete",
        "delete_group_forward",
        Cmd::InsDeleteGroupForward,
        C::None,
        Parity,
        "Delete word forward (CodeMirror default keymap, macOS)",
    );
    add(
        Insert,
        "Ctrl-Delete",
        "delete_group_forward",
        Cmd::InsDeleteGroupForward,
        C::None,
        Parity,
        "Delete word forward (CodeMirror default keymap)",
    );
    add(
        Insert,
        "Meta-Backspace",
        "delete_line_boundary_backward",
        Cmd::InsDeleteLineStart,
        C::None,
        Parity,
        "Delete to line start (CodeMirror default keymap, macOS)",
    );
    add(
        Insert,
        "Meta-Delete",
        "delete_line_boundary_forward",
        Cmd::InsDeleteLineEnd,
        C::None,
        Parity,
        "Delete to line end (CodeMirror default keymap, macOS)",
    );
    add(
        Insert,
        "Ctrl-w",
        "delete_word_backward",
        Cmd::InsDeleteGroupBackward,
        C::None,
        Extension,
        "Delete word backward (Helix)",
    );
    add(
        Insert,
        "Ctrl-u",
        "kill_to_line_start",
        Cmd::InsDeleteLineStart,
        C::None,
        Extension,
        "Delete to line start (Helix)",
    );
    add(
        Insert,
        "Ctrl-k",
        "kill_to_line_end",
        Cmd::InsDeleteLineEnd,
        C::None,
        Extension,
        "Delete to line end (Helix)",
    );

    // ---- normal / select ------------------------------------------------
    add(
        Normal,
        "Escape",
        "normal_mode",
        Cmd::Escape,
        C::None,
        Quirk,
        "Reset to normal mode from any pending state (from select mode too; never steps back to select)",
    );
    add(
        Normal,
        "/",
        "search",
        Cmd::Search,
        C::None,
        Parity,
        "Search forward (prompt)",
    );
    add(
        Normal,
        "s",
        "select_regex",
        Cmd::SelectRegex,
        C::None,
        Parity,
        "Select regex matches inside the selections (prompt)",
    );
    add(
        Normal,
        "Alt-s",
        "split_selection_on_newline",
        Cmd::SplitLines,
        C::None,
        Parity,
        "Split selections into lines",
    );
    add(
        Normal,
        ",",
        "keep_primary_selection",
        Cmd::KeepPrimary,
        C::None,
        Parity,
        "Keep only the primary selection",
    );
    add(
        Normal,
        "(",
        "rotate_selections_backward",
        Cmd::RotatePrev,
        C::None,
        Parity,
        "Make the previous selection primary",
    );
    add(
        Normal,
        ")",
        "rotate_selections_forward",
        Cmd::RotateNext,
        C::None,
        Parity,
        "Make the next selection primary",
    );
    for (d, k) in ["0", "1", "2", "3", "4", "5", "6", "7", "8", "9"]
        .iter()
        .enumerate()
    {
        add(
            Normal,
            k,
            "count",
            Cmd::Count(d as u8),
            C::None,
            Parity,
            "Count prefix (a lone 0 is ignored)",
        );
    }
    add(
        Normal,
        ":",
        "command_mode",
        Cmd::CommandLine,
        C::None,
        Parity,
        "Open the command line",
    );
    add(
        Normal,
        "y",
        "yank",
        Cmd::Yank,
        C::None,
        Quirk,
        "Yank selections (always returns to normal mode, even from select)",
    );
    add(
        Normal,
        "a",
        "append_mode",
        Cmd::Append,
        C::Temp,
        Parity,
        "Insert after the selection",
    );
    add(
        Normal,
        "A",
        "insert_at_line_end",
        Cmd::AppendLineEnd,
        C::Temp,
        Parity,
        "Insert at line end",
    );
    add(
        Normal,
        "I",
        "insert_at_line_start",
        Cmd::InsertLineStart,
        C::Temp,
        Parity,
        "Insert at the first non-blank",
    );
    add(
        Normal,
        "C",
        "copy_selection_on_next_line",
        Cmd::CopyDown,
        C::Add,
        Quirk,
        "Copy the primary selection to the next line where it fits (direction of the copy is flipped)",
    );
    add(
        Normal,
        "Alt-C",
        "copy_selection_on_prev_line",
        Cmd::CopyUp,
        C::Add,
        Extension,
        "Copy the primary selection to the previous line where it fits",
    );
    add(
        Normal,
        "c",
        "change_selection",
        Cmd::Change,
        C::Temp,
        Parity,
        "Delete selections (yank to the default register) and insert",
    );
    add(
        Normal,
        "d",
        "delete_selection",
        Cmd::Delete,
        C::Add,
        Quirk,
        "Delete selections (yanks to the default register, ignores \"x; count/register are not reset)",
    );
    add(
        Normal,
        "Alt-c",
        "change_selection_noyank",
        Cmd::ChangeNoYank,
        C::Temp,
        Parity,
        "Change without yanking",
    );
    add(
        Normal,
        "Alt-d",
        "delete_selection_noyank",
        Cmd::DeleteNoYank,
        C::Add,
        Parity,
        "Delete without yanking",
    );
    add(
        Normal,
        "P",
        "paste_before",
        Cmd::PasteBefore,
        C::Add,
        Quirk,
        "Paste before (with a count, the selection covers one copy only)",
    );
    add(
        Normal,
        "p",
        "paste_after",
        Cmd::PasteAfter,
        C::Add,
        Quirk,
        "Paste after (with a count, the selection covers one copy only)",
    );
    add(
        Normal,
        "R",
        "replace_with_yanked",
        Cmd::ReplaceWithYank,
        C::Add,
        Parity,
        "Replace selections with the register contents",
    );
    add(
        Normal,
        "r",
        "replace",
        Cmd::ReplaceChar,
        C::Add,
        Quirk,
        "Replace every grapheme of the primary selection with a char (newlines too)",
    );
    add(
        Normal,
        "w",
        "move_next_word_start",
        Cmd::WordNext,
        C::None,
        Quirk,
        "Next word (count ignored)",
    );
    add(
        Normal,
        "e",
        "move_next_word_end",
        Cmd::WordNext,
        C::None,
        Quirk,
        "Same as w (cm-helix quirk)",
    );
    add(
        Normal,
        "b",
        "move_prev_word_start",
        Cmd::WordPrev,
        C::None,
        Quirk,
        "Previous word (count ignored)",
    );
    add(
        Normal,
        "W",
        "move_next_long_word_start",
        Cmd::LongWordNext,
        C::None,
        Extension,
        "Next WORD",
    );
    add(
        Normal,
        "E",
        "move_next_long_word_end",
        Cmd::LongWordNext,
        C::None,
        Extension,
        "Next WORD (same as W, like e)",
    );
    add(
        Normal,
        "B",
        "move_prev_long_word_start",
        Cmd::LongWordPrev,
        C::None,
        Extension,
        "Previous WORD",
    );
    add(
        Normal,
        "v",
        "select_mode",
        Cmd::ToggleSelect,
        C::None,
        Parity,
        "Toggle select mode",
    );
    add(
        Normal,
        "g",
        "goto_mode",
        Cmd::GotoMinor,
        C::None,
        Parity,
        "Goto prefix",
    );
    add(
        Normal,
        "Space",
        "space_mode",
        Cmd::SpaceMinor,
        C::None,
        Parity,
        "Space prefix",
    );
    add(
        Normal,
        "m",
        "match_mode",
        Cmd::MatchMinor,
        C::None,
        Parity,
        "Match prefix",
    );
    add(
        Normal,
        "z",
        "view_mode",
        Cmd::ViewMinor,
        C::None,
        Extension,
        "View prefix",
    );
    add(
        Normal,
        "i",
        "insert_mode",
        Cmd::Insert,
        C::Temp,
        Parity,
        "Insert before the selection",
    );
    add(
        Normal,
        "h",
        "move_char_left",
        Cmd::Left,
        C::None,
        Parity,
        "Left",
    );
    add(
        Normal,
        "j",
        "move_visual_line_down",
        Cmd::Down,
        C::None,
        Quirk,
        "Down (single cursor forgets its goal column)",
    );
    add(
        Normal,
        "k",
        "move_visual_line_up",
        Cmd::Up,
        C::None,
        Quirk,
        "Up (single cursor forgets its goal column)",
    );
    add(
        Normal,
        "l",
        "move_char_right",
        Cmd::Right,
        C::None,
        Parity,
        "Right",
    );
    add(
        Normal,
        "ArrowDown",
        "move_visual_line_down",
        Cmd::Down,
        C::None,
        Parity,
        "alias of j",
    );
    add(
        Normal,
        "ArrowUp",
        "move_visual_line_up",
        Cmd::Up,
        C::None,
        Parity,
        "alias of k",
    );
    add(
        Normal,
        "ArrowRight",
        "move_char_right",
        Cmd::Right,
        C::None,
        Parity,
        "alias of l",
    );
    add(
        Normal,
        "ArrowLeft",
        "move_char_left",
        Cmd::Left,
        C::None,
        Parity,
        "alias of h",
    );
    add(
        Normal,
        "%",
        "select_all",
        Cmd::SelectAll,
        C::None,
        Parity,
        "Select the whole document",
    );
    add(
        Normal,
        "o",
        "open_below",
        Cmd::OpenBelow,
        C::Temp,
        Parity,
        "Open a line below (keeps indentation / list markers with continue_markup)",
    );
    add(
        Normal,
        "O",
        "open_above",
        Cmd::OpenAbove,
        C::Temp,
        Parity,
        "Open a line above (keeps indentation / list markers with continue_markup)",
    );
    add(
        Normal,
        "f",
        "find_next_char",
        Cmd::Find {
            inclusive: true,
            forward: true,
        },
        C::None,
        Quirk,
        "Find char forward (drops a pending register)",
    );
    add(
        Normal,
        "F",
        "find_prev_char",
        Cmd::Find {
            inclusive: true,
            forward: false,
        },
        C::None,
        Quirk,
        "Find char backward (drops a pending register)",
    );
    add(
        Normal,
        "t",
        "find_till_char",
        Cmd::Find {
            inclusive: false,
            forward: true,
        },
        C::None,
        Quirk,
        "Till char forward (drops a pending register)",
    );
    add(
        Normal,
        "T",
        "till_prev_char",
        Cmd::Find {
            inclusive: false,
            forward: false,
        },
        C::None,
        Quirk,
        "Till char backward (drops a pending register)",
    );
    add(Normal, "u", "undo", Cmd::Undo, C::None, Parity, "Undo");
    add(Normal, "U", "redo", Cmd::Redo, C::None, Parity, "Redo");
    add(
        Normal,
        "x",
        "extend_line_below",
        Cmd::ExtendLine,
        C::None,
        Quirk,
        "Select line / extend to the next line (a count always extends)",
    );
    add(
        Normal,
        "n",
        "search_next",
        Cmd::SearchNext,
        C::None,
        Quirk,
        "Next match (count not reset)",
    );
    add(
        Normal,
        "N",
        "search_prev",
        Cmd::SearchPrev,
        C::None,
        Quirk,
        "Previous match (cm-helix ring-buffer bug with counts)",
    );
    add(
        Normal,
        "Ctrl-a",
        "increment",
        Cmd::Increment,
        C::Add,
        Quirk,
        "Increment the selected number (JS Number() of the whole selection)",
    );
    add(
        Normal,
        "Ctrl-x",
        "decrement",
        Cmd::Decrement,
        C::Add,
        Quirk,
        "Decrement the selected number (JS Number() of the whole selection)",
    );
    add(
        Normal,
        "Ctrl-d",
        "page_cursor_half_down",
        Cmd::HalfPageDown,
        C::None,
        Parity,
        "Half page down",
    );
    add(
        Normal,
        "Ctrl-u",
        "page_cursor_half_up",
        Cmd::HalfPageUp,
        C::None,
        Parity,
        "Half page up",
    );
    add(
        Normal,
        "PageDown",
        "page_cursor_half_down",
        Cmd::HalfPageDown,
        C::None,
        Parity,
        "alias of Ctrl-d",
    );
    add(
        Normal,
        "PageUp",
        "page_cursor_half_up",
        Cmd::HalfPageUp,
        C::None,
        Parity,
        "alias of Ctrl-u",
    );
    add(
        Normal,
        ";",
        "collapse_selection",
        Cmd::Collapse,
        C::None,
        Parity,
        "Collapse to the cursor",
    );
    add(
        Normal,
        "Alt-;",
        "flip_selections",
        Cmd::Flip,
        C::None,
        Parity,
        "Flip anchor and head",
    );
    add(
        Normal,
        "Alt-:",
        "ensure_selections_forward",
        Cmd::EnsureForward,
        C::None,
        Parity,
        "Make selections forward",
    );
    add(
        Normal,
        "Alt-ArrowUp",
        "expand_selection",
        Cmd::ExpandSyntax,
        C::None,
        Quirk,
        "Expand to the parent syntax node (host syntax tree; history freezes when there is no parent)",
    );
    add(
        Normal,
        "Alt-ArrowDown",
        "shrink_selection",
        Cmd::ShrinkSyntax,
        C::None,
        Parity,
        "Undo the last expansion",
    );
    add(
        Normal,
        "Alt-ArrowRight",
        "select_next_sibling",
        Cmd::SiblingNext,
        C::None,
        Quirk,
        "Next syntax sibling (no tree: selects the whole doc)",
    );
    add(
        Normal,
        "Alt-ArrowLeft",
        "select_prev_sibling",
        Cmd::SiblingPrev,
        C::None,
        Quirk,
        "Previous syntax sibling (no tree: selects the whole doc)",
    );
    add(
        Normal,
        "Alt-o",
        "expand_selection",
        Cmd::ExpandSyntax,
        C::None,
        Parity,
        "alias of Alt-ArrowUp",
    );
    add(
        Normal,
        "Alt-i",
        "shrink_selection",
        Cmd::ShrinkSyntax,
        C::None,
        Parity,
        "alias of Alt-ArrowDown",
    );
    add(
        Normal,
        "Alt-n",
        "select_next_sibling",
        Cmd::SiblingNext,
        C::None,
        Parity,
        "alias of Alt-ArrowRight",
    );
    add(
        Normal,
        "Alt-p",
        "select_prev_sibling",
        Cmd::SiblingPrev,
        C::None,
        Parity,
        "alias of Alt-ArrowLeft",
    );
    add(
        Normal,
        "Ctrl-c",
        "toggle_comments",
        Cmd::ToggleComment,
        C::Add,
        Parity,
        "Toggle comments (block comments by line for Markdown)",
    );
    add(Normal, ">", "indent", Cmd::Indent, C::Add, Parity, "Indent");
    add(
        Normal,
        "<",
        "unindent",
        Cmd::Dedent,
        C::Add,
        Parity,
        "Dedent",
    );
    add(
        Normal,
        "`",
        "switch_to_lowercase",
        Cmd::Lower,
        C::Add,
        Parity,
        "Lower case",
    );
    add(
        Normal,
        "Alt-`",
        "switch_to_uppercase",
        Cmd::Upper,
        C::Add,
        Parity,
        "Upper case",
    );
    add(
        Normal,
        "~",
        "switch_case",
        Cmd::SwitchCase,
        C::Add,
        Parity,
        "Toggle case",
    );
    add(
        Normal,
        "*",
        "search_selection",
        Cmd::StarSearch,
        C::None,
        Parity,
        "Use the selections as the search pattern",
    );
    add(
        Normal,
        "_",
        "trim_selections",
        Cmd::Trim,
        C::None,
        Parity,
        "Trim whitespace from the selections",
    );
    add(
        Normal,
        "Home",
        "goto_line_start",
        Cmd::LineStart,
        C::None,
        Parity,
        "Line start",
    );
    add(
        Normal,
        "End",
        "goto_line_end",
        Cmd::LineEnd,
        C::None,
        Parity,
        "Line end",
    );
    add(
        Normal,
        "J",
        "join_selections",
        Cmd::Join,
        C::Add,
        Parity,
        "Join lines (primary selection)",
    );
    add(
        Normal,
        "[",
        "left_bracket_mode",
        Cmd::LeftBracketMinor,
        C::None,
        Parity,
        "[ prefix",
    );
    add(
        Normal,
        "]",
        "right_bracket_mode",
        Cmd::RightBracketMinor,
        C::None,
        Parity,
        "] prefix",
    );
    add(
        Normal,
        "\"",
        "select_register",
        Cmd::SelectRegister,
        C::None,
        Parity,
        "Select a register for the next command",
    );
    add(
        Normal,
        "Tab",
        "no_op",
        Cmd::Nop,
        C::None,
        Parity,
        "Swallowed",
    );
    add(
        Normal,
        "Shift-Tab",
        "no_op",
        Cmd::Nop,
        C::None,
        Parity,
        "Swallowed",
    );
    add(
        Normal,
        "S",
        "split_selection",
        Cmd::SplitRegex,
        C::None,
        Extension,
        "Split selections on regex matches (prompt)",
    );
    add(
        Normal,
        "K",
        "keep_selections",
        Cmd::KeepRegex,
        C::None,
        Extension,
        "Keep selections matching a regex (prompt)",
    );
    add(
        Normal,
        "Alt-K",
        "remove_selections",
        Cmd::RemoveRegex,
        C::None,
        Extension,
        "Remove selections matching a regex (prompt)",
    );

    // ---- goto -----------------------------------------------------------
    add(
        Goto,
        "g",
        "goto_file_start",
        Cmd::DocStart,
        C::None,
        Parity,
        "Document start",
    );
    add(
        Goto,
        "e",
        "goto_last_line",
        Cmd::LastLine,
        C::None,
        Quirk,
        "Last line (the line before a trailing empty line)",
    );
    add(
        Goto,
        "h",
        "goto_line_start",
        Cmd::LineStart,
        C::None,
        Parity,
        "Line start",
    );
    add(
        Goto,
        "j",
        "move_line_down",
        Cmd::GotoDown,
        C::None,
        Parity,
        "Down",
    );
    add(
        Goto,
        "k",
        "move_line_up",
        Cmd::GotoUp,
        C::None,
        Parity,
        "Up",
    );
    add(
        Goto,
        "l",
        "goto_line_end",
        Cmd::LineEnd,
        C::None,
        Parity,
        "Line end",
    );
    add(
        Goto,
        "s",
        "goto_first_nonwhitespace",
        Cmd::FirstNonBlank,
        C::None,
        Parity,
        "First non-blank",
    );
    add(
        Goto,
        "n",
        "goto_next_buffer",
        Cmd::BufferNext,
        C::None,
        Parity,
        "Next buffer (external)",
    );
    add(
        Goto,
        "p",
        "goto_previous_buffer",
        Cmd::BufferPrevious,
        C::None,
        Parity,
        "Previous buffer (external)",
    );
    add(
        Goto,
        "t",
        "goto_window_top",
        Cmd::WindowTop,
        C::None,
        Extension,
        "Top of the window",
    );
    add(
        Goto,
        "c",
        "goto_window_center",
        Cmd::WindowCenter,
        C::None,
        Extension,
        "Center of the window",
    );
    add(
        Goto,
        "b",
        "goto_window_bottom",
        Cmd::WindowBottom,
        C::None,
        Extension,
        "Bottom of the window",
    );

    // ---- match ----------------------------------------------------------
    add(
        Match,
        "s",
        "surround_add",
        Cmd::SurroundAdd,
        C::Add,
        Parity,
        "Surround the selections with a pair",
    );
    add(
        Match,
        "m",
        "match_brackets",
        Cmd::MatchBracket,
        C::None,
        Quirk,
        "Jump to the matching bracket (quotes are not matched)",
    );
    add(
        Match,
        "a",
        "select_textobject_around",
        Cmd::SelectAround,
        C::None,
        Quirk,
        "Select around a pair / m (closest pair) / p (paragraph); w and quotes with extensions",
    );
    add(
        Match,
        "i",
        "select_textobject_inner",
        Cmd::SelectInside,
        C::None,
        Quirk,
        "Select inside a pair / m / p; w and quotes with extensions",
    );
    add(
        Match,
        "d",
        "surround_delete",
        Cmd::SurroundDelete,
        C::Add,
        Extension,
        "Delete the surrounding pair",
    );
    add(
        Match,
        "r",
        "surround_replace",
        Cmd::SurroundReplace,
        C::Add,
        Extension,
        "Replace the surrounding pair",
    );

    // ---- space ----------------------------------------------------------
    add(
        Space,
        "y",
        "yank_to_clipboard",
        Cmd::ClipboardYank,
        C::None,
        Parity,
        "Yank to the clipboard (+)",
    );
    add(
        Space,
        "p",
        "paste_clipboard_after",
        Cmd::ClipboardPasteAfter,
        C::Add,
        Parity,
        "Paste the clipboard after",
    );
    add(
        Space,
        "P",
        "paste_clipboard_before",
        Cmd::ClipboardPasteBefore,
        C::Add,
        Parity,
        "Paste the clipboard before",
    );
    add(
        Space,
        "R",
        "replace_selections_with_clipboard",
        Cmd::ClipboardReplace,
        C::Add,
        Quirk,
        "Replace selections with the clipboard (cursors end up after the text)",
    );
    add(
        Space,
        "f",
        "file_picker",
        Cmd::FilePicker,
        C::None,
        Parity,
        "File picker (external)",
    );
    add(
        Space,
        "b",
        "buffer_picker",
        Cmd::BufferPicker,
        C::None,
        Parity,
        "Buffer picker (external)",
    );
    add(
        Space,
        "/",
        "global_search",
        Cmd::GlobalSearch,
        C::None,
        Parity,
        "Global search (external)",
    );
    add(
        Space,
        "c",
        "toggle_comments",
        Cmd::SpaceComment,
        C::Add,
        Parity,
        "Toggle comments",
    );

    // ---- brackets -------------------------------------------------------
    for (d, k) in ["0", "1", "2", "3", "4", "5", "6", "7", "8", "9"]
        .iter()
        .enumerate()
    {
        add(
            LeftBracket,
            k,
            "count",
            Cmd::Count(d as u8),
            C::None,
            Quirk,
            "Count digits are accepted after [",
        );
        add(
            RightBracket,
            k,
            "count",
            Cmd::Count(d as u8),
            C::None,
            Quirk,
            "Count digits are accepted after ]",
        );
    }
    add(
        LeftBracket,
        "Space",
        "add_newline_above",
        Cmd::InsertLineAbove,
        C::Add,
        Parity,
        "Add blank lines above",
    );
    add(
        RightBracket,
        "Space",
        "add_newline_below",
        Cmd::InsertLineBelow,
        C::Add,
        Parity,
        "Add blank lines below",
    );

    // ---- view (extension) -------------------------------------------------
    add(
        View,
        "z",
        "align_view_center",
        Cmd::AlignCenter,
        C::None,
        Extension,
        "Center the cursor line",
    );
    add(
        View,
        "c",
        "align_view_center",
        Cmd::AlignCenter,
        C::None,
        Extension,
        "Center the cursor line",
    );
    add(
        View,
        "t",
        "align_view_top",
        Cmd::AlignTop,
        C::None,
        Extension,
        "Cursor line to the top",
    );
    add(
        View,
        "b",
        "align_view_bottom",
        Cmd::AlignBottom,
        C::None,
        Extension,
        "Cursor line to the bottom",
    );
    add(
        View,
        "j",
        "scroll_down",
        Cmd::ScrollDown,
        C::None,
        Extension,
        "Scroll down one line",
    );
    add(
        View,
        "k",
        "scroll_up",
        Cmd::ScrollUp,
        C::None,
        Extension,
        "Scroll up one line",
    );
    add(
        View,
        "ArrowDown",
        "scroll_down",
        Cmd::ScrollDown,
        C::None,
        Extension,
        "Scroll down one line",
    );
    add(
        View,
        "ArrowUp",
        "scroll_up",
        Cmd::ScrollUp,
        C::None,
        Extension,
        "Scroll up one line",
    );
    v
}

impl BindingStatus {
    pub fn label(&self) -> &'static str {
        match self {
            BindingStatus::Parity => "parity",
            BindingStatus::Quirk => "quirk",
            BindingStatus::Extension => "extension",
        }
    }
}

/// The binding tables as Markdown (the generated part of `COMMANDS.md`).
/// Count digits are folded into one `0`–`9` row per table.
pub fn commands_markdown() -> String {
    let tables = [
        KeymapTable::Insert,
        KeymapTable::Normal,
        KeymapTable::Goto,
        KeymapTable::Match,
        KeymapTable::Space,
        KeymapTable::LeftBracket,
        KeymapTable::RightBracket,
        KeymapTable::View,
    ];
    let mut out = String::new();
    for table in tables {
        out.push_str(&format!("### {}\n\n", table.label()));
        out.push_str("| Key | Command | Status | Undo | Description |\n|---|---|---|---|---|\n");
        let mut digits_done = false;
        for b in bindings().iter().filter(|b| b.table == table) {
            let key = if matches!(b.cmd, Cmd::Count(_)) {
                if digits_done {
                    continue;
                }
                digits_done = true;
                "0`–`9".to_string()
            } else {
                b.key.replace('|', "\\|")
            };
            let undo = match b.checkpoint {
                CheckpointKind::None => "",
                CheckpointKind::Add => "yes",
                CheckpointKind::Temp => "session",
            };
            out.push_str(&format!(
                "| `{key}` | `{}` | {} | {undo} | {} |\n",
                b.command,
                b.status.label(),
                b.description.replace('|', "\\|")
            ));
        }
        out.push('\n');
    }
    out
}
