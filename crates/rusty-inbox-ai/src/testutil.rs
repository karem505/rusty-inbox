//! Test-only local HTTP mock: a `TcpListener` on 127.0.0.1:0 that records every request and answers
//! from a script or a handler. No network is ever used by `cargo test`.

use serde_json::Value;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// One captured request.
#[derive(Clone, Debug)]
pub struct Req {
    pub method: String,
    /// path and query
    pub path: String,
    /// header names lower-cased
    pub headers: Vec<(String, String)>,
    pub body: String,
}

impl Req {
    pub fn header(&self, name: &str) -> Option<&str> {
        let n = name.to_ascii_lowercase();
        self.headers.iter().find(|(k, _)| *k == n).map(|(_, v)| v.as_str())
    }
    pub fn json(&self) -> Value {
        serde_json::from_str(&self.body).unwrap_or_else(|e| panic!("request body is not JSON ({e}): {}", self.body))
    }
    pub fn form(&self) -> HashMap<String, String> {
        self.body.split('&').filter_map(|kv| kv.split_once('=')).map(|(k, v)| (unescape(k), unescape(v))).collect()
    }
    pub fn query(&self) -> HashMap<String, String> {
        let q = self.path.split_once('?').map(|x| x.1).unwrap_or("");
        Req { body: q.to_string(), ..self.clone() }.form()
    }
}

pub fn unescape(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = vec![];
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(if b[i] == b'+' { b' ' } else { b[i] });
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// A scripted answer.
#[derive(Clone, Debug)]
pub struct Resp {
    pub status: u16,
    pub content_type: String,
    pub body: Vec<u8>,
    /// keep the connection open this long after the body (a stalled stream)
    pub hold: Option<Duration>,
}

impl Resp {
    pub fn text(status: u16, content_type: &str, body: &str) -> Resp {
        Resp { status, content_type: content_type.into(), body: body.as_bytes().to_vec(), hold: None }
    }
    pub fn json(status: u16, v: &Value) -> Resp {
        Resp::text(status, "application/json", &v.to_string())
    }
    /// Server-sent events from `(event name, data)` pairs.
    pub fn sse(events: &[(&str, &str)]) -> Resp {
        let mut s = String::new();
        for (e, d) in events {
            if !e.is_empty() {
                s.push_str(&format!("event: {e}\n"));
            }
            s.push_str(&format!("data: {d}\n\n"));
        }
        Resp::text(200, "text/event-stream", &s)
    }
    pub fn ndjson(lines: &[Value]) -> Resp {
        Resp::text(200, "application/x-ndjson", &lines.iter().map(|l| format!("{l}\n")).collect::<String>())
    }
    pub fn held(mut self, d: Duration) -> Resp {
        self.hold = Some(d);
        self
    }
}

pub struct Mock {
    /// `http://127.0.0.1:<port>`
    pub url: String,
    reqs: Arc<Mutex<Vec<Req>>>,
    stop: Arc<AtomicBool>,
    addr: std::net::SocketAddr,
}

impl Mock {
    /// Answer every request with `handler(request, zero-based request number)`.
    pub fn start(handler: impl Fn(&Req, usize) -> Resp + Send + Sync + 'static) -> Mock {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock");
        let addr = listener.local_addr().unwrap();
        let reqs = Arc::new(Mutex::new(vec![]));
        let stop = Arc::new(AtomicBool::new(false));
        let handler = Arc::new(handler);
        let count = Arc::new(AtomicUsize::new(0));
        let (r2, s2) = (reqs.clone(), stop.clone());
        std::thread::spawn(move || {
            for conn in listener.incoming() {
                if s2.load(Ordering::Relaxed) {
                    break;
                }
                let Ok(conn) = conn else { continue };
                let (h, r, s, c) = (handler.clone(), r2.clone(), s2.clone(), count.clone());
                std::thread::spawn(move || serve(conn, &*h, &r, &s, &c));
            }
        });
        Mock { url: format!("http://{addr}"), reqs, stop, addr }
    }

    /// Answer the 1st request with `script[0]`, the 2nd with `script[1]` ... (500 when exhausted).
    pub fn scripted(script: Vec<Resp>) -> Mock {
        Mock::start(move |_, i| {
            script.get(i).cloned().unwrap_or_else(|| Resp::text(500, "text/plain", "mock script exhausted"))
        })
    }

    pub fn requests(&self) -> Vec<Req> {
        self.reqs.lock().unwrap().clone()
    }
}

impl Drop for Mock {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        let _ = TcpStream::connect(self.addr);
    }
}

fn serve(
    mut conn: TcpStream,
    handler: &(dyn Fn(&Req, usize) -> Resp + Send + Sync),
    reqs: &Mutex<Vec<Req>>,
    stop: &AtomicBool,
    count: &AtomicUsize,
) {
    let _ = conn.set_read_timeout(Some(Duration::from_secs(5)));
    let mut buf = vec![];
    let mut chunk = [0u8; 4096];
    let head_end = loop {
        if let Some(p) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break p;
        }
        match conn.read(&mut chunk) {
            Ok(0) | Err(_) => return,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
        }
    };
    let head = String::from_utf8_lossy(&buf[..head_end]).into_owned();
    let mut lines = head.lines();
    let mut first = lines.next().unwrap_or("").split_whitespace();
    let (method, path) = (first.next().unwrap_or("").to_string(), first.next().unwrap_or("").to_string());
    let headers: Vec<(String, String)> = lines
        .filter_map(|l| l.split_once(':'))
        .map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_string()))
        .collect();
    let len: usize = headers.iter().find(|(k, _)| k == "content-length").and_then(|(_, v)| v.parse().ok()).unwrap_or(0);
    let mut body = buf[head_end + 4..].to_vec();
    while body.len() < len {
        match conn.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(n) => body.extend_from_slice(&chunk[..n]),
        }
    }
    let req = Req { method, path, headers, body: String::from_utf8_lossy(&body).into_owned() };
    let idx = count.fetch_add(1, Ordering::SeqCst);
    reqs.lock().unwrap().push(req.clone());
    let resp = handler(&req, idx);
    let mut out =
        format!("HTTP/1.1 {} Mock\r\nContent-Type: {}\r\nConnection: close\r\n", resp.status, resp.content_type);
    if resp.hold.is_none() {
        out.push_str(&format!("Content-Length: {}\r\n", resp.body.len()));
    }
    out.push_str("\r\n");
    let _ = conn.write_all(out.as_bytes());
    let _ = conn.write_all(&resp.body);
    let _ = conn.flush();
    if let Some(d) = resp.hold {
        let t = Instant::now();
        while t.elapsed() < d && !stop.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}
