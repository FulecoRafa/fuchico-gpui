//! Bakes pele's CSS custom properties into a self-contained SVG.
//!
//! pele paints from `var(--pele-*)` (the original maps those onto the app
//! tokens in `globals.css`) and declares private aliases on the root
//! element (`--_s:var(--pele-surface,#f3f3f3)`). resvg and Typst do not
//! implement custom properties, so every `var(--name[, fallback])` is
//! replaced by its resolved value, the way the browser's cascade would.

use std::collections::HashMap;

/// Finds `--name:value` declarations in `style="..."` attributes.
fn collect_declarations(svg: &str, out: &mut HashMap<String, String>) {
    let mut rest = svg;
    while let Some(i) = rest.find("style=\"") {
        let body = &rest[i + 7..];
        let Some(end) = body.find('"') else { break };
        for decl in split_top_level(&body[..end], ';') {
            if let Some((k, v)) = decl.split_once(':') {
                let k = k.trim();
                if k.starts_with("--") {
                    out.entry(k.to_string())
                        .or_insert_with(|| v.trim().to_string());
                }
            }
        }
        rest = &body[end..];
    }
}

fn split_top_level(s: &str, sep: char) -> Vec<&str> {
    let mut out = Vec::new();
    let (mut depth, mut start) = (0i32, 0usize);
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

/// Replaces every `var(...)` in `value`. `depth` guards against cycles.
fn substitute(value: &str, vars: &HashMap<String, String>, depth: u32) -> String {
    if depth > 16 || !value.contains("var(") {
        return value.to_string();
    }
    let mut out = String::with_capacity(value.len());
    let mut rest = value;
    while let Some(i) = rest.find("var(") {
        out.push_str(&rest[..i]);
        let inner_start = i + 4;
        // Find the matching ')'.
        let mut d = 1i32;
        let mut end = None;
        for (j, c) in rest[inner_start..].char_indices() {
            match c {
                '(' => d += 1,
                ')' => {
                    d -= 1;
                    if d == 0 {
                        end = Some(inner_start + j);
                        break;
                    }
                }
                _ => {}
            }
        }
        let Some(end) = end else {
            out.push_str(&rest[i..]);
            return out;
        };
        let inner = &rest[inner_start..end];
        let (name, fallback) = match inner.find(',') {
            Some(c) => (inner[..c].trim(), Some(inner[c + 1..].trim())),
            None => (inner.trim(), None),
        };
        let resolved = match vars.get(name) {
            Some(v) => substitute(v, vars, depth + 1),
            None => fallback
                .map(|f| substitute(f, vars, depth + 1))
                .unwrap_or_default(),
        };
        out.push_str(&resolved);
        rest = &rest[end + 1..];
    }
    out.push_str(rest);
    out
}

/// Removes custom-property declarations from `style` attributes and moves
/// `rx`/`ry` (CSS geometry properties resvg ignores in `style`) into
/// attributes.
fn tidy_styles(svg: &str) -> String {
    let mut out = String::with_capacity(svg.len());
    let mut rest = svg;
    while let Some(i) = rest.find("style=\"") {
        out.push_str(&rest[..i]);
        let body = &rest[i + 7..];
        let Some(end) = body.find('"') else {
            out.push_str(&rest[i..]);
            return out;
        };
        let mut kept = Vec::new();
        let mut attrs = Vec::new();
        for decl in split_top_level(&body[..end], ';') {
            let d = decl.trim();
            if d.is_empty() || d.starts_with("--") {
                continue;
            }
            match d.split_once(':') {
                Some((k, v)) if matches!(k.trim(), "rx" | "ry") => {
                    attrs.push(format!("{}=\"{}\"", k.trim(), v.trim()));
                }
                _ => kept.push(d),
            }
        }
        if !kept.is_empty() {
            out.push_str("style=\"");
            out.push_str(&kept.join(";"));
            out.push('"');
            if !attrs.is_empty() {
                out.push(' ');
            }
        }
        out.push_str(&attrs.join(" "));
        rest = &body[end + 1..];
    }
    out.push_str(rest);
    out
}

/// Resolves all custom properties in `svg` against `vars` (the `--pele-*`
/// table) and the SVG's own declarations.
pub fn resolve(svg: &str, vars: &[(&str, String)]) -> String {
    let mut table: HashMap<String, String> = vars
        .iter()
        .map(|(k, v)| (k.to_string(), v.clone()))
        .collect();
    let mut own = HashMap::new();
    collect_declarations(svg, &mut own);
    for (k, v) in own {
        table.entry(k).or_insert(v);
    }
    tidy_styles(&substitute(svg, &table, 0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_aliases_fallbacks_and_rx() {
        let svg = r##"<svg style="--_s:var(--pele-surface,#f3f3f3);--_r:var(--pele-radius,4px);color:var(--_fg)"><rect fill="var(--_s)" style="rx:var(--_r);fill:#f96;"/><text fill="var(--pele-series-9, var(--_s))"/></svg>"##;
        let out = resolve(
            svg,
            &[
                ("--pele-surface", "#112233".into()),
                ("--pele-radius", "6px".into()),
            ],
        );
        assert!(!out.contains("var("), "{out}");
        assert!(
            out.contains(r##"<rect fill="#112233" style="fill:#f96" rx="6px"/>"##),
            "{out}"
        );
        assert!(out.contains(r##"<text fill="#112233"/>"##), "{out}");
        // Undefined, no fallback: `color:` is dropped to an empty value.
        assert!(out.starts_with(r#"<svg style="color:""#), "{out}");
    }
}
