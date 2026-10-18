//! Headless tests of the drawing pane: load, draw with real pointer/key
//! input, dirty tab, debounced autosave (+ `fs:file-written`), reload on
//! external change unless there are unsaved edits, malformed files.

use std::cell::RefCell;
use std::rc::Rc;

use fuchico_core::events::CoreEvent;
use gpui_kit::{AppContext, Entity, Point, TestAppContext, px};

use super::{ExcalidrawPane, create_drawing};
use crate::bus::{self, AppEvent};
use crate::snapshot::SnapTheme;
use crate::test_ui;
use crate::workspace::pane::{PaneContext, PaneEvent};

fn rects(pane: &Entity<ExcalidrawPane>, cx: &gpui_kit::App) -> usize {
    pane.read(cx).view().map_or(0, |v| {
        v.read(cx)
            .to_json()
            .matches("\"type\": \"rectangle\"")
            .count()
            + v.read(cx)
                .to_json()
                .matches("\"type\":\"rectangle\"")
                .count()
    })
}

fn on_disk(path: &str) -> usize {
    let s = std::fs::read_to_string(path).unwrap();
    s.matches("\"type\": \"rectangle\"").count() + s.matches("\"type\":\"rectangle\"").count()
}

#[gpui_kit::test]
async fn draw_autosave_dirty_and_external_reload(cx: &mut TestAppContext) {
    cx.update(test_ui::init);
    let dir = test_ui::temp_dir("excalidraw");
    let dir_s = dir.to_string_lossy().to_string();
    let rec = test_ui::record(cx, |e| {
        matches!(e, AppEvent::Core(CoreEvent::FileWritten { .. }))
    });

    // "New drawing" writes the template (source "template").
    let path = create_drawing(&dir_s, "Sketch").unwrap();
    assert!(path.ends_with("/Sketch.excalidraw"));
    assert!(
        create_drawing(&dir_s, "Sketch").is_err(),
        "existing files are not overwritten"
    );
    let template = std::fs::read_to_string(&path).unwrap();
    assert!(
        template.contains("\"type\": \"excalidraw\"")
            || template.contains("\"type\":\"excalidraw\"")
    );

    let dirty: Rc<RefCell<Vec<bool>>> = Rc::default();
    let p = path.clone();
    let (mut ui, pane) = test_ui::open(cx, (1000., 700.), move |window, cx| {
        ExcalidrawPane::new(
            p,
            PaneContext {
                root: None,
                ..Default::default()
            },
            window,
            cx,
        )
    });
    let sink = dirty.clone();
    let _sub = ui.cx.update(|_, cx| {
        cx.subscribe(&pane, move |_, ev: &PaneEvent, _| {
            if let PaneEvent::DirtyChanged(d) = ev {
                sink.borrow_mut().push(*d)
            }
        })
    });
    ui.wait("drawing loaded", |cx| pane.read(cx).view().is_some())
        .await;
    ui.sleep(10).await;
    assert_eq!(
        ui.cx.read(|cx| rec.read(cx).seen.clone()),
        vec![AppEvent::Core(CoreEvent::FileWritten {
            path: path.clone(),
            source: Some("template".into())
        })]
    );

    // ---- draw a rectangle: "r" then a real drag on the canvas ------------
    let fh = ui
        .cx
        .read(|cx| gpui_kit::Focusable::focus_handle(pane.read(cx), cx));
    ui.cx.update(|window, cx| window.focus(&fh, cx));
    ui.press("r");
    ui.drag(
        Point {
            x: px(300.),
            y: px(300.),
        },
        Point {
            x: px(500.),
            y: px(450.),
        },
    );
    assert_eq!(ui.cx.read(|cx| rects(&pane, cx)), 1);
    assert_eq!(dirty.borrow().as_slice(), &[true], "tab marked dirty");
    assert!(ui.cx.read(|cx| pane.read(cx).dirty));
    assert_eq!(on_disk(&path), 0, "not saved before the debounce");

    // ---- autosave after the 1 s debounce (source "editor") ---------------
    ui.sleep(1100).await;
    ui.wait("saved", |cx| !pane.read(cx).dirty).await;
    assert_eq!(on_disk(&path), 1);
    assert_eq!(dirty.borrow().as_slice(), &[true, false]);
    assert!(ui.cx.read(|cx| {
        rec.read(cx)
            .seen
            .contains(&AppEvent::Core(CoreEvent::FileWritten {
                path: path.clone(),
                source: Some("editor".into()),
            }))
    }));
    // Our own write does not reload anything away.
    ui.sleep(10).await;
    assert_eq!(ui.cx.read(|cx| rects(&pane, cx)), 1);

    // ---- external change → reload ---------------------------------------
    std::fs::write(&path, &template).unwrap();
    ui.cx.update(|_, cx| {
        bus::emit(
            cx,
            AppEvent::Core(CoreEvent::FsChanged {
                dir: dir_s.clone(),
                from: None,
            }),
        )
    });
    ui.wait("reloaded", |cx| rects(&pane, cx) == 0).await;
    assert!(!ui.cx.read(|cx| pane.read(cx).dirty));

    // ---- unsaved edits win over an external change -----------------------
    ui.press("r");
    ui.drag(
        Point {
            x: px(300.),
            y: px(300.),
        },
        Point {
            x: px(500.),
            y: px(450.),
        },
    );
    assert_eq!(ui.cx.read(|cx| rects(&pane, cx)), 1);
    assert!(ui.cx.read(|cx| pane.read(cx).dirty));
    std::fs::write(&path, &template).unwrap();
    ui.cx.update(|_, cx| {
        bus::emit(
            cx,
            AppEvent::Core(CoreEvent::FsChanged {
                dir: dir_s.clone(),
                from: None,
            }),
        )
    });
    ui.sleep(20).await;
    assert_eq!(
        ui.cx.read(|cx| rects(&pane, cx)),
        1,
        "dirty drawing is not replaced"
    );

    // ---- Cmd+S path: PaneItem::save flushes now ---------------------------
    ui.cx.update(|window, cx| {
        pane.update(cx, |p, cx| {
            crate::workspace::pane::PaneItem::save(p, window, cx)
        })
    });
    let p2 = path.clone();
    ui.wait("flushed", move |cx| {
        !pane.read(cx).dirty && on_disk(&p2) == 1
    })
    .await;

    // ---- malformed file -------------------------------------------------
    let bad = dir.join("bad.excalidraw").to_string_lossy().to_string();
    std::fs::write(&bad, "{ not json").unwrap();
    let (mut ui2, bad_pane) = test_ui::open(cx, (600., 400.), move |window, cx| {
        ExcalidrawPane::new(
            bad,
            PaneContext {
                root: None,
                ..Default::default()
            },
            window,
            cx,
        )
    });
    ui2.wait("loaded", |cx| bad_pane.read(cx).view().is_some())
        .await;
    assert!(
        ui2.cx
            .read(|cx| bad_pane.read(cx).view().unwrap().read(cx).error().is_some())
    );
}

