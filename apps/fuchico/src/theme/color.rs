//! CSS color values → sRGB. GPUI has no oklch / color-mix, so the theme
//! tables (spec 5 §4.3) and user CSS are resolved here.
//!
//! Supported: `oklch()`, `oklab()`, `#rgb[a]`, `#rrggbb[aa]`, `rgb()/rgba()`
//! (comma or space syntax), `hsl()/hsla()`, `transparent`, `white`, `black`,
//! `color-mix(in oklch|oklab|srgb, A [p%], B [q%])` and `var(--name)` through a
//! resolver callback.

use gpui_kit::{Hsla, Rgba};

/// Non-premultiplied sRGB, channels in `0..=1`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Color {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Color {
    pub const TRANSPARENT: Color = Color {
        r: 0.,
        g: 0.,
        b: 0.,
        a: 0.,
    };

    pub fn rgba(r: f32, g: f32, b: f32, a: f32) -> Self {
        Color { r, g, b, a }
    }

    pub fn with_alpha(self, a: f32) -> Self {
        Color { a, ..self }
    }

    pub fn to_hsla(self) -> Hsla {
        Rgba {
            r: self.r,
            g: self.g,
            b: self.b,
            a: self.a,
        }
        .into()
    }

    /// `#rrggbb` / `#rrggbbaa` (for snapshots / debugging).
    pub fn to_hex(self) -> String {
        let c = |v: f32| (v.clamp(0., 1.) * 255.).round() as u8;
        if self.a >= 1.0 {
            format!("#{:02x}{:02x}{:02x}", c(self.r), c(self.g), c(self.b))
        } else {
            format!(
                "#{:02x}{:02x}{:02x}{:02x}",
                c(self.r),
                c(self.g),
                c(self.b),
                c(self.a)
            )
        }
    }

    pub fn from_oklch(l: f32, c: f32, h_deg: f32, a: f32) -> Self {
        let h = h_deg.to_radians();
        Self::from_oklab(l, c * h.cos(), c * h.sin(), a)
    }

    pub fn from_oklab(l: f32, a_: f32, b_: f32, alpha: f32) -> Self {
        let l_ = l + 0.396_337_78 * a_ + 0.215_803_76 * b_;
        let m_ = l - 0.105_561_346 * a_ - 0.063_854_17 * b_;
        let s_ = l - 0.089_484_18 * a_ - 1.291_485_5 * b_;
        let (l3, m3, s3) = (l_ * l_ * l_, m_ * m_ * m_, s_ * s_ * s_);
        let r = 4.076_741_7 * l3 - 3.307_711_6 * m3 + 0.230_969_94 * s3;
        let g = -1.268_438 * l3 + 2.609_757_4 * m3 - 0.341_319_38 * s3;
        let b = -0.004_196_086_3 * l3 - 0.703_418_6 * m3 + 1.707_614_7 * s3;
        Color {
            r: gamma(r).clamp(0., 1.),
            g: gamma(g).clamp(0., 1.),
            b: gamma(b).clamp(0., 1.),
            a: alpha.clamp(0., 1.),
        }
    }

    /// `(L, a, b)` in Oklab.
    pub fn to_oklab(self) -> (f32, f32, f32) {
        let (r, g, b) = (linear(self.r), linear(self.g), linear(self.b));
        let l = 0.412_221_47 * r + 0.536_332_55 * g + 0.051_445_995 * b;
        let m = 0.211_903_5 * r + 0.680_699_5 * g + 0.107_396_96 * b;
        let s = 0.088_302_46 * r + 0.281_718_84 * g + 0.629_978_7 * b;
        let (l_, m_, s_) = (l.cbrt(), m.cbrt(), s.cbrt());
        (
            0.210_454_26 * l_ + 0.793_617_8 * m_ - 0.004_072_047 * s_,
            1.977_998_5 * l_ - 2.428_592_2 * m_ + 0.450_593_7 * s_,
            0.025_904_037 * l_ + 0.782_771_77 * m_ - 0.808_675_77 * s_,
        )
    }

    /// `(L, C, H°)` in Oklch.
    pub fn to_oklch(self) -> (f32, f32, f32) {
        let (l, a, b) = self.to_oklab();
        let c = (a * a + b * b).sqrt();
        let mut h = b.atan2(a).to_degrees();
        if h < 0. {
            h += 360.;
        }
        (l, c, h)
    }
}

