//! `MermaidView`: the GPUI port of `MermaidPane` / `MermaidWindowApp` /
//! `MermaidDiagramView`.

use crate::color::Rgba as Srgb;
use crate::icons::{self, Icon};
use crate::panzoom::{DragState, PanZoom};
use crate::raster::{self, Raster};
use crate::render::render_svg;
use crate::theme::MermaidTheme;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use std::collections::HashMap;
use std::sync::Arc;

/// Block lifecycle, mirroring `MermaidBlockState` (`loading | ready | removed`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlockStatus {
    Loading,
    Ready,
    Removed,
}

/// Which header the view draws above the canvas.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Chrome {
    /// Docked pane: title (left), "Open in new window" and "Close preview".
    Pane,
    /// Pop-out window: a single "Dock" button ("Dock back into main window").
    Window,
    /// Just the canvas and zoom toolbar.
    Bare,
}

/// Emitted by the toolbar/header; the host acts on them (close the dock,
/// open a window and stop tracking, re-dock and close the window).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MermaidEvent {
    Close,
    PopOut,
    Dock,
}

/// User-visible strings, so the host can localise the view. Defaults are
/// the original's (English) texts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MermaidStrings {
    pub loading: SharedString,
    pub removed: SharedString,
    pub zoom_out: SharedString,
    pub zoom_in: SharedString,
    pub reset_view: SharedString,
    pub open_in_window: SharedString,
    pub close_preview: SharedString,
    pub dock_tooltip: SharedString,
    pub dock: SharedString,
}

impl Default for MermaidStrings {
    fn default() -> Self {
        Self {
            loading: "Loading…".into(),
            removed: "Source block no longer available".into(),
            zoom_out: "Zoom out".into(),
            zoom_in: "Zoom in".into(),
            reset_view: "Reset view".into(),
            open_in_window: "Open in new window".into(),
            close_preview: "Close preview".into(),
            dock_tooltip: "Dock back into main window".into(),
            dock: "Dock".into(),
        }
    }
}

pub struct MermaidView {
    label: SharedString,
    strings: MermaidStrings,
    chrome: Chrome,
    theme: MermaidTheme,
    status: BlockStatus,
    /// Last text handed to `set_source`.
    source: Option<String>,
    /// Source the current SVG was rendered from (the last good one).
    svg_source: Option<String>,
    svg: Option<String>,
    error: Option<String>,
    pz: PanZoom,
    drag: DragState,
    has_centered: bool,
    canvas_bounds: Bounds<Pixels>,
    raster: Option<Raster>,
    icon_cache: HashMap<(Icon, u32, u32), Arc<RenderImage>>,
}

impl EventEmitter<MermaidEvent> for MermaidView {}

fn hsla(c: Srgb, alpha: f32) -> Hsla {
    gpui_kit::Rgba {
        r: c.r as f32 / 255.0,
        g: c.g as f32 / 255.0,
        b: c.b as f32 / 255.0,
        a: c.a * alpha,
    }
    .into()
}

impl MermaidView {
    pub fn new(
        label: impl Into<SharedString>,
        chrome: Chrome,
        theme: MermaidTheme,
        _cx: &mut Context<Self>,
    ) -> Self {
        Self {
            label: label.into(),
            strings: MermaidStrings::default(),
            chrome,
            theme,
            status: BlockStatus::Loading,
            source: None,
            svg_source: None,
            svg: None,
            error: None,
            pz: PanZoom::default(),
            drag: DragState::default(),
            has_centered: false,
            canvas_bounds: Bounds::default(),
            raster: None,
            icon_cache: HashMap::new(),
        }
    }

    /// Convenience: a view that already has its text (`ready`).
    pub fn with_source(
        label: impl Into<SharedString>,
        chrome: Chrome,
        theme: MermaidTheme,
        source: impl Into<String>,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut v = Self::new(label, chrome, theme, cx);
        v.apply_source(Some(source.into()));
        v
    }

