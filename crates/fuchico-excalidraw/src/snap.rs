//! Grid snapping and Excalidraw-style object snapping (bounding-box edges
//! and centres of other elements), with guide lines.

use crate::geom::P;

pub fn snap_to_grid(p: P, size: f64) -> P {
    if size <= 0.0 {
        return p;
    }
    [(p[0] / size).round() * size, (p[1] / size).round() * size]
}

/// A guide line segment (world coordinates) shown while snapping.
pub type Guide = [P; 2];

fn stops(b: [f64; 4]) -> ([f64; 3], [f64; 3]) {
    ([b[0], (b[0] + b[2]) / 2.0, b[2]], [b[1], (b[1] + b[3]) / 2.0, b[3]])
}

/// Offset to add to a moving box so one of its x/y stops (min, centre,
/// max) lines up with a stop of one of `others`, within `tol`. Returns
/// the offset and guides through the matched stops.
pub fn snap_box(moving: [f64; 4], others: &[[f64; 4]], tol: f64) -> ([f64; 2], Vec<Guide>) {
    let (mx, my) = stops(moving);
    let mut best_x: Option<(f64, f64)> = None; // (delta, target x)
    let mut best_y: Option<(f64, f64)> = None;
    for o in others {
        let (ox, oy) = stops(*o);
        for a in mx {
            for b in ox {
                let d = b - a;
                if d.abs() <= tol && best_x.map(|(bd, _)| d.abs() < bd.abs()).unwrap_or(true) {
                    best_x = Some((d, b));
                }
            }
        }
        for a in my {
            for b in oy {
                let d = b - a;
                if d.abs() <= tol && best_y.map(|(bd, _)| d.abs() < bd.abs()).unwrap_or(true) {
                    best_y = Some((d, b));
                }
            }
        }
    }
    let off = [best_x.map(|b| b.0).unwrap_or(0.0), best_y.map(|b| b.0).unwrap_or(0.0)];
    let moved = [moving[0] + off[0], moving[1] + off[1], moving[2] + off[0], moving[3] + off[1]];
    let mut guides = Vec::new();
    if let Some((_, x)) = best_x {
        let (mut y0, mut y1) = (moved[1], moved[3]);
        for o in others {
            let (ox, _) = stops(*o);
            if ox.iter().any(|v| (v - x).abs() < 1e-6) {
                y0 = y0.min(o[1]);
                y1 = y1.max(o[3]);
            }
        }
        guides.push([[x, y0], [x, y1]]);
    }
    if let Some((_, y)) = best_y {
        let (mut x0, mut x1) = (moved[0], moved[2]);
        for o in others {
            let (_, oy) = stops(*o);
            if oy.iter().any(|v| (v - y).abs() < 1e-6) {
                x0 = x0.min(o[0]);
                x1 = x1.max(o[2]);
            }
        }
        guides.push([[x0, y], [x1, y]]);
    }
    (off, guides)
}

/// Snaps a single point to other boxes' stops.
pub fn snap_point(p: P, others: &[[f64; 4]], tol: f64) -> (P, Vec<Guide>) {
    let (off, g) = snap_box([p[0], p[1], p[0], p[1]], others, tol);
    ([p[0] + off[0], p[1] + off[1]], g)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snaps_edges_and_centres() {
        let (off, g) = snap_box([103.0, 0.0, 153.0, 50.0], &[[0.0, 200.0, 100.0, 300.0]], 5.0);
        assert_eq!(off, [-3.0, 0.0]);
        assert_eq!(g.len(), 1);
        assert_eq!(snap_to_grid([29.0, 11.0], 20.0), [20.0, 20.0]);
    }
}
