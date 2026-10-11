// Akkhara JSON library ("json" / "ဂျေဆန်")
//
// Usage (from an .akk program), either spelling:
//
//     use json;                               # = နည်းပညာများ json ကို သုံးမည်။
//     d = json.parse("{\"a\": 1}");
//     print(json.stringify(d));
//
// This module owns the format only: reading JSON text into a tree, and writing
// a tree back out as JSON text. The tree is this module's own `Json` type, not
// the interpreter's `Value`, so the library is plain Rust with no dependency on
// the language runtime -- the interpreter converts at the boundary (see
// `eval_json_call` in src/interpreter.rs), which is also where the JSON tree
// becomes the dicts/lists a program indexes.
//
// No dependencies, no `serde`: the grammar is small, and a backend library
// should not pull a serialization framework into the binary for it.

/// A parsed JSON value.
///
/// Numbers that fit and carry no fraction or exponent become `Int`, so a JSON
/// `1` is the language's `ကိန်းပြည့်` and `1.0` / `1e3` are `ဒဿမကိန်း`; that
/// keeps `{"n": 1}` round-tripping as `{"n": 1}` rather than `1.0`.
#[derive(Debug, Clone, PartialEq)]
pub enum Json {
    /// `null`. The interpreter maps it to its own null value, which is what a
    /// program reads as `null` and writes back out unchanged.
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    Arr(Vec<Json>),
    /// Object members in document order. A key written twice keeps its first
    /// position with its last value, matching what a dict literal does.
    Obj(Vec<(String, Json)>),
}

/// Why a piece of text is not JSON, and where in it that became clear. `at` is
/// a byte offset, so the interpreter can point a reader at the right spot in a
/// body a client sent.
#[derive(Debug, Clone, PartialEq)]
pub struct ParseError {
    pub what: String,
    pub at: usize,
}

impl ParseError {
    fn new(what: impl Into<String>, at: usize) -> ParseError {
        ParseError {
            what: what.into(),
            at,
        }
    }
}

/// Deepest nesting accepted before the parser gives up. Each level costs a
/// stack frame, and a request body is attacker-controlled text: without a
/// limit, `[[[[[...` would overflow the stack instead of being refused.
pub const MAX_DEPTH: usize = 128;

/// Read JSON text into a tree.
pub fn parse(text: &str) -> Result<Json, ParseError> {
    let bytes = text.as_bytes();
    let mut p = Parser { bytes, at: 0 };
    p.skip_ws();
    let value = p.value(0)?;
    p.skip_ws();
    if p.at < bytes.len() {
        return Err(ParseError::new(
            format!(
                "unexpected {} after the value",
                describe_byte(bytes[p.at])
            ),
            p.at,
        ));
    }
    Ok(value)
}

fn describe_byte(b: u8) -> String {
    if b.is_ascii_graphic() || b == b' ' {
        format!("'{}'", b as char)
    } else {
        format!("byte 0x{:02x}", b)
    }
}

