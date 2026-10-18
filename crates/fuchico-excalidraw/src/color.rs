//! CSS color parsing (the subset Excalidraw writes) and Excalidraw's dark-mode
//! canvas filter (`invert(93%) hue-rotate(180deg)`).

/// Linear 0..=1 RGBA.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rgba {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Rgba {
    pub const TRANSPARENT: Rgba = Rgba { r: 0.0, g: 0.0, b: 0.0, a: 0.0 };

    pub fn new(r: f32, g: f32, b: f32, a: f32) -> Self {
        Self { r, g, b, a }
    }

    pub fn with_alpha(self, a: f32) -> Self {
        Self { a: self.a * a, ..self }
    }

    pub fn to_hsla(self) -> gpui_kit::Hsla {
        gpui_kit::Rgba { r: self.r, g: self.g, b: self.b, a: self.a }.into()
    }

    /// `#rrggbb` (alpha dropped).
    pub fn to_hex(self) -> String {
        let c = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        format!("#{:02x}{:02x}{:02x}", c(self.r), c(self.g), c(self.b))
    }

    /// The colour as Excalidraw's dark theme shows it: the whole canvas is
    /// drawn through `filter: invert(93%) hue-rotate(180deg)`.
    pub fn dark_filter(self) -> Self {
        let inv = |c: f32| 0.93 - 0.86 * c;
        let (r, g, b) = (inv(self.r), inv(self.g), inv(self.b));
        // hue-rotate(180deg) colour matrix (cos = -1, sin = 0).
        let r2 = -0.574 * r + 1.430 * g + 0.144 * b;
        let g2 = 0.426 * r + 0.430 * g + 0.144 * b;
        let b2 = 0.426 * r + 1.430 * g - 0.856 * b;
        Self {
            r: r2.clamp(0.0, 1.0),
            g: g2.clamp(0.0, 1.0),
            b: b2.clamp(0.0, 1.0),
            a: self.a,
        }
    }
}

pub fn is_transparent(s: &str) -> bool {
    let t = s.trim();
    t.is_empty()
        || t.eq_ignore_ascii_case("transparent")
        || parse(t).map(|c| c.a == 0.0).unwrap_or(false)
}

/// Parses `#rgb`, `#rgba`, `#rrggbb`, `#rrggbbaa`, `rgb()/rgba()`,
/// `transparent` and the common named colours.
pub fn parse(s: &str) -> Option<Rgba> {
    let t = s.trim().to_ascii_lowercase();
    if t == "transparent" {
        return Some(Rgba::TRANSPARENT);
    }
    if let Some(hex) = t.strip_prefix('#') {
        let h = |i: usize, n: usize| u8::from_str_radix(&hex[i..i + n], 16).ok();
        let (r, g, b, a) = match hex.len() {
            3 | 4 => {
                let d = |i| h(i, 1).map(|v| v * 17);
                (d(0)?, d(1)?, d(2)?, if hex.len() == 4 { d(3)? } else { 255 })
            }
            6 | 8 => (h(0, 2)?, h(2, 2)?, h(4, 2)?, if hex.len() == 8 { h(6, 2)? } else { 255 }),
            _ => return None,
        };
        return Some(Rgba::new(
            r as f32 / 255.0,
            g as f32 / 255.0,
            b as f32 / 255.0,
            a as f32 / 255.0,
        ));
    }
    if let Some(body) = t
        .strip_prefix("rgba(")
        .or_else(|| t.strip_prefix("rgb("))
        .and_then(|b| b.strip_suffix(')'))
    {
        let parts: Vec<f32> = body
            .split([',', ' ', '/'])
            .filter(|p| !p.is_empty())
            .filter_map(|p| {
                if let Some(pc) = p.strip_suffix('%') {
                    pc.parse::<f32>().ok().map(|v| v / 100.0 * 255.0)
                } else {
                    p.parse::<f32>().ok()
                }
            })
            .collect();
        if parts.len() < 3 {
            return None;
        }
        let a = parts.get(3).copied().unwrap_or(1.0);
        let a = if a > 1.0 { a / 255.0 } else { a };
        return Some(Rgba::new(parts[0] / 255.0, parts[1] / 255.0, parts[2] / 255.0, a));
    }
    let named = match t.as_str() {
        "black" => "#000000",
        "white" => "#ffffff",
        "red" => "#ff0000",
        "green" => "#008000",
        "blue" => "#0000ff",
        "yellow" => "#ffff00",
        "orange" => "#ffa500",
        "purple" => "#800080",
        "gray" | "grey" => "#808080",
        "pink" => "#ffc0cb",
        "cyan" => "#00ffff",
        "magenta" => "#ff00ff",
        "brown" => "#a52a2a",
        _ => return None,
    };
    parse(named)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses() {
        assert_eq!(parse("#fff").unwrap(), Rgba::new(1.0, 1.0, 1.0, 1.0));
        assert_eq!(parse("transparent").unwrap().a, 0.0);
        assert!(parse("#1e1e1e").is_some());
        assert!(parse("rgba(0, 0, 0, 0.5)").unwrap().a == 0.5);
        assert!(is_transparent("transparent"));
        assert!(!is_transparent("#ffc9c9"));
    }

    #[test]
    fn dark_filter_matches_excalidraw_background() {
        // White canvas → #121212 under invert(93%) hue-rotate(180deg).
        assert_eq!(parse("#ffffff").unwrap().dark_filter().to_hex(), "#121212");
    }
}
