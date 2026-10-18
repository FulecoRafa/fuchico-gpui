//! Pan/zoom math, a line-for-line port of `usePanZoom.ts`.
//!
//! The content is transformed as `translate(x, y) scale(scale)` with its
//! origin at the top-left of the canvas. All coordinates are canvas-local
//! logical pixels.

pub const MIN_SCALE: f32 = 0.1;
pub const MAX_SCALE: f32 = 8.0;
/// Toolbar zoom-in multiplier.
pub const ZOOM_IN_FACTOR: f32 = 1.25;
/// Toolbar zoom-out multiplier.
pub const ZOOM_OUT_FACTOR: f32 = 0.8;
/// Wheel sensitivity: `factor = exp(-deltaY * WHEEL_SENSITIVITY)`.
pub const WHEEL_SENSITIVITY: f32 = 0.001;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PanZoom {
    pub x: f32,
    pub y: f32,
    pub scale: f32,
}

#[derive(Clone, Copy, Debug)]
struct Drag {
    start_x: f32,
    start_y: f32,
    orig_x: f32,
    orig_y: f32,
}

impl Default for PanZoom {
    fn default() -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            scale: 1.0,
        }
    }
}

impl PanZoom {
    /// `reset()`: `{x:0, y:0, scale:1}` (diagram at the top-left, not centred).
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// `zoomAt(cx, cy, factor)`: clamp the scale, keep the point under
    /// `(cx, cy)` fixed.
    pub fn zoom_at(&mut self, cx: f32, cy: f32, factor: f32) {
        let next = (self.scale * factor).clamp(MIN_SCALE, MAX_SCALE);
        let ratio = next / self.scale;
        self.x = cx - (cx - self.x) * ratio;
        self.y = cy - (cy - self.y) * ratio;
        self.scale = next;
    }

    /// Wheel handler. `delta_y` is the DOM `deltaY` (positive = scroll down =
    /// zoom out). Every wheel event zooms; there is no scroll-to-pan.
    pub fn wheel(&mut self, cx: f32, cy: f32, delta_y: f32) {
        self.zoom_at(cx, cy, (-delta_y * WHEEL_SENSITIVITY).exp());
    }

    /// Trackpad pinch: `delta` is the fractional change (0.1 = +10%). The
    /// browser delivers pinch as ctrl+wheel and so goes through [`Self::wheel`];
    /// the equivalent here is multiplying by `1 + delta`.
    pub fn pinch(&mut self, cx: f32, cy: f32, delta: f32) {
        let factor = (1.0 + delta).max(0.01);
        self.zoom_at(cx, cy, factor);
    }

    /// Toolbar zoom-in, anchored at the canvas centre.
    pub fn zoom_in(&mut self, width: f32, height: f32) {
        self.zoom_at(width / 2.0, height / 2.0, ZOOM_IN_FACTOR);
    }

    /// Toolbar zoom-out, anchored at the canvas centre.
    pub fn zoom_out(&mut self, width: f32, height: f32) {
        self.zoom_at(width / 2.0, height / 2.0, ZOOM_OUT_FACTOR);
    }

    /// Scale and offset that fit a `content_w` x `content_h` diagram inside
    /// the canvas with `margin` px around it, centred. Not used by the
    /// original UI (it only has reset); provided for embedders.
    pub fn fit(
        &mut self,
        canvas_w: f32,
        canvas_h: f32,
        content_w: f32,
        content_h: f32,
        margin: f32,
    ) {
        if content_w <= 0.0 || content_h <= 0.0 {
            return;
        }
        let avail_w = (canvas_w - 2.0 * margin).max(1.0);
        let avail_h = (canvas_h - 2.0 * margin).max(1.0);
        let scale = (avail_w / content_w)
            .min(avail_h / content_h)
            .clamp(MIN_SCALE, MAX_SCALE);
        self.scale = scale;
        self.x = (canvas_w - content_w * scale) / 2.0;
        self.y = (canvas_h - content_h * scale) / 2.0;
    }
}

/// Drag-to-pan state: pointerdown records the start, pointermove sets
/// `x = origX + dx`, pointerup ends it. Any button works; no inertia.
#[derive(Clone, Copy, Debug, Default)]
pub struct DragState(Option<Drag>);

