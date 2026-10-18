//! Host integration traits and configuration.

use crate::text::{Text, count_column, find_cluster_break, find_column};

/// Vertical motion. The default implementation moves by logical lines and
/// uses text columns as the goal; a host with soft wrap overrides it to move
/// by visual lines (the goal is then whatever unit the host likes, e.g.
/// pixels — the engine only stores and passes it back).
pub trait VisualLines {
    /// CodeMirror `view.moveVertically`: move `lines` lines up/down from
    /// `pos`. Returns the new position and the goal to remember. Moving past
    /// the first line lands on 0, past the last line on `doc.len()`.
    fn move_vertically(
        &mut self,
        doc: &Text,
        pos: usize,
        goal: Option<usize>,
        forward: bool,
        lines: usize,
        tab_size: usize,
    ) -> (usize, Option<usize>) {
        logical_move_vertically(doc, pos, goal, forward, lines, tab_size)
    }

    /// Lines between `pos`'s line and the document end in the direction of
    /// travel (used to clamp half-page motion).
    fn lines_to_boundary(&mut self, doc: &Text, pos: usize, forward: bool) -> usize {
        let line = doc.line_at(pos).number;
        if forward {
            doc.lines() - line
        } else {
            line - 1
        }
    }
}

/// The logical-line implementation of [`VisualLines::move_vertically`].
pub fn logical_move_vertically(
    doc: &Text,
    pos: usize,
    goal: Option<usize>,
    forward: bool,
    lines: usize,
    tab_size: usize,
) -> (usize, Option<usize>) {
    if pos == if forward { doc.len() } else { 0 } {
        return (pos, goal);
    }
    let line = doc.line_at(pos);
    let text = doc.line_text(&line);
    let goal = goal.unwrap_or_else(|| count_column(&text, tab_size, pos - line.from));
    let lines = lines.max(1);
    let target = if forward {
        line.number + lines
    } else if line.number > lines {
        line.number - lines
    } else {
        return (0, Some(goal));
    };
    if target > doc.lines() {
        return (doc.len(), Some(goal));
    }
    let tl = doc.line(target);
    let ttext = doc.line_text(&tl);
    let mut off = find_column(&ttext, goal, tab_size, false).unwrap_or(0);
    // land on a cluster boundary
    let fixed = find_cluster_break(&ttext, find_cluster_break(&ttext, off, false), true);
    if fixed > off && off > 0 {
        off = find_cluster_break(&ttext, off, false);
    }
    (tl.from + off, Some(goal))
}

/// Everything cm-helix gets from the DOM/browser.
pub trait Host: VisualLines {
    /// System clipboard read (`+` register). `Err` is shown as an error
    /// message, like a rejected `navigator.clipboard.readText()`.
    fn clipboard_read(&mut self) -> Result<String, String> {
        Err("clipboard unavailable".into())
    }
    fn clipboard_write(&mut self, _text: &str) {}

    /// Half the viewport height, in lines (`Ctrl-d` / `Ctrl-u`).
    fn half_page_lines(&mut self) -> usize {
        20
    }

    /// First and last visible (1-based) line numbers, for `gt`/`gc`/`gb`
    /// (extension). `None` disables those commands.
    fn visible_lines(&mut self) -> Option<(usize, usize)> {
        None
    }

    /// Value of the `%` register (cm-helix `pathRegister`).
    fn path(&mut self) -> Option<String> {
        None
    }

    /// Smallest syntax node strictly enclosing `from..to` that is not the
    /// top node (CodeMirror `selectParentSyntax`). `None` = no syntax tree.
    fn syntax_parent(&mut self, _doc: &Text, _from: usize, _to: usize) -> Option<(usize, usize)> {
        None
    }

    /// Next/previous sibling of the innermost node covering `from..to`
    /// (cm-helix `moveToSibling`). `None` selects the whole document.
    fn syntax_sibling(
        &mut self,
        _doc: &Text,
        _from: usize,
        _to: usize,
        _forward: bool,
    ) -> Option<(usize, usize)> {
        None
    }
}

/// A host that keeps the clipboard in memory. Used by default.
#[derive(Debug, Default, Clone)]
pub struct MemoryHost {
    pub clipboard: Option<String>,
    pub half_page: Option<usize>,
    pub visible: Option<(usize, usize)>,
    pub path: Option<String>,
}

impl VisualLines for MemoryHost {}

impl Host for MemoryHost {
    fn clipboard_read(&mut self) -> Result<String, String> {
        Ok(self.clipboard.clone().unwrap_or_default())
    }
    fn clipboard_write(&mut self, text: &str) {
        self.clipboard = Some(text.to_string());
    }
    fn half_page_lines(&mut self) -> usize {
        self.half_page.unwrap_or(20)
    }
    fn visible_lines(&mut self) -> Option<(usize, usize)> {
        self.visible
    }
    fn path(&mut self) -> Option<String> {
        self.path.clone()
    }
}

/// Comment syntax of the current language (CodeMirror `commentTokens`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommentTokens {
    pub line: Option<String>,
    pub block: Option<(String, String)>,
}

impl CommentTokens {
    /// Markdown (`@codemirror/lang-markdown`): block comments only.
    pub fn markdown() -> Self {
        CommentTokens {
            line: None,
            block: Some(("<!--".into(), "-->".into())),
        }
    }
}

/// Which cm-helix "external commands" the host implements. Disabled ones
/// are no-ops, exactly like a missing handler in cm-helix.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExternalCommands {
    pub file_picker: bool,
    pub buffer_picker: bool,
    pub buffer_next: bool,
    pub buffer_previous: bool,
    pub buffer_close: bool,
    pub global_search: bool,
}

#[derive(Debug, Clone)]
pub struct Config {
    pub tab_size: usize,
    /// CodeMirror `indentUnit` (`"  "` by default).
    pub indent_unit: String,
    /// `editor.default-yank-register`.
    pub default_register: char,
    pub comment_tokens: CommentTokens,
    /// Keep indentation and continue Markdown list/quote markers on
    /// `o`/`O`/`Enter` (Fuchico issue #54). Not in cm-helix.
    pub continue_markup: bool,
    /// Enable bindings that cm-helix does not have (W/B/E, `z` view mode,
    /// gt/gc/gb, S/K/Alt-K, Alt-C, md/mr, miw/mi"/mi<, insert Ctrl-w/u/k).
    pub extensions: bool,
    /// Add Fuchico's `:write`/`:quit`/`:wq` typable commands.
    pub app_commands: bool,
    pub themes: Vec<String>,
    pub theme: Option<String>,
    pub externals: ExternalCommands,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            tab_size: 4,
            indent_unit: "  ".into(),
            default_register: '"',
            comment_tokens: CommentTokens::markdown(),
            continue_markup: true,
            extensions: true,
            app_commands: true,
            themes: Vec::new(),
            theme: None,
            externals: ExternalCommands::default(),
        }
    }
}

impl Config {
    /// Exactly what Fuchico's CodeMirror setup gives codemirror-helix (no
    /// extensions, no markup continuation).
    pub fn parity() -> Self {
        Config {
            continue_markup: false,
            extensions: false,
            ..Config::default()
        }
    }

    pub fn indent_unit_cols(&self) -> usize {
        count_column(&self.indent_unit, self.tab_size, usize::MAX).max(1)
    }

    pub fn indent_is_tab(&self) -> bool {
        self.indent_unit.starts_with('\t')
    }
}
