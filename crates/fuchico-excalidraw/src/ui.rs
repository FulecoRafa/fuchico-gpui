//! Rendering of `ExcalidrawView`: the canvas element (scene + overlay +
//! pointer wiring) and the Excalidraw-style chrome (toolbar island, hint,
//! properties panel, zoom/undo islands, main menu).

use std::collections::HashSet;

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::actions::{PALETTE, PALETTE_KEYS, PickerTarget};
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
    /// Filled polygon.
    Solid(&'static [[f32; 2]]),
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
    /// Library sidebar button (open book).
    Book,
    /// "More tools" (stock: circle, triangle and square).
    Extra,
    Laser,
    Mermaid,
    Search,
    External,
    Download,
    Trash,
    Group,
    Ungroup,
    FillHachure,
    FillCross,
    FillSolid,
    WidthThin,
    WidthBold,
    WidthExtra,
    StyleSolid,
    StyleDashed,
    StyleDotted,
    RoughArchitect,
    RoughArtist,
    RoughCartoonist,
    EdgeSharp,
    EdgeRound,
    ArrowSharp,
    ArrowCurved,
    ArrowElbow,
    Copy,
    /// Hyperlink (chain).
    Link,
    Edit,
}

fn icon_parts(i: Icon) -> Vec<Part> {
    use Part::*;
    match i {
        Icon::Lock => vec![
            RoundRect([5.0, 11.0, 19.0, 21.0], 2.0),
            Poly(
                &[
                    [8.0, 11.0],
                    [8.0, 7.5],
                    [10.0, 4.5],
                    [14.0, 4.5],
                    [16.0, 7.5],
                    [16.0, 11.0],
                ],
                false,
            ),
        ],
        Icon::Unlock => vec![
            RoundRect([5.0, 11.0, 19.0, 21.0], 2.0),
            Poly(
                &[
                    [8.0, 11.0],
                    [8.0, 7.5],
                    [10.0, 4.5],
                    [14.0, 4.5],
                    [16.0, 7.0],
                ],
                false,
            ),
        ],
        Icon::Hand => vec![
            Poly(&[[8.0, 13.0], [8.0, 6.5]], false),
            Poly(&[[11.0, 11.5], [11.0, 4.5]], false),
            Poly(&[[14.0, 11.5], [14.0, 5.5]], false),
            Poly(&[[17.0, 12.5], [17.0, 8.5]], false),
            Poly(
                &[
                    [17.0, 12.5],
                    [17.0, 16.0],
                    [15.0, 20.0],
                    [10.5, 20.0],
                    [7.0, 16.5],
                    [4.5, 12.5],
                    [6.0, 11.5],
                    [8.0, 13.0],
                ],
                false,
            ),
        ],
        Icon::Selection => vec![Poly(
            &[
                [6.0, 4.0],
                [6.0, 19.0],
                [10.0, 15.5],
                [12.5, 21.0],
                [15.0, 20.0],
                [12.5, 14.5],
                [18.0, 14.5],
            ],
            true,
        )],
        Icon::Rectangle => vec![RoundRect([4.0, 4.0, 20.0, 20.0], 3.0)],
        Icon::Diamond => vec![Poly(
            &[[12.0, 3.0], [21.0, 12.0], [12.0, 21.0], [3.0, 12.0]],
            true,
        )],
        Icon::Ellipse => vec![Circle([12.0, 12.0], 8.5)],
        Icon::Arrow => vec![
            Poly(&[[4.0, 12.0], [20.0, 12.0]], false),
            Poly(&[[14.0, 6.0], [20.0, 12.0], [14.0, 18.0]], false),
        ],
        Icon::Line => vec![Poly(&[[4.0, 12.0], [20.0, 12.0]], false)],
        Icon::Pencil => vec![
            Poly(
                &[
                    [4.0, 20.0],
                    [5.0, 15.0],
                    [15.5, 4.5],
                    [19.5, 8.5],
                    [9.0, 19.0],
                ],
                true,
            ),
            Poly(&[[13.5, 6.5], [17.5, 10.5]], false),
        ],
        // Stock's stylised "A" (outlined letter with a cross bar).
        Icon::Text => vec![
            Poly(
                &[[5.0, 20.0], [11.0, 4.0], [13.0, 4.0], [19.0, 20.0]],
                false,
            ),
            Poly(&[[8.0, 20.0], [12.0, 9.0], [16.0, 20.0]], false),
            Poly(&[[8.5, 15.0], [15.5, 15.0]], false),
        ],
        Icon::Image => vec![
            RoundRect([4.0, 5.0, 20.0, 19.0], 2.0),
            Dot([9.0, 10.0], 1.5),
            Poly(
                &[
                    [4.0, 17.0],
                    [9.5, 12.0],
                    [13.5, 15.5],
                    [16.0, 13.0],
                    [20.0, 16.5],
                ],
                false,
            ),
        ],
        Icon::Eraser => vec![
            Poly(
                &[
                    [19.0, 20.0],
                    [9.0, 20.0],
                    [4.0, 15.0],
                    [14.0, 5.0],
                    [20.0, 11.0],
                    [11.0, 20.0],
                ],
                false,
            ),
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
            Poly(
                &[
                    [4.0, 9.0],
                    [14.5, 9.0],
                    [18.0, 10.5],
                    [20.0, 14.0],
                    [18.0, 17.5],
                    [14.5, 19.0],
                    [11.0, 19.0],
                ],
                false,
            ),
        ],
        Icon::Redo => vec![
            Poly(&[[15.0, 14.0], [20.0, 9.0], [15.0, 4.0]], false),
            Poly(
                &[
                    [20.0, 9.0],
                    [9.5, 9.0],
                    [6.0, 10.5],
                    [4.0, 14.0],
                    [6.0, 17.5],
                    [9.5, 19.0],
                    [13.0, 19.0],
                ],
                false,
            ),
        ],
        Icon::Minus => vec![Poly(&[[6.0, 12.0], [18.0, 12.0]], false)],
        Icon::Book => vec![
            Poly(
                &[
                    [3.0, 5.0],
                    [9.0, 5.0],
                    [12.0, 7.0],
                    [12.0, 20.0],
                    [9.0, 18.0],
                    [3.0, 18.0],
                ],
                true,
            ),
            Poly(
                &[
                    [21.0, 5.0],
                    [15.0, 5.0],
                    [12.0, 7.0],
                    [12.0, 20.0],
                    [15.0, 18.0],
                    [21.0, 18.0],
                ],
                true,
            ),
        ],
        Icon::Extra => vec![
            Circle([12.0, 6.5], 3.5),
            Circle([6.5, 16.5], 3.5),
            RoundRect([14.0, 13.0, 21.0, 20.0], 1.0),
        ],
        Icon::Laser => vec![
            Poly(&[[4.0, 20.0], [13.0, 11.0]], false),
            Poly(
                &[[11.0, 9.0], [15.0, 13.0], [17.0, 11.0], [13.0, 7.0]],
                true,
            ),
            Poly(&[[18.0, 4.0], [18.5, 6.0]], false),
            Poly(&[[21.0, 7.0], [19.0, 7.5]], false),
            Poly(&[[20.5, 3.5], [19.5, 5.0]], false),
        ],
        Icon::Mermaid => vec![
            Poly(
                &[
                    [4.0, 5.0],
                    [8.5, 9.0],
                    [12.0, 13.0],
                    [15.5, 9.0],
                    [20.0, 5.0],
                ],
                false,
            ),
            Poly(&[[12.0, 13.0], [12.0, 20.0]], false),
            Poly(&[[9.0, 20.0], [15.0, 20.0]], false),
        ],
        Icon::Link => vec![
            Poly(
                &[
                    [3.9, 15.9],
                    [8.9, 10.9],
                    [13.1, 10.9],
                    [13.1, 15.1],
                    [8.1, 20.1],
                    [3.9, 20.1],
                ],
                true,
            ),
            Poly(
                &[
                    [10.9, 8.9],
                    [15.9, 3.9],
                    [20.1, 3.9],
                    [20.1, 8.1],
                    [15.1, 13.1],
                    [10.9, 13.1],
                ],
                true,
            ),
        ],
        Icon::Edit => vec![
            Poly(
                &[
                    [4.0, 20.0],
                    [5.0, 15.0],
                    [16.0, 4.0],
                    [20.0, 8.0],
                    [9.0, 19.0],
                ],
                true,
            ),
            Poly(&[[13.5, 6.5], [17.5, 10.5]], false),
        ],
        Icon::Search => vec![
            Circle([10.5, 10.5], 6.0),
            Poly(&[[15.0, 15.0], [20.0, 20.0]], false),
        ],
        Icon::FillHachure => vec![
            RoundRect([4.0, 4.0, 20.0, 20.0], 3.0),
            Poly(&[[4.5, 11.5], [11.5, 4.5]], false),
            Poly(&[[5.0, 18.0], [18.0, 5.0]], false),
            Poly(&[[11.5, 19.5], [19.5, 11.5]], false),
        ],
        Icon::FillCross => vec![
            RoundRect([4.0, 4.0, 20.0, 20.0], 3.0),
            Poly(&[[4.5, 11.5], [11.5, 4.5]], false),
            Poly(&[[5.0, 18.0], [18.0, 5.0]], false),
            Poly(&[[11.5, 19.5], [19.5, 11.5]], false),
            Poly(&[[12.5, 4.5], [19.5, 11.5]], false),
            Poly(&[[6.0, 5.0], [19.0, 18.0]], false),
            Poly(&[[4.5, 12.5], [11.5, 19.5]], false),
        ],
        Icon::FillSolid => vec![Solid(&[[5.0, 5.0], [19.0, 5.0], [19.0, 19.0], [5.0, 19.0]])],
        Icon::WidthThin => vec![Solid(&[
            [5.0, 11.5],
            [19.0, 11.5],
            [19.0, 12.5],
            [5.0, 12.5],
        ])],
        Icon::WidthBold => vec![Solid(&[
            [5.0, 10.75],
            [19.0, 10.75],
            [19.0, 13.25],
            [5.0, 13.25],
        ])],
        Icon::WidthExtra => vec![Solid(&[
            [5.0, 10.0],
            [19.0, 10.0],
            [19.0, 14.0],
            [5.0, 14.0],
        ])],
        Icon::StyleSolid => vec![Poly(&[[5.0, 12.0], [19.0, 12.0]], false)],
        Icon::StyleDashed => vec![
            Poly(&[[4.0, 12.0], [8.0, 12.0]], false),
            Poly(&[[10.0, 12.0], [14.0, 12.0]], false),
            Poly(&[[16.0, 12.0], [20.0, 12.0]], false),
        ],
        Icon::StyleDotted => vec![
            Dot([5.0, 12.0], 0.9),
            Dot([9.5, 12.0], 0.9),
            Dot([14.0, 12.0], 0.9),
            Dot([18.5, 12.0], 0.9),
        ],
        Icon::RoughArchitect => vec![Poly(
            &[
                [4.0, 15.0],
                [7.0, 12.0],
                [10.0, 10.5],
                [13.0, 11.0],
                [16.0, 12.5],
                [20.0, 10.0],
            ],
            false,
        )],
        Icon::RoughArtist => vec![
            Poly(
                &[
                    [4.0, 15.0],
                    [7.0, 11.5],
                    [10.0, 9.5],
                    [12.0, 12.0],
                    [14.5, 14.5],
                    [17.0, 11.0],
                    [20.0, 9.0],
                ],
                false,
            ),
            Poly(
                &[
                    [5.0, 14.5],
                    [8.0, 11.0],
                    [11.0, 10.5],
                    [14.0, 13.5],
                    [20.0, 10.0],
                ],
                false,
            ),
        ],
        Icon::RoughCartoonist => vec![
            Poly(
                &[
                    [4.0, 14.0],
                    [6.0, 10.0],
                    [8.5, 9.5],
                    [10.0, 13.0],
                    [12.0, 15.0],
                    [14.0, 12.0],
                    [16.0, 9.0],
                    [18.0, 10.5],
                    [20.0, 13.0],
                ],
                false,
            ),
            Poly(
                &[
                    [5.0, 15.0],
                    [9.0, 10.5],
                    [13.0, 14.0],
                    [17.0, 9.5],
                    [20.0, 11.5],
                ],
                false,
            ),
        ],
        Icon::EdgeSharp => vec![Poly(&[[6.0, 19.0], [6.0, 6.0], [19.0, 6.0]], false)],
        Icon::EdgeRound => vec![Poly(
            &[
                [6.0, 19.0],
                [6.0, 11.0],
                [7.0, 8.0],
                [8.5, 6.8],
                [11.0, 6.0],
                [19.0, 6.0],
            ],
            false,
        )],
        Icon::ArrowSharp => vec![
            Poly(&[[5.0, 19.0], [19.0, 5.0]], false),
            Poly(&[[12.0, 5.0], [19.0, 5.0], [19.0, 12.0]], false),
        ],
        Icon::ArrowCurved => vec![
            Poly(
                &[
                    [5.0, 19.0],
                    [6.0, 14.0],
                    [8.5, 9.5],
                    [12.5, 6.5],
                    [18.0, 5.0],
                ],
                false,
            ),
            Poly(&[[14.0, 2.0], [18.0, 5.0], [14.5, 8.5]], false),
        ],
        Icon::ArrowElbow => vec![
            Poly(
                &[[5.0, 19.0], [12.0, 19.0], [12.0, 6.0], [19.0, 6.0]],
                false,
            ),
            Poly(&[[16.0, 3.0], [19.0, 6.0], [16.0, 9.0]], false),
        ],
        Icon::Trash => vec![
            Poly(&[[4.0, 7.0], [20.0, 7.0]], false),
            Poly(&[[9.0, 7.0], [9.0, 4.0], [15.0, 4.0], [15.0, 7.0]], false),
            Poly(&[[6.0, 7.0], [7.0, 20.0], [17.0, 20.0], [18.0, 7.0]], false),
            Poly(&[[10.0, 11.0], [10.0, 16.0]], false),
            Poly(&[[14.0, 11.0], [14.0, 16.0]], false),
        ],
        Icon::Group => vec![
            RoundRect([3.0, 3.0, 21.0, 21.0], 2.0),
            RoundRect([7.0, 7.0, 13.0, 13.0], 1.0),
            RoundRect([11.0, 11.0, 17.0, 17.0], 1.0),
        ],
        Icon::Ungroup => vec![
            RoundRect([4.0, 4.0, 12.0, 12.0], 1.5),
            RoundRect([12.0, 12.0, 20.0, 20.0], 1.5),
        ],
        Icon::Download => vec![
            Poly(&[[12.0, 4.0], [12.0, 15.0]], false),
            Poly(&[[7.0, 10.0], [12.0, 15.0], [17.0, 10.0]], false),
            Poly(
                &[[4.0, 16.0], [4.0, 20.0], [20.0, 20.0], [20.0, 16.0]],
                false,
            ),
        ],
        Icon::Copy => vec![
            RoundRect([9.0, 9.0, 20.0, 20.0], 2.0),
            Poly(
                &[
                    [15.0, 9.0],
                    [15.0, 4.0],
                    [4.0, 4.0],
                    [4.0, 15.0],
                    [9.0, 15.0],
                ],
                false,
            ),
        ],
        Icon::External => vec![
            Poly(
                &[
                    [11.0, 5.0],
                    [5.0, 5.0],
                    [5.0, 19.0],
                    [19.0, 19.0],
                    [19.0, 13.0],
                ],
                false,
            ),
            Poly(&[[13.0, 4.0], [20.0, 4.0], [20.0, 11.0]], false),
            Poly(&[[20.0, 4.0], [11.0, 13.0]], false),
        ],
        Icon::Plus => vec![
            Poly(&[[6.0, 12.0], [18.0, 12.0]], false),
            Poly(&[[12.0, 6.0], [12.0, 18.0]], false),
        ],
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
                    Part::Solid(p) => (p.to_vec(), true, true),
                };
                let mut pb = if filled {
                    PathBuilder::fill()
                } else {
                    PathBuilder::stroke(px(width))
                };
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

/// The laser pointer's fading red trail.
fn paint_laser(
    trail: &[(std::time::Instant, [f64; 2], bool)],
    cam: Camera,
    origin: Point<Pixels>,
    window: &mut Window,
) {
    let decay = crate::view::LASER_DECAY.as_secs_f32();
    for w in trail.windows(2) {
        let (t, b, new) = w[1];
        if new {
            continue;
        }
        let life = 1.0 - t.elapsed().as_secs_f32() / decay;
        if life <= 0.0 {
            continue;
        }
        let mut pb = PathBuilder::stroke(px(1.0 + 3.0 * life));
        pb.move_to(cam.to_screen(origin, w[0].1));
        pb.line_to(cam.to_screen(origin, b));
        if let Ok(p) = pb.build() {
            window.paint_path(p, hsla(0.0, 1.0, 0.6, life));
        }
    }
}

struct Snapshot {
    view: Entity<ExcalidrawView>,
}

fn rect_screen(
    cam: Camera,
    origin: Point<Pixels>,
    el: &Element,
    r: [f64; 4],
) -> Vec<Point<Pixels>> {
    [[r[0], r[1]], [r[2], r[1]], [r[2], r[3]], [r[0], r[3]]]
        .iter()
        .map(|q| cam.to_screen(origin, geom::to_world(el, *q)))
        .collect()
}

fn paint_canvas(
    bounds: Bounds<Pixels>,
    hitbox: Hitbox,
    snap: Snapshot,
    window: &mut Window,
    cx: &mut App,
) {
    let entity = snap.view.clone();
    let (editing, focus) = {
        let v = entity.read(cx);
        (v.editing.is_some(), v.focus.clone())
    };
    if editing {
        window.handle_input(&focus, ElementInputHandler::new(bounds, entity.clone()), cx);
    }
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
            if v.scene.grid_enabled() {
                paint_grid(&v.scene, cam, bounds, pal.dark, window);
            }
            let frames: Vec<&Element> = v
                .scene
                .elements()
                .filter(|e| e.kind == "frame" || e.kind == "magicframe")
                .collect();
            let frame_ids: HashSet<&str> = frames.iter().map(|f| f.id.as_str()).collect();
            let editing = v.editing.as_ref().map(|e| e.id.as_str());
            for el in v.scene.elements() {
                // Off-screen culling.
                let bb = geom::aabb(el);
                let tl = cam.to_screen(origin, [bb[0], bb[1]]);
                let br = cam.to_screen(origin, [bb[2], bb[3]]);
                let pad = px(40.0);
                if br.x < bounds.left() - pad
                    || tl.x > bounds.right() + pad
                    || br.y < bounds.top() - pad
                    || tl.y > bounds.bottom() + pad
                {
                    continue;
                }
                let alpha = if v.erasing.contains(&el.id)
                    || el
                        .container_id()
                        .map(|c| v.erasing.contains(c))
                        .unwrap_or(false)
                {
                    0.2
                } else {
                    1.0
                };
                let frame =
                    el.f.frame_id
                        .get()
                        .filter(|f| frame_ids.contains(f.as_str()));
                let paint = |window: &mut Window, caches: &mut render::Caches| {
                    // Arrow labels sit on a canvas-coloured backing that hides the line.
                    if crate::export::is_arrow_label(el, &v.scene) && !el.text().is_empty() {
                        let q = rect_screen(
                            cam,
                            origin,
                            el,
                            [-4.0, -4.0, el.width() + 4.0, el.height() + 4.0],
                        );
                        let mut pb = PathBuilder::fill();
                        pb.move_to(q[0]);
                        for p in &q[1..] {
                            pb.line_to(*p);
                        }
                        pb.close();
                        if let Ok(path) = pb.build() {
                            window.paint_path(path, pal.canvas.to_hsla());
                        }
                    }
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
            paint_laser(&v.laser, cam, origin, window);
        });
        if !v.laser.is_empty() {
            let last = v.laser.iter().map(|l| l.0).max();
            if last.is_some_and(|t| t.elapsed() < crate::view::LASER_DECAY) {
                window.request_animation_frame();
            }
        }

        // Cursor.
        let cursor = match (&v.drag, v.tool) {
            (Some(Drag::Pan { .. }), _) => CursorStyle::ClosedHand,
            _ if v.space_down => CursorStyle::OpenHand,
            (_, Tool::Laser) => CursorStyle::Crosshair,
            _ if v.view_mode => CursorStyle::OpenHand,
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
                } else if v
                    .point_handles()
                    .iter()
                    .any(|(_, q, _)| geom::dist(wp, *q) <= 8.0 / cam.zoom)
                {
                    CursorStyle::PointingHand
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

/// Excalidraw's grid: a line every `gridSize`, a stronger one every
/// `gridStep` lines; hidden when lines would be closer than 6px.
fn paint_grid(
    scene: &crate::scene::Scene,
    cam: Camera,
    bounds: Bounds<Pixels>,
    dark: bool,
    window: &mut Window,
) {
    let gs = scene.grid_size();
    let step = scene.grid_step().max(1) as i64;
    let sp = gs * cam.zoom;
    if sp < 6.0 {
        return;
    }
    let base = if dark {
        gpui_kit::white()
    } else {
        gpui_kit::black()
    };
    let (minor, major) = (base.opacity(0.05), base.opacity(0.11));
    let tl = cam.to_world(bounds.origin, bounds.origin);
    let br = cam.to_world(bounds.origin, bounds.bottom_right());
    let mut i = (tl[0] / gs).floor() as i64;
    while (i as f64) * gs <= br[0] {
        let x = cam.to_screen(bounds.origin, [i as f64 * gs, 0.0]).x;
        let c = if i.rem_euclid(step) == 0 {
            major
        } else {
            minor
        };
        window.paint_quad(fill(
            Bounds::new(point(x, bounds.top()), size(px(1.0), bounds.size.height)),
            c,
        ));
        i += 1;
    }
    let mut j = (tl[1] / gs).floor() as i64;
    while (j as f64) * gs <= br[1] {
        let y = cam.to_screen(bounds.origin, [0.0, j as f64 * gs]).y;
        let c = if j.rem_euclid(step) == 0 {
            major
        } else {
            minor
        };
        window.paint_quad(fill(
            Bounds::new(point(bounds.left(), y), size(bounds.size.width, px(1.0))),
            c,
        ));
        j += 1;
    }
}

fn paint_overlay(
    v: &ExcalidrawView,
    style: &ExcalidrawStyle,
    pal: Palette,
    origin: Point<Pixels>,
    window: &mut Window,
) {
    let cam = v.cam;
    let sel = hsla_rgba(style.selection);
    let white = if pal.dark {
        Rgba::new(0.07, 0.07, 0.07, 1.0)
    } else {
        Rgba::new(1.0, 1.0, 1.0, 1.0)
    };
    let pad = 4.0 / cam.zoom;

    // Object-snapping guides.
    for g in &v.guides {
        let a = cam.to_screen(origin, g[0]);
        let b = cam.to_screen(origin, g[1]);
        render::stroke_screen(
            &[a, b],
            false,
            1.0,
            None,
            Rgba::new(1.0, 0.42, 0.42, 1.0),
            window,
        );
    }

    // Bind hint: highlight the target shape.
    if let Some(id) = &v.bind_hint
        && let Some(t) = v.scene.get(id)
    {
        let b = geom::local_bounds(t);
        let r = [b[0] - 3.0, b[1] - 3.0, b[2] + 3.0, b[3] + 3.0];
        render::stroke_screen(
            &rect_screen(cam, origin, t, r),
            true,
            4.0,
            None,
            sel.with_alpha(0.35),
            window,
        );
    }

    // Text caret.
    if let Some(ed) = &v.editing
        && let Some(el) = v.scene.get(&ed.id)
    {
        if let Some((x, y, lh)) = v.caret_local() {
            let a = cam.to_screen(origin, geom::to_world(el, [x, y]));
            let b = cam.to_screen(origin, geom::to_world(el, [x, y + lh]));
            let c = pal.resolve(el.stroke_color());
            render::stroke_screen(&[a, b], false, 1.5, None, c, window);
        }
        // Editing box.
        let r = [
            -pad,
            -pad,
            el.width() + pad,
            el.height().max(el.font_size() * el.line_height()) + pad,
        ];
        render::stroke_screen(
            &rect_screen(cam, origin, el, r),
            true,
            1.0,
            None,
            sel.with_alpha(0.5),
            window,
        );
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
                render::stroke_screen(
                    &rect_screen(cam, origin, el, r),
                    true,
                    1.0,
                    Some([4.0, 4.0]),
                    sel.with_alpha(0.6),
                    window,
                );
                continue;
            }
            let b = geom::local_bounds(el);
            let r = [b[0] - pad, b[1] - pad, b[2] + pad, b[3] + pad];
            render::stroke_screen(
                &rect_screen(cam, origin, el, r),
                true,
                1.0,
                if groupish { Some([4.0, 4.0]) } else { None },
                sel,
                window,
            );
        }
        if groupish && let Some(b) = v.selection_bbox() {
            let pts = [
                [b[0] - pad, b[1] - pad],
                [b[2] + pad, b[1] - pad],
                [b[2] + pad, b[3] + pad],
                [b[0] - pad, b[3] + pad],
            ]
            .map(|q| cam.to_screen(origin, q));
            render::stroke_screen(&pts, true, 1.0, None, sel, window);
        }
        for (h, q) in v.handles() {
            let at = cam.to_screen(origin, q);
            render::handle(
                at,
                if h == Handle::Rotate { 4.5 } else { 4.0 },
                h == Handle::Rotate,
                white,
                sel,
                window,
            );
        }
        for (_, q, mid) in v.point_handles() {
            let at = cam.to_screen(origin, q);
            if mid {
                render::handle(
                    at,
                    3.5,
                    true,
                    sel.with_alpha(0.25),
                    sel.with_alpha(0.6),
                    window,
                );
            } else {
                render::handle(at, 5.0, true, white, sel, window);
            }
        }
    }

    // Multi-point preview: show committed points.
    if let Some(id) = &v.multi
        && let Some(el) = v.scene.get(id)
    {
        let pts = el.points();
        for p in &pts[..pts.len().saturating_sub(1)] {
            render::handle(
                cam.to_screen(origin, geom::to_world(el, *p)),
                4.0,
                true,
                white,
                sel,
                window,
            );
        }
    }

    // Marquee.
    if let Some(Drag::Marquee { start, current }) = &v.drag {
        let a = cam.to_screen(origin, *start);
        let b = cam.to_screen(origin, *current);
        let r = Bounds::from_corners(
            point(a.x.min(b.x), a.y.min(b.y)),
            point(a.x.max(b.x), a.y.max(b.y)),
        );
        window.paint_quad(fill(r, sel.with_alpha(0.08).to_hsla()));
        let pts = [r.origin, r.top_right(), r.bottom_right(), r.bottom_left()];
        render::stroke_screen(&pts, true, 1.0, None, sel, window);
    }

    // Eraser trail cursor.
    if v.tool == Tool::Eraser
        && let Some(p) = v.pointer
    {
        let pts: Vec<Point<Pixels>> = (0..20)
            .map(|k| {
                let a = k as f32 / 20.0 * std::f32::consts::TAU;
                point(p.x + px(5.0 * a.cos()), p.y + px(5.0 * a.sin()))
            })
            .collect();
        let c = if pal.dark {
            Rgba::new(0.8, 0.8, 0.8, 0.8)
        } else {
            Rgba::new(0.2, 0.2, 0.2, 0.6)
        };
        render::stroke_screen(&pts, true, 1.0, None, c, window);
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
        .debug_selector(move || id.to_string())
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
    let sel = id.to_string();
    div()
        .id(id)
        .debug_selector(move || sel)
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
        .when(!active, |d| {
            d.bg(style.hover_bg.opacity(0.5))
                .hover(|s| s.bg(style.hover_bg))
        })
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
            on(this, window, cx);
            cx.notify();
        }))
        .child(label.into())
}

