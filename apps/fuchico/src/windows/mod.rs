//! Windows: the main window, single-file editor windows, geometry, the app
//! menu, single-instance forwarding and the OS open-file queue.

pub mod edit_menu;
pub mod editor_window;
pub mod geometry;
pub mod menu;
pub mod single_instance;
#[cfg(test)]
mod ui_tests;

use std::collections::HashMap;
use std::sync::LazyLock;

use fuchico_core::editor_window::editor_window_label;
use fuchico_core::open_files::OpenRequests;
use gpui_kit::{
    AnyWindowHandle, App, AppContext, Global, Point, TitlebarOptions, WindowOptions, px, size,
};

use crate::actions::*;
use crate::settings::{
    EDITOR_FONT_SIZE_DEFAULT, ModePref, PaletteId, SettingsStore, UI_SCALE_DEFAULT, clamp_editor_font_size,
    clamp_ui_scale,
};
use crate::workspace::Workspace;
use editor_window::EditorWindow;

pub const MAIN_LABEL: &str = "main";
pub const MAIN_SIZE: (f32, f32) = (1000., 700.);
pub const EDITOR_SIZE: (f32, f32) = (900., 650.);

/// Paths handed over by the OS / CLI / a second launch, drained by the
/// main workspace once its vault restore finishes.
pub static OPEN_REQUESTS: LazyLock<OpenRequests> = LazyLock::new(OpenRequests::default);

/// Queues `paths` and announces them on the bus (any thread).
pub fn push_open_requests(paths: Vec<String>) {
    let sink = crate::bus::sink();
    if paths.is_empty() {
        // A bare relaunch still brings the main window forward.
        sink.emit(fuchico_core::events::CoreEvent::OpenFiles(Vec::new()));
    } else {
        OPEN_REQUESTS.push(&*sink, paths);
    }
}

