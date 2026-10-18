//! A UI-agnostic Helix modal editing engine: a port of
//! [codemirror-helix](https://gitlab.com/_rvidal/codemirror-helix) 0.5.7 as
//! used by Fuchico, over a [`ropey`] rope.
//!
//! Positions are char offsets. Selections use CodeMirror semantics (a
//! normal-mode block cursor on char `n` is the range `n..n+1`); see
//! [`selection`]. Feed keys to [`Editor::handle_key`] and apply the returned
//! [`Effect`]s. `COMMANDS.md` lists every binding.

pub mod change;
mod commands;
mod edit;
mod editor;
pub mod effect;
pub mod history;
pub mod host;
pub mod jsnum;
pub mod keymap;
pub mod keys;
pub mod markup;
pub mod mode;
mod prompt;
pub mod registers;
pub mod search;
pub mod selection;
pub mod text;
mod textobj;
mod typable;

#[cfg(test)]
mod tests;

pub use change::{Change, ChangeSet, TextEdit};
pub use editor::Editor;
pub use effect::{Effect, External, Scroll, ScrollAlign, SearchKind};
pub use history::{Checkpoint, History};
pub use host::{
    CommentTokens, Config, ExternalCommands, Host, MemoryHost, VisualLines, logical_move_vertically,
};
pub use keymap::{
    Binding, BindingStatus, CheckpointKind, KeymapTable, bindings, commands_markdown,
};
pub use keys::{Key, KeyEvent, Modifiers, parse_keys};
pub use mode::{Expecting, Minor, Mode, ModeType};
pub use prompt::{Completions, PromptKind};
pub use registers::Registers;
pub use search::SearchQuery;
pub use selection::{Range, Selection};
pub use text::{Line, Text};
pub use typable::CommandInfo;
