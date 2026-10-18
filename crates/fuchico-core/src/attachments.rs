//! Pure parts of `src/modules/editor/lib/imageAttachments.ts` and
//! `pathDrop.ts`: attachment naming, relative links, and the Markdown to
//! insert when a file is dropped on a note.

use std::sync::LazyLock;

use chrono::NaiveDateTime;
use regex::Regex;

use crate::fs::file::fs_write_binary_bytes;

static SANITIZE_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[^A-Za-z0-9_.\-]+").unwrap());
static GENERIC_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^(image|screenshot|pasted|clipboard)").unwrap());
static STRIP_EXT_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\.[^.]+$").unwrap());
static IMAGE_EXT_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\.(png|jpe?g|gif|webp|svg|bmp)$").unwrap());
static MD_EXT_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\.md$").unwrap());

pub fn ext_for_mime(mime: &str) -> Option<&'static str> {
    Some(match mime {
        "image/png" => "png",
        "image/jpeg" => "jpg",
        "image/gif" => "gif",
        "image/webp" => "webp",
        "image/svg+xml" => "svg",
        "image/bmp" => "bmp",
        _ => return None,
    })
}

/// `YYYYMMDD-HHMMSS`.
pub fn timestamp(d: &NaiveDateTime) -> String {
    d.format("%Y%m%d-%H%M%S").to_string()
}

fn sanitize(name: &str) -> String {
    SANITIZE_RE.replace_all(name, "-").trim_matches('-').to_string()
}

/// On-disk name for a pasted/dropped image: keep a meaningful original
/// filename, but generic clipboard names get a timestamp.
pub fn attachment_name(file_name: &str, mime: &str, now: &NaiveDateTime) -> String {
    let ext = ext_for_mime(mime).map(String::from).unwrap_or_else(|| file_name.rsplit('.').next().unwrap_or("png").to_string());
    let base = sanitize(&STRIP_EXT_RE.replace(file_name, ""));
    let generic = base.is_empty() || GENERIC_RE.is_match(&base);
    if generic { format!("pasted-{}.{ext}", timestamp(now)) } else { format!("{base}.{ext}") }
}

fn dirname(p: &str) -> &str {
    match p.rfind('/') {
        Some(i) if i > 0 => &p[..i],
        _ => "/",
    }
}

/// Relative path from `from_dir` to `to_path`, POSIX-style.
pub fn relative_path(from_dir: &str, to_path: &str) -> String {
    let a: Vec<&str> = from_dir.split('/').filter(|s| !s.is_empty()).collect();
    let b: Vec<&str> = to_path.split('/').filter(|s| !s.is_empty()).collect();
    let mut i = 0;
    while i < a.len() && i < b.len() && a[i] == b[i] {
        i += 1;
    }
    let mut parts: Vec<&str> = vec![".."; a.len() - i];
    parts.extend(&b[i..]);
    parts.join("/")
}

pub struct SavedImage {
    pub path: String,
    /// File name without extension (the link's alt text).
    pub name: String,
}

/// Saves image bytes into `dir`, suffixing a timestamp if the name is taken
/// (`saveImage`).
pub fn save_image(bytes: &[u8], file_name: &str, mime: &str, dir: &str, now: &NaiveDateTime) -> Result<SavedImage, String> {
    let name = attachment_name(file_name, mime, now);
    let mut path = format!("{dir}/{name}");
    if let Err(e) = fs_write_binary_bytes(&path, bytes) {
        if !e.contains("already exists") {
            return Err(e);
        }
        let (stem, ext) = match name.rfind('.') {
            Some(dot) => (&name[..dot], &name[dot..]),
            None => (name.as_str(), ""),
        };
        path = format!("{dir}/{stem}-{}{ext}", timestamp(now));
        fs_write_binary_bytes(&path, bytes)?;
    }
    Ok(SavedImage { path, name: STRIP_EXT_RE.replace(&name, "").into_owned() })
}

