//! The loopback HTTP server: serves the page, its icon, and the single
//! `POST /__msg` endpoint that carries [`Envelope`]s to the host handler.
//!
//! The reserved message name `close` replies `{ok:true}` and then shuts the
//! server down (after the response has been written), so a hosted window can
//! dismiss itself without the host wiring anything special.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::Result;
use serde_json::{json, Value};

use crate::envelope::Envelope;

/// Handles one request. `name` is the message name, `data` its payload; the
/// returned value becomes the response `data`. Returning `Err` sends an error
/// response, which the shim turns into a thrown `Error`.
pub type Handler = Arc<dyn Fn(&str, Value) -> Result<Value> + Send + Sync>;
pub type Callback = Box<dyn Fn() + Send + Sync>;

/// Everything the bridge needs that is specific to the host.
pub struct Config {
    /// The JS global the page calls (`window[namespace]`).
    pub namespace: String,
    /// `document.title` set on the page, so a browser-hosted window is titled.
    pub title: String,
    /// The page's HTML source.
    pub index_html: String,
    /// Icon served at `/favicon.ico` (for a browser-hosted window's tab).
    pub icon_ico: &'static [u8],
    /// Appended to the page's CSP `content=`, e.g. `connect-src 'self';`.
    pub extra_csp: &'static str,
}

pub struct Server {
    addr: SocketAddr,
    page: String,
    icon: &'static [u8],
    on: Handler,
    stop: AtomicBool,
    on_close: Mutex<Option<Callback>>,
}

/// Starts the bridge on an ephemeral loopback port. The page is served at the
/// returned [`Server::url`].
pub fn serve(config: Config, on: Handler) -> Result<Arc<Server>> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let addr = listener.local_addr()?;
    let page = crate::shim::inject(
        &config.index_html,
        &config.namespace,
        &config.title,
        config.extra_csp,
    );
    let server = Arc::new(Server {
        addr,
        page,
        icon: config.icon_ico,
        on,
        stop: AtomicBool::new(false),
        on_close: Mutex::new(None),
    });

    let accept = server.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            if accept.stop.load(Ordering::SeqCst) {
                break;
            }
            let Ok(stream) = stream else { continue };
            let conn = accept.clone();
            std::thread::spawn(move || {
                let _ = handle_conn(&conn, stream);
            });
        }
    });

    Ok(server)
}

impl Server {
    pub fn url(&self) -> String {
        format!("http://{}/", self.addr)
    }

    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// Registers a callback fired exactly once when the server closes.
    pub fn set_on_close(&self, f: Callback) {
        *self.on_close.lock().unwrap() = Some(f);
    }

    pub fn close(&self) {
        if self.stop.swap(true, Ordering::SeqCst) {
            return;
        }
        // Unblock the accept loop.
        let _ = TcpStream::connect_timeout(&self.addr, Duration::from_millis(200));
        if let Some(cb) = self.on_close.lock().unwrap().take() {
            cb();
        }
    }
}

fn handle_conn(server: &Arc<Server>, mut stream: TcpStream) -> Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    let Some(req) = read_request(&mut stream)? else {
        return Ok(());
    };

    // Only answer requests that actually targeted the loopback address we
    // bound. This defeats DNS rebinding, where a remote page resolves its own
    // hostname to 127.0.0.1 and then reaches us as if it were same-origin.
    if !request_targets_loopback(&req, server.addr) {
        stream.write_all(&forbidden())?;
        stream.flush()?;
        return Ok(());
    }

    let mut close_after = false;
    let response = match (req.method.as_str(), req.path.as_str()) {
        ("GET", "/") => html_response(&server.page),
        ("GET", "/favicon.ico") => icon_response(server.icon),
        ("POST", "/__msg") => {
            let (resp, close) = dispatch(server, &req.body);
            close_after = close;
            resp
        }
        _ => not_found(),
    };

    stream.write_all(&response)?;
    stream.flush()?;
    if close_after {
        server.close();
    }
    Ok(())
}