fn swatch(
    id: SharedString,
    css: &str,
    active: bool,
    pal: Palette,
    style: &ExcalidrawStyle,
    on: impl Fn(&mut ExcalidrawView, &mut Window, &mut Context<ExcalidrawView>) + 'static,
    cx: &mut Context<ExcalidrawView>,
) -> Stateful<Div> {
    let transparent = color::is_transparent(css);
    let c = if transparent {
        style.island_bg
    } else {
        pal.resolve(css).to_hsla()
    };
    let sel = id.to_string();
    div()
        .id(id)
        .debug_selector(move || sel)
        .size(px(22.0))
        .rounded(px(4.0))
        .bg(c)
        .border_1()
        .border_color(if active {
            style.selection
        } else {
            style.island_border
        })
        .when(active, |d| d.border_2())
        .when(transparent, |d| {
            d.child(
                div()
                    .size_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(10.0))
                    .text_color(style.muted)
                    .child("∅"),
            )
        })
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
            on(this, window, cx);
            cx.notify();
        }))
}

fn section(title: &'static str, style: &ExcalidrawStyle) -> Div {
    div().flex().flex_col().gap(px(4.0)).child(
        div()
            .text_size(px(11.0))
            .text_color(style.muted)
            .child(title),
    )
}

/// Excalidraw's opacity slider (0-100, steps of 10 while dragging).
const SLIDER_W: f32 = 168.0;

