//! Registers (cm-helix `registersField` + `registersHistoryField`).

use std::collections::BTreeMap;

/// Register store. Clone it to share registers between editors (cm-helix
/// `globalInit`/`snapshot`), see [`crate::Editor::registers`] and
/// [`crate::Editor::set_registers`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Registers {
    values: BTreeMap<char, Vec<String>>,
    history: BTreeMap<char, Vec<String>>,
}

/// Registers that cannot be written (`_` black hole, `#` indices,
/// `.` selection contents, `%` file path).
pub fn is_read_only(reg: char) -> bool {
    matches!(reg, '_' | '%' | '.' | '#')
}

impl Registers {
    pub fn new() -> Self {
        Self::default()
    }

    /// Raw stored value (no special registers, no clipboard).
    pub fn get(&self, reg: char) -> Option<&Vec<String>> {
        self.values.get(&reg)
    }

    /// History of a register (`:` and `/` only), oldest first.
    pub fn history(&self, reg: char) -> &[String] {
        self.history.get(&reg).map(Vec::as_slice).unwrap_or(&[])
    }

    pub fn names(&self) -> Vec<char> {
        self.values.keys().copied().collect()
    }

    /// Apply a yank. Returns the text to write to the system clipboard when
    /// the register is `+`.
    pub fn write(&mut self, reg: char, value: Vec<String>) -> Option<String> {
        if is_read_only(reg) {
            return None;
        }
        let mut value = value;
        let mut clipboard = None;
        match reg {
            ':' => value.truncate(1),
            '+' => clipboard = Some(value.join("\n")),
            _ => {}
        }
        if matches!(reg, ':' | '/')
            && let Some(first) = value.first()
        {
            self.history.entry(reg).or_default().push(first.clone());
        }
        if value.is_empty() {
            self.values.remove(&reg);
        } else {
            self.values.insert(reg, value);
        }
        clipboard
    }
}