    /// Live update (`mermaid:update`). `Some(text)` re-renders; on a parse
    /// error the last good diagram stays on screen and the message goes in the
    /// banner. `None` means the source block no longer exists (`removed`).
    /// The pan/zoom is kept across updates; only the first successful render
    /// resets it.
    pub fn set_source(&mut self, text: Option<String>, cx: &mut Context<Self>) {
        self.apply_source(text);
        cx.notify();
    }

    /// Back to `loading` (waiting for the source to answer `mermaid:request`).
    pub fn set_loading(&mut self, cx: &mut Context<Self>) {
        self.status = BlockStatus::Loading;
        cx.notify();
    }

    fn apply_source(&mut self, text: Option<String>) {
        let Some(text) = text else {
            self.status = BlockStatus::Removed;
            self.source = None;
            return;
        };
        self.status = BlockStatus::Ready;
        if self.source.as_deref() == Some(text.as_str())
            && (self.svg.is_some() || self.error.is_some())
        {
            return;
        }
        self.render_text(&text);
        self.source = Some(text);
    }

    fn render_text(&mut self, text: &str) {
        match render_svg(text, &self.theme) {
            Ok(svg) => {
                self.svg = Some(svg);
                self.svg_source = Some(text.to_string());
                self.error = None;
                self.raster = None;
                if !self.has_centered {
                    self.has_centered = true;
                    self.pz.reset();
                }
            }
            Err(e) => self.error = Some(e.to_string()),
        }
    }

    /// Theme switch. Unlike the CSS-variable original this re-renders the SVG
    /// (colours are baked into it); the pan/zoom is untouched and rasters are
    /// cached per theme, so toggling back is free.
    pub fn set_theme(&mut self, theme: MermaidTheme, cx: &mut Context<Self>) {
        if self.theme == theme {
            return;
        }
        self.theme = theme;
        self.icon_cache.clear();
        if let Some(src) = self.svg_source.clone() {
            self.render_text(&src);
            if let Some(cur) = self.source.clone()
                && cur != src
            {
                self.render_text(&cur);
            }
        }
        cx.notify();
    }

    pub fn set_label(&mut self, label: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.label = label.into();
        cx.notify();
    }

    /// Replaces the view's UI strings (localisation).
    pub fn set_strings(&mut self, strings: MermaidStrings, cx: &mut Context<Self>) {
        if self.strings != strings {
            self.strings = strings;
            cx.notify();
        }
    }

    pub fn set_chrome(&mut self, chrome: Chrome, cx: &mut Context<Self>) {
        self.chrome = chrome;
        cx.notify();
    }

    pub fn pan_zoom(&self) -> PanZoom {
        self.pz
    }

