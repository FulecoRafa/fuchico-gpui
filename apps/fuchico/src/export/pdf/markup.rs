//! Markdown -> Typst markup for the native PDF export.
//!
//! The note goes through the same preprocessing as the HTML export
//! (frontmatter stripped, wikilinks rewritten by
//! [`fuchico_core::render_html::rewrite_wikilinks`], GFM tables /
//! strikethrough / task lists) and is then emitted as Typst markup whose
//! look mirrors `READING_CSS` (see [`preamble`]). Every piece of note text
//! is emitted as a Typst *string literal* (`#"...";`), so nothing the user
//! wrote can be interpreted as Typst syntax.
//!
//! Differences from the HTML export, on purpose:
//! * task items get exactly **one** checkbox (fixes the double-checkbox
//!   QUIRK of the original's `marked` renderer);
//! * fenced code is syntax-highlighted with `fuchico-markdown`'s grammars
//!   and the theme's `--syntax-*` colours;
//! * `mermaid` fences are rendered as vector diagrams (fuchico-mermaid SVG);
//! * raw HTML cannot be rendered: tags are dropped and their text kept
//!   (`<br>` becomes a line break).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use fuchico_core::frontmatter::parse_frontmatter;
use fuchico_core::render_html::rewrite_wikilinks;
use fuchico_markdown::highlight::{find_grammar, highlight_snippet};
use fuchico_markdown::languages::language_for_info;
use fuchico_markdown::style::{ColorToken, StyleSet, flatten_spans};
use pulldown_cmark::{Alignment, CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};

use super::PdfStyle;

/// The generated Typst source plus the binary files it references
/// (virtual path -> bytes).
#[derive(Debug, Default)]
pub struct TypstDoc {
    pub source: String,
    pub files: BTreeMap<String, Vec<u8>>,
}

/// Loads an image referenced by the note. `href` is the decoded markdown
/// destination; returns its bytes, or `None` when it cannot be embedded
/// (remote, missing, unsupported).
pub trait ImageLoader {
    fn load(&self, href: &str) -> Option<Vec<u8>>;
}

/// Reads images from disk relative to the note's folder (the PDF export's
/// `resolveHref` for images); `data:` URLs are decoded inline.
pub struct DiskImages {
    pub note_dir: PathBuf,
}

impl ImageLoader for DiskImages {
    fn load(&self, href: &str) -> Option<Vec<u8>> {
        if let Some(data) = href.strip_prefix("data:") {
            return decode_data_url(data);
        }
        if is_external(href) {
            return None;
        }
        let decoded = percent_decode(href);
        let p = Path::new(&decoded);
        let full = if p.is_absolute() { p.to_path_buf() } else { self.note_dir.join(p) };
        std::fs::read(full).ok()
    }
}

/// `/^[a-z][a-z0-9+.-]*:/i` or protocol-relative `//`.
pub fn is_external(href: &str) -> bool {
    if href.starts_with("//") {
        return true;
    }
    let mut chars = href.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() => {}
        _ => return false,
    }
    for c in chars {
        if c == ':' {
            return true;
        }
        if !(c.is_ascii_alphanumeric() || matches!(c, '+' | '.' | '-')) {
            return false;
        }
    }
    false
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && i + 2 < b.len()
            && let Ok(v) = u8::from_str_radix(std::str::from_utf8(&b[i + 1..i + 3]).unwrap_or("zz"), 16)
        {
            out.push(v);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn decode_data_url(rest: &str) -> Option<Vec<u8>> {
    use base64::Engine as _;
    let (meta, data) = rest.split_once(',')?;
    if meta.ends_with(";base64") {
        base64::engine::general_purpose::STANDARD.decode(data.trim()).ok()
    } else {
        Some(percent_decode(data).into_bytes())
    }
}

/// Escapes `s` as the body of a Typst string literal.
pub fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => {}
            '\t' => out.push_str("    "),
            c => out.push(c),
        }
    }
    out
}

/// `#"text";` — literal text in markup.
fn text_lit(s: &str) -> String {
    if s.is_empty() { String::new() } else { format!("#\"{}\";", esc(s)) }
}

