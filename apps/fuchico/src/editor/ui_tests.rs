//! Headless end-to-end tests of the editor pane: real key / pointer input
//! through gpui's test windows (no OS input, no focus stealing).

use std::cell::RefCell;
use std::rc::Rc;

use fuchico_core::events::CoreEvent;
use fuchico_editor::{EditorEvent, EngineKind, LineNumbers};
use gpui_kit::{App, AppContext, Entity, Focusable, Modifiers, TestAppContext};

use super::{AUTOSAVE_MS, EditorPane, State, format_bytes};
use crate::bus::{self, AppEvent};
use crate::settings::ui_state::UiState;
use crate::settings::{KeybindingMode, SettingsStore};
use crate::test_ui::{self, Ui};
use crate::workspace::MainViewKind;
use crate::workspace::pane::{PaneContext, PaneEvent};

fn text(pane: &Entity<EditorPane>, cx: &App) -> String {
    pane.read(cx)
        .view()
        .map(|v| v.read(cx).text())
        .unwrap_or_default()
}

fn disk(path: &str) -> String {
    std::fs::read_to_string(path).unwrap()
}

fn written(rec: &Entity<test_ui::BusRecorder>, path: &str, source: &str, cx: &App) -> usize {
    rec.read(cx)
        .seen
        .iter()
        .filter(|e| {
            matches!(e, AppEvent::Core(CoreEvent::FileWritten { path: p, source: Some(s) }) if p == path && s == source)
        })
        .count()
}

fn external_write(ui: &mut Ui, path: &str, content: &str) {
    std::fs::write(path, content).unwrap();
    let path = path.to_string();
    ui.cx.update(|_, cx| {
        bus::emit(
            cx,
            AppEvent::Core(CoreEvent::FileWritten {
                path,
                source: Some("replace".into()),
            }),
        )
    });
}

type Events = Rc<RefCell<Vec<PaneEvent>>>;

/// Opens `path` in a bare pane window and focuses the editor.
async fn open_pane(
    cx: &mut TestAppContext,
    path: &str,
    pcx: PaneContext,
) -> (Ui, Entity<EditorPane>, Events) {
    let p = path.to_string();
    let (mut ui, pane) = test_ui::open(cx, (1000., 700.), move |window, cx| {
        EditorPane::new(p, pcx, window, cx)
    });
    let events: Events = Rc::default();
    let sink = events.clone();
    ui.cx.update(|_, cx| {
        cx.subscribe(&pane, move |_, ev: &PaneEvent, _| {
            sink.borrow_mut().push(ev.clone())
        })
        .detach()
    });
    ui.wait("editor mounted", |cx| pane.read(cx).view().is_some())
        .await;
    let fh = ui.cx.read(|cx| pane.read(cx).focus_handle(cx));
    ui.cx.update(|window, cx| window.focus(&fh, cx));
    ui.frame();
    (ui, pane, events)
}

fn dirty_events(events: &Events) -> Vec<bool> {
    events
        .borrow()
        .iter()
        .filter_map(|e| match e {
            PaneEvent::DirtyChanged(d) => Some(*d),
            _ => None,
        })
        .collect()
}

