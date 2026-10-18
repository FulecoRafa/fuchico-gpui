//! Pure parts of `src/modules/editor/lib/taskHelpers.ts` (issue #45): the
//! canonical task syntax (`- [ ]`, `📅 YYYY-MM-DD [HH:MM]`, `🔁 rule [HH:MM]`)
//! expressed as text edits over a document string. Offsets are byte offsets.

use std::sync::LazyLock;

use chrono::{Datelike, Duration, NaiveDate};
use regex::Regex;

pub static DUE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"📅\s*([0-9]{4}-[0-9]{2}-[0-9]{2})(?:\s+([0-9]{2}:[0-9]{2}))?").unwrap());
pub static RECUR_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)🔁\s*(daily|weekdays|weekends|[a-z]{3}(?:,[a-z]{3})*)(?:\s+([0-9]{2}:[0-9]{2}))?").unwrap()
});
static LIST_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(\s*)([-*+]|[0-9]+[.)])\s+(?:\[([ xX])\]\s*)?").unwrap());

/// A replacement of `from..to` (byte offsets in the *original* document).
#[derive(Debug, Clone, PartialEq)]
pub struct TextChange {
    pub from: usize,
    pub to: usize,
    pub insert: String,
}

/// Applies non-overlapping changes expressed against the original text.
pub fn apply_changes(doc: &str, changes: &[TextChange]) -> String {
    let mut sorted: Vec<&TextChange> = changes.iter().collect();
    sorted.sort_by_key(|c| (c.from, c.to));
    let mut out = String::with_capacity(doc.len());
    let mut pos = 0;
    for c in sorted {
        out.push_str(&doc[pos..c.from]);
        out.push_str(&c.insert);
        pos = c.to;
    }
    out.push_str(&doc[pos..]);
    out
}

pub fn iso_date(d: NaiveDate) -> String {
    d.format("%Y-%m-%d").to_string()
}

const WEEKDAYS: [&str; 7] = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];

#[derive(Debug, Clone, PartialEq)]
pub struct DueOption {
    pub label: String,
    pub date: String,
    pub detail: String,
}

/// Quick-pick due dates: Today, Tomorrow, weekday names, "In a week".
pub fn due_date_options(now: NaiveDate) -> Vec<DueOption> {
    let mut raw = vec![("Today".to_string(), now), ("Tomorrow".to_string(), now + Duration::days(1))];
    for i in 2..=7 {
        let d = now + Duration::days(i);
        let label = if i == 7 {
            "In a week".to_string()
        } else {
            WEEKDAYS[d.weekday().num_days_from_sunday() as usize].to_string()
        };
        raw.push((label, d));
    }
    raw.into_iter()
        .map(|(label, d)| {
            let date = iso_date(d);
            DueOption { label, detail: date.clone(), date }
        })
        .collect()
}

pub struct RecurrenceOption {
    pub label: &'static str,
    pub rule: &'static str,
    pub detail: &'static str,
}

pub const RECURRENCE_OPTIONS: [RecurrenceOption; 6] = [
    RecurrenceOption { label: "Daily", rule: "daily", detail: "every day" },
    RecurrenceOption { label: "Weekdays", rule: "weekdays", detail: "Mon–Fri" },
    RecurrenceOption { label: "Weekends", rule: "weekends", detail: "Sat–Sun" },
    RecurrenceOption { label: "Weekly (Mon)", rule: "mon", detail: "every Monday" },
    RecurrenceOption { label: "Mon, Wed, Fri", rule: "mon,wed,fri", detail: "" },
    RecurrenceOption { label: "Tue, Thu", rule: "tue,thu", detail: "" },
];

/// (start, end) byte offsets of each line (without the `\n`).
fn line_spans(doc: &str) -> Vec<(usize, usize)> {
    let mut spans = Vec::new();
    let mut start = 0;
    for l in doc.split('\n') {
        spans.push((start, start + l.len()));
        start += l.len() + 1;
    }
    spans
}

fn line_index_at(spans: &[(usize, usize)], pos: usize) -> usize {
    spans.iter().position(|&(_, e)| pos <= e).unwrap_or(spans.len() - 1)
}

