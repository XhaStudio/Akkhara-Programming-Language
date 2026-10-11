// Akkhara HTTP server library ("server" / "ဆာဗာ")
//
// Usage (from an .akk program): import it, register handlers for paths, then
// serve, either spelling:
//
//     use server;                             # = နည်းပညာများ server ကို သုံးမည်။
//     server.get("/hello", "hello");
//     server.serve(8080);
//
// This module owns the wire protocol only: binding a port, parsing one HTTP/1.1
// request, and writing one response. What a path means, which function handles
// it, and what a handler returns are all the interpreter's business (see
// `eval_server_call` / `serve_server` in src/interpreter.rs), so the language
// side stays in one place and this side stays testable as plain Rust.
//
// Deliberately minimal, because a language server library should not drag in a
// runtime: std::net, one connection at a time, `Connection: close`, no
// keep-alive, no TLS, no chunked bodies, no async. A request is answered by the
// thread that accepted it -- which is also the only thread that can call back
// into the interpreter. Response headers and the content type are the
// interpreter's business (it knows what a handler asked for); this side only
// puts them on the wire, with any CR/LF stripped so a header value built from a
// request cannot split the response.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::time::Duration;

/// Largest request body accepted, in bytes. A bigger `Content-Length` is
/// answered with 413 instead of being read.
pub const MAX_BODY: usize = 1 << 20; // 1 MiB

/// Largest request head (request line + headers) accepted, in bytes.
const MAX_HEAD: usize = 64 << 10; // 64 KiB

/// How long a client gets to send its request line and headers. Without this
/// a client that connects and then says nothing would block the server (which
/// handles one connection at a time) forever.
const READ_TIMEOUT: Duration = Duration::from_secs(10);

/// One parsed request, already split into the pieces an Akkhara handler reads
/// through its request object.
pub struct Request {
    pub method: String,
    pub path: String,
    pub query: String,
    pub body: String,
    /// The header block as received, one `Name: value` per line.
    pub headers: String,
}

/// A listening socket. Dropping it closes the port.
pub struct Listener {
    inner: TcpListener,
}

/// One accepted connection: either a parsed request, or a `problem` the
/// interpreter should send back instead (a malformed request never reaches a
/// handler).
pub struct Pending {
    stream: TcpStream,
    pub request: Option<Request>,
    pub problem: Option<(u16, String)>,
}

impl Listener {
    /// Listen on `port` on every interface. A busy port is an error, so the
    /// program can report it instead of silently serving nothing.
    pub fn bind(port: u16) -> Result<Listener, String> {
        let inner = TcpListener::bind(("0.0.0.0", port))
            .map_err(|e| format!("could not listen on port {}: {}", port, e))?;
        Ok(Listener { inner })
    }

    /// Wait for the next connection and read one request from it.
    ///
    /// `Err` means the listener itself failed (the port went away); a client
    /// that sent something unreadable comes back as a `Pending` carrying the
    /// problem to reply with, so one bad request cannot stop the server.
    pub fn accept(&self) -> Result<Pending, String> {
        let (stream, _peer) = self
            .inner
            .accept()
            .map_err(|e| format!("accept failed: {}", e))?;
        stream
            .set_read_timeout(Some(READ_TIMEOUT))
            .map_err(|e| format!("could not arm the request timeout: {}", e))?;
        match read_request(&stream) {
            Ok(request) => Ok(Pending {
                stream,
                request: Some(request),
                problem: None,
            }),
            Err((status, body)) => Ok(Pending {
                stream,
                request: None,
                problem: Some((status, body)),
            }),
        }
    }
}

impl Pending {
    /// Write `status` + `body` and close. A client that has already gone away
    /// is an `Err`, not a panic: the server keeps serving.
    ///
    /// `content_type` is the whole header value, and an empty one leaves the
    /// header out (a 204 should not claim a type it has no body for).
    /// `extra_headers` are written after the standard ones -- `Allow` on a 405,
    /// `Location` on a redirect, or whatever a handler set. `head_only` writes
    /// the head and skips the body: that is a `HEAD` answer, and the
    /// `Content-Length` still describes the body it would have sent, which is
    /// what the method promises.
    pub fn respond(
        self,
        status: u16,
        body: &str,
        content_type: &str,
        extra_headers: &[(String, String)],
        head_only: bool,
    ) -> Result<(), String> {
        let mut stream = self.stream;
        let head = response_head(status, body.as_bytes().len(), content_type, extra_headers);
        let bytes = body.as_bytes();
        stream
            .write_all(head.as_bytes())
            .and_then(|_| {
                if head_only {
                    Ok(())
                } else {
                    stream.write_all(bytes)
                }
            })
            .and_then(|_| stream.flush())
            .map_err(|e| format!("could not write the response: {}", e))?;
        let _ = stream.shutdown(Shutdown::Write);
        Ok(())
    }
}

/// One header name/value as it goes on the wire, with any CR or LF removed: a
/// header a program built from a request could otherwise inject a header or
/// split the response in two.
fn clean_header_value(text: &str) -> String {
    text.chars()
        .filter(|c| *c != '\r' && *c != '\n')
        .collect::<String>()
        .trim()
        .to_string()
}

