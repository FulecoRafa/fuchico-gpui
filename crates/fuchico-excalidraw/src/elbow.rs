//! Elbow (orthogonal) arrow routing and `fixedPoint` bindings.
//!
//! Excalidraw binds elbow arrows with a `fixedPoint` ratio on the target's
//! bounding box and routes heading-aware orthogonal segments between the
//! two ends, leaving each bound shape perpendicular to the side it is
//! attached to.

use crate::geom::{self, P};
use crate::model::Element;

/// Clearance from a bound shape before the first turn.
pub const PADDING: f64 = 40.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Heading {
    Up,
    Down,
    Left,
    Right,
}

impl Heading {
    pub fn v(self) -> P {
        match self {
            Heading::Up => [0.0, -1.0],
            Heading::Down => [0.0, 1.0],
            Heading::Left => [-1.0, 0.0],
            Heading::Right => [1.0, 0.0],
        }
    }
    fn horizontal(self) -> bool {
        matches!(self, Heading::Left | Heading::Right)
    }
    fn from_vec(d: P) -> Self {
        if d[0].abs() >= d[1].abs() {
            if d[0] >= 0.0 {
                Heading::Right
            } else {
                Heading::Left
            }
        } else if d[1] >= 0.0 {
            Heading::Down
        } else {
            Heading::Up
        }
    }
}

/// Snaps a world point to the nearest side of `el`'s box, as a fixed-point
/// ratio (`[fx, fy]` in 0..1 of the unrotated box); a point near a side's
/// middle snaps to it.
pub fn fixed_point_for(el: &Element, p: P) -> [f64; 2] {
    let q = geom::to_local(el, p);
    let (w, h) = (el.width().abs().max(1e-6), el.height().abs().max(1e-6));
    let (fx, fy) = ((q[0] / w).clamp(0.0, 1.0), (q[1] / h).clamp(0.0, 1.0));
    // Distance to each side, in box units.
    let sides = [(fy, 0), (1.0 - fy, 1), (fx, 2), (1.0 - fx, 3)];
    let (_, side) = sides
        .into_iter()
        .fold((f64::MAX, 0), |a, b| if b.0 < a.0 { b } else { a });
    let snap = |t: f64| if (t - 0.5).abs() < 0.15 { 0.5 } else { t };
    match side {
        0 => [snap(fx), 0.0],
        1 => [snap(fx), 1.0],
        2 => [0.0, snap(fy)],
        _ => [1.0, snap(fy)],
    }
}

/// World position of a fixed point, `gap` outside the box.
pub fn fixed_point_world(el: &Element, fp: [f64; 2], gap: f64) -> (P, Heading) {
    let (w, h) = (el.width().abs(), el.height().abs());
    let local = [fp[0] * w, fp[1] * h];
    let heading = side_heading(fp);
    let hv = heading.v();
    let p = geom::to_world(el, [local[0] + hv[0] * gap, local[1] + hv[1] * gap]);
    // The heading in world space (rotation rounded to the nearest axis).
    let wv = geom::rotate(hv, [0.0, 0.0], el.angle());
    (p, Heading::from_vec(wv))
}

fn side_heading(fp: [f64; 2]) -> Heading {
    let d = [(fp[0] - 0.5), (fp[1] - 0.5)];
    if fp[1] <= 0.0 {
        Heading::Up
    } else if fp[1] >= 1.0 {
        Heading::Down
    } else if fp[0] <= 0.0 {
        Heading::Left
    } else if fp[0] >= 1.0 {
        Heading::Right
    } else {
        Heading::from_vec(d)
    }
}