impl DragState {
    pub fn begin(&mut self, pz: &PanZoom, px: f32, py: f32) {
        self.0 = Some(Drag {
            start_x: px,
            start_y: py,
            orig_x: pz.x,
            orig_y: pz.y,
        });
    }

    /// Returns true when the transform changed.
    pub fn moved(&self, pz: &mut PanZoom, px: f32, py: f32) -> bool {
        let Some(d) = self.0 else { return false };
        pz.x = d.orig_x + (px - d.start_x);
        pz.y = d.orig_y + (py - d.start_y);
        true
    }

    pub fn end(&mut self) {
        self.0 = None;
    }

    pub fn active(&self) -> bool {
        self.0.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-3
    }

    #[test]
    fn reset_is_origin_scale_one() {
        let mut p = PanZoom {
            x: 5.0,
            y: 9.0,
            scale: 3.0,
        };
        p.reset();
        assert_eq!(
            p,
            PanZoom {
                x: 0.0,
                y: 0.0,
                scale: 1.0
            }
        );
    }

    #[test]
    fn zoom_keeps_anchor_fixed() {
        let mut p = PanZoom {
            x: 30.0,
            y: -10.0,
            scale: 1.5,
        };
        let (cx, cy) = (120.0, 80.0);
        // content point under the cursor before and after
        let before = ((cx - p.x) / p.scale, (cy - p.y) / p.scale);
        p.zoom_at(cx, cy, 1.25);
        let after = ((cx - p.x) / p.scale, (cy - p.y) / p.scale);
        assert!(close(before.0, after.0) && close(before.1, after.1));
        assert!(close(p.scale, 1.875));
    }

    #[test]
    fn scale_is_clamped_and_anchor_uses_clamped_ratio() {
        let mut p = PanZoom::default();
        p.zoom_at(100.0, 100.0, 1000.0);
        assert_eq!(p.scale, MAX_SCALE);
        assert!(close(p.x, 100.0 - 100.0 * 8.0));
        p.zoom_at(0.0, 0.0, 1e-6);
        assert_eq!(p.scale, MIN_SCALE);
        // already at the limit: further zoom is a no-op
        let q = p;
        p.zoom_at(50.0, 50.0, 0.5);
        assert_eq!(p, q);
    }

    #[test]
    fn wheel_factor_is_exp_neg_delta_times_001() {
        let mut p = PanZoom::default();
        p.wheel(0.0, 0.0, 100.0);
        assert!(close(p.scale, (-0.1f32).exp()));
        let mut p = PanZoom::default();
        p.wheel(0.0, 0.0, -100.0);
        assert!(close(p.scale, 0.1f32.exp()));
    }

    #[test]
    fn buttons_zoom_about_centre() {
        let mut p = PanZoom::default();
        p.zoom_in(400.0, 300.0);
        assert!(close(p.scale, 1.25));
        assert!(close(p.x, 200.0 - 200.0 * 1.25));
        assert!(close(p.y, 150.0 - 150.0 * 1.25));
        p.zoom_out(400.0, 300.0);
        assert!(close(p.scale, 1.0));
        assert!(close(p.x, 0.0) && close(p.y, 0.0));
    }

    #[test]
    fn drag_pans_relative_to_start() {
        let mut p = PanZoom {
            x: 10.0,
            y: 20.0,
            scale: 2.0,
        };
        let mut d = DragState::default();
        assert!(!d.moved(&mut p, 5.0, 5.0));
        d.begin(&p, 100.0, 100.0);
        assert!(d.moved(&mut p, 130.0, 90.0));
        assert_eq!((p.x, p.y, p.scale), (40.0, 10.0, 2.0));
        d.end();
        assert!(!d.moved(&mut p, 0.0, 0.0));
    }

    #[test]
    fn fit_centres_content() {
        let mut p = PanZoom::default();
        p.fit(400.0, 300.0, 200.0, 100.0, 20.0);
        assert!(close(p.scale, 1.8));
        assert!(close(p.x, (400.0 - 200.0 * 1.8) / 2.0));
        assert!(close(p.y, (300.0 - 100.0 * 1.8) / 2.0));
    }

    #[test]
    fn pinch_scales_by_one_plus_delta() {
        let mut p = PanZoom::default();
        p.pinch(0.0, 0.0, 0.1);
        assert!(close(p.scale, 1.1));
    }
}
