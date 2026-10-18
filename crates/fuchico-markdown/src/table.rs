//! GFM tables: parsed model with source ranges, the original
//! auto-formatter, and the table widget's keyboard/structural operations as
//! pure text edits (port of `tableStyle.ts`).

use std::ops::Range;

use crate::text::{LineIndex, TextEdit};

/// Column alignment (`None` in an `Option<Align>` = unspecified).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Align {
    Left,
    Center,
    Right,
}

/// A cell: trimmed, `\|`-unescaped text plus its source range (trimmed
/// content; an empty range at the row end for cells missing in the source).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableCell {
    pub text: String,
    pub range: Range<usize>,
}

/// The cell contents of a table, independent of the source (what the widget
/// edits and what [`format_table`] writes).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TableData {
    pub header: Vec<String>,
    pub align: Vec<Option<Align>>,
    pub rows: Vec<Vec<String>>,
}

impl TableData {
    pub fn col_count(&self) -> usize {
        self.header.len()
    }

    /// Rows including the header (index 0), like the widget's `liveCells`.
    pub fn row_count(&self) -> usize {
        self.rows.len() + 1
    }

    pub fn cell(&self, row: usize, col: usize) -> &str {
        let r = if row == 0 { Some(&self.header) } else { self.rows.get(row - 1) };
        r.and_then(|r| r.get(col)).map(String::as_str).unwrap_or("")
    }

    pub fn set_cell(&mut self, row: usize, col: usize, text: impl Into<String>) {
        let r = if row == 0 { &mut self.header } else { &mut self.rows[row - 1] };
        if r.len() <= col {
            r.resize(col + 1, String::new());
        }
        r[col] = text.into();
    }

    /// Inserts a blank row after `after` (0 = header, i.e. becomes the first body row).
    pub fn insert_row(&mut self, after: usize) {
        let at = after.min(self.rows.len());
        self.rows.insert(at, vec![String::new(); self.col_count()]);
    }

    /// Removes row `row` (1-based body index). Never removes the header and
    /// keeps at least one body row. Returns whether a row was removed.
    pub fn remove_row(&mut self, row: usize) -> bool {
        if row == 0 || self.rows.len() <= 1 || row > self.rows.len() {
            return false;
        }
        self.rows.remove(row - 1);
        true
    }

    /// Inserts a blank column (unspecified alignment) after column `after`.
    pub fn insert_column(&mut self, after: usize) {
        let at = (after + 1).min(self.col_count());
        self.header.insert(at, String::new());
        for r in &mut self.rows {
            let at = at.min(r.len());
            r.insert(at, String::new());
        }
        let at = at.min(self.align.len());
        self.align.insert(at, None);
    }

    /// Deletes column `col` (keeps at least one column). New operation (the
    /// original widget had no delete-column).
    pub fn delete_column(&mut self, col: usize) -> bool {
        if self.col_count() <= 1 || col >= self.col_count() {
            return false;
        }
        self.header.remove(col);
        for r in &mut self.rows {
            if col < r.len() {
                r.remove(col);
            }
        }
        if col < self.align.len() {
            self.align.remove(col);
        }
        true
    }

    /// Sets a column's alignment (new operation; the original only kept the
    /// alignment it parsed).
    pub fn set_alignment(&mut self, col: usize, align: Option<Align>) {
        if self.align.len() <= col {
            self.align.resize(col + 1, None);
        }
        self.align[col] = align;
    }
}

/// A parsed table block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableModel {
    /// Source range (Lezer `Table` node: start of the header row to the end
    /// of the last row, no trailing newline).
    pub range: Range<usize>,
    pub header: Vec<TableCell>,
    pub align: Vec<Option<Align>>,
    pub rows: Vec<Vec<TableCell>>,
    /// Column widths of the formatted table (`max(3, longest cell)`, UTF-16 units).
    pub widths: Vec<usize>,
    /// `>` prefix of quoted tables (re-applied when writing back).
    pub quote_prefix: Option<String>,
}

