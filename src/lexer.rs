// Akkhara Lexer
// Converts raw source text into a flat stream of tokens (each tagged with its
// source line number). Statement boundaries are no longer one-per-physical-line:
// collection literals (list/tuple/set/dict/table) can span multiple lines, so
// the parser splits statements by scanning for the sentence terminator '။' at
// bracket depth 0.

#[derive(Debug, Clone, PartialEq)]
pub enum Tok {
    Ident(String), // Myanmar keyword or a variable/identifier name
    Str(String),   // string literal contents (without quotes)
    Num(String),   // normalized ascii numeral text, may contain '.'
    Op(char),      // + - * /
    Cmp(String),   // < > == != <= >=
    End,           // ။  (end of sentence)
    LBracket,      // [
    RBracket,      // ]
    LParen,        // (
    RParen,        // )
    LBrace,        // {
    RBrace,        // }
    Comma,         // ,
}

#[derive(Debug, Clone)]
pub struct Token {
    pub tok: Tok,
    pub line: usize,
}

fn myanmar_digit_to_ascii(c: char) -> Option<char> {
    match c {
        '၀' => Some('0'),
        '၁' => Some('1'),
        '၂' => Some('2'),
        '၃' => Some('3'),
        '၄' => Some('4'),
        '၅' => Some('5'),
        '၆' => Some('6'),
        '၇' => Some('7'),
        '၈' => Some('8'),
        '၉' => Some('9'),
        _ => None,
    }
}

fn is_digit_char(c: char) -> bool {
    c.is_ascii_digit() || myanmar_digit_to_ascii(c).is_some()
}

fn is_word_delim(c: char) -> bool {
    c.is_whitespace()
        || c == '"'
        || c == '#'
        || c == '+'
        || c == '-'
        || c == '*'
        || c == '/'
        || c == '%'
        || c == '['
        || c == ']'
        || c == '('
        || c == ')'
        || c == '{'
        || c == '}'
        || c == ','
        || c == '<'
        || c == '>'
        || c == '='
        || c == '!'
}

