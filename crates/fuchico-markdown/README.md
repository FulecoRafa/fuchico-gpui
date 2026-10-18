# fuchico-markdown

This crate is the UI-agnostic Markdown live-preview model for the native (GPUI) Fuchico.

It replaces every CodeMirror `ViewPlugin` / `StateField` in the original editor:
`markdownStyle`, `tableStyle`, `codeBlockStyle`, `frontmatterStyle`,
wikilink/tag decorations, `mermaidPreviewButton`, region folding and
decorations, `rulers`, outline, and the completion sources.

It has **no gpui dependency**. It outputs plain data: line kinds, classes,
heights, style runs, hide/replace ranges, widgets and fold ranges. The view
crate turns that data into elements. It reuses `fuchico-core` for:
- frontmatter;
- wikilink resolution;
- tag parsing;
- task helpers;
- fuzzy matching;
- outline extraction.

## API tour

```rust
use fuchico_markdown::{
    DecorationContext, FoldState, MarkdownModel, Settings, TextEdit, Widget,
};

let mut model = MarkdownModel::new("# Title\n\nSome *text* and [[Note]].\n", Settings::default());
let mut folds = FoldState::new();

// After each transaction: the new text plus the edits (old-doc offsets).
let edits = [TextEdit::insert(9, "More ")];
let new_text = fuchico_markdown::apply_edits(model.text(), &edits);
model.update(&new_text, &edits);
folds.map(&edits);

// Decorate only the visible lines.
let vault: Vec<String> = vec!["/vault/Note.md".into()];
let ctx = DecorationContext { vault_files: &vault, ..DecorationContext::new(Some(0), &folds) };
let deco = model.decorations(0..model.line_count(), &ctx);
for line in &deco.lines {
    let metrics = line.height.metrics();       // font scale, line height, colour
    for run in &line.styles { let _s = run.styles.resolve(); }
    for r in &line.replacements {
        match &r.widget {
            None => { /* hidden marker: collapse r.range */ }
            Some(Widget::Wikilink { label, resolved, .. }) => { /* pill */ }
            Some(_) => {}
        }
    }
    let _ = (metrics, &line.line_end, &line.block, line.hidden, &line.indent_guides);
}

// Folding.
model.toggle_fold(&mut folds, 0);   // heading section
let all = model.fold_all();
let placeholder = model.fold_placeholder(&all[0]);

// Completion.
let today = chrono::Local::now().date_naive();
let options = model.completions(&fuchico_markdown::CompletionContext {
    pos: 12, vault_files: &vault, all_tags: &[], today, read_note: &|_| None,
});
```

### `MarkdownModel` (lib.rs)
- `new(text, Settings)`
- `update(text, edits)`, `apply(edits)` and `update_rope(&Rope, edits)`
- `set_settings`
- `text()`, `lines()`, `line_count()` and `line_range(line)`
- `parsed()` and `line_info(line)`
- `is_inside_code(pos)`
- `table_count()`, `table(i)` and `table_at(pos)`
- `outline()`
- `rulers()`

`Settings` holds the fold markers (`:::fold` / `:::endfold`), `tab_size`
(2), `rulers` and `inline_images` (false).

### Decorations (decorate.rs)
`decorations(visible_lines, &DecorationContext)` returns a `LineDecorations` value, one `LineDecoration` per visible line. Each line has:

| Field | Contents |
|---|---|
| `kind` (`BlockKind`) | Paragraph, Heading(n), SetextHeading, Fence{Open/Body/Close}, Table, Frontmatter, Hr, ListItem, Blank, and so on |
| `quote_depth` / `list_depth` | Nesting depth |
| `callout` | Callout type and header flag |
| `classes` (`LineClass`) | The original `cm-*` line classes |
| `height` (`LineHeight`) | `.metrics()` gives the exact `chromeTheme` values |
| `indent_guides` | Columns where a guide is drawn |
| `styles` | Non-overlapping `StyleRun`s with a `StyleSet` of `StyleId`s; `.resolve()` gives colour token, bold, italic, underline, strike, mono |
| `replacements` | Sorted, non-overlapping. `widget: None` hides the range. Otherwise the widget is one of Checkbox, Link, Wikilink, Tag, CalloutHeader, Image, RegionStart/End/MarkerHidden or Fold |
| `line_end` | `FenceToolbar` (language list, language range, copy text, `set_language`) or `MermaidButton` |
| `block` | `BlockWidget::Table` on the first table line; the remaining table lines are `hidden: Block` |
| `hidden` | `No`, `Block` (under a block widget) or `Folded` |

`checkbox_toggle(doc, pos)` gives the edit for a checkbox click.

