//! Vim mode (`EngineKind::Vim`, fuchico-vim) through real keystrokes.

mod common;

use std::cell::RefCell;
use std::rc::Rc;

use common::*;
use fuchico_editor::{EditorEvent, EditorOptions, EngineKind, VimGlobals};
use gpui_kit::TestAppContext;

fn vim() -> EditorOptions {
    EditorOptions {
        engine: EngineKind::Vim,
        ..EditorOptions::default()
    }
}

#[gpui_kit::test]
async fn operators_and_visual_delete(cx: &mut TestAppContext) {
    let mut ui = open(cx, "one two three\nfour five\n", vim());
    assert!(ui.read(|v, _| v.engine().is_vim()));
    assert_eq!(ui.read(|v, _| v.mode_label()), "NORMAL");
    ui.press("d w");
    assert_eq!(ui.text(), "two three\nfour five\n");
    ui.press("c i w");
    assert_eq!(ui.read(|v, _| v.mode_label()), "INSERT");
    ui.type_text("2");
    ui.press("escape");
    assert_eq!(ui.text(), "2 three\nfour five\n");
    assert_eq!(ui.read(|v, _| v.mode_label()), "NORMAL");
    // visual `v` + `d`
    ui.press("j 0 v e");
    assert_eq!(ui.read(|v, _| v.mode_label()), "VISUAL");
    ui.press("d");
    assert_eq!(ui.text(), "2 three\n five\n");
    assert_eq!(ui.read(|v, _| v.mode_label()), "NORMAL");
    // `u` undoes the visual delete, Ctrl-r redoes it
    ui.press("u");
    assert_eq!(ui.text(), "2 three\nfour five\n");
    ui.press("ctrl-r");
    assert_eq!(ui.text(), "2 three\n five\n");
    // pending operator shown; no Helix mode chip in Vim
    ui.press("d");
    assert_eq!(ui.read(|v, _| v.pending_status()), "d");
    ui.press("escape");
    assert!(
        !ui.read(|v, _| v.debug_hits())
            .iter()
            .any(|(n, _)| n.contains("chip"))
    );
}

#[gpui_kit::test]
async fn substitute_via_the_prompt(cx: &mut TestAppContext) {
    let mut ui = open(cx, "a banana\nkeep a\n", vim());
    ui.press(":");
    assert_eq!(
        ui.read(|v, _| v.prompt()),
        Some((":".into(), String::new()))
    );
    ui.type_text("s/a/b/g");
    assert_eq!(
        ui.read(|v, _| v.prompt()),
        Some((":".into(), "s/a/b/g".into()))
    );
    ui.press("enter");
    assert_eq!(ui.read(|v, _| v.prompt()), None);
    assert_eq!(ui.text(), "b bbnbnb\nkeep a\n");
    // history: Up recalls the last command
    ui.press(": up");
    assert_eq!(
        ui.read(|v, _| v.prompt()),
        Some((":".into(), "s/a/b/g".into()))
    );
    ui.press("escape");
    // `:` in visual mode starts at `'<,'>`; Backspace edits, Escape closes
    ui.press("j V :");
    assert_eq!(
        ui.read(|v, _| v.prompt()),
        Some((":".into(), "'<,'>".into()))
    );
    ui.press("backspace");
    assert_eq!(
        ui.read(|v, _| v.prompt()),
        Some((":".into(), "'<,'".into()))
    );
    ui.press("escape");
    assert_eq!(ui.read(|v, _| v.prompt()), None);
    // errors are cm-vim notifications
    ui.press(":");
    ui.type_text("bogus");
    ui.press("enter");
    let msgs: Vec<String> = ui
        .events()
        .iter()
        .filter_map(|e| match e {
            EditorEvent::Message { text, .. } => Some(text.clone()),
            _ => None,
        })
        .collect();
    assert!(msgs.iter().any(|m| m.contains("bogus")), "{msgs:?}");
    let panel = ui.read(|v, _| v.debug_vim_panel()).unwrap_or_default();
    assert!(panel.contains("bogus"), "{panel}");
    // a newer dialog (the prompt) replaces the notification
    ui.press(":");
    assert_eq!(ui.read(|v, _| v.debug_vim_panel()), None);
    ui.press("escape");
}

#[gpui_kit::test]
async fn search_and_next(cx: &mut TestAppContext) {
    let mut ui = open(cx, "x foo y\nfoo z\nfoo\n", vim());
    ui.press("/");
    assert_eq!(
        ui.read(|v, _| v.prompt()),
        Some(("/".into(), String::new()))
    );
    ui.type_text("foo");
    // incremental `hlsearch` overlay while typing
    let marks = ui.read(|v, _| v.debug_search_marks());
    assert_eq!(marks.len(), 3, "{marks:?}");
    ui.press("enter");
    assert_eq!(ui.read(|v, _| v.prompt()), None);
    assert_eq!(ui.head(), 2);
    ui.press("n");
    assert_eq!(ui.head(), 8);
    ui.press("n");
    assert_eq!(ui.head(), 14);
    ui.press("shift-n");
    assert_eq!(ui.head(), 8);
    // the match highlight stays after the search (`hlsearch`)
    assert_eq!(ui.read(|v, _| v.debug_search_marks()).len(), 3);
    // `?` searches backwards; cancelling a search prompt clears the
    // highlight (cm-vim `onPromptKeyDown` Escape)
    ui.press("?");
    assert_eq!(
        ui.read(|v, _| v.prompt()),
        Some(("?".into(), String::new()))
    );
    ui.press("escape");
    assert!(ui.read(|v, _| v.debug_search_marks()).is_empty());
}