fn opacity_slider(
    op: f64,
    track: std::rc::Rc<std::cell::Cell<Bounds<Pixels>>>,
    style: &ExcalidrawStyle,
    cx: &mut Context<ExcalidrawView>,
) -> impl IntoElement {
    let set = move |v: &mut ExcalidrawView, x: Pixels, cx: &mut Context<ExcalidrawView>| {
        let b = v.opacity_track.get();
        let w = f32::from(b.size.width).max(1.0);
        let t = (f32::from(x - b.origin.x) / w).clamp(0.0, 1.0) as f64;
        let o = (t * 100.0).round();
        v.apply(
            move |e, d| {
                e.f.opacity.set(Num(o));
                d.opacity = o;
            },
            cx,
        );
    };
    let frac = (op / 100.0).clamp(0.0, 1.0) as f32;
    div()
        .id("op-slider")
        .debug_selector(|| "op-slider".into())
        .relative()
        .w(px(SLIDER_W))
        .h(px(20.0))
        .flex()
        .items_center()
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |v, ev: &MouseDownEvent, _, cx| {
                cx.stop_propagation();
                v.slider_drag = true;
                set(v, ev.position.x, cx);
            }),
        )
        .on_mouse_move(cx.listener(move |v, ev: &MouseMoveEvent, _, cx| {
            if v.slider_drag && ev.pressed_button == Some(MouseButton::Left) {
                set(v, ev.position.x, cx);
            }
        }))
        .on_mouse_up(
            MouseButton::Left,
            cx.listener(|v, _, _, _| v.slider_drag = false),
        )
        .child(
            canvas(move |b, _, _| track.set(b), |_, _, _, _| {})
                .absolute()
                .size_full(),
        )
        .child(
            div()
                .w_full()
                .h(px(4.0))
                .rounded(px(2.0))
                .bg(style.active_bg),
        )
        .child(
            div()
                .absolute()
                .left(px(SLIDER_W * frac - 7.0))
                .size(px(14.0))
                .rounded(px(7.0))
                .bg(style.fg),
        )
}

/// A 32×32 icon option button (stock properties panel).
fn prop_button(
    id: SharedString,
    ic: Icon,
    active: bool,
    style: &ExcalidrawStyle,
    on: impl Fn(&mut ExcalidrawView, &mut Window, &mut Context<ExcalidrawView>) + 'static,
    cx: &mut Context<ExcalidrawView>,
) -> Stateful<Div> {
    let sel = id.to_string();
    div()
        .id(id)
        .debug_selector(move || sel)
        .size(px(32.0))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(8.0))
        .when(active, |d| {
            d.bg(style.active_bg)
                .border_1()
                .border_color(style.selection)
        })
        .when(!active, |d| {
            d.bg(style.hover_bg.opacity(0.5))
                .hover(|s| s.bg(style.hover_bg))
        })
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
            on(this, window, cx);
            cx.notify();
        }))
        .child(icon(
            ic,
            16.0,
            if active { style.active_fg } else { style.fg },
        ))
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
        let panel_h = (window.viewport_size().height - px(88.0)).max(px(200.0));
        let entity = cx.entity();

        let root =
            div()
                .id("excalidraw")
                .relative()
                .size_full()
                .overflow_hidden()
                .bg(pal.canvas.to_hsla())
                .track_focus(&self.focus)
                .key_context("Excalidraw")
                .on_key_down(cx.listener(|this, ev: &KeyDownEvent, window, cx| {
                    this.on_key_down(ev, window, cx)
                }))
                .on_key_up(
                    cx.listener(|this, ev: &KeyUpEvent, window, cx| this.on_key_up(ev, window, cx)),
                )
                .on_scroll_wheel(cx.listener(|this, ev: &ScrollWheelEvent, window, cx| {
                    this.on_scroll(ev, window, cx)
                }))
                .on_pinch(
                    cx.listener(|this, ev: &PinchEvent, window, cx| this.on_pinch(ev, window, cx)),
                )
                .on_drop(cx.listener(|this, paths: &ExternalPaths, window, cx| {
                    this.on_drop_paths(paths, window, cx)
                }))
                .on_drop(cx.listener(|this, d: &LibraryDrag, window, cx| {
                    let at = this.world(window.mouse_position());
                    this.insert_library_item(&d.id, Some(at), cx);
                }));

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
                move |bounds, hitbox, window, cx| {
                    paint_canvas(bounds, hitbox, Snapshot { view: entity }, window, cx)
                },
            )
            .absolute()
            .size_full(),
        )
        .child(self.render_top(&style, cx))
        .when(
            self.show_properties() && !self.zen && !self.view_mode,
            |d| d.child(self.render_properties(&style, pal, panel_h, cx)),
        )
        .children(self.render_link_icons(&style, cx))
        .when_some(self.render_link_popover(&style, cx), |d, p| d.child(p))
        .when(self.stats_open, |d| d.child(self.render_stats(&style, cx)))
        .child(self.render_bottom(&style, cx))
        .when(self.menu_open, |d| {
            d.child(self.render_menu(&style, pal, cx))
        })
        .when(self.library_open, |d| {
            d.child(self.render_library(&style, cx))
        })
        .when_some(self.picker, |d, t| {
            d.child(self.render_picker(t, &style, pal, cx))
        })
        .when(self.context_menu.is_some(), |d| {
            d.child(self.render_context_menu(&style, cx))
        })
        .when_some(self.dialog, |d, dlg| {
            d.child(self.render_dialog(dlg, &style, cx))
        })
        .when_some(self.toast.clone(), |d, t| d.child(render_toast(t, &style)))
    }
}