/// The document preamble: page, fonts and the `READING_CSS` look.
pub fn preamble(style: &PdfStyle) -> String {
    let c = |name: &str| style.color(name);
    let sans = font_list(&style.font_sans, &["Inter", "Libertinus Serif"]);
    let mono = font_list(&style.font_mono, &["JetBrains Mono", "DejaVu Sans Mono"]);
    format!(
        r#"#set document(title: "{title}")
#set page(paper: "{paper}", margin: (x: 2cm, y: 2.2cm), fill: {bg})
#set text(font: {sans}, size: 12pt, fill: {fg}, top-edge: "ascender", bottom-edge: "descender", hyphenate: false)
#set par(leading: 0.48em, spacing: 1em, justify: false)
#set block(spacing: 1em)
#let monofonts = {mono}
#show heading: it => {{
  let sizes = (2em, 1.5em, 1.25em, 1em, 0.83em, 0.67em)
  set text(size: sizes.at(it.level - 1), weight: "bold", fill: {kw})
  set par(leading: 0.2em)
  block(above: if it.level == 1 {{ 0.67em }} else {{ 1.2em }}, below: 0.5em, sticky: true, it.body)
}}
#show raw: set text(font: monofonts, size: 0.9em)
#show raw.where(block: false): it => box(fill: {muted}, inset: (x: 0.35em), outset: (y: 0.18em), radius: 3pt, it)
#show link: set text(fill: {func})
#show strike: set text(fill: {fg})
#set table(stroke: 0.75pt + {border}, inset: (x: 7.5pt, y: 4.5pt), fill: (x, y) => if y == 0 {{ {muted} }})
#show table.cell.where(y: 0): set text(weight: "bold")
#set list(indent: 0.4em, body-indent: 0.6em, spacing: 0.5em)
#set enum(indent: 0.2em, body-indent: 0.6em, spacing: 0.5em)
#let mdcode(body) = block(width: 100%, fill: {muted}, inset: (x: 12pt, y: 10.5pt), radius: 6pt, breakable: true, {{
  set text(font: monofonts, size: 0.9em, hyphenate: false)
  set par(leading: 0.42em, justify: false)
  body
}})
#let mdquote(body) = block(stroke: (left: 2.25pt + {border}), inset: (left: 1em, y: 0.2em), {{
  set text(fill: {mutedfg})
  body
}})
#let mdhr() = block(above: 2em, below: 2em, line(length: 100%, stroke: 0.75pt + {border}))
#let mdcheck(c) = box(baseline: 0.13em, width: 0.85em, height: 0.85em, radius: 0.18em,
  stroke: 0.07em + (if c {{ {primary} }} else {{ {mutedfg} }}), fill: if c {{ {primary} }} else {{ none }},
  if c {{ align(center + horizon, text(size: 0.7em, fill: {primaryfg}, weight: "bold", font: ("DejaVu Sans Mono",), "\u{{2713}}")) }}) + h(0.5em)
#let mdimg(p, alt) = layout(size => {{
  let im = image(p, alt: alt)
  let w = measure(im).width * 0.75
  if w > size.width {{ image(p, alt: alt, width: size.width) }} else {{ image(p, alt: alt, width: w) }}
}})
"#,
        title = esc(&style.title),
        paper = style.paper,
        bg = c("--background"),
        fg = c("--foreground"),
        kw = c("--syntax-keyword"),
        muted = c("--muted"),
        mutedfg = c("--muted-foreground"),
        func = c("--syntax-function"),
        border = c("--border"),
        primary = c("--primary"),
        primaryfg = c("--primary-foreground"),
        sans = sans,
        mono = mono,
    )
}

/// `("User Font", "Fallback", ...)` as a Typst array.
fn font_list(first: &str, fallbacks: &[&str]) -> String {
    let mut names: Vec<String> = Vec::new();
    for f in first.split(',').map(|s| s.trim().trim_matches(['"', '\''])).filter(|s| !s.is_empty()) {
        names.push(f.to_string());
    }
    for f in fallbacks {
        if !names.iter().any(|n| n.eq_ignore_ascii_case(f)) {
            names.push(f.to_string());
        }
    }
    let items: Vec<String> = names.iter().map(|n| format!("\"{}\"", esc(n))).collect();
    format!("({},)", items.join(", "))
}