/// Lex the whole source file into a flat Vec<Token>.
pub fn lex(src: &str) -> Result<Vec<Token>, String> {
    // Strip a leading UTF-8 BOM (\u{FEFF}), which Windows tools such as
    // PowerShell's `Out-File -Encoding utf8` commonly prepend to files.
    // Left in place, it silently attaches itself to the first identifier
    // on line 1, making it compare unequal to the same-looking identifier
    // used later in the file.
    let src = src.strip_prefix('\u{FEFF}').unwrap_or(src);

    let mut tokens: Vec<Token> = Vec::new();
    let chars: Vec<char> = src.chars().collect();
    let n = chars.len();
    let mut i = 0;
    let mut line = 1usize;

    while i < n {
        let c = chars[i];

        if c == '\n' {
            line += 1;
            i += 1;
            continue;
        }

        if c.is_whitespace() {
            i += 1;
            continue;
        }

        if c == '#' {
            // rest of the physical line is a comment
            while i < n && chars[i] != '\n' {
                i += 1;
            }
            continue;
        }

        if c == '"' {
            let mut s = String::new();
            let start_line = line;
            i += 1;
            while i < n && chars[i] != '"' {
                if chars[i] == '\n' {
                    line += 1;
                }
                s.push(chars[i]);
                i += 1;
            }
            if i >= n {
                return Err(format!(
                    "E003 လိုင်း {} တွင် string တန်ဖိုးအတွက် ပိတ် '\"' မရှိပါ။",
                    start_line
                ));
            }
            i += 1; // skip closing quote
            tokens.push(Token {
                tok: Tok::Str(s),
                line: start_line,
            });
            continue;
        }

        if c == '+' || c == '-' || c == '*' || c == '/' || c == '%' {
            tokens.push(Token {
                tok: Tok::Op(c),
                line,
            });
            i += 1;
            continue;
        }

        if c == '[' {
            tokens.push(Token { tok: Tok::LBracket, line });
            i += 1;
            continue;
        }
        if c == ']' {
            tokens.push(Token { tok: Tok::RBracket, line });
            i += 1;
            continue;
        }
        if c == '(' {
            tokens.push(Token { tok: Tok::LParen, line });
            i += 1;
            continue;
        }
        if c == ')' {
            tokens.push(Token { tok: Tok::RParen, line });
            i += 1;
            continue;
        }
        if c == '{' {
            tokens.push(Token { tok: Tok::LBrace, line });
            i += 1;
            continue;
        }
        if c == '}' {
            tokens.push(Token { tok: Tok::RBrace, line });
            i += 1;
            continue;
        }
        if c == ',' {
            tokens.push(Token { tok: Tok::Comma, line });
            i += 1;
            continue;
        }

        if c == '<' || c == '>' {
            if i + 1 < n && chars[i + 1] == '=' {
                tokens.push(Token {
                    tok: Tok::Cmp(format!("{}=", c)),
                    line,
                });
                i += 2;
            } else {
                tokens.push(Token {
                    tok: Tok::Cmp(c.to_string()),
                    line,
                });
                i += 1;
            }
            continue;
        }

        if c == '=' {
            if i + 1 < n && chars[i + 1] == '=' {
                tokens.push(Token {
                    tok: Tok::Cmp("==".to_string()),
                    line,
                });
                i += 2;
            } else {
                // A lone '=' isn't meaningful in Akkhara syntax; skip it so
                // the parser reports a clear syntax error at the statement
                // level rather than the lexer choking on it.
                i += 1;
            }
            continue;
        }

        if c == '!' {
            if i + 1 < n && chars[i + 1] == '=' {
                tokens.push(Token {
                    tok: Tok::Cmp("!=".to_string()),
                    line,
                });
                i += 2;
            } else {
                i += 1;
            }
            continue;
        }

        if is_digit_char(c) {
            let mut s = String::new();
            while i < n {
                let cc = chars[i];
                if cc.is_ascii_digit() {
                    s.push(cc);
                    i += 1;
                } else if let Some(d) = myanmar_digit_to_ascii(cc) {
                    s.push(d);
                    i += 1;
                } else if cc == '.'
                    && i + 1 < n
                    && (chars[i + 1].is_ascii_digit()
                    || myanmar_digit_to_ascii(chars[i + 1]).is_some())
                {
                    s.push('.');
                    i += 1;
                } else {
                    break;
                }
            }
            tokens.push(Token {
                tok: Tok::Num(s),
                line,
            });
            continue;
        }

        // Otherwise: gather a "word" (identifier / keyword) until a delimiter.
        let mut w = String::new();
        while i < n && !is_word_delim(chars[i]) {
            w.push(chars[i]);
            i += 1;
        }
        if w.is_empty() {
            i += 1;
            continue;
        }

        // A word may end with the sentence terminator '။' fused onto it,
        // e.g. "ဖြစ်၏။", "ဖော်ပြပါ။", "မေးပါ။", "ပြောင်းပါ။".
        let (mut core, end_tok) = if let Some(stripped) = w.strip_suffix('။') {
            (stripped.to_string(), true)
        } else {
            (w, false)
        };

        // Keywords are sometimes fused directly onto a preceding word with
        // no space, e.g. "ကိန်းပြည့်သို့ ပြောင်းပါ" written as one run
        // "ကိန်းပြည့်သို့ပြောင်းပါ", or "1 တိုးခြင်းဖြင့်". Greedily peel
        // known keyword suffixes off the end -- but NOT type-name keywords
        // (ကိန်းပြည့်, စာရင်း, etc.): those are "content" words that a
        // legitimate identifier can easily end in (e.g. a class named
        // "လူစာရင်း" ends in "စာရင်း", the "list" type keyword, and got
        // wrongly split in two when type names were peelable). The
        // remaining entries are short grammatical/function words that are
        // much less likely to collide with a user-chosen identifier.
        const KEYWORDS: &[&str] = &[
            "ပြောင်းပါ",
            "ဖော်ပြပါ",
            "မြှောက်ပါ",
            "လျော့ပါ",
            "စားပါ",
            "တိုးပါ",
            "မေးပါ",
            "အတွက်",
            "ဖြစ်၏",
            "သည်",
            "မှာ",
            "ကို",
            "သို့",
        ];
        let mut suffix_tokens: Vec<String> = Vec::new();
        loop {
            let mut matched = false;
            for kw in KEYWORDS {
                if core.len() > kw.len() && core.ends_with(kw) {
                    core.truncate(core.len() - kw.len());
                    suffix_tokens.push(kw.to_string());
                    matched = true;
                    break;
                }
            }
            if !matched {
                break;
            }
        }

        if !suffix_tokens.is_empty() {
            if !core.is_empty() {
                tokens.push(Token {
                    tok: Tok::Ident(core),
                    line,
                });
            }
            for kw in suffix_tokens.into_iter().rev() {
                tokens.push(Token {
                    tok: Tok::Ident(kw),
                    line,
                });
            }
        } else if !core.is_empty() {
            tokens.push(Token {
                tok: Tok::Ident(core),
                line,
            });
        }

        if end_tok {
            tokens.push(Token { tok: Tok::End, line });
        }
    }

    Ok(tokens)
}