struct Parser<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Parser<'a> {
    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.at).copied()
    }

    fn skip_ws(&mut self) {
        // JSON whitespace is exactly these four characters.
        while matches!(self.peek(), Some(b' ') | Some(b'\t') | Some(b'\n') | Some(b'\r')) {
            self.at += 1;
        }
    }

    fn expect(&mut self, byte: u8) -> Result<(), ParseError> {
        if self.peek() == Some(byte) {
            self.at += 1;
            Ok(())
        } else {
            Err(ParseError::new(
                match self.peek() {
                    Some(_) => format!("expected '{}'", byte as char),
                    None => format!("expected '{}' but the text ended", byte as char),
                },
                self.at,
            ))
        }
    }

    fn value(&mut self, depth: usize) -> Result<Json, ParseError> {
        if depth > MAX_DEPTH {
            return Err(ParseError::new(
                format!("nested more than {} levels deep", MAX_DEPTH),
                self.at,
            ));
        }
        self.skip_ws();
        match self.peek() {
            None => Err(ParseError::new("the text ended where a value was expected", self.at)),
            Some(b'{') => self.object(depth),
            Some(b'[') => self.array(depth),
            Some(b'"') => Ok(Json::Str(self.string()?)),
            Some(b't') => self.literal("true", Json::Bool(true)),
            Some(b'f') => self.literal("false", Json::Bool(false)),
            Some(b'n') => self.literal("null", Json::Null),
            Some(c) if c == b'-' || c.is_ascii_digit() => self.number(),
            Some(c) => Err(ParseError::new(
                format!("expected a value, found {}", describe_byte(c)),
                self.at,
            )),
        }
    }

    fn literal(&mut self, word: &str, value: Json) -> Result<Json, ParseError> {
        if self.bytes[self.at..].starts_with(word.as_bytes()) {
            self.at += word.len();
            Ok(value)
        } else {
            // e.g. `tru` / `nul`: the common case is a typo of one of the three.
            let end = self
                .bytes
                .iter()
                .skip(self.at)
                .position(|b| !b.is_ascii_alphabetic())
                .map(|i| self.at + i)
                .unwrap_or(self.bytes.len());
            let found = String::from_utf8_lossy(&self.bytes[self.at..end]).into_owned();
            Err(ParseError::new(
                format!("expected `{}`, found `{}`", word, found),
                self.at,
            ))
        }
    }

    fn number(&mut self) -> Result<Json, ParseError> {
        let start = self.at;
        if self.peek() == Some(b'-') {
            self.at += 1;
        }
        // Integer part: a leading 0 stands alone (`01` is not JSON).
        match self.peek() {
            Some(b'0') => {
                self.at += 1;
                if matches!(self.peek(), Some(c) if c.is_ascii_digit()) {
                    return Err(ParseError::new(
                        "a number cannot have a leading zero",
                        self.at,
                    ));
                }
            }
            Some(c) if c.is_ascii_digit() => {
                while matches!(self.peek(), Some(c) if c.is_ascii_digit()) {
                    self.at += 1;
                }
            }
            _ => return Err(ParseError::new("expected a digit", self.at)),
        }
        let mut is_float = false;
        if self.peek() == Some(b'.') {
            is_float = true;
            self.at += 1;
            if !matches!(self.peek(), Some(c) if c.is_ascii_digit()) {
                return Err(ParseError::new("expected a digit after '.'", self.at));
            }
            while matches!(self.peek(), Some(c) if c.is_ascii_digit()) {
                self.at += 1;
            }
        }
        if matches!(self.peek(), Some(b'e') | Some(b'E')) {
            is_float = true;
            self.at += 1;
            if matches!(self.peek(), Some(b'+') | Some(b'-')) {
                self.at += 1;
            }
            if !matches!(self.peek(), Some(c) if c.is_ascii_digit()) {
                return Err(ParseError::new("expected a digit in the exponent", self.at));
            }
            while matches!(self.peek(), Some(c) if c.is_ascii_digit()) {
                self.at += 1;
            }
        }
        // ASCII here by construction, so this cannot fail on encoding.
        let text = std::str::from_utf8(&self.bytes[start..self.at]).unwrap_or("");
        if !is_float {
            if let Ok(i) = text.parse::<i64>() {
                return Ok(Json::Int(i));
            }
            // Out of i64 range: fall through to a float rather than refusing a
            // number JSON itself allows.
        }
        match text.parse::<f64>() {
            Ok(f) if f.is_finite() => Ok(Json::Float(f)),
            _ => Err(ParseError::new(
                format!("`{}` is not a usable number", text),
                start,
            )),
        }
    }

    fn string(&mut self) -> Result<String, ParseError> {
        self.expect(b'"')?;
        let mut out = String::new();
        loop {
            let byte = match self.peek() {
                Some(b) => b,
                None => {
                    return Err(ParseError::new(
                        "the string was not closed before the text ended",
                        self.at,
                    ))
                }
            };
            match byte {
                b'"' => {
                    self.at += 1;
                    return Ok(out);
                }
                b'\\' => {
                    self.at += 1;
                    let esc = self.peek().ok_or_else(|| {
                        ParseError::new("the text ended inside an escape", self.at)
                    })?;
                    self.at += 1;
                    match esc {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'b' => out.push('\u{8}'),
                        b'f' => out.push('\u{c}'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'u' => out.push(self.unicode_escape()?),
                        other => {
                            return Err(ParseError::new(
                                format!("`\\{}` is not a JSON escape", other as char),
                                self.at - 1,
                            ))
                        }
                    }
                }
                // Raw control characters must be escaped in JSON.
                b if b < 0x20 => {
                    return Err(ParseError::new(
                        format!("{} must be escaped inside a string", describe_byte(b)),
                        self.at,
                    ))
                }
                _ => {
                    // Copy one whole UTF-8 character: the text was validated on
                    // the way in, so step by the byte length class.
                    let len = utf8_len(byte);
                    let end = (self.at + len).min(self.bytes.len());
                    match std::str::from_utf8(&self.bytes[self.at..end]) {
                        Ok(s) => out.push_str(s),
                        Err(_) => {
                            return Err(ParseError::new(
                                "the text is not valid UTF-8",
                                self.at,
                            ))
                        }
                    }
                    self.at = end;
                }
            }
        }
    }

    /// The four hex digits after `\u`, combining a surrogate pair when one
    /// follows. A lone surrogate becomes U+FFFD, the same replacement a text
    /// decoder uses, rather than an error: the text stays usable.
    fn unicode_escape(&mut self) -> Result<char, ParseError> {
        let first = self.hex4()?;
        if (0xD800..0xDC00).contains(&first) {
            if self.peek() == Some(b'\\')
                && self.bytes.get(self.at + 1) == Some(&b'u')
                && self.at + 6 <= self.bytes.len()
            {
                let save = self.at;
                self.at += 2;
                let second = self.hex4()?;
                if (0xDC00..0xE000).contains(&second) {
                    let combined =
                        0x10000 + ((first - 0xD800) << 10) + (second - 0xDC00);
                    return char::from_u32(combined)
                        .ok_or_else(|| ParseError::new("not a usable code point", save));
                }
                // Not a low surrogate: leave the second escape for the next
                // iteration of the string loop.
                self.at = save;
            }
            return Ok('\u{FFFD}');
        }
        if (0xDC00..0xE000).contains(&first) {
            return Ok('\u{FFFD}');
        }
        char::from_u32(first).ok_or_else(|| ParseError::new("not a usable code point", self.at - 4))
    }

    fn hex4(&mut self) -> Result<u32, ParseError> {
        if self.at + 4 > self.bytes.len() {
            return Err(ParseError::new(
                "a \\u escape needs four hex digits",
                self.at,
            ));
        }
        let digits = &self.bytes[self.at..self.at + 4];
        let mut value = 0u32;
        for &d in digits {
            let nibble = (d as char).to_digit(16).ok_or_else(|| {
                ParseError::new(
                    format!("`{}` is not a hex digit", describe_byte(d)),
                    self.at,
                )
            })?;
            value = value * 16 + nibble;
        }
        self.at += 4;
        Ok(value)
    }

    fn array(&mut self, depth: usize) -> Result<Json, ParseError> {
        self.expect(b'[')?;
        let mut items = Vec::new();
        self.skip_ws();
        if self.peek() == Some(b']') {
            self.at += 1;
            return Ok(Json::Arr(items));
        }
        loop {
            items.push(self.value(depth + 1)?);
            self.skip_ws();
            match self.peek() {
                Some(b',') => {
                    self.at += 1;
                    self.skip_ws();
                    if self.peek() == Some(b']') {
                        return Err(ParseError::new("a trailing comma is not JSON", self.at));
                    }
                }
                Some(b']') => {
                    self.at += 1;
                    return Ok(Json::Arr(items));
                }
                Some(c) => {
                    return Err(ParseError::new(
                        format!("expected ',' or ']', found {}", describe_byte(c)),
                        self.at,
                    ))
                }
                None => {
                    return Err(ParseError::new(
                        "the array was not closed before the text ended",
                        self.at,
                    ))
                }
            }
        }
    }

    fn object(&mut self, depth: usize) -> Result<Json, ParseError> {
        self.expect(b'{')?;
        let mut members: Vec<(String, Json)> = Vec::new();
        self.skip_ws();
        if self.peek() == Some(b'}') {
            self.at += 1;
            return Ok(Json::Obj(members));
        }
        loop {
            self.skip_ws();
            if self.peek() != Some(b'"') {
                return Err(ParseError::new(
                    match self.peek() {
                        Some(c) => format!("an object key must be a string, found {}", describe_byte(c)),
                        None => "the object was not closed before the text ended".to_string(),
                    },
                    self.at,
                ));
            }
            let key = self.string()?;
            self.skip_ws();
            self.expect(b':')?;
            let value = self.value(depth + 1)?;
            // Last value wins for a repeated key, in the first position -- the
            // same rule a dict literal uses.
            match members.iter_mut().find(|(k, _)| *k == key) {
                Some(slot) => slot.1 = value,
                None => members.push((key, value)),
            }
            self.skip_ws();
            match self.peek() {
                Some(b',') => {
                    self.at += 1;
                    self.skip_ws();
                    if self.peek() == Some(b'}') {
                        return Err(ParseError::new("a trailing comma is not JSON", self.at));
                    }
                }
                Some(b'}') => {
                    self.at += 1;
                    return Ok(Json::Obj(members));
                }
                Some(c) => {
                    return Err(ParseError::new(
                        format!("expected ',' or '}}', found {}", describe_byte(c)),
                        self.at,
                    ))
                }
                None => {
                    return Err(ParseError::new(
                        "the object was not closed before the text ended",
                        self.at,
                    ))
                }
            }
        }
    }
}