/// Add or remove the `[ ]` checkbox on every line touched by `selections`
/// (`(from, to)` byte ranges). Plain lines become `- [ ] ...`, list items get
/// a checkbox, task lines lose it. `None` if there is nothing to change.
pub fn toggle_task_line(doc: &str, selections: &[(usize, usize)]) -> Option<Vec<TextChange>> {
    let spans = line_spans(doc);
    let mut changes = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for &(from, to) in selections {
        let first = line_index_at(&spans, from);
        let last = line_index_at(&spans, to);
        #[allow(clippy::needless_range_loop)]
        for n in first..=last {
            if !seen.insert(n) {
                continue;
            }
            let (ls, le) = spans[n];
            let text = &doc[ls..le];
            if text.trim().is_empty() && first != last {
                continue;
            }
            match LIST_RE.captures(text) {
                None => {
                    let indent = text.len() - text.trim_start().len();
                    changes.push(TextChange { from: ls + indent, to: ls + indent, insert: "- [ ] ".into() });
                }
                Some(m) if m.get(3).is_some() => {
                    changes.push(TextChange {
                        from: ls,
                        to: ls + m[0].len(),
                        insert: format!("{}{} ", &m[1], &m[2]),
                    });
                }
                Some(m) => {
                    let at = ls + m[0].len();
                    changes.push(TextChange { from: at, to: at, insert: "[ ] ".into() });
                }
            }
        }
    }
    (!changes.is_empty()).then_some(changes)
}

#[derive(Debug, Clone, PartialEq)]
pub struct LineTokenEdit {
    pub change: TextChange,
    /// New cursor position (original-document coordinates mapped *after* the
    /// change is applied), only set when the token was appended.
    pub cursor: Option<usize>,
}

/// Replace the line's `📅 ...`/`🔁 ...` token with `emoji value [time]`
/// (keeping an existing time when none is given), or append one at the end
/// of the line containing `head`.
pub fn line_token_edit(doc: &str, head: usize, re: &Regex, emoji: &str, value: &str, time: Option<&str>) -> LineTokenEdit {
    let spans = line_spans(doc);
    let (ls, le) = spans[line_index_at(&spans, head)];
    let text = &doc[ls..le];
    let m = re.captures(text);
    let t = time.or_else(|| m.as_ref().and_then(|c| c.get(2)).map(|x| x.as_str()));
    let token = format!("{emoji} {value}{}", t.map(|t| format!(" {t}")).unwrap_or_default());
    if let Some(c) = m {
        let whole = c.get(0).unwrap();
        return LineTokenEdit {
            change: TextChange { from: ls + whole.start(), to: ls + whole.end(), insert: token },
            cursor: None,
        };
    }
    let trimmed_end = text.trim_end().len();
    let insert = format!("{}{token}", if trimmed_end > 0 { " " } else { "" });
    let cursor = ls + trimmed_end + insert.len();
    LineTokenEdit { change: TextChange { from: ls + trimmed_end, to: le, insert }, cursor: Some(cursor) }
}

// ─── Autocomplete: `@due`, `@repeat`, `📅`, `🔁` ────────────────────────────

static TASK_TRIGGER_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?:^|\s)(@(?:d(?:ue?)?|date|today|tom(?:orrow)?|r(?:ep(?:eat)?)?|every|recur)?(?:\s\S*)?|📅\s*\S*|🔁\s*\S*)$")
        .unwrap()
});
static DUE_KIND_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^@(d|date|today|tom)").unwrap());
static RECUR_KIND_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^@(r|every|recur)").unwrap());
static QUERY_STRIP_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(@\w*|📅|🔁)\s*").unwrap());

