//! Single instance (replaces `tauri_plugin_single_instance`): the first
//! process listens for later launches; a second launch sends its argv file
//! paths (resolved against its own cwd) as one newline-separated message
//! and exits.
//!
//! - Unix: a Unix socket `fuchico.sock` in the config dir.
//! - Windows: a TCP listener on `127.0.0.1` (loopback only, so no firewall
//!   prompt) whose port and a random token sit in `fuchico.port` in the
//!   config dir. The second launch must present the token, and it only exits
//!   once the primary acknowledges, so a stale port file (after a crash, or
//!   a port reused by another program) never swallows a launch.

use std::io::BufRead;

/// Outcome of [`acquire`].
pub enum Instance<L = Listener> {
    /// We are the primary instance; incoming batches arrive on the listener.
    Primary(Option<L>),
    /// Another instance took our paths; exit now.
    Forwarded,
}

#[cfg(unix)]
pub use unix_socket::{Listener, acquire, release, serve};

#[cfg(windows)]
pub use loopback::{Listener, acquire, release, serve};

#[cfg(unix)]
mod unix_socket {
    use std::io::{BufReader, Write};
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::path::PathBuf;

    use super::{Instance, decode, encode};

    pub type Listener = UnixListener;

    fn socket_path() -> Option<PathBuf> {
        crate::persist::config_dir().map(|d| d.join("fuchico.sock"))
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
}

/// Loopback TCP transport. Built on Windows, and in unit tests everywhere so
/// the protocol is exercised on the dev machines too.
#[cfg(any(windows, test))]
mod loopback {
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::{Ipv4Addr, Shutdown, SocketAddr, TcpListener, TcpStream};
    use std::path::{Path, PathBuf};
    use std::time::Duration;

    use super::{Instance, decode, encode};

    const PORT_FILE: &str = "fuchico.port";
    const ACK: &str = "ok";
    const TIMEOUT: Duration = Duration::from_millis(1500);

    pub struct Listener {
        tcp: TcpListener,
        token: String,
    }

    fn port_file(dir: &Path) -> PathBuf {
        dir.join(PORT_FILE)
    }

    /// Tries to hand `paths` to a running instance; otherwise becomes primary.
    #[cfg(windows)]
    pub fn acquire(paths: &[String]) -> Instance {
        match crate::persist::config_dir() {
            Some(dir) => acquire_in(&dir, paths),
            None => Instance::Primary(None),
        }
    }

    /// Removes the port file on quit.
    #[cfg(windows)]
    pub fn release() {
        if let Some(dir) = crate::persist::config_dir() {
            release_in(&dir);
        }
    }

    /// Tries to hand `paths` to the instance recorded in `dir`; otherwise
    /// binds a fresh loopback port and records it there.
    pub fn acquire_in(dir: &Path, paths: &[String]) -> Instance<Listener> {
        if forward(dir, paths).is_some() {
            return Instance::Forwarded;
        }
        match listen(dir) {
            Ok(l) => Instance::Primary(Some(l)),
            Err(e) => {
                log::warn!("single instance: listen in {}: {e}", dir.display());
                Instance::Primary(None)
            }
        }
    }

    /// `Some` only when a primary accepted the token and acknowledged.
    fn forward(dir: &Path, paths: &[String]) -> Option<()> {
        let raw = std::fs::read_to_string(port_file(dir)).ok()?;
        let (port, token) = raw.trim().split_once(' ')?;
        let port: u16 = port.parse().ok()?;
        let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
        let mut stream = TcpStream::connect_timeout(&addr, TIMEOUT).ok()?;
        stream.set_read_timeout(Some(TIMEOUT)).ok()?;
        stream.set_write_timeout(Some(TIMEOUT)).ok()?;
        stream
            .write_all(format!("{token}\n{}", encode(paths)).as_bytes())
            .ok()?;
        stream.shutdown(Shutdown::Write).ok()?;
        let mut reply = String::new();
        stream.take(16).read_to_string(&mut reply).ok()?;
        (reply.trim() == ACK).then_some(())
    }

