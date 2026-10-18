//! Host interface: events, clipboard, mermaid previews, drops, zoom,
//! paste-image, and the `Editor*` actions / widgets.

mod common;

use common::*;
use fuchico_editor::{EditorEvent, EditorOptions, GoToLine, ZoomDirection};
use gpui_kit::{ClipboardEntry, ClipboardItem, Image, ImageFormat, Modifiers, TestAppContext};

#[gpui_kit::test]
async fn link_wikilink_and_tag_clicks(cx: &mut TestAppContext) {
    let opts = EditorOptions {
        vault_files: vec!["Note.md".into()],
        ..Default::default()
    };
    let mut ui = open(
        cx,
        "top\nsee [[Note]] and [site](https://example.com) #todo\n",
        opts,
    );
    let p = ui.hit("Wikilink {");
    ui.click(p, Modifiers::secondary_key());
    let p = ui.hit("Link(");
    ui.click(p, Modifiers::secondary_key());
    let p = ui.hit("Tag(");
    ui.click(p, Modifiers::secondary_key());
    let ev = ui.events();
    assert!(
        ev.iter().any(
            |e| matches!(e, EditorEvent::OpenWikilink { target, resolved }
            if target == "Note" && resolved.as_deref() == Some("Note.md"))
        ),
        "{ev:?}"
    );
    assert!(
        ev.iter()
            .any(|e| matches!(e, EditorEvent::OpenLink(u) if u == "https://example.com"))
    );
    assert!(
        ev.iter()
            .any(|e| matches!(e, EditorEvent::TagClicked(t) if t == "todo"))
    );
}

