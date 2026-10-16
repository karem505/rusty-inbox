//! One running copy per user. A second launch (`rusty-inbox`, `rusty-inbox --compose`,
//! `rusty-inbox mailto:...`) hands its request to the running copy and exits, so a keyboard
//! shortcut or a mailto link always lands in the open window (or brings it back from the tray).
//!
//! Transport: a Unix socket in the runtime dir on Linux/macOS, a loopback TCP port (written to a
//! file next to the data) on Windows. Requests are one line of text: `show`, `compose`, or a
//! `mailto:` URL.

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::time::Duration;

/// What a launch asks for, from its command line.
pub fn request_from_args(args: &[String]) -> String {
    if let Some(m) = args.iter().find(|a| a.to_ascii_lowercase().starts_with("mailto:")) {
        return m.clone();
    }
    if args.iter().any(|a| a == "--compose") {
        return "compose".into();
    }
    "show".into()
}

/// Only these requests are accepted from the socket.
pub fn valid_request(r: &str) -> bool {
    r == "show" || r == "compose" || (r.to_ascii_lowercase().starts_with("mailto:") && r.len() < 8192)
}

fn endpoint() -> PathBuf {
    if let Some(d) = std::env::var_os("RUSTY_INBOX_DATA_DIR") {
        return PathBuf::from(d).join("instance");
    }
    #[cfg(target_os = "linux")]
    if let Some(r) = std::env::var_os("XDG_RUNTIME_DIR") {
        return PathBuf::from(r).join("rusty-inbox.sock");
    }
    crate::engine::data_dir().join("instance")
}

/// Hand `request` to a running copy. True when one took it (this process should exit).
pub fn forward(request: &str) -> bool {
    let msg = format!("{request}\n");
    #[cfg(unix)]
    {
        if let Ok(mut s) = std::os::unix::net::UnixStream::connect(endpoint()) {
            let _ = s.set_write_timeout(Some(Duration::from_secs(2)));
            return s.write_all(msg.as_bytes()).is_ok() && ack(s);
        }
        false
    }
    #[cfg(not(unix))]
    {
        let Some(port) = std::fs::read_to_string(endpoint()).ok().and_then(|p| p.trim().parse::<u16>().ok()) else {
            return false;
        };
        let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
        if let Ok(mut s) = std::net::TcpStream::connect_timeout(&addr, Duration::from_millis(500)) {
            let _ = s.set_write_timeout(Some(Duration::from_secs(2)));
            return s.write_all(msg.as_bytes()).is_ok() && ack(s);
        }
        false
    }
}

fn ack(s: impl std::io::Read) -> bool {
    let mut line = String::new();
    let mut r = BufReader::new(s);
    matches!(r.read_line(&mut line), Ok(n) if n > 0) && line.trim() == "ok"
}

/// Become the running copy: accept requests on a background thread and pass each to `handle`.
pub fn listen(handle: impl Fn(String) + Send + 'static) {
    let path = endpoint();
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    #[cfg(unix)]
    {
        // a stale socket from a crashed run: nobody answers, so take it over
        let _ = std::fs::remove_file(&path);
        let listener = match std::os::unix::net::UnixListener::bind(&path) {
            Ok(l) => l,
            Err(e) => {
                tracing::warn!("single instance: {e}");
                return;
            }
        };
        std::thread::spawn(move || {
            for mut s in listener.incoming().flatten() {
                serve(&mut s, &handle);
            }
        });
    }
    #[cfg(not(unix))]
    {
        let listener = match std::net::TcpListener::bind(("127.0.0.1", 0)) {
            Ok(l) => l,
            Err(e) => {
                tracing::warn!("single instance: {e}");
                return;
            }
        };
        if let Ok(a) = listener.local_addr() {
            let _ = std::fs::write(&path, a.port().to_string());
        }
        std::thread::spawn(move || {
            for mut s in listener.incoming().flatten() {
                serve(&mut s, &handle);
            }
        });
    }
}

fn serve<S: std::io::Read + Write>(s: &mut S, handle: &impl Fn(String)) {
    let mut line = String::new();
    {
        let mut r = BufReader::new(&mut *s);
        if r.read_line(&mut line).is_err() {
            return;
        }
    }
    let req = line.trim().to_string();
    if valid_request(&req) {
        let _ = s.write_all(b"ok\n");
        handle(req);
    } else {
        let _ = s.write_all(b"no\n");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn requests_from_the_command_line() {
        assert_eq!(request_from_args(&args(&["rusty-inbox"])), "show");
        assert_eq!(request_from_args(&args(&["rusty-inbox", "--compose"])), "compose");
        assert_eq!(request_from_args(&args(&["rusty-inbox", "MAILTO:a@b.c?subject=x"])), "MAILTO:a@b.c?subject=x");
        assert!(valid_request("mailto:a@b.c") && valid_request("show") && !valid_request("rm -rf"));
    }

    #[test]
    fn a_second_launch_reaches_the_first() {
        let dir = std::env::temp_dir().join(format!("ri-instance-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::env::set_var("RUSTY_INBOX_DATA_DIR", &dir);
        assert!(!forward("show"), "nobody listening yet");
        let (tx, rx) = std::sync::mpsc::channel();
        listen(move |r| tx.send(r).unwrap());
        std::thread::sleep(Duration::from_millis(100));
        assert!(forward("mailto:someone@example.org?subject=Hi"));
        assert_eq!(rx.recv_timeout(Duration::from_secs(2)).unwrap(), "mailto:someone@example.org?subject=Hi");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
