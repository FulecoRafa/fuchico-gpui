//! Mode state (cm-helix `modeField`).

/// The major mode, as reported to the status line (`NOR`/`INS`/`SEL`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ModeType {
    Normal,
    Insert,
    Select,
}

impl ModeType {
    /// cm-helix `toExternalMode`.
    pub fn label(&self) -> &'static str {
        match self {
            ModeType::Normal => "NOR",
            ModeType::Insert => "INS",
            ModeType::Select => "SEL",
        }
    }
}

/// Pending "minor" keymap.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Minor {
    Normal,
    Goto,
    Match,
    Space,
    LeftBracket,
    RightBracket,
    /// `z` view mode (extension, not in cm-helix).
    View,
}

impl Minor {
    /// cm-helix `minorModeStr`.
    pub fn status(&self) -> &'static str {
        match self {
            Minor::Normal => "",
            Minor::Goto => "g",
            Minor::Match => "m",
            Minor::Space => "<space>",
            Minor::LeftBracket => "[",
            Minor::RightBracket => "]",
            Minor::View => "z",
        }
    }
}

/// What the next typed character will be used for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Expecting {
    /// `f` / `F` / `t` / `T`.
    Find { inclusive: bool, forward: bool },
    /// `r<char>`.
    Replace,
    /// `ms<char>`.
    Surround,
    /// `ma<char>` (`true`) / `mi<char>` (`false`).
    Delimiters { inclusive: bool },
    /// `"<reg>`.
    Register,
    /// Insert-mode `Ctrl-r <reg>`.
    InsertRegister,
    /// `md<char>` (extension).
    DeleteSurround,
    /// `mr<from><to>` (extension); `Some(c)` once the first char was typed.
    ReplaceSurround(Option<char>),
}

impl Expecting {
    /// The status text cm-helix shows for the pending key.
    pub fn status(&self) -> &'static str {
        match self {
            Expecting::Find {
                inclusive: true,
                forward: true,
            } => "f",
            Expecting::Find {
                inclusive: true,
                forward: false,
            } => "F",
            Expecting::Find {
                inclusive: false,
                forward: true,
            } => "t",
            Expecting::Find {
                inclusive: false,
                forward: false,
            } => "T",
            Expecting::Replace => "r",
            Expecting::Surround => "s",
            Expecting::Delimiters { inclusive: true } => "a",
            Expecting::Delimiters { inclusive: false } => "i",
            Expecting::Register => "\"",
            Expecting::InsertRegister => "<C-r>",
            Expecting::DeleteSurround => "d",
            Expecting::ReplaceSurround(_) => "r",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Mode {
    pub ty: ModeType,
    /// `None` in insert mode (cm-helix `MODE_EFF.INSERT` has no minor).
    pub minor: Option<Minor>,
    pub count: Option<usize>,
    pub register: Option<char>,
    pub expecting: Option<Expecting>,
}

impl Default for Mode {
    fn default() -> Self {
        Mode::NORMAL
    }
}

impl Mode {
    pub const NORMAL: Mode = Mode {
        ty: ModeType::Normal,
        minor: Some(Minor::Normal),
        count: None,
        register: None,
        expecting: None,
    };
    pub const SELECT: Mode = Mode {
        ty: ModeType::Select,
        minor: Some(Minor::Normal),
        count: None,
        register: None,
        expecting: None,
    };
    pub const INSERT: Mode = Mode {
        ty: ModeType::Insert,
        minor: None,
        count: None,
        register: None,
        expecting: None,
    };

    /// `MODE_EFF.NORMAL` or `MODE_EFF.SELECT` depending on the current type.
    pub fn reset_for(ty: ModeType) -> Mode {
        if ty == ModeType::Select {
            Mode::SELECT
        } else {
            Mode::NORMAL
        }
    }

    /// cm-helix `cmdCount`.
    pub fn count(&self) -> usize {
        self.count.unwrap_or(1)
    }

    /// cm-helix `resetCount`.
    pub fn without_count(&self) -> Mode {
        Mode {
            count: None,
            ..*self
        }
    }

    /// cm-helix `resetMode` (keeps type and minor only).
    pub fn reset(&self) -> Mode {
        Mode {
            ty: self.ty,
            minor: self.minor,
            count: None,
            register: None,
            expecting: None,
        }
    }

    pub fn is_plain_normal(&self) -> bool {
        self.ty == ModeType::Normal
            && self.minor == Some(Minor::Normal)
            && self.expecting.is_none()
            && self.register.is_none()
            && self.count.is_none()
    }

    /// cm-helix `modeStatus` (e.g. `"3g"`, `"mi"`, `"f"`).
    pub fn status(&self) -> String {
        let mut out = String::new();
        if self.ty != ModeType::Insert {
            if let Some(c) = self.count {
                out.push_str(&c.to_string());
            }
            if let Some(m) = self.minor {
                out.push_str(m.status());
            }
        }
        if let Some(e) = self.expecting {
            out.push_str(e.status());
        }
        out
    }
}
