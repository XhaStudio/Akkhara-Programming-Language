// Akkhara HTTP library: request
//
// Usage (from an .akk program):
//     နည်းပညာများ request ကို အသုံးပြုပါ။
//
// This module is Akkhara's equivalent of Python's `requests`. It is
// compiled into the akk binary as a Rust module (see the `#[path]`
// include in src/main.rs) and registered by the interpreter when a
// program imports the request library, exactly like ကျပန်း (random)
// and အချိန် (time).
//
// Akkhara-level syntax powered by this module:
//
//   GET  -- member syntax, `<lib> ၏ <mod>`:
//       response သည် request ၏ "https://api.example.com/data" ဖြစ်၏။
//
//   POST -- send syntax:
//       y အတွက် request ၏ "https://api.example.com/submit" သို့ "a=1&b=2" ကို ပို့ပါ။
//
// Both produce a *response object* whose fields are read with the same
// `၏` member syntax:
//       response ၏ အခြေအနေကုဒ် ကို ဖော်ပြပါ။   # status code (int)
//       response ၏ စာသား ကို ဖော်ပြပါ။         # body text (string)
//       response ၏ အောင်မြင် ကို ဖော်ပြပါ။      # True when status is 2xx

use std::io::Read;
use std::time::Duration;

/// Class name reported for response objects, so a script that prints a
/// whole response (or checks its type) sees something meaningful.
pub const RESPONSE_CLASS: &str = "တုံ့ပြန်ချက်";

/// Field name: the numeric HTTP status code.
pub const FIELD_STATUS: &str = "အခြေအနေကုဒ်";
/// Field name: the response body as text.
pub const FIELD_BODY: &str = "စာသား";
/// Field name: True when the status code is in the 2xx range.
pub const FIELD_OK: &str = "အောင်မြင်";
/// Field name: the URL that was actually requested (after redirects).
pub const FIELD_URL: &str = "လိပ်စာ";

/// Maximum response body size read into memory (10 MB), so a runaway
/// server can't exhaust memory or hang the interpreter.
const MAX_BODY_BYTES: u64 = 10 * 1024 * 1024;

/// Whole-request timeout. Generous but finite, so a hung connection
/// can't freeze a running Akkhara program forever.
const TIMEOUT: Duration = Duration::from_secs(30);

/// The outcome of an HTTP request, before the interpreter wraps it into
/// an Akkhara object value.
pub struct HttpResponse {
    pub body: String,
    pub status: i64,
    pub url: String,
}

impl HttpResponse {
    /// True when the status code is a 2xx success.
    pub fn ok(&self) -> bool {
        (200..300).contains(&self.status)
    }
}

/// Rejects obviously malformed URLs before we hand them to the HTTP
/// stack, so a typo produces a clear Akkhara error rather than an
/// opaque transport message.
fn validate_url(url: &str) -> Result<(), String> {
    let trimmed = url.trim();
    if trimmed.is_empty() {
        return Err("E072 တောင်းဆိုရန် လိပ်စာ (URL) သည် ဗလာ ဖြစ်နေပါသည်။".to_string());
    }
    if !(trimmed.starts_with("http://") || trimmed.starts_with("https://")) {
        return Err(format!(
            "E072 လိပ်စာ \"{}\" သည် မှန်ကန်ခြင်းမရှိပါ။ \"http://\" (သို့) \"https://\" ဖြင့် စရပါမည်။",
            trimmed
        ));
    }
    // Reject a bare scheme with no host, e.g. "https://".
    let rest = trimmed
        .trim_start_matches("https://")
        .trim_start_matches("http://");
    if rest.is_empty() || rest.starts_with('/') {
        return Err(format!(
            "E072 လိပ်စာ \"{}\" တွင် host (ဆိုဒ်အမည်) ပါဝင်ခြင်း မရှိပါ။",
            trimmed
        ));
    }
    Ok(())
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout(TIMEOUT)
        .user_agent(concat!("akk/", env!("CARGO_PKG_VERSION")))
        .build()
}

/// Performs an HTTP GET request against `url`.
pub fn get(url: &str) -> Result<HttpResponse, String> {
    let url = url.trim();
    validate_url(url)?;
    read_response(agent().get(url).call(), url)
}

/// Performs an HTTP POST request against `url`, sending `body` as the
/// request body. A body that looks like JSON (starts with `{` or `[`)
/// is sent as `application/json`; a body that looks like form data
/// (contains `=` and no spaces) is sent as
/// `application/x-www-form-urlencoded`; anything else as `text/plain`.
pub fn post(url: &str, body: &str) -> Result<HttpResponse, String> {
    let url = url.trim();
    validate_url(url)?;
    read_response(
        agent()
            .post(url)
            .set("Content-Type", guess_content_type(body))
            .send_string(body),
        url,
    )
}

/// Picks a sensible Content-Type so scripts don't have to think about
/// headers for the two common cases (JSON payloads and form posts).
fn guess_content_type(body: &str) -> &'static str {
    let t = body.trim_start();
    if t.starts_with('{') || t.starts_with('[') {
        "application/json"
    } else if body.contains('=') && !body.contains(' ') {
        "application/x-www-form-urlencoded"
    } else {
        "text/plain; charset=utf-8"
    }
}

/// Shared response handling for GET/POST: reads the body (success and
/// HTTP-error responses alike) into a capped-size string, and turns
/// only genuine transport failures (DNS, connection refused, timeout,
/// TLS, ...) into an `Err`.
fn read_response(
    result: Result<ureq::Response, ureq::Error>,
    requested_url: &str,
) -> Result<HttpResponse, String> {
    match result {
        Ok(resp) => {
            let status = resp.status() as i64;
            let url = resp.get_url().to_string();
            let body = read_body(resp.into_reader())?;
            Ok(HttpResponse { body, status, url })
        }
        // ureq reports 4xx/5xx as `Err(Status(..))`, but an Akkhara
        // script still wants to read the body (e.g. an API's JSON error
        // message) and branch on the status itself, so this is a normal
        // response here -- not a script-level error.
        Err(ureq::Error::Status(code, resp)) => {
            let url = resp.get_url().to_string();
            let body = read_body(resp.into_reader())?;
            Ok(HttpResponse {
                body,
                status: code as i64,
                url,
            })
        }
        Err(ureq::Error::Transport(t)) => Err(format!(
            "E070 \"{}\" သို့ ကွန်ရက်တောင်းဆိုမှု မအောင်မြင်ပါ။ အင်တာနက်ချိတ်ဆက်မှု (သို့) လိပ်စာကို စစ်ဆေးပါ - {}",
            requested_url, t
        )),
    }
}

fn read_body(reader: impl Read) -> Result<String, String> {
    let mut buf = Vec::new();
    reader
        .take(MAX_BODY_BYTES + 1)
        .read_to_end(&mut buf)
        .map_err(|e| {
            format!(
                "E071 တောင်းဆိုချက်၏ အဖြေကို ဖတ်ရာတွင် error ဖြစ်ပေါ်ခဲ့ပါသည် - {}",
                e
            )
        })?;

    if buf.len() as u64 > MAX_BODY_BYTES {
        return Err(format!(
            "E073 တောင်းဆိုချက်၏ အဖြေသည် ကြီးလွန်းပါသည်။ အများဆုံး {} MB သာ လက်ခံပါသည်။",
            MAX_BODY_BYTES / (1024 * 1024)
        ));
    }

    // Binary or otherwise non-UTF-8 payloads shouldn't crash a script;
    // replace invalid sequences rather than erroring out, matching what
    // a script author expects when printing a response.
    Ok(String::from_utf8_lossy(&buf).into_owned())
}