impl ExcalidrawView {
    pub(crate) fn show_properties(&self) -> bool {
        (!self.selected.is_empty() && self.editing.is_none() || self.editing.is_some())
            || matches!(
                self.tool,
                Tool::Rectangle
                    | Tool::Diamond
                    | Tool::Ellipse
                    | Tool::Arrow
                    | Tool::Line
                    | Tool::Freedraw
                    | Tool::Text
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
        let sep = || {
            div()
                .w(px(1.0))
                .h(px(24.0))
                .mx(px(4.0))
                .bg(style.island_border)
        };
        let mut bar = island(style).child(tool_button(
            "t-lock",
            if self.tool_locked {
                Icon::Lock
            } else {
                Icon::Unlock
            },
            None,
            self.tool_locked,
            style,
            |v, _, _| v.tool_locked = !v.tool_locked,
            cx,
        ));
        bar = bar.child(sep());
        for (id, ic, hint, tool) in tools {
            bar = bar.child(tool_button(
                id,
                ic,
                hint,
                t == tool,
                style,
                move |v, w, cx| v.set_tool(tool, w, cx),
                cx,
            ));
        }
        // Stock "More tools": shows the active extra tool's icon.
        let extra_icon = match t {
            Tool::Frame => Icon::Frame,
            Tool::Laser => Icon::Laser,
            _ => Icon::Extra,
        };
        bar = bar.child(sep()).child(
            div()
                .relative()
                .child(tool_button(
                    "t-extra",
                    extra_icon,
                    None,
                    self.extra_open || matches!(t, Tool::Frame | Tool::Laser),
                    style,
                    |v, _, _| v.extra_open = !v.extra_open,
                    cx,
                ))
                .when(self.extra_open, |d| {
                    d.child(
                        div()
                            .absolute()
                            .top(px(46.0))
                            .right(px(-4.0))
                            .child(self.render_extra_tools(style, cx)),
                    )
                }),
        );
        let chrome = !self.zen && !self.view_mode;

        let menu =
            div()
                .absolute()
                .top(px(16.0))
                .left(px(16.0))
                .child(island(style).p(px(0.0)).child(tool_button(
                    "menu",
                    Icon::Menu,
                    None,
                    self.menu_open,
                    style,
                    |v, _, _| v.menu_open = !v.menu_open,
                    cx,
                )));

        div()
            .absolute()
            .top(px(0.0))
            .left(px(0.0))
            .right(px(0.0))
            .child(menu)
            .when(chrome && !self.library_open, |d| {
                d.child(
                    div()
                        .absolute()
                        .top(px(16.0))
                        .right(px(16.0))
                        .child(library_button(self.library_open, style, cx)),
                )
            })
            .when(chrome, |d| {
                d.child(
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
            })
    }

    fn render_bottom(&self, style: &ExcalidrawStyle, cx: &mut Context<Self>) -> impl IntoElement {
        let zoom = format!("{}%", (self.cam.zoom * 100.0).round());
        let undo_c = if self.can_undo() {
            style.fg
        } else {
            style.muted.opacity(0.5)
        };
        let redo_c = if self.can_redo() {
            style.fg
        } else {
            style.muted.opacity(0.5)
        };
        let plain = |id: &'static str,
                     ic: Icon,
                     c: Hsla,
                     on: fn(&mut ExcalidrawView, &mut Context<ExcalidrawView>),
                     cx: &mut Context<Self>| {
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
        let exit = |id: &'static str, label: &'static str, cx: &mut Context<Self>| {
            div()
                .id(id)
                .debug_selector(move || id.to_string())
                .px(px(12.0))
                .h(px(36.0))
                .flex()
                .items_center()
                .rounded(px(8.0))
                .border_1()
                .border_color(style.island_border)
                .bg(style.island_bg)
                .text_size(px(13.0))
                .text_color(style.fg)
                .hover(|s| s.bg(style.hover_bg))
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    if id == "exit-zen" {
                        this.zen = false;
                    } else {
                        this.set_view_mode(false);
                    }
                    cx.notify();
                }))
                .child(label)
        };
        let help = div().absolute().bottom(px(16.0)).right(px(16.0)).child(
            island(style).p(px(0.0)).child(
                div()
                    .id("help")
                    .debug_selector(|| "help".into())
                    .size(px(36.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(8.0))
                    .hover(|s| s.bg(style.hover_bg))
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                        this.open_dialog(crate::actions::Dialog::Help);
                        cx.notify();
                    }))
                    .child(
                        div()
                            .size(px(16.0))
                            .rounded_full()
                            .border_1()
                            .border_color(style.fg)
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_size(px(10.0))
                            .text_color(style.fg)
                            .child("?"),
                    ),
            ),
        );
        let modes = div()
            .absolute()
            .bottom(px(16.0))
            .left_0()
            .right_0()
            .flex()
            .justify_center()
            .gap(px(8.0))
            .when(self.zen, |d| d.child(exit("exit-zen", "Exit zen mode", cx)))
            .when(self.view_mode, |d| {
                d.child(exit("exit-view", "Exit view mode", cx))
            });
        let left = div()
            .absolute()
            .bottom(px(16.0))
            .left(px(16.0))
            .flex()
            .gap(px(8.0))
            .child(
                island(style)
                    .child(plain(
                        "z-out",
                        Icon::Minus,
                        style.fg,
                        |v, _| v.zoom_step(1.0 / 1.1, None),
                        cx,
                    ))
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
                    .child(plain(
                        "z-in",
                        Icon::Plus,
                        style.fg,
                        |v, _| v.zoom_step(1.1, None),
                        cx,
                    )),
            )
            .when(!self.view_mode && !self.zen, |d| {
                d.child(
                    island(style)
                        .child(plain("undo", Icon::Undo, undo_c, |v, cx| v.undo(cx), cx))
                        .child(plain("redo", Icon::Redo, redo_c, |v, cx| v.redo(cx), cx)),
                )
            });
        div()
            .absolute()
            .inset_0()
            .child(modes)
            .child(left)
            .when(!self.zen, |d| d.child(help))
    }

    fn render_menu(
        &self,
        style: &ExcalidrawStyle,
        pal: Palette,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let bg = self.scene.view_background();
        let item = |id: &'static str,
                    label: &'static str,
                    key: &'static str,
                    on: fn(&mut ExcalidrawView, &mut Window, &mut Context<ExcalidrawView>),
                    cx: &mut Context<Self>| {
            div()
                .id(id)
                .debug_selector(move || id.to_string())
                .w_full()
                .h(px(32.0))
                .px(px(10.0))
                .flex()
                .items_center()
                .justify_between()
                .rounded(px(6.0))
                .text_size(px(13.0))
                .text_color(style.fg)
                .hover(|s| s.bg(style.hover_bg))
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                    this.menu_open = false;
                    on(this, window, cx);
                    cx.notify();
                }))
                .child(label)
                .child(div().text_size(px(11.0)).text_color(style.muted).child(key))
        };
        let link =
            |id: &'static str, label: &'static str, url: &'static str, cx: &mut Context<Self>| {
                div()
                    .id(id)
                    .w_full()
                    .h(px(32.0))
                    .px(px(10.0))
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .rounded(px(6.0))
                    .text_size(px(13.0))
                    .text_color(style.fg)
                    .hover(|s| s.bg(style.hover_bg))
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.menu_open = false;
                        cx.open_url(url);
                    }))
                    .child(icon(Icon::External, 14.0, style.fg))
                    .child(label)
            };
        let sep = || {
            div()
                .my(px(4.0))
                .h(px(1.0))
                .w_full()
                .bg(style.island_border)
        };
        let mut swatches = row();
        for c in CANVAS_BG {
            let active = bg.eq_ignore_ascii_case(c);
            swatches = swatches.child(swatch(
                format!("cbg-{c}").into(),
                c,
                active,
                pal,
                style,
                move |v, _, cx| v.set_background(c, cx),
                cx,
            ));
        }
        swatches = swatches.child(swatch(
            "cbg-picker".into(),
            &bg,
            self.picker == Some(PickerTarget::Canvas),
            pal,
            style,
            |v, _, _| v.toggle_picker(PickerTarget::Canvas),
            cx,
        ));
        use crate::actions::Dialog;
        let view_only = self.view_mode;
        div().absolute().top(px(64.0)).left(px(16.0)).child(
            island(style)
                .id("main-menu")
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .flex_col()
                .items_start()
                .w(px(240.0))
                .p(px(8.0))
                .gap(px(0.0))
                .when(!view_only, |d| {
                    d.child(item(
                        "m-open",
                        "Open",
                        "⌘O",
                        |v, _, cx| v.open_scene_prompt(cx),
                        cx,
                    ))
                })
                .child(item(
                    "m-save",
                    "Save to...",
                    "",
                    |v, _, cx| v.save_scene_prompt(cx),
                    cx,
                ))
                .child(item(
                    "m-export",
                    "Export image...",
                    "⌘⇧E",
                    |v, _, _| v.open_dialog(Dialog::Export),
                    cx,
                ))
                .child(item(
                    "m-find",
                    "Find on canvas",
                    "⌘F",
                    |v, _, _| v.open_search(),
                    cx,
                ))
                .child(item(
                    "m-help",
                    "Help",
                    "?",
                    |v, _, _| v.open_dialog(Dialog::Help),
                    cx,
                ))
                .when(!view_only, |d| {
                    d.child(item(
                        "m-reset",
                        "Reset the canvas",
                        "",
                        |v, _, cx| v.reset_canvas(cx),
                        cx,
                    ))
                })
                .child(sep())
                .child(
                    div()
                        .px(px(10.0))
                        .py(px(4.0))
                        .text_size(px(11.0))
                        .text_color(style.muted)
                        .child("Excalidraw links"),
                )
                .child(link(
                    "m-github",
                    "GitHub",
                    "https://github.com/excalidraw/excalidraw",
                    cx,
                ))
                .child(link(
                    "m-follow",
                    "Follow us",
                    "https://x.com/excalidraw",
                    cx,
                ))
                .child(link(
                    "m-discord",
                    "Discord chat",
                    "https://discord.gg/UexuTaE",
                    cx,
                ))
                .when(!view_only, |d| {
                    d.child(sep()).child(
                        div()
                            .px(px(10.0))
                            .py(px(4.0))
                            .child(section("Canvas background", style).child(swatches)),
                    )
                }),
        )
    }

    fn render_extra_tools(
        &self,
        style: &ExcalidrawStyle,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let t = self.tool;
        let item = |id: &'static str,
                    ic: Icon,
                    label: &'static str,
                    key: &'static str,
                    active: bool,
                    on: fn(&mut ExcalidrawView, &mut Window, &mut Context<ExcalidrawView>),
                    cx: &mut Context<Self>| {
            div()
                .id(id)
                .debug_selector(move || id.to_string())
                .w_full()
                .h(px(32.0))
                .px(px(10.0))
                .flex()
                .items_center()
                .gap(px(10.0))
                .rounded(px(6.0))
                .when(active, |d| d.bg(style.active_bg))
                .hover(|s| s.bg(style.hover_bg))
                .text_size(px(13.0))
                .text_color(style.fg)
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                    this.extra_open = false;
                    on(this, window, cx);
                    cx.notify();
                }))
                .child(icon(ic, 15.0, style.fg))
                .child(div().flex_1().child(label))
                .child(div().text_size(px(11.0)).text_color(style.muted).child(key))
        };
        island(style)
            .id("extra-tools")
            .debug_selector(|| "extra-tools".into())
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .flex_col()
            .items_start()
            .w(px(200.0))
            .p(px(6.0))
            .gap(px(0.0))
            .child(item(
                "x-frame",
                Icon::Frame,
                "Frame tool",
                "F",
                t == Tool::Frame,
                |v, w, cx| v.set_tool(Tool::Frame, w, cx),
                cx,
            ))
            .child(item(
                "x-laser",
                Icon::Laser,
                "Laser pointer",
                "K",
                t == Tool::Laser,
                |v, w, cx| v.set_tool(Tool::Laser, w, cx),
                cx,
            ))
            .child(
                div()
                    .px(px(10.0))
                    .pt(px(6.0))
                    .pb(px(2.0))
                    .text_size(px(12.0))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(style.fg)
                    .child("Generate"),
            )
            .child(item(
                "x-mermaid",
                Icon::Mermaid,
                "Mermaid to Excalidraw",
                "",
                false,
                |v, _, _| v.open_dialog(crate::actions::Dialog::Mermaid),
                cx,
            ))
    }

    fn render_stats(&self, style: &ExcalidrawStyle, cx: &mut Context<Self>) -> impl IntoElement {
        let kv = |k: &'static str, v: String| {
            div()
                .flex()
                .w_full()
                .justify_between()
                .text_size(px(12.0))
                .text_color(style.fg)
                .child(div().text_color(style.muted).child(k))
                .child(v)
        };
        let n = self.scene.elements().count();
        let bb = self.scene.elements().map(geom::aabb).reduce(geom::union);
        let (sw, sh) = bb.map(|b| (b[2] - b[0], b[3] - b[1])).unwrap_or((0.0, 0.0));
        let mut panel = island(style)
            .id("stats")
            .debug_selector(|| "stats".into())
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .flex_col()
            .items_start()
            .w(px(220.0))
            .p(px(12.0))
            .gap(px(6.0))
            .child(
                div()
                    .flex()
                    .w_full()
                    .justify_between()
                    .items_center()
                    .child(
                        div()
                            .text_size(px(14.0))
                            .font_weight(FontWeight::BOLD)
                            .text_color(style.fg)
                            .child("Stats"),
                    )
                    .child(text_button(
                        "stats-close".into(),
                        "×",
                        false,
                        style,
                        |v, _, _| v.stats_open = false,
                        cx,
                    )),
            )
            .child(section("General", style))
            .child(kv("Shapes", n.to_string()))
            .child(kv("Width", format!("{}", sw.round())))
            .child(kv("Height", format!("{}", sh.round())));
        let list = self.selection_list();
        if list.len() == 1 {
            let e = list[0];
            panel = panel
                .child(section("Element properties", style))
                .child(kv("Type", e.kind.clone()))
                .child(kv("X", format!("{}", e.x().round())))
                .child(kv("Y", format!("{}", e.y().round())))
                .child(kv("W", format!("{}", e.width().round())))
                .child(kv("H", format!("{}", e.height().round())))
                .child(kv("A", format!("{}°", e.angle().to_degrees().round())));
            if e.kind == "text" {
                panel = panel.child(kv("Font size", format!("{}", e.font_size().round())));
            }
        } else if list.len() > 1 {
            let b = list
                .iter()
                .map(|e| geom::aabb(e))
                .reduce(geom::union)
                .unwrap();
            panel = panel
                .child(section("Element properties", style))
                .child(kv("Shapes", list.len().to_string()))
                .child(kv("X", format!("{}", b[0].round())))
                .child(kv("Y", format!("{}", b[1].round())))
                .child(kv("W", format!("{}", (b[2] - b[0]).round())))
                .child(kv("H", format!("{}", (b[3] - b[1]).round())));
        }
        let top = if self.zen || self.view_mode {
            16.0
        } else {
            64.0
        };
        div().absolute().top(px(top)).right(px(16.0)).child(panel)
    }

    fn render_properties(
        &self,
        style: &ExcalidrawStyle,
        pal: Palette,
        max_h: Pixels,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let list = self.selection_list();
        let d = &self.defaults;
        let first = list.first().copied();
        // What's being styled: the selection, or the active tool's kind.
        let kinds: HashSet<String> = if list.is_empty() {
            self.tool
                .element_kind()
                .map(|k| k.to_string())
                .into_iter()
                .collect()
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
        let stroke = first
            .map(|e| e.stroke_color().to_string())
            .unwrap_or(d.stroke_color.clone());
        let bgc = first
            .map(|e| e.background_color().to_string())
            .unwrap_or(d.background_color.clone());
        let fill_s = first
            .map(|e| e.fill_style().to_string())
            .unwrap_or(d.fill_style.clone());
        let sw = first.map(|e| e.stroke_width()).unwrap_or(d.stroke_width);
        let ss = first
            .map(|e| e.stroke_style().to_string())
            .unwrap_or(d.stroke_style.clone());
        let rough = first.map(|e| e.roughness()).unwrap_or(d.roughness);
        let op = first.map(|e| e.opacity()).unwrap_or(d.opacity);
        let round = first
            .map(|e| e.f.roundness.get().is_some())
            .unwrap_or(d.round);
        let text_el = list.iter().find(|e| e.kind == "text").copied().or_else(|| {
            list.iter().find_map(|e| {
                e.bound_elements()
                    .iter()
                    .find(|b| b.kind == "text")
                    .and_then(|b| self.scene.get(&b.id))
            })
        });
        let fs = text_el.map(|e| e.font_size()).unwrap_or(d.font_size);
        let ff = text_el.map(|e| e.font_family()).unwrap_or(d.font_family);
        let ta = text_el
            .map(|e| e.text_align().to_string())
            .unwrap_or(d.text_align.clone());
        let arrow = list.iter().find(|e| e.kind == "arrow").copied();
        let sa = arrow
            .map(|e| e.f.start_arrowhead.get().cloned())
            .unwrap_or(d.start_arrowhead.clone());
        let ea = arrow
            .map(|e| e.f.end_arrowhead.get().cloned())
            .unwrap_or(d.end_arrowhead.clone());

        let mut panel = island(style)
            .id("props")
            .flex_col()
            .items_start()
            .w(px(204.0))
            .p(px(12.0))
            .gap(px(10.0))
            .max_h(max_h)
            .overflow_y_scroll();

        let shape_like = has(&[
            "rectangle",
            "diamond",
            "ellipse",
            "arrow",
            "line",
            "freedraw",
            "text",
            "frame",
        ]) || kinds.is_empty();
        if shape_like {
            let mut r = row();
            for c in STROKES {
                let active = stroke.eq_ignore_ascii_case(c);
                r = r.child(swatch(
                    format!("s-{c}").into(),
                    c,
                    active,
                    pal,
                    style,
                    move |v, _, cx| {
                        v.apply(
                            |e, d| {
                                e.f.stroke_color.set(c.into());
                                d.stroke_color = c.into();
                            },
                            cx,
                        )
                    },
                    cx,
                ));
            }
            r = r.child(div().w(px(1.0)).h(px(22.0)).bg(style.island_border));
            r = r.child(swatch(
                "s-picker".into(),
                &stroke,
                self.picker == Some(PickerTarget::Stroke),
                pal,
                style,
                |v, _, _| v.toggle_picker(PickerTarget::Stroke),
                cx,
            ));
            panel = panel.child(section("Stroke", style).child(r));
        }
        if has(&["rectangle", "diamond", "ellipse", "line", "freedraw"]) {
            let mut r = row();
            for c in BACKGROUNDS {
                let active = bgc.eq_ignore_ascii_case(c);
                r = r.child(swatch(
                    format!("b-{c}").into(),
                    c,
                    active,
                    pal,
                    style,
                    move |v, _, cx| {
                        v.apply(
                            |e, d| {
                                e.f.background_color.set(c.into());
                                d.background_color = c.into();
                            },
                            cx,
                        )
                    },
                    cx,
                ));
            }
            r = r.child(div().w(px(1.0)).h(px(22.0)).bg(style.island_border));
            r = r.child(swatch(
                "b-picker".into(),
                &bgc,
                self.picker == Some(PickerTarget::Background),
                pal,
                style,
                |v, _, _| v.toggle_picker(PickerTarget::Background),
                cx,
            ));
            panel = panel.child(section("Background", style).child(r));
            if !color::is_transparent(&bgc) {
                let mut r = row();
                for (k, label) in [
                    ("hachure", Icon::FillHachure),
                    ("cross-hatch", Icon::FillCross),
                    ("solid", Icon::FillSolid),
                ] {
                    r = r.child(prop_button(
                        format!("fill-{k}").into(),
                        label,
                        fill_s == k,
                        style,
                        move |v, _, cx| {
                            v.apply(
                                |e, d| {
                                    e.f.fill_style.set(k.into());
                                    d.fill_style = k.into();
                                },
                                cx,
                            )
                        },
                        cx,
                    ));
                }
                panel = panel.child(section("Fill", style).child(r));
            }
        }
        if has(&[
            "rectangle",
            "diamond",
            "ellipse",
            "arrow",
            "line",
            "freedraw",
        ]) {
            let mut r = row();
            for (w, label) in [
                (1.0, Icon::WidthThin),
                (2.0, Icon::WidthBold),
                (4.0, Icon::WidthExtra),
            ] {
                r = r.child(prop_button(
                    format!("sw-{w}").into(),
                    label,
                    (sw - w).abs() < 0.01,
                    style,
                    move |v, _, cx| {
                        v.apply(
                            |e, d| {
                                e.f.stroke_width.set(Num(w));
                                d.stroke_width = w;
                            },
                            cx,
                        )
                    },
                    cx,
                ));
            }
            panel = panel.child(section("Stroke width", style).child(r));
        }
        if has(&["rectangle", "diamond", "ellipse", "arrow", "line"]) {
            let mut r = row();
            for (k, label) in [
                ("solid", Icon::StyleSolid),
                ("dashed", Icon::StyleDashed),
                ("dotted", Icon::StyleDotted),
            ] {
                r = r.child(prop_button(
                    format!("ss-{k}").into(),
                    label,
                    ss == k,
                    style,
                    move |v, _, cx| {
                        v.apply(
                            |e, d| {
                                e.f.stroke_style.set(k.into());
                                d.stroke_style = k.into();
                            },
                            cx,
                        )
                    },
                    cx,
                ));
            }
            panel = panel.child(section("Stroke style", style).child(r));
            let mut r = row();
            for (k, label) in [
                (0.0, Icon::RoughArchitect),
                (1.0, Icon::RoughArtist),
                (2.0, Icon::RoughCartoonist),
            ] {
                r = r.child(prop_button(
                    format!("rg-{k}").into(),
                    label,
                    (rough - k).abs() < 0.01,
                    style,
                    move |v, _, cx| {
                        v.apply(
                            |e, d| {
                                e.f.roughness.set(Num(k));
                                d.roughness = k;
                            },
                            cx,
                        )
                    },
                    cx,
                ));
            }
            panel = panel.child(section("Sloppiness", style).child(r));
        }
        if has(&["arrow"]) {
            let cur = first
                .filter(|e| e.kind == "arrow")
                .map(|e| {
                    if e.f.elbowed.get().copied().unwrap_or(false) {
                        "elbow"
                    } else if e.f.roundness.get().is_some() {
                        "round"
                    } else {
                        "sharp"
                    }
                })
                .unwrap_or(d.arrow_type.as_str())
                .to_string();
            let mut r = row();
            for (k, label) in [
                ("sharp", Icon::ArrowSharp),
                ("round", Icon::ArrowCurved),
                ("elbow", Icon::ArrowElbow),
            ] {
                r = r.child(prop_button(
                    format!("at-{k}").into(),
                    label,
                    cur == k,
                    style,
                    move |v, _, cx| v.set_arrow_type(k, cx),
                    cx,
                ));
            }
            panel = panel.child(section("Arrow type", style).child(r));
        }
        if has(&["rectangle", "diamond", "line"]) {
            let mut r = row();
            for (k, label) in [(false, Icon::EdgeSharp), (true, Icon::EdgeRound)] {
                r = r.child(prop_button(
                    format!("edge-{k}").into(),
                    label,
                    round == k,
                    style,
                    move |v, _, cx| {
                        v.apply(
                            |e, d| {
                                d.round = k;
                                if e.kind.is_empty() {
                                    return;
                                }
                                if k {
                                    let kind = if matches!(
                                        e.kind.as_str(),
                                        "line" | "arrow" | "diamond"
                                    ) {
                                        2.0
                                    } else {
                                        3.0
                                    };
                                    e.f.roundness.set(Roundness {
                                        kind: Num(kind),
                                        value: Default::default(),
                                        extra: Default::default(),
                                    });
                                } else {
                                    e.f.roundness.set_null();
                                }
                            },
                            cx,
                        )
                    },
                    cx,
                ));
            }
            panel = panel.child(section("Edges", style).child(r));
        }
        if has(&["arrow"]) {
            let heads: [(Option<&'static str>, &'static str); 12] = [
                (None, "None"),
                (Some("arrow"), "Arrow"),
                (Some("bar"), "Bar"),
                (Some("circle"), "●"),
                (Some("circle_outline"), "○"),
                (Some("triangle"), "▲"),
                (Some("triangle_outline"), "△"),
                (Some("diamond"), "◆"),
                (Some("diamond_outline"), "◇"),
                (Some("crowfoot_one"), "Cf1"),
                (Some("crowfoot_many"), "CfN"),
                (Some("crowfoot_one_or_many"), "Cf1N"),
            ];
            let glyph =
                |k: Option<&str>| heads.iter().find(|h| h.0 == k).map(|h| h.1).unwrap_or("?");
            let mut buttons = row();
            let mut popover = None;
            for start in [true, false] {
                let cur = if start { sa.clone() } else { ea.clone() };
                let id = if start { "ah-start" } else { "ah-end" };
                let label = match glyph(cur.as_deref()) {
                    "None" => {
                        if start {
                            "×—"
                        } else {
                            "—×"
                        }
                    }
                    "Arrow" => {
                        if start {
                            "←"
                        } else {
                            "→"
                        }
                    }
                    g => g,
                };
                buttons = buttons.child(
                    text_button(
                        id.into(),
                        label,
                        self.ah_open == Some(start),
                        style,
                        move |v, _, _| {
                            v.ah_open = if v.ah_open == Some(start) {
                                None
                            } else {
                                Some(start)
                            }
                        },
                        cx,
                    )
                    .size(px(32.0))
                    .rounded(px(8.0)),
                );
            }
            if let Some(start) = self.ah_open {
                let cur = if start { sa.clone() } else { ea.clone() };
                let mut r = row();
                for (k, label) in heads {
                    let active = cur.as_deref() == k;
                    r = r.child(text_button(
                        format!("ah-{start}-{label}").into(),
                        label,
                        active,
                        style,
                        move |v, _, cx| {
                            v.ah_open = None;
                            v.apply(
                                |e, d| {
                                    let f = if start {
                                        &mut e.f.start_arrowhead
                                    } else {
                                        &mut e.f.end_arrowhead
                                    };
                                    if e.kind == "arrow" {
                                        match k {
                                            Some(k) => f.set(k.into()),
                                            None => f.set_null(),
                                        }
                                    }
                                    if start {
                                        d.start_arrowhead = k.map(String::from)
                                    } else {
                                        d.end_arrowhead = k.map(String::from)
                                    }
                                },
                                cx,
                            )
                        },
                        cx,
                    ));
                }
                popover = Some(
                    island(style)
                        .id("ah-popover")
                        .debug_selector(|| "ah-popover".into())
                        .w(px(180.0))
                        .p(px(6.0))
                        .child(r.w_full()),
                );
            }
            panel = panel.child(
                section("Arrowheads", style)
                    .child(buttons)
                    .children(popover),
            );
        }
        if has(&["text"]) {
            let mut r = row();
            for (s, label) in [(16.0, "S"), (20.0, "M"), (28.0, "L"), (36.0, "XL")] {
                r = r.child(text_button(
                    format!("fs-{s}").into(),
                    label,
                    (fs - s).abs() < 0.01,
                    style,
                    move |v, _, cx| {
                        v.apply(
                            |e, d| {
                                if e.kind == "text" {
                                    e.f.font_size.set(Num(s));
                                }
                                d.font_size = s;
                            },
                            cx,
                        )
                    },
                    cx,
                ));
            }
            panel = panel.child(section("Font size", style).child(r));
            let mut r = row();
            for (f, label) in [
                (5u32, "Hand"),
                (6, "Normal"),
                (8, "Code"),
                (7, "Lilita"),
                (1, "Virgil"),
                (2, "Helvetica"),
                (3, "Cascadia"),
                (9, "Liberation"),
            ] {
                r = r.child(text_button(
                    format!("ff-{f}").into(),
                    label,
                    ff == f,
                    style,
                    move |v, _, cx| {
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
                    },
                    cx,
                ));
            }
            panel = panel.child(section("Font family", style).child(r));
            let mut r = row();
            for (k, label) in [("left", "Left"), ("center", "Center"), ("right", "Right")] {
                r = r.child(text_button(
                    format!("ta-{k}").into(),
                    label,
                    ta == k,
                    style,
                    move |v, _, cx| {
                        v.apply(
                            |e, d| {
                                if e.kind == "text" {
                                    e.f.text_align.set(k.into());
                                }
                                d.text_align = k.into();
                            },
                            cx,
                        )
                    },
                    cx,
                ));
            }
            panel = panel.child(section("Text align", style).child(r));
            let bound_text = self.scene.elements().find(|t| {
                t.kind == "text" && t.container_id().is_some_and(|c| self.selected.contains(c))
            });
            if let Some(t) = bound_text {
                let va = t.vertical_align().to_string();
                let mut r = row();
                for (k, label) in [("top", "Top"), ("middle", "Middle"), ("bottom", "Bottom")] {
                    r = r.child(text_button(
                        format!("va-{k}").into(),
                        label,
                        va == k,
                        style,
                        move |v, _, cx| {
                            v.apply(
                                |e, _| {
                                    if e.kind == "text" && e.container_id().is_some() {
                                        e.f.vertical_align.set(k.into());
                                    }
                                },
                                cx,
                            );
                        },
                        cx,
                    ));
                }
                panel = panel.child(section("Vertical align", style).child(r));
            }
        }
        // Opacity: stock slider with 0 / 100 under the ends.
        {
            let r = div()
                .flex()
                .flex_col()
                .gap(px(2.0))
                .child(opacity_slider(op, self.opacity_track.clone(), style, cx))
                .child(
                    div()
                        .w(px(SLIDER_W))
                        .flex()
                        .justify_between()
                        .text_size(px(10.0))
                        .text_color(style.fg)
                        .child("0")
                        .child("100"),
                );
            panel = panel.child(section("Opacity", style).child(r));
        }
        if !list.is_empty() {
            let mut r = row();
            for (k, label, fwd, end) in [
                ("back", "⇊", false, true),
                ("bwd", "↓", false, false),
                ("fwd", "↑", true, false),
                ("front", "⇈", true, true),
            ] {
                r = r.child(text_button(
                    format!("z-{k}").into(),
                    label,
                    false,
                    style,
                    move |v, _, cx| {
                        let s = v.selected.clone();
                        v.scene.reorder(&s, fwd, end);
                        v.commit(cx);
                    },
                    cx,
                ));
            }
            panel = panel.child(section("Layers", style).child(r));
            if list.len() >= 2 {
                use crate::actions::Align;
                let mut r = row();
                for (k, label, a) in [
                    ("left", "⇤", Align::Left),
                    ("cx", "↔", Align::CenterX),
                    ("right", "⇥", Align::Right),
                    ("top", "⤒", Align::Top),
                    ("cy", "↕", Align::CenterY),
                    ("bottom", "⤓", Align::Bottom),
                ] {
                    r = r.child(text_button(
                        format!("al-{k}").into(),
                        label,
                        false,
                        style,
                        move |v, _, cx| v.align(a, cx),
                        cx,
                    ));
                }
                r = r.child(text_button(
                    "dist-h".into(),
                    "⇹",
                    false,
                    style,
                    |v, _, cx| v.distribute(true, cx),
                    cx,
                ));
                r = r.child(text_button(
                    "dist-v".into(),
                    "⇳",
                    false,
                    style,
                    |v, _, cx| v.distribute(false, cx),
                    cx,
                ));
                panel = panel.child(section("Align", style).child(r));
            }
            let grouped = list.iter().any(|e| !e.group_ids().is_empty());
            let r = row()
                .child(prop_button(
                    "a-dup".into(),
                    Icon::Copy,
                    false,
                    style,
                    |v, _, cx| {
                        v.duplicate_selection(10.0, cx);
                        v.commit(cx);
                    },
                    cx,
                ))
                .child(prop_button(
                    "a-del".into(),
                    Icon::Trash,
                    false,
                    style,
                    |v, _, cx| v.delete_selection(cx),
                    cx,
                ))
                .when(list.len() > 1, |r| {
                    r.child(prop_button(
                        "a-grp".into(),
                        Icon::Group,
                        false,
                        style,
                        |v, _, cx| {
                            let s = v.selected.clone();
                            v.scene.group(&s);
                            v.commit(cx);
                        },
                        cx,
                    ))
                })
                .when(grouped, |r| {
                    r.child(prop_button(
                        "a-ungrp".into(),
                        Icon::Ungroup,
                        false,
                        style,
                        |v, _, cx| {
                            let s = v.selected.clone();
                            v.scene.ungroup(&s);
                            v.commit(cx);
                        },
                        cx,
                    ))
                })
                .when(list.len() == 1 && list[0].container_id().is_none(), |r| {
                    r.child(prop_button(
                        "a-link".into(),
                        Icon::Link,
                        false,
                        style,
                        |v, _, _| v.start_link_edit(),
                        cx,
                    ))
                })
                .child(prop_button(
                    "a-lock".into(),
                    Icon::Lock,
                    false,
                    style,
                    |v, _, cx| {
                        let s = v.selected.clone();
                        for id in &s {
                            if let Some(e) = v.scene.get_mut(id) {
                                e.f.locked.set(true);
                            }
                        }
                        v.selected.clear();
                        v.commit(cx);
                    },
                    cx,
                ));
            panel = panel.child(section("Actions", style).child(r));
        }
        div().absolute().top(px(72.0)).left(px(16.0)).child(panel)
    }
}