#[gpui_kit::test]
async fn type_dirty_autosave_save_and_external_reload(cx: &mut TestAppContext) {
    cx.update(test_ui::init_shell);
    let dir = test_ui::temp_dir("editor");
    let path = dir.join("note.md").to_string_lossy().to_string();
    let dir_s = dir.to_string_lossy().to_string();
    std::fs::write(&path, "# Title\n\nbody\n").unwrap();
    let rec = test_ui::record(cx, |e| {
        matches!(e, AppEvent::Core(CoreEvent::FileWritten { .. }))
    });
    let (mut ui, pane, events) = open_pane(
        cx,
        &path,
        PaneContext {
            root: Some(dir_s.clone()),
            ..Default::default()
        },
    )
    .await;
    assert_eq!(ui.cx.read(|cx| text(&pane, cx)), "# Title\n\nbody\n");

    // ---- typing (Helix insert mode) marks the tab dirty -------------------
    ui.press("i");
    ui.type_text("Hi ");
    ui.press("escape");
    assert_eq!(ui.cx.read(|cx| text(&pane, cx)), "Hi # Title\n\nbody\n");
    assert_eq!(dirty_events(&events), [true]);
    assert_eq!(disk(&path), "# Title\n\nbody\n", "nothing written yet");

    // ---- 800 ms trailing autosave (source "editor") ------------------------
    ui.sleep(AUTOSAVE_MS - 100).await;
    assert_eq!(disk(&path), "# Title\n\nbody\n", "debounced");
    ui.sleep(200).await;
    ui.wait("autosaved", |cx| !pane.read(cx).dirty).await;
    assert_eq!(disk(&path), "Hi # Title\n\nbody\n");
    assert_eq!(dirty_events(&events), [true, false]);
    assert_eq!(ui.cx.read(|cx| written(&rec, &path, "editor", cx)), 1);
    ui.wait("stats", |cx| pane.read(cx).stats.words == 4).await;

    // ---- saving an unchanged buffer is a no-op ----------------------------
    ui.press("cmd-s");
    ui.sleep(10).await;
    assert_eq!(ui.cx.read(|cx| written(&rec, &path, "editor", cx)), 1);

    // ---- Mod-s writes right away; `:w` too ---------------------------------
    ui.press("i");
    ui.type_text("A");
    ui.press("escape");
    ui.press("cmd-s");
    ui.sleep(10).await;
    assert_eq!(disk(&path), "Hi A# Title\n\nbody\n");
    assert!(!ui.cx.read(|cx| pane.read(cx).dirty));
    let view = ui.cx.read(|cx| pane.read(cx).view().unwrap().clone());
    ui.cx.update(|_, cx| {
        view.update(cx, |v, cx| {
            v.handle_keys("iB<Esc>", cx);
            v.execute_command("w", cx);
        })
    });
    ui.sleep(10).await;
    assert_eq!(disk(&path), "Hi AB# Title\n\nbody\n");

    // ---- external change, clean buffer → reload ---------------------------
    external_write(&mut ui, &path, "from elsewhere\n");
    ui.wait("reloaded", |cx| text(&pane, cx) == "from elsewhere\n")
        .await;
    assert!(!ui.cx.read(|cx| pane.read(cx).dirty));
    assert!(!ui.cx.read(|cx| pane.read(cx).has_conflict()));
    // A folder change (the watcher: another app) reloads as well.
    std::fs::write(&path, "via watcher\n").unwrap();
    let d = dir_s.clone();
    ui.cx.update(|_, cx| {
        bus::emit(
            cx,
            AppEvent::Core(CoreEvent::FsChanged { dir: d, from: None }),
        )
    });
    ui.wait("reloaded from fs:changed", |cx| {
        text(&pane, cx) == "via watcher\n"
    })
    .await;

    // ---- our own writes never reload --------------------------------------
    let p = path.clone();
    ui.cx.update(|_, cx| {
        bus::emit(
            cx,
            AppEvent::Core(CoreEvent::FileWritten {
                path: p,
                source: Some("editor".into()),
            }),
        )
    });
    ui.sleep(10).await;
    assert_eq!(ui.cx.read(|cx| text(&pane, cx)), "via watcher\n");

    // ---- external change while dirty → conflict banner --------------------
    ui.press("i");
    ui.type_text("mine ");
    ui.press("escape");
    external_write(&mut ui, &path, "theirs\n");
    ui.wait("conflict", |cx| pane.read(cx).has_conflict()).await;
    assert_eq!(
        ui.cx.read(|cx| text(&pane, cx)),
        "mine via watcher\n",
        "unsaved edits are kept"
    );
    assert!(ui.has("editor-conflict-keep") && ui.has("editor-conflict-reload"));
    // "Keep my version" hides the banner; the next autosave wins.
    ui.click("editor-conflict-keep");
    assert!(!ui.cx.read(|cx| pane.read(cx).has_conflict()));
    assert!(!ui.has("editor-conflict-keep"));
    ui.sleep(AUTOSAVE_MS + 50).await;
    assert_eq!(disk(&path), "mine via watcher\n");

    // "Reload from disk" takes theirs and drops the pending autosave.
    ui.press("i");
    ui.type_text("x");
    ui.press("escape");
    external_write(&mut ui, &path, "theirs again\n");
    ui.wait("conflict 2", |cx| pane.read(cx).has_conflict())
        .await;
    ui.click("editor-conflict-reload");
    ui.wait("took theirs", |cx| text(&pane, cx) == "theirs again\n")
        .await;
    assert!(!ui.cx.read(|cx| pane.read(cx).dirty));
    ui.sleep(AUTOSAVE_MS + 50).await;
    assert_eq!(disk(&path), "theirs again\n");
    let _ = std::fs::remove_dir_all(&dir);
}