    fn listen(dir: &Path) -> std::io::Result<Listener> {
        std::fs::create_dir_all(dir)?;
        let tcp = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
        let port = tcp.local_addr()?.port();
        let token = format!("{:032x}", rand::random::<u128>());
        // Write then rename, so a concurrent launch never reads half a file.
        let tmp = dir.join(format!("{PORT_FILE}.{}", std::process::id()));
        std::fs::write(&tmp, format!("{port} {token}\n"))?;
        std::fs::rename(&tmp, port_file(dir))?;
        Ok(Listener { tcp, token })
    }

    /// Serves forwarded launches on a background thread, calling `on_paths`
    /// for each authenticated batch (may be empty: a bare relaunch just
    /// focuses).
    pub fn serve(listener: Listener, on_paths: impl Fn(Vec<String>) + Send + 'static) {
        std::thread::Builder::new()
            .name("fuchico-single-instance".into())
            .spawn(move || {
                for stream in listener.tcp.incoming().flatten() {
                    if let Some(paths) = handle(stream, &listener.token) {
                        on_paths(paths);
                    }
                }
            })
            .ok();
    }

    fn handle(mut stream: TcpStream, token: &str) -> Option<Vec<String>> {
        stream.set_read_timeout(Some(TIMEOUT)).ok()?;
        let mut reader = BufReader::new(stream.try_clone().ok()?);
        let mut first = String::new();
        reader.read_line(&mut first).ok()?;
        if first.trim_end() != token {
            return None;
        }
        let paths = decode(reader);
        stream.write_all(format!("{ACK}\n").as_bytes()).ok()?;
        Some(paths)
    }

    pub fn release_in(dir: &Path) {
        let _ = std::fs::remove_file(port_file(dir));
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
    use std::sync::mpsc;
    use std::time::Duration;

    #[test]
    fn wire_format_round_trips() {
        let paths = vec!["/v/a b.md".to_string(), "/x/c.md".to_string()];
        assert_eq!(decode(std::io::Cursor::new(encode(&paths))), paths);
        assert!(decode(std::io::Cursor::new(encode(&[]))).is_empty());
    }

    #[test]
    fn loopback_forwards_to_primary_and_rejects_a_forged_token() {
        let dir = tempfile::tempdir().unwrap();
        let Instance::Primary(Some(listener)) = loopback::acquire_in(dir.path(), &[]) else {
            panic!("first launch must be primary");
        };
        let (tx, rx) = mpsc::channel();
        loopback::serve(listener, move |p| tx.send(p).unwrap());

        let paths = vec!["C:/v/a b.md".to_string(), "C:/x/c.md".to_string()];
        assert!(matches!(loopback::acquire_in(dir.path(), &paths), Instance::Forwarded));
        assert_eq!(rx.recv_timeout(Duration::from_secs(3)).unwrap(), paths);
        // A bare relaunch forwards an empty batch (focus only).
        assert!(matches!(loopback::acquire_in(dir.path(), &[]), Instance::Forwarded));
        assert_eq!(rx.recv_timeout(Duration::from_secs(3)).unwrap(), Vec::<String>::new());

        // A wrong token is ignored, and the launch becomes primary instead.
        let port_file = dir.path().join("fuchico.port");
        let raw = std::fs::read_to_string(&port_file).unwrap();
        let port = raw.split(' ').next().unwrap().to_string();
        std::fs::write(&port_file, format!("{port} forged\n")).unwrap();
        assert!(matches!(loopback::acquire_in(dir.path(), &paths), Instance::Primary(Some(_))));
        assert!(rx.recv_timeout(Duration::from_millis(300)).is_err());

        loopback::release_in(dir.path());
        assert!(!port_file.exists());
    }

    #[test]
    fn loopback_stale_or_garbled_port_file_becomes_primary() {
        let dir = tempfile::tempdir().unwrap();
        let port_file = dir.path().join("fuchico.port");
        // Grab a free port, then close it so nothing listens there.
        let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        std::fs::write(&port_file, format!("{port} abc\n")).unwrap();
        assert!(matches!(loopback::acquire_in(dir.path(), &[]), Instance::Primary(Some(_))));
        std::fs::write(&port_file, "garbage").unwrap();
        assert!(matches!(loopback::acquire_in(dir.path(), &[]), Instance::Primary(Some(_))));
    }
}
