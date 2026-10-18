//! Asset source: gpui-component's default icon bundle plus the extra Lucide
//! icons the shell uses, and the bundled UI/editor fonts.

use std::borrow::Cow;

use gpui_kit::{App, AssetSource, Result, SharedString};

/// The full shared Lucide catalog; only the names listed in
/// [`ExtraIcons`] (plus the component defaults) are embedded.
pub use gpui_kit::assets::IconName as AppIcon;

gpui_kit::assets::icon_assets!(
    pub ExtraIcons,
    [
        AppWindow, CalendarClock, Files, FilePlus, FolderPlus, Hash, PenTool, Pencil, Scissors, Trash, X,
        Link, Link2, Workflow, Download, Upload, Keyboard, Languages, Type, ListChecks, Clock, CircleDot,
    ]
);

pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if let Some(b) = ExtraIcons.load(path)? {
            return Ok(Some(b));
        }
        gpui_kit::assets::Assets.load(path)
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut v = gpui_kit::assets::Assets.list(path)?;
        v.extend(ExtraIcons.list(path)?);
        v.sort();
        v.dedup();
        Ok(v)
    }
}

/// Inter (UI) and JetBrains Mono (editor), the original's defaults.
const FONTS: &[&[u8]] = &[
    include_bytes!("../assets/fonts/Inter-Regular.ttf"),
    include_bytes!("../assets/fonts/Inter-Medium.ttf"),
    include_bytes!("../assets/fonts/Inter-SemiBold.ttf"),
    include_bytes!("../assets/fonts/Inter-Bold.ttf"),
    include_bytes!("../assets/fonts/Inter-Italic.ttf"),
    include_bytes!("../assets/fonts/JetBrainsMono-Regular.ttf"),
    include_bytes!("../assets/fonts/JetBrainsMono-Bold.ttf"),
    include_bytes!("../assets/fonts/JetBrainsMono-Italic.ttf"),
    include_bytes!("../assets/fonts/JetBrainsMono-BoldItalic.ttf"),
];

pub fn register_fonts(cx: &mut App) {
    let fonts = FONTS.iter().map(|b| Cow::Borrowed(*b)).collect();
    if let Err(e) = cx.text_system().add_fonts(fonts) {
        log::warn!("font registration failed: {e}");
    }
}
