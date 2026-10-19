//! [`Engine`]: the key engine behind an [`crate::EditorView`], either the
//! Helix engine (`fuchico-helix`, also used non-modally for
//! [`crate::EngineKind::Normal`]) or the Vim engine (`fuchico-vim`). Both
//! share the same document / selection / effect types (`fuchico-vim`
//! re-exports `fuchico-helix`'s), so the view talks to one API and only
//! engine-specific features (prompt prefix, Vim cursor shapes, the
//! `hlsearch` overlay, register reads) branch.

use std::cell::RefCell;
use std::rc::Rc;

use fuchico_helix::{
    Change, Completions, Config, Effect, Host, KeyEvent, ModeType, PromptKind, Selection, Text,
};
pub use fuchico_vim::{CursorShape as VimCursorShape, VimGlobals};

/// Vim state shared by every Vim editor of a window (cm-vim keeps one
/// `vimGlobalState` per page): registers, search, macros, mappings.
pub type SharedVimGlobals = Rc<RefCell<VimGlobals>>;

/// The Vim engine plus the globals it shares with other editors.
pub struct VimEngine {
    ed: fuchico_vim::Editor,
    shared: Option<SharedVimGlobals>,
}

impl VimEngine {
    /// Runs `f` with the shared globals swapped into the editor (O(1): the
    /// editor's own copy waits in the cell meanwhile).
    fn with<R>(&mut self, f: impl FnOnce(&mut fuchico_vim::Editor) -> R) -> R {
        if let Some(s) = &self.shared {
            std::mem::swap(self.ed.globals_mut(), &mut *s.borrow_mut());
        }
        let r = f(&mut self.ed);
        if let Some(s) = &self.shared {
            std::mem::swap(self.ed.globals_mut(), &mut *s.borrow_mut());
        }
        r
    }

    /// Reads the live globals (shared or the editor's own).
    pub fn read_globals<R>(&self, f: impl FnOnce(&VimGlobals) -> R) -> R {
        match &self.shared {
            Some(s) => f(&s.borrow()),
            None => f(self.ed.globals()),
        }
    }

    pub fn editor(&self) -> &fuchico_vim::Editor {
        &self.ed
    }
}

/// The view's key engine.
pub enum Engine {
    Helix(Box<fuchico_helix::Editor>),
    Vim(Box<VimEngine>),
}

macro_rules! both {
    ($self:expr, $e:ident => $body:expr) => {
        match $self {
            Engine::Helix($e) => $body,
            Engine::Vim(v) => {
                let $e = &v.ed;
                $body
            }
        }
    };
}

macro_rules! both_mut {
    ($self:expr, $e:ident => $body:expr) => {
        match $self {
            Engine::Helix($e) => $body,
            Engine::Vim(v) => v.with(|$e| $body),
        }
    };
}

impl Engine {
    pub fn helix(text: &str, config: Config, host: Box<dyn Host>) -> Self {
        Engine::Helix(Box::new(fuchico_helix::Editor::with_host(
            text, config, host,
        )))
    }

    /// A Vim engine. cm-vim's own commands only: Fuchico's `:q` / `:wq` /
    /// `:x` extras stay off (`app_commands`), like the original, which
    /// defines no Vim ex commands.
    pub fn vim(
        text: &str,
        config: Config,
        host: Box<dyn Host>,
        shared: Option<SharedVimGlobals>,
    ) -> Self {
        let config = Config {
            app_commands: false,
            ..config
        };
        Engine::Vim(Box::new(VimEngine {
            ed: fuchico_vim::Editor::with_host(text, config, host),
            shared,
        }))
    }

    pub fn is_vim(&self) -> bool {
        matches!(self, Engine::Vim(_))
    }
    pub fn as_helix(&self) -> Option<&fuchico_helix::Editor> {
        match self {
            Engine::Helix(e) => Some(e),
            Engine::Vim(_) => None,
        }
    }
    pub fn as_helix_mut(&mut self) -> Option<&mut fuchico_helix::Editor> {
        match self {
            Engine::Helix(e) => Some(e),
            Engine::Vim(_) => None,
        }
    }
    pub fn as_vim(&self) -> Option<&VimEngine> {
        match self {
            Engine::Vim(v) => Some(v),
            Engine::Helix(_) => None,
        }
    }
    /// Runs `f` on the Vim editor (with the shared globals in place).
    pub fn with_vim<R>(&mut self, f: impl FnOnce(&mut fuchico_vim::Editor) -> R) -> Option<R> {
        match self {
            Engine::Vim(v) => Some(v.with(f)),
            Engine::Helix(_) => None,
        }
    }
    pub(crate) fn set_shared_vim_globals(&mut self, shared: Option<SharedVimGlobals>) {
        if let Engine::Vim(v) = self {
            v.shared = shared;
        }
    }