enum Frame {
    Buf(String),
    Heading(u8, String),
    Quote(String),
    Code { info: Option<String>, text: String },
    List { start: Option<u64>, items: Vec<ListItem>, loose: bool },
    Item(ListItem),
    Table { aligns: Vec<Alignment>, rows: Vec<Vec<Cell>>, row: Vec<Cell> },
    Cell(Cell),
    Wrap { open: &'static str, buf: String },
    Link { dest: String, buf: String },
    Image { dest: String, alt: String },
}

#[derive(Default)]
struct ListItem {
    buf: String,
    task: Option<bool>,
}

#[derive(Default)]
struct Cell {
    buf: String,
    chars: usize,
}

struct Emitter<'a> {
    style: &'a PdfStyle,
    images: &'a dyn ImageLoader,
    out: String,
    stack: Vec<Frame>,
    files: BTreeMap<String, Vec<u8>>,
    n_images: usize,
    n_diagrams: usize,
}

impl<'a> Emitter<'a> {
    fn sink(&mut self) -> &mut String {
        for f in self.stack.iter_mut().rev() {
            match f {
                Frame::Buf(b) | Frame::Heading(_, b) | Frame::Quote(b) => return b,
                Frame::Wrap { buf, .. } | Frame::Link { buf, .. } => return buf,
                Frame::Item(i) => return &mut i.buf,
                Frame::Cell(c) => return &mut c.buf,
                // Text inside these is captured separately.
                Frame::Code { .. } | Frame::Image { .. } => unreachable!("code/image text is captured"),
                Frame::List { .. } | Frame::Table { .. } => continue,
            }
        }
        &mut self.out
    }

    fn push(&mut self, s: &str) {
        self.sink().push_str(s);
    }

    /// Plain text: captured by code blocks / image alts, counted for table
    /// column widths, otherwise emitted as a string literal.
    fn text(&mut self, t: &str) {
        match self.stack.last_mut() {
            Some(Frame::Code { text, .. }) => {
                text.push_str(t);
                return;
            }
            Some(Frame::Image { alt, .. }) => {
                alt.push_str(t);
                return;
            }
            _ => {}
        }
        if let Some(Frame::Cell(c)) = self.stack.iter_mut().rev().find(|f| matches!(f, Frame::Cell(_))) {
            c.chars += t.chars().count();
        }
        let lit = text_lit(t);
        self.push(&lit);
    }

    fn block(&mut self, s: &str) {
        self.push("\n");
        self.push(s);
        self.push("\n");
    }

    fn start(&mut self, tag: Tag) {
        match tag {
            Tag::Paragraph => {
                if let Some(Frame::Item(_)) = self.stack.last()
                    && let Some(Frame::List { loose, .. }) = self.stack.iter_mut().rev().nth(1)
                {
                    *loose = true;
                }
            }
            Tag::Heading { level, .. } => {
                let n = match level {
                    HeadingLevel::H1 => 1,
                    HeadingLevel::H2 => 2,
                    HeadingLevel::H3 => 3,
                    HeadingLevel::H4 => 4,
                    HeadingLevel::H5 => 5,
                    HeadingLevel::H6 => 6,
                };
                self.stack.push(Frame::Heading(n, String::new()));
            }
            Tag::BlockQuote(_) => self.stack.push(Frame::Quote(String::new())),
            Tag::CodeBlock(kind) => {
                let info = match kind {
                    CodeBlockKind::Fenced(i) => Some(i.to_string()),
                    CodeBlockKind::Indented => None,
                };
                self.stack.push(Frame::Code { info, text: String::new() });
            }
            Tag::HtmlBlock => self.stack.push(Frame::Buf(String::new())),
            Tag::List(start) => self.stack.push(Frame::List { start, items: Vec::new(), loose: false }),
            Tag::Item => self.stack.push(Frame::Item(ListItem::default())),
            Tag::Table(aligns) => self.stack.push(Frame::Table { aligns, rows: Vec::new(), row: Vec::new() }),
            Tag::TableHead | Tag::TableRow => {}
            Tag::TableCell => self.stack.push(Frame::Cell(Cell::default())),
            Tag::Emphasis => self.stack.push(Frame::Wrap { open: "#emph[", buf: String::new() }),
            Tag::Strong => self.stack.push(Frame::Wrap { open: "#strong[", buf: String::new() }),
            Tag::Strikethrough => self.stack.push(Frame::Wrap { open: "#strike[", buf: String::new() }),
            Tag::Superscript => self.stack.push(Frame::Wrap { open: "#super[", buf: String::new() }),
            Tag::Subscript => self.stack.push(Frame::Wrap { open: "#sub[", buf: String::new() }),
            Tag::Link { dest_url, .. } => self.stack.push(Frame::Link { dest: dest_url.to_string(), buf: String::new() }),
            Tag::Image { dest_url, .. } => self.stack.push(Frame::Image { dest: dest_url.to_string(), alt: String::new() }),
            Tag::FootnoteDefinition(_)
            | Tag::DefinitionList
            | Tag::DefinitionListTitle
            | Tag::DefinitionListDefinition
            | Tag::MetadataBlock(_) => self.stack.push(Frame::Buf(String::new())),
        }
    }

