use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;
use std::{fs, io::Write};

use serde::Serialize;

use tempfile::NamedTempFile;

use crate::events::{CoreEvent, EventSink};

const MAX_READ_BYTES: u64 = 10 * 1024 * 1024; // 10 MB
const BINARY_SNIFF_BYTES: usize = 8 * 1024;

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum ReadResult {
    Text {
        content: String,
        size: u64,
    },
    Binary {
        size: u64,
    },
    /// File exceeds MAX_READ_BYTES. UI decides whether to offer "open anyway".
    TooLarge {
        size: u64,
        limit: u64,
    },
}

#[derive(Serialize)]
#[serde(rename_all = "lowercase")]
pub enum StatKind {
    File,
    Dir,
    Symlink,
}

#[derive(Serialize)]
pub struct FileStat {
    pub size: u64,
    pub mtime: u64,
    pub kind: StatKind,
}

pub fn fs_read_file(path: String) -> Result<ReadResult, String> {
    let p = PathBuf::from(&path);
    let meta = std::fs::metadata(&p).map_err(|e| {
        log::debug!("fs_read_file stat({}) failed: {e}", p.display());
        e.to_string()
    })?;

    let size = meta.len();
    if size > MAX_READ_BYTES {
        return Ok(ReadResult::TooLarge {
            size,
            limit: MAX_READ_BYTES,
        });
    }

    let bytes = std::fs::read(&p).map_err(|e| {
        log::debug!("fs_read_file read({}) failed: {e}", p.display());
        e.to_string()
    })?;

    // Null-byte sniff on the first chunk. Not perfect (misses UTF-16 BOM
    // cases) but catches the common "this is a PNG" mistake cheaply.
    let sniff_len = bytes.len().min(BINARY_SNIFF_BYTES);
    if bytes[..sniff_len].contains(&0) {
        return Ok(ReadResult::Binary { size });
    }

    match String::from_utf8(bytes) {
        Ok(content) => Ok(ReadResult::Text { content, size }),
        Err(_) => Ok(ReadResult::Binary { size }),
    }
}

/// Atomic write via O_EXCL tempfile in the target's parent, then rename.
/// The random suffix is what blocks pre-staged symlink attacks.
pub(crate) fn write_atomic(target: &Path, content: &[u8]) -> std::io::Result<()> {
    let parent = target.parent().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "path has no parent")
    })?;
    let mut tmp = NamedTempFile::new_in(parent)?;
    tmp.as_file_mut().write_all(content)?;
    tmp.as_file_mut().sync_all()?;
    tmp.persist(target).map_err(|e| e.error)?;
    Ok(())
}

pub fn fs_write_file(
    path: String,
    content: String,
    source: Option<String>,
    sink: &dyn EventSink,
) -> Result<(), String> {
    let target = PathBuf::from(&path);
    let original_permissions = fs::metadata(&target).ok().map(|m| m.permissions());
    write_atomic(&target, content.as_bytes()).map_err(|e| {
        log::warn!("fs_write_file({}) failed: {e}", target.display());
        e.to_string()
    })?;

    if let Some(perms) = original_permissions {
        let _ = fs::set_permissions(&target, perms);
    }
    sink.emit(CoreEvent::FileWritten { path, source });

    Ok(())
}

/// [`fs_write_file`] for binary content (e.g. an exported PDF): same
/// atomic write, permission keeping and `fs:file-written` event.
pub fn fs_write_bytes(
    path: String,
    content: &[u8],
    source: Option<String>,
    sink: &dyn EventSink,
) -> Result<(), String> {
    let target = PathBuf::from(&path);
    let original_permissions = fs::metadata(&target).ok().map(|m| m.permissions());
    write_atomic(&target, content).map_err(|e| {
        log::warn!("fs_write_bytes({}) failed: {e}", target.display());
        e.to_string()
    })?;
    if let Some(perms) = original_permissions {
        let _ = fs::set_permissions(&target, perms);
    }
    sink.emit(CoreEvent::FileWritten { path, source });
    Ok(())
}

pub fn fs_canonicalize(path: String) -> Result<String, String> {
    let p = PathBuf::from(&path);
    let canon = std::fs::canonicalize(&p).map_err(|e| e.to_string())?;
    Ok(super::to_canon(&canon))
}

pub fn fs_stat(path: String) -> Result<FileStat, String> {
    let p = PathBuf::from(&path);
    let meta = std::fs::metadata(&p).map_err(|e| e.to_string())?;
    let kind = if meta.is_dir() {
        StatKind::Dir
    } else if meta.file_type().is_symlink() {
        StatKind::Symlink
    } else {
        StatKind::File
    };
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    Ok(FileStat {
        size: meta.len(),
        mtime,
        kind,
    })
}


