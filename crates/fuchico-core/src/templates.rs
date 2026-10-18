//! Port of `src/modules/palette/lib/templates.ts`: note templates and daily
//! notes (issue #26). `{{date}}`, `{{time}}`, `{{title}}` and
//! `{{date:FORMAT}}` tokens are expanded on use.

use std::sync::LazyLock;

use chrono::{Datelike, NaiveDateTime, Timelike};
use regex::{Captures, Regex};

use crate::events::{CoreEvent, EventSink};
use crate::fs::file::{ReadResult, fs_read_file, fs_write_file};
use crate::fs::mutate::fs_create_dir;

/// Template file reserved for seeding daily notes.
pub const DAILY_TEMPLATE: &str = "daily.md";

/// Supports `YYYY MM DD HH mm ss`, replaced in that order like the TS.
pub fn format_date(d: &NaiveDateTime, fmt: &str) -> String {
    fmt.replace("YYYY", &d.year().to_string())
        .replace("MM", &format!("{:02}", d.month()))
        .replace("DD", &format!("{:02}", d.day()))
        .replace("HH", &format!("{:02}", d.hour()))
        .replace("mm", &format!("{:02}", d.minute()))
        .replace("ss", &format!("{:02}", d.second()))
}

pub fn format_date_default(d: &NaiveDateTime) -> String {
    format_date(d, "YYYY-MM-DD")
}

static DATE_FMT_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\{\{\s*date:([^}]+?)\s*\}\}").unwrap());
static DATE_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\{\{\s*date\s*\}\}").unwrap());
static TIME_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\{\{\s*time\s*\}\}").unwrap());
static TITLE_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\{\{\s*title\s*\}\}").unwrap());

pub fn expand_template(text: &str, title: &str, now: &NaiveDateTime) -> String {
    let s = DATE_FMT_RE.replace_all(text, |c: &Captures| format_date(now, c[1].trim()));
    let s = DATE_RE.replace_all(&s, |_: &Captures| format_date_default(now));
    let s = TIME_RE.replace_all(&s, |_: &Captures| format_date(now, "HH:mm"));
    TITLE_RE
        .replace_all(&s, |_: &Captures| title.to_string())
        .into_owned()
}

pub fn basename(path: &str) -> &str {
    &path[path.rfind('/').map_or(0, |i| i + 1)..]
}

/// File name without its last extension (`/a/b.tar.gz` -> `b.tar`).
pub fn stem(path: &str) -> String {
    let b = basename(path);
    match b.rfind('.') {
        // JS `/\.[^.]+$/` needs at least one char after the dot.
        Some(i) if i + 1 < b.len() => b[..i].to_string(),
        _ => b.to_string(),
    }
}

fn trim_slashes(s: &str) -> &str {
    s.trim_matches('/')
}

/// Absolute paths of template files among `vault_files`, sorted.
pub fn list_templates<S: AsRef<str>>(
    root: &str,
    templates_folder: &str,
    vault_files: &[S],
) -> Vec<String> {
    let prefix = format!("{root}/{}/", trim_slashes(templates_folder));
    let mut out: Vec<String> = vault_files
        .iter()
        .map(|f| f.as_ref())
        .filter(|f| f.starts_with(&prefix) && f.ends_with(".md") && basename(f) != DAILY_TEMPLATE)
        .map(String::from)
        .collect();
    out.sort();
    out
}

fn exists(path: &str) -> bool {
    crate::fs::file::fs_stat(path.to_string()).is_ok()
}

fn read_text(path: &str) -> Option<String> {
    match fs_read_file(path.to_string()) {
        Ok(ReadResult::Text { content, .. }) => Some(content),
        _ => None,
    }
}

fn ensure_dir(path: &str) -> Result<(), String> {
    match fs_create_dir(path.to_string()) {
        Err(e) if !e.contains("already exists") => Err(e),
        _ => Ok(()),
    }
}

fn create_note(path: &str, content: String, sink: &dyn EventSink) -> Result<(), String> {
    let dir = &path[..path.rfind('/').unwrap_or(0)];
    ensure_dir(dir)?;
    fs_write_file(
        path.to_string(),
        content,
        Some("template".to_string()),
        sink,
    )?;
    // `notifyFsChanged(dir)`
    sink.emit(CoreEvent::FsChanged {
        dir: dir.to_string(),
        from: None,
    });
    Ok(())
}