fn js_len(s: &str) -> usize {
    s.encode_utf16().count()
}

fn unescape_pipes(s: &str) -> String {
    s.replace("\\|", "|")
}

fn escape_pipes(s: &str) -> String {
    s.replace('|', "\\|")
}

/// Splits a row into trimmed cell ranges (relative to `row`), on unescaped
/// pipes, ignoring one leading and one trailing pipe.
fn split_row(row: &str) -> Vec<Range<usize>> {
    let b = row.as_bytes();
    let mut pipes = Vec::new();
    let mut i = 0;
    let mut in_code = 0usize;
    while i < b.len() {
        match b[i] {
            b'\\' => i += 1,
            b'`' => {
                let n = b[i..].iter().take_while(|c| **c == b'`').count();
                in_code = if in_code == n { 0 } else if in_code == 0 { n } else { in_code };
                i += n - 1;
            }
            b'|' if in_code == 0 => pipes.push(i),
            _ => {}
        }
        i += 1;
    }
    let trimmed_start = row.len() - row.trim_start().len();
    let trimmed_end = row.trim_end().len();
    let mut bounds = vec![];
    let mut start = trimmed_start;
    if pipes.first() == Some(&trimmed_start) {
        pipes.remove(0);
        start = trimmed_start + 1;
    }
    let mut end = trimmed_end;
    if trimmed_end > 0 && pipes.last() == Some(&(trimmed_end - 1)) && trimmed_end > start {
        pipes.pop();
        end = trimmed_end - 1;
    }
    let mut s = start;
    for p in pipes {
        bounds.push(s..p);
        s = p + 1;
    }
    bounds.push(s..end.max(s));
    bounds
        .into_iter()
        .map(|r| {
            let cell = &row[r.clone()];
            let a = r.start + (cell.len() - cell.trim_start().len());
            let e = r.start + cell.trim_end().len();
            a..e.max(a)
        })
        .collect()
}

fn parse_align(cell: &str) -> Option<Align> {
    let s = cell.trim();
    match (s.starts_with(':'), s.ends_with(':')) {
        (true, true) => Some(Align::Center),
        (false, true) => Some(Align::Right),
        (true, false) => Some(Align::Left),
        _ => None,
    }
}

/// Strips a quote prefix (`>` runs and spaces) from a row line, returning
/// the prefix length.
fn quote_prefix_len(line: &str) -> usize {
    let b = line.as_bytes();
    let mut i = 0;
    loop {
        while i < b.len() && b[i] == b' ' {
            i += 1;
        }
        if i < b.len() && b[i] == b'>' {
            i += 1;
            if i < b.len() && b[i] == b' ' {
                i += 1;
            }
        } else {
            break;
        }
    }
    i
}

impl TableModel {
    /// Parses the table block at `range` of `doc`.
    pub fn parse(doc: &str, range: Range<usize>, quoted: bool) -> Self {
        let idx = LineIndex::new(&doc[range.clone()]);
        let mut header = Vec::new();
        let mut align = Vec::new();
        let mut rows = Vec::new();
        let mut quote_prefix = None;
        for l in 0..idx.len() {
            let lr = idx.line_range(l);
            let abs = range.start + lr.start..range.start + lr.end;
            let line = &doc[abs.clone()];
            let skip = if quoted {
                let n = quote_prefix_len(line);
                if l == 1 {
                    quote_prefix = Some(line[..n].to_string());
                }
                if l == 0 { 0 } else { n }
            } else {
                0
            };
            let row = &line[skip..];
            let base = abs.start + skip;
            let cells: Vec<TableCell> = split_row(row)
                .into_iter()
                .map(|r| TableCell { text: unescape_pipes(&row[r.clone()]), range: base + r.start..base + r.end })
                .collect();
            match l {
                0 => header = cells,
                1 => align = split_row(row).iter().map(|r| parse_align(&row[r.clone()])).collect(),
                _ => rows.push(cells),
            }
        }
        while align.len() < header.len() {
            align.push(None);
        }
        // Missing cells are reported empty, positioned at the row end.
        let n = header.len();
        for r in &mut rows {
            let end = r.last().map(|c| c.range.end).unwrap_or(range.end);
            while r.len() < n {
                r.push(TableCell { text: String::new(), range: end..end });
            }
        }
        let data_rows: Vec<Vec<String>> = rows.iter().map(|r| r.iter().map(|c| c.text.clone()).collect()).collect();
        let head: Vec<String> = header.iter().map(|c| c.text.clone()).collect();
        let widths = column_widths(&head, &data_rows);
        if quote_prefix.is_none() && quoted {
            quote_prefix = Some("> ".into());
        }
        TableModel { range, header, align, rows, widths, quote_prefix }
    }

