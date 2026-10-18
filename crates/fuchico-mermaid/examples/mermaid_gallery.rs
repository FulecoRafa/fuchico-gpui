//! Gallery window: one `MermaidView` per sample, selectable from a side list.
//! `MERMAID_SAMPLE=<index>` picks the initial sample, `MERMAID_DARK=1` the dark theme,
//! `MERMAID_SOURCE=<file>` loads a custom diagram.

use fuchico_mermaid::samples::SAMPLES;
use fuchico_mermaid::{Chrome, MermaidTheme, MermaidView};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

struct Gallery {
    view: Entity<MermaidView>,
    selected: usize,
    theme: MermaidTheme,
}

impl Gallery {
    fn select(&mut self, i: usize, cx: &mut Context<Self>) {
        self.selected = i;
        let src = SAMPLES[i].1.to_string();
        self.view.update(cx, |v, cx| v.set_source(Some(src), cx));
        cx.notify();
    }
}

impl Render for Gallery {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = self.theme.clone();
        let fg: Hsla = rgb_of(&t, &t.foreground);
        let bg: Hsla = rgb_of(&t, &t.background);
        let sel: Hsla = rgb_of(&t, &t.secondary);
        let items = SAMPLES.iter().enumerate().map(|(i, (name, _))| {
            div()
                .id(("sample", i))
                .px_3()
                .py_1()
                .text_size(px(12.0))
                .cursor_pointer()
                .when(i == self.selected, |d| d.bg(sel))
                .child(*name)
                .on_click(cx.listener(move |this, _, _, cx| this.select(i, cx)))
        });
        div()
            .size_full()
            .flex()
            .bg(bg)
            .text_color(fg)
            .child(
                div()
                    .w(px(250.0))
                    .flex_none()
                    .flex()
                    .flex_col()
                    .py_2()
                    .children(items),
            )
            .child(div().flex_1().min_w_0().child(self.view.clone()))
    }
}

fn rgb_of(t: &MermaidTheme, css: &str) -> Hsla {
    let c = t.resolve(css);
    gpui_kit::Rgba {
        r: c.r as f32 / 255.0,
        g: c.g as f32 / 255.0,
        b: c.b as f32 / 255.0,
        a: 1.0,
    }
    .into()
}

fn main() {
    let dark = std::env::var("MERMAID_DARK").is_ok();
    let theme = if dark {
        MermaidTheme::dark()
    } else {
        MermaidTheme::light()
    };
    let start: usize = std::env::var("MERMAID_SAMPLE")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let custom = std::env::var("MERMAID_SOURCE")
        .ok()
        .and_then(|p| std::fs::read_to_string(p).ok());
    gpui_kit::application().run(move |cx| {
        gpui_kit::init(cx);
        let opts = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                None,
                size(px(1100.0), px(720.0)),
                cx,
            ))),
            ..Default::default()
        };
        let theme2 = theme.clone();
        gpui_kit::open_window(opts, cx, move |_, cx| {
            let src = custom
                .clone()
                .unwrap_or_else(|| SAMPLES[start.min(SAMPLES.len() - 1)].1.to_string());
            let view = cx.new(|cx| {
                MermaidView::with_source("Diagram", Chrome::Pane, theme2.clone(), src, cx)
            });
            cx.new(|_| Gallery {
                view,
                selected: start,
                theme: theme2,
            })
        })
        .expect("window");
        if std::env::var_os("FUCHICO_BACKGROUND").is_none() {
            cx.activate(true);
        }
    });
}