    fn end(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Paragraph => self.push("\n\n"),
            TagEnd::Heading(_) => {
                if let Some(Frame::Heading(n, b)) = self.stack.pop() {
                    self.block(&format!("#heading(level: {n})[{b}];"));
                }
            }
            TagEnd::BlockQuote(_) => {
                if let Some(Frame::Quote(b)) = self.stack.pop() {
                    self.block(&format!("#mdquote[{}];", b.trim()));
                }
            }
            TagEnd::CodeBlock => {
                if let Some(Frame::Code { info, text }) = self.stack.pop() {
                    let s = self.code_block(info.as_deref(), &text);
                    self.block(&s);
                }
            }
            TagEnd::List(_) => {
                if let Some(Frame::List { start, items, loose }) = self.stack.pop() {
                    let s = list_markup(start, &items, loose);
                    self.block(&s);
                }
            }
            TagEnd::Item => {
                if let Some(Frame::Item(item)) = self.stack.pop()
                    && let Some(Frame::List { items, .. }) = self.stack.last_mut()
                {
                    items.push(item);
                }
            }
            TagEnd::TableCell => {
                if let Some(Frame::Cell(c)) = self.stack.pop()
                    && let Some(Frame::Table { row, .. }) = self.stack.last_mut()
                {
                    row.push(c);
                }
            }
            TagEnd::TableHead | TagEnd::TableRow => {
                if let Some(Frame::Table { rows, row, .. }) = self.stack.last_mut()
                    && !row.is_empty()
                {
                    rows.push(std::mem::take(row));
                }
            }
            TagEnd::Table => {
                if let Some(Frame::Table { aligns, rows, .. }) = self.stack.pop() {
                    let s = table_markup(&aligns, &rows);
                    self.block(&s);
                }
            }
            TagEnd::Emphasis | TagEnd::Strong | TagEnd::Strikethrough | TagEnd::Superscript | TagEnd::Subscript => {
                if let Some(Frame::Wrap { open, buf }) = self.stack.pop() {
                    self.push(&format!("{open}{buf}];"));
                }
            }
            TagEnd::Link => {
                if let Some(Frame::Link { dest, buf }) = self.stack.pop() {
                    if dest.is_empty() {
                        self.push(&buf);
                    } else {
                        self.push(&format!("#link(\"{}\")[{buf}];", esc(&dest)));
                    }
                }
            }
            TagEnd::Image => {
                if let Some(Frame::Image { dest, alt }) = self.stack.pop() {
                    let s = self.image(&dest, &alt);
                    self.push(&s);
                }
            }
            TagEnd::HtmlBlock
            | TagEnd::FootnoteDefinition
            | TagEnd::DefinitionList
            | TagEnd::DefinitionListTitle
            | TagEnd::DefinitionListDefinition
            | TagEnd::MetadataBlock(_) => {
                if let Some(Frame::Buf(b)) = self.stack.pop() {
                    self.push(&b);
                    self.push("\n\n");
                }
            }
        }
    }

    fn html(&mut self, html: &str) {
        for (is_tag, piece) in split_html(html) {
            if is_tag {
                let name = piece.trim_start_matches(['<', '/']).split([' ', '>', '/']).next().unwrap_or("");
                if name.eq_ignore_ascii_case("br") {
                    self.push("#linebreak();");
                }
            } else {
                let t = decode_entities(piece);
                if !t.trim().is_empty() {
                    self.text(&t);
                }
            }
        }
    }

    fn image(&mut self, dest: &str, alt: &str) -> String {
        let fallback = || format!("#emph[{}];", text_lit(if alt.is_empty() { dest } else { alt }));
        let Some(bytes) = self.images.load(dest) else { return fallback() };
        let Some(ext) = sniff_image(&bytes) else { return fallback() };
        let name = format!("/img-{}.{ext}", self.n_images);
        self.n_images += 1;
        self.files.insert(name.clone(), bytes);
        format!("#box(mdimg(\"{name}\", \"{}\"));", esc(alt))
    }

    fn code_block(&mut self, info: Option<&str>, text: &str) -> String {
        let info = info.unwrap_or("").trim();
        let code = text.strip_suffix('\n').unwrap_or(text);
        if info.eq_ignore_ascii_case("mermaid") {
            match fuchico_mermaid::render_svg(code, &self.style.mermaid) {
                Ok(svg) => {
                    let name = format!("/mermaid-{}.svg", self.n_diagrams);
                    self.n_diagrams += 1;
                    self.files.insert(name.clone(), svg.into_bytes());
                    return format!("#align(center, mdimg(\"{name}\", \"mermaid\"));");
                }
                Err(e) => log::info!("pdf export: mermaid block not rendered: {e}"),
            }
        }
        let grammar = language_for_info(info).and_then(|d| d.grammar).and_then(find_grammar);
        let mut body = String::new();
        let mut runs: Vec<(std::ops::Range<usize>, Option<ColorToken>)> = Vec::new();
        if let Some(g) = grammar {
            let spans = highlight_snippet(code, g);
            let mut pos = 0;
            for run in flatten_spans(&spans) {
                if run.range.start > pos {
                    runs.push((pos..run.range.start, None));
                }
                runs.push((run.range.clone(), color_of(run.styles)));
                pos = run.range.end;
            }
            if pos < code.len() {
                runs.push((pos..code.len(), None));
            }
        } else {
            runs.push((0..code.len(), None));
        }
        for (range, color) in runs {
            let Some(seg) = code.get(range) else { continue };
            for (i, line) in seg.split('\n').enumerate() {
                if i > 0 {
                    body.push_str("#linebreak();");
                }
                if line.is_empty() {
                    continue;
                }
                // Leading/trailing spaces would be collapsed by the text
                // shaper only in markup; inside strings they are kept, but a
                // non-breaking space keeps indentation intact at line starts.
                let line = keep_indent(line);
                match color {
                    Some(c) => {
                        body.push_str(&format!("#text(fill: {}, \"{}\");", self.style.token_color(c), esc(&line)))
                    }
                    None => body.push_str(&text_lit(&line)),
                }
            }
        }
        format!("#mdcode[{body}];")
    }
}

