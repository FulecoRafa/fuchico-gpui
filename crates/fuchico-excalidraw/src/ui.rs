//! Rendering of `ExcalidrawView`: the canvas element (scene + overlay +
//! pointer wiring) and the Excalidraw-style chrome (toolbar island, hint,
//! properties panel, zoom/undo islands, main menu).

use std::collections::HashSet;

use gpui_kit::*;
use gpui_kit::prelude::FluentBuilder as _;

use crate::color::{self, Rgba};
use crate::geom;
use crate::model::{Element, Num, Roundness};
use crate::render::{self, Camera, Palette};
use crate::style::ExcalidrawStyle;
use crate::text;
use crate::view::{Drag, ExcalidrawView, Handle, Tool};

fn hsla_rgba(h: Hsla) -> Rgba {
    let c = gpui_kit::Rgba::from(h);
    Rgba::new(c.r, c.g, c.b, c.a)
}

// ---------------------------------------------------------------- icons

/// Icon geometry in a 24×24 box: polylines (closed flag) stroked.
enum Part {
    Poly(&'static [[f32; 2]], bool),
    Circle([f32; 2], f32),
    Dot([f32; 2], f32),
    RoundRect([f32; 4], f32),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Icon {
    Lock,
    Unlock,
    Hand,
    Selection,
    Rectangle,
    Diamond,
    Ellipse,
    Arrow,
    Line,
    Pencil,
    Text,
    Image,
    Eraser,
    Frame,
    Menu,
    Undo,
    Redo,
    Minus,
    Plus,
}

fn icon_parts(i: Icon) -> Vec<Part> {
    use Part::*;
    match i {
        Icon::Lock => vec![
            RoundRect([5.0, 11.0, 19.0, 21.0], 2.0),
            Poly(&[[8.0, 11.0], [8.0, 7.5], [10.0, 4.5], [14.0, 4.5], [16.0, 7.5], [16.0, 11.0]], false),
        ],
        Icon::Unlock => vec![
            RoundRect([5.0, 11.0, 19.0, 21.0], 2.0),
            Poly(&[[8.0, 11.0], [8.0, 7.5], [10.0, 4.5], [14.0, 4.5], [16.0, 7.0]], false),
        ],
        Icon::Hand => vec![
            Poly(&[[8.0, 13.0], [8.0, 6.5]], false),
            Poly(&[[11.0, 11.5], [11.0, 4.5]], false),
            Poly(&[[14.0, 11.5], [14.0, 5.5]], false),
            Poly(&[[17.0, 12.5], [17.0, 8.5]], false),
            Poly(&[[17.0, 12.5], [17.0, 16.0], [15.0, 20.0], [10.5, 20.0], [7.0, 16.5], [4.5, 12.5], [6.0, 11.5], [8.0, 13.0]], false),
        ],
        Icon::Selection => vec![Poly(
            &[[6.0, 4.0], [6.0, 19.0], [10.0, 15.5], [12.5, 21.0], [15.0, 20.0], [12.5, 14.5], [18.0, 14.5]],
            true,
        )],
        Icon::Rectangle => vec![RoundRect([4.0, 4.0, 20.0, 20.0], 3.0)],
        Icon::Diamond => vec![Poly(&[[12.0, 3.0], [21.0, 12.0], [12.0, 21.0], [3.0, 12.0]], true)],
        Icon::Ellipse => vec![Circle([12.0, 12.0], 8.5)],
        Icon::Arrow => vec![
            Poly(&[[5.0, 19.0], [19.0, 5.0]], false),
            Poly(&[[11.0, 5.0], [19.0, 5.0], [19.0, 13.0]], false),
        ],
        Icon::Line => vec![Poly(&[[4.0, 12.0], [20.0, 12.0]], false)],
        Icon::Pencil => vec![
            Poly(&[[4.0, 20.0], [5.0, 15.0], [15.5, 4.5], [19.5, 8.5], [9.0, 19.0]], true),
            Poly(&[[13.5, 6.5], [17.5, 10.5]], false),
        ],
        Icon::Text => vec![
            Poly(&[[5.0, 6.0], [5.0, 4.0], [19.0, 4.0], [19.0, 6.0]], false),
            Poly(&[[12.0, 4.0], [12.0, 20.0]], false),
            Poly(&[[9.5, 20.0], [14.5, 20.0]], false),
        ],
        Icon::Image => vec![
            RoundRect([4.0, 5.0, 20.0, 19.0], 2.0),
            Dot([9.0, 10.0], 1.5),
            Poly(&[[4.0, 17.0], [9.5, 12.0], [13.5, 15.5], [16.0, 13.0], [20.0, 16.5]], false),
        ],
        Icon::Eraser => vec![
            Poly(&[[19.0, 20.0], [9.0, 20.0], [4.0, 15.0], [14.0, 5.0], [20.0, 11.0], [11.0, 20.0]], false),
            Poly(&[[18.0, 13.0], [11.5, 6.5]], false),
        ],
        Icon::Frame => vec![
            Poly(&[[8.0, 3.0], [8.0, 21.0]], false),
            Poly(&[[16.0, 3.0], [16.0, 21.0]], false),
            Poly(&[[3.0, 8.0], [21.0, 8.0]], false),
            Poly(&[[3.0, 16.0], [21.0, 16.0]], false),
        ],
        Icon::Menu => vec![
            Poly(&[[4.0, 7.0], [20.0, 7.0]], false),
            Poly(&[[4.0, 12.0], [20.0, 12.0]], false),
            Poly(&[[4.0, 17.0], [20.0, 17.0]], false),
        ],
        Icon::Undo => vec![
            Poly(&[[9.0, 14.0], [4.0, 9.0], [9.0, 4.0]], false),
            Poly(&[[4.0, 9.0], [14.5, 9.0], [18.0, 10.5], [20.0, 14.0], [18.0, 17.5], [14.5, 19.0], [11.0, 19.0]], false),
        ],
        Icon::Redo => vec![
            Poly(&[[15.0, 14.0], [20.0, 9.0], [15.0, 4.0]], false),
            Poly(&[[20.0, 9.0], [9.5, 9.0], [6.0, 10.5], [4.0, 14.0], [6.0, 17.5], [9.5, 19.0], [13.0, 19.0]], false),
        ],
        Icon::Minus => vec![Poly(&[[6.0, 12.0], [18.0, 12.0]], false)],
        Icon::Plus => vec![Poly(&[[6.0, 12.0], [18.0, 12.0]], false), Poly(&[[12.0, 6.0], [12.0, 18.0]], false)],
    }
}

fn circle_pts(c: [f32; 2], r: f32) -> Vec<[f32; 2]> {
    (0..24)
        .map(|k| {
            let a = k as f32 / 24.0 * std::f32::consts::TAU;
            [c[0] + r * a.cos(), c[1] + r * a.sin()]
        })
        .collect()
}

fn round_rect_pts(r: [f32; 4], rad: f32) -> Vec<[f32; 2]> {
    let mut out = Vec::new();
    let corners = [
        ([r[2] - rad, r[1] + rad], -90.0f32),
        ([r[2] - rad, r[3] - rad], 0.0),
        ([r[0] + rad, r[3] - rad], 90.0),
        ([r[0] + rad, r[1] + rad], 180.0),
    ];
    for (c, start) in corners {
        for k in 0..=4 {
            let a = (start + k as f32 * 22.5).to_radians();
            out.push([c[0] + rad * a.cos(), c[1] + rad * a.sin()]);
        }
    }
    out
}

fn icon(i: Icon, size: f32, color: Hsla) -> impl IntoElement {
    canvas(
        |_, _, _| (),
        move |b, _, window, _| {
            let s = f32::from(b.size.width) / 24.0;
            let at = |q: [f32; 2]| point(b.origin.x + px(q[0] * s), b.origin.y + px(q[1] * s));
            let width = (1.6 * s).max(1.0);
            for part in icon_parts(i) {
                let (pts, closed, filled): (Vec<[f32; 2]>, bool, bool) = match part {
                    Part::Poly(p, c) => (p.to_vec(), c, false),
                    Part::Circle(c, r) => (circle_pts(c, r), true, false),
                    Part::Dot(c, r) => (circle_pts(c, r), true, true),
                    Part::RoundRect(r, rad) => (round_rect_pts(r, rad), true, false),
                };
                let mut pb = if filled { PathBuilder::fill() } else { PathBuilder::stroke(px(width)) };
                pb.move_to(at(pts[0]));
                for q in &pts[1..] {
                    pb.line_to(at(*q));
                }
                if closed {
                    pb.close();
                }
                if let Ok(p) = pb.build() {
                    window.paint_path(p, color);
                }
            }
        },
    )
    .size(px(size))
}

// ---------------------------------------------------------------- canvas

struct Snapshot {
    view: Entity<ExcalidrawView>,
}

fn rect_screen(cam: Camera, origin: Point<Pixels>, el: &Element, r: [f64; 4]) -> Vec<Point<Pixels>> {
    [[r[0], r[1]], [r[2], r[1]], [r[2], r[3]], [r[0], r[3]]]
        .iter()
        .map(|q| cam.to_screen(origin, geom::to_world(el, *q)))
        .collect()
}

fn paint_canvas(bounds: Bounds<Pixels>, hitbox: Hitbox, snap: Snapshot, window: &mut Window, cx: &mut App) {
    let entity = snap.view.clone();
    {
        let v = entity.read(cx);
        v.bounds.set(bounds);
        let style = v.style(cx);
        let pal = v.palette(cx);
        let cam = v.cam;
        let origin = bounds.origin;
        let caches = v.caches.clone();
        let mut caches = caches.borrow_mut();
        caches.retain(&v.scene);
        render::paint_background(bounds, pal.canvas, window);

        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            let frames: Vec<&Element> = v.scene.elements().filter(|e| e.kind == "frame" || e.kind == "magicframe").collect();
            let frame_ids: HashSet<&str> = frames.iter().map(|f| f.id.as_str()).collect();
            let editing = v.editing.as_ref().map(|e| e.id.as_str());
            for el in v.scene.elements() {
                // Off-screen culling.
                let bb = geom::aabb(el);
                let tl = cam.to_screen(origin, [bb[0], bb[1]]);
                let br = cam.to_screen(origin, [bb[2], bb[3]]);
                let pad = px(40.0);
                if br.x < bounds.left() - pad || tl.x > bounds.right() + pad || br.y < bounds.top() - pad || tl.y > bounds.bottom() + pad {
                    continue;
                }
                let alpha = if v.erasing.contains(&el.id) || el.container_id().map(|c| v.erasing.contains(c)).unwrap_or(false) { 0.2 } else { 1.0 };
                let frame = el.f.frame_id.get().filter(|f| frame_ids.contains(f.as_str()));
                let paint = |window: &mut Window, caches: &mut render::Caches| {
                    render::paint_element(el, &v.scene, caches, cam, origin, pal, alpha, window);
                };
                if editing == Some(el.id.as_str()) && el.text().is_empty() {
                    continue;
                }
                match frame.and_then(|f| v.scene.get(f)) {
                    Some(f) => {
                        let mask = render::frame_mask(f, cam, origin);
                        window.with_content_mask(Some(mask), |w| paint(w, &mut caches));
                    }
                    None => paint(window, &mut caches),
                }
            }
            for f in &frames {
                render::paint_frame(f, cam, origin, pal, window);
            }
            paint_overlay(v, &style, pal, origin, window);
        });

        // Cursor.
        let cursor = match (&v.drag, v.tool) {
            (Some(Drag::Pan { .. }), _) => CursorStyle::ClosedHand,
            _ if v.space_down => CursorStyle::OpenHand,
            (_, Tool::Hand) => CursorStyle::OpenHand,
            (_, Tool::Text) => CursorStyle::IBeam,
            (_, Tool::Selection) => {
                let p = window.mouse_position();
                let wp = cam.to_world(origin, p);
                if v.editing.is_some() {
                    CursorStyle::IBeam
                } else if let Some(h) = v.hit_handle(wp) {
                    let a = v.single().map(|e| e.angle()).unwrap_or(0.0);
                    handle_cursor(h, a)
                } else if v.point_handles().iter().any(|(_, q, _)| geom::dist(wp, *q) <= 8.0 / cam.zoom) {
                    CursorStyle::PointingHand
                } else if v.hit_element(wp).is_some() {
                    CursorStyle::Arrow
                } else {
                    CursorStyle::Arrow
                }
            }
            (_, Tool::Eraser) => CursorStyle::Crosshair,
            _ => CursorStyle::Crosshair,
        };
        window.set_cursor_style(cursor, &hitbox);
    }

