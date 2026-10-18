//! Shortcuts (spec 5 §2): the CodeMirror-style binding format used in
//! settings (`Mod-Shift-p`), its conversion to GPUI keystrokes, the fixed
//! shortcut table, the rebindable action list, and the global key bindings.

use gpui_kit::{App, KeyBinding, Keystroke};

use crate::actions::*;
use crate::i18n::Msg;

pub const IS_MAC: bool = cfg!(target_os = "macos");

/// `format_binding` for the current platform.
pub fn display_binding(binding: &str) -> String {
    fuchico_core::shortcuts::format_binding(binding, IS_MAC)
}

/// A rebindable editor action (`SHORTCUT_ACTIONS`).
#[derive(Clone, Copy, Debug)]
pub struct ShortcutAction {
    pub id: &'static str,
    pub label: Msg,
    pub desc: Msg,
}

pub const SHORTCUT_ACTIONS: [ShortcutAction; 9] = [
    ShortcutAction { id: "openOutline", label: Msg::ShortcutActionOpenOutlineLabel, desc: Msg::ShortcutActionOpenOutlineDesc },
    ShortcutAction { id: "toggleCheckboxAtCursor", label: Msg::ShortcutActionToggleCheckboxAtCursorLabel, desc: Msg::ShortcutActionToggleCheckboxAtCursorDesc },
    ShortcutAction { id: "insertDate", label: Msg::ShortcutActionInsertDateLabel, desc: Msg::ShortcutActionInsertDateDesc },
    ShortcutAction { id: "insertDateTime", label: Msg::ShortcutActionInsertDateTimeLabel, desc: Msg::ShortcutActionInsertDateTimeDesc },
    ShortcutAction { id: "insertRegion", label: Msg::ShortcutActionInsertRegionLabel, desc: Msg::ShortcutActionInsertRegionDesc },
    ShortcutAction { id: "insertTable", label: Msg::ShortcutActionInsertTableLabel, desc: Msg::ShortcutActionInsertTableDesc },
    ShortcutAction { id: "toggleTaskLine", label: Msg::ShortcutActionToggleTaskLineLabel, desc: Msg::ShortcutActionToggleTaskLineDesc },
    ShortcutAction { id: "pickDueDate", label: Msg::ShortcutActionPickDueDateLabel, desc: Msg::ShortcutActionPickDueDateDesc },
    ShortcutAction { id: "pickRecurrence", label: Msg::ShortcutActionPickRecurrenceLabel, desc: Msg::ShortcutActionPickRecurrenceDesc },
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShortcutGroup {
    Global,
    Editor,
    Explorer,
    Tabs,
}

impl ShortcutGroup {
    pub fn label(self) -> Msg {
        match self {
            ShortcutGroup::Global => Msg::ShortcutsGroupGlobal,
            ShortcutGroup::Editor => Msg::ShortcutsGroupEditor,
            ShortcutGroup::Explorer => Msg::ShortcutsGroupExplorer,
            ShortcutGroup::Tabs => Msg::ShortcutsGroupTabs,
        }
    }
}

/// A non-rebindable shortcut row (`FIXED_SHORTCUTS`).
#[derive(Clone, Copy, Debug)]
pub struct FixedShortcut {
    pub group: ShortcutGroup,
    pub label: Msg,
    pub desc: Msg,
    /// Raw binding text, formatted with [`display_binding`].
    pub keys: &'static str,
    /// When set, rendered translated instead of `keys`.
    pub keys_msg: Option<Msg>,
}

impl FixedShortcut {
    pub fn keys_text(&self) -> String {
        match self.keys_msg {
            Some(m) => crate::i18n::tr(m, &[]),
            None => display_binding(self.keys),
        }
    }
}

macro_rules! fixed {
    ($g:ident, $l:ident, $d:ident, $k:expr) => {
        FixedShortcut { group: ShortcutGroup::$g, label: Msg::$l, desc: Msg::$d, keys: $k, keys_msg: None }
    };
    ($g:ident, $l:ident, $d:ident, $k:expr, $km:ident) => {
        FixedShortcut { group: ShortcutGroup::$g, label: Msg::$l, desc: Msg::$d, keys: $k, keys_msg: Some(Msg::$km) }
    };
}

pub const FIXED_SHORTCUTS: [FixedShortcut; 17] = [
    fixed!(Global, ShortcutsFixedCommandPaletteLabel, ShortcutsFixedCommandPaletteDesc, "Mod-Shift-p"),
    fixed!(Global, ShortcutsFixedQuickOpenLabel, ShortcutsFixedQuickOpenDesc, "Mod-p"),
    fixed!(Global, ShortcutsFixedEditorFontSizeLabel, ShortcutsFixedEditorFontSizeDesc, "Mod-= / Mod-- / Mod-0"),
    fixed!(Global, ShortcutsFixedUiZoomLabel, ShortcutsFixedUiZoomDesc, "Mod-Shift-= / Mod-Shift-- / Mod-Shift-0"),
    fixed!(Editor, ShortcutsFixedHelixPaletteLabel, ShortcutsFixedHelixPaletteDesc, ":"),
    fixed!(Editor, ShortcutsFixedSaveLabel, ShortcutsFixedSaveDesc, "Mod-s"),
    fixed!(Editor, ShortcutsFixedFindReplaceLabel, ShortcutsFixedFindReplaceDesc, "Mod-f"),
    fixed!(Editor, ShortcutsFixedTaskAutocompleteLabel, ShortcutsFixedTaskAutocompleteDesc, "@"),
    fixed!(Editor, ShortcutsFixedPasteImageLabel, ShortcutsFixedPasteImageDesc, "Mod-v"),
    fixed!(Editor, ShortcutsFixedFollowLinkLabel, ShortcutsFixedFollowLinkDesc, "Click", ShortcutsFixedFollowLinkKeys),
    fixed!(Explorer, ShortcutsFixedExplorerNavigateLabel, ShortcutsFixedExplorerNavigateDesc, "↑ ↓ ← →"),
    fixed!(Explorer, ShortcutsFixedExplorerOpenLabel, ShortcutsFixedExplorerOpenDesc, "Enter"),
    fixed!(Explorer, ShortcutsFixedExplorerRenameLabel, ShortcutsFixedExplorerRenameDesc, "F2"),
    fixed!(Explorer, ShortcutsFixedExplorerDeleteLabel, ShortcutsFixedExplorerDeleteDesc, "Delete / Backspace"),
    fixed!(Explorer, ShortcutsFixedExplorerTypeAheadLabel, ShortcutsFixedExplorerTypeAheadDesc, "a–z"),
    fixed!(Explorer, ShortcutsFixedExplorerMenuLabel, ShortcutsFixedExplorerMenuDesc, "Right-click", ShortcutsFixedExplorerMenuKeys),
    fixed!(Tabs, ShortcutsFixedTabMenuLabel, ShortcutsFixedTabMenuDesc, "Right-click", ShortcutsFixedTabMenuKeys),
];

/// Named keys: binding (`KeyboardEvent.key`) ↔ GPUI key name.
const NAMED_KEYS: [(&str, &str); 18] = [
    ("Enter", "enter"),
    ("Escape", "escape"),
    ("Backspace", "backspace"),
    ("Delete", "delete"),
    ("Tab", "tab"),
    ("ArrowUp", "up"),
    ("ArrowDown", "down"),
    ("ArrowLeft", "left"),
    ("ArrowRight", "right"),
    ("Home", "home"),
    ("End", "end"),
    ("PageUp", "pageup"),
    ("PageDown", "pagedown"),
    ("Insert", "insert"),
    (" ", "space"),
    ("Space", "space"),
    ("ContextMenu", "menu"),
    ("Help", "help"),
];

/// US-layout shifted symbols: what `Shift+<base>` types.
const SHIFTED: [(char, char); 21] = [
    ('`', '~'),
    ('1', '!'),
    ('2', '@'),
    ('3', '#'),
    ('4', '$'),
    ('5', '%'),
    ('6', '^'),
    ('7', '&'),
    ('8', '*'),
    ('9', '('),
    ('0', ')'),
    ('-', '_'),
    ('=', '+'),
    ('[', '{'),
    (']', '}'),
    ('\\', '|'),
    (';', ':'),
    ('\'', '"'),
    (',', '<'),
    ('.', '>'),
    ('/', '?'),
];

/// Parsed binding: modifiers + key in binding spelling.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Binding {
    pub mod_: bool,
    pub meta: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub key: String,
}