#[gpui_kit::test]
async fn flush_on_close_scroll_persistence_and_goto(cx: &mut TestAppContext) {
    cx.update(test_ui::init_shell);
    let dir = test_ui::temp_dir("editor-flush");
    let path = dir.join("long.md").to_string_lossy().to_string();
    let body: String = (1..=300).map(|i| format!("line {i}\n")).collect();
    std::fs::write(&path, &body).unwrap();
    let (mut ui, pane, _) = open_pane(cx, &path, PaneContext::default()).await;

    // Unsaved edits + a scroll offset, then the window goes away.
    ui.press("i");
    ui.type_text("edited ");
    ui.press("escape");
    let view = ui.cx.read(|cx| pane.read(cx).view().unwrap().clone());
    ui.cx
        .update(|_, cx| view.update(cx, |v, cx| v.set_scroll_position(420., cx)));
    ui.frame();
    let scroll = ui.cx.read(|cx| view.read(cx).scroll_position());
    assert!(scroll > 100., "scrolled: {scroll}");
    drop(view);
    drop(pane);
    ui.cx.update(|window, _| window.remove_window());
    cx.run_until_parked();
    assert!(
        disk(&path).starts_with("edited line 1\n"),
        "flushed on close"
    );
    let saved = cx.read(|cx| UiState::get(cx).scroll.get(&path));
    assert_eq!(saved, Some(scroll as f64));

    // Reopening restores the offset.
    let (mut ui, pane, _) = open_pane(cx, &path, PaneContext::default()).await;
    let view = ui.cx.read(|cx| pane.read(cx).view().unwrap().clone());
    ui.frame();
    assert_eq!(ui.cx.read(|cx| view.read(cx).scroll_position()), scroll);

    // `pane:goto-line` (agenda rows) selects the whole line.
    let p = path.clone();
    ui.cx
        .update(|_, cx| bus::emit(cx, AppEvent::GotoLine { path: p, line: 3 }));
    ui.frame();
    let (sel, line) = ui.cx.read(|cx| {
        let v = view.read(cx);
        let r = v.selection().main();
        let (a, b) = (r.from(), r.to());
        (
            v.text().chars().skip(a).take(b - a).collect::<String>(),
            v.line_col().0,
        )
    });
    assert_eq!(sel.trim_end(), "line 3");
    assert_eq!(line, 3);
    let _ = std::fs::remove_dir_all(&dir);
}

#[gpui_kit::test]
async fn settings_theme_and_shortcuts_apply_live(cx: &mut TestAppContext) {
    cx.update(test_ui::init_shell);
    let dir = test_ui::temp_dir("editor-settings");
    let path = dir.join("s.md").to_string_lossy().to_string();
    std::fs::write(&path, "one\ntwo\n").unwrap();
    let (mut ui, pane, events) = open_pane(cx, &path, PaneContext::default()).await;
    let view = ui.cx.read(|cx| pane.read(cx).view().unwrap().clone());
    let opts = |ui: &mut Ui| ui.cx.read(|cx| view.read(cx).options().clone());
    let o = opts(&mut ui);
    assert_eq!(o.engine, EngineKind::Helix);
    assert!(o.mode_chip && o.command_line_via_host);
    assert_eq!(o.tab_size, 2);

    ui.cx.update(|_, cx| {
        SettingsStore::update(cx, |s| {
            s.relative_line_numbers = true;
            s.tab_size = 4;
            s.rulers = vec![80];
            s.fold_start_marker = "<<<".into();
            s.fold_end_marker = ">>>".into();
            s.editor_font_size = 20.;
        })
    });
    ui.frame();
    let o = opts(&mut ui);
    assert_eq!(o.line_numbers, LineNumbers::Relative);
    assert_eq!(o.tab_size, 4);
    assert_eq!(o.rulers, vec![80]);
    assert_eq!(
        (o.fold_start_marker.as_str(), o.fold_end_marker.as_str()),
        ("<<<", ">>>")
    );
    ui.wait("font size", |cx| {
        view.read(cx).style().font_size == gpui_kit::px(20.)
    })
    .await;

    // `:` in Helix normal mode opens the command palette pre-filled ":".
    ui.press(":");
    assert!(
        events
            .borrow()
            .iter()
            .any(|e| matches!(e, PaneEvent::OpenCommandPalette(q) if q == ":"))
    );
    ui.press("escape");

    // Rebindable shortcut: insertDate moves from Mod-Shift-d to Mod-Shift-y.
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    ui.press("cmd-shift-d");
    assert!(
        ui.cx.read(|cx| view.read(cx).text()).contains(&today),
        "default binding"
    );
    ui.cx
        .update(|_, cx| view.update(cx, |v, cx| v.set_text("one\ntwo\n", cx)));
    ui.cx.update(|_, cx| {
        SettingsStore::update(cx, |s| {
            s.shortcuts
                .insert("insertDate".into(), "Mod-Shift-y".into());
        })
    });
    ui.frame();
    ui.press("cmd-shift-d");
    assert!(
        !ui.cx.read(|cx| view.read(cx).text()).contains(&today),
        "old binding is gone"
    );
    ui.press("cmd-shift-y");
    assert!(
        ui.cx.read(|cx| view.read(cx).text()).contains(&today),
        "new binding inserts the date"
    );

    // Engine seam: Normal is non-modal; Vim runs fuchico-vim without the
    // Helix chip / `:` palette.
    ui.cx
        .update(|_, cx| SettingsStore::update(cx, |s| s.keybinding_mode = KeybindingMode::Normal));
    ui.frame();
    assert_eq!(opts(&mut ui).engine, EngineKind::Normal);
    ui.cx
        .update(|_, cx| view.update(cx, |v, cx| v.set_text("one\n", cx)));
    ui.type_text("z");
    assert!(
        ui.cx.read(|cx| view.read(cx).text()).contains('z'),
        "Normal mode types right away"
    );
    ui.cx
        .update(|_, cx| SettingsStore::update(cx, |s| s.keybinding_mode = KeybindingMode::Vim));
    ui.frame();
    let o = opts(&mut ui);
    assert_eq!(o.engine, EngineKind::Vim);
    assert!(!o.mode_chip && !o.command_line_via_host);
    assert!(ui.cx.read(|cx| view.read(cx).engine().is_vim()));
    let _ = std::fs::remove_dir_all(&dir);
}

