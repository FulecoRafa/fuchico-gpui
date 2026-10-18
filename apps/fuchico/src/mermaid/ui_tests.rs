//! Headless tests of the diagram protocol: dock pane, editor-side source
//! tracker (debounce, request, closed), pop-out window and re-docking.

use fuchico_mermaid::{BlockStatus, MermaidEvent};
use gpui_kit::prelude::*;
use gpui_kit::{
    AppContext, Context, Entity, Point, Subscription, TestAppContext, Window, div, px, size,
};

use super::window::{MermaidWindow, is_open};
use super::{MermaidPane, close_block, is_tracked, open_block, update_block};
use crate::bus::AppEvent;
use crate::snapshot::SnapTheme;
use crate::test_ui::{self, Ui};
use crate::workspace::view::DockEvent;

/// Stands in for the workspace's right dock.
struct Host {
    pane: Option<Entity<MermaidPane>>,
    closes: usize,
    _sub: Option<Subscription>,
}

impl Host {
    fn dock(&mut self, key: &str, text: Option<&str>, window: &mut Window, cx: &mut Context<Self>) {
        let pane = cx.new(|cx| {
            MermaidPane::new(
                key.into(),
                "Diagram".into(),
                text.map(Into::into),
                window,
                cx,
            )
        });
        self._sub = Some(cx.subscribe(&pane, |this, _, ev: &DockEvent, cx| match ev {
            DockEvent::Close => {
                this.closes += 1;
                this.pane = None;
                this._sub = None;
                cx.notify();
            }
        }));
        self.pane = Some(pane);
        cx.notify();
    }
}

impl Render for Host {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().flex().child(div().flex_1()).child(
            div()
                .w(px(380.))
                .h_full()
                .debug_selector(|| "dock".into())
                .children(self.pane.clone()),
        )
    }
}

const A: &str = "graph TD\n  A --> B";
const B: &str = "graph TD\n  A --> C\n  C --> D";

fn pane_status(host: &Entity<Host>, cx: &gpui_kit::App) -> Option<BlockStatus> {
    host.read(cx)
        .pane
        .as_ref()
        .map(|p| p.read(cx).view().read(cx).status())
}

fn pane_svg(host: &Entity<Host>, cx: &gpui_kit::App) -> Option<String> {
    host.read(cx)
        .pane
        .as_ref()
        .and_then(|p| p.read(cx).view().read(cx).svg().map(str::to_string))
}

fn updates(
    rec: &Entity<test_ui::BusRecorder>,
    key: &str,
    cx: &gpui_kit::App,
) -> Vec<Option<String>> {
    rec.read(cx)
        .seen
        .iter()
        .filter_map(|e| match e {
            AppEvent::MermaidUpdate { block_key, text } if block_key == key => Some(text.clone()),
            _ => None,
        })
        .collect()
}

fn count(
    rec: &Entity<test_ui::BusRecorder>,
    cx: &gpui_kit::App,
    f: impl Fn(&AppEvent) -> bool,
) -> usize {
    rec.read(cx).seen.iter().filter(|e| f(e)).count()
}

fn mermaid_window(ui: &mut Ui) -> Option<(gpui_kit::AnyWindowHandle, Entity<MermaidWindow>)> {
    ui.cx.update(|_, cx| super::window::window_for("k2", cx))
}

