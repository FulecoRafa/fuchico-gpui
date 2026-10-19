//! Vim modal engine for Fuchico: a port of @replit/codemirror-vim 6.3.0
//! with the same Host / Effect / prompt API as `fuchico-helix`.
//!
//! Feed keys with [`Editor::handle_key`] (or [`Editor::handle_keys`] in
//! `"3dw<Esc>"` notation) and text input with [`Editor::insert_text`];
//! apply the returned [`Effect`]s. The `:` / `/` / `?` prompts are owned by
//! the editor: [`Editor::prompt_kind`], [`Editor::prompt_input`],
//! [`Editor::prompt_close`], [`Editor::prompt_history`],
//! [`Editor::prompt_completions`]; `:s///c` asks through
//! [`Editor::confirm_prompt`]. Registers, search, macros and mappings live
//! in [`VimGlobals`] (share them between editors with
//! [`Editor::take_globals`] / [`Editor::set_globals`]).
//!
//! Known deviations from cm-vim: `%` is not syntax-aware; page / scroll
//! motions use a fixed viewport estimate; tag text objects (`it` / `at`)
//! are no-ops; `=` (indentAuto) does nothing; Tab in insert mode and
//! `<C-c>` copy are left to the host (`Effect::Unhandled`); the search
//! highlight is not debounced; `:q`, `:wq` and `:x` are Fuchico extras
//! gated by `Config::app_commands`.

mod actions;
mod adapter;
mod cmdlist;
mod dispatch;
mod editor;
mod ex;
mod history;
mod insert;
mod keymap;
mod motions;
mod operators;
mod pos;
mod prompt;
mod search;
mod searchops;
mod state;
mod util;

pub use cmdlist::commands_markdown;
pub use dispatch::vim_key_from_event;
pub use editor::*;
pub use ex::{ExCommand, ExKind};
pub use fuchico_helix::{
    Change, ChangeSet, Completions, Config, Effect, Host, Key, KeyEvent, MemoryHost, Modifiers,
    ModeType, PromptKind, Range, Scroll, ScrollAlign, SearchKind, Selection, Text,
};
pub use pos::Pos;
pub use state::{OptionValue, VimGlobals};
