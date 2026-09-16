// Akkhara HTTP Request library ("request")
//
// Usage (from an .akk program):
//     နည်းပညာများ request ကို အသုံးပြုပါ။
//
// This module provides simple HTTP GET/POST support, similar in spirit to
// Python's `requests` library. It is backed by the `ureq` crate, which is
// already a dependency of this binary (used elsewhere for package
// downloads -- see src/main.rs). It is compiled into the `akk` binary as a
// Rust module and registered by the interpreter when a program imports the
// "request" library.
//
// Akkhara-level syntax powered by this module:
//
//     response သည် request ၏ "https://api.example.com/data" ဖြစ်၏။
//     response ၏ အခြေအနေကုဒ် ကို ဖော်ပြပါ။
//
//     y အတွက် request ၏ "https://api.example.com/submit" သို့ "a=1&b=2" ကို ပို့ပါ။
//
// A response is represented as an Akkhara object tagged "request-response"
// (see RESP_CLASS in src/interpreter.rs) with three readable fields,
// accessed with the generic "<expr> ၏ <field>" syntax:
//
//     အခြေအနေကုဒ်   -- HTTP status code (integer), e.g. 200, 404
//     အကြောင်းအရာ   -- response body text
//     အောင်မြင်မှု    -- true when the status code is below 400

use std::sync::Arc;
use std::time::Duration;

/// A single HTTP response, translated into plain data that the
/// interpreter wraps into an Akkhara response object.
pub struct HttpResponse {
    pub status_code: i64,
    pub body: String,
    pub ok: bool,
}

/// How long to wait for a request before giving up. Kept generous since
/// Akkhara programs have no way to configure this themselves yet.
const REQUEST_TIMEOUT_SECS: u64 = 20;

/// Builds an agent using the OS's native TLS stack (via the `native-tls`
/// crate/feature) rather than ureq's default rustls backend. ureq treats
/// native-tls as an opt-in that must be wired up explicitly through
/// `tls_connector` -- it is never picked automatically, even with the
/// feature enabled -- so we do that here once per request.
fn agent() -> ureq::Agent {
    let tls_connector = native_tls::TlsConnector::new()
        .expect("failed to initialize the system TLS backend");
    ureq::AgentBuilder::new()
        .tls_connector(Arc::new(tls_connector))
        .timeout_connect(Duration::from_secs(REQUEST_TIMEOUT_SECS))
        .timeout(Duration::from_secs(REQUEST_TIMEOUT_SECS))
        .build()
}

/// Turns a `ureq::Response` into our plain `HttpResponse`, reading the body
/// as text. Any non-UTF8 body is lossily converted rather than failing the
/// whole request, since a network response is exactly the kind of thing an
/// Akkhara program should be able to inspect even when something about it
/// is unexpected.
fn to_http_response(status: u16, resp: ureq::Response) -> HttpResponse {
    let body = resp.into_string().unwrap_or_default();
    HttpResponse {
        status_code: status as i64,
        ok: status < 400,
        body,
    }
}

fn check_url(url: &str) -> Result<(), String> {
    if url.trim().is_empty() {
        return Err(
            "E079 request ၏ url တန်ဖိုးသည် ဗလာ ဖြစ်နေပါသည်။ URL တစ်ခု ပေးပါ။".to_string(),
        );
    }
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err(format!(
            "E079 request ၏ url \"{}\" သည် \"http://\" သို့မဟုတ် \"https://\" ဖြင့် မစပါ။",
            url
        ));
    }
    Ok(())
}

/// Perform an HTTP GET request against `url`.
///
/// Network failures (DNS, connection refused, timeout, TLS errors, etc.)
/// are surfaced as a Result::Err with a Burmese error message, matching the
/// rest of the interpreter's error style. HTTP-level error statuses (404,
/// 500, ...) are NOT treated as failures -- they come back as a normal
/// response object with `အောင်မြင်မှု` set to false, so a program can
/// inspect `အခြေအနေကုဒ်` itself instead of the interpreter aborting.
pub fn get(url: &str) -> Result<HttpResponse, String> {
    check_url(url)?;
    match agent().get(url).call() {
        Ok(resp) => Ok(to_http_response(resp.status(), resp)),
        Err(ureq::Error::Status(code, resp)) => Ok(to_http_response(code, resp)),
        Err(ureq::Error::Transport(t)) => Err(format!(
            "E080 request ၏ GET \"{}\" တောင်းဆိုမှု မအောင်မြင်ပါ - {}",
            url, t
        )),
    }
}

/// Perform an HTTP POST request against `url` with an
/// `application/x-www-form-urlencoded` body (e.g. `"a=1&b=2"`), the same
/// encoding Python's `requests.post(url, data="...")` sends by default.
pub fn post(url: &str, data: &str) -> Result<HttpResponse, String> {
    check_url(url)?;
    match agent()
        .post(url)
        .set("Content-Type", "application/x-www-form-urlencoded")
        .send_string(data)
    {
        Ok(resp) => Ok(to_http_response(resp.status(), resp)),
        Err(ureq::Error::Status(code, resp)) => Ok(to_http_response(code, resp)),
        Err(ureq::Error::Transport(t)) => Err(format!(
            "E081 request ၏ POST \"{}\" တောင်းဆိုမှု မအောင်မြင်ပါ - {}",
            url, t
        )),
    }
}
