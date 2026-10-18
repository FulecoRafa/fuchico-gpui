//! Effects returned to the host after each input.

use crate::change::TextEdit;
use crate::mode::ModeType;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScrollAlign {
    /// Scroll the minimum amount to make the target visible.
    Nearest,
    Center,
    Top,
    Bottom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scroll {
    /// Make `pos` (char offset) visible with the given alignment.
    To { pos: usize, align: ScrollAlign },
    /// Scroll the view by `n` lines without moving the cursor (positive =
    /// down). Extension (`zj`/`zk`).
    Lines(isize),
    /// Restore the scroll position the view had when the current prompt was
    /// opened (search/command cancelled).
    RestorePromptStart,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchKind {
    /// `/` — forward search, previews the next match.
    Forward,
    /// `s` — select regex matches inside the selections.
    Selection,
    /// `Space /` — global (workspace) search, handed to the host.
    Global,
    /// `S` — split selections on matches (extension).
    Split,
    /// `K` — keep selections matching (extension).
    Keep,
    /// `Alt-K` — remove selections matching (extension).
    Remove,
}

impl SearchKind {
    /// Prompt label cm-helix shows.
    pub fn label(&self) -> &'static str {
        match self {
            SearchKind::Global => "global-search:",
            SearchKind::Split => "split:",
            SearchKind::Keep => "keep:",
            SearchKind::Remove => "remove:",
            _ => "search:",
        }
    }
}

/// Commands cm-helix delegates to the embedding app ("external commands").
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum External {
    FilePicker,
    BufferPicker,
    BufferNext,
    BufferPrevious,
    BufferClose(Vec<String>),
    GlobalSearch(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    /// The document changed. Edits are relative to the previous document.
    DocChanged(Vec<TextEdit>),
    /// The selection changed (compare with [`crate::Editor::selection`]).
    SelectionChanged,
    ModeChanged {
        from: ModeType,
        to: ModeType,
    },
    /// Pending-key status (count, minor mode, register) changed; the new
    /// text is [`crate::Editor::pending_status`].
    StatusChanged,
    Scroll(Scroll),
    /// A status-line message (cm-helix `showMessage` / `showError`).
    Message {
        text: String,
        error: bool,
    },
    /// `:` was pressed: open the command line (Fuchico maps it to its
    /// command palette). `placeholder` is the last command (`:` register).
    OpenCommandLine {
        placeholder: Option<String>,
    },
    /// Open a search-style prompt; drive it with
    /// [`crate::Editor::prompt_input`] / [`crate::Editor::prompt_close`].
    OpenSearch {
        kind: SearchKind,
        placeholder: Option<String>,
    },
    External(External),
    /// `:write`.
    Save,
    /// `:quit`.
    Quit,
    /// A host-registered typable command was executed.
    HostCommand {
        name: String,
        args: Vec<String>,
    },
    /// `:theme <name>` switched themes.
    ChangeTheme(String),
    /// The key is not bound in the current mode; the host may handle it
    /// (CodeMirror would fall through to its default keymap).
    Unhandled,
}