#[gpui_kit::test]
async fn dock_debounce_request_popout_and_redock(cx: &mut TestAppContext) {
    cx.update(test_ui::init);
    let rec = test_ui::record(cx, |e| {
        matches!(
            e,
            AppEvent::MermaidUpdate { .. }
                | AppEvent::MermaidRequest { .. }
                | AppEvent::MermaidClosed { .. }
                | AppEvent::MermaidDockRequest { .. }
        )
    });
    let (mut ui, host) = test_ui::open(cx, (1000., 700.), |_, _| Host {
        pane: None,
        closes: 0,
        _sub: None,
    });

    // ---- "▶ Preview diagram": track + dock with the initial text ----------
    ui.cx.update(|window, cx| {
        open_block("k1", A, cx);
        host.update(cx, |h, cx| h.dock("k1", Some(A), window, cx));
    });
    ui.frame();
    assert!(ui.cx.update(|_, cx| is_tracked("k1", cx)));
    assert_eq!(
        ui.cx.read(|cx| pane_status(&host, cx)),
        Some(BlockStatus::Ready)
    );
    let svg_a = ui.cx.read(|cx| pane_svg(&host, cx)).expect("rendered");
    // Initial text given: no request.
    assert_eq!(
        ui.cx
            .read(|cx| count(&rec, cx, |e| matches!(e, AppEvent::MermaidRequest { .. }))),
        0
    );

    // ---- edits are debounced (400 ms), unchanged text is not re-sent -----
    ui.cx
        .update(|_, cx| update_block("k1", Some("graph TD\n  A --> X".into()), cx));
    ui.sleep(200).await;
    ui.cx.update(|_, cx| update_block("k1", Some(B.into()), cx));
    ui.sleep(300).await;
    assert!(
        ui.cx.read(|cx| updates(&rec, "k1", cx)).is_empty(),
        "debounce restarted by the 2nd edit"
    );
    ui.sleep(150).await;
    assert_eq!(
        ui.cx.read(|cx| updates(&rec, "k1", cx)),
        vec![Some(B.to_string())]
    );
    let svg_b = ui.cx.read(|cx| pane_svg(&host, cx)).unwrap();
    assert_ne!(svg_a, svg_b, "pane re-rendered from mermaid:update");
    ui.cx.update(|_, cx| update_block("k1", Some(B.into()), cx));
    ui.sleep(500).await;
    assert_eq!(
        ui.cx.read(|cx| updates(&rec, "k1", cx)).len(),
        1,
        "same text is not re-emitted"
    );

    // ---- the block disappears → removed ----------------------------------
    ui.cx.update(|_, cx| update_block("k1", None, cx));
    ui.sleep(450).await;
    assert_eq!(
        ui.cx.read(|cx| pane_status(&host, cx)),
        Some(BlockStatus::Removed)
    );
    ui.cx.update(|_, cx| update_block("k1", Some(A.into()), cx));
    ui.sleep(450).await;
    assert_eq!(
        ui.cx.read(|cx| pane_status(&host, cx)),
        Some(BlockStatus::Ready)
    );

    // ---- "Close preview" (real click on the header button) ---------------
    let dock = ui.bounds("dock").unwrap();
    // Rightmost header button: 10px row padding, 30px wide, ~34px tall row.
    ui.click_at(Point {
        x: dock.right() - px(25.),
        y: dock.top() + px(17.),
    });
    assert_eq!(ui.cx.read(|cx| host.read(cx).closes), 1);
    ui.sleep(10).await;
    assert_eq!(
        ui.cx.read(|cx| count(&rec, cx, |e| *e
            == AppEvent::MermaidClosed {
                block_key: "k1".into()
            })),
        1
    );
    assert!(
        !ui.cx.update(|_, cx| is_tracked("k1", cx)),
        "mermaid:closed untracks"
    );

    // ---- dock without text → mermaid:request → editor answers -----------
    ui.cx.update(|window, cx| {
        open_block("k2", A, cx);
        host.update(cx, |h, cx| h.dock("k2", None, window, cx));
    });
    assert_eq!(
        ui.cx.read(|cx| pane_status(&host, cx)),
        Some(BlockStatus::Loading)
    );
    ui.sleep(10).await;
    assert_eq!(
        ui.cx.read(|cx| count(&rec, cx, |e| *e
            == AppEvent::MermaidRequest {
                block_key: "k2".into()
            })),
        1
    );
    assert_eq!(
        ui.cx.read(|cx| pane_status(&host, cx)),
        Some(BlockStatus::Ready)
    );

    // ---- "Open in new window" (real click): window, no closed ------------
    let windows_before = ui.cx.windows().len();
    ui.click_at(Point {
        x: dock.right() - px(63.),
        y: dock.top() + px(17.),
    });
    ui.sleep(10).await;
    assert_eq!(ui.cx.read(|cx| host.read(cx).closes), 2);
    assert!(ui.cx.update(|_, cx| is_open("k2", cx)));
    assert_eq!(ui.cx.windows().len(), windows_before + 1);
    assert_eq!(
        ui.cx
            .read(|cx| count(&rec, cx, |e| matches!(e, AppEvent::MermaidClosed { .. }))),
        1
    );
    assert!(ui.cx.update(|_, cx| is_tracked("k2", cx)));
    // The window asks for the text on mount.
    assert_eq!(
        ui.cx.read(|cx| count(&rec, cx, |e| *e
            == AppEvent::MermaidRequest {
                block_key: "k2".into()
            })),
        2
    );

    let (win, mw) = mermaid_window(&mut ui).expect("diagram window");
    let mut wui = Ui::new(win, &ui.cx);
    assert_eq!(
        wui.cx.read(|cx| mw.read(cx).view().read(cx).status()),
        BlockStatus::Ready
    );
    // Live updates reach the window.
    let before = wui
        .cx
        .read(|cx| mw.read(cx).view().read(cx).svg().map(str::to_string));
    wui.cx
        .update(|_, cx| update_block("k2", Some(B.into()), cx));
    wui.sleep(450).await;
    let after = wui
        .cx
        .read(|cx| mw.read(cx).view().read(cx).svg().map(str::to_string));
    assert!(after.is_some() && after != before);

    // ---- "Dock" (real click): dock-request, window closes, no closed -----
    let wb = wui.cx.update(|window, _| window.bounds());
    wui.click_at(Point {
        x: wb.size.width - px(30.),
        y: px(17.),
    });
    drop(mw);
    ui.sleep(10).await;
    assert_eq!(
        ui.cx.read(|cx| count(&rec, cx, |e| matches!(e, AppEvent::MermaidDockRequest { block_key, label } if block_key == "k2" && label == "Diagram"))),
        1
    );
    assert!(!ui.cx.update(|_, cx| is_open("k2", cx)));
    assert_eq!(
        ui.cx
            .read(|cx| count(&rec, cx, |e| matches!(e, AppEvent::MermaidClosed { .. }))),
        1
    );
    assert!(ui.cx.update(|_, cx| is_tracked("k2", cx)));

    // ---- pop out again and close the window some other way → closed ----
    ui.cx.update(|_, cx| {
        super::window::open_mermaid_window("k2".into(), "Diagram".into(), None, cx)
    });
    ui.sleep(10).await;
    let (win, _) = mermaid_window(&mut ui).expect("diagram window");
    win.update(&mut ui.cx, |_, window, _| window.remove_window())
        .unwrap();
    ui.sleep(10).await;
    assert_eq!(
        ui.cx.read(|cx| count(&rec, cx, |e| *e
            == AppEvent::MermaidClosed {
                block_key: "k2".into()
            })),
        1
    );
    assert!(!ui.cx.update(|_, cx| is_tracked("k2", cx)));

    // ---- close_block stops updates ---------------------------------------
    ui.cx.update(|window, cx| {
        open_block("k3", A, cx);
        host.update(cx, |h, cx| h.dock("k3", Some(A), window, cx));
        close_block("k3", cx);
        update_block("k3", Some(B.into()), cx);
    });
    ui.sleep(450).await;
    assert!(ui.cx.read(|cx| updates(&rec, "k3", cx)).is_empty());

    // ---- the view's own events are honoured even without a click --------
    let pane = ui.cx.read(|cx| host.read(cx).pane.clone()).unwrap();
    ui.cx.update(|_, cx| {
        pane.read(cx)
            .view()
            .clone()
            .update(cx, |_, cx| cx.emit(MermaidEvent::Close))
    });
    ui.frame();
    assert!(ui.cx.read(|cx| host.read(cx).pane.is_none()));
}

#[test]
fn snapshots() {
    for dark in [false, true] {
        let name = if dark {
            "mermaid-pane-dark"
        } else {
            "mermaid-pane-light"
        };
        crate::snapshot::snapshot(
            name,
            size(px(380.), px(420.)),
            SnapTheme::default_mode(dark),
            |window, cx| {
                cx.new(|cx| {
                    MermaidPane::new("snap".into(), "Diagram".into(), Some(B.into()), window, cx)
                })
            },
        );
        let name = if dark {
            "mermaid-window-dark"
        } else {
            "mermaid-window-light"
        };
        crate::snapshot::snapshot(
            name,
            size(px(720.), px(560.)),
            SnapTheme::default_mode(dark),
            |window, cx| {
                cx.new(|cx| {
                    MermaidWindow::new("snap".into(), "Diagram".into(), Some(B.into()), window, cx)
                })
            },
        );
    }
}