### Tables (table.rs)
- `TableModel::parse`
- `TableData` ops:
  - `set_cell`, `insert_row`, `remove_row`, `insert_column`
  - `delete_column` and `set_alignment` (both new)
- `format_table` (canonical rewrite) and `flush` / `reformat`, which return a `TextEdit` or `None` when nothing changed
- `default_table_markdown`
- Widget state machine:
  - `table_key(doc, table, data, TableEditState, TableKey)`
  - `table_activate`, `table_add_row`, `table_add_column`, `table_remove_row`, `table_delete_column`, `table_set_alignment`

  Each returns a `TableOutcome {edit, selection, state, data}`. This gives the exact h/j/k/l, Enter, Tab, Shift-Tab and Escape behaviour, including exit-before/after and selection parking.

### Folding (fold.rs)
- `foldable(line)` tries the sources in the original order: region marker, heading section, then syntax fold. The syntax fold is the outermost multi-line node starting on the line: list, blockquote, fence, table, paragraph or frontmatter.
- `fold_all()` and `toggle_fold(&mut FoldState, line)`.
- `fold_placeholder(range)` returns `…` or the region placeholder `name · N lines`.
- `FoldState` methods:
  - `fold`, `unfold_in`, `folded_at` and `covering`;
  - `map(edits)`, with the start mapped After and the end Before;
  - `reveal(pos)`, which unfolds when the cursor enters a fold.
- For code files, `code_foldable(text, idx, line, CodeFoldStyle, tab)` folds by brackets or by indent. Indent is used for Python and HTML.

### Highlighting (highlight.rs, languages.rs)
- `languages::LANGUAGES` holds the 143 `@codemirror/language-data` entries, in order. Each entry has its exact names and aliases and a syntect grammar name. **All 143 entries have a grammar:**
  - 90 use two-face (bat) grammars.
  - 53 use `.sublime-syntax` ports of the CodeMirror legacy modes the original loaded, in `grammars/<slug>.sublime-syntax`. Brainfuck, APL, Cobol, Haxe, Pug, SAS, XQuery and others are among them. Each keeps the legacy mode's keyword, builtin and atom lists and its comment, string and number rules, and maps legacy token types to scopes the way `StreamLanguage` maps them to highlight tags.
- `build.rs` links two-face's set with `grammars/` and writes an uncompressed syntect dump to `OUT_DIR`. `syntax_set()` decodes it lazily on first use, in about 2.7 ms.
  - A grammar file that fails to load is skipped with a cargo warning.
  - `tests/languages.rs` then reports that language as missing.
- `match_language_name(name, fuzzy)` matches CodeMirror's `LanguageDescription.matchLanguageName`, including the fuzzy word-boundary search.
- Other functions:
  - `toolbar_options`;
  - `resolve_file_language(path)` and `resolve_display_name`;
  - `FileHighlighter`, the incremental per-line highlighter for non-Markdown files.
- Fence bodies are highlighted through a content-hashed `SnippetCache`. Typing outside a fence never re-highlights it.

### Rulers (rulers.rs)
- `parse_rulers` is exact JS `Number` semantics: integers in 1..=1000, deduped and sorted.
- `format_rulers`, `column_x` and `ruler_positions`.

### Completion (completion.rs)
- `should_open_completion(before, picked)` is the explicit-open trigger.
- `MarkdownModel::completions(&CompletionContext)` runs the providers in the original order: wikilink, heading, tag, task, word. Each provider applies its own trigger regex, fuzzy ranking and cap (50 / 50 / 50 / - / 20).
- Each option carries:
  - `range`, a ready `edit` and `cursor`;
  - `picked`, which suppresses the re-open, as `pickedCompletion` did;
  - `boost` and `custom_apply`.
- The list is in global provider order, because the original used `filter:false`. Consecutive duplicates are removed and the list is capped at 100 rendered options.
- **The original has no frontmatter-key completion**, so none is provided.

## Parser choice: pulldown-cmark, full reparse

- **Speed.** pulldown-cmark 0.13 is the fastest maintained CommonMark+GFM parser in Rust.
- **Offsets.** It gives byte-offset events (`into_offset_iter`). That is all a decoration model needs: node kinds and ranges, including markers (`HeaderMark`, `EmphasisMark`, `CodeMark`, `QuoteMark`, `TaskMarker` equivalents are derived from the ranges).
- **GFM coverage.** It supports tables, task lists, strikethrough, footnotes and autolinks with the same GFM rules as `@lezer/markdown`. Both treat a pipe-less line directly after a table as a body row; this was checked against the lezer dist.

