//! The pop-out diagram window (`MermaidWindowApp`), labelled
//! `mermaid-<blockKey>`, 720×560. It asks for the text on mount; "Dock"
//! sends `mermaid:dock-request {blockKey, label}` and closes the window;
//! closing it any other way sends `mermaid:closed {blockKey}`. Once the
//! source editor is gone it simply keeps the last diagram.

use std::collections::HashMap;

use fuchico_mermaid::{Chrome, MermaidEvent, MermaidView};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyWindowHandle, App, Context, Entity, Global, Subscription, TitlebarOptions, WeakEntity,
    Window, WindowOptions, div, px, size,
};

use crate::bus::{self, AppEvent};
use crate::theme::AppTheme;

pub const WINDOW_SIZE: (f32, f32) = (720., 560.);

/// `"mermaid-" + blockKey`.
pub fn window_label(block_key: &str) -> String {
    format!("mermaid-{block_key}")
}

/// Open diagram windows by label.
#[derive(Default)]
struct MermaidWindows(HashMap<String, (AnyWindowHandle, WeakEntity<MermaidWindow>)>);
impl Global for MermaidWindows {}

fn registry(cx: &mut App) -> &mut HashMap<String, (AnyWindowHandle, WeakEntity<MermaidWindow>)> {
    &mut cx.default_global::<MermaidWindows>().0
}

pub fn is_open(block_key: &str, cx: &mut App) -> bool {
    let Some((h, _)) = registry(cx).get(&window_label(block_key)).cloned() else {
        return false;
    };
    h.update(cx, |_, _, _| ()).is_ok()
}

/// The open diagram window for `block_key`, if any.
pub fn window_for(
    block_key: &str,
    cx: &mut App,
) -> Option<(AnyWindowHandle, Entity<MermaidWindow>)> {
    let (h, w) = registry(cx).get(&window_label(block_key))?.clone();
    Some((h, w.upgrade()?))
}

/// `openMermaidWindow(blockKey, label)`. An already open window for the
/// block is brought forward instead. `text` (the dock's last diagram) is
/// shown until the editor answers the request.
pub fn open_mermaid_window(block_key: String, label: String, text: Option<String>, cx: &mut App) {
    let wl = window_label(&block_key);
    if let Some((h, _)) = registry(cx).get(&wl).cloned() {
        let background = crate::background_mode();
        if h.update(cx, |_, w, _| {
            if !background {
                w.activate_window()
            }
        })
        .is_ok()
        {
            return;
        }
        registry(cx).remove(&wl);
    }
    let opts = WindowOptions {
        // Remembered per label, like the original's window-state plugin.
        window_bounds: Some(crate::windows::geometry::initial_bounds(
            &wl,
            WINDOW_SIZE,
            cx,
        )),
        titlebar: Some(TitlebarOptions {
            title: Some(label.clone().into()),
            ..Default::default()
        }),
        window_min_size: Some(size(px(320.), px(240.))),
        app_id: Some("fuchico".into()),
        focus: !crate::background_mode(),
        ..Default::default()
    };
    match gpui_kit::open_window(opts, cx, |window, cx| {
        cx.new(|cx| MermaidWindow::new(block_key, label, text, window, cx))
    }) {
        Ok((handle, view)) => {
            registry(cx).insert(wl, (handle, view.downgrade()));
        }
        Err(e) => log::error!("open diagram window: {e}"),
    }
}

pub struct MermaidWindow {
    block_key: String,
    view: Entity<MermaidView>,
    /// Re-docking: the dock takes over, so no `mermaid:closed`.
    docking: bool,
    _subs: Vec<Subscription>,
}

impl MermaidWindow {
    pub fn new(
        block_key: String,
        label: String,
        text: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let view = super::new_view(&label, Chrome::Window, text, cx);
        let subs = vec![
            crate::windows::geometry::track(window_label(&block_key), window, cx),
            bus::subscribe(cx, |this: &mut MermaidWindow, ev, cx| {
                super::apply_update(&this.block_key, ev, &this.view, cx);
            }),
            cx.subscribe_in(
                &view,
                window,
                move |this, _, ev: &MermaidEvent, window, _| {
                    if *ev == MermaidEvent::Dock {
                        this.docking = true;
                        bus::send(AppEvent::MermaidDockRequest {
                            block_key: this.block_key.clone(),
                            label: label.clone(),
                        });
                        window.remove_window();
                    }
                },
            ),
            super::follow_theme(&view, cx),
            cx.observe_window_appearance(window, |_, _, cx| crate::theme::apply::refresh(cx)),
            cx.on_release(|this, cx| {
                registry(cx).remove(&window_label(&this.block_key));
                if !this.docking {
                    bus::send(AppEvent::MermaidClosed {
                        block_key: this.block_key.clone(),
                    });
                }
            }),
        ];
        bus::send(AppEvent::MermaidRequest {
            block_key: block_key.clone(),
        });
        MermaidWindow {
            block_key,
            view,
            docking: false,
            _subs: subs,
        }
    }

    pub fn block_key(&self) -> &str {
        &self.block_key
    }

    pub fn view(&self) -> &Entity<MermaidView> {
        &self.view
    }
}

impl Render for MermaidWindow {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let th = AppTheme::get(cx);
        div()
            .size_full()
            .bg(th.colors.background)
            .text_color(th.colors.foreground)
            .font_family(th.ui_font.clone())
            .child(self.view.clone())
    }
}

#[cfg(test)]
mod tests {
    use gpui_kit::{TestAppContext, VisualTestContext, px, size};

    use crate::windows::geometry;

    #[test]
    fn labels() {
        assert_eq!(super::window_label("k-1"), "mermaid-k-1");
    }

    #[gpui_kit::test]
    async fn window_geometry_is_remembered(cx: &mut TestAppContext) {
        crate::persist::set_test_dir(crate::test_ui::temp_dir("mermaid-geom-cfg"));
        cx.update(crate::test_ui::init_shell);
        let open = |cx: &mut TestAppContext| {
            cx.update(|cx| {
                super::open_mermaid_window(
                    "k".into(),
                    "Diagram".into(),
                    Some("graph TD\nA-->B".into()),
                    cx,
                )
            });
            cx.run_until_parked();
            cx.update(|cx| super::registry(cx).get("mermaid-k").unwrap().0)
        };
        let h = open(cx);
        let vcx = VisualTestContext::from_window(h, cx);
        vcx.simulate_resize(size(px(900.), px(650.)));
        vcx.run_until_parked();
        let saved = cx
            .read(|cx| geometry::initial_bounds("mermaid-k", super::WINDOW_SIZE, cx).get_bounds());
        assert_eq!(
            (saved.size.width, saved.size.height),
            (px(900.), px(650.)),
            "recorded under the window label"
        );

        cx.update(|cx| h.update(cx, |_, w, _| w.remove_window()).unwrap());
        cx.run_until_parked();
        let h = open(cx);
        let b = cx.update(|cx| h.update(cx, |_, w, _| w.bounds()).unwrap());
        assert_eq!((b.size.width, b.size.height), (px(900.), px(650.)));
    }
}