/// Orthogonal route from `s` leaving along `hs` to `e`, arriving against
/// `he` (the direction pointing *out of* the end target). Unbound ends
/// pass `None` for their heading.
pub fn route(s: P, hs: Option<Heading>, e: P, he: Option<Heading>) -> Vec<P> {
    let d = [e[0] - s[0], e[1] - s[1]];
    let hs_ = hs.unwrap_or_else(|| Heading::from_vec(d));
    let he_ = he.unwrap_or_else(|| {
        // Arrive along the dominant axis of the approach.
        Heading::from_vec([-d[0], -d[1]])
    });
    let ps = if hs.is_some() { PADDING } else { 0.0 };
    let pe = if he.is_some() { PADDING } else { 0.0 };
    let s1 = [s[0] + hs_.v()[0] * ps, s[1] + hs_.v()[1] * ps];
    let e1 = [e[0] + he_.v()[0] * pe, e[1] + he_.v()[1] * pe];
    let mut pts = vec![s, s1];
    match (hs_.horizontal(), he_.horizontal()) {
        (true, true) => {
            // Leaving the way we're heading? Use a vertical middle segment.
            let forward = (e1[0] - s1[0]) * hs_.v()[0] >= 0.0;
            if forward {
                let mx = (s1[0] + e1[0]) / 2.0;
                pts.push([mx, s1[1]]);
                pts.push([mx, e1[1]]);
            } else {
                let my = (s1[1] + e1[1]) / 2.0;
                pts.push([s1[0], my]);
                pts.push([e1[0], my]);
            }
        }
        (false, false) => {
            let forward = (e1[1] - s1[1]) * hs_.v()[1] >= 0.0;
            if forward {
                let my = (s1[1] + e1[1]) / 2.0;
                pts.push([s1[0], my]);
                pts.push([e1[0], my]);
            } else {
                let mx = (s1[0] + e1[0]) / 2.0;
                pts.push([mx, s1[1]]);
                pts.push([mx, e1[1]]);
            }
        }
        (true, false) => pts.push([e1[0], s1[1]]),
        (false, true) => pts.push([s1[0], e1[1]]),
    }
    pts.push(e1);
    pts.push(e);
    simplify(pts)
}

/// Drops duplicate and collinear points.
pub fn simplify(pts: Vec<P>) -> Vec<P> {
    let mut out: Vec<P> = Vec::new();
    for p in pts {
        if out
            .last()
            .map(|l| geom::dist(*l, p) < 1e-6)
            .unwrap_or(false)
        {
            continue;
        }
        if out.len() >= 2 {
            let a = out[out.len() - 2];
            let b = out[out.len() - 1];
            let cross = (b[0] - a[0]) * (p[1] - b[1]) - (b[1] - a[1]) * (p[0] - b[0]);
            let dot = (b[0] - a[0]) * (p[0] - b[0]) + (b[1] - a[1]) * (p[1] - b[1]);
            if cross.abs() < 1e-6 && dot >= 0.0 {
                out.pop();
            }
        }
        out.push(p);
    }
    if out.len() < 2 {
        out.push(out[0]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn orthogonal(pts: &[P]) -> bool {
        pts.windows(2)
            .all(|w| (w[0][0] - w[1][0]).abs() < 1e-6 || (w[0][1] - w[1][1]).abs() < 1e-6)
    }

    #[test]
    fn routes_are_orthogonal() {
        for (hs, he) in [
            (Some(Heading::Right), Some(Heading::Left)),
            (Some(Heading::Down), Some(Heading::Left)),
            (Some(Heading::Right), Some(Heading::Right)),
            (None, None),
            (Some(Heading::Up), None),
        ] {
            let r = route([0.0, 0.0], hs, [200.0, 120.0], he);
            assert!(orthogonal(&r), "{hs:?}->{he:?}: {r:?}");
            assert_eq!(r[0], [0.0, 0.0]);
            assert_eq!(*r.last().unwrap(), [200.0, 120.0]);
        }
        // Facing sides: a Z shape.
        let r = route(
            [0.0, 0.0],
            Some(Heading::Right),
            [200.0, 100.0],
            Some(Heading::Left),
        );
        assert_eq!(
            r,
            vec![[0.0, 0.0], [100.0, 0.0], [100.0, 100.0], [200.0, 100.0]]
        );
    }
}