/// Vim mode end to end in the pane: the engine switches live (text kept),
/// `:w` writes the file, and two panes of one window share registers.
#[gpui_kit::test]
async fn vim_mode_switch_save_and_shared_registers(cx: &mut TestAppContext) {
    cx.update(test_ui::init_shell);
    let dir = test_ui::temp_dir("editor-vim");
    let path = dir.join("v.md").to_string_lossy().to_string();
    std::fs::write(&path, "alpha beta\n").unwrap();
    let rec = test_ui::record(cx, |e| {
        matches!(e, AppEvent::Core(CoreEvent::FileWritten { .. }))
    });
    let (mut ui, pane, events) = open_pane(cx, &path, PaneContext::default()).await;
    let view = ui.cx.read(|cx| pane.read(cx).view().unwrap().clone());
    let vtext = |ui: &mut Ui| ui.cx.read(|cx| view.read(cx).text());

    // Helix edit, then switch to Vim at runtime: the text survives.
    ui.press("i");
    ui.type_text("x");
    ui.press("escape");
    assert_eq!(vtext(&mut ui), "xalpha beta\n");
    ui.cx
        .update(|_, cx| SettingsStore::update(cx, |s| s.keybinding_mode = KeybindingMode::Vim));
    ui.frame();
    assert!(ui.cx.read(|cx| view.read(cx).engine().is_vim()));
    assert_eq!(vtext(&mut ui), "xalpha beta\n");

    // Vim keys: `x` deletes, `dw` deletes a word.
    ui.press("0");
    ui.press("x");
    ui.press("d");
    ui.press("w");
    assert_eq!(vtext(&mut ui), "beta\n");

    // `:` opens cm-vim's own prompt (not the palette); `:w` saves.
    ui.press(":");
    assert!(
        !events
            .borrow()
            .iter()
            .any(|e| matches!(e, PaneEvent::OpenCommandPalette(_)))
    );
    assert_eq!(
        ui.cx.read(|cx| view.read(cx).prompt()),
        Some((":".to_string(), String::new()))
    );
    ui.type_text("w");
    ui.press("enter");
    ui.wait("written", |cx| written(&rec, &path, "editor", cx) >= 1)
        .await;
    assert_eq!(disk(&path), "beta\n");
    assert_eq!(ui.cx.read(|cx| view.read(cx).prompt()), None);

    // Registers are shared with the window's other Vim editors.
    ui.press("y");
    ui.press("y");
    let other = ui.cx.update(|window, cx| {
        let v = cx.new(|cx| {
            fuchico_editor::EditorView::new(
                "one\n",
                fuchico_editor::EditorOptions {
                    engine: EngineKind::Vim,
                    ..Default::default()
                },
                fuchico_editor::EditorStyle::ayu_dark(),
                window,
                cx,
            )
        });
        let g = super::vim_globals_for(window, cx);
        v.update(cx, |v, _| v.set_vim_globals(Some(g)));
        v
    });
    ui.cx.update(|_, cx| {
        other.update(cx, |v, cx| {
            let fx = v
                .engine_mut()
                .handle_key(fuchico_helix::KeyEvent::char('p'));
            v.apply_effects(fx, cx);
        })
    });
    assert_eq!(ui.cx.read(|cx| other.read(cx).text()), "one\nbeta\n");

    // Back to Helix: text kept, Helix keys again.
    ui.cx
        .update(|_, cx| SettingsStore::update(cx, |s| s.keybinding_mode = KeybindingMode::Helix));
    ui.frame();
    assert!(!ui.cx.read(|cx| view.read(cx).engine().is_vim()));
    assert_eq!(vtext(&mut ui), "beta\n");
    let _ = std::fs::remove_dir_all(&dir);
}