fn response_head(
    status: u16,
    len: usize,
    content_type: &str,
    extra_headers: &[(String, String)],
) -> String {
    let mut head = format!("HTTP/1.1 {} {}\r\n", status, reason(status));
    let content_type = clean_header_value(content_type);
    if !content_type.is_empty() {
        head.push_str(&format!("Content-Type: {}\r\n", content_type));
    }
    // 204 and 304 are defined to carry no body, so they carry no length.
    if status != 204 && status != 304 {
        head.push_str(&format!("Content-Length: {}\r\n", len));
    }
    for (name, value) in extra_headers {
        let name = clean_header_value(name);
        if !name.is_empty() {
            head.push_str(&format!("{}: {}\r\n", name, clean_header_value(value)));
        }
    }
    head.push_str("Connection: close\r\n\r\n");
    head
}

/// Percent-decode one URL component: `%XX` becomes the byte it names, and
/// anything else is copied. `plus_is_space` is true for query text, where a
/// `+` means a space (form encoding) and false for a path, where it is a
/// literal plus. Invalid escapes and invalid UTF-8 are kept as they came in --
/// a malformed request should not become a panic or an empty value.
pub fn decode_component(text: &str, plus_is_space: bool) -> String {
    let bytes = text.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' => {
                let nibble = |c: u8| (c as char).to_digit(16);
                match (
                    bytes.get(i + 1).copied().and_then(nibble),
                    bytes.get(i + 2).copied().and_then(nibble),
                ) {
                    (Some(hi), Some(lo)) => {
                        out.push((hi * 16 + lo) as u8);
                        i += 3;
                    }
                    // A stray `%` is text, not an escape.
                    _ => {
                        out.push(b'%');
                        i += 1;
                    }
                }
            }
            b'+' if plus_is_space => {
                out.push(b' ');
                i += 1;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The query text after `?` as key/value pairs, in the order they appear.
/// `a=1&b=2` -> `[("a","1"), ("b","2")]`; a piece with no `=` has an empty
/// value, and empty pieces are skipped. Decoding is the caller's business (it
/// knows whether the text came from a query or a path), so this only splits.
pub fn split_query(query: &str) -> Vec<(String, String)> {
    let mut pairs = Vec::new();
    for piece in query.split('&') {
        if piece.is_empty() {
            continue;
        }
        let (key, value) = match piece.split_once('=') {
            Some((k, v)) => (k, v),
            None => (piece, ""),
        };
        pairs.push((
            decode_component(key, true),
            decode_component(value, true),
        ));
    }
    pairs
}

/// Reason phrase for the statuses the library and a handler can produce.
fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        201 => "Created",
        202 => "Accepted",
        204 => "No Content",
        301 => "Moved Permanently",
        302 => "Found",
        304 => "Not Modified",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        409 => "Conflict",
        413 => "Payload Too Large",
        422 => "Unprocessable Entity",
        429 => "Too Many Requests",
        500 => "Internal Server Error",
        501 => "Not Implemented",
        503 => "Service Unavailable",
        _ => "Status",
    }
}

/// Read one request. `Err((status, body))` is what to answer with when the
/// connection did not carry a usable HTTP/1.1 request.
fn read_request(stream: &TcpStream) -> Result<Request, (u16, String)> {
    let mut reader = BufReader::new(stream);

    let mut line = String::new();
    let read = reader.read_line(&mut line).map_err(|e| {
        (
            400,
            format!("could not read the request line: {}", e),
        )
    })?;
    if read == 0 {
        return Err((400, "the connection closed before a request arrived".to_string()));
    }
    let mut parts = line.split_whitespace();
    let (method, target) = match (parts.next(), parts.next()) {
        (Some(m), Some(t)) => (m.to_string(), t.to_string()),
        _ => {
            return Err((
                400,
                format!("not an HTTP request line: {}", line.trim()),
            ))
        }
    };
    if method.is_empty() || !method.bytes().all(|b| b.is_ascii_uppercase()) {
        return Err((400, format!("not an HTTP method: {}", method)));
    }
    if !target.starts_with('/') {
        return Err((400, format!("not an HTTP request target: {}", target)));
    }

    let mut headers = String::new();
    let mut content_length = 0usize;
    loop {
        let mut header = String::new();
        let read = reader
            .read_line(&mut header)
            .map_err(|e| (400, format!("could not read the headers: {}", e)))?;
        if read == 0 || header == "\r\n" || header == "\n" {
            break;
        }
        if headers.len() + header.len() > MAX_HEAD {
            return Err((400, "the request headers are too large".to_string()));
        }
        if let Some((name, value)) = header.split_once(':') {
            if name.trim().eq_ignore_ascii_case("content-length") {
                content_length = value.trim().parse().map_err(|_| {
                    (
                        400,
                        format!("not a Content-Length: {}", value.trim()),
                    )
                })?;
            }
        }
        headers.push_str(header.trim_end());
        headers.push('\n');
    }

    if content_length > MAX_BODY {
        return Err((
            413,
            format!(
                "the request body is {} bytes; this server takes up to {}",
                content_length, MAX_BODY
            ),
        ));
    }
    let mut raw = vec![0u8; content_length];
    if content_length > 0 {
        reader
            .read_exact(&mut raw)
            .map_err(|e| (400, format!("could not read the request body: {}", e)))?;
    }
    let body = String::from_utf8_lossy(&raw).into_owned();

    let (path, query) = match target.split_once('?') {
        Some((p, q)) => (p.to_string(), q.to_string()),
        None => (target, String::new()),
    };

    Ok(Request {
        method,
        path,
        query,
        body,
        headers,
    })
}
