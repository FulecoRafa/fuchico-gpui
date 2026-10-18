//! Element geometry: local frames, rotation, bounds and hit-testing, using
//! Excalidraw's conventions (rotation about the centre of the element's
//! local bounding box; linear/freedraw points relative to `x`/`y`).

use crate::color;
use crate::model::Element;
use crate::shape;

pub type P = [f64; 2];

/// `[min_x, min_y, max_x, max_y]` of the element in local coordinates
/// (relative to `x`/`y`, unrotated).
pub fn local_bounds(el: &Element) -> [f64; 4] {
    if el.has_points() {
        let pts = el.points();
        if pts.is_empty() {
            return [0.0, 0.0, 0.0, 0.0];
        }
        let mut b = [f64::MAX, f64::MAX, f64::MIN, f64::MIN];
        for p in pts {
            b[0] = b[0].min(p[0]);
            b[1] = b[1].min(p[1]);
            b[2] = b[2].max(p[0]);
            b[3] = b[3].max(p[1]);
        }
        b
    } else {
        let (w, h) = (el.width(), el.height());
        [w.min(0.0), h.min(0.0), w.max(0.0), h.max(0.0)]
    }
}

/// Local centre (relative to `x`/`y`).
pub fn local_center(el: &Element) -> P {
    let b = local_bounds(el);
    [(b[0] + b[2]) / 2.0, (b[1] + b[3]) / 2.0]
}

/// Rotation centre in world coordinates.
pub fn center(el: &Element) -> P {
    let c = local_center(el);
    [el.x() + c[0], el.y() + c[1]]
}

pub fn rotate(p: P, c: P, a: f64) -> P {
    if a == 0.0 {
        return p;
    }
    let (s, co) = a.sin_cos();
    let (dx, dy) = (p[0] - c[0], p[1] - c[1]);
    [c[0] + dx * co - dy * s, c[1] + dx * s + dy * co]
}

/// Local (relative to x/y) → world.
pub fn to_world(el: &Element, p: P) -> P {
    rotate([el.x() + p[0], el.y() + p[1]], center(el), el.angle())
}

/// World → local (relative to x/y, unrotated).
pub fn to_local(el: &Element, p: P) -> P {
    let q = rotate(p, center(el), -el.angle());
    [q[0] - el.x(), q[1] - el.y()]
}

/// World-space axis-aligned bounds of the rotated element.
pub fn aabb(el: &Element) -> [f64; 4] {
    let b = local_bounds(el);
    if el.has_points() && el.angle() != 0.0 {
        // Tighter: rotate the points themselves.
        let mut r = [f64::MAX, f64::MAX, f64::MIN, f64::MIN];
        for p in el.points() {
            let w = to_world(el, p);
            r = [
                r[0].min(w[0]),
                r[1].min(w[1]),
                r[2].max(w[0]),
                r[3].max(w[1]),
            ];
        }
        return r;
    }
    let mut r = [f64::MAX, f64::MAX, f64::MIN, f64::MIN];
    for p in [[b[0], b[1]], [b[2], b[1]], [b[2], b[3]], [b[0], b[3]]] {
        let w = to_world(el, p);
        r = [
            r[0].min(w[0]),
            r[1].min(w[1]),
            r[2].max(w[0]),
            r[3].max(w[1]),
        ];
    }
    r
}

pub fn union(a: [f64; 4], b: [f64; 4]) -> [f64; 4] {
    [
        a[0].min(b[0]),
        a[1].min(b[1]),
        a[2].max(b[2]),
        a[3].max(b[3]),
    ]
}

pub fn dist(a: P, b: P) -> f64 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)).sqrt()
}

