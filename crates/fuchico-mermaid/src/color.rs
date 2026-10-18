//! Minimal CSS colour parsing (hex, rgb(), oklch()) so the app's theme tokens,
//! which are written as `oklch(...)` in `globals.css`, can be handed to
//! renderers that only understand sRGB.

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rgba {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: f32,
}

impl Rgba {
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 1.0 }
    }

    /// `#rrggbb` (alpha dropped) or `rgba(r,g,b,a)` when translucent.
    pub fn css(&self) -> String {
        if self.a >= 0.999 {
            self.hex()
        } else {
            format!("rgba({},{},{},{:.3})", self.r, self.g, self.b, self.a)
        }
    }

    pub fn hex(&self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
    }

    /// Source-over mix of `self` (with its alpha scaled by `t`) on `base`.
    pub fn mix_over(&self, base: Rgba, t: f32) -> Rgba {
        let a = (self.a * t).clamp(0.0, 1.0);
        let m = |f: u8, b: u8| (f as f32 * a + b as f32 * (1.0 - a)).round() as u8;
        Rgba::rgb(m(self.r, base.r), m(self.g, base.g), m(self.b, base.b))
    }
}

fn gamma(c: f32) -> f32 {
    let c = c.clamp(0.0, 1.0);
    if c <= 0.003_130_8 {
        12.92 * c
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

/// OKLCH -> sRGB (gamut clipped by channel clamping, like browsers' fallback).
pub fn oklch_to_rgb(l: f32, c: f32, h_deg: f32) -> Rgba {
    let h = h_deg.to_radians();
    let (a, b) = (c * h.cos(), c * h.sin());
    let l_ = l + 0.396_337_78 * a + 0.215_803_76 * b;
    let m_ = l - 0.105_561_346 * a - 0.063_854_17 * b;
    let s_ = l - 0.089_484_18 * a - 1.291_485_5 * b;
    let (l3, m3, s3) = (l_ * l_ * l_, m_ * m_ * m_, s_ * s_ * s_);
    let r = 4.076_741_7 * l3 - 3.307_711_6 * m3 + 0.230_969_94 * s3;
    let g = -1.268_438 * l3 + 2.609_757_4 * m3 - 0.341_319_4 * s3;
    let bl = -0.004_196_086_3 * l3 - 0.703_418_6 * m3 + 1.707_614_7 * s3;
    let q = |v: f32| (gamma(v) * 255.0).round() as u8;
    Rgba::rgb(q(r), q(g), q(bl))
}

fn num(s: &str) -> Option<f32> {
    let s = s.trim();
    if let Some(p) = s.strip_suffix('%') {
        return p.trim().parse::<f32>().ok().map(|v| v / 100.0);
    }
    s.trim_end_matches("deg").trim().parse().ok()
}

pub fn parse(input: &str) -> Option<Rgba> {
    let s = input.trim();
    if let Some(hex) = s.strip_prefix('#') {
        let d = |i: usize| u8::from_str_radix(hex.get(i..i + 1)?, 16).ok().map(|v| v * 17);
        let p = |i: usize| u8::from_str_radix(hex.get(i..i + 2)?, 16).ok();
        return match hex.len() {
            3 => Some(Rgba::rgb(d(0)?, d(1)?, d(2)?)),
            6 => Some(Rgba::rgb(p(0)?, p(2)?, p(4)?)),
            8 => Some(Rgba { a: p(6)? as f32 / 255.0, ..Rgba::rgb(p(0)?, p(2)?, p(4)?) }),
            _ => None,
        };
    }
    let lower = s.to_ascii_lowercase();
    let (name, rest) = lower.split_once('(')?;
    let body = rest.strip_suffix(')')?;
    let (main, alpha) = match body.split_once('/') {
        Some((m, a)) => (m, num(a)),
        None => (body, None),
    };
    let parts: Vec<&str> = main
        .split(|c: char| c == ',' || c.is_whitespace())
        .filter(|p| !p.is_empty())
        .collect();
    let mut out = match name.trim() {
        "oklch" if parts.len() >= 3 => oklch_to_rgb(num(parts[0])?, num(parts[1])?, num(parts[2])?),
        "rgb" | "rgba" if parts.len() >= 3 => {
            let ch = |p: &str| -> Option<u8> {
                if p.ends_with('%') {
                    Some((num(p)? * 255.0).round().clamp(0.0, 255.0) as u8)
                } else {
                    Some(p.parse::<f32>().ok()?.round().clamp(0.0, 255.0) as u8)
                }
            };
            let mut c = Rgba::rgb(ch(parts[0])?, ch(parts[1])?, ch(parts[2])?);
            if let Some(a) = parts.get(3) {
                c.a = num(a)?;
            }
            c
        }
        _ => return None,
    };
    if let Some(a) = alpha {
        out.a = a;
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_forms() {
        assert_eq!(parse("#fff"), Some(Rgba::rgb(255, 255, 255)));
        assert_eq!(parse("#282a36"), Some(Rgba::rgb(0x28, 0x2a, 0x36)));
    }

    #[test]
    fn oklch_white_and_black() {
        assert_eq!(parse("oklch(1 0 0)").unwrap().hex(), "#ffffff");
        assert_eq!(parse("oklch(0 0 0)").unwrap().hex(), "#000000");
    }

    #[test]
    fn oklch_known_value() {
        // oklch(0.577 0.245 27.325) is Tailwind's red-600, ~#e7000b.
        let c = parse("oklch(0.577 0.245 27.325)").unwrap();
        assert!(c.r > 215 && c.g < 30 && c.b < 40, "{c:?}");
    }

    #[test]
    fn rgb_alpha() {
        let c = parse("rgba(10, 20, 30, 0.5)").unwrap();
        assert_eq!((c.r, c.g, c.b), (10, 20, 30));
        assert!((c.a - 0.5).abs() < 1e-6);
    }
}
