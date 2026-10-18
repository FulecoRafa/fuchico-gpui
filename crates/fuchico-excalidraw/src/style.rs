//! Theme for the editor chrome. The canvas itself follows Excalidraw's
//! rules (dark mode = filtered canvas colours); this only styles islands.

use gpui_kit::{rgb, Hsla};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ExcalidrawStyle {
    /// Dark canvas (Excalidraw's `theme: "dark"`).
    pub dark: bool,
    pub island_bg: Hsla,
    pub island_border: Hsla,
    pub fg: Hsla,
    pub muted: Hsla,
    pub active_bg: Hsla,
    pub active_fg: Hsla,
    pub hover_bg: Hsla,
    /// Selection outline / handles.
    pub selection: Hsla,
}

impl ExcalidrawStyle {
    pub fn light() -> Self {
        Self {
            dark: false,
            island_bg: rgb(0xffffff).into(),
            island_border: rgb(0xe9e9ef).into(),
            fg: rgb(0x1b1b1f).into(),
            muted: rgb(0x8f8f99).into(),
            active_bg: rgb(0xe0dfff).into(),
            active_fg: rgb(0x030064).into(),
            hover_bg: rgb(0xf1f0ff).into(),
            selection: rgb(0x6965db).into(),
        }
    }

    pub fn dark() -> Self {
        Self {
            dark: true,
            island_bg: rgb(0x232329).into(),
            island_border: rgb(0x2e2d39).into(),
            fg: rgb(0xe3e3e8).into(),
            muted: rgb(0x8f8f99).into(),
            active_bg: rgb(0x403e6a).into(),
            active_fg: rgb(0xe0dfff).into(),
            hover_bg: rgb(0x2e2d39).into(),
            selection: rgb(0xa8a5ff).into(),
        }
    }
}

impl Default for ExcalidrawStyle {
    fn default() -> Self {
        Self::light()
    }
}