    // Pointer wiring.
    {
        let e = entity.clone();
        let hb = hitbox.clone();
        window.on_mouse_event(move |ev: &MouseDownEvent, phase, window, cx| {
            if phase == DispatchPhase::Bubble && hb.is_hovered(window) {
                e.update(cx, |v, cx| v.on_down(ev, window, cx));
            }
        });
    }
    {
        let e = entity.clone();
        let hb = hitbox.clone();
        window.on_mouse_event(move |ev: &MouseMoveEvent, phase, window, cx| {
            if phase != DispatchPhase::Bubble {
                return;
            }
            let dragging = e.read(cx).drag.is_some() || e.read(cx).multi.is_some();
            if dragging || hb.is_hovered(window) {
                e.update(cx, |v, cx| v.on_move(ev, window, cx));
            }
        });
    }
    {
        let e = entity;
        window.on_mouse_event(move |ev: &MouseUpEvent, phase, window, cx| {
            if phase == DispatchPhase::Bubble && e.read(cx).drag.is_some() {
                e.update(cx, |v, cx| v.on_up(ev, window, cx));
            }
        });
    }
}

fn handle_cursor(h: Handle, angle: f64) -> CursorStyle {
    if h == Handle::Rotate {
        return CursorStyle::PointingHand;
    }
    let base = match h {
        Handle::E | Handle::W => 0.0,
        Handle::NE | Handle::SW => 45.0,
        Handle::N | Handle::S => 90.0,
        Handle::NW | Handle::SE => 135.0,
        Handle::Rotate => 0.0,
    };
    let a = (base - angle.to_degrees()).rem_euclid(180.0);
    let k = ((a + 22.5) / 45.0).floor() as i32 % 4;
    match k {
        0 => CursorStyle::ResizeLeftRight,
        1 => CursorStyle::ResizeUpRightDownLeft,
        2 => CursorStyle::ResizeUpDown,
        _ => CursorStyle::ResizeUpLeftDownRight,
    }
}

