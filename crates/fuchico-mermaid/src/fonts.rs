//! The font database shared by text measurement (layout) and rasterization,
//! so labels are laid out with the same faces they are painted in.
//!
//! The original lays out with a canvas `measureText` in `--font-sans` /
//! `--font-mono`; here [`crate::measure`] shapes with these faces and resvg
//! paints with them.

use resvg::usvg::fontdb::{self, Database, Family};
use std::sync::{Arc, Mutex, RwLock};

static EXTRA: Mutex<Vec<&'static [u8]>> = Mutex::new(Vec::new());
static DB: RwLock<Option<Arc<Database>>> = RwLock::new(None);

/// Registers bundled fonts (e.g. the app's Inter / JetBrains Mono) so
/// diagrams use them even when they are not installed system-wide.
/// Re-registering the same font is a no-op.
pub fn add_fonts(fonts: &[&'static [u8]]) {
    let mut extra = EXTRA.lock().unwrap_or_else(|e| e.into_inner());
    let mut changed = false;
    for f in fonts {
        if !extra.iter().any(|e| std::ptr::eq(*e, *f)) {
            extra.push(f);
            changed = true;
        }
    }
    if changed {
        *DB.write().unwrap_or_else(|e| e.into_inner()) = None;
        crate::measure::invalidate();
    }
}

fn build() -> Database {
    let mut db = Database::new();
    db.load_system_fonts();
    for f in EXTRA.lock().unwrap_or_else(|e| e.into_inner()).iter() {
        db.load_font_source(fontdb::Source::Binary(Arc::new(*f)));
    }
    let has = |db: &Database, name: &str| {
        db.faces()
            .any(|f| f.families.iter().any(|(n, _)| n == name))
    };
    let sans = [
        "Inter",
        "Inter Variable",
        "Helvetica Neue",
        "Helvetica",
        "Arial",
    ]
    .into_iter()
    .find(|n| has(&db, n))
    .unwrap_or("Helvetica");
    let mono = ["JetBrains Mono", "SF Mono", "Menlo", "Courier New"]
        .into_iter()
        .find(|n| has(&db, n))
        .unwrap_or("Menlo");
    db.set_sans_serif_family(sans);
    db.set_monospace_family(mono);
    db.set_serif_family("Times New Roman");
    db
}

/// The shared database (built on first use, rebuilt after [`add_fonts`]).
pub fn fontdb() -> Arc<Database> {
    if let Some(db) = DB.read().unwrap_or_else(|e| e.into_inner()).as_ref() {
        return db.clone();
    }
    let mut w = DB.write().unwrap_or_else(|e| e.into_inner());
    w.get_or_insert_with(|| Arc::new(build())).clone()
}

/// The default sans-serif family name the database resolved to.
pub fn default_sans() -> String {
    let db = fontdb();
    db.family_name(&Family::SansSerif).to_string()
}

/// Splits a CSS `font-family` list into names (quotes stripped).
pub fn split_families(css: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    for ch in css.chars() {
        match (quote, ch) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), c) => cur.push(c),
            (None, '"' | '\'') => quote = Some(ch),
            (None, ',') => {
                let t = cur.trim();
                if !t.is_empty() {
                    out.push(t.to_string());
                }
                cur.clear();
            }
            (None, c) => cur.push(c),
        }
    }
    let t = cur.trim();
    if !t.is_empty() {
        out.push(t.to_string());
    }
    out
}

/// A CSS family name as a fontdb family (generic keywords and the
/// browser-only system aliases map to the generic families).
pub fn family_of(name: &str) -> Family<'_> {
    match name.to_ascii_lowercase().as_str() {
        "sans-serif" | "-apple-system" | "blinkmacsystemfont" | "system-ui" | "ui-sans-serif" => {
            Family::SansSerif
        }
        "monospace" | "ui-monospace" | "sfmono-regular" => Family::Monospace,
        "serif" | "ui-serif" => Family::Serif,
        "cursive" => Family::Cursive,
        "fantasy" => Family::Fantasy,
        _ => Family::Name(name),
    }
}

/// A family list safe to embed in a double-quoted SVG attribute: names
/// with spaces are single-quoted, generic keywords left bare.
pub fn css_family_list(css: &str) -> String {
    split_families(css)
        .iter()
        .map(|n| {
            let n = n.replace(['\'', '"', '<', '>', '&'], "");
            if matches!(family_of(&n), Family::Name(_)) && n.contains(' ') {
                format!("'{n}'")
            } else {
                n
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_and_quotes_family_lists() {
        let css = "Inter, \"Inter Variable\", -apple-system, 'Segoe UI', sans-serif";
        assert_eq!(
            split_families(css),
            [
                "Inter",
                "Inter Variable",
                "-apple-system",
                "Segoe UI",
                "sans-serif"
            ]
        );
        assert_eq!(
            css_family_list(css),
            "Inter, 'Inter Variable', -apple-system, 'Segoe UI', sans-serif"
        );
        assert!(matches!(family_of("BlinkMacSystemFont"), Family::SansSerif));
        assert!(matches!(family_of("ui-monospace"), Family::Monospace));
    }
}