/// The URL a hyperlink opens: scheme-less links are treated as https.
pub(crate) fn link_url(link: &str) -> String {
    let l = link.trim();
    if l.contains("://") || l.starts_with("mailto:") || l.starts_with("tel:") {
        l.to_string()
    } else {
        format!("https://{l}")
    }
}

impl ExcalidrawView {
    /// Screen-space (view-local) bbox of an element.
    fn screen_bbox(&self, el: &crate::model::Element) -> [f32; 4] {
        let bb = geom::aabb(el);
        let c = self.cam;
        let f = |x: f64, s: f64| ((x + s) * c.zoom) as f32;
        [
            f(bb[0], c.scroll_x),
            f(bb[1], c.scroll_y),
            f(bb[2], c.scroll_x),
            f(bb[3], c.scroll_y),
        ]
    }

    /// Stock's hyperlink handle: a small chain button above the top-right
    /// corner of every element that has a link; clicking it opens the URL.
    fn render_link_icons(
        &self,
        style: &ExcalidrawStyle,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let editing = self.editing.as_ref().map(|e| e.id.as_str());
        self.scene
            .elements()
            .filter(|e| !e.link().is_empty() && Some(e.id.as_str()) != editing)
            .map(|el| {
                let r = self.screen_bbox(el);
                let id = format!("link-icon-{}", el.id);
                let sel = id.clone();
                let url = link_url(el.link());
                div()
                    .id(SharedString::from(id))
                    .debug_selector(move || sel)
                    .absolute()
                    .left(px(r[2] - 4.0))
                    .top(px(r[1] - 26.0))
                    .size(px(22.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(4.0))
                    .bg(style.island_bg)
                    .border_1()
                    .border_color(style.island_border)
                    .cursor_pointer()
                    .hover(|s| s.bg(style.hover_bg))
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(cx.listener(move |_, _: &ClickEvent, _, cx| cx.open_url(&url)))
                    .child(icon(Icon::Link, 14.0, style.selection))
                    .into_any_element()
            })
            .collect()
    }

    /// The hyperlink editor (when open) or the link preview for a single
    /// selected element that has a link.
    fn render_link_popover(
        &self,
        style: &ExcalidrawStyle,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if self.view_mode || self.dialog.is_some() {
            return None;
        }
        let (id, editing_text) = match &self.link_edit {
            Some((id, t)) => (id.clone(), Some(t.clone())),
            None => {
                if self.selected.len() != 1 || self.drag.is_some() || self.editing.is_some() {
                    return None;
                }
                let id = self.selected.iter().next()?.clone();
                if self.scene.get(&id)?.link().is_empty() {
                    return None;
                }
                (id, None)
            }
        };
        let el = self.scene.get(&id)?;
        let r = self.screen_bbox(el);
        let link = el.link().to_string();
        let w = 320.0;
        let b = self.bounds.get();
        let x = r[0].min(f32::from(b.size.width) - w - 8.0).max(8.0);
        let y = (r[1] - 80.0).max(8.0);
        let rid = id.clone();
        let remove = prop_button(
            "link-remove".into(),
            Icon::Trash,
            false,
            style,
            move |v, _, cx| {
                v.link_edit = None;
                v.set_link(&rid, "", cx);
            },
            cx,
        );
        let mut row = island(style)
            .id("link-popover")
            .debug_selector(|| "link-popover".into())
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .w(px(w))
            .p(px(6.0))
            .gap(px(4.0));
        row = match editing_text {
            Some(t) => {
                let empty = t.is_empty();
                row.child(
                    div()
                        .id("link-input")
                        .debug_selector(|| "link-input".into())
                        .flex_1()
                        .min_w_0()
                        .h(px(32.0))
                        .px(px(8.0))
                        .flex()
                        .items_center()
                        .overflow_hidden()
                        .rounded(px(6.0))
                        .border_1()
                        .border_color(style.selection)
                        .text_size(px(13.0))
                        .text_color(if empty { style.muted } else { style.fg })
                        .child(if empty {
                            "Type or paste your link here".to_string()
                        } else {
                            format!("{t}|")
                        }),
                )
            }
            None => {
                let url = link_url(&link);
                row.child(
                    div()
                        .id("link-open")
                        .debug_selector(|| "link-open".into())
                        .flex_1()
                        .min_w_0()
                        .px(px(8.0))
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis()
                        .text_size(px(13.0))
                        .text_color(style.selection)
                        .cursor_pointer()
                        .hover(|s| s.underline())
                        .on_click(cx.listener(move |_, _: &ClickEvent, _, cx| cx.open_url(&url)))
                        .child(link),
                )
                .child(prop_button(
                    "link-edit".into(),
                    Icon::Edit,
                    false,
                    style,
                    |v, _, _| v.start_link_edit(),
                    cx,
                ))
            }
        };
        Some(
            div()
                .absolute()
                .left(px(x))
                .top(px(y))
                .child(row.child(remove))
                .into_any_element(),
        )
    }
}

fn render_toast(msg: String, style: &ExcalidrawStyle) -> impl IntoElement {
    div()
        .absolute()
        .bottom(px(64.0))
        .left_0()
        .right_0()
        .flex()
        .justify_center()
        .child(
            island(style)
                .id("toast")
                .debug_selector(|| "toast".into())
                .px(px(12.0))
                .py(px(6.0))
                .text_size(px(13.0))
                .text_color(style.fg)
                .child(msg),
        )
}

/// Excalidraw's keyboard shortcuts in its help dialog's order and grouping,
/// limited to what this editor implements. Each entry: label and the
/// alternative key sequences ("or" between them).
type Keys = &'static [&'static [&'static str]];
const HELP: &[(&str, &[(&str, Keys)])] = &[
    (
        "Tools",
        &[
            ("Hand (panning tool)", &[&["H"]]),
            ("Selection", &[&["V"], &["1"]]),
            ("Rectangle", &[&["R"], &["2"]]),
            ("Diamond", &[&["D"], &["3"]]),
            ("Ellipse", &[&["O"], &["4"]]),
            ("Arrow", &[&["A"], &["5"]]),
            ("Line", &[&["L"], &["6"]]),
            ("Draw", &[&["P"], &["7"]]),
            ("Text", &[&["T"], &["8"]]),
            ("Insert image", &[&["9"]]),
            ("Eraser", &[&["E"], &["0"]]),
            ("Frame tool", &[&["F"]]),
            ("Laser pointer", &[&["K"]]),
            (
                "Pick color from canvas",
                &[&["I"], &["Shift", "S"], &["Shift", "G"]],
            ),
            ("Edit text / add label", &[&["Enter"]]),
            (
                "Add new line (text editor)",
                &[&["Enter"], &["Shift", "Enter"]],
            ),
            (
                "Finish editing (text editor)",
                &[&["Esc"], &["Cmd", "Enter"]],
            ),
            ("Curved arrow", &[&["A", "click", "click", "click"]]),
            ("Curved line", &[&["L", "click", "click", "click"]]),
            ("Keep selected tool active after drawing", &[&["Q"]]),
        ],
    ),
    (
        "View",
        &[
            ("Zoom in", &[&["Cmd", "+"]]),
            ("Zoom out", &[&["Cmd", "-"]]),
            ("Reset zoom", &[&["Cmd", "0"]]),
            ("Zoom to fit all elements", &[&["Shift", "1"]]),
            ("Zoom to selection", &[&["Shift", "2"]]),
            ("Move page up/down", &[&["PgUp/PgDn"]]),
            ("Move page left/right", &[&["Shift", "PgUp/PgDn"]]),
            ("Zen mode", &[&["Option", "Z"]]),
            ("Snap to objects", &[&["Option", "S"]]),
            ("Toggle grid", &[&["Cmd", "'"]]),
            ("View mode", &[&["Option", "R"]]),
            ("Canvas & Shape properties", &[&["Option", "/"]]),
            ("Find on canvas", &[&["Cmd", "F"]]),
        ],
    ),
    (
        "Editor",
        &[
            ("Move canvas", &[&["Space", "drag"], &["Wheel", "drag"]]),
            ("Reset the canvas", &[&["Cmd", "Delete"]]),
            ("Delete", &[&["Delete"]]),
            ("Cut", &[&["Cmd", "X"]]),
            ("Copy", &[&["Cmd", "C"]]),
            ("Paste", &[&["Cmd", "V"]]),
            ("Select all", &[&["Cmd", "A"]]),
            ("Add element to selection", &[&["Shift", "click"]]),
            ("Copy to clipboard as PNG", &[&["Shift", "Option", "C"]]),
            ("Copy styles", &[&["Cmd", "Option", "C"]]),
            ("Paste styles", &[&["Cmd", "Option", "V"]]),
            ("Send to back", &[&["Cmd", "Option", "["]]),
            ("Bring to front", &[&["Cmd", "Option", "]"]]),
            ("Send backward", &[&["Cmd", "["]]),
            ("Bring forward", &[&["Cmd", "]"]]),
            ("Align top", &[&["Cmd", "Shift", "Up"]]),
            ("Align bottom", &[&["Cmd", "Shift", "Down"]]),
            ("Align left", &[&["Cmd", "Shift", "Left"]]),
            ("Align right", &[&["Cmd", "Shift", "Right"]]),
            ("Duplicate", &[&["Cmd", "D"], &["Option", "drag"]]),
            ("Lock/unlock selection", &[&["Cmd", "Shift", "L"]]),
            ("Undo", &[&["Cmd", "Z"]]),
            ("Redo", &[&["Cmd", "Shift", "Z"]]),
            ("Group selection", &[&["Cmd", "G"]]),
            ("Ungroup selection", &[&["Cmd", "Shift", "G"]]),
            ("Flip horizontal", &[&["Shift", "H"]]),
            ("Flip vertical", &[&["Shift", "V"]]),
            ("Show stroke color picker", &[&["S"]]),
            ("Show background color picker", &[&["G"]]),
            ("Decrease font size", &[&["Cmd", "Shift", "<"]]),
            ("Increase font size", &[&["Cmd", "Shift", ">"]]),
        ],
    ),
];

/// Links in the help dialog's header (stock order).
const HELP_LINKS: [(&str, &str); 4] = [
    ("Documentation", "https://docs.excalidraw.com"),
    ("Read our blog", "https://plus.excalidraw.com/blog"),
    (
        "Found an issue? Submit",
        "https://github.com/excalidraw/excalidraw/issues",
    ),
    ("YouTube", "https://youtube.com/@excalidraw"),
];

/// Default definition the stock Mermaid dialog starts with.
const MERMAID_SAMPLE: &str = "flowchart TD\n A[Christmas] -->|Get money| B(Go shopping)\n B --> C{Let me think}\n C -->|One| D[Laptop]\n C -->|Two| E[iPhone]\n C -->|Three| F[Car]";

fn library_button(
    open: bool,
    style: &ExcalidrawStyle,
    cx: &mut Context<ExcalidrawView>,
) -> impl IntoElement {
    div()
        .id("lib-toggle")
        .debug_selector(|| "lib-toggle".into())
        .occlude()
        .h(px(36.0))
        .px(px(12.0))
        .flex()
        .items_center()
        .gap(px(8.0))
        .rounded(px(8.0))
        .bg(if open {
            style.active_bg
        } else {
            style.hover_bg
        })
        .hover(|s| s.bg(style.active_bg))
        .text_size(px(13.0))
        .text_color(style.fg)
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(cx.listener(|v, _: &ClickEvent, _, cx| {
            v.library_open = !v.library_open;
            cx.notify();
        }))
        .child(icon(Icon::Book, 16.0, style.fg))
        .child("Library")
}

/// A key chip in the help dialog.
fn key_chip(k: &'static str, style: &ExcalidrawStyle) -> Div {
    div()
        .px(px(6.0))
        .h(px(22.0))
        .flex()
        .items_center()
        .rounded(px(4.0))
        .bg(style.active_bg)
        .text_size(px(10.0))
        .text_color(style.fg)
        .child(k)
}

/// Paints a library item's elements fitted into `b`.
fn paint_thumbnail(
    els: &[Element],
    b: Bounds<Pixels>,
    caches: &std::rc::Rc<std::cell::RefCell<render::Caches>>,
    pal: Palette,
    window: &mut Window,
) {
    let Some(bb) = els.iter().map(geom::aabb).reduce(geom::union) else {
        return;
    };
    let pad = 8.0;
    let (w, h) = (
        f64::from(f32::from(b.size.width)) - 2.0 * pad,
        f64::from(f32::from(b.size.height)) - 2.0 * pad,
    );
    let (bw, bh) = ((bb[2] - bb[0]).max(1.0), (bb[3] - bb[1]).max(1.0));
    let zoom = (w / bw).min(h / bh).min(1.0);
    let cam = Camera {
        scroll_x: (w / 2.0 + pad) / zoom - (bb[0] + bb[2]) / 2.0,
        scroll_y: (h / 2.0 + pad) / zoom - (bb[1] + bb[3]) / 2.0,
        zoom,
    };
    let mut scene = crate::scene::Scene::default();
    for e in els {
        scene.add(e.clone());
    }
    let mut caches = caches.borrow_mut();
    window.with_content_mask(Some(ContentMask { bounds: b }), |window| {
        for el in scene.elements() {
            render::paint_element(el, &scene, &mut caches, cam, b.origin, pal, 1.0, window);
        }
    });
}

/// A library item being dragged onto the canvas.
#[derive(Clone)]
pub(crate) struct LibraryDrag {
    pub id: String,
    pub name: SharedString,
}

impl Render for LibraryDrag {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .px(px(8.0))
            .py(px(4.0))
            .rounded(px(6.0))
            .bg(gpui_kit::white().opacity(0.9))
            .border_1()
            .border_color(gpui_kit::black().opacity(0.2))
            .text_size(px(12.0))
            .text_color(gpui_kit::black())
            .child(self.name.clone())
    }
}

impl ExcalidrawView {
    fn render_library(&self, style: &ExcalidrawStyle, cx: &mut Context<Self>) -> impl IntoElement {
        let pal = self.palette(cx);
        let tab =
            |id: &'static str, ic: Icon, active: bool, search: bool, cx: &mut Context<Self>| {
                div()
                    .id(id)
                    .debug_selector(move || id.to_string())
                    .size(px(36.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(8.0))
                    .when(active, |d| d.bg(style.selection))
                    .when(!active, |d| d.hover(|s| s.bg(style.hover_bg)))
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(cx.listener(move |v, _: &ClickEvent, _, cx| {
                        v.sidebar_search = search;
                        v.search_focus = search;
                        v.lib_menu = false;
                        cx.notify();
                    }))
                    .child(icon(
                        ic,
                        16.0,
                        if active { style.island_bg } else { style.fg },
                    ))
            };
        let header = div()
            .flex()
            .items_center()
            .justify_between()
            .pb(px(10.0))
            .border_b_1()
            .border_color(style.island_border)
            .child(
                div()
                    .flex()
                    .p(px(2.0))
                    .rounded(px(10.0))
                    .border_1()
                    .border_color(style.island_border)
                    .child(tab(
                        "sb-search",
                        Icon::Search,
                        self.sidebar_search,
                        true,
                        cx,
                    ))
                    .child(tab(
                        "sb-library",
                        Icon::Book,
                        !self.sidebar_search,
                        false,
                        cx,
                    )),
            )
            .child(text_button(
                "lib-close".into(),
                "×",
                false,
                style,
                |v, _, _| {
                    v.library_open = false;
                    v.search_focus = false;
                    v.lib_menu = false;
                },
                cx,
            ));
        let body = if self.sidebar_search {
            self.render_search(style, cx).into_any_element()
        } else {
            self.render_library_tab(style, pal, cx).into_any_element()
        };
        div()
            .id("library")
            .debug_selector(|| "library".into())
            .occlude()
            .absolute()
            .top_0()
            .right_0()
            .bottom_0()
            .w(px(302.0))
            .flex()
            .flex_col()
            .p(px(12.0))
            .gap(px(12.0))
            .bg(style.island_bg)
            .border_l_1()
            .border_color(style.island_border)
            .shadow_sm()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(header)
            .child(body)
    }