    pub fn status(&self) -> BlockStatus {
        self.status
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    pub fn svg(&self) -> Option<&str> {
        self.svg.as_deref()
    }

    /// Logical size of the diagram, once rendered.
    pub fn content_size(&self) -> Option<(f32, f32)> {
        self.raster.as_ref().map(|r| (r.width, r.height))
    }

    pub fn reset(&mut self, cx: &mut Context<Self>) {
        self.pz.reset();
        cx.notify();
    }

    pub fn zoom_in(&mut self, cx: &mut Context<Self>) {
        let s = self.canvas_bounds.size;
        self.pz.zoom_in(f32::from(s.width), f32::from(s.height));
        cx.notify();
    }

    pub fn zoom_out(&mut self, cx: &mut Context<Self>) {
        let s = self.canvas_bounds.size;
        self.pz.zoom_out(f32::from(s.width), f32::from(s.height));
        cx.notify();
    }

    /// Fit the diagram into the canvas (not a button in the original UI).
    pub fn fit(&mut self, cx: &mut Context<Self>) {
        if let Some((w, h)) = self.content_size() {
            let s = self.canvas_bounds.size;
            self.pz
                .fit(f32::from(s.width), f32::from(s.height), w, h, 24.0);
            cx.notify();
        }
    }

    fn local(&self, p: Point<Pixels>) -> (f32, f32) {
        (
            f32::from(p.x - self.canvas_bounds.origin.x),
            f32::from(p.y - self.canvas_bounds.origin.y),
        )
    }

    fn on_wheel(&mut self, e: &ScrollWheelEvent, cx: &mut Context<Self>) {
        // DOM deltaY is positive when scrolling down; GPUI's is the opposite.
        let dy = match e.delta {
            ScrollDelta::Pixels(p) => -f32::from(p.y),
            ScrollDelta::Lines(l) => -l.y * 20.0,
        };
        let (cx_, cy_) = self.local(e.position);
        self.pz.wheel(cx_, cy_, dy);
        cx.notify();
    }

    fn on_pinch(&mut self, e: &PinchEvent, cx: &mut Context<Self>) {
        let (cx_, cy_) = self.local(e.position);
        self.pz.pinch(cx_, cy_, e.delta);
        cx.notify();
    }

    fn ensure_raster(&mut self, dpr: f32) {
        let (Some(svg), Some(src)) = (&self.svg, &self.svg_source) else {
            return;
        };
        let want = dpr * self.pz.scale;
        if let Some(r) = &self.raster
            && r.scale >= want * 0.8
            && r.scale <= want * 3.0
        {
            return;
        }
        let scale = raster::quantize_scale(want);
        self.raster = raster::rasterize_cached(src, &self.theme, Some(svg), scale).ok();
    }

    fn icon(&mut self, icon: Icon, color: Srgb, px: f32, dpr: f32) -> Option<Arc<RenderImage>> {
        let key = (
            icon,
            (color.r as u32) << 16 | (color.g as u32) << 8 | color.b as u32,
            (px * 10.0) as u32,
        );
        if let Some(i) = self.icon_cache.get(&key) {
            return Some(i.clone());
        }
        let img = raster::rasterize_icon(icons::svg(icon), &color.hex(), px, (dpr * 2.0).ceil())?;
        self.icon_cache.insert(key, img.clone());
        Some(img)
    }
}

fn paint_canvas(
    view: Entity<MermaidView>,
    bounds: Bounds<Pixels>,
    raster: Option<Raster>,
    pz: PanZoom,
    dot: Hsla,
    window: &mut Window,
) {
    // Dotted grid: radial-gradient(1px dot) over 16px tiles.
    let step = 16.0;
    let (w, h) = (f32::from(bounds.size.width), f32::from(bounds.size.height));
    let mut y = 0.0;
    while y < h {
        let mut x = 0.0;
        while x < w {
            let o = bounds.origin + point(px(x + step / 2.0 - 1.0), px(y + step / 2.0 - 1.0));
            window.paint_quad(
                fill(Bounds::new(o, size(px(2.0), px(2.0))), dot).corner_radii(px(1.0)),
            );
            x += step;
        }
        y += step;
    }
    if let Some(r) = raster {
        let origin = bounds.origin + point(px(pz.x), px(pz.y));
        let image_bounds = Bounds::new(
            origin,
            size(px(r.width * pz.scale), px(r.height * pz.scale)),
        );
        let _ = window.paint_image(bounds, image_bounds, Corners::default(), r.image, 0, false);
    }
    // Pointer capture: keep panning when the cursor leaves the canvas.
    let v = view.clone();
    window.on_mouse_event(move |e: &MouseMoveEvent, phase, window, cx| {
        if phase != DispatchPhase::Bubble {
            return;
        }
        v.update(cx, |this, cx| {
            if this.drag.active() {
                let (x, y) = (f32::from(e.position.x), f32::from(e.position.y));
                let mut pz = this.pz;
                if this.drag.moved(&mut pz, x, y) {
                    this.pz = pz;
                    cx.notify();
                    window.refresh();
                }
            }
        });
    });
    let v = view;
    window.on_mouse_event(move |_: &MouseUpEvent, phase, _, cx| {
        if phase == DispatchPhase::Bubble {
            v.update(cx, |this, cx| {
                if this.drag.active() {
                    this.drag.end();
                    cx.notify();
                }
            });
        }
    });
}

impl Render for MermaidView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let dpr = window.scale_factor();
        self.ensure_raster(dpr);
        let t = self.theme.clone();
        let bg = t.resolve(&t.background);
        let fg = t.resolve(&t.foreground);
        let muted = t.resolve(&t.muted_foreground);
        let card = t.resolve(&t.card);
        let border = t.resolve(&t.border);
        let accent = t.resolve(&t.accent);
        let destructive = t.resolve(&t.destructive);
        let radius_sm = px(t.radius_sm as f32);
        let radius_md = px(t.radius_sm as f32 * 8.0 / 6.0);
        let entity = cx.entity();
        let strings = self.strings.clone();

