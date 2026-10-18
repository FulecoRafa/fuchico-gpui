//! Offscreen snapshots (no window is ever shown, nothing takes focus):
//! `target/snapshots/editor-ayu-{dark,light}.png` of the original's
//! `demo-notes/editor-features.md`, for comparison with
//! `obsidian-helix/docs/screenshots/editor.png`.

mod common;

use std::path::PathBuf;
use std::sync::Arc;

use fuchico_editor::{EditorOptions, EditorStyle, EditorView};
use gpui_kit::{AppContext, Focusable, HeadlessAppContext, px, size};

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

fn note() -> String {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default();
    let p = home.join("Documents/Dev/obsidian-helix/demo-notes/editor-features.md");
    std::fs::read_to_string(p).unwrap_or_else(|_| {
        "# Editor Features Playground\n\nSome **bold**, `code`, [[link]] #tag\n\n- [ ] task\n- [x] done\n".into()
    })
}

fn snapshot(
    name: &str,
    style: EditorStyle,
    text: String,
    prepare: impl FnOnce(&mut EditorView, &mut gpui_kit::Context<EditorView>),
) -> PathBuf {
    let mut cx = HeadlessAppContext::with_platform(
        text_system(),
        Arc::new(gpui_kit::assets::Assets),
        || gpui_platform::current_headless_renderer(),
    );
    cx.update(|cx| {
        gpui_kit::init(cx);
        common::register_fonts(cx);
        fuchico_editor::init(cx);
    });
    let mut prepare = Some(prepare);
    let handle = cx
        .open_window(size(px(1660.), px(1150.)), |window, cx| {
            let view =
                cx.new(|cx| EditorView::new(&text, EditorOptions::default(), style, window, cx));
            view.update(cx, |v, cx| (prepare.take().unwrap())(v, cx));
            let fh = view.focus_handle(cx);
            window.focus(&fh, cx);
            cx.new(|cx| gpui_kit::base::Root::new(view, window, cx))
        })
        .expect("open headless window");
    cx.run_until_parked();
    for _ in 0..2 {
        cx.update_window(handle.into(), |_, window, cx| {
            window.refresh();
            window.draw(cx).clear(cx);
        })
        .expect("draw");
    }
    let image = cx
        .capture_screenshot(handle.into())
        .expect("headless capture");
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/snapshots");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(format!("{name}.png"));
    image.save(&path).expect("write png");
    path
}

#[test]
fn snapshot_editor_features() {
    let dark = snapshot(
        "editor-ayu-dark",
        EditorStyle::ayu_dark(),
        note(),
        |_, _| {},
    );
    let light = snapshot(
        "editor-ayu-light",
        EditorStyle::ayu_light(),
        note(),
        |_, _| {},
    );
    assert!(dark.exists() && light.exists());
}

const WIDGETS: &str = "# Widgets\n\nSome **bold**, *italic*, ~~strike~~, `code`, [link](https://x.y), [[Wiki]] and #tag.\n\n> A blockquote line\n> and another.\n\n---\n\n```rust\nfn main() {\n    println!(\"hi\");\n}\n```\n\n| Name | Qty |\n| :--- | ---: |\n| apple | 3 |\n| pear | 10 |\n\n```mermaid\ngraph TD\nA-->B\n```\n\n- [ ] open task\n- [x] done task\n\n:::fold Region\ninside\n:::endfold\n";

#[test]
fn snapshot_widgets_and_prompt() {
    let p = snapshot(
        "editor-widgets-ayu-dark",
        EditorStyle::ayu_dark(),
        WIDGETS.into(),
        |v, cx| {
            let mut o = v.options().clone();
            o.rulers = vec![80];
            v.set_options(o, cx);
            v.handle_keys(":", cx); // opens the prompt (later keys would bypass overlays)
        },
    );
    assert!(p.exists());
}
