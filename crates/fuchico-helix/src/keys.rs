//! Key events.
//!
//! Printable keys are [`Key::Char`] carrying the character the keyboard
//! produced (`'A'` for shift+a, `':'` for shift+;). Shift is ignored for
//! chars, exactly like CodeMirror's key names. For Alt/Ctrl chords send the
//! *base* character (`Alt` + `'s'`, not the macOS `'ß'`).

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Key {
    Char(char),
    Escape,
    Enter,
    Backspace,
    Delete,
    Tab,
    ArrowUp,
    ArrowDown,
    ArrowLeft,
    ArrowRight,
    Home,
    End,
    PageUp,
    PageDown,
    F(u8),
    /// Anything else, by name.
    Other(String),
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct Modifiers {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub meta: bool,
}

impl Modifiers {
    pub const NONE: Modifiers = Modifiers {
        ctrl: false,
        alt: false,
        shift: false,
        meta: false,
    };
    pub fn ctrl() -> Self {
        Modifiers {
            ctrl: true,
            ..Self::NONE
        }
    }
    pub fn alt() -> Self {
        Modifiers {
            alt: true,
            ..Self::NONE
        }
    }
    pub fn shift() -> Self {
        Modifiers {
            shift: true,
            ..Self::NONE
        }
    }
    pub fn is_none(&self) -> bool {
        !self.ctrl && !self.alt && !self.meta
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct KeyEvent {
    pub key: Key,
    pub modifiers: Modifiers,
}

impl KeyEvent {
    pub fn new(key: Key, modifiers: Modifiers) -> Self {
        KeyEvent { key, modifiers }
    }
    pub fn char(c: char) -> Self {
        KeyEvent::new(Key::Char(c), Modifiers::NONE)
    }
    pub fn key(key: Key) -> Self {
        KeyEvent::new(key, Modifiers::NONE)
    }

    /// Parse a CodeMirror-style key name: `"x"`, `"Space"`, `"Escape"`,
    /// `"Alt-s"`, `"Ctrl-a"`, `"Shift-Tab"`, `"Alt-ArrowUp"`.
    pub fn parse(name: &str) -> Option<KeyEvent> {
        let mut mods = Modifiers::NONE;
        let mut rest = name;
        loop {
            // A trailing "-" is the minus key itself.
            if rest.len() > 1 {
                if let Some(r) = rest
                    .strip_prefix("Ctrl-")
                    .or_else(|| rest.strip_prefix("C-"))
                {
                    mods.ctrl = true;
                    rest = r;
                    continue;
                }
                if let Some(r) = rest
                    .strip_prefix("Alt-")
                    .or_else(|| rest.strip_prefix("A-"))
                {
                    mods.alt = true;
                    rest = r;
                    continue;
                }
                if let Some(r) = rest
                    .strip_prefix("Shift-")
                    .or_else(|| rest.strip_prefix("S-"))
                {
                    mods.shift = true;
                    rest = r;
                    continue;
                }
                if let Some(r) = rest
                    .strip_prefix("Meta-")
                    .or_else(|| rest.strip_prefix("Cmd-"))
                    .or_else(|| rest.strip_prefix("Mod-"))
                {
                    mods.meta = true;
                    rest = r;
                    continue;
                }
            }
            break;
        }
        let key = match rest {
            "Space" => Key::Char(' '),
            "Escape" | "Esc" => Key::Escape,
            "Enter" | "Return" => Key::Enter,
            "Backspace" => Key::Backspace,
            "Delete" => Key::Delete,
            "Tab" => Key::Tab,
            "ArrowUp" | "Up" => Key::ArrowUp,
            "ArrowDown" | "Down" => Key::ArrowDown,
            "ArrowLeft" | "Left" => Key::ArrowLeft,
            "ArrowRight" | "Right" => Key::ArrowRight,
            "Home" => Key::Home,
            "End" => Key::End,
            "PageUp" => Key::PageUp,
            "PageDown" => Key::PageDown,
            s if s.chars().count() == 1 => Key::Char(s.chars().next()?),
            s if s.starts_with('F') && s[1..].parse::<u8>().is_ok() => Key::F(s[1..].parse().ok()?),
            "" => return None,
            s => Key::Other(s.to_string()),
        };
        Some(KeyEvent::new(key, mods))
    }

    /// Canonical CodeMirror-style name used for keymap lookup
    /// (`Ctrl-`, `Alt-`, `Meta-`, `Shift-` prefixes, shift dropped for chars).
    pub fn name(&self) -> String {
        let base = match &self.key {
            Key::Char(' ') => "Space".to_string(),
            Key::Char(c) => c.to_string(),
            Key::Escape => "Escape".into(),
            Key::Enter => "Enter".into(),
            Key::Backspace => "Backspace".into(),
            Key::Delete => "Delete".into(),
            Key::Tab => "Tab".into(),
            Key::ArrowUp => "ArrowUp".into(),
            Key::ArrowDown => "ArrowDown".into(),
            Key::ArrowLeft => "ArrowLeft".into(),
            Key::ArrowRight => "ArrowRight".into(),
            Key::Home => "Home".into(),
            Key::End => "End".into(),
            Key::PageUp => "PageUp".into(),
            Key::PageDown => "PageDown".into(),
            Key::F(n) => format!("F{n}"),
            Key::Other(s) => s.clone(),
        };
        let mut out = String::new();
        if self.modifiers.ctrl {
            out.push_str("Ctrl-");
        }
        if self.modifiers.alt {
            out.push_str("Alt-");
        }
        if self.modifiers.meta {
            out.push_str("Meta-");
        }
        if self.modifiers.shift && !matches!(self.key, Key::Char(_)) {
            out.push_str("Shift-");
        }
        out.push_str(&base);
        out
    }

    /// The typed char for a plain (no ctrl/alt/meta) printable key.
    pub fn plain_char(&self) -> Option<char> {
        match self.key {
            Key::Char(c) if self.modifiers.is_none() => Some(c),
            _ => None,
        }
    }

    /// cm-helix `isChar`: single character key names and `Space`.
    pub fn is_char(&self) -> bool {
        self.plain_char().is_some()
    }
}

impl From<&str> for KeyEvent {
    fn from(s: &str) -> Self {
        KeyEvent::parse(s).unwrap_or_else(|| panic!("invalid key name {s:?}"))
    }
}

impl From<char> for KeyEvent {
    fn from(c: char) -> Self {
        KeyEvent::char(c)
    }
}

/// Parse a sequence of keys written like `"3w<A-s>d<esc>"`: plain chars, or
/// `<...>` groups with a CodeMirror name (`<Escape>`, `<Alt-s>`, `<C-r>`,
/// `<space>`, `<esc>`, `<ret>`, `<lt>`).
pub fn parse_keys(s: &str) -> Vec<KeyEvent> {
    let mut out = Vec::new();
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '<' {
            let mut name = String::new();
            let mut closed = false;
            for n in chars.by_ref() {
                if n == '>' {
                    closed = true;
                    break;
                }
                name.push(n);
            }
            if !closed || name.is_empty() {
                out.push(KeyEvent::char('<'));
                for n in name.chars() {
                    out.push(KeyEvent::char(n));
                }
                continue;
            }
            // split `C-A-x` style modifier prefixes from the base key name
            let last = name.char_indices().last().map_or(0, |(i, _)| i);
            let split = name[..last].rfind('-').map_or(0, |i| i + 1);
            let (prefix, base) = name.split_at(split);
            let lower = base.to_ascii_lowercase();
            let alias = match lower.as_str() {
                "esc" => Some("Escape"),
                "ret" | "cr" | "enter" => Some("Enter"),
                "space" => Some("Space"),
                "lt" => Some("<"),
                "gt" => Some(">"),
                "bs" | "backspace" => Some("Backspace"),
                "del" | "delete" => Some("Delete"),
                "tab" => Some("Tab"),
                "left" => Some("ArrowLeft"),
                "right" => Some("ArrowRight"),
                "up" => Some("ArrowUp"),
                "down" => Some("ArrowDown"),
                "home" => Some("Home"),
                "end" => Some("End"),
                "pageup" => Some("PageUp"),
                "pagedown" => Some("PageDown"),
                _ => None,
            };
            let full = match alias {
                Some(a) => format!("{prefix}{a}"),
                None => name.clone(),
            };
            let ev = KeyEvent::parse(&full).unwrap_or_else(|| panic!("bad key {name}"));
            out.push(ev);
        } else {
            out.push(KeyEvent::char(c));
        }
    }
    out
}
