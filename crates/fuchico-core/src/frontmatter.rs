//! Port of `src/modules/frontmatter/lib/parseFrontmatter.ts`.
//!
//! Dependency-free parser for YAML frontmatter (`---\n...\n---`) at the very
//! start of a note. Supports scalars, quoted strings, inline lists and block
//! lists only (same scope cut as the TS version). Offsets are **byte**
//! offsets (the TS version used UTF-16 units).

use std::sync::LazyLock;

use regex::Regex;

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    List(Vec<Value>),
}

/// Insertion-ordered key/value pairs (a duplicate key overwrites in place,
/// like a JS object).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Data(pub Vec<(String, Value)>);

impl Data {
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.0.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }

    fn set(&mut self, key: String, value: Value) {
        if let Some(slot) = self.0.iter_mut().find(|(k, _)| *k == key) {
            slot.1 = value;
        } else {
            self.0.push((key, value));
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ParsedFrontmatter {
    pub data: Data,
    /// Raw YAML text between the fences.
    pub raw: String,
    /// Byte offset where the note body starts (after the closing fence line).
    pub body_start: usize,
    /// 1-indexed line number of the closing fence.
    pub end_line: usize,
}

static KEY_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^([^:\s][^:]*):[ \t]?(.*)$").unwrap());
static INT_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^-?[0-9]+$").unwrap());
static FLOAT_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^-?[0-9]+\.[0-9]+$").unwrap());

pub fn parse_frontmatter(text: &str) -> Option<ParsedFrontmatter> {
    if !text.starts_with("---") {
        return None;
    }
    let lines: Vec<&str> = text.split('\n').collect();
    if lines[0].trim() != "---" {
        return None;
    }
    let close_idx = (1..lines.len()).find(|&i| {
        let t = lines[i].trim();
        t == "---" || t == "..."
    })?;

    let yaml_lines: Vec<&str> = lines[1..close_idx]
        .iter()
        .map(|l| l.strip_suffix('\r').unwrap_or(l))
        .collect();
    let raw = yaml_lines.join("\n");
    let data = parse_yaml_block(&yaml_lines);

    // Offset of the start of line `close_idx + 1`.
    let mut offset = 0usize;
    let mut body_start = text.len();
    for (i, l) in lines.iter().enumerate() {
        if i == close_idx + 1 {
            body_start = offset;
            break;
        }
        offset += l.len() + 1;
    }
    Some(ParsedFrontmatter {
        data,
        raw,
        body_start,
        end_line: close_idx + 1,
    })
}

fn parse_yaml_block(lines: &[&str]) -> Data {
    let mut data = Data::default();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        let trimmed_line = line.trim();
        if trimmed_line.is_empty() || trimmed_line.starts_with('#') {
            i += 1;
            continue;
        }
        let Some(m) = KEY_RE.captures(line) else {
            i += 1;
            continue;
        };
        let key = m[1].trim().to_string();
        let value_str = m[2].trim();

        if value_str.is_empty() {
            let mut items = Vec::new();
            let mut j = i + 1;
            while j < lines.len() {
                let next_trimmed = lines[j].trim();
                if next_trimmed.is_empty() {
                    j += 1;
                    continue;
                }
                if next_trimmed.starts_with("- ") || next_trimmed == "-" {
                    let item_val = if next_trimmed == "-" {
                        ""
                    } else {
                        next_trimmed[2..].trim()
                    };
                    items.push(coerce_scalar(strip_quotes(item_val)));
                    j += 1;
                    continue;
                }
                break;
            }
            if !items.is_empty() {
                data.set(key, Value::List(items));
                i = j;
                continue;
            }
            data.set(key, Value::Null);
            i += 1;
            continue;
        }

        if value_str.starts_with('[') && value_str.ends_with(']') && value_str.len() >= 2 {
            let inner = value_str[1..value_str.len() - 1].trim();
            let list = if inner.is_empty() {
                Vec::new()
            } else {
                split_top_level_comma(inner)
                    .iter()
                    .map(|s| coerce_scalar(strip_quotes(s.trim())))
                    .collect()
            };
            data.set(key, Value::List(list));
        } else {
            data.set(key, coerce_scalar(strip_quotes(value_str)));
        }
        i += 1;
    }
    data
}

fn strip_quotes(s: &str) -> &str {
    let b = s.as_bytes();
    if b.len() >= 2
        && ((b[0] == b'"' && b[b.len() - 1] == b'"') || (b[0] == b'\'' && b[b.len() - 1] == b'\''))
    {
        &s[1..s.len() - 1]
    } else {
        s
    }
}

fn coerce_scalar(s: &str) -> Value {
    match s {
        "" | "null" | "~" => Value::Null,
        "true" => Value::Bool(true),
        "false" => Value::Bool(false),
        _ if INT_RE.is_match(s) => match s.parse::<i64>() {
            Ok(n) => Value::Int(n),
            Err(_) => s
                .parse::<f64>()
                .map(Value::Float)
                .unwrap_or_else(|_| Value::Str(s.to_string())),
        },
        _ if FLOAT_RE.is_match(s) => s
            .parse::<f64>()
            .map(Value::Float)
            .unwrap_or_else(|_| Value::Str(s.to_string())),
        _ => Value::Str(s.to_string()),
    }
}

/// Splits a comma-separated inline-list body, respecting quoted segments.
fn split_top_level_comma(s: &str) -> Vec<String> {
    let mut result = Vec::new();
    let mut cur = String::new();
    let mut in_quote: Option<char> = None;
    for ch in s.chars() {
        if let Some(q) = in_quote {
            cur.push(ch);
            if ch == q {
                in_quote = None;
            }
            continue;
        }
        match ch {
            '"' | '\'' => {
                in_quote = Some(ch);
                cur.push(ch);
            }
            ',' => result.push(std::mem::take(&mut cur)),
            _ => cur.push(ch),
        }
    }
    result.push(cur);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_scalars_lists_and_body_start() {
        let text = "---\ntitle: \"Hello\"\nn: 3\nf: 1.5\nok: true\nnone:\ntags: [a, \"b, c\"]\nlist:\n  - x\n  - 2\n---\nbody";
        let fm = parse_frontmatter(text).unwrap();
        assert_eq!(fm.data.get("title"), Some(&Value::Str("Hello".into())));
        assert_eq!(fm.data.get("n"), Some(&Value::Int(3)));
        assert_eq!(fm.data.get("f"), Some(&Value::Float(1.5)));
        assert_eq!(fm.data.get("ok"), Some(&Value::Bool(true)));
        assert_eq!(fm.data.get("none"), Some(&Value::Null));
        assert_eq!(
            fm.data.get("tags"),
            Some(&Value::List(vec![
                Value::Str("a".into()),
                Value::Str("b, c".into())
            ]))
        );
        assert_eq!(
            fm.data.get("list"),
            Some(&Value::List(vec![Value::Str("x".into()), Value::Int(2)]))
        );
        assert_eq!(&text[fm.body_start..], "body");
        assert_eq!(fm.end_line, 11);
    }

    #[test]
    fn rejects_missing_fences() {
        assert!(parse_frontmatter("# no frontmatter").is_none());
        assert!(parse_frontmatter("---\ntitle: x\nno close").is_none());
        assert!(parse_frontmatter(" ---\nx: 1\n---").is_none());
    }

    #[test]
    fn closing_fence_without_body_and_crlf() {
        let fm = parse_frontmatter("---\r\na: 1\r\n---").unwrap();
        assert_eq!(fm.data.get("a"), Some(&Value::Int(1)));
        assert_eq!(fm.body_start, "---\r\na: 1\r\n---".len());
    }
}