/// Replaces runs of spaces with non-breaking spaces so indentation and
/// alignment survive Typst's whitespace handling.
fn keep_indent(s: &str) -> String {
    s.replace(' ', "\u{a0}")
}

fn color_of(styles: StyleSet) -> Option<ColorToken> {
    styles.resolve().color
}

fn list_markup(start: Option<u64>, items: &[ListItem], loose: bool) -> String {
    let all_tasks = !items.is_empty() && items.iter().all(|i| i.task.is_some());
    let body: Vec<String> = items
        .iter()
        .map(|i| {
            let mut content = String::new();
            if let Some(c) = i.task {
                content.push_str(&format!("#mdcheck({c});"));
            }
            content.push_str(i.buf.trim());
            format!("[{content}]")
        })
        .collect();
    let tight = if loose { "false" } else { "true" };
    match start {
        Some(n) => format!("#enum(start: {n}, tight: {tight}, {});", body.join(", ")),
        None if all_tasks => format!("#list(marker: none, indent: 0pt, body-indent: 0pt, tight: {tight}, {});", body.join(", ")),
        None => format!("#list(tight: {tight}, {});", body.join(", ")),
    }
}

fn table_markup(aligns: &[Alignment], rows: &[Vec<Cell>]) -> String {
    let ncols = aligns.len().max(rows.iter().map(|r| r.len()).max().unwrap_or(0)).max(1);
    // Width weights from the longest cell text of each column (CSS auto
    // table layout distributes width by content too).
    let mut weights = vec![1usize; ncols];
    for r in rows {
        for (i, c) in r.iter().enumerate().take(ncols) {
            weights[i] = weights[i].max(c.chars.clamp(3, 40));
        }
    }
    let cols: Vec<String> = weights.iter().map(|w| format!("{w}fr")).collect();
    let al: Vec<&str> = (0..ncols)
        .map(|i| match aligns.get(i) {
            Some(Alignment::Center) => "center",
            Some(Alignment::Right) => "right",
            _ => "left",
        })
        .collect();
    let cell = |c: Option<&Cell>| format!("[{}]", c.map(|c| c.buf.trim()).unwrap_or(""));
    let row_cells = |r: &Vec<Cell>| (0..ncols).map(|i| cell(r.get(i))).collect::<Vec<_>>().join(", ");
    let mut parts = Vec::new();
    if let Some(head) = rows.first() {
        parts.push(format!("table.header({})", row_cells(head)));
    }
    for r in rows.iter().skip(1) {
        parts.push(row_cells(r));
    }
    format!("#table(columns: ({},), align: ({},), {});", cols.join(", "), al.join(", "), parts.join(", "))
}