fn utf8_len(first: u8) -> usize {
    match first {
        0x00..=0x7f => 1,
        0xc0..=0xdf => 2,
        0xe0..=0xef => 3,
        _ => 4,
    }
}

/// Write a tree as compact JSON text: no spaces, keys in the order they were
/// read (or built), one line.
pub fn write(value: &Json) -> String {
    let mut out = String::new();
    write_into(value, &mut out);
    out
}

fn write_into(value: &Json, out: &mut String) {
    match value {
        Json::Null => out.push_str("null"),
        Json::Bool(true) => out.push_str("true"),
        Json::Bool(false) => out.push_str("false"),
        Json::Int(i) => out.push_str(&i.to_string()),
        Json::Float(f) => out.push_str(&write_float(*f)),
        Json::Str(s) => write_string(s, out),
        Json::Arr(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_into(item, out);
            }
            out.push(']');
        }
        Json::Obj(members) => {
            out.push('{');
            for (i, (key, member)) in members.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_string(key, out);
                out.push(':');
                write_into(member, out);
            }
            out.push('}');
        }
    }
}

/// A float as JSON sees it: always with a `.` or an exponent, so `1.0` does not
/// come out as `1` and silently become an integer on the way back in.
fn write_float(f: f64) -> String {
    if !f.is_finite() {
        // JSON has no NaN or infinity. They cannot be parsed either, so this
        // only happens for a value the program computed.
        return "null".to_string();
    }
    let text = format!("{}", f);
    if text.contains('.') || text.contains('e') || text.contains('E') {
        text
    } else {
        format!("{}.0", text)
    }
}