#[gpui_kit::test]
async fn binary_and_image_files_show_a_status_screen(cx: &mut TestAppContext) {
    cx.update(test_ui::init_shell);
    assert_eq!(format_bytes(512), "512 B");
    assert_eq!(format_bytes(1536), "1.5 KB");
    let dir = test_ui::temp_dir("editor-binary");
    let bin = dir.join("blob.bin").to_string_lossy().to_string();
    std::fs::write(&bin, [0u8, 1, 2, 0, 255, 0]).unwrap();
    let p = bin.clone();
    let (mut ui, pane) = test_ui::open(cx, (600., 400.), move |window, cx| {
        EditorPane::new(p, PaneContext::default(), window, cx)
    });
    ui.wait("loaded", |cx| {
        matches!(
            pane.read(cx).state,
            State::NotText {
                too_large: false,
                ..
            }
        )
    })
    .await;
    assert!(ui.cx.read(|cx| pane.read(cx).view().is_none()));
    let _ = std::fs::remove_dir_all(&dir);
}

#[gpui_kit::test]
async fn paste_and_drop_images_into_attachments(cx: &mut TestAppContext) {
    cx.update(test_ui::init_shell);
    let dir = test_ui::temp_dir("editor-images");
    let root = dir.to_string_lossy().to_string();
    std::fs::create_dir_all(dir.join("notes")).unwrap();
    let path = dir.join("notes/n.md").to_string_lossy().to_string();
    std::fs::write(&path, "x\n").unwrap();
    let (mut ui, pane, _) = open_pane(
        cx,
        &path,
        PaneContext {
            root: Some(root.clone()),
            ..Default::default()
        },
    )
    .await;
    let view = ui.cx.read(|cx| pane.read(cx).view().unwrap().clone());
    ui.cx.update(|_, cx| {
        view.update(cx, |_, cx| {
            cx.emit(EditorEvent::PasteImage {
                mime: "image/png".into(),
                bytes: vec![1, 2, 3],
            })
        })
    });
    // The placeholder goes in at once, then becomes the link.
    assert!(
        ui.cx
            .read(|cx| view.read(cx).text())
            .contains("![uploading image…]()")
    );
    ui.wait("pasted link", |cx| {
        view.read(cx).text().contains("](../attachments/pasted-")
    })
    .await;
    assert!(!ui.cx.read(|cx| view.read(cx).text()).contains("uploading"));
    let saved: Vec<_> = std::fs::read_dir(dir.join("attachments"))
        .unwrap()
        .flatten()
        .collect();
    assert_eq!(saved.len(), 1);
    assert_eq!(std::fs::read(saved[0].path()).unwrap(), vec![1, 2, 3]);

    // A Finder drop of an image copies it in and links it.
    let src = test_ui::temp_dir("editor-images-src");
    let outside = src.join("Diagram.png");
    std::fs::write(&outside, [9u8, 9]).unwrap();
    ui.cx.update(|_, cx| {
        pane.update(cx, |p, cx| {
            p.drop_external(std::slice::from_ref(&outside), cx)
        })
    });
    ui.wait("dropped link", |cx| {
        view.read(cx)
            .text()
            .contains("![Diagram](../attachments/Diagram.png)")
    })
    .await;
    assert!(dir.join("attachments/Diagram.png").exists());

    // Without a vault nothing happens.
    let lone_dir = test_ui::temp_dir("editor-images-lone");
    let lone = lone_dir.join("l.md").to_string_lossy().to_string();
    std::fs::write(&lone, "y\n").unwrap();
    let (mut ui2, pane2, _) = open_pane(cx, &lone, PaneContext::default()).await;
    let v2 = ui2.cx.read(|cx| pane2.read(cx).view().unwrap().clone());
    ui2.cx.update(|_, cx| {
        v2.update(cx, |_, cx| {
            cx.emit(EditorEvent::PasteImage {
                mime: "image/png".into(),
                bytes: vec![1],
            })
        })
    });
    ui2.sleep(20).await;
    assert_eq!(ui2.cx.read(|cx| v2.read(cx).text()), "y\n");
    assert!(!lone_dir.join("attachments").exists());
    for d in [dir, src, lone_dir] {
        let _ = std::fs::remove_dir_all(d);
    }
}