/// Splits HTML into `(is_tag, text)` pieces.
fn split_html(s: &str) -> Vec<(bool, &str)> {
    let mut out = Vec::new();
    let mut rest = s;
    while let Some(open) = rest.find('<') {
        if open > 0 {
            out.push((false, &rest[..open]));
        }
        match rest[open..].find('>') {
            Some(close) => {
                out.push((true, &rest[open..open + close + 1]));
                rest = &rest[open + close + 1..];
            }
            None => {
                out.push((false, &rest[open..]));
                rest = "";
            }
        }
    }
    if !rest.is_empty() {
        out.push((false, rest));
    }
    out
}

fn decode_entities(s: &str) -> String {
    s.replace("&nbsp;", "\u{a0}")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&amp;", "&")
}

/// Image formats Typst can embed, detected from the bytes.
pub fn sniff_image(b: &[u8]) -> Option<&'static str> {
    if b.starts_with(b"\x89PNG") {
        Some("png")
    } else if b.starts_with(&[0xff, 0xd8, 0xff]) {
        Some("jpg")
    } else if b.starts_with(b"GIF8") {
        Some("gif")
    } else if b.len() > 12 && &b[0..4] == b"RIFF" && &b[8..12] == b"WEBP" {
        Some("webp")
    } else {
        let head = String::from_utf8_lossy(&b[..b.len().min(512)]).to_lowercase();
        (head.contains("<svg") || (head.trim_start().starts_with("<?xml") && head.contains("svg"))).then_some("svg")
    }
}