impl Binding {
    /// Parses one chunk (no ` / ` alternatives). `Mod--` = Mod + `-`.
    pub fn parse(s: &str) -> Option<Binding> {
        let mut b = Binding::default();
        let mut rest = s;
        while let Some((head, tail)) = rest.split_once('-') {
            if tail.is_empty() {
                break;
            }
            match head {
                "Mod" => b.mod_ = true,
                "Meta" | "Cmd" => b.meta = true,
                "Ctrl" | "Control" => b.ctrl = true,
                "Alt" => b.alt = true,
                "Shift" => b.shift = true,
                _ => break,
            }
            rest = tail;
        }
        if rest.is_empty() {
            return None;
        }
        b.key = rest.to_string();
        Some(b)
    }

    /// Canonical `Mod-Alt-Shift-key` text.
    pub fn to_binding_string(&self) -> String {
        let mut parts: Vec<&str> = Vec::new();
        if self.mod_ {
            parts.push("Mod");
        }
        if self.meta {
            parts.push("Meta");
        }
        if self.ctrl {
            parts.push("Ctrl");
        }
        if self.alt {
            parts.push("Alt");
        }
        if self.shift {
            parts.push("Shift");
        }
        parts.push(&self.key);
        parts.join("-")
    }

    /// GPUI keystroke strings matching this binding. Several when the
    /// platform reports Shift+symbol as the shifted character (macOS sends
    /// `cmd-shift-.` as `cmd->`).
    pub fn to_gpui_keystrokes(&self, is_mac: bool) -> Vec<String> {
        let key = gpui_key_name(&self.key);
        let mut mods = Vec::new();
        let (cmd, ctrl) = match (self.mod_, is_mac) {
            (true, true) => (true, self.ctrl),
            (true, false) => (self.meta, true),
            (false, _) => (self.meta, self.ctrl),
        };
        if ctrl {
            mods.push("ctrl");
        }
        if self.alt {
            mods.push("alt");
        }
        if cmd {
            mods.push(if is_mac { "cmd" } else { "super" });
        }
        let join = |mods: &[&str], shift: bool, key: &str| {
            let mut v: Vec<&str> = mods.to_vec();
            if shift {
                v.push("shift");
            }
            v.push(key);
            v.join("-")
        };
        let mut out = vec![join(&mods, self.shift, &key)];
        if self.shift
            && let Some(shifted) = single_char(&key).and_then(shifted_of)
        {
            out.push(join(&mods, false, &shifted.to_string()));
        }
        out
    }
}