    pub fn data(&self) -> TableData {
        TableData {
            header: self.header.iter().map(|c| c.text.clone()).collect(),
            align: self.align.clone(),
            rows: self.rows.iter().map(|r| r.iter().map(|c| c.text.clone()).collect()).collect(),
        }
    }

    /// The cell at `(row, col)` (row 0 = header).
    pub fn cell(&self, row: usize, col: usize) -> Option<&TableCell> {
        if row == 0 { self.header.get(col) } else { self.rows.get(row - 1)?.get(col) }
    }

    /// `(row, col)` of the cell containing byte offset `pos`, if any.
    pub fn cell_at(&self, pos: usize) -> Option<(usize, usize)> {
        let all = std::iter::once(&self.header).chain(self.rows.iter());
        for (r, row) in all.enumerate() {
            for (c, cell) in row.iter().enumerate() {
                if cell.range.start <= pos && pos <= cell.range.end {
                    return Some((r, c));
                }
            }
        }
        None
    }

    /// Text the widget writes back for `data`, with the quote prefix
    /// re-applied to every line after the first.
    pub fn render(&self, data: &TableData) -> String {
        let t = format_table(data);
        match &self.quote_prefix {
            Some(p) => t.split('\n').enumerate().map(|(i, l)| if i == 0 { l.to_string() } else { format!("{p}{l}") }).collect::<Vec<_>>().join("\n"),
            None => t,
        }
    }

    /// `flush`: the edit replacing the table with `data` formatted, or
    /// `None` when the text is unchanged.
    pub fn flush(&self, doc: &str, data: &TableData) -> Option<TextEdit> {
        let text = self.render(data);
        (doc[self.range.clone()] != text).then(|| TextEdit::new(self.range.clone(), text))
    }

    /// The "reformat table" edit (pads every column).
    pub fn reformat(&self, doc: &str) -> Option<TextEdit> {
        self.flush(doc, &self.data())
    }
}

fn column_widths(header: &[String], rows: &[Vec<String>]) -> Vec<usize> {
    let n = header.len();
    let mut w: Vec<usize> = header.iter().map(|h| js_len(h).max(3)).collect();
    for r in rows {
        for (i, c) in r.iter().enumerate().take(n) {
            w[i] = w[i].max(js_len(c));
        }
    }
    w
}

fn pad_cell(text: &str, width: usize, align: Option<Align>) -> String {
    let pad = width.saturating_sub(js_len(text));
    match align {
        Some(Align::Right) => format!("{}{text}", " ".repeat(pad)),
        Some(Align::Center) => {
            let left = pad / 2;
            format!("{}{text}{}", " ".repeat(left), " ".repeat(pad - left))
        }
        _ => format!("{text}{}", " ".repeat(pad)),
    }
}

fn format_data_row(cells: &[String], widths: &[usize], align: &[Option<Align>]) -> String {
    let parts: Vec<String> = widths
        .iter()
        .enumerate()
        .map(|(i, w)| {
            let c = escape_pipes(cells.get(i).map(String::as_str).unwrap_or(""));
            format!(" {} ", pad_cell(&c, *w, align.get(i).copied().flatten()))
        })
        .collect();
    format!("|{}|", parts.join("|"))
}