/// Converts a note to a Typst document.
pub fn to_typst(markdown: &str, style: &PdfStyle, images: &dyn ImageLoader) -> TypstDoc {
    let body = match parse_frontmatter(markdown) {
        Some(fm) => &markdown[fm.body_start..],
        None => markdown,
    };
    let src = rewrite_wikilinks(body);
    let opts = Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS;
    let mut em = Emitter {
        style,
        images,
        out: String::new(),
        stack: Vec::new(),
        files: BTreeMap::new(),
        n_images: 0,
        n_diagrams: 0,
    };
    for ev in Parser::new_ext(&src, opts) {
        match ev {
            Event::Start(tag) => em.start(tag),
            Event::End(tag) => em.end(tag),
            Event::Text(t) => em.text(&t),
            Event::Code(t) => {
                if let Some(Frame::Cell(c)) = em.stack.iter_mut().rev().find(|f| matches!(f, Frame::Cell(_))) {
                    c.chars += t.chars().count();
                }
                if let Some(Frame::Image { alt, .. }) = em.stack.last_mut() {
                    alt.push_str(&t);
                } else {
                    em.push(&format!("#raw(\"{}\");", esc(&t)));
                }
            }
            Event::InlineMath(t) | Event::DisplayMath(t) => em.text(&t),
            Event::Html(h) | Event::InlineHtml(h) => {
                if matches!(em.stack.last(), Some(Frame::Code { .. }) | Some(Frame::Image { .. })) {
                    em.text(&h);
                } else {
                    em.html(&h);
                }
            }
            Event::FootnoteReference(r) => em.push(&format!("#super[{}];", text_lit(&format!("[{r}]")))),
            Event::SoftBreak => em.text(" "),
            Event::HardBreak => em.push("#linebreak();"),
            Event::Rule => em.block("#mdhr();"),
            Event::TaskListMarker(c) => {
                if let Some(Frame::Item(i)) = em.stack.last_mut() {
                    i.task = Some(c);
                }
            }
        }
    }
    // Unbalanced input cannot happen with pulldown-cmark, but never lose text.
    while let Some(f) = em.stack.pop() {
        if let Frame::Buf(b) = f {
            em.out.push_str(&b);
        }
    }
    TypstDoc { source: format!("{}\n{}", preamble(style), em.out), files: em.files }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct NoImages;
    impl ImageLoader for NoImages {
        fn load(&self, _: &str) -> Option<Vec<u8>> {
            None
        }
    }

    fn body(md: &str) -> String {
        let d = to_typst(md, &PdfStyle::test(), &NoImages);
        let pre = preamble(&PdfStyle::test());
        d.source[pre.len()..].trim().to_string()
    }

    #[test]
    fn escapes_user_text() {
        assert_eq!(esc(r#"a "b" \ #c"#), r#"a \"b\" \\ #c"#);
        assert_eq!(body("Hi #x $y$ *z*"), "#\"Hi #x $y$ \";#emph[#\"z\";];");
    }

    #[test]
    fn frontmatter_is_stripped_and_wikilinks_rewritten() {
        let b = body("---\ntitle: x\n---\nsee [[Note B|alias]]");
        assert!(!b.contains("title"));
        assert!(b.contains("#link(\"Note%20B.html\")[#\"alias\";];"), "{b}");
    }

    #[test]
    fn task_items_have_a_single_checkbox() {
        let b = body("- [ ] a\n- [x] b\n");
        assert_eq!(b.matches("#mdcheck(").count(), 2);
        assert!(b.contains("#mdcheck(false);#\"a\";"));
        assert!(b.contains("#mdcheck(true);#\"b\";"));
        assert!(b.contains("marker: none"));
    }

    #[test]
    fn headings_lists_tables_rules() {
        let b = body("# T\n\n1. one\n2. two\n\n| a | b |\n|:-:|--:|\n| 1 | 2 |\n\n---\n");
        assert!(b.contains("#heading(level: 1)[#\"T\";];"));
        assert!(b.contains("#enum(start: 1, tight: true, [#\"one\";], [#\"two\";]);"));
        assert!(b.contains("align: (center, right,)"), "{b}");
        assert!(b.contains("table.header([#\"a\";], [#\"b\";]), [#\"1\";], [#\"2\";]"), "{b}");
        assert!(b.contains("#mdhr();"));
    }

    #[test]
    fn code_is_highlighted_with_theme_colours() {
        let b = body("```rust\nfn main() {}\n```\n");
        assert!(b.starts_with("#mdcode["), "{b}");
        assert!(b.contains("#text(fill: rgb("), "{b}");
    }

    #[test]
    fn mermaid_becomes_a_vector_image() {
        let d = to_typst("```mermaid\ngraph TD; A-->B\n```\n", &PdfStyle::test(), &NoImages);
        assert!(d.source.contains("mdimg(\"/mermaid-0.svg\""));
        assert!(String::from_utf8_lossy(&d.files["/mermaid-0.svg"]).contains("<svg"));
    }

    #[test]
    fn missing_image_falls_back_to_alt() {
        assert_eq!(body("![[pic.png]]"), "#emph[#\"pic.png\";];");
    }

    #[test]
    fn html_is_reduced_to_text() {
        assert_eq!(body("a<br>b <b>c</b>"), "#\"a\";#linebreak();#\"b \";#\"c\";");
    }

    #[test]
    fn external_hrefs() {
        assert!(is_external("https://x"));
        assert!(is_external("//cdn/x.png"));
        assert!(is_external("data:image/png;base64,AA"));
        assert!(!is_external("pics/a.png"));
        assert!(!is_external("/abs/a.png"));
    }
}
