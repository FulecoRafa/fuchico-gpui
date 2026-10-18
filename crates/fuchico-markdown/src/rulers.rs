//! Vertical rulers (`rulers.ts`): settings text <-> columns, and column ->
//! x position.

/// JavaScript `Number(s)` for a whitespace-free token, as `f64`
/// (`NaN` for anything JS would reject).
fn js_number(s: &str) -> f64 {
    if s.is_empty() {
        return 0.0;
    }
    let lower = s.to_ascii_lowercase();
    for (prefix, radix) in [("0x", 16), ("0o", 8), ("0b", 2)] {
        if let Some(digits) = lower.strip_prefix(prefix) {
            if digits.is_empty() || !digits.chars().all(|c| c.is_digit(radix)) {
                return f64::NAN;
            }
            return digits.chars().fold(0.0, |acc, c| acc * radix as f64 + c.to_digit(radix).unwrap() as f64);
        }
    }
    let body = s.strip_prefix(['+', '-']).unwrap_or(s);
    if body == "Infinity" {
        return if s.starts_with('-') { f64::NEG_INFINITY } else { f64::INFINITY };
    }
    // StrDecimalLiteral: digits [. digits] [e[+-]digits], at least one digit
    // in the mantissa.
    let b = body.as_bytes();
    let mut i = 0;
    let int_digits = b.iter().take_while(|c| c.is_ascii_digit()).count();
    i += int_digits;
    let mut frac_digits = 0;
    if b.get(i) == Some(&b'.') {
        i += 1;
        frac_digits = b[i..].iter().take_while(|c| c.is_ascii_digit()).count();
        i += frac_digits;
    }
    if int_digits + frac_digits == 0 {
        return f64::NAN;
    }
    if matches!(b.get(i), Some(b'e' | b'E')) {
        let mut j = i + 1;
        if matches!(b.get(j), Some(b'+' | b'-')) {
            j += 1;
        }
        let exp_digits = b[j.min(b.len())..].iter().take_while(|c| c.is_ascii_digit()).count();
        if exp_digits == 0 {
            return f64::NAN;
        }
        i = j + exp_digits;
    }
    if i != b.len() {
        return f64::NAN;
    }
    s.parse::<f64>().unwrap_or_else(|_| {
        // Rust rejects "5." / ".5e1"-style forms in some versions; patch up.
        let fixed = s.replace(".e", ".0e").replace(".E", ".0E");
        let fixed = if fixed.ends_with('.') { format!("{fixed}0") } else { fixed };
        fixed.parse().unwrap_or(f64::NAN)
    })
}

/// `parseRulers`: unique ascending columns from text like `"80, 120"`.
/// Anything that is not an integer in `1..=1000` (by JS `Number`) is dropped.
pub fn parse_rulers(text: &str) -> Vec<u32> {
    let mut cols: Vec<u32> = Vec::new();
    for part in text.split(|c: char| c.is_whitespace() || c == ',' || c == ';') {
        if part.is_empty() {
            continue;
        }
        let n = js_number(part);
        if n.is_finite() && n.fract() == 0.0 && n > 0.0 && n <= 1000.0 {
            let n = n as u32;
            if !cols.contains(&n) {
                cols.push(n);
            }
        }
    }
    cols.sort_unstable();
    cols
}

/// `formatRulers`.
pub fn format_rulers(columns: &[u32]) -> String {
    columns.iter().map(|c| c.to_string()).collect::<Vec<_>>().join(", ")
}

/// X offset of ruler `column`: `content_left + padding + column * char_width`.
pub fn column_x(content_left: f32, column: u32, char_width: f32) -> f32 {
    content_left + column as f32 * char_width
}

/// All ruler x offsets for `columns`.
pub fn ruler_positions(columns: &[u32], content_left: f32, char_width: f32) -> Vec<f32> {
    columns.iter().map(|c| column_x(content_left, *c, char_width)).collect()
}