#[gpui_kit::test]
async fn save_dirty_and_zoom_events(cx: &mut TestAppContext) {
    let mut ui = open(cx, "x\n", EditorOptions::default());
    ui.press("cmd-s");
    assert!(ui.events().iter().any(|e| matches!(e, EditorEvent::Save)));
    ui.press("i");
    ui.type_text("y");
    let ev = ui.events();
    assert!(
        ev.iter()
            .any(|e| matches!(e, EditorEvent::DirtyChanged(true)))
    );
    assert!(
        ev.iter()
            .any(|e| matches!(e, EditorEvent::Changed { dirty: true }))
    );
    ui.update(|v, _, cx| v.mark_saved(cx));
    assert!(!ui.read(|v, _| v.is_dirty()));
    ui.clear_events();
    ui.press("cmd-=");
    ui.press("cmd--");
    ui.press("cmd-0");
    let z: Vec<ZoomDirection> = ui
        .events()
        .into_iter()
        .filter_map(|e| {
            if let EditorEvent::Zoom(d) = e {
                Some(d)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(
        z,
        vec![ZoomDirection::In, ZoomDirection::Out, ZoomDirection::Reset]
    );
}

#[gpui_kit::test]
async fn clipboard_yank_paste_and_mod_keys(cx: &mut TestAppContext) {
    let mut ui = open(cx, "hello world\n", EditorOptions::default());
    // Helix `Space y` yanks the selection to the system clipboard.
    ui.press("w");
    ui.press("space");
    ui.press("y");
    let clip = ui.cx.read_from_clipboard().and_then(|c| c.text());
    assert!(
        clip.as_deref().is_some_and(|c| c.starts_with("hello")),
        "{clip:?}"
    );
    // `Space p` pastes the system clipboard.
    ui.cx
        .write_to_clipboard(ClipboardItem::new_string("XY".into()));
    ui.press("space");
    ui.press("p");
    assert!(ui.text().contains("XY"), "{:?}", ui.text());
    // Mod-c / Mod-v.
    ui.update(|v, _, cx| v.set_selection(0, 5, cx));
    ui.press("cmd-c");
    assert_eq!(
        ui.cx
            .read_from_clipboard()
            .and_then(|c| c.text())
            .as_deref(),
        Some("hello")
    );
}

#[gpui_kit::test]
async fn paste_image_is_forwarded(cx: &mut TestAppContext) {
    let mut ui = open(cx, "x\n", EditorOptions::default());
    let img = Image::from_bytes(ImageFormat::Png, vec![0x89, b'P', b'N', b'G']);
    ui.cx.write_to_clipboard(ClipboardItem {
        entries: vec![ClipboardEntry::Image(img)],
    });
    ui.press("cmd-v");
    let ev = ui.events();
    assert!(
        ev.iter().any(|e| matches!(e, EditorEvent::PasteImage { mime, bytes } if mime == "image/png" && bytes.len() == 4)),
        "{ev:?}"
    );
    ui.update(|v, _, cx| v.insert_image_link("![[img.png]]", cx));
    assert!(ui.text().contains("![[img.png]]"));
}

#[gpui_kit::test]
async fn dropped_paths_become_links(cx: &mut TestAppContext) {
    let opts = EditorOptions {
        path: Some("notes/today.md".into()),
        ..Default::default()
    };
    let mut ui = open(cx, "start\n", opts);
    ui.press("i");
    ui.update(|v, _, cx| {
        v.insert_dropped_paths(&["notes/pic.png".into(), "docs/spec.pdf".into()], cx)
    });
    let t = ui.text();
    assert!(t.contains("pic.png") && t.contains("spec.pdf"), "{t:?}");
}

#[gpui_kit::test]
async fn mermaid_open_update_close(cx: &mut TestAppContext) {
    let text = "intro\n```mermaid\ngraph TD\nA-->B\n```\nend\n";
    let mut ui = open(cx, text, EditorOptions::default());
    let p = ui.hit("Mermaid {");
    ui.click(p, Modifiers::none());
    let key = ui
        .events()
        .iter()
        .find_map(|e| match e {
            EditorEvent::MermaidOpen(b) => Some((b.key, b.source.clone())),
            _ => None,
        })
        .expect("MermaidOpen");
    assert_eq!(key.1, "graph TD\nA-->B");
    // Editing the fence body sends an update.
    let pos = text.find("B\n```").unwrap();
    ui.update(|v, _, cx| v.set_selection(pos + 1, pos + 1, cx));
    ui.press("i");
    ui.type_text("-->C");
    let upd = ui.events().iter().rev().find_map(|e| match e {
        EditorEvent::MermaidUpdate(b) => Some(b.source.clone()),
        _ => None,
    });
    assert_eq!(upd.as_deref(), Some("graph TD\nA-->B-->C"));
    // Deleting the fence closes it.
    ui.press("escape");
    ui.update(|v, _, cx| v.set_text("intro\nend\n", cx));
    assert!(
        ui.events()
            .iter()
            .any(|e| matches!(e, EditorEvent::MermaidClose { key: k } if *k == key.0))
    );
    assert!(ui.read(|v, _| v.open_mermaid_blocks()).is_empty());
}

#[gpui_kit::test]
async fn checkbox_click_and_toggle_actions(cx: &mut TestAppContext) {
    let mut ui = open(
        cx,
        "title\n- [ ] task one\nplain\n",
        EditorOptions::default(),
    );
    let p = ui.hit("Checkbox(");
    ui.click(p, Modifiers::none());
    assert_eq!(ui.text(), "title\n- [x] task one\nplain\n");
    // Mod-Enter toggles the checkbox on the cursor line.
    ui.update(|v, _, cx| v.set_selection(8, 8, cx));
    ui.press("cmd-enter");
    assert_eq!(ui.text(), "title\n- [ ] task one\nplain\n");
    // Mod-Shift-Enter makes a task line of "plain".
    ui.update(|v, _, cx| v.set_selection(21, 21, cx));
    ui.press("cmd-shift-enter");
    assert_eq!(ui.text(), "title\n- [ ] task one\n- [ ] plain\n");
}

#[gpui_kit::test]
async fn insert_date_region_and_pickers(cx: &mut TestAppContext) {
    let mut ui = open(cx, "\n", EditorOptions::default());
    ui.press("i");
    ui.press("cmd-shift-d");
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    assert!(ui.text().starts_with(&today), "{:?}", ui.text());
    ui.press("cmd-alt-d");
    assert!(ui.text().matches(&today).count() == 2);
    ui.press("escape");
    ui.update(|v, _, cx| {
        v.set_text("body\n", cx);
        v.set_selection(0, 0, cx);
    });
    // Normal mode: the block cursor is a 1-char selection, so the line is wrapped.
    ui.press("cmd-shift-r");
    assert!(
        ui.text().starts_with(":::fold Region\nbody\n:::endfold"),
        "{:?}",
        ui.text()
    );
    // Due-date picker inserts the token and opens the date completion.
    ui.update(|v, _, cx| v.set_text("- [ ] task\n", cx));
    ui.press("cmd-shift-.");
    assert!(ui.text().contains("📅"), "{:?}", ui.text());
    assert_eq!(ui.read(|v, _| v.mode_label()), "INS");
    assert!(
        ui.read(|v, _| v.completion_labels())
            .is_some_and(|l| !l.is_empty()),
        "date options"
    );
    ui.press("escape");
    ui.press("escape");
    ui.press("cmd-shift-,");
    assert!(ui.text().contains("🔁"), "{:?}", ui.text());
}

#[gpui_kit::test]
async fn table_insert_edit_and_widget(cx: &mut TestAppContext) {
    let mut ui = open(cx, "\n", EditorOptions::default());
    ui.press("cmd-alt-t");
    assert_eq!(ui.read(|v, _| v.table_cell()), Some((0, 0, true)));
    assert!(ui.text().contains("| Column 1"), "{:?}", ui.text());
    ui.type_text("Name");
    assert!(ui.text().contains("Name"), "{:?}", ui.text());
    ui.press("tab");
    assert_eq!(
        ui.read(|v, _| v.table_cell()).map(|c| (c.0, c.1)),
        Some((0, 1))
    );
    ui.press("escape");
    // The rendered widget exposes cells and +row/+column buttons.
    ui.update(|v, _, cx| v.set_selection(0, 0, cx));
    ui.press("escape");
    let names: Vec<String> = ui
        .read(|v, _| v.debug_hits())
        .into_iter()
        .map(|h| h.0)
        .collect();
    assert!(
        names.iter().any(|n| n.starts_with("TableCell")),
        "{names:?}"
    );
    let rows_before = ui.text().lines().count();
    let p = ui.hit("TableAddRow");
    ui.click(p, Modifiers::none());
    assert_eq!(ui.text().lines().count(), rows_before + 1);
}

#[gpui_kit::test]
async fn fence_toolbar_copy_and_language(cx: &mut TestAppContext) {
    let text = "top\n```rust\nfn main() {}\n```\n";
    let mut ui = open(cx, text, EditorOptions::default());
    let p = ui.hit("Copy {");
    ui.click(p, Modifiers::none());
    assert_eq!(
        ui.cx
            .read_from_clipboard()
            .and_then(|c| c.text())
            .as_deref(),
        Some("fn main() {}")
    );
    let p = ui.hit("Lang(");
    ui.click(p, Modifiers::none());
    ui.press("down");
    ui.press("enter");
    assert!(
        !ui.text().contains("```rust\n"),
        "language changed: {:?}",
        ui.text()
    );
}

#[gpui_kit::test]
async fn go_to_line_action(cx: &mut TestAppContext) {
    let text: String = (1..=100).map(|i| format!("l{i}\n")).collect();
    let mut ui = open(cx, &text, EditorOptions::default());
    ui.cx.dispatch_action(GoToLine { line: 42 });
    ui.frame();
    assert_eq!(ui.read(|v, _| v.line_col().0), 42);
    let (a, b) = ui.read(|v, _| v.visible_lines()).unwrap();
    assert!(a <= 42 && 42 <= b);
}

#[gpui_kit::test]
async fn host_command_and_external_effects(cx: &mut TestAppContext) {
    let mut opts = EditorOptions::default();
    opts.externals.file_picker = true;
    let mut ui = open(cx, "x\n", opts);
    // `Space f` (file picker) is handed to the host.
    ui.press("space");
    ui.press("f");
    let ev = ui.events();
    assert!(
        ev.iter().any(|e| matches!(
            e,
            EditorEvent::External(fuchico_helix::External::FilePicker)
        )),
        "{ev:?}"
    );
}