/// Writes raw bytes (base64-encoded over IPC) — used for pasted/dropped
/// image attachments. Creates the parent directory as needed and refuses to
/// overwrite an existing file so attachments are never clobbered.
pub fn fs_write_binary(path: String, data_base64: String) -> Result<(), String> {
    use base64::Engine;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(data_base64.as_bytes())
        .map_err(|e| format!("invalid base64: {e}"))?;
    fs_write_binary_bytes(&path, &bytes)
}

/// Native-API variant of [`fs_write_binary`] taking raw bytes (no base64 hop).
/// Note the original validated existence before decoding; both orders fail
/// the same way for valid input.
pub fn fs_write_binary_bytes(path: &str, bytes: &[u8]) -> Result<(), String> {
    let target = PathBuf::from(&path);
    if target.exists() {
        return Err(format!("already exists: {}", target.display()));
    }
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    write_atomic(&target, bytes).map_err(|e| {
        log::warn!("fs_write_binary({}) failed: {e}", target.display());
        e.to_string()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::NullSink;

    #[test]
    fn read_file_classifies_utf8_as_text() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("a.txt");
        std::fs::write(&f, b"hello world").unwrap();
        match fs_read_file(f.to_string_lossy().into_owned()).unwrap() {
            ReadResult::Text { content, size } => {
                assert_eq!(content, "hello world");
                assert_eq!(size, 11);
            }
            _ => panic!("expected text"),
        }
    }

    #[test]
    fn read_file_detects_binary_via_null_byte() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("a.bin");
        std::fs::write(&f, b"PNG\0\x89image").unwrap();
        assert!(matches!(
            fs_read_file(f.to_string_lossy().into_owned()).unwrap(),
            ReadResult::Binary { .. }
        ));
    }

    #[test]
    fn read_file_detects_binary_via_invalid_utf8() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("a.bin");
        // Invalid UTF-8 with no null byte: must still classify as binary.
        std::fs::write(&f, [0xff, 0xfe, 0xfd, 0xfc]).unwrap();
        assert!(matches!(
            fs_read_file(f.to_string_lossy().into_owned()).unwrap(),
            ReadResult::Binary { .. }
        ));
    }

    #[test]
    fn overwrites_existing_target() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("note.txt");
        std::fs::write(&target, b"old").unwrap();
        write_atomic(&target, b"new").unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"new");
    }

    #[cfg(unix)]
    #[test]
    fn does_not_follow_legacy_staging_symlink() {
        use std::os::unix::fs::symlink;
        let dir = tempfile::tempdir().unwrap();
        let outside = dir.path().join("outside.txt");
        std::fs::write(&outside, b"untouched").unwrap();

        let target = dir.path().join("note.txt");
        // Pre-stage a symlink at the legacy deterministic staging path.
        let legacy = dir.path().join(".note.txt.helix.tmp");
        symlink(&outside, &legacy).unwrap();

        write_atomic(&target, b"payload").unwrap();

        assert_eq!(std::fs::read(&target).unwrap(), b"payload");
        // The pre-staged symlink target must not have been written through.
        assert_eq!(std::fs::read(&outside).unwrap(), b"untouched");
    }

    fn s(p: &std::path::Path) -> String {
        p.to_string_lossy().into_owned()
    }

    #[test]
    fn read_file_reports_too_large_without_reading() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("big.md");
        // Sparse file: cheap to create, larger than the 10 MiB cap.
        let file = std::fs::File::create(&f).unwrap();
        file.set_len(MAX_READ_BYTES + 1).unwrap();
        match fs_read_file(s(&f)).unwrap() {
            ReadResult::TooLarge { size, limit } => {
                assert_eq!(size, 10 * 1024 * 1024 + 1);
                assert_eq!(limit, 10_485_760);
            }
            _ => panic!("expected toolarge"),
        }
        let json = serde_json::to_value(fs_read_file(s(&f)).unwrap()).unwrap();
        assert_eq!(json, serde_json::json!({"kind": "toolarge", "size": 10_485_761u64, "limit": 10_485_760u64}));
    }

    #[test]
    fn read_file_at_exactly_the_limit_is_read() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("edge.md");
        let file = std::fs::File::create(&f).unwrap();
        file.set_len(MAX_READ_BYTES).unwrap();
        // All NULs, so it is read and classified as binary (not toolarge).
        assert!(matches!(fs_read_file(s(&f)).unwrap(), ReadResult::Binary { size } if size == MAX_READ_BYTES));
    }

    #[test]
    fn read_file_missing_is_an_io_error_string() {
        let dir = tempfile::tempdir().unwrap();
        let err = fs_read_file(s(&dir.path().join("nope.md"))).err().unwrap();
        assert!(err.contains("No such file or directory"), "{err}");
    }

    #[test]
    fn read_file_text_json_shape() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("a.md");
        std::fs::write(&f, "hi").unwrap();
        let json = serde_json::to_value(fs_read_file(s(&f)).unwrap()).unwrap();
        assert_eq!(json, serde_json::json!({"kind": "text", "content": "hi", "size": 2}));
    }

    #[test]
    fn write_file_writes_and_emits_file_written_with_path_as_passed() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("note.md");
        let (tx, rx) = crossbeam_channel::unbounded();
        fs_write_file(s(&f), "body".into(), Some("editor".into()), &tx).unwrap();
        assert_eq!(std::fs::read_to_string(&f).unwrap(), "body");
        assert_eq!(rx.try_recv().unwrap(), CoreEvent::FileWritten { path: s(&f), source: Some("editor".into()) });
        assert!(rx.try_recv().is_err());

        // No source -> omitted from the payload.
        fs_write_file(s(&f), "again".into(), None, &tx).unwrap();
        let ev = rx.try_recv().unwrap();
        assert_eq!(serde_json::to_value(&ev).unwrap(), serde_json::json!({"path": s(&f)}));
    }

    #[test]
    fn write_file_missing_parent_fails_without_event() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("no/such/dir/note.md");
        let (tx, rx) = crossbeam_channel::unbounded();
        assert!(fs_write_file(s(&f), "x".into(), None, &tx).is_err());
        assert!(!f.exists());
        assert!(rx.try_recv().is_err());
    }

    #[cfg(unix)]
    #[test]
    fn write_file_restores_permissions_and_new_files_are_0600() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("note.md");
        std::fs::write(&f, "old").unwrap();
        std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o644)).unwrap();
        fs_write_file(s(&f), "new".into(), None, &NullSink).unwrap();
        assert_eq!(std::fs::metadata(&f).unwrap().permissions().mode() & 0o777, 0o644);

        let fresh = dir.path().join("fresh.md");
        fs_write_file(s(&fresh), "x".into(), None, &NullSink).unwrap();
        assert_eq!(std::fs::metadata(&fresh).unwrap().permissions().mode() & 0o777, 0o600);
    }

    #[test]
    fn stat_reports_kind_size_and_mtime() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("a.md");
        std::fs::write(&f, "12345").unwrap();
        let st = fs_stat(s(&f)).unwrap();
        assert!(matches!(st.kind, StatKind::File));
        assert_eq!(st.size, 5);
        assert!(st.mtime > 1_600_000_000_000, "mtime is ms since epoch: {}", st.mtime);

        let d = fs_stat(s(dir.path())).unwrap();
        assert!(matches!(d.kind, StatKind::Dir));
        let json = serde_json::to_value(&d).unwrap();
        assert_eq!(json["kind"], "dir");

        assert!(fs_stat(s(&dir.path().join("missing"))).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn stat_follows_symlinks() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("t.md");
        std::fs::write(&target, "abc").unwrap();
        let link = dir.path().join("l.md");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        let st = fs_stat(s(&link)).unwrap();
        assert!(matches!(st.kind, StatKind::File));
        assert_eq!(st.size, 3);
        // Broken link: metadata() fails -> Err (used as an existence check).
        std::fs::remove_file(&target).unwrap();
        assert!(fs_stat(s(&link)).is_err());
    }

    #[test]
    fn write_binary_decodes_creates_parents_and_never_clobbers() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("attachments/deep/img.png");
        fs_write_binary(s(&f), "AAEC/w==".into()).unwrap();
        assert_eq!(std::fs::read(&f).unwrap(), vec![0u8, 1, 2, 255]);

        let err = fs_write_binary(s(&f), "AAEC/w==".into()).unwrap_err();
        assert_eq!(err, format!("already exists: {}", f.display()));
        assert_eq!(std::fs::read(&f).unwrap(), vec![0u8, 1, 2, 255]);

        let bad = dir.path().join("bad.png");
        let err = fs_write_binary(s(&bad), "not base64!".into()).unwrap_err();
        assert!(err.starts_with("invalid base64: "), "{err}");
        assert!(!bad.exists());
        // STANDARD alphabet requires padding.
        assert!(fs_write_binary(s(&bad), "AAEC/w".into()).unwrap_err().starts_with("invalid base64: "));
    }

    #[cfg(unix)]
    #[test]
    fn write_binary_files_are_0600() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("img.png");
        fs_write_binary(s(&f), "AA==".into()).unwrap();
        assert_eq!(std::fs::metadata(&f).unwrap().permissions().mode() & 0o777, 0o600);
    }

    #[cfg(unix)]
    #[test]
    fn canonicalize_resolves_symlinks_and_requires_existence() {
        let dir = tempfile::tempdir().unwrap();
        let real = std::fs::canonicalize(dir.path()).unwrap().join("real.md");
        std::fs::write(&real, "").unwrap();
        let link = dir.path().join("link.md");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        assert_eq!(fs_canonicalize(s(&link)).unwrap(), s(&real));
        assert!(fs_canonicalize(s(&dir.path().join("missing"))).is_err());
    }
}
