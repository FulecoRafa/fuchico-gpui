//! Offscreen snapshots for visual checks (test-only).
//!
//! Renders a view through gpui's `HeadlessAppContext` with the real macOS
//! text system and the headless Metal renderer: the window is opened with
//! `focus: false, show: false`, so no NSWindow ever appears and nothing
//! takes focus. The frame is written as a PNG under `target/snapshots/`.
//!
//! ```ignore
//! let png = snapshot("agenda-ayu-light", size(px(1280.), px(800.)), SnapTheme::AYU_LIGHT, |window, cx| {
//!     cx.new(|cx| MyView::new(window, cx))
//! });
//! ```

use std::path::PathBuf;
use std::sync::Arc;

use gpui_kit::{App, AppContext, Entity, HeadlessAppContext, Pixels, Render, Size, Window};

use crate::settings::{ModePref, PaletteId, SettingsStore};

/// Palette + light/dark mode to render with.
#[derive(Clone, Copy, Debug)]
pub struct SnapTheme {
    pub palette: PaletteId,
    pub mode: ModePref,
}

impl SnapTheme {
    pub const AYU_LIGHT: SnapTheme = SnapTheme { palette: PaletteId::Ayu, mode: ModePref::Light };
    pub const AYU_DARK: SnapTheme = SnapTheme { palette: PaletteId::Ayu, mode: ModePref::Dark };
    pub const DRACULA: SnapTheme = SnapTheme { palette: PaletteId::Dracula, mode: ModePref::Dark };

    /// The default palette in light or dark mode.
    pub fn default_mode(dark: bool) -> SnapTheme {
        SnapTheme { palette: PaletteId::default(), mode: if dark { ModePref::Dark } else { ModePref::Light } }
    }
}

/// The real platform text system. On macOS it is built directly because
/// `MacPlatform::new` (behind `current_platform`) insists on the main
/// thread, and libtest runs tests on worker threads.
fn text_system() -> Arc<dyn gpui_kit::PlatformTextSystem> {
    #[cfg(target_os = "macos")]
    {
        Arc::new(gpui_apple::AppleTextSystem::new())
    }
    #[cfg(not(target_os = "macos"))]
    {
        gpui_platform::current_platform(true).text_system()
    }
}

/// `<workspace>/target/snapshots`.
pub fn snapshot_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/snapshots")
}

/// Points `persist` at a scratch config dir before anything reads it, so
/// snapshots never touch (or depend on) the user's real settings.
fn isolate_config() {
    let dir = snapshot_dir().join(".config");
    let _ = std::fs::create_dir_all(&dir);
    crate::persist::set_test_dir(dir);
}

/// App globals the views expect (same order as `main`), with `theme`
/// applied through the settings store like a user choice would be.
pub fn init_app(theme: SnapTheme, cx: &mut App) {
    gpui_kit::init(cx);
    crate::assets::register_fonts(cx);
    SettingsStore::init(SettingsStore::load(), cx);
    SettingsStore::update(cx, |s| {
        s.palette = theme.palette;
        s.mode = theme.mode;
    });
    // Compare against the original's (English) screenshots.
    crate::i18n::set_locale(crate::i18n::Locale::En);
    cx.set_global(crate::settings::ui_state::UiState::load());
    cx.set_global(crate::windows::geometry::WindowStates::load());
    crate::bus::init(cx);
    crate::theme::init(cx);
    crate::keymap::bind_global_keys(cx);
}

/// Renders `build`'s view (wrapped in gpui-kit's `Root`, like a real
/// window) offscreen at `size` and saves `target/snapshots/<name>.png`.
/// `prepare` runs after the first layout settles (e.g. to switch views);
/// pending tasks (scans, …) are drained before capturing.
pub fn snapshot_with<V: Render>(
    name: &str,
    size: Size<Pixels>,
    theme: SnapTheme,
    build: impl FnOnce(&mut Window, &mut App) -> Entity<V>,
    prepare: impl FnOnce(&Entity<V>, &mut Window, &mut App),
) -> PathBuf {
    isolate_config();
    let mut cx = HeadlessAppContext::with_platform(text_system(), Arc::new(crate::assets::Assets), || {
        gpui_platform::current_headless_renderer()
    });
    cx.update(|cx| init_app(theme, cx));
    let mut content = None;
    let handle = cx
        .open_window(size, |window, cx| {
            let view = build(window, cx);
            content = Some(view.clone());
            cx.new(|cx| gpui_kit::base::Root::new(view, window, cx))
        })
        .expect("open headless window");
    let view = content.expect("view built");
    cx.run_until_parked();
    cx.update_window(handle.into(), |_, window, cx| prepare(&view, window, cx)).expect("prepare");
    // Background work (file scans) may hop threads; let it land.
    cx.allow_parking();
    for _ in 0..20 {
        cx.run_until_parked();
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    cx.forbid_parking();
    cx.update_window(handle.into(), |_, window, cx| {
        window.refresh();
        window.draw(cx).clear(cx);
    })
    .expect("draw");
    let image = cx.capture_screenshot(handle.into()).expect("headless capture (Metal renderer)");
    let dir = snapshot_dir();
    std::fs::create_dir_all(&dir).expect("create target/snapshots");
    let path = dir.join(format!("{name}.png"));
    image.save(&path).expect("write snapshot png");
    path.canonicalize().unwrap_or(path)
}

/// [`snapshot_with`] without a prepare step.
pub fn snapshot<V: Render>(
    name: &str,
    size: Size<Pixels>,
    theme: SnapTheme,
    build: impl FnOnce(&mut Window, &mut App) -> Entity<V>,
) -> PathBuf {
    snapshot_with(name, size, theme, build, |_, _, _| {})
}

/// Copies `src` (e.g. the original's `demo-notes`) into a fresh scratch
/// dir under `target/snapshots/vaults/<name>` and returns its path.
pub fn copy_vault(src: &std::path::Path, name: &str) -> PathBuf {
    fn copy(from: &std::path::Path, to: &std::path::Path) {
        std::fs::create_dir_all(to).unwrap();
        for entry in std::fs::read_dir(from).unwrap().flatten() {
            let (p, t) = (entry.path(), to.join(entry.file_name()));
            if p.is_dir() {
                copy(&p, &t);
            } else {
                std::fs::copy(&p, &t).unwrap();
            }
        }
    }
    let dst = snapshot_dir().join("vaults").join(name);
    let _ = std::fs::remove_dir_all(&dst);
    copy(src, &dst);
    dst.canonicalize().unwrap_or(dst)
}