fn write_string(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            // Everything else, including all of Myanmar, is written as itself:
            // JSON is UTF-8 text, and escaping it would only make the output
            // unreadable.
            c => out.push(c),
        }
    }
    out.push('"');
}

/// Write a tree as indented JSON text, `spaces` per level (2 is the usual
/// choice). Empty arrays and objects stay on one line, the way most tools
/// print them.
pub fn write_pretty(value: &Json, spaces: usize) -> String {
    let mut out = String::new();
    write_pretty_into(value, spaces, 0, &mut out);
    out
}

fn write_pretty_into(value: &Json, spaces: usize, depth: usize, out: &mut String) {
    let pad = |out: &mut String, depth: usize| {
        out.push('\n');
        out.push_str(&" ".repeat(spaces * depth));
    };
    match value {
        Json::Arr(items) if !items.is_empty() => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                pad(out, depth + 1);
                write_pretty_into(item, spaces, depth + 1, out);
            }
            pad(out, depth);
            out.push(']');
        }
        Json::Obj(members) if !members.is_empty() => {
            out.push('{');
            for (i, (key, member)) in members.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                pad(out, depth + 1);
                write_string(key, out);
                out.push_str(": ");
                write_pretty_into(member, spaces, depth + 1, out);
            }
            pad(out, depth);
            out.push('}');
        }
        other => write_into(other, out),
    }
}