fn paint_overlay(v: &ExcalidrawView, style: &ExcalidrawStyle, pal: Palette, origin: Point<Pixels>, window: &mut Window) {
    let cam = v.cam;
    let sel = hsla_rgba(style.selection);
    let white = if pal.dark { Rgba::new(0.07, 0.07, 0.07, 1.0) } else { Rgba::new(1.0, 1.0, 1.0, 1.0) };
    let pad = 4.0 / cam.zoom;

    // Bind hint: highlight the target shape.
    if let Some(id) = &v.bind_hint {
        if let Some(t) = v.scene.get(id) {
            let b = geom::local_bounds(t);
            let r = [b[0] - 3.0, b[1] - 3.0, b[2] + 3.0, b[3] + 3.0];
            render::stroke_screen(&rect_screen(cam, origin, t, r), true, 4.0, None, sel.with_alpha(0.35), window);
        }
    }

    // Text caret.
    if let Some(ed) = &v.editing {
        if let Some(el) = v.scene.get(&ed.id) {
            if let Some((x, y, lh)) = v.caret_local() {
                let a = cam.to_screen(origin, geom::to_world(el, [x, y]));
                let b = cam.to_screen(origin, geom::to_world(el, [x, y + lh]));
                let c = pal.resolve(el.stroke_color());
                render::stroke_screen(&[a, b], false, 1.5, None, c, window);
            }
            // Editing box.
            let r = [-pad, -pad, el.width() + pad, el.height().max(el.font_size() * el.line_height()) + pad];
            render::stroke_screen(&rect_screen(cam, origin, el, r), true, 1.0, None, sel.with_alpha(0.5), window);
        }
    }

    // Selection outlines.
    let list = v.selection_list();
    let groupish = list.len() > 1;
    if v.editing.is_none() && v.multi.is_none() {
        for el in &list {
            if el.container_id().is_some() {
                continue;
            }
            if el.is_linear() && !groupish {
                // Linear: outline the points' bbox lightly.
                let b = geom::local_bounds(el);
                let r = [b[0] - pad, b[1] - pad, b[2] + pad, b[3] + pad];
                render::stroke_screen(&rect_screen(cam, origin, el, r), true, 1.0, Some([4.0, 4.0]), sel.with_alpha(0.6), window);
                continue;
            }
            let b = geom::local_bounds(el);
            let r = [b[0] - pad, b[1] - pad, b[2] + pad, b[3] + pad];
            render::stroke_screen(&rect_screen(cam, origin, el, r), true, 1.0, if groupish { Some([4.0, 4.0]) } else { None }, sel, window);
        }
        if groupish {
            if let Some(b) = v.selection_bbox() {
                let pts = [[b[0] - pad, b[1] - pad], [b[2] + pad, b[1] - pad], [b[2] + pad, b[3] + pad], [b[0] - pad, b[3] + pad]]
                    .map(|q| cam.to_screen(origin, q));
                render::stroke_screen(&pts, true, 1.0, None, sel, window);
            }
        }
        for (h, q) in v.handles() {
            let at = cam.to_screen(origin, q);
            render::handle(at, if h == Handle::Rotate { 4.5 } else { 4.0 }, h == Handle::Rotate, white, sel, window);
        }
        for (_, q, mid) in v.point_handles() {
            let at = cam.to_screen(origin, q);
            if mid {
                render::handle(at, 3.5, true, sel.with_alpha(0.25), sel.with_alpha(0.6), window);
            } else {
                render::handle(at, 5.0, true, white, sel, window);
            }
        }
    }

    // Multi-point preview: show committed points.
    if let Some(id) = &v.multi {
        if let Some(el) = v.scene.get(id) {
            let pts = el.points();
            for p in &pts[..pts.len().saturating_sub(1)] {
                render::handle(cam.to_screen(origin, geom::to_world(el, *p)), 4.0, true, white, sel, window);
            }
        }
    }

    // Marquee.
    if let Some(Drag::Marquee { start, current }) = &v.drag {
        let a = cam.to_screen(origin, *start);
        let b = cam.to_screen(origin, *current);
        let r = Bounds::from_corners(point(a.x.min(b.x), a.y.min(b.y)), point(a.x.max(b.x), a.y.max(b.y)));
        window.paint_quad(fill(r, sel.with_alpha(0.08).to_hsla()));
        let pts = [r.origin, r.top_right(), r.bottom_right(), r.bottom_left()];
        render::stroke_screen(&pts, true, 1.0, None, sel, window);
    }

    // Eraser trail cursor.
    if v.tool == Tool::Eraser {
        if let Some(p) = v.pointer {
            let pts: Vec<Point<Pixels>> = (0..20)
                .map(|k| {
                    let a = k as f32 / 20.0 * std::f32::consts::TAU;
                    point(p.x + px(5.0 * a.cos()), p.y + px(5.0 * a.sin()))
                })
                .collect();
            let c = if pal.dark { Rgba::new(0.8, 0.8, 0.8, 0.8) } else { Rgba::new(0.2, 0.2, 0.2, 0.6) };
            render::stroke_screen(&pts, true, 1.0, None, c, window);
        }
    }
}