fn format_delimiter_row(widths: &[usize], align: &[Option<Align>]) -> String {
    let parts: Vec<String> = widths
        .iter()
        .enumerate()
        .map(|(i, w)| {
            let n = (*w).max(3);
            let d = match align.get(i).copied().flatten() {
                Some(Align::Left) => format!(":{}", "-".repeat(n - 1)),
                Some(Align::Right) => format!("{}:", "-".repeat(n - 1)),
                Some(Align::Center) => format!(":{}:", "-".repeat(n - 2)),
                None => "-".repeat(n),
            };
            format!(" {d} ")
        })
        .collect();
    format!("|{}|", parts.join("|"))
}

/// `formatTable`: padded header, delimiter row and body rows joined by `\n`.
pub fn format_table(t: &TableData) -> String {
    let widths = column_widths(&t.header, &t.rows);
    let mut out = vec![format_data_row(&t.header, &widths, &t.align), format_delimiter_row(&widths, &t.align)];
    out.extend(t.rows.iter().map(|r| format_data_row(r, &widths, &t.align)));
    out.join("\n")
}

/// `DEFAULT_TABLE_MARKDOWN` (the "insert table" command).
pub fn default_table_markdown() -> String {
    format_table(&TableData {
        header: vec!["Column 1".into(), "Column 2".into()],
        align: vec![None, None],
        rows: vec![vec![String::new(), String::new()]],
    })
}

/// The widget's active-cell state (`TableEditState`). `row` 0 is the header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TableEditState {
    /// Table start offset (mapped through edits with [`crate::text::Assoc::Before`]).
    pub anchor: usize,
    pub row: usize,
    pub col: usize,
    pub editing: bool,
}

/// Keys the table widget handles.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TableKey {
    /// `h` / ArrowLeft (navigating only).
    Left,
    /// `l` / ArrowRight.
    Right,
    /// `j` / ArrowDown.
    Down,
    /// `k` / ArrowUp.
    Up,
    /// `i` / Enter while navigating: start editing.
    Edit,
    /// Enter while editing: next row (appends one past the end).
    Enter,
    Tab,
    ShiftTab,
    Escape,
}

/// Result of a table operation: an optional text edit (old-document
/// offsets), the new selection head (new-document offset) and the new edit
/// state (`None` = left the table).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableOutcome {
    pub edit: Option<TextEdit>,
    pub selection: usize,
    pub state: Option<TableEditState>,
    /// The cell contents after the operation.
    pub data: TableData,
}

fn finish(doc: &str, table: &TableModel, data: TableData, next: Next) -> TableOutcome {
    let edit = table.flush(doc, &data);
    let new_len = edit.as_ref().map(|e| e.insert.len()).unwrap_or(table.range.len());
    let from = table.range.start;
    let to = from + new_len;
    let doc_len = doc.len() - table.range.len() + new_len;
    match next {
        Next::Activate { row, col, editing } => TableOutcome {
            edit,
            // Parks the selection just inside the table (see `activate`).
            selection: (from + 1).min(from.max(to.saturating_sub(1))),
            state: Some(TableEditState { anchor: from, row, col, editing }),
            data,
        },
        Next::Exit { before } => TableOutcome {
            edit,
            selection: if before { from.saturating_sub(1) } else { doc_len.min(to + 1) },
            state: None,
            data,
        },
    }
}

enum Next {
    Activate { row: usize, col: usize, editing: bool },
    Exit { before: bool },
}