        let make_btn = |this: &mut Self,
                        id: &'static str,
                        icon: Icon,
                        tip: SharedString,
                        on: fn(&mut Self, &mut Context<Self>)| {
            let img = this.icon(icon, muted, 14.0, dpr);
            div()
                .id(id)
                .flex()
                .items_center()
                .justify_center()
                .size(px(24.0))
                .rounded(radius_sm)
                .cursor_pointer()
                .hover(move |s| s.bg(hsla(accent, 0.14)))
                .tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx))
                .when_some(img, |d, img| {
                    d.child(gpui_kit::img(ImageSource::Render(img)).size(px(14.0)))
                })
                .on_click(cx.listener(move |this, _, _, cx| on(this, cx)))
        };

        let toolbar = div()
            .absolute()
            .top(px(10.0))
            .right(px(10.0))
            .flex()
            .gap(px(2.0))
            .p(px(2.0))
            .border_1()
            .border_color(hsla(border, 0.6))
            .rounded(radius_md)
            .bg(hsla(card, 0.85))
            .child(make_btn(
                self,
                "mm-zoom-out",
                Icon::ZoomOut,
                strings.zoom_out.clone(),
                |t, cx| t.zoom_out(cx),
            ))
            .child(make_btn(
                self,
                "mm-zoom-in",
                Icon::ZoomIn,
                strings.zoom_in.clone(),
                |t, cx| t.zoom_in(cx),
            ))
            .child(make_btn(
                self,
                "mm-reset",
                Icon::Reset,
                strings.reset_view.clone(),
                |t, cx| t.reset(cx),
            ));

        let banner = |text: String, color: Srgb, border_c: Srgb, alpha: f32| {
            div()
                .absolute()
                .top(px(12.0))
                .left(px(12.0))
                .max_w(relative(0.6))
                .px(px(10.0))
                .py(px(4.0))
                .border_1()
                .border_color(hsla(border_c, alpha))
                .rounded(radius_md)
                .bg(hsla(card, 0.9))
                .text_size(px(12.0))
                .text_color(hsla(color, 1.0))
                .child(text)
        };

        let mut banners = div();
        if self.status == BlockStatus::Loading && self.svg.is_none() {
            banners = banners.child(banner(strings.loading.to_string(), muted, border, 0.6));
        }
        if self.status == BlockStatus::Removed {
            banners = banners.child(banner(strings.removed.to_string(), muted, border, 0.6));
        }
        if let Some(err) = &self.error {
            banners = banners.child(banner(err.clone(), destructive, destructive, 0.4));
        }

        let header = match self.chrome {
            Chrome::Bare => None,
            chrome => {
                let btn_base = |id: &'static str| {
                    div()
                        .id(id)
                        .flex()
                        .items_center()
                        .gap(px(6.0))
                        .border_1()
                        .border_color(hsla(border, 1.0))
                        .rounded(radius_sm)
                        .px(px(7.0))
                        .py(px(3.0))
                        .text_size(px(11.0))
                        .text_color(hsla(muted, 1.0))
                        .cursor_pointer()
                        .hover(move |s| s.bg(hsla(accent, 0.12)))
                };
                let mut row = div()
                    .flex()
                    .items_center()
                    .justify_end()
                    .gap(px(8.0))
                    .px(px(10.0))
                    .py(px(6.0))
                    .border_b_1()
                    .border_color(hsla(border, 1.0))
                    .bg(hsla(card, 1.0));
                if chrome == Chrome::Pane {
                    let pop = self.icon(Icon::ExternalLink, muted, 14.0, dpr);
                    let close = self.icon(Icon::Close, muted, 14.0, dpr);
                    let (tip_pop, tip_close) = (
                        strings.open_in_window.clone(),
                        strings.close_preview.clone(),
                    );
                    row = row
                        .child(
                            div()
                                .flex_1()
                                .text_size(px(12.0))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(hsla(fg, 1.0))
                                .child(self.label.clone()),
                        )
                        .child(
                            btn_base("mm-popout")
                                .tooltip(move |window, cx| {
                                    Tooltip::new(tip_pop.clone()).build(window, cx)
                                })
                                .when_some(pop, |d, i| {
                                    d.child(gpui_kit::img(ImageSource::Render(i)).size(px(14.0)))
                                })
                                .on_click(cx.listener(|_, _, _, cx| cx.emit(MermaidEvent::PopOut))),
                        )
                        .child(
                            btn_base("mm-close")
                                .tooltip(move |window, cx| {
                                    Tooltip::new(tip_close.clone()).build(window, cx)
                                })
                                .when_some(close, |d, i| {
                                    d.child(gpui_kit::img(ImageSource::Render(i)).size(px(14.0)))
                                })
                                .on_click(cx.listener(|_, _, _, cx| cx.emit(MermaidEvent::Close))),
                        );
                } else {
                    let dock = self.icon(Icon::PanelRightOpen, muted, 14.0, dpr);
                    let tip_dock = strings.dock_tooltip.clone();
                    row = row.child(
                        btn_base("mm-dock")
                            .tooltip(move |window, cx| {
                                Tooltip::new(tip_dock.clone()).build(window, cx)
                            })
                            .when_some(dock, |d, i| {
                                d.child(gpui_kit::img(ImageSource::Render(i)).size(px(14.0)))
                            })
                            .child(strings.dock.clone())
                            .on_click(cx.listener(|_, _, _, cx| cx.emit(MermaidEvent::Dock))),
                    );
                }
                Some(row)
            }
        };

        let raster = self.raster.clone();
        let pz = self.pz;
        let dot = hsla(fg, 0.08);
        let grabbing = self.drag.active();
        let ent_pre = entity.clone();
        let ent_paint = entity.clone();

        let canvas_el = div()
            .id("mm-canvas")
            .absolute()
            .inset_0()
            .overflow_hidden()
            .cursor(if grabbing {
                CursorStyle::ClosedHand
            } else {
                CursorStyle::OpenHand
            })
            .on_scroll_wheel(cx.listener(|this, e: &ScrollWheelEvent, _, cx| {
                this.on_wheel(e, cx);
                cx.stop_propagation();
            }))
            .on_pinch(cx.listener(|this, e: &PinchEvent, _, cx| {
                this.on_pinch(e, cx);
                cx.stop_propagation();
            }))
            .child(
                canvas(
                    move |bounds, _, cx| {
                        ent_pre.update(cx, |v, _| v.canvas_bounds = bounds);
                        bounds
                    },
                    move |_, bounds, window, _| {
                        paint_canvas(ent_paint, bounds, raster, pz, dot, window)
                    },
                )
                .size_full(),
            );

        let canvas_el = [MouseButton::Left, MouseButton::Right, MouseButton::Middle]
            .into_iter()
            .fold(canvas_el, |el, button| {
                el.on_mouse_down(
                    button,
                    cx.listener(|this, e: &MouseDownEvent, _, cx| {
                        this.drag
                            .begin(&this.pz, f32::from(e.position.x), f32::from(e.position.y));
                        cx.notify();
                    }),
                )
            });

        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(hsla(bg, 1.0))
            .when_some(header, |d, h| d.child(h))
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .child(canvas_el)
                    .child(banners)
                    .child(toolbar),
            )
    }
}