// ---------------------------------------------------------------- chrome helpers

fn island(style: &ExcalidrawStyle) -> Div {
    div()
        .occlude()
        .flex()
        .items_center()
        .bg(style.island_bg)
        .rounded(px(8.0))
        .border_1()
        .border_color(style.island_border)
        .shadow_sm()
        .p(px(4.0))
        .gap(px(2.0))
}

fn tool_button(
    id: &'static str,
    ic: Icon,
    hint: Option<&'static str>,
    active: bool,
    style: &ExcalidrawStyle,
    on: impl Fn(&mut ExcalidrawView, &mut Window, &mut Context<ExcalidrawView>) + 'static,
    cx: &mut Context<ExcalidrawView>,
) -> Stateful<Div> {
    let fg = if active { style.active_fg } else { style.fg };
    div()
        .id(id)
        .relative()
        .size(px(36.0))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(8.0))
        .when(active, |d| d.bg(style.active_bg))
        .when(!active, |d| d.hover(|s| s.bg(style.hover_bg)))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
            on(this, window, cx);
            cx.notify();
        }))
        .child(icon(ic, 17.0, fg))
        .when_some(hint, |d, h| {
            d.child(
                div()
                    .absolute()
                    .bottom(px(1.0))
                    .right(px(3.0))
                    .text_size(px(9.0))
                    .text_color(style.muted)
                    .child(h),
            )
        })
}

fn text_button(
    id: SharedString,
    label: impl Into<SharedString>,
    active: bool,
    style: &ExcalidrawStyle,
    on: impl Fn(&mut ExcalidrawView, &mut Window, &mut Context<ExcalidrawView>) + 'static,
    cx: &mut Context<ExcalidrawView>,
) -> Stateful<Div> {
    div()
        .id(id)
        .min_w(px(28.0))
        .h(px(28.0))
        .px(px(6.0))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(6.0))
        .text_size(px(12.0))
        .text_color(if active { style.active_fg } else { style.fg })
        .when(active, |d| d.bg(style.active_bg))
        .when(!active, |d| d.bg(style.hover_bg.opacity(0.5)).hover(|s| s.bg(style.hover_bg)))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
            on(this, window, cx);
            cx.notify();
        }))
        .child(label.into())
}