/// Markdown image link for a saved attachment, relative to the note.
pub fn image_link(saved: &SavedImage, current_path: &str) -> String {
    format!("![{}]({})", saved.name, relative_path(dirname(current_path), &saved.path))
}

/// Markdown to insert for a vault file dragged from the explorer into the
/// note at `current_path`: notes become `[[wikilinks]]`, images embed,
/// anything else becomes a relative link.
pub fn link_for_dropped_path(path: &str, current_path: &str) -> String {
    let name = &path[path.rfind('/').map_or(0, |i| i + 1)..];
    if MD_EXT_RE.is_match(name) {
        return format!("[[{}]]", MD_EXT_RE.replace(name, ""));
    }
    let rel = relative_path(dirname(current_path), path);
    let label = STRIP_EXT_RE.replace(name, "");
    if IMAGE_EXT_RE.is_match(name) { format!("![{label}]({rel})") } else { format!("[{label}]({rel})") }
}

/// Text to insert for dropped `paths`, one link per line; prefixed with a
/// space when glued to a non-whitespace char (`char_before` the drop point).
pub fn drop_insert_text(paths: &[String], current_path: &str, char_before: Option<char>) -> String {
    let links = paths.iter().map(|p| link_for_dropped_path(p, current_path)).collect::<Vec<_>>().join("\n");
    match char_before {
        Some(c) if !c.is_whitespace() => format!(" {links}"),
        _ => links,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    fn now() -> NaiveDateTime {
        NaiveDate::from_ymd_opt(2026, 8, 27).unwrap().and_hms_opt(9, 5, 7).unwrap()
    }

    #[test]
    fn relative_path_links_from_notes_folder_to_attachments() {
        assert_eq!(relative_path("/v", "/v/attachments/a.png"), "attachments/a.png");
        assert_eq!(relative_path("/v/sub/deep", "/v/attachments/a.png"), "../../attachments/a.png");
        assert_eq!(relative_path("/v/attachments", "/v/attachments/a.png"), "a.png");
    }

    #[test]
    fn dropped_paths_become_links() {
        let here = "/vault/notes/today.md";
        assert_eq!(link_for_dropped_path("/vault/sub/Inner Note.md", here), "[[Inner Note]]");
        assert_eq!(link_for_dropped_path("/vault/attachments/pic.png", here), "![pic](../attachments/pic.png)");
        assert_eq!(link_for_dropped_path("/vault/notes/spec.pdf", here), "[spec](spec.pdf)");
    }

    #[test]
    fn drop_insert_separates_from_adjacent_text() {
        let p = vec!["/v/a.md".to_string(), "/v/b.md".to_string()];
        assert_eq!(drop_insert_text(&p, "/v/c.md", Some('x')), " [[a]]\n[[b]]");
        assert_eq!(drop_insert_text(&p, "/v/c.md", Some(' ')), "[[a]]\n[[b]]");
        assert_eq!(drop_insert_text(&p, "/v/c.md", None), "[[a]]\n[[b]]");
    }

    #[test]
    fn attachment_names() {
        assert_eq!(attachment_name("image.png", "image/png", &now()), "pasted-20260827-090507.png");
        assert_eq!(attachment_name("Screenshot 2026.png", "image/jpeg", &now()), "pasted-20260827-090507.jpg");
        assert_eq!(attachment_name("my photo!.jpeg", "", &now()), "my-photo.jpeg");
    }

    #[test]
    fn save_image_suffixes_on_collision() {
        let dir = tempfile::tempdir().unwrap();
        let d = dir.path().join("att").to_string_lossy().into_owned();
        let a = save_image(b"1", "cat.png", "image/png", &d, &now()).unwrap();
        let b = save_image(b"2", "cat.png", "image/png", &d, &now()).unwrap();
        assert!(a.path.ends_with("/att/cat.png"));
        assert!(b.path.ends_with("/att/cat-20260827-090507.png"));
        assert_eq!(image_link(&a, &format!("{d}/../n.md")), format!("![cat]({})", relative_path(&format!("{d}/.."), &a.path)));
    }
}