/// Whether `Host` (and, when present, `Origin`) name our loopback address.
fn request_targets_loopback(req: &Request, addr: SocketAddr) -> bool {
    let host_ok = req
        .host
        .eq_ignore_ascii_case(&format!("127.0.0.1:{}", addr.port()))
        || req
            .host
            .eq_ignore_ascii_case(&format!("localhost:{}", addr.port()));
    if !host_ok {
        return false;
    }
    if req.origin.is_empty() {
        return true;
    }
    req.origin
        .eq_ignore_ascii_case(&format!("http://127.0.0.1:{}", addr.port()))
        || req
            .origin
            .eq_ignore_ascii_case(&format!("http://localhost:{}", addr.port()))
}

/// A parsed request line + the headers we care about.
struct Request {
    method: String,
    path: String,
    host: String,
    origin: String,
    body: String,
}

/// Parses the request line, headers and body. Returns `None` for an empty or
/// malformed request.
fn read_request(stream: &mut TcpStream) -> Result<Option<Request>> {
    let mut buf = Vec::new();
    let mut tmp = [0u8; 4096];
    let header_end;
    loop {
        let n = stream.read(&mut tmp)?;
        if n == 0 {
            return Ok(None);
        }
        buf.extend_from_slice(&tmp[..n]);
        if let Some(pos) = find_subsequence(&buf, b"\r\n\r\n") {
            header_end = pos + 4;
            break;
        }
        if buf.len() > 1 << 20 {
            return Ok(None);
        }
    }

    let header_text = String::from_utf8_lossy(&buf[..header_end]).into_owned();
    let mut lines = header_text.split("\r\n");
    let request_line = lines.next().unwrap_or("");
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("").to_string();
    let path = parts.next().unwrap_or("/").to_string();

    let mut content_length = 0usize;
    let mut host = String::new();
    let mut origin = String::new();
    for line in lines {
        if let Some((k, v)) = line.split_once(':') {
            let v = v.trim();
            if k.eq_ignore_ascii_case("content-length") {
                content_length = v.parse().unwrap_or(0);
            } else if k.eq_ignore_ascii_case("host") {
                host = v.to_string();
            } else if k.eq_ignore_ascii_case("origin") {
                origin = v.to_string();
            }
        }
    }

    let mut body = buf[header_end..].to_vec();
    while body.len() < content_length {
        let n = stream.read(&mut tmp)?;
        if n == 0 {
            break;
        }
        body.extend_from_slice(&tmp[..n]);
    }
    Ok(Some(Request {
        method,
        path,
        host,
        origin,
        body: String::from_utf8_lossy(&body).into_owned(),
    }))
}

/// Dispatches a request envelope. Returns the raw response and whether the
/// server should shut down afterwards.
fn dispatch(server: &Arc<Server>, body: &str) -> (Vec<u8>, bool) {
    let env: Envelope = match serde_json::from_str(body) {
        Ok(e) => e,
        Err(e) => {
            let resp = json!({
                "id": 0, "kind": "response", "name": "",
                "data": null, "error": format!("malformed request: {e}"),
            });
            return (json_response(&resp.to_string()), false);
        }
    };

    if env.name == "close" {
        let resp = json!({
            "id": env.id, "kind": "response", "name": env.name,
            "data": { "ok": true }, "error": null,
        });
        return (json_response(&resp.to_string()), true);
    }

    let result = (server.on)(&env.name, env.data);
    let resp = match result {
        Ok(data) => json!({
            "id": env.id, "kind": "response", "name": env.name,
            "data": data, "error": null,
        }),
        Err(e) => json!({
            "id": env.id, "kind": "response", "name": env.name,
            "data": null, "error": e.to_string(),
        }),
    };
    (json_response(&resp.to_string()), false)
}

fn find_subsequence(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

fn json_response(body: &str) -> Vec<u8> {
    format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    )
    .into_bytes()
}

fn html_response(body: &str) -> Vec<u8> {
    format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    )
    .into_bytes()
}