/// Handles a key in the table widget. `data` is the live cell contents
/// (possibly edited since the last flush).
pub fn table_key(doc: &str, table: &TableModel, mut data: TableData, st: TableEditState, key: TableKey) -> TableOutcome {
    let cols = data.col_count().max(1);
    let rows = data.row_count();
    let clamp_c = |c: isize| c.clamp(0, cols as isize - 1) as usize;
    let next = match key {
        TableKey::Left | TableKey::Right | TableKey::Up | TableKey::Down if !st.editing => {
            let (dr, dc) = match key {
                TableKey::Left => (0, -1),
                TableKey::Right => (0, 1),
                TableKey::Down => (1, 0),
                _ => (-1, 0),
            };
            let c = clamp_c(st.col as isize + dc);
            let r = st.row as isize + dr;
            if r < 0 {
                Next::Exit { before: true }
            } else if r as usize > rows - 1 {
                Next::Exit { before: false }
            } else {
                Next::Activate { row: r as usize, col: c, editing: false }
            }
        }
        TableKey::Left | TableKey::Right | TableKey::Up | TableKey::Down => {
            Next::Activate { row: st.row, col: st.col, editing: st.editing }
        }
        TableKey::Edit => Next::Activate { row: st.row, col: st.col, editing: true },
        TableKey::Enter if st.editing => {
            let nr = st.row + 1;
            if nr > rows - 1 {
                data.insert_row(rows - 1);
            }
            Next::Activate { row: nr, col: st.col, editing: true }
        }
        TableKey::Enter => Next::Activate { row: st.row, col: st.col, editing: true },
        TableKey::Tab => {
            let mut r = st.row;
            let mut c = st.col as isize + 1;
            if c > cols as isize - 1 {
                c = 0;
                r += 1;
                if r > rows - 1 {
                    data.insert_row(rows - 1);
                    r = rows;
                }
            }
            Next::Activate { row: r, col: clamp_c(c), editing: true }
        }
        TableKey::ShiftTab => {
            let mut r = st.row;
            let mut c = st.col as isize - 1;
            if c < 0 {
                c = cols as isize - 1;
                r = r.saturating_sub(1);
            }
            Next::Activate { row: r, col: clamp_c(c), editing: true }
        }
        TableKey::Escape if st.editing => Next::Activate { row: st.row, col: st.col, editing: false },
        TableKey::Escape => Next::Exit { before: true },
    };
    finish(doc, table, data, next)
}

/// Activates a cell (click / programmatic), flushing pending edits.
pub fn table_activate(doc: &str, table: &TableModel, data: TableData, row: usize, col: usize, editing: bool) -> TableOutcome {
    finish(doc, table, data, Next::Activate { row, col, editing })
}

/// "+ Add row" (appends after the last row); keeps the state.
pub fn table_add_row(doc: &str, table: &TableModel, mut data: TableData, st: Option<TableEditState>) -> TableOutcome {
    let n = data.row_count();
    data.insert_row(n - 1);
    keep(doc, table, data, st)
}

/// "+" add column (appends after the last column); keeps the state.
pub fn table_add_column(doc: &str, table: &TableModel, mut data: TableData, st: Option<TableEditState>) -> TableOutcome {
    let n = data.col_count();
    data.insert_column(n.saturating_sub(1));
    keep(doc, table, data, st)
}

/// "×" remove row `row`: re-activates `min(activeRow, rows - 1)` in
/// navigation mode.
pub fn table_remove_row(doc: &str, table: &TableModel, mut data: TableData, row: usize, st: Option<TableEditState>) -> TableOutcome {
    if !data.remove_row(row) {
        return keep(doc, table, data, st);
    }
    let active = st.map(|s| s.row.min(data.row_count() - 1)).unwrap_or(0);
    let col = st.map(|s| s.col).unwrap_or(0);
    finish(doc, table, data, Next::Activate { row: active, col, editing: false })
}

/// Deletes column `col`, clamping the active column.
pub fn table_delete_column(doc: &str, table: &TableModel, mut data: TableData, col: usize, st: Option<TableEditState>) -> TableOutcome {
    data.delete_column(col);
    let st = st.map(|mut s| {
        s.col = s.col.min(data.col_count().saturating_sub(1));
        s
    });
    keep(doc, table, data, st)
}