fn single_char(s: &str) -> Option<char> {
    let mut it = s.chars();
    let c = it.next()?;
    it.next().is_none().then_some(c)
}

fn shifted_of(c: char) -> Option<char> {
    SHIFTED.iter().find(|(b, _)| *b == c).map(|(_, s)| *s)
}

fn base_of(c: char) -> Option<char> {
    SHIFTED.iter().find(|(_, s)| *s == c).map(|(b, _)| *b)
}

fn gpui_key_name(key: &str) -> String {
    if let Some((_, g)) = NAMED_KEYS.iter().find(|(b, _)| *b == key) {
        return g.to_string();
    }
    if key.len() > 1 && key.starts_with('F') && key[1..].chars().all(|c| c.is_ascii_digit()) {
        return key.to_ascii_lowercase();
    }
    key.to_lowercase()
}

fn binding_key_name(gpui_key: &str) -> String {
    if let Some((b, _)) = NAMED_KEYS.iter().find(|(_, g)| *g == gpui_key) {
        return b.to_string();
    }
    if gpui_key.len() > 1 && gpui_key.starts_with('f') && gpui_key[1..].chars().all(|c| c.is_ascii_digit()) {
        return gpui_key.to_ascii_uppercase();
    }
    if single_char(gpui_key).is_some() {
        return gpui_key.to_lowercase();
    }
    gpui_key.to_string()
}

/// Converts a whole binding (with ` / ` alternatives) to GPUI keystrokes.
pub fn binding_to_keystrokes(binding: &str) -> Vec<String> {
    binding
        .split(" / ")
        .filter_map(Binding::parse)
        .flat_map(|b| b.to_gpui_keystrokes(IS_MAC))
        .collect()
}