#[test]
fn snapshots() {
    let dir = test_ui::temp_dir("excalidraw-snap");
    let path = dir.join("Snap.excalidraw");
    std::fs::write(&path, SAMPLE).unwrap();
    let p = path.to_string_lossy().to_string();
    for dark in [false, true] {
        let name = if dark {
            "excalidraw-dark"
        } else {
            "excalidraw-light"
        };
        let p = p.clone();
        crate::snapshot::snapshot(
            name,
            gpui_kit::size(px(900.), px(600.)),
            SnapTheme::default_mode(dark),
            move |window, cx| {
                cx.new(|cx| {
                    ExcalidrawPane::new(
                        p,
                        PaneContext {
                            root: None,
                            ..Default::default()
                        },
                        window,
                        cx,
                    )
                })
            },
        );
    }
}

const SAMPLE: &str = r##"{"type":"excalidraw","version":2,"source":"test","elements":[
{"id":"r1","type":"rectangle","x":120,"y":140,"width":220,"height":120,"angle":0,"strokeColor":"#1e1e1e","backgroundColor":"#a5d8ff","fillStyle":"hachure","strokeWidth":2,"strokeStyle":"solid","roughness":1,"opacity":100,"groupIds":[],"frameId":null,"roundness":{"type":3},"seed":11,"version":1,"versionNonce":1,"isDeleted":false,"boundElements":null,"updated":1,"link":null,"locked":false},
{"id":"e1","type":"ellipse","x":440,"y":160,"width":160,"height":100,"angle":0,"strokeColor":"#e03131","backgroundColor":"transparent","fillStyle":"solid","strokeWidth":2,"strokeStyle":"solid","roughness":1,"opacity":100,"groupIds":[],"frameId":null,"roundness":null,"seed":22,"version":1,"versionNonce":2,"isDeleted":false,"boundElements":null,"updated":1,"link":null,"locked":false},
{"id":"t1","type":"text","x":150,"y":300,"width":200,"height":25,"angle":0,"strokeColor":"#1e1e1e","backgroundColor":"transparent","fillStyle":"solid","strokeWidth":2,"strokeStyle":"solid","roughness":1,"opacity":100,"groupIds":[],"frameId":null,"roundness":null,"seed":33,"version":1,"versionNonce":3,"isDeleted":false,"boundElements":null,"updated":1,"link":null,"locked":false,"text":"Hello drawing","fontSize":20,"fontFamily":1,"textAlign":"left","verticalAlign":"top","containerId":null,"originalText":"Hello drawing","lineHeight":1.25,"baseline":18}
],"appState":{"viewBackgroundColor":"#ffffff","gridSize":null},"files":{}}"##;

