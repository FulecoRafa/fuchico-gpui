//! Port of the pure helpers in `src/lib/editorWindow.ts` (single-file editor
//! windows, issue #29).

pub const EDITOR_WINDOW_PREFIX: &str = "editor-";

fn to_base36(mut n: u32) -> String {
    if n == 0 {
        return "0".into();
    }
    let mut digits = Vec::new();
    while n > 0 {
        digits.push(std::char::from_digit(n % 36, 36).unwrap());
        n /= 36;
    }
    digits.iter().rev().collect()
}

/// Stable, capability-safe window label derived from the path (31-multiplier
/// hash over UTF-16 code units, base 36).
pub fn editor_window_label(path: &str) -> String {
    let mut h: u32 = 0;
    for u in path.encode_utf16() {
        h = h.wrapping_mul(31).wrapping_add(u as u32);
    }
    format!("{EDITOR_WINDOW_PREFIX}{}", to_base36(h))
}

/// `application/x-www-form-urlencoded` component (JS `URLSearchParams`).
fn form_encode(s: &str) -> String {
    let mut out = String::new();
    for &b in s.as_bytes() {
        match b {
            b'*' | b'-' | b'.' | b'_' => out.push(b as char),
            b' ' => out.push('+'),
            b if b.is_ascii_alphanumeric() => out.push(b as char),
            b => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

pub fn editor_window_url(path: &str, root_path: Option<&str>) -> String {
    let mut q = format!("window=editor&path={}", form_encode(path));
    if let Some(root) = root_path {
        q.push_str(&format!("&root={}", form_encode(root)));
    }
    format!("index.html?{q}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derives_a_stable_capability_safe_label() {
        let a = editor_window_label("/vault/notes/Café ünïcode.md");
        assert_eq!(a, editor_window_label("/vault/notes/Café ünïcode.md"));
        assert!(a.starts_with("editor-") && a[7..].chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit()) && a.len() > 7);
        assert_ne!(a, editor_window_label("/vault/notes/other.md"));
    }

    #[test]
    fn label_matches_js_hash() {
        // JS: h = 0; for c of "a": h = (h*31 + 97) >>> 0  => 97 => "2p" in base 36.
        assert_eq!(editor_window_label("a"), "editor-2p");
    }

    #[test]
    fn builds_the_route_with_path_and_root() {
        assert_eq!(editor_window_url("/v/a b.md", Some("/v")), "index.html?window=editor&path=%2Fv%2Fa+b.md&root=%2Fv");
        assert_eq!(editor_window_url("/v/a.md", None), "index.html?window=editor&path=%2Fv%2Fa.md");
    }
}