/// `bindingFromEvent` for the rebinding UI. `None` for modifier-only presses.
/// Shifted symbols are normalized back to `Shift-<base>` (US layout) so a
/// recorded binding matches the defaults (`Mod-Shift-.`).
pub fn binding_from_keystroke(ks: &Keystroke) -> Option<String> {
    let key = ks.key.as_str();
    if key.is_empty() || matches!(key, "shift" | "control" | "alt" | "platform" | "cmd" | "function" | "fn" | "capslock") {
        return None;
    }
    let m = &ks.modifiers;
    let mut b = Binding {
        mod_: m.control || m.platform,
        alt: m.alt,
        shift: m.shift,
        key: binding_key_name(key),
        ..Default::default()
    };
    if let Some(c) = single_char(&b.key) {
        if let Some(base) = base_of(c) {
            b.shift = true;
            b.key = base.to_string();
        } else if c.is_ascii_uppercase() {
            b.shift = true;
            b.key = c.to_ascii_lowercase().to_string();
        }
    }
    Some(b.to_binding_string())
}

/// Window-level keys bound once at startup. Context-free bindings apply in
/// every window; `Workspace` ones only in the main window.
pub fn bind_global_keys(cx: &mut App) {
    let m = if IS_MAC { "cmd" } else { "ctrl" };
    let k = |s: &str| s.replace("MOD", m);
    let mut bindings = vec![
        // Editor font size: Mod = / - / 0 (and numpad).
        KeyBinding::new(&k("MOD-="), EditorZoomIn, None),
        KeyBinding::new(&k("MOD--"), EditorZoomOut, None),
        KeyBinding::new(&k("MOD-0"), EditorZoomReset, None),
        // UI zoom: Mod-Shift = / - / 0. macOS reports these as the shifted
        // characters with Shift dropped, other platforms keep `shift`.
        KeyBinding::new(&k("MOD-shift-="), UiZoomIn, None),
        KeyBinding::new(&k("MOD-shift-+"), UiZoomIn, None),
        KeyBinding::new(&k("MOD-shift--"), UiZoomOut, None),
        KeyBinding::new(&k("MOD-shift-_"), UiZoomOut, None),
        KeyBinding::new(&k("MOD-shift-0"), UiZoomReset, None),
        KeyBinding::new(&k("MOD-shift-)"), UiZoomReset, None),
        KeyBinding::new(&k("MOD-s"), Save, None),
        // Parity: the shell binds no tab keys (01-shell "Global keyboard").
        // Mod-O / Mod-, are menu accelerators added by the port.
        KeyBinding::new(&k("MOD-o"), OpenFolder, Some("Workspace")),
        KeyBinding::new(&k("MOD-shift-p"), CommandPalette, Some("Workspace")),
        KeyBinding::new(&k("MOD-p"), QuickOpen, Some("Workspace")),
        KeyBinding::new(&k("MOD-,"), ShowSettings, Some("Workspace")),
    ];
    if IS_MAC {
        bindings.extend([
            KeyBinding::new("cmd-+", UiZoomIn, None),
            KeyBinding::new("cmd-_", UiZoomOut, None),
            KeyBinding::new("cmd-)", UiZoomReset, None),
            KeyBinding::new("cmd-q", Quit, None),
            KeyBinding::new("cmd-h", Hide, None),
            KeyBinding::new("cmd-alt-h", HideOthers, None),
            KeyBinding::new("cmd-m", Minimize, None),
            KeyBinding::new("cmd-shift-w", CloseWindow, None),
            // The palette listener accepts Ctrl as well as Cmd (spec 4 §1).
            KeyBinding::new("ctrl-shift-p", CommandPalette, Some("Workspace")),
            KeyBinding::new("ctrl-p", QuickOpen, Some("Workspace")),
        ]);
    } else {
        bindings.push(KeyBinding::new("ctrl-+", EditorZoomIn, None));
        bindings.push(KeyBinding::new("ctrl-q", Quit, None));
    }
    cx.bind_keys(bindings);
}

#[cfg(test)]
mod tests {
    use super::*;
    use fuchico_core::shortcuts::format_binding;