    fn render_search(&self, style: &ExcalidrawStyle, cx: &mut Context<Self>) -> impl IntoElement {
        let matches = self.search_matches();
        let q = self.search_query.clone();
        let field_text: SharedString = if q.is_empty() && !self.search_focus {
            "Find on canvas".into()
        } else if self.search_focus {
            format!("{q}▏").into()
        } else {
            q.clone().into()
        };
        let field = div()
            .id("search-input")
            .debug_selector(|| "search-input".into())
            .h(px(36.0))
            .px(px(10.0))
            .flex()
            .items_center()
            .gap(px(8.0))
            .rounded(px(8.0))
            .border_1()
            .border_color(if self.search_focus {
                style.selection
            } else {
                style.island_border
            })
            .text_size(px(13.0))
            .text_color(if q.is_empty() && !self.search_focus {
                style.muted
            } else {
                style.fg
            })
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(|v, _: &ClickEvent, _, cx| {
                v.search_focus = true;
                cx.notify();
            }))
            .child(icon(Icon::Search, 14.0, style.muted))
            .child(field_text);
        let mut col = div().flex().flex_col().gap(px(6.0)).child(field);
        if !q.trim().is_empty() {
            let count = match matches.len() {
                0 => "No matches".to_string(),
                1 => "1 result".to_string(),
                n => format!("{n} results"),
            };
            col = col.child(
                div()
                    .id("search-count")
                    .debug_selector(|| "search-count".into())
                    .text_size(px(12.0))
                    .text_color(style.muted)
                    .child(count),
            );
            let ql = q.trim().to_lowercase();
            let mut list = div()
                .id("search-results")
                .flex()
                .flex_col()
                .gap(px(2.0))
                .overflow_y_scroll();
            for (i, id) in matches.iter().enumerate() {
                let Some(el) = self.scene.get(id) else {
                    continue;
                };
                let text = el.text().replace('\n', " ");
                let preview: String = text.chars().take(80).collect();
                let lower = preview.to_lowercase();
                // Highlight the first hit (char-aligned when lowercase keeps length).
                let parts = lower
                    .find(&ql)
                    .filter(|_| lower.len() == preview.len())
                    .map(|s| {
                        let e = s + ql.len();
                        (
                            preview[..s].to_string(),
                            preview[s..e].to_string(),
                            preview[e..].to_string(),
                        )
                    })
                    .unwrap_or((preview.clone(), String::new(), String::new()));
                let sel = format!("search-hit-{i}");
                let active = i == self.search_index && self.selected.len() == 1;
                list = list.child(
                    div()
                        .id(SharedString::from(sel.clone()))
                        .debug_selector(move || sel)
                        .px(px(8.0))
                        .py(px(6.0))
                        .rounded(px(6.0))
                        .flex()
                        .text_size(px(13.0))
                        .text_color(style.fg)
                        .when(active, |d| d.bg(style.active_bg))
                        .hover(|s| s.bg(style.hover_bg))
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .on_click(cx.listener(move |v, _: &ClickEvent, _, cx| {
                            v.focus_match(i);
                            cx.notify();
                        }))
                        .child(parts.0)
                        .child(
                            div()
                                .bg(gpui_kit::rgba(0xffc94a99))
                                .rounded(px(2.0))
                                .child(parts.1),
                        )
                        .child(parts.2),
                );
            }
            col = col.child(list);
        }
        col
    }