/// Sets column `col`'s alignment.
pub fn table_set_alignment(doc: &str, table: &TableModel, mut data: TableData, col: usize, align: Option<Align>, st: Option<TableEditState>) -> TableOutcome {
    data.set_alignment(col, align);
    keep(doc, table, data, st)
}

fn keep(doc: &str, table: &TableModel, data: TableData, st: Option<TableEditState>) -> TableOutcome {
    match st {
        Some(s) => finish(doc, table, data, Next::Activate { row: s.row, col: s.col, editing: s.editing }),
        None => {
            let edit = table.flush(doc, &data);
            TableOutcome { edit, selection: table.range.start, state: None, data }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const T: &str = "| a | b |\n| :-- | --: |\n| 1 | 2 |\n| 3 | 4 |";

    fn model(doc: &str) -> TableModel {
        TableModel::parse(doc, 0..doc.len(), false)
    }

    #[test]
    fn parses_cells_ranges_alignment() {
        let m = model(T);
        assert_eq!(m.header.iter().map(|c| c.text.as_str()).collect::<Vec<_>>(), ["a", "b"]);
        assert_eq!(m.align, [Some(Align::Left), Some(Align::Right)]);
        assert_eq!(m.rows.len(), 2);
        assert_eq!(&T[m.rows[1][1].range.clone()], "4");
        assert_eq!(m.widths, [3, 3]);
        assert_eq!(m.cell_at(m.rows[0][1].range.start), Some((1, 1)));
    }

    #[test]
    fn rows_without_outer_pipes_keep_all_cells() {
        let doc = "a | b\n--|--\nx | y";
        let m = model(doc);
        assert_eq!(m.data().rows, vec![vec!["x".to_string(), "y".to_string()]]);
        assert_eq!(m.data().header, ["a", "b"]);
    }

    #[test]
    fn escaped_pipes_and_missing_cells() {
        let doc = "| a | b |\n|---|---|\n| x \\| y |";
        let m = model(doc);
        assert_eq!(m.data().rows[0], ["x | y", ""]);
        let out = format_table(&m.data());
        assert!(out.contains("x \\| y"));
    }

    #[test]
    fn formatting_matches_original() {
        let data = TableData {
            header: vec!["Name".into(), "n".into(), "c".into()],
            align: vec![None, Some(Align::Right), Some(Align::Center)],
            rows: vec![vec!["x".into(), "12".into(), "ab".into()]],
        };
        assert_eq!(format_table(&data), "| Name |   n |  c  |\n| ---- | --: | :-: |\n| x    |  12 | ab  |");
        assert_eq!(default_table_markdown(), "| Column 1 | Column 2 |\n| -------- | -------- |\n|          |          |");
    }

    #[test]
    fn reformat_only_when_changed() {
        let m = model(T);
        let e = m.reformat(T).unwrap();
        assert_eq!(e.insert, "| a   |   b |\n| :-- | --: |\n| 1   |   2 |\n| 3   |   4 |");
        let doc2 = e.insert.clone();
        assert!(model(&doc2).reformat(&doc2).is_none());
    }

    fn st(row: usize, col: usize, editing: bool) -> TableEditState {
        TableEditState { anchor: 0, row, col, editing }
    }

    #[test]
    fn navigation_and_exit() {
        let doc = format!("x\n{T}\ny");
        let m = TableModel::parse(&doc, 2..2 + T.len(), false);
        let fmt_len = m.render(&m.data()).len();
        let o = table_key(&doc, &m, m.data(), st(0, 1, false), TableKey::Right);
        assert_eq!(o.state.unwrap().col, 1); // clamped
        assert_eq!(o.selection, 3);
        let o = table_key(&doc, &m, m.data(), st(0, 0, false), TableKey::Up);
        assert!(o.state.is_none());
        assert_eq!(o.selection, 1);
        let o = table_key(&doc, &m, m.data(), st(2, 0, false), TableKey::Down);
        assert!(o.state.is_none());
        assert_eq!(o.selection, 2 + fmt_len + 1);
        let o = table_key(&doc, &m, m.data(), st(1, 0, false), TableKey::Escape);
        assert!(o.state.is_none());
        let o = table_key(&doc, &m, m.data(), st(1, 0, true), TableKey::Escape);
        assert_eq!(o.state, Some(TableEditState { anchor: 2, row: 1, col: 0, editing: false }));
        // Arrow keys while editing are the input's own (no move).
        let o = table_key(&doc, &m, m.data(), st(1, 0, true), TableKey::Down);
        assert_eq!(o.state.unwrap().row, 1);
    }

    #[test]
    fn tab_wraps_and_appends() {
        let m = model(T);
        let o = table_key(T, &m, m.data(), st(1, 1, true), TableKey::Tab);
        assert_eq!((o.state.unwrap().row, o.state.unwrap().col), (2, 0));
        let o = table_key(T, &m, m.data(), st(2, 1, true), TableKey::Tab);
        assert_eq!((o.state.unwrap().row, o.state.unwrap().col), (3, 0));
        assert_eq!(o.data.rows.len(), 3);
        assert!(o.edit.unwrap().insert.ends_with("|     |     |"));
        let o = table_key(T, &m, m.data(), st(1, 0, true), TableKey::ShiftTab);
        assert_eq!((o.state.unwrap().row, o.state.unwrap().col), (0, 1));
        let o = table_key(T, &m, m.data(), st(0, 0, true), TableKey::ShiftTab);
        assert_eq!((o.state.unwrap().row, o.state.unwrap().col), (0, 1));
    }

    #[test]
    fn enter_moves_down_and_appends() {
        let m = model(T);
        let o = table_key(T, &m, m.data(), st(1, 1, true), TableKey::Enter);
        assert_eq!(o.state.unwrap().row, 2);
        assert_eq!(o.data.rows.len(), 2);
        let o = table_key(T, &m, m.data(), st(2, 1, true), TableKey::Enter);
        assert_eq!(o.state.unwrap().row, 3);
        assert_eq!(o.data.rows.len(), 3);
        let o = table_key(T, &m, m.data(), st(2, 1, false), TableKey::Edit);
        assert!(o.state.unwrap().editing);
    }

    #[test]
    fn structural_ops() {
        let m = model(T);
        let o = table_add_column(T, &m, m.data(), None);
        assert_eq!(o.data.col_count(), 3);
        assert_eq!(o.data.align[2], None);
        let o = table_remove_row(T, &m, m.data(), 2, Some(st(2, 1, true)));
        assert_eq!(o.data.rows.len(), 1);
        assert_eq!(o.state.unwrap().row, 1);
        assert!(!o.state.unwrap().editing);
        let mut d = o.data.clone();
        assert!(!d.remove_row(1)); // keep one body row
        assert!(!d.remove_row(0)); // never the header
        let o = table_delete_column(T, &m, m.data(), 0, Some(st(1, 1, false)));
        assert_eq!(o.data.header, ["b"]);
        assert_eq!(o.state.unwrap().col, 0);
        let o = table_set_alignment(T, &m, m.data(), 0, Some(Align::Center), None);
        assert!(o.edit.unwrap().insert.contains("| :-: |"));
        let o = table_add_row(T, &m, m.data(), None);
        assert_eq!(o.data.rows.len(), 3);
    }

    #[test]
    fn quoted_tables_keep_prefix() {
        let doc = "> | a |\n> |---|\n> | b |";
        let start = 2;
        let m = TableModel::parse(doc, start..doc.len(), true);
        assert_eq!(m.data().rows[0], ["b"]);
        let mut d = m.data();
        d.set_cell(1, 0, "xyz");
        let e = m.flush(doc, &d).unwrap();
        assert_eq!(e.insert, "| a   |\n> | --- |\n> | xyz |");
    }
}