    // ---- reads -----------------------------------------------------------------

    pub fn doc(&self) -> &Text {
        both!(self, e => e.doc())
    }
    pub fn text(&self) -> String {
        both!(self, e => e.text())
    }
    pub fn selection(&self) -> &Selection {
        both!(self, e => e.selection())
    }
    pub fn mode_type(&self) -> ModeType {
        match self {
            Engine::Helix(e) => e.mode().ty,
            Engine::Vim(v) => v.ed.mode_type(),
        }
    }
    pub fn mode_label(&self) -> &'static str {
        both!(self, e => e.mode_label())
    }
    pub fn pending_status(&self) -> String {
        both!(self, e => e.pending_status())
    }
    pub fn line_col(&self) -> (usize, usize) {
        both!(self, e => e.line_col())
    }
    pub fn block_cursors(&self) -> Vec<usize> {
        both!(self, e => e.block_cursors())
    }
    pub fn config(&self) -> &Config {
        both!(self, e => e.config())
    }
    pub fn config_mut(&mut self) -> &mut Config {
        match self {
            Engine::Helix(e) => e.config_mut(),
            Engine::Vim(v) => v.ed.config_mut(),
        }
    }

    // ---- edits / keys ----------------------------------------------------------

    pub fn set_selection(&mut self, sel: Selection) -> Vec<Effect> {
        both_mut!(self, e => e.set_selection(sel))
    }
    pub fn set_selection_from_pointer(&mut self, sel: Selection) -> Vec<Effect> {
        both_mut!(self, e => e.set_selection_from_pointer(sel))
    }
    pub fn set_text(&mut self, text: &str) -> Vec<Effect> {
        both_mut!(self, e => e.set_text(text))
    }
    /// An edit inside the running insert session (no undo step of its own
    /// in Helix).
    pub fn apply_changes(&mut self, changes: Vec<Change>) -> Vec<Effect> {
        both_mut!(self, e => e.apply_changes(changes))
    }
    /// A host edit recorded as an undo step (`group` coalesces runs in
    /// Helix; Vim's CodeMirror history groups by time on its own).
    pub fn apply_host_edit(&mut self, changes: Vec<Change>, group: Option<&str>) -> Vec<Effect> {
        match self {
            Engine::Helix(e) => e.apply_host_edit(changes, group),
            Engine::Vim(v) => v.with(|e| e.apply_changes(changes)),
        }
    }
    pub fn handle_key(&mut self, ev: KeyEvent) -> Vec<Effect> {
        both_mut!(self, e => e.handle_key(ev))
    }
    pub fn insert_text(&mut self, text: &str) -> Vec<Effect> {
        both_mut!(self, e => e.insert_text(text))
    }
    pub fn execute_command(&mut self, line: &str) -> Vec<Effect> {
        both_mut!(self, e => e.execute_command(line))
    }

    // ---- prompt -----------------------------------------------------------------

    pub fn prompt_input(&mut self, value: &str) -> Vec<Effect> {
        both_mut!(self, e => e.prompt_input(value))
    }
    pub fn prompt_close(&mut self, accept: bool) -> Vec<Effect> {
        both_mut!(self, e => e.prompt_close(accept))
    }
    pub fn prompt_history(&mut self, older: bool) -> Option<String> {
        both_mut!(self, e => e.prompt_history(older))
    }
    pub fn prompt_completions(&mut self, value: &str) -> Completions {
        both_mut!(self, e => e.prompt_completions(value))
    }
    pub fn prompt_accept_completion(&self, value: &str, option: &str) -> String {
        both!(self, e => e.prompt_accept_completion(value, option))
    }
    /// Helix `Ctrl-r <reg>` in the command line (cm-vim's prompt has none).
    pub fn prompt_read_register(&mut self, reg: char) -> Result<String, String> {
        match self {
            Engine::Helix(e) => e.prompt_read_register(reg),
            Engine::Vim(_) => Err(String::new()),
        }
    }
    /// The open engine prompt's kind (Vim only; Helix prompts are driven by
    /// the view).
    pub fn vim_prompt_kind(&self) -> Option<PromptKind> {
        self.as_vim().and_then(|v| v.ed.prompt_kind())
    }
}