    fn render_library_tab(
        &self,
        style: &ExcalidrawStyle,
        pal: Palette,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        const CELL: f32 = 62.0;
        let heading = |t: &'static str| {
            div()
                .text_size(px(16.0))
                .font_weight(FontWeight::BOLD)
                .text_color(style.selection)
                .child(t)
        };
        let mut grid = div()
            .id("lib-list")
            .flex()
            .flex_wrap()
            .gap(px(8.0))
            .w_full();
        let can_add = !self.selected.is_empty();
        if can_add {
            grid = grid.child(
                div()
                    .id("lib-add")
                    .debug_selector(|| "lib-add".into())
                    .size(px(CELL))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(8.0))
                    .border_1()
                    .border_color(style.island_border)
                    .hover(|s| s.bg(style.hover_bg))
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(
                        cx.listener(|v, _: &ClickEvent, _, cx| v.add_selection_to_library(cx)),
                    )
                    .child(
                        div()
                            .size(px(26.0))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(6.0))
                            .bg(style.selection)
                            .text_color(style.island_bg)
                            .text_size(px(18.0))
                            .child("+"),
                    ),
            );
        }
        for (i, item) in self.library.items.iter().enumerate() {
            let id = crate::library::item_id(item).to_string();
            let els = crate::library::item_elements(item);
            let n = els.len();
            let name: SharedString = crate::library::item_name(item)
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string())
                .unwrap_or_else(|| {
                    format!(
                        "Item {} ({n} element{})",
                        i + 1,
                        if n == 1 { "" } else { "s" }
                    )
                })
                .into();
            let sel = format!("lib-item-{i}");
            let (id2, id3) = (id.clone(), id.clone());
            let caches = self.lib_caches.clone();
            grid = grid.child(
                div()
                    .id(SharedString::from(sel.clone()))
                    .debug_selector(move || sel)
                    .group("lib-cell")
                    .relative()
                    .size(px(CELL))
                    .rounded(px(8.0))
                    .border_1()
                    .border_color(style.island_border)
                    .bg(pal.canvas.to_hsla())
                    .overflow_hidden()
                    .hover(|s| s.border_color(style.selection))
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.insert_library_item(&id2, None, cx)
                    }))
                    .on_drag(
                        LibraryDrag {
                            id: id3,
                            name: name.clone(),
                        },
                        |d, _, _, cx| cx.new(|_| d.clone()),
                    )
                    .child(
                        canvas(
                            |_, _, _| (),
                            move |b, _, window, _| paint_thumbnail(&els, b, &caches, pal, window),
                        )
                        .absolute()
                        .inset_0(),
                    )
                    .child(
                        div()
                            .absolute()
                            .top(px(1.0))
                            .right(px(1.0))
                            .invisible()
                            .group_hover("lib-cell", |s| s.visible())
                            .child(
                                text_button(
                                    format!("lib-remove-{i}").into(),
                                    "×",
                                    false,
                                    style,
                                    move |v, _, _| v.library.remove(&id),
                                    cx,
                                )
                                .min_w(px(18.0))
                                .h(px(18.0))
                                .px(px(2.0)),
                            ),
                    ),
            );
        }
        let lib_menu = self.lib_menu.then(|| {
            let mi = |id: &'static str,
                      label: &'static str,
                      on: fn(&mut ExcalidrawView, &mut Context<ExcalidrawView>),
                      cx: &mut Context<Self>| {
                div()
                    .id(id)
                    .debug_selector(move || id.to_string())
                    .w_full()
                    .h(px(30.0))
                    .px(px(10.0))
                    .flex()
                    .items_center()
                    .rounded(px(6.0))
                    .text_size(px(13.0))
                    .text_color(style.fg)
                    .hover(|s| s.bg(style.hover_bg))
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(cx.listener(move |v, _: &ClickEvent, _, cx| {
                        v.lib_menu = false;
                        on(v, cx);
                        cx.notify();
                    }))
                    .child(label)
            };
            island(style)
                .id("lib-menu")
                .debug_selector(|| "lib-menu".into())
                .absolute()
                .top(px(30.0))
                .right_0()
                .w(px(180.0))
                .flex_col()
                .items_start()
                .child(mi(
                    "lib-import",
                    "Open",
                    |v, cx| v.import_library_prompt(cx),
                    cx,
                ))
                .child(mi(
                    "lib-export",
                    "Save to...",
                    |v, cx| v.export_library_prompt(cx),
                    cx,
                ))
                .child(mi(
                    "lib-reset",
                    "Reset library",
                    |v, cx| v.reset_library(cx),
                    cx,
                ))
        });
        let empty = |title: &'static str, sub: Option<&'static str>| {
            div()
                .w_full()
                .py(px(24.0))
                .flex()
                .flex_col()
                .items_center()
                .gap(px(8.0))
                .child(div().text_size(px(14.0)).text_color(style.fg).child(title))
                .when_some(sub, |d, s| {
                    d.child(
                        div()
                            .text_size(px(12.0))
                            .text_color(style.muted)
                            .text_center()
                            .child(s),
                    )
                })
        };
        div()
            .flex_1()
            .flex()
            .flex_col()
            .gap(px(12.0))
            .min_h_0()
            .child(
                div()
                    .id("lib-scroll")
                    .flex_1()
                    .flex()
                    .flex_col()
                    .gap(px(12.0))
                    .overflow_y_scroll()
                    .child(
                        div()
                            .relative()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(heading("Personal Library"))
                            .child(text_button(
                                "lib-more".into(),
                                "⋮",
                                self.lib_menu,
                                style,
                                |v, _, _| v.lib_menu = !v.lib_menu,
                                cx,
                            ))
                            .children(lib_menu),
                    )
                    .when(self.library.items.is_empty() && !can_add, |d| {
                        d.child(empty(
                            "No items added yet...",
                            Some(
                                "Select an item on canvas to add it here, or install a library from the public repository, below.",
                            ),
                        ))
                    })
                    .child(grid),
            )
            .child(
                div()
                    .id("lib-browse")
                    .debug_selector(|| "lib-browse".into())
                    .h(px(36.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(8.0))
                    .bg(style.selection)
                    .text_color(style.island_bg)
                    .text_size(px(13.0))
                    .font_weight(FontWeight::SEMIBOLD)
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(cx.listener(|_, _: &ClickEvent, _, cx| {
                        cx.open_url("https://libraries.excalidraw.com")
                    }))
                    .child("Browse libraries"),
            )
    }

    fn render_picker(
        &self,
        t: PickerTarget,
        style: &ExcalidrawStyle,
        pal: Palette,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let cur = self.current_color(t);
        let pos = crate::actions::palette_pos(&cur);
        let mut grid = div()
            .flex()
            .flex_wrap()
            .gap(px(6.0))
            .w(px(5.0 * 28.0 + 4.0 * 6.0));
        for (i, (name, shades)) in PALETTE.iter().enumerate() {
            let c = shades[crate::actions::default_shade(t)];
            let active = pos.map(|p| p.0) == Some(i);
            grid = grid.child(
                swatch(
                    format!("pk-{name}").into(),
                    c,
                    active,
                    pal,
                    style,
                    move |v, _, cx| v.set_color(t, c, cx),
                    cx,
                )
                .size(px(28.0))
                .relative()
                .child(
                    div()
                        .absolute()
                        .bottom(px(0.0))
                        .right(px(2.0))
                        .text_size(px(9.0))
                        .text_color(style.muted)
                        .child(PALETTE_KEYS[i]),
                ),
            );
        }
        let mut shades_row = row().gap(px(6.0));
        if let Some((hue, Some(k))) = pos {
            for (n, c) in PALETTE[hue].1.iter().enumerate() {
                let c: &'static str = c;
                shades_row = shades_row.child(
                    swatch(
                        format!("pk-shade-{}", n + 1).into(),
                        c,
                        n == k,
                        pal,
                        style,
                        move |v, _, cx| v.set_color(t, c, cx),
                        cx,
                    )
                    .size(px(28.0)),
                );
            }
        }
        let hex_text = match &self.hex {
            Some(h) => format!("#{h}▏"),
            None => cur.clone(),
        };
        let hex = div()
            .id("pk-hex")
            .debug_selector(|| "pk-hex".into())
            .flex_1()
            .h(px(28.0))
            .px(px(8.0))
            .flex()
            .items_center()
            .rounded(px(6.0))
            .border_1()
            .border_color(if self.hex.is_some() {
                style.selection
            } else {
                style.island_border
            })
            .text_size(px(13.0))
            .text_color(style.fg)
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                this.hex = Some(String::new());
                cx.notify();
            }))
            .child(hex_text);
        let dropper = text_button(
            "pk-eyedropper".into(),
            "Pick",
            self.eyedropper.is_some(),
            style,
            move |v, _, _| {
                v.eyedropper = if v.eyedropper.is_some() {
                    None
                } else {
                    Some(t)
                };
            },
            cx,
        );
        let title = match t {
            PickerTarget::Stroke => "Stroke",
            PickerTarget::Background => "Background",
            PickerTarget::Canvas => "Canvas background",
        };
        div().absolute().top(px(72.0)).left(px(232.0)).child(
            island(style)
                .id("picker")
                .debug_selector(|| "picker".into())
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .flex_col()
                .items_start()
                .p(px(10.0))
                .gap(px(8.0))
                .child(section("Colors", style).child(grid))
                .when(pos.is_some_and(|p| p.1.is_some()), |d| {
                    d.child(section("Shades", style).child(shades_row))
                })
                .child(section("Hex code", style).child(row().w_full().child(hex).child(dropper)))
                .child(
                    div()
                        .text_size(px(10.0))
                        .text_color(style.muted)
                        .child(format!("{title}: keys q–b pick a colour, 1–5 a shade")),
                ),
        )
    }

    fn render_context_menu(
        &self,
        style: &ExcalidrawStyle,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let at = self.context_menu.unwrap_or_default();
        let b = self.bounds.get();
        let entries = self.context_entries();
        let rows = entries.iter().filter(|e| e.is_some()).count() as f32;
        let seps = entries.iter().filter(|e| e.is_none()).count() as f32;
        let (w, h) = (240.0, rows * 28.0 + seps * 9.0 + 12.0);
        let x = f32::from(at.x - b.origin.x)
            .min(f32::from(b.size.width) - w - 8.0)
            .max(8.0);
        let y = f32::from(at.y - b.origin.y)
            .min(f32::from(b.size.height) - h - 8.0)
            .max(8.0);
        let mut list = island(style)
            .id("context-menu")
            .debug_selector(|| "context-menu".into())
            .flex_col()
            .items_stretch()
            .w(px(w))
            .p(px(6.0))
            .gap(px(0.0))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_mouse_down(MouseButton::Right, |_, _, cx| cx.stop_propagation());
        let mut last_sep = true;
        for e in entries {
            match e {
                None => {
                    if !last_sep {
                        list = list.child(
                            div()
                                .my(px(4.0))
                                .h(px(1.0))
                                .w_full()
                                .bg(style.island_border),
                        );
                    }
                    last_sep = true;
                }
                Some((a, label, key, checked)) => {
                    last_sep = false;
                    let id = format!("cm-{}", a.id());
                    let sel = id.clone();
                    list = list.child(
                        div()
                            .id(SharedString::from(id))
                            .debug_selector(move || sel)
                            .h(px(28.0))
                            .px(px(8.0))
                            .flex()
                            .items_center()
                            .justify_between()
                            .rounded(px(4.0))
                            .text_size(px(13.0))
                            .text_color(if a == crate::actions::Action::Delete {
                                gpui_kit::red()
                            } else {
                                style.fg
                            })
                            .hover(|s| s.bg(style.hover_bg))
                            .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                                this.run_action(a, window, cx)
                            }))
                            .child(format!("{}{}", if checked { "✓ " } else { "" }, label))
                            .child(div().text_size(px(11.0)).text_color(style.muted).child(key)),
                    );
                }
            }
        }
        div().absolute().left(px(x)).top(px(y)).child(list)
    }

    fn render_dialog(
        &self,
        dlg: crate::actions::Dialog,
        style: &ExcalidrawStyle,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        use crate::actions::Dialog;
        let title = match dlg {
            Dialog::Export => "Export image",
            Dialog::Help => "Help",
            Dialog::Mermaid => "Mermaid to Excalidraw",
            Dialog::ResetLibrary => "Reset library",
        };
        let close = text_button(
            "dlg-close".into(),
            "×",
            false,
            style,
            |v, _, _| v.dialog = None,
            cx,
        );
        let header = div()
            .flex()
            .w_full()
            .justify_between()
            .items_center()
            .child(div().text_size(px(16.0)).text_color(style.fg).child(title))
            .child(close);
        let body = match dlg {
            Dialog::Help => self.render_help(style, cx).into_any_element(),
            Dialog::Mermaid => self.render_mermaid(style).into_any_element(),
            Dialog::Export => self.render_export(style, cx).into_any_element(),
            Dialog::ResetLibrary => div()
                .flex()
                .flex_col()
                .gap(px(16.0))
                .w_full()
                .child(
                    div()
                        .text_size(px(14.0))
                        .text_color(style.fg)
                        .child("Are you sure you want to clear the library?"),
                )
                .child(
                    row()
                        .justify_end()
                        .w_full()
                        .gap(px(8.0))
                        .child(text_button(
                            "dlg-cancel".into(),
                            "Cancel",
                            false,
                            style,
                            |v, _, _| v.dialog = None,
                            cx,
                        ))
                        .child(text_button(
                            "dlg-confirm".into(),
                            "Confirm",
                            true,
                            style,
                            |v, _, cx| v.confirm_reset_library(cx),
                            cx,
                        )),
                )
                .into_any_element(),
        };
        div()
            .id("dialog-backdrop")
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .bg(gpui_kit::black().opacity(0.3))
            .occlude()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    this.dialog = None;
                    cx.notify();
                }),
            )
            .child(
                island(style)
                    .id(SharedString::from(format!(
                        "dialog-{}",
                        title.to_lowercase().replace(' ', "-")
                    )))
                    .debug_selector(move || {
                        format!("dialog-{}", title.to_lowercase().replace(' ', "-"))
                    })
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .flex_col()
                    .items_start()
                    .w(px(match dlg {
                        Dialog::Help => 920.0,
                        Dialog::Mermaid => 1200.0,
                        Dialog::Export => 1000.0,
                        Dialog::ResetLibrary => 400.0,
                    }))
                    .max_h(self.bounds.get().size.height - px(40.0))
                    .overflow_y_scroll()
                    .p(px(16.0))
                    .gap(px(12.0))
                    .child(header)
                    .child(body),
            )
    }

    fn render_help(&self, style: &ExcalidrawStyle, cx: &mut Context<Self>) -> impl IntoElement {
        let mut links = row().gap(px(8.0));
        for (i, (label, url)) in HELP_LINKS.iter().enumerate() {
            let url = *url;
            links = links.child(
                div()
                    .id(SharedString::from(format!("help-link-{i}")))
                    .h(px(36.0))
                    .px(px(12.0))
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .rounded(px(8.0))
                    .bg(style.hover_bg)
                    .hover(|s| s.bg(style.active_bg))
                    .text_size(px(12.0))
                    .text_color(style.fg)
                    .on_click(cx.listener(move |_, _: &ClickEvent, _, cx| cx.open_url(url)))
                    .child(icon(Icon::External, 14.0, style.fg))
                    .child(*label),
            );
        }
        let group = |name: &'static str, items: &'static [(&'static str, Keys)]| {
            let mut table = div()
                .flex()
                .flex_col()
                .w_full()
                .rounded(px(6.0))
                .border_1()
                .border_color(style.island_border);
            for (n, (label, alts)) in items.iter().enumerate() {
                let mut keys = div().flex().items_center().gap(px(4.0));
                for (k, seq) in alts.iter().enumerate() {
                    if k > 0 {
                        keys =
                            keys.child(div().text_size(px(12.0)).text_color(style.fg).child("or"));
                    }
                    for key in seq.iter() {
                        keys = keys.child(key_chip(key, style));
                    }
                }
                table = table.child(
                    div()
                        .flex()
                        .justify_between()
                        .items_center()
                        .gap(px(8.0))
                        .min_h(px(38.0))
                        .px(px(12.0))
                        .when(n > 0, |d| d.border_t_1().border_color(style.island_border))
                        .text_size(px(13.0))
                        .text_color(style.fg)
                        .child(*label)
                        .child(keys),
                );
            }
            div()
                .flex()
                .flex_col()
                .w_full()
                .gap(px(8.0))
                .child(
                    div()
                        .text_size(px(15.0))
                        .font_weight(FontWeight::BOLD)
                        .text_color(style.fg)
                        .child(name),
                )
                .child(table)
        };
        let (tools, view, editor) = (HELP[0], HELP[1], HELP[2]);
        div()
            .flex()
            .flex_col()
            .w_full()
            .gap(px(16.0))
            .child(links)
            .child(
                div()
                    .text_size(px(17.0))
                    .font_weight(FontWeight::BOLD)
                    .text_color(style.fg)
                    .child("Keyboard shortcuts"),
            )
            .child(
                div()
                    .flex()
                    .gap(px(24.0))
                    .items_start()
                    .w_full()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .gap(px(16.0))
                            .child(group(tools.0, tools.1))
                            .child(group(view.0, view.1)),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .child(group(editor.0, editor.1)),
                    ),
            )
    }

    fn render_mermaid(&self, style: &ExcalidrawStyle) -> impl IntoElement {
        let pane = |title: &'static str, body: AnyElement| {
            div()
                .flex()
                .flex_col()
                .flex_1()
                .gap(px(6.0))
                .child(div().text_size(px(13.0)).text_color(style.fg).child(title))
                .child(
                    div()
                        .h(px(360.0))
                        .w_full()
                        .rounded(px(6.0))
                        .border_1()
                        .border_color(style.island_border)
                        .p(px(12.0))
                        .child(body),
                )
        };
        let mut src = div()
            .flex()
            .flex_col()
            .font_family("Menlo")
            .text_size(px(12.0))
            .text_color(style.fg);
        for l in MERMAID_SAMPLE.lines() {
            src = src.child(l.to_string());
        }
        let err = div()
            .flex()
            .flex_col()
            .items_center()
            .gap(px(12.0))
            .text_color(gpui_kit::red())
            .child(
                div()
                    .text_size(px(24.0))
                    .font_weight(FontWeight::BOLD)
                    .child("Error!"),
            )
            .child(
                div()
                    .id("mermaid-error")
                    .debug_selector(|| "mermaid-error".into())
                    .font_family("Menlo")
                    .text_size(px(12.0))
                    .child(crate::actions::MERMAID_UNAVAILABLE),
            );
        div()
            .flex()
            .flex_col()
            .w_full()
            .gap(px(12.0))
            .child(
                div()
                    .text_size(px(13.0))
                    .italic()
                    .text_color(style.fg)
                    .child("Currently only Flowchart, Sequence, and Class Diagrams are supported. The other types will be rendered as image in Excalidraw."),
            )
            .child(
                div()
                    .flex()
                    .gap(px(24.0))
                    .w_full()
                    .child(pane("Mermaid Syntax", src.into_any_element()))
                    .child(pane("Preview", err.into_any_element())),
            )
            .child(
                div().flex().w_full().justify_end().child(
                    div()
                        .px(px(24.0))
                        .h(px(36.0))
                        .flex()
                        .items_center()
                        .rounded(px(8.0))
                        .bg(style.selection.opacity(0.5))
                        .text_size(px(13.0))
                        .text_color(gpui_kit::white())
                        .child("Insert →"),
                ),
            )
    }

    fn render_export(&self, style: &ExcalidrawStyle, cx: &mut Context<Self>) -> impl IntoElement {
        let e = self.export.clone();
        let accent = style.selection;
        let toggle = |id: &'static str,
                      label: &'static str,
                      on: bool,
                      f: fn(&mut crate::actions::ExportState),
                      cx: &mut Context<Self>| {
            let sel = id.to_string();
            div()
                .flex()
                .w_full()
                .h(px(32.0))
                .justify_between()
                .items_center()
                .text_size(px(15.0))
                .text_color(style.fg)
                .child(label)
                .child(
                    // Stock switch: a pill with a sliding knob.
                    div()
                        .id(id)
                        .debug_selector(move || sel)
                        .w(px(38.0))
                        .h(px(20.0))
                        .p(px(3.0))
                        .flex()
                        .items_center()
                        .when(on, |d| d.justify_end())
                        .rounded(px(10.0))
                        .border_1()
                        .border_color(if on { accent } else { style.fg })
                        .bg(if on { accent } else { style.island_bg })
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .on_click(cx.listener(move |v, _: &ClickEvent, _, cx| {
                            f(&mut v.export);
                            cx.notify();
                        }))
                        .child(div().size(px(12.0)).rounded(px(6.0)).bg(if on {
                            style.island_bg
                        } else {
                            style.fg
                        })),
                )
        };
        let mut scales = div()
            .flex()
            .p(px(2.0))
            .gap(px(2.0))
            .rounded(px(8.0))
            .border_1()
            .border_color(style.island_border);
        for s in [1u8, 2, 3] {
            let id = format!("ex-{s}");
            let sel = id.clone();
            let active = e.scale == s as f64;
            scales = scales.child(
                div()
                    .id(SharedString::from(id))
                    .debug_selector(move || sel)
                    .w(px(26.0))
                    .h(px(22.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(6.0))
                    .text_size(px(11.0))
                    .when(active, |d| d.bg(accent).text_color(style.island_bg))
                    .when(!active, |d| {
                        d.text_color(accent).hover(|s| s.bg(style.hover_bg))
                    })
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(cx.listener(move |v, _: &ClickEvent, _, cx| {
                        v.export.scale = s as f64;
                        cx.notify();
                    }))
                    .child(format!("{s}×")),
            );
        }
        let primary = |id: &'static str,
                       ic: Icon,
                       label: &'static str,
                       on: fn(&mut ExcalidrawView, &mut Context<ExcalidrawView>),
                       cx: &mut Context<Self>| {
            div()
                .id(id)
                .debug_selector(move || id.to_string())
                .flex_shrink_0()
                .h(px(40.0))
                .px(px(12.0))
                .flex()
                .items_center()
                .gap(px(8.0))
                .rounded(px(8.0))
                .bg(accent)
                .hover(|s| s.opacity(0.9))
                .text_color(style.island_bg)
                .text_size(px(12.0))
                .font_weight(FontWeight::SEMIBOLD)
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(cx.listener(move |v, _: &ClickEvent, _, cx| {
                    on(v, cx);
                    cx.notify();
                }))
                .child(icon(ic, 16.0, style.island_bg))
                .child(label)
        };
        let entity = cx.entity();
        let preview = div()
            .id("ex-preview")
            .debug_selector(|| "ex-preview".into())
            .flex_1()
            .h(px(360.0))
            .rounded(px(8.0))
            .border_1()
            .border_color(style.island_border)
            .overflow_hidden()
            .child(
                canvas(
                    |_, _, _| (),
                    move |b, _, window, cx| {
                        paint_export_preview(entity.read(cx), b, window);
                    },
                )
                .size_full(),
            );
        let settings = div()
            .w(px(370.0))
            .flex_shrink_0()
            .flex()
            .flex_col()
            .gap(px(6.0))
            .child(toggle(
                "ex-bg",
                "Background",
                e.background,
                |s| s.background = !s.background,
                cx,
            ))
            .child(toggle(
                "ex-dark",
                "Dark mode",
                e.dark,
                |s| s.dark = !s.dark,
                cx,
            ))
            .child(toggle(
                "ex-embed",
                "Embed scene",
                e.embed_scene,
                |s| s.embed_scene = !s.embed_scene,
                cx,
            ))
            .when(!self.selected.is_empty(), |d| {
                d.child(toggle(
                    "ex-only",
                    "Only selected",
                    e.only_selected,
                    |s| s.only_selected = !s.only_selected,
                    cx,
                ))
            })
            .child(
                div()
                    .flex()
                    .w_full()
                    .h(px(36.0))
                    .justify_between()
                    .items_center()
                    .text_size(px(15.0))
                    .text_color(style.fg)
                    .child("Scale")
                    .child(scales),
            )
            .child(div().flex_1())
            .child(
                row()
                    .gap(px(8.0))
                    .justify_end()
                    .child(primary(
                        "ex-png",
                        Icon::Download,
                        "PNG",
                        |v, cx| v.export_to_file(false, cx),
                        cx,
                    ))
                    .child(primary(
                        "ex-svg",
                        Icon::Download,
                        "SVG",
                        |v, cx| v.export_to_file(true, cx),
                        cx,
                    ))
                    .child(primary(
                        "ex-copy",
                        Icon::Copy,
                        "Copy to clipboard",
                        |v, cx| {
                            v.copy_as_png(cx);
                        },
                        cx,
                    )),
            );
        div()
            .flex()
            .w_full()
            .h(px(360.0))
            .gap(px(32.0))
            .child(preview)
            .child(settings)
    }
}