fn gamma(x: f32) -> f32 {
    if x >= 0.003_130_8 {
        1.055 * x.powf(1. / 2.4) - 0.055
    } else {
        12.92 * x
    }
}

fn linear(x: f32) -> f32 {
    if x >= 0.040_45 {
        ((x + 0.055) / 1.055).powf(2.4)
    } else {
        x / 12.92
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MixSpace {
    Oklch,
    Oklab,
    Srgb,
}

/// CSS `color-mix(in <space>, a pa, b pb)` with already-normalised weights
/// (`wa + wb == 1`) and the alpha multiplier for weights summing below 100%.
pub fn mix(space: MixSpace, a: Color, b: Color, wa: f32, alpha_mult: f32) -> Color {
    let wb = 1. - wa;
    // Premultiplied interpolation, as CSS Color 4 specifies.
    let alpha = a.a * wa + b.a * wb;
    let out = if alpha <= 0. {
        Color::TRANSPARENT
    } else {
        match space {
            MixSpace::Srgb => {
                let ch = |x: f32, y: f32| (x * a.a * wa + y * b.a * wb) / alpha;
                Color::rgba(ch(a.r, b.r), ch(a.g, b.g), ch(a.b, b.b), alpha)
            }
            MixSpace::Oklab => {
                let (la, aa, ba) = a.to_oklab();
                let (lb, ab, bb) = b.to_oklab();
                let ch = |x: f32, y: f32| (x * a.a * wa + y * b.a * wb) / alpha;
                Color::from_oklab(ch(la, lb), ch(aa, ab), ch(ba, bb), alpha)
            }
            MixSpace::Oklch => {
                let (la, ca, mut ha) = a.to_oklch();
                let (lb, cb, mut hb) = b.to_oklch();
                // Powerless hue for achromatic colors takes the other's hue.
                if ca < 1e-4 {
                    ha = hb;
                }
                if cb < 1e-4 {
                    hb = ha;
                }
                // Shorter hue arc.
                let mut dh = hb - ha;
                if dh > 180. {
                    dh -= 360.;
                } else if dh < -180. {
                    dh += 360.;
                }
                let ch = |x: f32, y: f32| (x * a.a * wa + y * b.a * wb) / alpha;
                Color::from_oklch(ch(la, lb), ch(ca, cb), ha + dh * wb, alpha)
            }
        }
    };
    out.with_alpha(out.a * alpha_mult)
}

/// Parses a CSS color. `var(--x)` references go through `resolve_var`
/// (which receives the name including the leading `--`).
pub fn parse_color(input: &str, resolve_var: &dyn Fn(&str) -> Option<Color>) -> Option<Color> {
    let s = input.trim();
    let lower = s.to_ascii_lowercase();
    match lower.as_str() {
        "transparent" => return Some(Color::TRANSPARENT),
        "white" => return Some(Color::rgba(1., 1., 1., 1.)),
        "black" => return Some(Color::rgba(0., 0., 0., 1.)),
        _ => {}
    }
    if let Some(hex) = s.strip_prefix('#') {
        return parse_hex(hex);
    }
    let (name, args) = split_fn(s)?;
    match name.to_ascii_lowercase().as_str() {
        "var" => {
            let parts = split_top(args, ',');
            let var = parts.first()?.trim();
            resolve_var(var).or_else(|| parts.get(1).and_then(|f| parse_color(f, resolve_var)))
        }
        "oklch" => {
            let (c, a) = channels(args)?;
            if c.len() != 3 {
                return None;
            }
            let l = number_or_pct(&c[0], 1.0)?;
            let ch = number_or_pct(&c[1], 0.4)?;
            let h = if c[2] == "none" { 0. } else { hue(&c[2])? };
            Some(Color::from_oklch(l, ch, h, a))
        }
        "oklab" => {
            let (c, a) = channels(args)?;
            if c.len() != 3 {
                return None;
            }
            Some(Color::from_oklab(
                number_or_pct(&c[0], 1.0)?,
                number_or_pct(&c[1], 0.4)?,
                number_or_pct(&c[2], 0.4)?,
                a,
            ))
        }
        "rgb" | "rgba" => {
            let (c, a) = channels(args)?;
            if c.len() != 3 {
                return None;
            }
            let ch = |v: &str| number_or_pct(v, 255.).map(|x| (x / 255.).clamp(0., 1.));
            Some(Color::rgba(ch(&c[0])?, ch(&c[1])?, ch(&c[2])?, a))
        }
        "hsl" | "hsla" => {
            let (c, a) = channels(args)?;
            if c.len() != 3 {
                return None;
            }
            let h = hue(&c[0])? / 360.;
            let sat = number_or_pct(&c[1], 100.)? / 100.;
            let l = number_or_pct(&c[2], 100.)? / 100.;
            let rgba: Rgba = Hsla {
                h: h.rem_euclid(1.),
                s: sat,
                l,
                a,
            }
            .into();
            Some(Color::rgba(rgba.r, rgba.g, rgba.b, rgba.a))
        }
        "color-mix" => parse_color_mix(args, resolve_var),
        _ => None,
    }
}

fn parse_color_mix(args: &str, resolve_var: &dyn Fn(&str) -> Option<Color>) -> Option<Color> {
    let parts = split_top(args, ',');
    if parts.len() != 3 {
        return None;
    }
    let space = parts[0].trim().to_ascii_lowercase();
    let space = match space.strip_prefix("in ")?.trim() {
        "oklch" => MixSpace::Oklch,
        "oklab" => MixSpace::Oklab,
        "srgb" | "srgb-linear" => MixSpace::Srgb,
        _ => return None,
    };
    let (ca, pa) = color_and_pct(parts[1], resolve_var)?;
    let (cb, pb) = color_and_pct(parts[2], resolve_var)?;
    let (pa, pb) = match (pa, pb) {
        (None, None) => (0.5, 0.5),
        (Some(a), None) => (a, 1. - a),
        (None, Some(b)) => (1. - b, b),
        (Some(a), Some(b)) => (a, b),
    };
    let sum = pa + pb;
    if sum <= 0. {
        return None;
    }
    let alpha_mult = if sum < 1. { sum } else { 1. };
    Some(mix(space, ca, cb, pa / sum, alpha_mult))
}

fn color_and_pct(
    part: &str,
    resolve_var: &dyn Fn(&str) -> Option<Color>,
) -> Option<(Color, Option<f32>)> {
    let tokens = split_top(part.trim(), ' ');
    let tokens: Vec<&str> = tokens
        .into_iter()
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .collect();
    let mut pct = None;
    let mut color_tokens = Vec::new();
    for t in tokens {
        if let Some(p) = t.strip_suffix('%')
            && let Ok(v) = p.parse::<f32>()
        {
            pct = Some(v / 100.);
            continue;
        }
        color_tokens.push(t);
    }
    let color = parse_color(&color_tokens.join(" "), resolve_var)?;
    Some((color, pct))
}

fn parse_hex(hex: &str) -> Option<Color> {
    if !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let digit = |i: usize, n: usize| -> Option<f32> {
        let v = u8::from_str_radix(&hex[i..i + n], 16).ok()?;
        Some(if n == 1 {
            (v * 17) as f32 / 255.
        } else {
            v as f32 / 255.
        })
    };
    match hex.len() {
        3 | 4 => Some(Color::rgba(
            digit(0, 1)?,
            digit(1, 1)?,
            digit(2, 1)?,
            if hex.len() == 4 { digit(3, 1)? } else { 1. },
        )),
        6 | 8 => Some(Color::rgba(
            digit(0, 2)?,
            digit(2, 2)?,
            digit(4, 2)?,
            if hex.len() == 8 { digit(6, 2)? } else { 1. },
        )),
        _ => None,
    }
}

/// `name(args)` → `(name, args)`.
fn split_fn(s: &str) -> Option<(&str, &str)> {
    let open = s.find('(')?;
    let inner = s[open + 1..].strip_suffix(')')?;
    Some((s[..open].trim(), inner))
}

/// Splits on `sep` outside parentheses.
fn split_top(s: &str, sep: char) -> Vec<&str> {
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut start = 0;
    for (i, c) in s.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => depth -= 1,
            c if c == sep && depth == 0 => {
                out.push(&s[start..i]);
                start = i + c.len_utf8();
            }
            _ => {}
        }
    }
    out.push(&s[start..]);
    out
}

