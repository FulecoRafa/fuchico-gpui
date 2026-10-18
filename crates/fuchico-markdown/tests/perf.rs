//! Re-parse cost on a 5k-line note, every keystroke. Run with
//! `cargo test -p fuchico-markdown --release --test perf -- --nocapture`
//! for the README numbers; the debug-build budget is generous.

use std::time::Instant;

use fuchico_markdown::*;

fn note(lines: usize) -> String {
    let block = [
        "# Heading with *emphasis* and `code`",
        "",
        "Some paragraph text with a [link](http://example.com), a [[Wiki Link]] and #tag.",
        "- [ ] task item @due 📅 2026-01-01",
        "- [x] done item with **bold** and ~~strike~~",
        "> [!NOTE] Callout title",
        "> callout body line",
        "",
        "```rust",
        "fn main() { let x = 42; println!(\"{x}\"); }",
        "```",
        "",
        "| a | b |",
        "|---|--:|",
        "| 1 | 2 |",
        "",
        ":::fold Region",
        "  indented text www.example.com :smile:",
        ":::endfold",
        "",
    ];
    let mut out = Vec::with_capacity(lines);
    let mut i = 0;
    while out.len() < lines {
        out.push(block[i % block.len()].to_string());
        i += 1;
    }
    out.join("\n")
}

#[test]
fn reparse_5k_lines() {
    let text = note(5000);
    let t = Instant::now();
    let mut m = MarkdownModel::new(&text, Settings::default());
    println!("initial parse: {:.2} ms", t.elapsed().as_secs_f64() * 1e3);
    assert_eq!(m.line_count(), 5000);
    let folds = FoldState::new();
    let files = vec!["/v/Wiki Link.md".to_string()];

    // Warm up (grammar loading, caches).
    let ctx = DecorationContext { cursor_line: Some(2500), folds: &folds, vault_files: &files, table_edit: None };
    let w = Instant::now();
    let _ = m.decorations(2480..2540, &ctx);
    println!("cold first decorations (syntax-set load + first fence highlight): {:.1} ms", w.elapsed().as_secs_f64() * 1e3);

    let n = 30;
    let mut doc = text.clone();
    let mut parse_total = 0.0;
    let mut deco_total = 0.0;
    let mut worst: f64 = 0.0;
    for k in 0..n {
        // Type one char in the middle of the document.
        let at = doc.len() / 2 + k;
        let at = (at..doc.len()).find(|p| doc.is_char_boundary(*p)).unwrap();
        let edit = TextEdit::insert(at, "x");
        doc = apply_edits(&doc, std::slice::from_ref(&edit));
        let t0 = Instant::now();
        m.update(&doc, std::slice::from_ref(&edit));
        let t1 = Instant::now();
        let line = m.lines().line_of(at);
        let ctx = DecorationContext { cursor_line: Some(line), folds: &folds, vault_files: &files, table_edit: None };
        let d = m.decorations(line.saturating_sub(30)..line + 30, &ctx);
        let t2 = Instant::now();
        assert_eq!(d.lines.len(), 60);
        let p = (t1 - t0).as_secs_f64() * 1e3;
        parse_total += p;
        deco_total += (t2 - t1).as_secs_f64() * 1e3;
        worst = worst.max(p + (t2 - t1).as_secs_f64() * 1e3);
    }
    let t = Instant::now();
    let folds_all = m.fold_all();
    let fold_ms = t.elapsed().as_secs_f64() * 1e3;
    let t = Instant::now();
    let full = m.decorations(0..m.line_count(), &DecorationContext::new(None, &folds));
    let full_ms = t.elapsed().as_secs_f64() * 1e3;
    println!(
        "5k lines ({} KB): update avg {:.2} ms, visible decorations (60 lines) avg {:.2} ms, worst keystroke {:.2} ms; \
         fold_all {:.2} ms ({} folds); full-document decorations {:.2} ms",
        doc.len() / 1024,
        parse_total / n as f64,
        deco_total / n as f64,
        worst,
        fold_ms,
        folds_all.len(),
        full_ms
    );
    assert_eq!(full.lines.len(), 5000);
    let budget = if cfg!(debug_assertions) { 400.0 } else { 16.0 };
    assert!((parse_total + deco_total) / (n as f64) < budget);
}
