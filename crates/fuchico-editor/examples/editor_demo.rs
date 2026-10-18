//! Standalone editor window on a Markdown note.
//!
//! `cargo run -p fuchico-editor --example editor_demo [-- <file.md>]`
//! (default: `demo-notes/editor-features.md`). `EDITOR_LIGHT=1` picks Ayu
//! light; `FUCHICO_BACKGROUND=1` opens the window without activating the app.
//! Ctrl-S / `:w` writes the file back.

use std::path::PathBuf;

use fuchico_editor::{EditorEvent, EditorOptions, EditorStyle, EditorView};
use gpui_kit::*;

fn note_path() -> PathBuf {
    if let Some(p) = std::env::args().nth(1) {
        return PathBuf::from(p);
    }
    let local =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../demo-notes/editor-features.md");
    if local.exists() {
        return local;
    }
    dirs_home().join("Documents/Dev/obsidian-helix/demo-notes/editor-features.md")
}

fn dirs_home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default()
}

fn register_fonts(cx: &mut App) {
    let fonts: Vec<std::borrow::Cow<'static, [u8]>> = vec![
        include_bytes!("../../../apps/fuchico/assets/fonts/JetBrainsMono-Regular.ttf")
            .as_slice()
            .into(),
        include_bytes!("../../../apps/fuchico/assets/fonts/JetBrainsMono-Bold.ttf")
            .as_slice()
            .into(),
        include_bytes!("../../../apps/fuchico/assets/fonts/JetBrainsMono-Italic.ttf")
            .as_slice()
            .into(),
        include_bytes!("../../../apps/fuchico/assets/fonts/JetBrainsMono-BoldItalic.ttf")
            .as_slice()
            .into(),
        include_bytes!("../../../apps/fuchico/assets/fonts/Inter-Regular.ttf")
            .as_slice()
            .into(),
        include_bytes!("../../../apps/fuchico/assets/fonts/Inter-SemiBold.ttf")
            .as_slice()
            .into(),
        include_bytes!("../../../apps/fuchico/assets/fonts/Inter-Bold.ttf")
            .as_slice()
            .into(),
    ];
    let _ = cx.text_system().add_fonts(fonts);
}

fn main() {
    let path = note_path();
    let text =
        std::fs::read_to_string(&path).unwrap_or_else(|e| format!("# {}\n\n{e}\n", path.display()));
    let style = if std::env::var_os("EDITOR_LIGHT").is_some() {
        EditorStyle::ayu_light()
    } else {
        EditorStyle::ayu_dark()
    };
    gpui_kit::application().run(move |cx| {
        gpui_kit::init(cx);
        register_fonts(cx);
        fuchico_editor::init(cx);
        let opts = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                None,
                size(px(1100.0), px(800.0)),
                cx,
            ))),
            ..Default::default()
        };
        gpui_kit::open_window(opts, cx, move |window, cx| {
            let view =
                cx.new(|cx| EditorView::new(&text, EditorOptions::default(), style, window, cx));
            let save_path = path.clone();
            cx.subscribe(&view, move |view, ev: &EditorEvent, cx| match ev {
                EditorEvent::Save => {
                    let text = view.read(cx).text();
                    if std::fs::write(&save_path, text).is_ok() {
                        view.update(cx, |v, cx| v.mark_saved(cx));
                    }
                }
                EditorEvent::Quit => cx.quit(),
                _ => {}
            })
            .detach();
            let fh = view.focus_handle(cx);
            window.focus(&fh, cx);
            view
        })
        .expect("window");
        if std::env::var_os("FUCHICO_BACKGROUND").is_none() {
            cx.activate(true);
        }
    });
}