fn swatch(
    id: SharedString,
    css: &'static str,
    active: bool,
    pal: Palette,
    style: &ExcalidrawStyle,
    on: impl Fn(&mut ExcalidrawView, &mut Window, &mut Context<ExcalidrawView>) + 'static,
    cx: &mut Context<ExcalidrawView>,
) -> Stateful<Div> {
    let transparent = color::is_transparent(css);
    let c = if transparent { style.island_bg } else { pal.resolve(css).to_hsla() };
    div()
        .id(id)
        .size(px(22.0))
        .rounded(px(4.0))
        .bg(c)
        .border_1()
        .border_color(if active { style.selection } else { style.island_border })
        .when(active, |d| d.border_2())
        .when(transparent, |d| {
            d.child(
                div().size_full().flex().items_center().justify_center().text_size(px(10.0)).text_color(style.muted).child("∅"),
            )
        })
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
            on(this, window, cx);
            cx.notify();
        }))
}

fn section(title: &'static str, style: &ExcalidrawStyle) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(px(4.0))
        .child(div().text_size(px(11.0)).text_color(style.muted).child(title))
}

fn row() -> Div {
    div().flex().flex_wrap().gap(px(4.0))
}

const STROKES: [&str; 5] = ["#1e1e1e", "#e03131", "#2f9e44", "#1971c2", "#f08c00"];
const BACKGROUNDS: [&str; 5] = ["transparent", "#ffc9c9", "#b2f2bb", "#a5d8ff", "#ffec99"];
const CANVAS_BG: [&str; 5] = ["#ffffff", "#f8f9fa", "#f5faff", "#fffce8", "#fdf8f6"];

// ---------------------------------------------------------------- render

impl Render for ExcalidrawView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let style = self.style(cx);
        let pal = self.palette(cx);
        let entity = cx.entity();

        let root = div()
            .id("excalidraw")
            .relative()
            .size_full()
            .overflow_hidden()
            .bg(pal.canvas.to_hsla())
            .track_focus(&self.focus)
            .key_context("Excalidraw")
            .on_key_down(cx.listener(|this, ev: &KeyDownEvent, window, cx| this.on_key_down(ev, window, cx)))
            .on_key_up(cx.listener(|this, ev: &KeyUpEvent, window, cx| this.on_key_up(ev, window, cx)))
            .on_scroll_wheel(cx.listener(|this, ev: &ScrollWheelEvent, window, cx| this.on_scroll(ev, window, cx)))
            .on_pinch(cx.listener(|this, ev: &PinchEvent, window, cx| this.on_pinch(ev, window, cx)))
            .on_drop(cx.listener(|this, paths: &ExternalPaths, window, cx| this.on_drop_paths(paths, window, cx)));

        if let Some(err) = self.error.clone() {
            return root.child(
                div()
                    .size_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_color(style.fg)
                    .child(err),
            );
        }

        let _ = window;
        root.child(
            canvas(
                |bounds, window, _| window.insert_hitbox(bounds, HitboxBehavior::Normal),
                move |bounds, hitbox, window, cx| paint_canvas(bounds, hitbox, Snapshot { view: entity }, window, cx),
            )
                .absolute()
                .size_full(),
        )
        .child(self.render_top(&style, cx))
        .when(self.show_properties(), |d| d.child(self.render_properties(&style, pal, cx)))
        .child(self.render_bottom(&style, cx))
        .when(self.menu_open, |d| d.child(self.render_menu(&style, pal, cx)))
    }
}

impl ExcalidrawView {
    fn show_properties(&self) -> bool {
        (!self.selected.is_empty() && self.editing.is_none() || self.editing.is_some())
            || matches!(
                self.tool,
                Tool::Rectangle | Tool::Diamond | Tool::Ellipse | Tool::Arrow | Tool::Line | Tool::Freedraw | Tool::Text
            )
    }

