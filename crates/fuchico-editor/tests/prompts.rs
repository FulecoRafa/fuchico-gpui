//! `:` command line, search prompt, Mod-f find bar, outline, completion
//! popup and the mode chip / status.

mod common;

use common::*;
use fuchico_editor::{EditorEvent, EditorOptions};
use gpui_kit::TestAppContext;

#[gpui_kit::test]
async fn command_line_completions_history_and_save(cx: &mut TestAppContext) {
    let mut ui = open(cx, "hello\n", EditorOptions::default());
    ui.press(":");
    assert_eq!(
        ui.read(|v, _| v.prompt()),
        Some((":".into(), String::new()))
    );
    assert!(
        !ui.read(|v, _| v.prompt_completions()).is_empty(),
        "commands listed"
    );
    ui.type_text("wri");
    assert_eq!(ui.read(|v, _| v.prompt()).unwrap().1, "wri");
    let comps = ui.read(|v, _| v.prompt_completions());
    assert!(comps.iter().any(|c| c.starts_with("write")), "{comps:?}");
    // Tab cycles completions into the input.
    ui.press("tab");
    let v = ui.read(|v, _| v.prompt()).unwrap().1;
    assert!(v.starts_with("write"), "{v}");
    // Backspace to empty, then type `w` and accept: Save.
    ui.press("ctrl-u");
    ui.type_text("w");
    ui.clear_events();
    ui.press("enter");
    assert_eq!(ui.read(|v, _| v.prompt()), None);
    assert!(
        ui.events().iter().any(|e| matches!(e, EditorEvent::Save)),
        "{:?}",
        ui.events()
    );
    // History: `:` + Up recalls `w`.
    ui.press(":");
    ui.press("up");
    assert_eq!(ui.read(|v, _| v.prompt()).unwrap().1, "w");
    ui.press("escape");
    assert_eq!(ui.read(|v, _| v.prompt()), None);
    // `:q` quits.
    ui.press(":");
    ui.type_text("q");
    ui.press("enter");
    assert!(ui.events().iter().any(|e| matches!(e, EditorEvent::Quit)));
    // Mode chip / status: the mode label is shown.
    assert_eq!(ui.read(|v, _| v.mode_label()), "NOR");
}

#[gpui_kit::test]
async fn command_line_via_host(cx: &mut TestAppContext) {
    let opts = EditorOptions {
        command_line_via_host: true,
        ..Default::default()
    };
    let mut ui = open(cx, "hello\n", opts);
    ui.press(":");
    assert_eq!(ui.read(|v, _| v.prompt()), None);
    assert!(
        ui.events()
            .iter()
            .any(|e| matches!(e, EditorEvent::OpenCommandLine { .. }))
    );
    // The palette executes the typed line.
    ui.clear_events();
    ui.update(|v, _, cx| v.execute_command("w", cx));
    assert!(ui.events().iter().any(|e| matches!(e, EditorEvent::Save)));
}

#[gpui_kit::test]
async fn search_prompt_moves_to_match(cx: &mut TestAppContext) {
    let mut ui = open(cx, "alpha beta\ngamma beta\n", EditorOptions::default());
    ui.press("/");
    assert_eq!(ui.read(|v, _| v.prompt()).unwrap().0, "search:");
    ui.type_text("gam");
    ui.press("enter");
    let r = ui.read(|v, _| v.selection().main());
    assert_eq!((r.from(), r.to()), (11, 14));
    // `n` repeats.
    ui.press("/");
    ui.type_text("beta");
    ui.press("enter");
    assert_eq!(ui.read(|v, _| v.selection().main().from()), 17);
    ui.press("n");
    assert_eq!(ui.read(|v, _| v.selection().main().from()), 6);
    // Escape cancels and restores.
    ui.press("/");
    ui.type_text("zzz");
    ui.press("escape");
    assert_eq!(ui.read(|v, _| v.prompt()), None);
}

#[gpui_kit::test]
async fn find_bar_mod_f(cx: &mut TestAppContext) {
    let mut ui = open(cx, "one two one three one\n", EditorOptions::default());
    ui.press("cmd-f");
    assert_eq!(ui.read(|v, _| v.find_query()), Some((String::new(), 0, 0)));
    ui.type_text("one");
    assert_eq!(ui.read(|v, _| v.find_query()), Some(("one".into(), 0, 3)));
    let r = ui.read(|v, _| v.selection().main());
    assert_eq!((r.from(), r.to()), (0, 3));
    ui.press("enter");
    assert_eq!(ui.read(|v, _| v.selection().main().from()), 8);
    ui.press("enter");
    assert_eq!(ui.read(|v, _| v.selection().main().from()), 18);
    ui.press("shift-enter");
    assert_eq!(ui.read(|v, _| v.selection().main().from()), 8);
    ui.press("escape");
    assert_eq!(ui.read(|v, _| v.find_query()), None);
}

#[gpui_kit::test]
async fn outline_mod_o_filters_and_jumps(cx: &mut TestAppContext) {
    let text = "# Intro\nx\n## Setup\ny\n## Usage\nz\n# Appendix\n";
    let mut ui = open(cx, text, EditorOptions::default());
    ui.press("cmd-o");
    let items = ui.read(|v, _| v.outline_items()).unwrap();
    assert_eq!(items, vec!["Intro", "Setup", "Usage", "Appendix"]);
    ui.type_text("usa");
    assert_eq!(ui.read(|v, _| v.outline_items()).unwrap(), vec!["Usage"]);
    ui.press("enter");
    assert_eq!(ui.read(|v, _| v.outline_items()), None);
    assert_eq!(ui.read(|v, _| v.line_col().0), 5);
}

#[gpui_kit::test]
async fn completion_popup_wikilinks_and_tags(cx: &mut TestAppContext) {
    let opts = EditorOptions {
        vault_files: vec!["notes/Alpha.md".into(), "Beta.md".into()],
        all_tags: vec!["project".into(), "personal".into()],
        ..Default::default()
    };
    let mut ui = open(cx, "\n", opts);
    ui.press("i");
    ui.type_text("[[Al");
    let labels = ui
        .read(|v, _| v.completion_labels())
        .expect("completion open");
    assert!(labels.iter().any(|l| l.contains("Alpha")), "{labels:?}");
    ui.press("enter");
    assert!(ui.text().starts_with("[[Alpha"), "{:?}", ui.text());
    assert_eq!(ui.read(|v, _| v.completion_labels()), None);
    // Tags.
    ui.type_text(" #pro");
    let labels = ui
        .read(|v, _| v.completion_labels())
        .expect("tag completion");
    assert!(labels.iter().any(|l| l.contains("project")), "{labels:?}");
    ui.press("escape");
    assert_eq!(ui.read(|v, _| v.completion_labels()), None);
    assert_eq!(
        ui.read(|v, _| v.mode_label()),
        "INS",
        "escape closed only the popup"
    );
}

#[gpui_kit::test]
async fn pending_status_and_messages(cx: &mut TestAppContext) {
    let mut ui = open(cx, "a b c\n", EditorOptions::default());
    ui.press("3");
    assert_eq!(ui.read(|v, _| v.pending_status()), "3");
    // (`w` keeps the count: a ported codemirror-helix quirk.) Escape clears.
    ui.press("escape");
    assert_eq!(ui.read(|v, _| v.pending_status()), "");
    ui.press(":");
    ui.type_text("nosuchcommand");
    ui.press("enter");
    let m = ui.read(|v, _| v.message());
    assert!(matches!(m, Some((_, true))), "error message: {m:?}");
}