pub fn task_trigger(before: &str) -> bool {
    TASK_TRIGGER_RE.is_match(before)
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TaskKind {
    Due,
    Recur,
}

fn kind_of(text: &str) -> Option<TaskKind> {
    if text.starts_with('📅') || DUE_KIND_RE.is_match(text) {
        return Some(TaskKind::Due);
    }
    if text.starts_with('🔁') || RECUR_KIND_RE.is_match(text) {
        return Some(TaskKind::Recur);
    }
    if text == "@" {
        return Some(TaskKind::Due);
    }
    None
}

#[derive(Debug, Clone, PartialEq)]
pub struct TaskCompletion {
    pub label: String,
    pub detail: String,
    /// Text to insert in place of `from..cursor` (already includes any
    /// existing time suffix).
    pub insert: String,
    pub boost: i32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TaskCompletions {
    pub kind: TaskKind,
    /// Byte offset *within `before`* where the replaced range starts.
    pub from: usize,
    pub options: Vec<TaskCompletion>,
}

/// `before` is the line text up to the cursor; `line` the whole line (for the
/// existing time to keep).
pub fn task_completions(before: &str, line: &str, today: NaiveDate) -> Option<TaskCompletions> {
    let caps = TASK_TRIGGER_RE.captures(before)?;
    let text = caps.get(1).unwrap().as_str();
    let from = before.len() - text.len();
    let kind = kind_of(text)?;
    let re: &Regex = if kind == TaskKind::Due { &DUE_RE } else { &RECUR_RE };
    let existing_time = re.captures(line).and_then(|c| c.get(2)).map(|m| m.as_str());
    let suffix = existing_time.map(|t| format!(" {t}")).unwrap_or_default();
    let mk = |label: String, detail: String, token: String, boost: i32| TaskCompletion {
        label,
        detail,
        insert: format!("{token}{suffix}"),
        boost,
    };

    let mut options: Vec<TaskCompletion> = match kind {
        TaskKind::Due => due_date_options(today)
            .into_iter()
            .enumerate()
            .map(|(i, o)| mk(o.label, o.date.clone(), format!("📅 {}", o.date), 10 - i as i32))
            .collect(),
        TaskKind::Recur => RECURRENCE_OPTIONS
            .iter()
            .enumerate()
            .map(|(i, o)| mk(o.label.into(), o.detail.into(), format!("🔁 {}", o.rule), 10 - i as i32))
            .collect(),
    };
    if text == "@" {
        options.extend(
            RECURRENCE_OPTIONS.iter().map(|o| mk(format!("Repeat: {}", o.label), o.detail.into(), format!("🔁 {}", o.rule), -1)),
        );
    }
    let query = QUERY_STRIP_RE.replace(text, "").to_lowercase();
    if !query.is_empty() {
        options.retain(|o| o.label.to_lowercase().contains(&query) || o.detail.to_lowercase().contains(&query));
        if options.is_empty() {
            return None;
        }
    }
    Some(TaskCompletions { kind, from, options })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn toggled(doc: &str) -> String {
        match toggle_task_line(doc, &[(doc.len(), doc.len())]) {
            Some(ch) => apply_changes(doc, &ch),
            None => doc.to_string(),
        }
    }

    fn with_token(doc: &str, re: &Regex, emoji: &str, v: &str, t: Option<&str>, cursor: Option<usize>) -> String {
        let e = line_token_edit(doc, cursor.unwrap_or(doc.len()), re, emoji, v, t);
        apply_changes(doc, &[e.change])
    }

    #[test]
    fn turns_a_plain_line_into_a_task() {
        assert_eq!(toggled("Buy milk"), "- [ ] Buy milk");
    }

    #[test]
    fn adds_and_removes_checkbox() {
        assert_eq!(toggled("- item"), "- [ ] item");
        assert_eq!(toggled("- [ ] item"), "- item");
        assert_eq!(toggled("- [x] done"), "- done");
    }

    #[test]
    fn keeps_indentation() {
        assert_eq!(toggled("  nested"), "  - [ ] nested");
    }

    #[test]
    fn appends_a_date_token() {
        assert_eq!(with_token("- [ ] call vet", &DUE_RE, "📅", "2026-08-28", None, None), "- [ ] call vet 📅 2026-08-28");
    }

    #[test]
    fn replaces_existing_date_and_keeps_time() {
        assert_eq!(
            with_token("- [ ] x 📅 2026-01-01 10:00 tail", &DUE_RE, "📅", "2026-02-02", None, Some(3)),
            "- [ ] x 📅 2026-02-02 10:00 tail"
        );
    }

    #[test]
    fn appends_a_recurrence_token_with_a_time() {
        assert_eq!(
            with_token("- [ ] trash", &RECUR_RE, "🔁", "mon,wed,fri", Some("08:00"), None),
            "- [ ] trash 🔁 mon,wed,fri 08:00"
        );
    }

    #[test]
    fn due_date_options_span_a_week() {
        let opts = due_date_options(NaiveDate::from_ymd_opt(2026, 8, 27).unwrap());
        assert_eq!((opts[0].label.as_str(), opts[0].date.as_str()), ("Today", "2026-08-27"));
        assert_eq!((opts[1].label.as_str(), opts[1].date.as_str()), ("Tomorrow", "2026-08-28"));
        assert_eq!(opts[2].label, "Saturday");
        let last = opts.last().unwrap();
        assert_eq!((last.label.as_str(), last.date.as_str()), ("In a week", "2026-09-03"));
    }

    #[test]
    fn completion_trigger_and_filtering() {
        let today = NaiveDate::from_ymd_opt(2026, 8, 27).unwrap();
        let c = task_completions("- [ ] x @due", "- [ ] x @due", today).unwrap();
        assert_eq!(c.kind, TaskKind::Due);
        assert_eq!(&"- [ ] x @due"[c.from..], "@due");
        assert_eq!(c.options[0].insert, "📅 2026-08-27");
        let bare = task_completions("- [ ] x @", "- [ ] x @", today).unwrap();
        assert_eq!(bare.options.len(), 8 + 6);
        let narrowed = task_completions("t 📅 tom", "t 📅 2026-08-01 09:00 📅 tom", today).unwrap();
        assert_eq!(narrowed.options.len(), 1);
        assert_eq!(narrowed.options[0].insert, "📅 2026-08-28 09:00");
        assert!(task_completions("plain text", "plain text", today).is_none());
        assert!(task_trigger("a 🔁"));
    }
}