    fn render_top(&self, style: &ExcalidrawStyle, cx: &mut Context<Self>) -> impl IntoElement {
        let t = self.tool;
        let tools: [(&'static str, Icon, Option<&'static str>, Tool); 11] = [
            ("t-hand", Icon::Hand, None, Tool::Hand),
            ("t-sel", Icon::Selection, Some("1"), Tool::Selection),
            ("t-rect", Icon::Rectangle, Some("2"), Tool::Rectangle),
            ("t-dia", Icon::Diamond, Some("3"), Tool::Diamond),
            ("t-ell", Icon::Ellipse, Some("4"), Tool::Ellipse),
            ("t-arr", Icon::Arrow, Some("5"), Tool::Arrow),
            ("t-line", Icon::Line, Some("6"), Tool::Line),
            ("t-draw", Icon::Pencil, Some("7"), Tool::Freedraw),
            ("t-text", Icon::Text, Some("8"), Tool::Text),
            ("t-img", Icon::Image, Some("9"), Tool::Image),
            ("t-eras", Icon::Eraser, Some("0"), Tool::Eraser),
        ];
        let sep = || div().w(px(1.0)).h(px(24.0)).mx(px(4.0)).bg(style.island_border);
        let mut bar = island(style).child(tool_button(
            "t-lock",
            if self.tool_locked { Icon::Lock } else { Icon::Unlock },
            None,
            self.tool_locked,
            style,
            |v, _, _| v.tool_locked = !v.tool_locked,
            cx,
        ));
        bar = bar.child(sep());
        for (id, ic, hint, tool) in tools {
            bar = bar.child(tool_button(id, ic, hint, t == tool, style, move |v, w, cx| v.set_tool(tool, w, cx), cx));
        }
        bar = bar.child(sep()).child(tool_button(
            "t-frame",
            Icon::Frame,
            Some("F"),
            t == Tool::Frame,
            style,
            |v, w, cx| v.set_tool(Tool::Frame, w, cx),
            cx,
        ));

        let menu = div().absolute().top(px(16.0)).left(px(16.0)).child(
            island(style).p(px(0.0)).child(tool_button(
                "menu",
                Icon::Menu,
                None,
                self.menu_open,
                style,
                |v, _, _| v.menu_open = !v.menu_open,
                cx,
            )),
        );

        div()
            .absolute()
            .top(px(0.0))
            .left(px(0.0))
            .right(px(0.0))
            .child(menu)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .pt(px(16.0))
                    .gap(px(6.0))
                    .child(bar)
                    .when_some(self.hint(), |d, h| {
                        d.child(div().text_size(px(12.0)).text_color(style.muted).child(h))
                    }),
            )
    }

    fn render_bottom(&self, style: &ExcalidrawStyle, cx: &mut Context<Self>) -> impl IntoElement {
        let zoom = format!("{}%", (self.cam.zoom * 100.0).round());
        let undo_c = if self.can_undo() { style.fg } else { style.muted.opacity(0.5) };
        let redo_c = if self.can_redo() { style.fg } else { style.muted.opacity(0.5) };
        let plain = |id: &'static str, ic: Icon, c: Hsla, on: fn(&mut ExcalidrawView, &mut Context<ExcalidrawView>), cx: &mut Context<Self>| {
            div()
                .id(id)
                .size(px(32.0))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(6.0))
                .hover(|s| s.bg(style.hover_bg))
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    on(this, cx);
                    cx.notify();
                }))
                .child(icon(ic, 15.0, c))
        };
        div()
            .absolute()
            .bottom(px(16.0))
            .left(px(16.0))
            .flex()
            .gap(px(8.0))
            .child(
                island(style)
                    .child(plain("z-out", Icon::Minus, style.fg, |v, _| v.zoom_step(1.0 / 1.1, None), cx))
                    .child(
                        div()
                            .id("z-reset")
                            .min_w(px(48.0))
                            .flex()
                            .justify_center()
                            .text_size(px(12.0))
                            .text_color(style.fg)
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                let z = this.cam.zoom;
                                this.zoom_step(1.0 / z, None);
                                cx.notify();
                            }))
                            .child(zoom),
                    )
                    .child(plain("z-in", Icon::Plus, style.fg, |v, _| v.zoom_step(1.1, None), cx)),
            )
            .child(
                island(style)
                    .child(plain("undo", Icon::Undo, undo_c, |v, cx| v.undo(cx), cx))
                    .child(plain("redo", Icon::Redo, redo_c, |v, cx| v.redo(cx), cx)),
            )
    }

    fn render_menu(&self, style: &ExcalidrawStyle, pal: Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let bg = self.scene.view_background();
        let item = |id: &'static str, label: &'static str, on: fn(&mut ExcalidrawView, &mut Context<ExcalidrawView>), cx: &mut Context<Self>| {
            div()
                .id(id)
                .px(px(10.0))
                .py(px(6.0))
                .rounded(px(6.0))
                .text_size(px(13.0))
                .text_color(style.fg)
                .hover(|s| s.bg(style.hover_bg))
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    on(this, cx);
                    this.menu_open = false;
                    cx.notify();
                }))
                .child(label)
        };
        let mut swatches = row();
        for c in CANVAS_BG {
            let active = bg.eq_ignore_ascii_case(c);
            swatches = swatches.child(swatch(format!("cbg-{c}").into(), c, active, pal, style, move |v, _, cx| v.set_background(c, cx), cx));
        }
        div().absolute().top(px(64.0)).left(px(16.0)).child(
            island(style)
                .flex_col()
                .items_start()
                .w(px(220.0))
                .p(px(8.0))
                .gap(px(4.0))
                .child(item("m-fit", "Zoom to fit all elements", |v, _| v.zoom_to_fit(), cx))
                .child(item("m-reset", "Reset the canvas", |v, cx| v.reset_canvas(cx), cx))
                .child(div().h(px(1.0)).w_full().bg(style.island_border))
                .child(section("Canvas background", style).child(swatches)),
        )
    }

    fn render_properties(&self, style: &ExcalidrawStyle, pal: Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let list = self.selection_list();
        let d = &self.defaults;
        let first = list.first().copied();
        // What's being styled: the selection, or the active tool's kind.
        let kinds: HashSet<String> = if list.is_empty() {
            self.tool.element_kind().map(|k| k.to_string()).into_iter().collect()
        } else {
            let mut k: HashSet<String> = list.iter().map(|e| e.kind.clone()).collect();
            // A container with bound text gets the text section too.
            for e in &list {
                if e.bound_elements().iter().any(|b| b.kind == "text") {
                    k.insert("text".into());
                }
            }
            k
        };
        let has = |k: &[&str]| kinds.iter().any(|x| k.contains(&x.as_str()));
        let stroke = first.map(|e| e.stroke_color().to_string()).unwrap_or(d.stroke_color.clone());
        let bgc = first.map(|e| e.background_color().to_string()).unwrap_or(d.background_color.clone());
        let fill_s = first.map(|e| e.fill_style().to_string()).unwrap_or(d.fill_style.clone());
        let sw = first.map(|e| e.stroke_width()).unwrap_or(d.stroke_width);
        let ss = first.map(|e| e.stroke_style().to_string()).unwrap_or(d.stroke_style.clone());
        let rough = first.map(|e| e.roughness()).unwrap_or(d.roughness);
        let op = first.map(|e| e.opacity()).unwrap_or(d.opacity);
        let round = first.map(|e| e.f.roundness.get().is_some()).unwrap_or(d.round);
        let text_el = list
            .iter()
            .find(|e| e.kind == "text")
            .copied()
            .or_else(|| {
                list.iter()
                    .find_map(|e| e.bound_elements().iter().find(|b| b.kind == "text").and_then(|b| self.scene.get(&b.id)))
            });
        let fs = text_el.map(|e| e.font_size()).unwrap_or(d.font_size);
        let ff = text_el.map(|e| e.font_family()).unwrap_or(d.font_family);
        let ta = text_el.map(|e| e.text_align().to_string()).unwrap_or(d.text_align.clone());
        let arrow = list.iter().find(|e| e.kind == "arrow").copied();
        let sa = arrow.map(|e| e.f.start_arrowhead.get().cloned()).unwrap_or(d.start_arrowhead.clone());
        let ea = arrow.map(|e| e.f.end_arrowhead.get().cloned()).unwrap_or(d.end_arrowhead.clone());

        let mut panel = island(style)
            .id("props")
            .flex_col()
            .items_start()
            .w(px(204.0))
            .p(px(12.0))
            .gap(px(10.0))
            .max_h(px(560.0))
            .overflow_y_scroll();

        let shape_like = has(&["rectangle", "diamond", "ellipse", "arrow", "line", "freedraw", "text", "frame"]) || kinds.is_empty();
        if shape_like {
            let mut r = row();
            for c in STROKES {
                let active = stroke.eq_ignore_ascii_case(c);
                r = r.child(swatch(format!("s-{c}").into(), c, active, pal, style, move |v, _, cx| {
                    v.apply(|e, d| { e.f.stroke_color.set(c.into()); d.stroke_color = c.into(); }, cx)
                }, cx));
            }
            panel = panel.child(section("Stroke", style).child(r));
        }
        if has(&["rectangle", "diamond", "ellipse", "line", "freedraw"]) {
            let mut r = row();
            for c in BACKGROUNDS {
                let active = bgc.eq_ignore_ascii_case(c);
                r = r.child(swatch(format!("b-{c}").into(), c, active, pal, style, move |v, _, cx| {
                    v.apply(|e, d| { e.f.background_color.set(c.into()); d.background_color = c.into(); }, cx)
                }, cx));
            }
            panel = panel.child(section("Background", style).child(r));
            if !color::is_transparent(&bgc) {
                let mut r = row();
                for (k, label) in [("hachure", "Hachure"), ("cross-hatch", "Cross"), ("solid", "Solid")] {
                    r = r.child(text_button(format!("fill-{k}").into(), label, fill_s == k, style, move |v, _, cx| {
                        v.apply(|e, d| { e.f.fill_style.set(k.into()); d.fill_style = k.into(); }, cx)
                    }, cx));
                }
                panel = panel.child(section("Fill", style).child(r));
            }
        }
        if has(&["rectangle", "diamond", "ellipse", "arrow", "line", "freedraw"]) {
            let mut r = row();
            for (w, label) in [(1.0, "Thin"), (2.0, "Bold"), (4.0, "Extra")] {
                r = r.child(text_button(format!("sw-{w}").into(), label, (sw - w).abs() < 0.01, style, move |v, _, cx| {
                    v.apply(|e, d| { e.f.stroke_width.set(Num(w)); d.stroke_width = w; }, cx)
                }, cx));
            }
            panel = panel.child(section("Stroke width", style).child(r));
        }
        if has(&["rectangle", "diamond", "ellipse", "arrow", "line"]) {
            let mut r = row();
            for (k, label) in [("solid", "───"), ("dashed", "- - -"), ("dotted", "· · ·")] {
                r = r.child(text_button(format!("ss-{k}").into(), label, ss == k, style, move |v, _, cx| {
                    v.apply(|e, d| { e.f.stroke_style.set(k.into()); d.stroke_style = k.into(); }, cx)
                }, cx));
            }
            panel = panel.child(section("Stroke style", style).child(r));
            let mut r = row();
            for (k, label) in [(0.0, "Architect"), (1.0, "Artist"), (2.0, "Cartoonist")] {
                r = r.child(text_button(format!("rg-{k}").into(), label, (rough - k).abs() < 0.01, style, move |v, _, cx| {
                    v.apply(|e, d| { e.f.roughness.set(Num(k)); d.roughness = k; }, cx)
                }, cx));
            }
            panel = panel.child(section("Sloppiness", style).child(r));
        }
        if has(&["rectangle", "diamond", "line", "arrow"]) {
            let mut r = row();
            for (k, label) in [(false, "Sharp"), (true, "Round")] {
                r = r.child(text_button(format!("edge-{k}").into(), label, round == k, style, move |v, _, cx| {
                    v.apply(
                        |e, d| {
                            d.round = k;
                            if e.kind.is_empty() {
                                return;
                            }
                            if k {
                                let kind = if matches!(e.kind.as_str(), "line" | "arrow" | "diamond") { 2.0 } else { 3.0 };
                                e.f.roundness.set(Roundness { kind: Num(kind), value: Default::default(), extra: Default::default() });
                            } else {
                                e.f.roundness.set_null();
                            }
                        },
                        cx,
                    )
                }, cx));
            }
            panel = panel.child(section("Edges", style).child(r));
        }
        if has(&["arrow"]) {
            let heads: [(Option<&'static str>, &'static str); 5] =
                [(None, "None"), (Some("arrow"), "Arrow"), (Some("triangle"), "Tri"), (Some("bar"), "Bar"), (Some("dot"), "Dot")];
            for (start, title) in [(true, "Start arrowhead"), (false, "End arrowhead")] {
                let cur = if start { sa.clone() } else { ea.clone() };
                let mut r = row();
                for (k, label) in heads {
                    let active = cur.as_deref() == k;
                    r = r.child(text_button(format!("ah-{start}-{label}").into(), label, active, style, move |v, _, cx| {
                        v.apply(
                            |e, d| {
                                let f = if start { &mut e.f.start_arrowhead } else { &mut e.f.end_arrowhead };
                                if e.kind == "arrow" {
                                    match k {
                                        Some(k) => f.set(k.into()),
                                        None => f.set_null(),
                                    }
                                }
                                if start { d.start_arrowhead = k.map(String::from) } else { d.end_arrowhead = k.map(String::from) }
                            },
                            cx,
                        )
                    }, cx));
                }
                panel = panel.child(section(title, style).child(r));
            }
        }
        if has(&["text"]) {
            let mut r = row();
            for (s, label) in [(16.0, "S"), (20.0, "M"), (28.0, "L"), (36.0, "XL")] {
                r = r.child(text_button(format!("fs-{s}").into(), label, (fs - s).abs() < 0.01, style, move |v, _, cx| {
                    v.apply(|e, d| { if e.kind == "text" { e.f.font_size.set(Num(s)); } d.font_size = s; }, cx)
                }, cx));
            }
            panel = panel.child(section("Font size", style).child(r));
            let mut r = row();
            for (f, label) in [(5u32, "Hand"), (6, "Normal"), (8, "Code"), (1, "Virgil"), (2, "Helvetica"), (3, "Cascadia")] {
                r = r.child(text_button(format!("ff-{f}").into(), label, ff == f, style, move |v, _, cx| {
                    v.apply(
                        |e, d| {
                            if e.kind == "text" {
                                e.f.font_family.set(Num(f as f64));
                                e.f.line_height.set(Num(text::default_line_height(f)));
                            }
                            d.font_family = f;
                        },
                        cx,
                    )
                }, cx));
            }
            panel = panel.child(section("Font family", style).child(r));
            let mut r = row();
            for (k, label) in [("left", "Left"), ("center", "Center"), ("right", "Right")] {
                r = r.child(text_button(format!("ta-{k}").into(), label, ta == k, style, move |v, _, cx| {
                    v.apply(|e, d| { if e.kind == "text" { e.f.text_align.set(k.into()); } d.text_align = k.into(); }, cx)
                }, cx));
            }
            panel = panel.child(section("Text align", style).child(r));
        }
        // Opacity.
        {
            let r = row()
                .items_center()
                .child(text_button("op-".into(), "−", false, style, move |v, _, cx| {
                    let o = (op - 10.0).max(0.0);
                    v.apply(move |e, d| { e.f.opacity.set(Num(o)); d.opacity = o; }, cx)
                }, cx))
                .child(div().min_w(px(36.0)).flex().justify_center().text_size(px(12.0)).text_color(style.fg).child(format!("{}", op.round())))
                .child(text_button("op+".into(), "+", false, style, move |v, _, cx| {
                    let o = (op + 10.0).min(100.0);
                    v.apply(move |e, d| { e.f.opacity.set(Num(o)); d.opacity = o; }, cx)
                }, cx));
            panel = panel.child(section("Opacity", style).child(r));
        }
        if !list.is_empty() {
            let mut r = row();
            for (k, label, fwd, end) in [("back", "⇊", false, true), ("bwd", "↓", false, false), ("fwd", "↑", true, false), ("front", "⇈", true, true)] {
                r = r.child(text_button(format!("z-{k}").into(), label, false, style, move |v, _, cx| {
                    let s = v.selected.clone();
                    v.scene.reorder(&s, fwd, end);
                    v.commit(cx);
                }, cx));
            }
            panel = panel.child(section("Layers", style).child(r));
            let grouped = list.iter().any(|e| !e.group_ids().is_empty());
            let r = row()
                .child(text_button("a-dup".into(), "Duplicate", false, style, |v, _, cx| {
                    v.duplicate_selection(10.0, cx);
                    v.commit(cx);
                }, cx))
                .child(text_button("a-del".into(), "Delete", false, style, |v, _, cx| v.delete_selection(cx), cx))
                .when(list.len() > 1, |r| {
                    r.child(text_button("a-grp".into(), "Group", false, style, |v, _, cx| {
                        let s = v.selected.clone();
                        v.scene.group(&s);
                        v.commit(cx);
                    }, cx))
                })
                .when(grouped, |r| {
                    r.child(text_button("a-ungrp".into(), "Ungroup", false, style, |v, _, cx| {
                        let s = v.selected.clone();
                        v.scene.ungroup(&s);
                        v.commit(cx);
                    }, cx))
                })
                .child(text_button("a-lock".into(), "Lock", false, style, |v, _, cx| {
                    let s = v.selected.clone();
                    for id in &s {
                        if let Some(e) = v.scene.get_mut(id) {
                            e.f.locked.set(true);
                        }
                    }
                    v.selected.clear();
                    v.commit(cx);
                }, cx));
            panel = panel.child(section("Actions", style).child(r));
        }
        div().absolute().top(px(72.0)).left(px(16.0)).child(panel)
    }
}