/// The export dialog's live preview: the exported area fitted into `b` on
/// a checkerboard, with the export's background and dark-mode settings.
fn paint_export_preview(v: &ExcalidrawView, b: Bounds<Pixels>, window: &mut Window) {
    let tile = 10.0f32;
    let (w, h) = (f32::from(b.size.width), f32::from(b.size.height));
    let light: Hsla = gpui_kit::rgb(0xffffff).into();
    let dark_tile: Hsla = gpui_kit::rgb(0xe9ecef).into();
    window.paint_quad(fill(b, light));
    let (cols, rows) = ((w / tile).ceil() as i32, (h / tile).ceil() as i32);
    for r in 0..rows {
        for c in 0..cols {
            if (r + c) % 2 == 1 {
                let o = point(
                    b.origin.x + px(c as f32 * tile),
                    b.origin.y + px(r as f32 * tile),
                );
                window.paint_quad(fill(Bounds::new(o, size(px(tile), px(tile))), dark_tile));
            }
        }
    }
    let opts = v.export_options();
    let els = crate::export::export_elements(&v.scene, opts.only.as_ref());
    let Some(bb) = crate::export::export_bounds(&els) else {
        return;
    };
    let pad = opts.padding;
    let bb = [bb[0] - pad, bb[1] - pad, bb[2] + pad, bb[3] + pad];
    let margin = 24.0;
    let (aw, ah) = (f64::from(w) - 2.0 * margin, f64::from(h) - 2.0 * margin);
    let (bw, bh) = ((bb[2] - bb[0]).max(1.0), (bb[3] - bb[1]).max(1.0));
    let zoom = (aw / bw).min(ah / bh).min(2.0);
    let cam = Camera {
        scroll_x: f64::from(w) / 2.0 / zoom - (bb[0] + bb[2]) / 2.0,
        scroll_y: f64::from(h) / 2.0 / zoom - (bb[1] + bb[3]) / 2.0,
        zoom,
    };
    let bg = color::parse(&v.scene.view_background()).unwrap_or(Rgba::new(1.0, 1.0, 1.0, 1.0));
    let pal = Palette {
        dark: opts.dark,
        canvas: if opts.dark { bg.dark_filter() } else { bg },
    };
    window.with_content_mask(Some(ContentMask { bounds: b }), |window| {
        if opts.background {
            let s = |x: f64, y: f64| {
                point(
                    b.origin.x + px(((x + cam.scroll_x) * zoom) as f32),
                    b.origin.y + px(((y + cam.scroll_y) * zoom) as f32),
                )
            };
            window.paint_quad(fill(
                Bounds::from_corners(s(bb[0], bb[1]), s(bb[2], bb[3])),
                pal.canvas.to_hsla(),
            ));
        }
        let caches = v.caches.clone();
        let mut caches = caches.borrow_mut();
        for el in els {
            render::paint_element(el, &v.scene, &mut caches, cam, b.origin, pal, 1.0, window);
        }
    });
}