pub fn dist_to_segment(p: P, a: P, b: P) -> f64 {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let l2 = dx * dx + dy * dy;
    if l2 == 0.0 {
        return dist(p, a);
    }
    let t = (((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / l2).clamp(0.0, 1.0);
    dist(p, [a[0] + t * dx, a[1] + t * dy])
}

fn dist_to_polyline(p: P, pts: &[P], closed: bool) -> f64 {
    let mut d = f64::MAX;
    for w in pts.windows(2) {
        d = d.min(dist_to_segment(p, w[0], w[1]));
    }
    if closed && pts.len() > 2 {
        d = d.min(dist_to_segment(p, pts[pts.len() - 1], pts[0]));
    }
    if pts.len() == 1 {
        d = dist(p, pts[0]);
    }
    d
}

fn point_in_polygon(p: P, poly: &[P]) -> bool {
    let mut inside = false;
    let n = poly.len();
    let mut j = n.wrapping_sub(1);
    for i in 0..n {
        let (a, b) = (poly[i], poly[j]);
        if (a[1] > p[1]) != (b[1] > p[1])
            && p[0] < (b[0] - a[0]) * (p[1] - a[1]) / (b[1] - a[1]) + a[0]
        {
            inside = !inside;
        }
        j = i;
    }
    inside
}

/// Catmull-Rom sampling of a rounded linear element (matches the visual
/// curve well enough for hit-testing).
pub fn sampled_points(el: &Element) -> Vec<P> {
    let pts = el.points();
    if el.f.roundness.get().is_none() || pts.len() < 3 || el.kind == "freedraw" {
        return pts;
    }
    let mut out = Vec::new();
    for i in 0..pts.len() - 1 {
        let p0 = pts[i.saturating_sub(1)];
        let p1 = pts[i];
        let p2 = pts[i + 1];
        let p3 = pts[(i + 2).min(pts.len() - 1)];
        for k in 0..8 {
            let t = k as f64 / 8.0;
            let t2 = t * t;
            let t3 = t2 * t;
            let f = |a: f64, b: f64, c: f64, d: f64| {
                0.5 * (2.0 * b
                    + (-a + c) * t
                    + (2.0 * a - 5.0 * b + 4.0 * c - d) * t2
                    + (-a + 3.0 * b - 3.0 * c + d) * t3)
            };
            out.push([f(p0[0], p1[0], p2[0], p3[0]), f(p0[1], p1[1], p2[1], p3[1])]);
        }
    }
    out.push(*pts.last().unwrap());
    out
}

/// Whether a world point hits the element (`tol` in world units).
/// `filled_hint` forces interior hits (bound-text containers, images).
pub fn hit(el: &Element, p: P, tol: f64) -> bool {
    let q = to_local(el, p);
    let b = local_bounds(el);
    let (w, h) = (b[2] - b[0], b[3] - b[1]);
    let filled = !color::is_transparent(el.background_color());
    let tol = tol + el.stroke_width() / 2.0;
    match el.kind.as_str() {
        "text" | "image" | "embeddable" | "iframe" | "magicframe" => {
            q[0] >= b[0] - tol && q[0] <= b[2] + tol && q[1] >= b[1] - tol && q[1] <= b[3] + tol
        }
        "frame" => {
            // The border or the name label above it.
            let border =
                dist_to_polyline(q, &[[0.0, 0.0], [w, 0.0], [w, h], [0.0, h]], true) <= tol;
            let label = q[1] < 0.0 && q[1] > -24.0 && q[0] >= 0.0 && q[0] <= w.min(200.0);
            border || label
        }
        "rectangle" => {
            let inside = q[0] >= 0.0 && q[0] <= w && q[1] >= 0.0 && q[1] <= h;
            (filled && inside)
                || dist_to_polyline(q, &[[0.0, 0.0], [w, 0.0], [w, h], [0.0, h]], true) <= tol
        }
        "diamond" => {
            let d = shape::diamond_points(el);
            let poly: Vec<P> = d.to_vec();
            (filled && point_in_polygon(q, &poly)) || dist_to_polyline(q, &poly, true) <= tol
        }
        "ellipse" => {
            let (rx, ry) = (w / 2.0, h / 2.0);
            if rx <= 0.0 || ry <= 0.0 {
                return dist(q, [rx, ry]) <= tol;
            }
            let (dx, dy) = (q[0] - rx, q[1] - ry);
            let n = ((dx / rx).powi(2) + (dy / ry).powi(2)).sqrt();
            if filled && n <= 1.0 {
                return true;
            }
            (n - 1.0).abs() * rx.min(ry) <= tol
        }
        "line" | "arrow" | "freedraw" => {
            let pts = sampled_points(el);
            let closed = filled && shape::is_path_a_loop(&el.points());
            if closed && point_in_polygon(q, &pts) {
                return true;
            }
            let extra = if el.kind == "freedraw" {
                el.stroke_width() * 2.0
            } else {
                0.0
            };
            dist_to_polyline(q, &pts, false) <= tol + extra
        }
        _ => q[0] >= b[0] - tol && q[0] <= b[2] + tol && q[1] >= b[1] - tol && q[1] <= b[3] + tol,
    }
}

/// Whether the element's world AABB lies fully inside `r`.
pub fn inside_rect(el: &Element, r: [f64; 4]) -> bool {
    let b = aabb(el);
    b[0] >= r[0] && b[1] >= r[1] && b[2] <= r[2] && b[3] <= r[3]
}

/// Whether the element intersects a world rect (eraser / frames).
pub fn intersects_rect(el: &Element, r: [f64; 4]) -> bool {
    let b = aabb(el);
    b[0] <= r[2] && b[2] >= r[0] && b[1] <= r[3] && b[3] >= r[1]
}

/// Point on the outline of a bindable element where the ray from `from`
/// toward the element centre first meets it (world coordinates).
pub fn outline_toward_center(el: &Element, from: P) -> P {
    let c = center(el);
    let f = to_local(el, from);
    let lc = local_center(el);
    let (w, h) = (el.width().abs(), el.height().abs());
    let (dx, dy) = (lc[0] - f[0], lc[1] - f[1]);
    let len = (dx * dx + dy * dy).sqrt();
    if len < 1e-6 {
        return c;
    }
    // Parametrise from the centre outwards: point = lc - t * dir.
    let (ux, uy) = (-dx / len, -dy / len);
    let t = match el.kind.as_str() {
        "ellipse" => {
            let (rx, ry) = (w / 2.0, h / 2.0);
            if rx <= 0.0 || ry <= 0.0 {
                0.0
            } else {
                1.0 / ((ux / rx).powi(2) + (uy / ry).powi(2)).sqrt()
            }
        }
        "diamond" => {
            // |x|/(w/2) + |y|/(h/2) = 1
            let (hw, hh) = (w / 2.0, h / 2.0);
            if hw <= 0.0 || hh <= 0.0 {
                0.0
            } else {
                1.0 / (ux.abs() / hw + uy.abs() / hh)
            }
        }
        _ => {
            let tx = if ux.abs() > 1e-9 {
                (w / 2.0) / ux.abs()
            } else {
                f64::MAX
            };
            let ty = if uy.abs() > 1e-9 {
                (h / 2.0) / uy.abs()
            } else {
                f64::MAX
            };
            tx.min(ty)
        }
    };
    let t = t.min(len);
    to_world(el, [lc[0] + ux * t, lc[1] + uy * t])
}

/// First intersection (world) of the ray `from + t·(toward − from)`, t ≥ 0,
/// with the element's outline.
pub fn ray_outline(el: &Element, from: P, toward: P) -> Option<P> {
    let f = to_local(el, from);
    let t = to_local(el, toward);
    let (w, h) = (el.width().abs(), el.height().abs());
    let d = [t[0] - f[0], t[1] - f[1]];
    if d[0].abs() < 1e-12 && d[1].abs() < 1e-12 {
        return None;
    }
    let best = match el.kind.as_str() {
        "ellipse" => {
            let (rx, ry) = (w / 2.0, h / 2.0);
            if rx <= 0.0 || ry <= 0.0 {
                return None;
            }
            let (ox, oy) = ((f[0] - rx) / rx, (f[1] - ry) / ry);
            let (dx, dy) = (d[0] / rx, d[1] / ry);
            let a = dx * dx + dy * dy;
            let b = 2.0 * (ox * dx + oy * dy);
            let c = ox * ox + oy * oy - 1.0;
            let disc = b * b - 4.0 * a * c;
            if disc < 0.0 {
                return None;
            }
            let s = disc.sqrt();
            [(-b - s) / (2.0 * a), (-b + s) / (2.0 * a)]
                .into_iter()
                .filter(|t| *t >= 0.0)
                .reduce(f64::min)
        }
        _ => {
            let poly: Vec<P> = if el.kind == "diamond" {
                vec![[w / 2.0, 0.0], [w, h / 2.0], [w / 2.0, h], [0.0, h / 2.0]]
            } else {
                vec![[0.0, 0.0], [w, 0.0], [w, h], [0.0, h]]
            };
            let mut best: Option<f64> = None;
            for i in 0..poly.len() {
                let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
                let e = [b[0] - a[0], b[1] - a[1]];
                let den = d[0] * e[1] - d[1] * e[0];
                if den.abs() < 1e-12 {
                    continue;
                }
                let q = [a[0] - f[0], a[1] - f[1]];
                let tt = (q[0] * e[1] - q[1] * e[0]) / den;
                let u = (q[0] * d[1] - q[1] * d[0]) / den;
                if tt >= 0.0
                    && (-1e-9..=1.0 + 1e-9).contains(&u)
                    && best.map(|b| tt < b).unwrap_or(true)
                {
                    best = Some(tt);
                }
            }
            best
        }
    }?;
    Some(to_world(el, [f[0] + d[0] * best, f[1] + d[1] * best]))
}

/// Excalidraw-style focus of a binding: the signed offset of the line
/// `adj → end` from the target's centre, as a fraction of the target's
/// half-extent across that line (−1..1; 0 aims at the centre).
pub fn focus_of(el: &Element, adj: P, end: P) -> f64 {
    let c = center(el);
    let d = [end[0] - adj[0], end[1] - adj[1]];
    let len = (d[0] * d[0] + d[1] * d[1]).sqrt();
    if len < 1e-9 {
        return 0.0;
    }
    // Signed distance of the centre from the line, along the normal of
    // the centre direction (so focus_point inverts it).
    let dc = [c[0] - adj[0], c[1] - adj[1]];
    let lc = (dc[0] * dc[0] + dc[1] * dc[1]).sqrt();
    if lc < 1e-9 {
        return 0.0;
    }
    let n = [-dc[1] / lc, dc[0] / lc];
    // Where the line passes the centre's depth.
    let along = (dc[0] * d[0] + dc[1] * d[1]) / len;
    let hit = [adj[0] + d[0] / len * along, adj[1] + d[1] / len * along];
    let off = (hit[0] - c[0]) * n[0] + (hit[1] - c[1]) * n[1];
    let half = half_extent(el, n);
    if half < 1e-9 {
        0.0
    } else {
        (off / half).clamp(-1.0, 1.0)
    }
}

/// Point the binding with `focus` aims at, seen from `adj`.
pub fn focus_point(el: &Element, adj: P, focus: f64) -> P {
    let c = center(el);
    let d = [c[0] - adj[0], c[1] - adj[1]];
    let len = (d[0] * d[0] + d[1] * d[1]).sqrt();
    if len < 1e-9 || focus == 0.0 {
        return c;
    }
    let n = [-d[1] / len, d[0] / len];
    let half = half_extent(el, n);
    [c[0] + n[0] * focus * half, c[1] + n[1] * focus * half]
}

fn half_extent(el: &Element, n: P) -> f64 {
    // Support function of the rotated box along n.
    let (w, h) = (el.width().abs() / 2.0, el.height().abs() / 2.0);
    let (s, c) = el.angle().sin_cos();
    w * (c * n[0] + s * n[1]).abs() + h * (-s * n[0] + c * n[1]).abs()
}

pub fn is_bindable(el: &Element) -> bool {
    matches!(
        el.kind.as_str(),
        "rectangle" | "diamond" | "ellipse" | "text" | "image" | "frame" | "embeddable" | "iframe"
    ) && el.container_id().is_none()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn el(json: &str) -> Element {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn rotation_round_trip() {
        let e = el(
            r#"{"id":"a","type":"rectangle","x":10,"y":20,"width":100,"height":50,"angle":0.7}"#,
        );
        let w = to_world(&e, [30.0, 5.0]);
        let l = to_local(&e, w);
        assert!((l[0] - 30.0).abs() < 1e-9 && (l[1] - 5.0).abs() < 1e-9);
    }

    #[test]
    fn hits() {
        let r = el(
            r#"{"id":"a","type":"rectangle","x":0,"y":0,"width":100,"height":50,"backgroundColor":"transparent"}"#,
        );
        assert!(hit(&r, [0.0, 25.0], 2.0));
        assert!(!hit(&r, [50.0, 25.0], 2.0));
        let l = el(
            r#"{"id":"b","type":"line","x":0,"y":0,"width":100,"height":0,"points":[[0,0],[100,0]]}"#,
        );
        assert!(hit(&l, [50.0, 1.0], 2.0));
        assert!(!hit(&l, [50.0, 30.0], 2.0));
        let e = el(r#"{"id":"c","type":"ellipse","x":0,"y":0,"width":100,"height":100}"#);
        assert!(hit(&e, [50.0, 0.0], 2.0));
        let p = outline_toward_center(&e, [-100.0, 50.0]);
        assert!((p[0] - 0.0).abs() < 1e-6 && (p[1] - 50.0).abs() < 1e-6);
    }
}
