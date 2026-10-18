//! Single instance (replaces `tauri_plugin_single_instance`): the first
//! process listens on a Unix socket in the config dir; a second launch
//! sends its argv file paths (resolved against its own cwd) as one
//! newline-separated message and exits.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;

fn socket_path() -> Option<PathBuf> {
    crate::persist::config_dir().map(|d| d.join("fuchico.sock"))
}

/// Outcome of [`acquire`].
pub enum Instance {
    /// We are the primary instance; incoming batches arrive on the listener.
    Primary(Option<UnixListener>),
    /// Another instance took our paths; exit now.
    Forwarded,
}

/// Tries to hand `paths` to a running instance; otherwise becomes primary.
pub fn acquire(paths: &[String]) -> Instance {
    let Some(sock) = socket_path() else { return Instance::Primary(None) };
    if let Ok(mut stream) = UnixStream::connect(&sock) {
        let msg = encode(paths);
        if stream.write_all(msg.as_bytes()).is_ok() {
            return Instance::Forwarded;
        }
    }
    // Stale socket from a crash, or none: take it over.
    let _ = std::fs::remove_file(&sock);
    if let Some(dir) = sock.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    match UnixListener::bind(&sock) {
        Ok(l) => Instance::Primary(Some(l)),
        Err(e) => {
            log::warn!("single instance: bind {}: {e}", sock.display());
            Instance::Primary(None)
        }
    }
}

/// Serves forwarded launches on a background thread, calling `on_paths`
/// for each batch (may be empty: a bare relaunch just focuses).
pub fn serve(listener: UnixListener, on_paths: impl Fn(Vec<String>) + Send + 'static) {
    std::thread::Builder::new()
        .name("fuchico-single-instance".into())
        .spawn(move || {
            for stream in listener.incoming().flatten() {
                on_paths(decode(BufReader::new(stream)));
            }
        })
        .ok();
}

/// Removes the socket on quit.
pub fn release() {
    if let Some(sock) = socket_path() {
        let _ = std::fs::remove_file(sock);
    }
}

fn encode(paths: &[String]) -> String {
    let mut s = paths.join("\n");
    s.push('\n');
    s
}

fn decode(r: impl BufRead) -> Vec<String> {
    r.lines().map_while(Result::ok).filter(|l| !l.is_empty()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wire_format_round_trips() {
        let paths = vec!["/v/a b.md".to_string(), "/x/c.md".to_string()];
        assert_eq!(decode(std::io::Cursor::new(encode(&paths))), paths);
        assert!(decode(std::io::Cursor::new(encode(&[]))).is_empty());
    }
}