#[gpui_kit::test]
async fn write_emits_save(cx: &mut TestAppContext) {
    let mut ui = open(cx, "text\n", vim());
    ui.press("x");
    ui.clear_events();
    ui.press(":");
    ui.type_text("w");
    ui.press("enter");
    assert!(ui.events().iter().any(|e| matches!(e, EditorEvent::Save)));
    // `:q` is not wired (the original defines no Vim ex commands)
    ui.clear_events();
    ui.press(":");
    ui.type_text("q");
    ui.press("enter");
    assert!(!ui.events().iter().any(|e| matches!(e, EditorEvent::Quit)));
}

#[gpui_kit::test]
async fn registers_shared_between_views(cx: &mut TestAppContext) {
    let shared = Rc::new(RefCell::new(VimGlobals::default()));
    let mut a = open(cx, "shared line\n", vim());
    let g = shared.clone();
    a.update(|v, _, _| v.set_vim_globals(Some(g)));
    a.press("\" a y y");
    a.press("y w");

    let mut b = open(cx, "other\n", vim());
    let g = shared.clone();
    b.update(|v, _, _| v.set_vim_globals(Some(g)));
    b.press("\" a p");
    assert_eq!(b.text(), "other\nshared line\n");
    b.press("shift-p");
    assert_eq!(b.text(), "other\nshared shared line\n");
    // search is shared too
    a.press("/");
    a.type_text("line");
    a.press("enter");
    b.press("g g n");
    assert_eq!(b.head(), 20);
}

#[gpui_kit::test]
async fn switching_engines_keeps_the_text(cx: &mut TestAppContext) {
    let mut ui = open(cx, "hello world\n", EditorOptions::default());
    ui.press("w");
    ui.press("d");
    assert_eq!(ui.text(), " world\n");
    let head = ui.head();
    ui.update(|v, _, cx| v.set_options(vim(), cx));
    assert!(ui.read(|v, _| v.engine().is_vim()));
    assert_eq!(ui.text(), " world\n");
    assert_eq!(ui.head(), head);
    assert!(ui.read(|v, _| v.is_dirty()));
    ui.press("0 d w");
    assert_eq!(ui.text(), "world\n");
    ui.update(|v, _, cx| v.set_options(EditorOptions::default(), cx));
    assert!(!ui.read(|v, _| v.engine().is_vim()));
    assert_eq!(ui.text(), "world\n");
    ui.press("i");
    ui.type_text("hx");
    assert_eq!(ui.read(|v, _| v.mode_label()), "INS");
    assert_eq!(ui.text(), "hxworld\n");
    // Normal <-> Vim as well
    ui.press("escape");
    ui.update(|v, _, cx| {
        v.set_options(
            EditorOptions {
                engine: EngineKind::Normal,
                ..EditorOptions::default()
            },
            cx,
        )
    });
    ui.update(|v, _, cx| v.set_options(vim(), cx));
    assert_eq!(ui.read(|v, _| v.mode_label()), "NORMAL");
    assert_eq!(ui.text(), "hxworld\n");
}

#[gpui_kit::test]
async fn cursor_shapes_and_panel(cx: &mut TestAppContext) {
    let mut ui = open(cx, "abc\n", vim());
    let h = |ui: &mut Ui| {
        let c = ui.read(|v, _| v.debug_cursors());
        assert_eq!(c.len(), 1, "{c:?}");
        c[0].0.size.height
    };
    let full = h(&mut ui);
    ui.press("d");
    let half = h(&mut ui);
    assert!(
        (half - full / 2.).abs() < gpui_kit::px(0.5),
        "{half:?} {full:?}"
    );
    ui.press("escape");
    ui.press("shift-r");
    let under = h(&mut ui);
    assert!(under < half);
    ui.press("escape");
    ui.press("i");
    let bar = ui.read(|v, _| v.debug_cursors());
    assert!(bar[0].0.size.width < gpui_kit::px(2.));
    ui.press("escape");
    // `q a` shows `recording @a` in the panel
    ui.press("q a");
    assert!(
        ui.read(|v, _| v.debug_vim_panel())
            .is_some_and(|t| t == "recording @a")
    );
    ui.press("q");
    assert_eq!(ui.read(|v, _| v.debug_vim_panel()), None);
}