/// `file://` URLs from the OS → absolute paths (others dropped).
pub fn paths_from_urls(urls: &[String]) -> Vec<String> {
    urls.iter()
        .filter_map(|u| u.strip_prefix("file://"))
        .map(percent_decode)
        .filter(|p| std::path::Path::new(p).is_file())
        .collect()
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let Ok(b) = u8::from_str_radix(std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or("zz"), 16)
        {
            out.push(b);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Open windows by label.
#[derive(Default)]
pub struct WindowRegistry {
    main: Option<AnyWindowHandle>,
    editors: HashMap<String, AnyWindowHandle>,
}

impl Global for WindowRegistry {}

impl WindowRegistry {
    fn get(cx: &mut App) -> &mut WindowRegistry {
        if !cx.has_global::<WindowRegistry>() {
            cx.set_global(WindowRegistry::default());
        }
        cx.global_mut::<WindowRegistry>()
    }
}

fn options(label: &str, title: &str, default: (f32, f32), cx: &App) -> WindowOptions {
    WindowOptions {
        window_bounds: Some(geometry::initial_bounds(label, default, cx)),
        titlebar: Some(TitlebarOptions {
            title: Some(title.to_string().into()),
            appears_transparent: false,
            traffic_light_position: None::<Point<gpui_kit::Pixels>>,
        }),
        window_min_size: Some(size(px(480.), px(320.))),
        app_id: Some("fuchico".into()),
        focus: !crate::background_mode(),
        ..Default::default()
    }
}

/// Opens (or focuses) the main window.
pub fn open_main_window(cx: &mut App) {
    if let Some(h) = WindowRegistry::get(cx).main
        && h.update(cx, |_, w, _| w.activate_window()).is_ok()
    {
        return;
    }
    let opts = options(MAIN_LABEL, "Fuchico", MAIN_SIZE, cx);
    match gpui_kit::open_window(opts, cx, |window, cx| {
        cx.new(|cx| {
            let mut ws = Workspace::new(window, cx);
            ws.hold(geometry::track(MAIN_LABEL.into(), window, cx));
            ws
        })
    }) {
        Ok((handle, _)) => WindowRegistry::get(cx).main = Some(handle),
        Err(e) => log::error!("open main window: {e}"),
    }
}

/// `openEditorWindow(path, rootPath)`: at most one window per file.
pub fn open_editor_window(path: String, root: Option<String>, cx: &mut App) {
    let label = editor_window_label(&path);
    if let Some(h) = WindowRegistry::get(cx).editors.get(&label).copied() {
        if h.update(cx, |_, w, _| w.activate_window()).is_ok() {
            return;
        }
        WindowRegistry::get(cx).editors.remove(&label);
    }
    let title = crate::workspace::tabs::basename(&path).to_string();
    let opts = options(&label, &title, EDITOR_SIZE, cx);
    let l = label.clone();
    match gpui_kit::open_window(opts, cx, |window, cx| cx.new(|cx| EditorWindow::new(path, root, l, window, cx))) {
        Ok((handle, _)) => {
            WindowRegistry::get(cx).editors.insert(label, handle);
        }
        Err(e) => log::error!("open editor window: {e}"),
    }
}

/// `on_reopen` (dock icon click): show the main window again.
pub fn reopen(cx: &mut App) {
    open_main_window(cx);
}

/// Global (any-window) actions: app, zoom, theme.
pub fn register_global_actions(cx: &mut App) {
    cx.on_window_closed(|cx, id| {
        let reg = WindowRegistry::get(cx);
        if reg.main.is_some_and(|h| h.window_id() == id) {
            reg.main = None;
        }
        reg.editors.retain(|_, h| h.window_id() != id);
    })
    .detach();
    cx.on_app_quit(|cx| {
        geometry::save_now(cx);
        async {
            crate::persist::flush();
            single_instance::release();
        }
    })
    .detach();

    cx.on_action(|_: &Quit, cx| cx.quit());
    cx.on_action(|_: &Hide, cx| cx.hide());
    cx.on_action(|_: &HideOthers, cx| cx.hide_other_apps());
    cx.on_action(|_: &ShowAll, cx| cx.unhide_other_apps());
    cx.on_action(|_: &Minimize, cx| with_active(cx, |w, _| w.minimize_window()));
    cx.on_action(|_: &Zoom, cx| with_active(cx, |w, _| w.zoom_window()));
    cx.on_action(|_: &CloseWindow, cx| with_active(cx, |w, _| w.remove_window()));
    edit_menu::register(cx);
    cx.on_action(|_: &About, cx| {
        with_active(cx, |w, cx| {
            crate::ui::toast_info(w, cx, format!("Fuchico {}", env!("CARGO_PKG_VERSION")))
        })
    });
    cx.on_action(|_: &OpenFolder, cx| {
        // Reached when no workspace handled it (e.g. from an editor window).
        open_main_window(cx);
    });

    cx.on_action(|_: &EditorZoomIn, cx| zoom_font(cx, Some(1.)));
    cx.on_action(|_: &EditorZoomOut, cx| zoom_font(cx, Some(-1.)));
    cx.on_action(|_: &EditorZoomReset, cx| zoom_font(cx, None));
    cx.on_action(|_: &UiZoomIn, cx| zoom_ui(cx, Some(0.1)));
    cx.on_action(|_: &UiZoomOut, cx| zoom_ui(cx, Some(-0.1)));
    cx.on_action(|_: &UiZoomReset, cx| zoom_ui(cx, None));
    cx.on_action(|_: &ToggleTheme, cx| {
        let dark = crate::theme::AppTheme::get(cx).appearance.is_dark();
        SettingsStore::update(cx, |s| {
            if s.palette == PaletteId::Dracula {
                return;
            }
            s.mode = if dark { ModePref::Light } else { ModePref::Dark };
        });
    });
}

fn with_active(cx: &mut App, f: impl FnOnce(&mut gpui_kit::Window, &mut App)) {
    if let Some(h) = cx.active_window() {
        h.update(cx, |_, w, cx| f(w, cx)).ok();
    }
}

pub(crate) fn zoom_font(cx: &mut App, delta: Option<f32>) {
    SettingsStore::update(cx, |s| {
        s.editor_font_size = match delta {
            Some(d) => clamp_editor_font_size(s.editor_font_size + d),
            None => EDITOR_FONT_SIZE_DEFAULT,
        }
    });
}

fn zoom_ui(cx: &mut App, delta: Option<f32>) {
    SettingsStore::update(cx, |s| {
        s.ui_scale = match delta {
            // Round to one decimal so repeated steps don't drift.
            Some(d) => clamp_ui_scale(((s.ui_scale + d) * 10.).round() / 10.),
            None => UI_SCALE_DEFAULT,
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_urls_decode() {
        assert_eq!(percent_decode("/v/a%20b.md"), "/v/a b.md");
        assert_eq!(percent_decode("/v/%C3%A9.md"), "/v/é.md");
        assert_eq!(percent_decode("/v/100%"), "/v/100%");
        assert!(paths_from_urls(&["https://x".into(), "file:///definitely/missing.md".into()]).is_empty());
    }

    /// Finder "Open With" / double-click on a `.md` (CFBundleDocumentTypes):
    /// AppKit hands the files to `application:openURLs:` as `file://` URLs,
    /// which `main` routes through `paths_from_urls` → `push_open_requests`.
    #[test]
    fn finder_open_urls_reach_the_open_queue() {
        let dir = crate::test_ui::temp_dir("open-urls");
        let note = dir.join("my note.md");
        std::fs::write(&note, "# hi\n").unwrap();
        let url = format!("file://{}", note.to_string_lossy().replace(' ', "%20"));
        let paths = paths_from_urls(&[url, format!("file://{}", dir.to_string_lossy())]);
        assert_eq!(paths, vec![note.to_string_lossy().into_owned()]);
        let queue = OpenRequests::default();
        queue.push(&*crate::bus::sink(), paths.clone());
        assert_eq!(queue.take(), paths);
    }
}
