//! Headless tests of the export flows: save dialog (default directory),
//! write with source "export", outcome dialogs, cancel, failure, the live
//! buffer provider and the non-markdown guard.

use fuchico_core::events::CoreEvent;
use gpui_kit::prelude::*;
use gpui_kit::{Context, TestAppContext, Window, div};

use super::{export_html, export_pdf, set_buffer_provider};
use crate::bus::AppEvent;
use crate::test_ui;

struct Blank;

impl Render for Blank {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full()
    }
}

const NOTE: &str = "---\ntags: [x]\n---\n# Trip plan\n\n- [ ] book train\n- [x] pack\n\n```mermaid\ngraph TD\n  A --> B\n```\n\n| a | b |\n|---|---|\n| 1 | 2 |\n";

#[gpui_kit::test]
async fn html_and_pdf_export_dialogs(cx: &mut TestAppContext) {
    cx.update(test_ui::init);
    let dir = test_ui::temp_dir("export");
    let note = dir.join("Trip.md");
    std::fs::write(&note, NOTE).unwrap();
    let note_s = note.to_string_lossy().to_string();
    let rec = test_ui::record(cx, |e| matches!(e, AppEvent::Core(CoreEvent::FileWritten { .. })));
    let (mut ui, _blank) = test_ui::open(cx, (800., 600.), |_, _| Blank);

    // ---- HTML: dialog defaults to the note's folder ----------------------
    let n = note_s.clone();
    ui.cx.update(|window, cx| export_html(&n, window, cx));
    ui.cx.run_until_parked();
    assert!(ui.cx.did_prompt_for_new_path());
    let d = dir.clone();
    // No extension typed: ".html" is added.
    ui.cx.simulate_new_path_selection(move |shown| {
        assert_eq!(shown, d.as_path());
        Some(shown.join("Trip"))
    });
    ui.cx.run_until_parked();
    let out = dir.join("Trip.html");
    let html = std::fs::read_to_string(&out).expect("html written");
    assert!(html.contains("<title>Trip</title>"), "{html}");
    assert!(html.contains("--background"), "theme variables inlined");
    assert!(html.contains("Trip plan"));
    let (msg, _) = ui.cx.pending_prompt().expect("outcome dialog");
    assert_eq!(msg, crate::t!(ExportSaved, path = out.display()));
    ui.cx.simulate_prompt_answer("OK");
    assert!(ui.cx.read(|cx| rec.read(cx).seen.contains(&AppEvent::Core(CoreEvent::FileWritten {
        path: out.to_string_lossy().into_owned(),
        source: Some("export".into()),
    }))));

    // ---- cancel: nothing written, no dialog -------------------------------
    std::fs::remove_file(&out).unwrap();
    let n = note_s.clone();
    ui.cx.update(|window, cx| export_html(&n, window, cx));
    ui.cx.run_until_parked();
    ui.cx.simulate_new_path_selection(|_| None);
    ui.cx.run_until_parked();
    assert!(!out.exists());
    assert!(!ui.cx.has_pending_prompt());

    // ---- failure: export.failed, critical --------------------------------
    let n = note_s.clone();
    ui.cx.update(|window, cx| export_html(&n, window, cx));
    ui.cx.run_until_parked();
    ui.cx.simulate_new_path_selection(|shown| Some(shown.join("missing-dir").join("x.html")));
    ui.cx.run_until_parked();
    let (msg, _) = ui.cx.pending_prompt().expect("failure dialog");
    let prefix = crate::t!(ExportFailed, message = "");
    assert!(msg.starts_with(prefix.trim_end()), "{msg}");
    ui.cx.simulate_prompt_answer("OK");

    // ---- the live (unsaved) buffer wins over the file --------------------
    let n = note_s.clone();
    ui.cx.update(|window, cx| {
        set_buffer_provider(cx, move |p, _| (p == n).then(|| "# Unsaved heading\n".to_string()));
        export_html(&note_s, window, cx)
    });
    ui.cx.run_until_parked();
    ui.cx.simulate_new_path_selection(|shown| Some(shown.join("live.html")));
    ui.cx.run_until_parked();
    let live = std::fs::read_to_string(dir.join("live.html")).unwrap();
    assert!(live.contains("Unsaved heading") && !live.contains("Trip plan"));
    ui.cx.simulate_prompt_answer("OK");
    ui.cx.update(|_, cx| set_buffer_provider(cx, |_, _| None));

    // ---- PDF: same dialog, PDF bytes, "OK" / "Open" outcome --------------
    let n = note.to_string_lossy().to_string();
    ui.cx.update(|window, cx| export_pdf(&n, window, cx));
    ui.cx.run_until_parked();
    ui.cx.simulate_new_path_selection(|shown| Some(shown.join("Trip")));
    ui.cx.run_until_parked();
    let pdf = std::fs::read(dir.join("Trip.pdf")).expect("pdf written");
    assert!(pdf.starts_with(b"%PDF-"));
    assert!(pdf.len() > 2000);
    let (msg, _) = ui.cx.pending_prompt().expect("outcome dialog");
    assert!(msg.contains("Trip.pdf"));
    // ("Open" would hand the file to the OS viewer, unavailable here.)
    ui.cx.simulate_prompt_answer("OK");
    assert!(ui.cx.read(|cx| rec.read(cx).seen.iter().any(|e| matches!(e,
        AppEvent::Core(CoreEvent::FileWritten { path, source: Some(s) }) if path.ends_with("Trip.pdf") && s == "export"))));

    // ---- non-markdown notes are refused before any dialog ----------------
    let drawing = dir.join("d.excalidraw").to_string_lossy().to_string();
    ui.cx.update(|window, cx| {
        export_html(&drawing, window, cx);
        export_pdf(&drawing, window, cx);
        super::print_pdf(&drawing, window, cx);
    });
    ui.cx.run_until_parked();
    assert!(!ui.cx.did_prompt_for_new_path());
    assert!(!ui.cx.has_pending_prompt());
}