/// Opens (creating on first use) today's daily note and returns its path,
/// seeded from `<templates>/daily.md` when present.
pub fn open_daily_note(
    root: &str,
    daily_notes_folder: &str,
    templates_folder: &str,
    now: &NaiveDateTime,
    sink: &dyn EventSink,
) -> Result<String, String> {
    let title = format_date_default(now);
    let folder = trim_slashes(daily_notes_folder);
    let path = format!("{root}/{folder}/{title}.md");
    if exists(&path) {
        return Ok(path);
    }
    let template = read_text(&format!(
        "{root}/{}/{DAILY_TEMPLATE}",
        trim_slashes(templates_folder)
    ));
    let content = expand_template(
        template.as_deref().unwrap_or("# {{title}}\n\n"),
        &title,
        now,
    );
    create_note(&path, content, sink)?;
    Ok(path)
}

/// Creates a new note in the vault root from `template_path`, named after
/// the template plus a timestamp.
pub fn create_from_template(
    root: &str,
    template_path: &str,
    now: &NaiveDateTime,
    sink: &dyn EventSink,
) -> Result<String, String> {
    let template = read_text(template_path).unwrap_or_default();
    let title = format!(
        "{} {}",
        stem(template_path),
        format_date(now, "YYYY-MM-DD HHmm")
    );
    let mut path = format!("{root}/{title}.md");
    let mut n = 2;
    while exists(&path) {
        path = format!("{root}/{title} {n}.md");
        n += 1;
    }
    create_note(&path, expand_template(&template, &title, now), sink)?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::NullSink;
    use chrono::NaiveDate;

    fn now() -> NaiveDateTime {
        NaiveDate::from_ymd_opt(2026, 8, 27)
            .unwrap()
            .and_hms_opt(9, 5, 7)
            .unwrap()
    }

    #[test]
    fn formats_dates_with_the_supported_tokens() {
        assert_eq!(format_date_default(&now()), "2026-08-27");
        assert_eq!(format_date(&now(), "DD/MM/YYYY HH:mm"), "27/08/2026 09:05");
    }

    #[test]
    fn expands_template_variables() {
        let out = expand_template(
            "# {{title}}\n{{date}} {{ time }} {{date:YYYY}} {{unknown}}",
            "T",
            &now(),
        );
        assert_eq!(out, "# T\n2026-08-27 09:05 2026 {{unknown}}");
    }

    #[test]
    fn lists_markdown_templates_excluding_reserved_daily() {
        let files = [
            "/v/templates/daily.md",
            "/v/templates/meeting.md",
            "/v/templates/notes.txt",
            "/v/notes.md",
            "/v/templatesX/a.md",
        ];
        assert_eq!(
            list_templates("/v", "/templates/", &files),
            vec!["/v/templates/meeting.md"]
        );
        assert_eq!(stem("/v/templates/meeting.md"), "meeting");
    }

    #[test]
    fn daily_note_is_created_from_template_once() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_string_lossy().into_owned();
        std::fs::create_dir(dir.path().join("templates")).unwrap();
        std::fs::write(dir.path().join("templates/daily.md"), "Day {{date}}\n").unwrap();
        let (tx, rx) = crossbeam_channel::unbounded();
        let p = open_daily_note(&root, "daily", "templates", &now(), &tx).unwrap();
        assert!(p.ends_with("daily/2026-08-27.md"));
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "Day 2026-08-27\n");
        assert!(
            rx.try_iter()
                .any(|e| matches!(e, CoreEvent::FsChanged { .. }))
        );
        // Second call does not rewrite.
        std::fs::write(&p, "edited").unwrap();
        open_daily_note(&root, "daily", "templates", &now(), &NullSink).unwrap();
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "edited");
    }

    #[test]
    fn create_from_template_avoids_collisions() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_string_lossy().into_owned();
        let tpl = dir.path().join("meeting.md");
        std::fs::write(&tpl, "# {{title}}").unwrap();
        let tpl = tpl.to_string_lossy().into_owned();
        let a = create_from_template(&root, &tpl, &now(), &NullSink).unwrap();
        let b = create_from_template(&root, &tpl, &now(), &NullSink).unwrap();
        assert!(a.ends_with("meeting 2026-08-27 0905.md"));
        assert!(b.ends_with("meeting 2026-08-27 0905 2.md"));
        assert_eq!(
            std::fs::read_to_string(&a).unwrap(),
            "# meeting 2026-08-27 0905"
        );
    }
}