/// Side-by-side material for the parity round: the editor in the states
/// the original's stock Excalidraw was captured in (same scene, same
/// 1280×800 viewport). Writes `target/snapshots/excal-*.png`.
#[test]
fn snapshots_compare_with_stock() {
    use fuchico_excalidraw::{DebugUi, ExcalidrawStyle, ExcalidrawView, Tool};
    let lib = test_ui::temp_dir("excal-cmp-lib").join("library.excalidrawlib");
    // SAFETY: test-only; read once when each view is created.
    unsafe { std::env::set_var(fuchico_excalidraw::library::LIBRARY_ENV, &lib) };
    type Prep = fn(&mut ExcalidrawView, &mut gpui_kit::Window, &mut gpui_kit::Context<ExcalidrawView>);
    let cases: &[(&str, bool, Prep)] = &[
        ("default", false, |_, _, _| {}),
        ("default-dark", true, |_, _, _| {}),
        ("menu", false, |v, _, cx| v.debug_open(DebugUi::Menu, cx)),
        ("selected-rect", false, |v, _, cx| v.select_ids(&["el1"], cx)),
        ("selected-arrow", false, |v, _, cx| v.select_ids(&["el4"], cx)),
        ("selected-text", false, |v, _, cx| v.select_ids(&["el5"], cx)),
        ("tool-rect", false, |v, w, cx| v.set_tool(Tool::Rectangle, w, cx)),
        ("help", false, |v, _, cx| v.debug_open(DebugUi::Help, cx)),
        ("library", false, |v, _, cx| v.debug_open(DebugUi::Library, cx)),
        ("ctx-canvas", false, |v, _, cx| v.debug_open(DebugUi::ContextMenu(900., 600.), cx)),
        ("ctx-element", false, |v, _, cx| {
            v.select_ids(&["el1"], cx);
            v.debug_open(DebugUi::ContextMenu(200., 175.), cx)
        }),
        ("export", false, |v, _, cx| v.debug_open(DebugUi::Export, cx)),
        ("picker", false, |v, _, cx| {
            v.select_ids(&["el1"], cx);
            v.debug_open(DebugUi::StrokePicker, cx)
        }),
    ];
    for (name, dark, prep) in cases {
        let prep = *prep;
        let dark = *dark;
        crate::snapshot::snapshot_with(
            &format!("excal-{name}"),
            gpui_kit::size(px(1280.), px(800.)),
            SnapTheme::default_mode(dark),
            move |window, cx| {
                let style: fuchico_excalidraw::StyleFn = Rc::new(move |_| {
                    if dark {
                        ExcalidrawStyle::dark()
                    } else {
                        ExcalidrawStyle::light()
                    }
                });
                cx.new(|cx| {
                    ExcalidrawView::new(COMPARE, style, Rc::new(|_, _| Ok(())), window, cx)
                })
            },
            move |view, window, cx| view.update(cx, |v, cx| prep(v, window, cx)),
        );
    }
}

const COMPARE: &str = include_str!("../../../../crates/fuchico-excalidraw/tests/fixtures/compare.excalidraw");