/// Channels of `rgb(1 2 3 / 50%)` or `rgb(1, 2, 3, 0.5)`; returns alpha.
fn channels(args: &str) -> Option<(Vec<String>, f32)> {
    let (main, alpha) = match args.split_once('/') {
        Some((m, a)) => (m, Some(a.trim())),
        None => (args, None),
    };
    let mut parts: Vec<String> = main
        .split(|c: char| c == ',' || c.is_whitespace())
        .filter(|p| !p.is_empty())
        .map(|p| p.to_ascii_lowercase())
        .collect();
    let alpha = match alpha {
        Some(a) => number_or_pct(a, 1.0)?,
        None if parts.len() == 4 => {
            let a = parts.pop()?;
            number_or_pct(&a, 1.0)?
        }
        None => 1.0,
    };
    Some((parts, alpha.clamp(0., 1.)))
}

/// `50%` → `scale * 0.5`; plain numbers pass through.
fn number_or_pct(v: &str, scale: f32) -> Option<f32> {
    let v = v.trim();
    if v == "none" {
        return Some(0.);
    }
    match v.strip_suffix('%') {
        Some(p) => p.trim().parse::<f32>().ok().map(|x| x / 100. * scale),
        None => v.parse::<f32>().ok(),
    }
}

fn hue(v: &str) -> Option<f32> {
    let v = v.trim();
    if let Some(d) = v.strip_suffix("deg") {
        return d.parse().ok();
    }
    if let Some(r) = v.strip_suffix("rad") {
        return r.parse::<f32>().ok().map(f32::to_degrees);
    }
    if let Some(t) = v.strip_suffix("turn") {
        return t.parse::<f32>().ok().map(|x| x * 360.);
    }
    v.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> Color {
        parse_color(s, &|_| None).unwrap_or_else(|| panic!("failed to parse {s}"))
    }

    #[test]
    fn hex_and_rgb() {
        assert_eq!(p("#282a36").to_hex(), "#282a36");
        assert_eq!(p("#fff").to_hex(), "#ffffff");
        assert_eq!(p("rgb(255 255 255 / 10%)").to_hex(), "#ffffff1a");
        assert_eq!(p("rgba(76, 79, 105, 0.15)").to_hex(), "#4c4f6926");
        assert_eq!(p("rgb(76 79 105 / 20%)").to_hex(), "#4c4f6933");
        assert_eq!(p("hsl(0 100% 50%)").to_hex(), "#ff0000");
    }

    #[test]
    fn oklch_matches_reference_values() {
        assert_eq!(p("oklch(1 0 0)").to_hex(), "#ffffff");
        // Reference: OKLab -> linear sRGB (Ottosson matrices) -> sRGB gamma.
        assert_eq!(p("oklch(0.148 0.004 228.8)").to_hex(), "#090b0c");
        assert_eq!(p("oklch(0.577 0.245 27.325)").to_hex(), "#e7000b");
        assert_eq!(p("oklch(1 0 0 / 10%)").to_hex(), "#ffffff1a");
        assert_eq!(p("oklch(50% 0 0)").to_hex(), p("oklch(0.5 0 0)").to_hex());
    }

    #[test]
    fn oklch_round_trip() {
        let c = p("#bd93f9");
        let (l, ch, h) = c.to_oklch();
        assert_eq!(Color::from_oklch(l, ch, h, 1.).to_hex(), "#bd93f9");
    }

    #[test]
    fn color_mix_transparent_sets_alpha() {
        let c = p("color-mix(in oklch, #ff0000 30%, transparent)");
        assert_eq!(c.to_hex(), "#ff00004d");
    }

    #[test]
    fn color_mix_two_colors() {
        assert_eq!(
            p("color-mix(in oklch, white 50%, white)").to_hex(),
            "#ffffff"
        );
        let mid = p("color-mix(in srgb, black, white)");
        assert_eq!(mid.to_hex(), "#808080");
        // Default weights mix halfway in oklch lightness.
        let (l, _, _) = p("color-mix(in oklch, black, white)").to_oklch();
        assert!((l - 0.5).abs() < 0.01);
    }

    #[test]
    fn var_references() {
        let resolver = |name: &str| (name == "--primary").then(|| Color::rgba(1., 0., 0., 1.));
        let c = parse_color(
            "color-mix(in oklch, var(--primary) 50%, transparent)",
            &resolver,
        )
        .unwrap();
        assert_eq!(c.to_hex(), "#ff000080");
        assert_eq!(
            parse_color("var(--nope, #000)", &resolver)
                .unwrap()
                .to_hex(),
            "#000000"
        );
        assert!(parse_color("var(--nope)", &resolver).is_none());
    }

    #[test]
    fn rejects_garbage() {
        for s in [
            "",
            "#12",
            "#ggg",
            "oklch(1 0)",
            "rgb(1,2)",
            "color-mix(in hsl, red, blue)",
            "foo(1)",
        ] {
            assert!(parse_color(s, &|_| None).is_none(), "{s} should not parse");
        }
    }
}