#[gpui_kit::test]
async fn editor_window_pane_keeps_the_inline_command_line(cx: &mut TestAppContext) {
    cx.update(test_ui::init_shell);
    let dir = test_ui::temp_dir("editor-window");
    let path = dir.join("w.md").to_string_lossy().to_string();
    std::fs::write(&path, "w\n").unwrap();
    let (mut ui, pane, events) = open_pane(
        cx,
        &path,
        PaneContext {
            root: None,
            in_window: true,
        },
    )
    .await;
    let o = ui
        .cx
        .read(|cx| pane.read(cx).view().unwrap().read(cx).options().clone());
    assert!(!o.command_line_via_host && o.mode_chip);
    ui.press(":");
    assert!(
        !events
            .borrow()
            .iter()
            .any(|e| matches!(e, PaneEvent::OpenCommandPalette(_)))
    );
    assert!(
        ui.cx
            .read(|cx| pane.read(cx).view().unwrap().read(cx).prompt().is_some()),
        "inline `:` bar"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// The pane inside a real workspace: vault index, Mod-click wikilinks, tags,
/// relative links, mermaid dock, tab-switch flush, export of the unsaved
/// buffer, Helix externals and `:q`.
#[gpui_kit::test]
async fn workspace_navigation_mermaid_export_and_tab_flush(cx: &mut TestAppContext) {
    cx.update(test_ui::init_shell);
    let dir = test_ui::temp_dir("editor-ws");
    let root = dir.to_string_lossy().to_string();
    let a = format!("{root}/a.md");
    let b = format!("{root}/b.md");
    std::fs::write(
        &a,
        "start\nsee [[b]] and #todo\n\n```mermaid\ngraph TD\nA-->B\n```\n",
    )
    .unwrap();
    std::fs::write(&b, "# B\n").unwrap();
    let (mut ui, ws) = test_ui::open_workspace(cx);
    let (r, a2) = (root.clone(), a.clone());
    ui.cx.update(|window, cx| {
        ws.update(cx, |ws, cx| {
            ws.set_root(Some(r), window, cx);
            ws.open_file(a2, None, window, cx);
        })
    });
    let mounted = |ws: &Entity<crate::workspace::Workspace>, cx: &mut App| {
        ws.read(cx)
            .active_editor()
            .is_some_and(|p| p.read(cx).view().is_some())
    };
    ui.wait("a mounted", |cx| mounted(&ws, cx)).await;
    let view_a = ui.cx.read(|cx| {
        ws.read(cx)
            .active_editor()
            .unwrap()
            .read(cx)
            .view()
            .unwrap()
            .clone()
    });
    ui.wait("vault indexed", |cx| {
        view_a
            .read(cx)
            .options()
            .vault_files
            .iter()
            .any(|f| f.ends_with("b.md"))
    })
    .await;
    assert!(view_a.read_with(&ui.cx, |v, _| {
        v.options().all_tags.iter().any(|t| t == "todo")
    }));

    // ---- mermaid "Preview" → dock, tracked block ---------------------------
    ui.cx
        .update(|_, cx| view_a.update(cx, |v, cx| v.open_mermaid(27, cx)));
    ui.frame();
    let key = ui
        .cx
        .read(|cx| ws.read(cx).mermaid_dock_key())
        .expect("mermaid docked");
    assert!(ui.cx.update(|_, cx| crate::mermaid::is_tracked(&key, cx)));

    // ---- export sees the unsaved buffer ------------------------------------
    ui.cx
        .update(|_, cx| view_a.update(cx, |v, cx| v.handle_keys("ggiEXPORTED <Esc>", cx)));
    let a3 = a.clone();
    let content = ui
        .cx
        .update(|_, cx| crate::export::note_content(&a3, cx))
        .await
        .unwrap();
    assert!(content.starts_with("EXPORTED start"), "{content}");

    // ---- Mod-click a wikilink → opens b; a is flushed ---------------------
    ui.frame();
    let hits = ui.cx.read(|cx| view_a.read(cx).debug_hits());
    let at = hits
        .iter()
        .find(|(n, _)| n.starts_with("Wikilink"))
        .expect("wikilink hit")
        .1
        .center();
    ui.cx
        .simulate_mouse_move(at, None, Modifiers::secondary_key());
    ui.frame();
    ui.cx.simulate_click(at, Modifiers::secondary_key());
    ui.cx.run_until_parked();
    ui.frame();
    assert_eq!(
        ui.cx.read(|cx| ws.read(cx).tabs().active.clone()),
        Some(b.clone())
    );
    assert!(
        disk(&a).starts_with("EXPORTED start"),
        "flushed on tab switch"
    );
    drop(view_a);
    ui.wait("b mounted", |cx| mounted(&ws, cx)).await;
    assert!(
        !ui.cx.update(|_, cx| crate::mermaid::is_tracked(&key, cx)),
        "previews of a closed pane stop tracking"
    );

    // ---- Helix `gp` → previous tab -----------------------------------------
    let fh = ui.cx.read(|cx| {
        ws.read(cx)
            .active_editor()
            .unwrap()
            .read(cx)
            .focus_handle(cx)
    });
    ui.cx.update(|window, cx| window.focus(&fh, cx));
    ui.press("g p");
    assert_eq!(
        ui.cx.read(|cx| ws.read(cx).tabs().active.clone()),
        Some(a.clone())
    );
    ui.wait("a again", |cx| mounted(&ws, cx)).await;
    let view_a = ui.cx.read(|cx| {
        ws.read(cx)
            .active_editor()
            .unwrap()
            .read(cx)
            .view()
            .unwrap()
            .clone()
    });

    // ---- relative markdown link → b ---------------------------------------
    ui.cx
        .update(|_, cx| view_a.update(cx, |_, cx| cx.emit(EditorEvent::OpenLink("./b.md".into()))));
    ui.frame();
    assert_eq!(
        ui.cx.read(|cx| ws.read(cx).tabs().active.clone()),
        Some(b.clone())
    );
    ui.wait("b mounted 2", |cx| mounted(&ws, cx)).await;
    let view_b = ui.cx.read(|cx| {
        ws.read(cx)
            .active_editor()
            .unwrap()
            .read(cx)
            .view()
            .unwrap()
            .clone()
    });

    // ---- tag click → tags view --------------------------------------------
    ui.cx
        .update(|_, cx| view_b.update(cx, |_, cx| cx.emit(EditorEvent::TagClicked("todo".into()))));
    ui.frame();
    assert_eq!(ui.cx.read(|cx| ws.read(cx).main_view()), MainViewKind::Tags);
    ui.cx
        .update(|window, cx| ws.update(cx, |ws, cx| ws.set_view(MainViewKind::Editor, window, cx)));

    // ---- `:q` closes the tab ----------------------------------------------
    ui.cx
        .update(|_, cx| view_b.update(cx, |v, cx| v.execute_command("q", cx)));
    ui.frame();
    assert!(
        !ui.cx
            .read(|cx| ws.read(cx).tabs().tabs.iter().any(|t| t.path == b)),
        "`:q` closed b"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn image_upload_placeholder_and_failure_comment() {
    use super::{ImageSource, save_all, upload_placeholder};
    assert_eq!(upload_placeholder(1), "![uploading image…]()");
    assert_eq!(upload_placeholder(3), "![uploading 3 images…]()");
    let dir = test_ui::temp_dir("editor-images-fail");
    // `attachments` is a file: saving fails; the rest is skipped.
    let blocker = dir.join("attachments");
    std::fs::write(&blocker, "x").unwrap();
    let now = chrono::Local::now().naive_local();
    let links = save_all(
        vec![
            ImageSource::Bytes(String::new(), "image/png".into(), vec![1]),
            ImageSource::Bytes(String::new(), "image/png".into(), vec![2]),
        ],
        &blocker.to_string_lossy(),
        &dir.join("n.md").to_string_lossy(),
        &now,
    );
    assert_eq!(links.len(), 1, "{links:?}");
    assert!(
        links[0].starts_with("<!-- image upload failed: ") && links[0].ends_with(" -->"),
        "{links:?}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[gpui_kit::test]
async fn removed_upload_placeholder_inserts_nothing(cx: &mut TestAppContext) {
    cx.update(test_ui::init_shell);
    let dir = test_ui::temp_dir("editor-images-removed");
    let root = dir.to_string_lossy().to_string();
    let path = dir.join("n.md").to_string_lossy().to_string();
    std::fs::write(&path, "x\n").unwrap();
    let (mut ui, pane, _) = open_pane(
        cx,
        &path,
        PaneContext {
            root: Some(root),
            ..Default::default()
        },
    )
    .await;
    let view = ui.cx.read(|cx| pane.read(cx).view().unwrap().clone());
    ui.cx.update(|_, cx| {
        view.update(cx, |_, cx| {
            cx.emit(EditorEvent::PasteImage {
                mime: "image/png".into(),
                bytes: vec![1],
            })
        })
    });
    assert!(ui.cx.read(|cx| view.read(cx).text()).contains("uploading"));
    // The user deletes the placeholder before the save finishes.
    ui.cx
        .update(|_, cx| view.update(cx, |v, cx| v.set_text("x\n", cx)));
    ui.sleep(50).await;
    assert_eq!(ui.cx.read(|cx| view.read(cx).text()), "x\n");
    let _ = std::fs::remove_dir_all(&dir);
}

#[gpui_kit::test]
async fn reload_to_binary_or_missing_file_switches_the_view(cx: &mut TestAppContext) {
    cx.update(test_ui::init_shell);
    let dir = test_ui::temp_dir("editor-reload-kind");
    let path = dir.join("n.md").to_string_lossy().to_string();
    std::fs::write(&path, "x\n").unwrap();
    let (mut ui, pane, _) = open_pane(cx, &path, PaneContext::default()).await;
    // Rewritten as binary: the status screen replaces the editor.
    std::fs::write(&path, [0u8, 1, 2, 0, 255, 0]).unwrap();
    let p = path.clone();
    ui.cx.update(|_, cx| {
        bus::emit(
            cx,
            AppEvent::Core(CoreEvent::FileWritten {
                path: p,
                source: Some("x".into()),
            }),
        )
    });
    ui.wait("binary", |cx| {
        matches!(
            pane.read(cx).state,
            State::NotText {
                too_large: false,
                ..
            }
        )
    })
    .await;
    assert!(ui.cx.read(|cx| pane.read(cx).view().is_none()));

    // A pane on a file that then disappears shows the error screen.
    let other = dir.join("m.md").to_string_lossy().to_string();
    std::fs::write(&other, "y\n").unwrap();
    let (mut ui2, pane2, _) = open_pane(cx, &other, PaneContext::default()).await;
    std::fs::remove_file(&other).unwrap();
    let p = other.clone();
    ui2.cx.update(|_, cx| {
        bus::emit(
            cx,
            AppEvent::Core(CoreEvent::FileWritten {
                path: p,
                source: Some("x".into()),
            }),
        )
    });
    ui2.wait("error", |cx| {
        matches!(pane2.read(cx).state, State::Failed(_))
    })
    .await;
    let _ = std::fs::remove_dir_all(&dir);
}

#[gpui_kit::test]
async fn wikilink_heading_completion_reads_the_other_note(cx: &mut TestAppContext) {
    cx.update(test_ui::init_shell);
    let dir = test_ui::temp_dir("editor-heading-complete");
    let root = dir.to_string_lossy().to_string();
    std::fs::write(dir.join("Other.md"), "# Alpha\n\ntext\n\n## Beta\n").unwrap();
    let path = dir.join("n.md").to_string_lossy().to_string();
    std::fs::write(&path, "\n").unwrap();
    let (mut ui, pane, _) = open_pane(
        cx,
        &path,
        PaneContext {
            root: Some(root),
            ..Default::default()
        },
    )
    .await;
    ui.wait("vault scanned", |cx| {
        pane.read(cx)
            .vault
            .0
            .iter()
            .any(|f| f.ends_with("Other.md"))
    })
    .await;
    ui.press("i");
    ui.type_text("[[Other#");
    let labels = ui
        .cx
        .read(|cx| pane.read(cx).view().unwrap().read(cx).completion_labels())
        .unwrap_or_default();
    assert!(labels.iter().any(|l| l.contains("Alpha")), "{labels:?}");
    assert!(labels.iter().any(|l| l.contains("Beta")), "{labels:?}");
    let _ = std::fs::remove_dir_all(&dir);
}