    #[test]
    fn parse_and_print() {
        let b = Binding::parse("Mod-Shift-p").unwrap();
        assert!(b.mod_ && b.shift && !b.alt);
        assert_eq!(b.key, "p");
        assert_eq!(Binding::parse("Mod--").unwrap().key, "-");
        assert_eq!(Binding::parse("Mod-Shift--").unwrap().key, "-");
        assert_eq!(Binding::parse("a").unwrap().to_binding_string(), "a");
        assert_eq!(Binding::parse("Shift-Alt-Mod-x").unwrap().to_binding_string(), "Mod-Alt-Shift-x");
        assert_eq!(Binding::parse(""), None);
    }

    #[test]
    fn to_gpui() {
        let ks = |s: &str, mac| Binding::parse(s).unwrap().to_gpui_keystrokes(mac);
        assert_eq!(ks("Mod-Shift-p", true), vec!["cmd-shift-p"]);
        assert_eq!(ks("Mod-Shift-p", false), vec!["ctrl-shift-p"]);
        assert_eq!(ks("Mod-Enter", true), vec!["cmd-enter"]);
        assert_eq!(ks("Alt-ArrowUp", true), vec!["alt-up"]);
        assert_eq!(ks("Mod-Shift-.", true), vec!["cmd-shift-.", "cmd->"]);
        assert_eq!(ks("Mod-Alt-d", false), vec!["ctrl-alt-d"]);
        assert_eq!(ks("F2", true), vec!["f2"]);
        assert_eq!(binding_to_keystrokes("Mod-= / Mod--"), {
            let m = if IS_MAC { "cmd" } else { "ctrl" };
            vec![format!("{m}-="), format!("{m}--")]
        });
        // Every generated keystroke parses in GPUI.
        for a in SHORTCUT_ACTIONS {
            let def = crate::settings::Settings::default();
            for s in binding_to_keystrokes(def.shortcut(a.id)) {
                Keystroke::parse(&s).unwrap_or_else(|_| panic!("{s}"));
            }
        }
    }

    #[test]
    fn from_keystroke() {
        let f = |s: &str| binding_from_keystroke(&Keystroke::parse(s).unwrap());
        assert_eq!(f("cmd-shift-p").as_deref(), Some("Mod-Shift-p"));
        assert_eq!(f("ctrl-alt-d").as_deref(), Some("Mod-Alt-d"));
        assert_eq!(f("cmd->").as_deref(), Some("Mod-Shift-."));
        assert_eq!(f("cmd-enter").as_deref(), Some("Mod-Enter"));
        assert_eq!(f("alt-up").as_deref(), Some("Alt-ArrowUp"));
        assert_eq!(f("f2").as_deref(), Some("F2"));
        assert_eq!(f("a").as_deref(), Some("a"));
    }

    #[test]
    fn display_matches_original_tests() {
        assert_eq!(format_binding("Mod-Shift-p", true), "⌘⇧P");
        assert_eq!(format_binding("Mod-Shift-p", false), "Ctrl+Shift+P");
        assert_eq!(format_binding("Mod-s", true), "⌘S");
        assert_eq!(format_binding("Mod-Shift-Enter", true), "⌘⇧Enter");
        assert_eq!(format_binding("Alt-ArrowUp", true), "⌥ArrowUp");
        assert_eq!(format_binding("Mod-.", false), "Ctrl+.");
        assert_eq!(format_binding("Mod--", true), "⌘-");
        assert_eq!(format_binding("Mod-Shift--", false), "Ctrl+Shift+-");
        for s in ["F2", "Right-click", "Delete / Backspace"] {
            assert_eq!(format_binding(s, true), s);
        }
        assert_eq!(format_binding("Mod-= / Mod-- / Mod-0", false), "Ctrl+= / Ctrl+- / Ctrl+0");
    }

    #[test]
    fn tables_are_complete() {
        assert_eq!(SHORTCUT_ACTIONS.len(), crate::settings::DEFAULT_SHORTCUTS.len());
        for (a, (id, _)) in SHORTCUT_ACTIONS.iter().zip(crate::settings::DEFAULT_SHORTCUTS) {
            assert_eq!(a.id, id);
        }
        let groups: Vec<_> = FIXED_SHORTCUTS.iter().map(|f| f.group).collect();
        assert_eq!(groups.iter().filter(|g| **g == ShortcutGroup::Editor).count(), 6);
    }
}
