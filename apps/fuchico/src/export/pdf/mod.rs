//! Native PDF export: markdown -> Typst markup ([`markup`]) -> PDF
//! (typst + typst-pdf), entirely in-process — no webview, fonts embedded.
//!
//! [`render_pdf`] is pure and blocking; callers run it off the UI thread.

pub mod markup;
pub mod world;

use std::path::Path;

use fuchico_markdown::style::ColorToken;
use fuchico_mermaid::MermaidTheme;
use typst_layout::PagedDocument;

pub use markup::DiskImages;

/// Everything the PDF needs from the app theme, as plain (Send) data.
#[derive(Debug, Clone)]
pub struct PdfStyle {
    pub title: String,
    /// Typst paper name (`"a4"` / `"us-letter"`).
    pub paper: &'static str,
    /// CSS variable name (`--background`) -> `#rrggbb[aa]`.
    pub colors: Vec<(String, String)>,
    pub font_sans: String,
    pub font_mono: String,
    pub mermaid: MermaidTheme,
}

impl PdfStyle {
    /// A Typst colour expression for a CSS variable (black when missing).
    pub fn color(&self, css_name: &str) -> String {
        let hex = self
            .colors
            .iter()
            .find(|(n, _)| n == css_name)
            .map(|(_, v)| v.as_str())
            .unwrap_or("#000000");
        format!("rgb(\"{}\")", markup::esc(hex))
    }

    pub fn token_color(&self, t: ColorToken) -> String {
        self.color(match t {
            ColorToken::Foreground => "--foreground",
            ColorToken::Muted => "--muted-foreground",
            ColorToken::Destructive => "--destructive",
            ColorToken::Primary => "--primary",
            ColorToken::SyntaxComment => "--syntax-comment",
            ColorToken::SyntaxString => "--syntax-string",
            ColorToken::SyntaxNumber => "--syntax-number",
            ColorToken::SyntaxKeyword => "--syntax-keyword",
            ColorToken::SyntaxFunction => "--syntax-function",
            ColorToken::SyntaxType => "--syntax-type",
        })
    }

    /// Paper size for a locale: US Letter for the Americas' letter
    /// countries, A4 elsewhere.
    pub fn paper_for_locale(locale: &str) -> &'static str {
        let region = locale
            .split(['-', '_'])
            .nth(1)
            .unwrap_or("")
            .to_ascii_uppercase();
        if matches!(
            region.as_str(),
            "US" | "CA" | "MX" | "PH" | "CL" | "CO" | "VE"
        ) {
            "us-letter"
        } else {
            "a4"
        }
    }

    #[cfg(test)]
    pub fn test() -> Self {
        let colors = [
            ("--background", "#ffffff"),
            ("--foreground", "#1a1a1a"),
            ("--muted", "#f0f0f0"),
            ("--muted-foreground", "#777777"),
            ("--border", "#dddddd"),
            ("--primary", "#333333"),
            ("--primary-foreground", "#ffffff"),
            ("--destructive", "#cc0000"),
            ("--syntax-keyword", "#fa8d3e"),
            ("--syntax-string", "#86b300"),
            ("--syntax-number", "#a37acc"),
            ("--syntax-function", "#f2ae49"),
            ("--syntax-type", "#399ee6"),
            ("--syntax-comment", "#787b80"),
        ];
        PdfStyle {
            title: "Test".into(),
            paper: "a4",
            colors: colors
                .iter()
                .map(|(a, b)| (a.to_string(), b.to_string()))
                .collect(),
            font_sans: "Inter".into(),
            font_mono: "JetBrains Mono".into(),
            mermaid: MermaidTheme::light(),
        }
    }
}

/// Renders a note to PDF bytes. `note_dir` resolves relative image paths.
pub fn render_pdf(markdown: &str, style: &PdfStyle, note_dir: &Path) -> Result<Vec<u8>, String> {
    let doc = markup::to_typst(
        markdown,
        style,
        &DiskImages {
            note_dir: note_dir.to_path_buf(),
        },
    );
    compile(doc)
}

pub fn compile(doc: markup::TypstDoc) -> Result<Vec<u8>, String> {
    let world = world::DocWorld::new(doc.source, doc.files)?;
    let warned = typst::compile::<PagedDocument>(&world);
    for w in &warned.warnings {
        log::debug!("pdf export warning: {}", w.message);
    }
    let document = warned.output.map_err(|errs| diag_message(&errs))?;
    typst_pdf::pdf(&document, &typst_pdf::PdfOptions::default()).map_err(|errs| diag_message(&errs))
}

fn diag_message(errs: &[typst::diag::SourceDiagnostic]) -> String {
    errs.iter()
        .map(|e| e.message.to_string())
        .collect::<Vec<_>>()
        .join("; ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paper_by_locale() {
        assert_eq!(PdfStyle::paper_for_locale("en-US"), "us-letter");
        assert_eq!(PdfStyle::paper_for_locale("pt-BR"), "a4");
        assert_eq!(PdfStyle::paper_for_locale("en"), "a4");
    }

    #[test]
    fn renders_a_rich_note() {
        let md = r#"---
title: x
---
# Heading *one*

Some **bold**, `code`, ~~strike~~ and a [[Wiki Link]] with "quotes" #tag $5.

## Lists

- plain
  - nested
- [ ] open task
- [x] done task

1. first
2. second

> quoted text

```rust
fn main() {
    println!("hi");
}
```

```mermaid
graph TD; A-->B
```

| Left | Center | Right |
|:-----|:------:|------:|
| a | b | c |

---

![](tiny.png) ![missing](nope.png) a<br>b
"#;
        let dir = std::env::temp_dir().join(format!("fuchico-pdf-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // 1x1 PNG
        let png = base64_png();
        std::fs::write(dir.join("tiny.png"), png).unwrap();
        let pdf = render_pdf(md, &PdfStyle::test(), &dir).expect("pdf");
        assert!(pdf.starts_with(b"%PDF"));
        assert!(pdf.len() > 1000);
        if let Ok(out) = std::env::var("FUCHICO_PDF_TEST_OUT") {
            std::fs::write(out, &pdf).unwrap();
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn hostile_text_cannot_break_out() {
        let md =
            "\"; #panic(\"x\") ]] ) } #let a = 1 \\ `#panic()` [x](\"q) <script>#panic()</script>";
        render_pdf(md, &PdfStyle::test(), Path::new("/nonexistent")).expect("pdf");
    }

    fn base64_png() -> Vec<u8> {
        use base64::Engine as _;
        base64::engine::general_purpose::STANDARD
            .decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNkYAAAAAYAAjCB0C8AAAAASUVORK5CYII=")
            .unwrap()
    }
}
