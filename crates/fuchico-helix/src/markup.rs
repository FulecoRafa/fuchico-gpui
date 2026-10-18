//! Markdown list / quote continuation for `o`, `O` and insert-mode `Enter`
//! (Fuchico issue #54). Not part of codemirror-helix; enabled with
//! [`crate::Config::continue_markup`].

/// The structural prefix of a Markdown line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinePrefix {
    /// Leading spaces/tabs.
    pub indent: String,
    /// Block quote markers (`"> "`, `"> > "`, ...).
    pub quote: String,
    /// Whitespace between the quote markers and the list marker.
    pub inner_indent: String,
    pub list: Option<ListMarker>,
    /// Char length of the whole prefix (where the item content starts).
    pub len: usize,
    /// The content after the prefix is blank.
    pub empty: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ListKind {
    Bullet(char),
    Ordered { number: u64, delim: char },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListMarker {
    pub kind: ListKind,
    /// `- [ ]` / `- [x]` task item.
    pub task: bool,
}

impl LinePrefix {
    /// Is there anything to continue (a list item or a quote)?
    pub fn has_markup(&self) -> bool {
        self.list.is_some() || !self.quote.is_empty()
    }

    /// Prefix for a new line opened below (`next = true`) or above.
    pub fn continuation(&self, next: bool) -> String {
        let mut out = String::new();
        out.push_str(&self.indent);
        out.push_str(&self.quote);
        if let Some(list) = &self.list {
            out.push_str(&self.inner_indent);
            match list.kind {
                ListKind::Bullet(c) => {
                    out.push(c);
                    out.push(' ');
                }
                ListKind::Ordered { number, delim } => {
                    let n = if next { number + 1 } else { number };
                    out.push_str(&n.to_string());
                    out.push(delim);
                    out.push(' ');
                }
            }
            if list.task {
                out.push_str("[ ] ");
            }
        }
        out
    }

    /// What an empty item line becomes when `Enter` is pressed on it: the
    /// list marker is dropped, or (for a bare quote) one quote level.
    pub fn cleared(&self) -> String {
        let mut out = self.indent.clone();
        if self.list.is_some() {
            out.push_str(&self.quote);
        } else {
            // drop the last "> " level
            let q = self.quote.trim_end();
            let q = q.strip_suffix('>').unwrap_or(q);
            out.push_str(q);
        }
        out
    }
}

/// Parse the Markdown prefix of `line`.
pub fn parse_prefix(line: &str) -> LinePrefix {
    let chars: Vec<char> = line.chars().collect();
    let mut i = 0;
    while i < chars.len() && (chars[i] == ' ' || chars[i] == '\t') {
        i += 1;
    }
    let indent: String = chars[..i].iter().collect();
    let quote_start = i;
    loop {
        let mut j = i;
        while j < chars.len() && j - i < 3 && chars[j] == ' ' && i > quote_start {
            j += 1;
        }
        if j < chars.len() && chars[j] == '>' {
            j += 1;
            if j < chars.len() && chars[j] == ' ' {
                j += 1;
            }
            i = j;
        } else {
            break;
        }
    }
    let quote: String = chars[quote_start..i].iter().collect();
    let inner_start = i;
    while i < chars.len() && (chars[i] == ' ' || chars[i] == '\t') {
        i += 1;
    }
    let inner_indent: String = chars[inner_start..i].iter().collect();
    let mut list = None;
    let marker_start = i;
    let at_space_or_end = |k: usize| k == chars.len() || chars[k] == ' ' || chars[k] == '\t';
    if i < chars.len() && matches!(chars[i], '-' | '*' | '+') && at_space_or_end(i + 1) {
        list = Some(ListKind::Bullet(chars[i]));
        i += 1;
    } else {
        let mut j = i;
        while j < chars.len() && chars[j].is_ascii_digit() && j - i < 9 {
            j += 1;
        }
        if j > i && j < chars.len() && matches!(chars[j], '.' | ')') && at_space_or_end(j + 1) {
            let number: u64 = chars[i..j].iter().collect::<String>().parse().unwrap_or(1);
            list = Some(ListKind::Ordered {
                number,
                delim: chars[j],
            });
            i = j + 1;
        }
    }
    let list = list.map(|kind| {
        // marker followed by spaces
        while i < chars.len() && chars[i] == ' ' {
            i += 1;
        }
        let rest: String = chars[i..].iter().collect();
        let mut task = false;
        for t in ["[ ]", "[x]", "[X]"] {
            if rest.starts_with(t) && (rest.len() == 3 || rest[3..].starts_with(' ')) {
                task = true;
                i += 3;
                while i < chars.len() && chars[i] == ' ' {
                    i += 1;
                }
                break;
            }
        }
        ListMarker { kind, task }
    });
    let (inner_indent, len) = if list.is_some() {
        (inner_indent, i)
    } else {
        // no list: the "inner indent" is part of the content
        (
            String::new(),
            inner_start.max(marker_start.min(inner_start)),
        )
    };
    let empty = chars[len..].iter().all(|c| c.is_whitespace());
    LinePrefix {
        indent,
        quote,
        inner_indent,
        list,
        len,
        empty,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bullets() {
        let p = parse_prefix("  - item");
        assert_eq!(p.indent, "  ");
        assert_eq!(p.len, 4);
        assert_eq!(p.continuation(true), "  - ");
        assert!(!p.empty);
        assert_eq!(parse_prefix("* a").continuation(true), "* ");
        assert_eq!(parse_prefix("+ a").continuation(true), "+ ");
        assert!(parse_prefix("-a").list.is_none());
        assert!(parse_prefix("- ").empty);
        assert!(parse_prefix("-").empty);
    }

    #[test]
    fn ordered() {
        let p = parse_prefix("9. nine");
        assert_eq!(p.continuation(true), "10. ");
        assert_eq!(p.continuation(false), "9. ");
        assert_eq!(parse_prefix("3) x").continuation(true), "4) ");
        assert!(parse_prefix("3x").list.is_none());
    }

    #[test]
    fn tasks_and_quotes() {
        assert_eq!(parse_prefix("- [x] done").continuation(true), "- [ ] ");
        assert_eq!(parse_prefix("> quote").continuation(true), "> ");
        assert_eq!(parse_prefix("> > deep").continuation(true), "> > ");
        assert_eq!(parse_prefix("> - in quote").continuation(true), "> - ");
        assert_eq!(parse_prefix("plain").continuation(true), "");
        assert_eq!(parse_prefix("    code").continuation(true), "    ");
        assert!(!parse_prefix("    code").has_markup());
    }

    #[test]
    fn cleared() {
        assert_eq!(parse_prefix("  - ").cleared(), "  ");
        assert_eq!(parse_prefix("> - ").cleared(), "> ");
        assert_eq!(parse_prefix("> ").cleared(), "");
        assert_eq!(parse_prefix("> > ").cleared(), "> ");
    }
}