fn icon_response(icon: &[u8]) -> Vec<u8> {
    let mut out = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: image/x-icon\r\nContent-Length: {}\r\nCache-Control: max-age=86400\r\nConnection: close\r\n\r\n",
        icon.len()
    )
    .into_bytes();
    out.extend_from_slice(icon);
    out
}

fn not_found() -> Vec<u8> {
    b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec()
}

fn forbidden() -> Vec<u8> {
    b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};

    fn serve_for_test() -> Arc<Server> {
        serve(
            Config {
                namespace: "test".to_string(),
                title: "T".to_string(),
                index_html: "<html><head></head><body></body></html>".to_string(),
                icon_ico: &[],
                extra_csp: "connect-src 'self';",
            },
            Arc::new(|name, data| {
                if name == "boom" {
                    return Err(anyhow::anyhow!("nope"));
                }
                Ok(json!({ "echo": name, "data": data }))
            }),
        )
        .unwrap()
    }

    /// Sends a raw request to `addr` and returns the full response text.
    fn request(addr: SocketAddr, host: &str, origin: &str, method: &str, path: &str, body: &str) -> String {
        let mut s = TcpStream::connect(addr).unwrap();
        let origin_line = if origin.is_empty() {
            String::new()
        } else {
            format!("Origin: {origin}\r\n")
        };
        let req = format!(
            "{method} {path} HTTP/1.1\r\nHost: {host}\r\n{origin_line}Content-Length: {}\r\n\r\n{body}",
            body.len()
        );
        s.write_all(req.as_bytes()).unwrap();
        let mut out = String::new();
        s.read_to_string(&mut out).unwrap();
        out
    }

    fn envelope(id: u64, name: &str) -> String {
        format!(r#"{{"id":{id},"kind":"request","name":"{name}","data":{{"x":1}}}}"#)
    }

    #[test]
    fn dispatches_requests_and_reports_handler_errors() {
        let server = serve_for_test();
        let addr = server.addr();
        let host = format!("127.0.0.1:{}", addr.port());

        let ok = request(addr, &host, "", "POST", "/__msg", &envelope(1, "hello"));
        assert!(ok.contains(r#""echo":"hello""#), "got: {ok}");

        let err = request(addr, &host, "", "POST", "/__msg", &envelope(2, "boom"));
        assert!(err.contains(r#""error":"nope""#), "got: {err}");

        let bad = request(addr, &host, "", "POST", "/__msg", "{not json");
        assert!(bad.contains("malformed request"), "got: {bad}");
    }

    #[test]
    fn reserved_close_replies_then_stops_the_server() {
        let server = serve_for_test();
        let addr = server.addr();
        let host = format!("127.0.0.1:{}", addr.port());

        let resp = request(addr, &host, "", "POST", "/__msg", &envelope(9, "close"));
        assert!(resp.contains(r#""ok":true"#), "got: {resp}");
        std::thread::sleep(Duration::from_millis(150));
        assert!(server.stop.load(Ordering::SeqCst));
    }

    #[test]
    fn rejects_requests_that_do_not_target_loopback() {
        let server = serve_for_test();
        let addr = server.addr();

        let wrong_host = request(addr, "evil.example:1234", "", "POST", "/__msg", &envelope(1, "hello"));
        assert!(wrong_host.starts_with("HTTP/1.1 403"), "got: {wrong_host}");

        let right_host = format!("127.0.0.1:{}", addr.port());
        let wrong_origin = request(
            addr,
            &right_host,
            "http://evil.example",
            "POST",
            "/__msg",
            &envelope(1, "hello"),
        );
        assert!(wrong_origin.starts_with("HTTP/1.1 403"), "got: {wrong_origin}");
    }

    #[test]
    fn serves_page_and_icon() {
        let server = serve_for_test();
        let addr = server.addr();
        let host = format!("127.0.0.1:{}", addr.port());
        let page = request(addr, &host, "", "GET", "/", "");
        assert!(page.contains("text/html"), "got: {page}");
        let icon = request(addr, &host, "", "GET", "/favicon.ico", "");
        assert!(icon.contains("image/x-icon"), "got: {icon}");
    }
}