Alternatives considered:
- **tree-sitter-markdown** would give incremental parsing. However, it splits block and inline grammars, its GFM coverage is weaker, and its C dependency complicates the build.
- **comrak** builds an arena AST and is about 2-3x slower.

A full reparse per keystroke was measured: it stays far under a frame on a 5k-line note (see below). Incremental parsing was therefore not worth the complexity. `update()` accepts the edits for API stability and so a future incremental path can use them.

Frontmatter is cut out before the parse and styled with `fuchico_core::parse_frontmatter`, so YAML never parses as a setext heading or hr. The cut is limited to the first 20,000 chars, as in the original.

### Perf (release, Apple Silicon, `cargo test --release -p fuchico-markdown --test perf -- --nocapture`)

The test document is a 5,000-line note (97 KB) with headings, lists, tasks, tables, fences, wikilinks and tags.

| Operation | Time |
|---|---|
| Initial parse | 1.6-4.9 ms |
| Cold first `decorations` (syntax-set dump decode + first fence highlight) | 2.6-2.7 ms (about 9 ms on a cold page cache) |
| `update` (full reparse), average | 1.2-1.6 ms |
| `decorations` for 60 visible lines, average | 0.04-0.06 ms |
| Worst keystroke (update + decorations) | 1.5-2.8 ms |
| `fold_all` (249 folds) | 0.07 ms |
| `decorations` over the full document | 1.6-1.7 ms |

## Deviations from the original (deliberate or forced)

- **Frontmatter**
  - It is not Markdown-parsed (see above).
  - It is foldable as one block.
  - It never gets live-preview hiding.
- **Code detection for tags and words**
  - Tag and word skipping uses whole code ranges (fences and code spans).
  - The original checked the Lezer node at the match position, so `#include` inside a C fence was decorated as a tag. This is a fix.
- **Fence toolbar language range**
  - The range starts after the fence's actual backtick or tilde run, not a fixed 3 chars.
  - Changing the language of a ```` ```` ```` fence therefore works.
- **Tables**
  - Rows without outer pipes are parsed correctly.
  - Tables inside blockquotes keep their `> ` prefix when rewritten.
  - `delete_column` and `set_alignment` are new ops. The original UI had neither; nothing exposes them yet.
- **Images**
  - `![a](b)` renders as an `Image` widget only when `Settings::inline_images` is set (default off, matching the original).
- **Link widgets**
  - These are single-line only. A link whose text wraps a newline stays raw, as in the original, where the regex rejected it.
- **Indent guides**
  - These are new. The original had none.
- **Legacy-mode ports**
  - The tokenizer state machines (indent tracking, parser-driven scopes in Haxe, Gherkin's `allow*` flags, Pug's embedded JS and similar) are approximated with sublime contexts and lookarounds, so a few edge tokens differ.
  - Two legacy bugs are fixed:
    - Pig strings are coloured, where the legacy mode returned `error`.
    - The mscgen constants need word boundaries.
- **Diff colours**
  - `diff` additions take the string colour and deletions the destructive colour.
  - The original theme had no `inserted` / `deleted` rule, so diff fences only coloured punctuation there.
- **Code-file folding**
  - This uses bracket and indent heuristics in place of Lezer/legacy-mode syntax folds.
- **Unclosed-fence copy text**
  - The copy text excludes the last line. This is faithful to the original's quirk.
- **No frontmatter-key completion**
  - The original has none either.

## Tests

- No vitest tests existed for the editor plugins.
- The pure-helper vitest suites are already ported in `fuchico-core`: parseTags, taskHelpers, frontmatter, fuzzyMatch, outline and wikilink resolution.

This crate adds:

| File | Coverage |
|---|---|
| `tests/golden.rs` | Golden render per construct, cursor on vs off: ATX/setext headings, emphasis, strike, code, blockquotes, all callouts, hr, lists, checkboxes, links, autolinks, images, wikilinks (resolved/unresolved/alias), tags, fences + toolbar + mermaid button, tables, frontmatter, regions, indent guides, viewport windowing |
| `tests/folding.rs` | Fold sources, priorities, region placeholder, mapping, reveal, fold_all |
| `tests/completion.rs` | Every provider's trigger, ranking, caps, apply edits, picked flags, ordering |
| `tests/languages.rs` | All 143 fence languages: grammar present in the set, and the sample in `tests/lang_samples/<slug>.txt` highlights with at least 2 distinct styles (`LANG_FILTER=slug,...` for a subset) |
| `tests/rulers.rs` | JS `Number` parsing edge cases and positions |
| `tests/perf.rs` | 5k-line bench (asserts a generous budget, prints numbers) |
| Unit tests | Language matching, highlight scopes, table ops/state machine, text mapping, code folding |
