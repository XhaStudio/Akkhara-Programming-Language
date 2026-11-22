//! One renderer for a message that has to point at the source: `E###` text,
//! then the line itself, a caret where the problem is, and the hint that says
//! what belongs there.
//!
//! The annotated errors are: a declaration with no value (`E001`), a
//! statement missing its `။`/`;` terminator (`E002`), an unterminated string
//! literal (`E003`), brackets that are not correctly paired (`E004`),
//! dictionary entries in the old shape (`E005`), a missing `ကို` particle
//! (`E006`), a missing value (`E007`), the removed `ပြောင်းပါ` sentence
//! (`E008`/`E009`), a math statement with no amount (`E010`), the
//! `if`/`while` header mistakes (`E011`-`E013`, `E015`-`E017`), and the
//! `အလုပ်`/`ပုံသေ`/call header mistakes (`E018`-`E029`, `E068`-`E070`, `E095`),
//! library imports and calls (`E072`, `E075`-`E077`, `E084`, `E093`, `E094`),
//! the eng declaration/block/value forms (`E100`-`E102`, `E106`, `E109`,
//! `E118`, `E137`, `E150`-`E153`, `E156`-`E158`), and the runtime errors the
//! interpreter raises for declarations and destructuring (`E103`-`E105`, and
//! floor division by zero, `E073`/`E074`, plus the `E154`/`E155` split).
//! E001-E003 are
//! missing at the end of the line, so their caret sits one past the last
//! character. E004 marks the two brackets that do not pair, wherever they are
//! on the line. Any other message is returned unchanged.

/// The words that close an `if` header, in either spelling.
const THEN_WORDS: [&str; 4] = ["ဖြစ်ရင်", "ဖြစ်လျှင်", "မဖြစ်ရင်", "မဖြစ်လျှင်"];

/// The words that close a `while` header, in either spelling.
const WHILE_WORDS: [&str; 4] = ["ဖြစ်နေရင်", "ဖြစ်နေစဉ်", "မဖြစ်နေရင်", "မဖြစ်နေစဉ်"];

/// The hint under a bracket pair that does not close properly.
const BRACKET_LABEL: &str = "ကွင်းများမှန်ကန်စွာ မတွဲထားပါ။";

/// One source line under a caret, for the errors above.
struct Hint {
    line: usize,
    label: &'static str,
}

/// The line number a message names: `E001 လိုင်း 3 တွင် ...`.
fn line_number(message: &str) -> Option<usize> {
    let rest = message.split_once("လိုင်း ")?.1;
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    digits.parse().ok()
}

/// What the caret should say, when this is a message the renderer annotates.
fn hint(message: &str) -> Option<Hint> {
    // `E001` also covers other syntax mistakes (`… သည် လိုအပ်ပါသည်။`,
    // malformed catch clauses), which no caret helps with.
    if message.starts_with("E001") && message.contains("တန်ဖိုးသတ်မှတ်ထားခြင်းမရှိပါ")
    {
        return Some(Hint {
            line: line_number(message)?,
            label: "Value တစ်ခုခုလိုအပ်",
        });
    }
    // `E002` is the missing statement terminator: `။` in a Myanmar statement,
    // `;` in an eng one.
    if message.starts_with("E002") {
        let label = if message.contains("';'") {
            "`;` မပိတ်ထား"
        } else {
            "`။` မပိတ်ထား"
        };
        return Some(Hint {
            line: line_number(message)?,
            label,
        });
    }
    // `E003` is an unterminated string literal. The same code also names an
    // unclosed bracket, whose message has nothing to do with a quote.
    if message.starts_with("E003") {
        if message.contains("string တန်ဖိုးအတွက်") {
            return Some(Hint {
                line: line_number(message)?,
                label: "`\"` မပိတ်ထား",
            });
        }
        if message.contains("စာသား တန်ဖိုးအတွက်") {
            return Some(Hint {
                line: line_number(message)?,
                label: "`\"\"\"` မပိတ်ထား",
            });
        }
    }
    None
}

/// One bracket found in the source, with the line it is on and the column it
/// occupies when that line is drawn (Myanmar syllables count as one column).
struct Bracket {
    ch: char,
    line: usize,
    column: usize,
}

/// The first pair of brackets that do not match, scanning the source the way
/// the lexer reads it: `#` comments and string literals are skipped, so a
/// bracket inside `"..."` is text, not structure. `None` when the source has
/// no mismatched pair. The opening bracket is `None` when a closing bracket
/// has nothing to close.
fn first_bracket_mismatch(source: &str) -> Option<(Option<Bracket>, Bracket)> {
    let chars: Vec<(usize, char)> = source.char_indices().collect();
    let mut stack: Vec<Bracket> = Vec::new();
    let mut line = 1usize;
    let mut line_start = 0usize;
    let mut i = 0usize;
    while i < chars.len() {
        let (byte, c) = chars[i];
        if c == '\n' {
            line += 1;
            line_start = byte + 1;
            i += 1;
            continue;
        }
        if c == '#' {
            while i < chars.len() && chars[i].1 != '\n' {
                i += 1;
            }
            continue;
        }
        if c == '"' {
            let triple = i + 2 < chars.len() && chars[i + 1].1 == '"' && chars[i + 2].1 == '"';
            i += if triple { 3 } else { 1 };
            while i < chars.len() {
                if triple
                    && i + 2 < chars.len()
                    && chars[i].1 == '"'
                    && chars[i + 1].1 == '"'
                    && chars[i + 2].1 == '"'
                {
                    i += 3;
                    break;
                }
                if !triple && chars[i].1 == '"' {
                    i += 1;
                    break;
                }
                if chars[i].1 == '\\' {
                    i += 2;
                    continue;
                }
                if chars[i].1 == '\n' {
                    line += 1;
                    line_start = chars[i].0 + 1;
                }
                i += 1;
            }
            continue;
        }
        let column = crate::text::len(&source[line_start..byte]) as usize;
        match c {
            '[' | '(' | '{' => stack.push(Bracket {
                ch: c,
                line,
                column,
            }),
            ']' | ')' | '}' => {
                let wanted = match c {
                    ']' => '[',
                    ')' => '(',
                    _ => '{',
                };
                match stack.pop() {
                    Some(open) if open.ch == wanted => {}
                    open => {
                        return Some((
                            open,
                            Bracket {
                                ch: c,
                                line,
                                column,
                            },
                        ));
                    }
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

/// The function word an `E006` message quotes, between the first pair of
/// double quotes: `E006 ...၌ "<fname>။" ...`.
fn quoted_keyword(message: &str) -> Option<&str> {
    let rest = message.split_once('"')?.1;
    Some(rest.split_once('"')?.0)
}

/// The `E006` report: the source line, a caret at the spot `ကို` belongs (in
/// front of the function word), and the hint that says so.
///
/// ```text
/// E006 လိုင်း 1၌ "ဖော်ပြပါ။" လုပ်ဆောင်ရန် "ဖော်ပြပါ။" ၏အရှေ့၌ "ကို" ခံရေးရန်လိုအပ်သည်။
///
/// line 1| "hi" ဖော်ပြပါ။
///              ↑
///              |
///              |
///         "ကို" ခံရေးရန်
/// ```
fn missing_particle_snippet(message: &str, source: &str) -> Option<String> {
    let line = line_number(message)?;
    let text = source.lines().nth(line.saturating_sub(1))?;
    let text = text.trim_end_matches('\r');
    let quoted = quoted_keyword(message)?;
    let keyword = quoted.strip_suffix('။').unwrap_or(quoted);
    // `ဖော်ပြပါ` may have been written with its short spelling `ပြပါ`, so try
    // the spelling the source can actually contain as well.
    let short = if keyword == "ဖော်ပြပါ" {
        Some("ပြပါ")
    } else {
        None
    };
    let byte = text
        .rfind(keyword)
        .or_else(|| short.and_then(|short| text.rfind(short)))?;
    let gutter = format!("line {}| ", line);
    let mut column = gutter.chars().count() + crate::text::len(&text[..byte]) as usize;
    // The caret sits where the missing `ကို` goes: just before the function
    // word. When a space already separates it from the value, point at that
    // space instead of the word's first character.
    if text[..byte]
        .chars()
        .next_back()
        .is_some_and(char::is_whitespace)
    {
        column -= 1;
    }
    let pad = " ".repeat(column);
    Some(format!(
        "{}\n\n{}{}\n{}↑\n{}|\n{}|\n        {}",
        message, gutter, text, pad, pad, pad, "\"ကို\" ခံရေးရန်"
    ))
}

/// The `E007` report: the source line, a caret at the spot the value belongs
/// (in front of `ကို`), and the hint that says the value is missing.
///
/// ```text
/// E007 လိုင်း 1၌ "ဖော်ပြပါ။" လုပ်ဆောင်ရန် တန်ဖိုး မရှိသဖြင့် "ဖော်ပြပါ။" ကို မလုပ်ဆောင်နိုင်ပါ။
///
/// line 1|   ကို ဖော်ပြပါ။
///           ↑
///           |
///           |
///         တန်ဖိုး မရှိခြင်း
/// ```
fn missing_value_snippet(message: &str, source: &str) -> Option<String> {
    let line = line_number(message)?;
    let text = source.lines().nth(line.saturating_sub(1))?;
    let text = text.trim_end_matches('\r');
    // The value belongs right before the `ကို` particle. If the line has no
    // particle at all, point at the function word instead.
    let byte = text.find("ကို").or_else(|| {
        let quoted = quoted_keyword(message)?;
        let keyword = quoted.strip_suffix('။').unwrap_or(quoted);
        text.find(keyword)
    })?;
    let gutter = format!("line {}| ", line);
    let column = gutter.chars().count() + crate::text::len(&text[..byte]) as usize;
    let pad = " ".repeat(column);
    Some(format!(
        "{}\n\n{}{}\n{}↑\n{}|\n{}|\n        {}",
        message, gutter, text, pad, pad, pad, "တန်ဖိုး မရှိခြင်း"
    ))
}

/// The report for a message whose fix goes at the first `needle` in the line
/// it names: the line, a caret under the needle, and `label`.
fn at_needle_snippet(message: &str, source: &str, needles: &[&str], label: &str) -> Option<String> {
    let line = line_number(message)?;
    let text = source.lines().nth(line.saturating_sub(1))?;
    let text = text.trim_end_matches('\r');
    let byte = needles
        .iter()
        .filter_map(|needle| text.find(needle))
        .min()?;
    Some(draw_snippet(message, line, text, byte, label))
}

/// The report for a message whose fix belongs at the end of the line it names.
fn at_end_snippet(message: &str, source: &str, label: &str) -> Option<String> {
    let line = line_number(message)?;
    let text = source.lines().nth(line.saturating_sub(1))?;
    let text = text.trim_end_matches('\r');
    Some(draw_snippet(message, line, text, text.len(), label))
}

/// The report for a message whose fix goes just after `needle`: the name a
/// keyword like `အလုပ်` asked for, for instance.
fn after_needle_snippet(message: &str, source: &str, needle: &str, label: &str) -> Option<String> {
    let line = line_number(message)?;
    let text = source.lines().nth(line.saturating_sub(1))?;
    let text = text.trim_end_matches('\r');
    let at = text.find(needle)?;
    Some(draw_snippet(message, line, text, at + needle.len(), label))
}

/// The shared report shape: message, blank line, numbered source line, a
/// caret and two bars at `byte`, then `label`. `byte` is a byte offset into
/// `text`, and `text::len` counts Myanmar syllables so the caret lines up
/// with what the reader sees.
fn draw_snippet(message: &str, line: usize, text: &str, byte: usize, label: &str) -> String {
    let gutter = format!("line {}| ", line);
    let column = gutter.chars().count() + crate::text::len(&text[..byte]) as usize;
    let pad = " ".repeat(column);
    format!(
        "{}\n\n{}{}\n{}↑\n{}|\n{}|\n        {}",
        message, gutter, text, pad, pad, pad, label
    )
}

/// A row of `mark` characters at each of `columns`, long enough to hold the
/// rightmost one.
fn marks(columns: &[usize], mark: char) -> String {
    let width = columns.iter().copied().max().unwrap_or(0);
    let mut row = vec![' '; width + 1];
    for &column in columns {
        if let Some(cell) = row.get_mut(column) {
            *cell = mark;
        }
    }
    row.into_iter().collect()
}

/// The `E004` report: the source line with a caret under each of the two
/// brackets that do not pair.
///
/// ```text
/// E004 လိုင်း 1 တွင် ကွင်းများ ({}/[]/()) မှန်ကန်စွာ မတွဲထားပါ။
/// line 1| (]
///         ↑↑
///         ||
///         ||
///         ကွင်းများမှန်ကန်စွာ မတွဲထားပါ။
/// ```
///
/// When the two brackets are on different lines only the closing one is
/// marked, because the report names one line. A source with no mismatched
/// pair returns `None`, so the caller keeps the message as it was.
fn bracket_snippet(message: &str, source: &str) -> Option<String> {
    let (open, close) = first_bracket_mismatch(source)?;
    let text = source.lines().nth(close.line.saturating_sub(1))?;
    let text = text.trim_end_matches('\r');
    let gutter = format!("line {}| ", close.line);
    let gutter_len = gutter.chars().count();
    let mut columns = vec![gutter_len + close.column];
    if let Some(open) = open {
        if open.line == close.line {
            columns.insert(0, gutter_len + open.column);
        }
    }
    let carets = marks(&columns, '↑');
    let pipes = marks(&columns, '|');
    Some(format!(
        "{}\n{}{}\n{}\n{}\n{}\n        {}",
        message, gutter, text, carets, pipes, pipes, BRACKET_LABEL
    ))
}

/// `message` with the line it names drawn under it:
///
/// ```text
/// E002 လိုင်း 1 ၏ စာကြောင်းအဆုံးတွင် '။' မရှိပါ။
/// line 1| "hi" ကို ဖော်ပြပါ
///                              ↑
///                              |
///                              |
///                         `။` မပိတ်ထား
/// ```
///
/// A message with no line, or a line the source does not have, is returned
/// as-is: the error text is never lost to a rendering problem.
pub(crate) fn with_snippet(message: &str, source: &str) -> String {
    // A message that already carries a report is left alone: a library error
    // is rendered against the library's own source before it propagates, and
    // the caller must not draw it again against the entry file.
    if message.contains("\n\nline ") {
        return message.to_string();
    }
    if message.starts_with("E004") && message.contains("မတွဲထားပါ") {
        return bracket_snippet(message, source).unwrap_or_else(|| message.to_string());
    }
    if message.starts_with("E006") && message.contains("ခံရေးရန်") {
        return missing_particle_snippet(message, source).unwrap_or_else(|| message.to_string());
    }
    if message.starts_with("E007") && message.contains("တန်ဖိုး မရှိသဖြင့်")
    {
        return missing_value_snippet(message, source).unwrap_or_else(|| message.to_string());
    }
    // `E008`/`E009`: the removed `<value> ကို <type> သို့ ပြောင်းပါ။` sentence.
    if (message.starts_with("E008") || message.starts_with("E009"))
        && message.contains("ဖယ်ရှားလိုက်ပါပြီ")
    {
        return at_needle_snippet(message, source, &["ပြောင်းပါ"], "\"ပြောင်းပါ\" မလိုအပ်တော့ပါ")
            .unwrap_or_else(|| message.to_string());
    }
    // `E010`: a math statement with no amount, e.g. `ကို တိုးပါ။`. The amount
    // belongs in front of the `ကို`, the same spot `E007` points at.
    if message.starts_with("E010") && message.contains("ကိုလုပ်ဆောင်ရန် တန်ဖိုး မရှိသဖြင့်")
    {
        return missing_value_snippet(message, source).unwrap_or_else(|| message.to_string());
    }
    // `E011`/`E015`: the `if`/`while` header has no condition. The condition
    // belongs in front of the word that closes the header.
    if (message.starts_with("E011") || message.starts_with("E015"))
        && message.contains("အခြေအနေ (စည်းကမ်းချက်) လိုအပ်ပါသည်")
    {
        let words: Vec<&str> = THEN_WORDS
            .iter()
            .copied()
            .chain(WHILE_WORDS.iter().copied())
            .collect();
        return at_needle_snippet(message, source, &words, "အခြေအနေ (စည်းကမ်းချက်) လိုအပ်")
            .unwrap_or_else(|| message.to_string());
    }
    // `E012`/`E016`: a condition that is written in a shape the parser cannot
    // read. The bracketed condition is the part to look at.
    if (message.starts_with("E012") || message.starts_with("E016"))
        && message.contains("အခြေအနေ ရေးသားပုံ မှားနေပါသည်")
    {
        let label = "အခြေအနေ ရေးသားပုံ မှား";
        return at_needle_snippet(message, source, &["("], label)
            .or_else(|| at_end_snippet(message, source, label))
            .unwrap_or_else(|| message.to_string());
    }
    // `E013`/`E017`: the header's closing word (`ဖြစ်လျှင်`, `ဖြစ်နေစဉ်`, ...)
    // is missing, so the fix goes at the end of the header line.
    if message.starts_with("E013") && message.contains("အခြေအနေ အပြီးမှာ")
    {
        return at_end_snippet(message, source, "\"ဖြစ်လျှင်\" လိုအပ်")
            .unwrap_or_else(|| message.to_string());
    }
    if message.starts_with("E017") && message.contains("အခြေအနေ အပြီးမှာ")
    {
        return at_end_snippet(message, source, "\"ဖြစ်နေစဉ်\" လိုအပ်")
            .unwrap_or_else(|| message.to_string());
    }
    // `E018`: the `အလုပ်` header has no function name after the keyword.
    if message.starts_with("E018") && message.contains("အတွက် အမည် လိုအပ်ပါသည်")
    {
        return after_needle_snippet(message, source, "အလုပ်", "အလုပ် အမည် လိုအပ်")
            .unwrap_or_else(|| message.to_string());
    }
    // `E019`: the `အလုပ်` parameter list is malformed. The list's `(` is the
    // part to look at; a header without one points just past `အတွက်`.
    if message.starts_with("E019") && message.contains("အချက် ရေးသားပုံ မှားနေပါသည်")
    {
        return at_needle_snippet(message, source, &["("], "အချက် ရေးသားပုံ မှား")
            .or_else(|| after_needle_snippet(message, source, "အတွက်", "အချက် ရေးသားပုံ မှား"))
            .unwrap_or_else(|| message.to_string());
    }
    // `E020`: the `အလုပ်` header is not closed by `ဖြင့်` / `သည်`.
    if message.starts_with("E020") && message.contains("ခေါင်းစီး အပြီးမှာ")
    {
        return at_end_snippet(message, source, "\"ဖြင့်\" / \"သည်\" လိုအပ်")
            .unwrap_or_else(|| message.to_string());
    }
    // `E021`/`E069`: the word where the called function's name belongs is not
    // a name. Without an assignment that spot is the name itself, in front of
    // `ကို`; the assigning form names it after `အတွက်`.
    if message.starts_with("E069") && message.contains("ခေါ်လိုသော အလုပ်၏ အမည် မှားနေပါသည်")
    {
        return after_needle_snippet(message, source, "အတွက်", "အလုပ် အမည် မှား")
            .unwrap_or_else(|| message.to_string());
    }
    if message.starts_with("E021") && message.contains("ခေါ်လိုသော အလုပ်၏ အမည် မှားနေပါသည်")
    {
        return at_needle_snippet(message, source, &["ကို"], "အလုပ် အမည် မှား")
            .unwrap_or_else(|| message.to_string());
    }
    // `E022`/`E070`: the call sentence dropped its `ကို` particle.
    if (message.starts_with("E022") || message.starts_with("E070"))
        && message.contains("ပုံစံဖြင့် ရေးရပါမည်")
    {
        return at_needle_snippet(message, source, &["လုပ်ပါ", "လုပ်ရန်"], "\"ကို\" ခံရေးရန်")
            .unwrap_or_else(|| message.to_string());
    }
    // `E023`: the argument list of `လုပ်ရန်` has no closing `ဖြင့်`.
    if message.starts_with("E023") && message.contains("အချက် အပြီးမှာ") {
        return at_end_snippet(message, source, "\"ဖြင့်\" လိုအပ်")
            .unwrap_or_else(|| message.to_string());
    }
    // `E024`: the argument belongs in front of `ဖြင့်`.
    if message.starts_with("E024") && message.contains("အချက် လိုအပ်ပါသည်")
    {
        return at_needle_snippet(message, source, &["ဖြင့်"], "အချက် လိုအပ်")
            .or_else(|| at_end_snippet(message, source, "အချက် လိုအပ်"))
            .unwrap_or_else(|| message.to_string());
    }
    // `E068`: the assigning call form has no variable name in front of `အတွက်`.
    if message.starts_with("E068") && message.contains("ကိန်းရှင် အမည် လိုအပ်ပါသည်")
    {
        return at_needle_snippet(message, source, &["အတွက်"], "ကိန်းရှင် အမည် လိုအပ်")
            .unwrap_or_else(|| message.to_string());
    }
    // `E025`: the `ပုံသေ` header has no class name after the keyword.
    if message.starts_with("E025") && message.contains("အတွက် အမည် လိုအပ်ပါသည်")
    {
        return after_needle_snippet(message, source, "ပုံသေ", "ပုံသေ အမည် လိုအပ်")
            .unwrap_or_else(|| message.to_string());
    }
    // `E026`: the `ပုံသေ` header is not closed by `သည်` or `။`.
    if message.starts_with("E026") && message.contains("၏ အပြီးမှာ") {
        return at_end_snippet(message, source, "\"သည်\" သို့မဟုတ် '။' လိုအပ်")
            .unwrap_or_else(|| message.to_string());
    }
    // `E027`: something other than an `အလုပ်` sits inside the class body, so
    // the statement itself is flagged.
    if message.starts_with("E027") && message.contains("များသာ ပါဝင်နိုင်ပါသည်")
    {
        let Some(line) = line_number(message) else {
            return message.to_string();
        };
        let Some(text) = source.lines().nth(line.saturating_sub(1)) else {
            return message.to_string();
        };
        let text = text.trim_end_matches('\r');
        return draw_snippet(message, line, text, 0, "\"အလုပ်\" များသာ ပါဝင်နိုင်");
    }
    // `E095`: the constructor's parameter list is malformed.
    if message.starts_with("E095") && message.contains("တည်ဆောက်သူ အချက် ရေးသားပုံ မှားနေပါသည်")
    {
        return at_needle_snippet(message, source, &["("], "တည်ဆောက်သူ အချက် မှား")
            .or_else(|| after_needle_snippet(message, source, "ပုံသေ", "တည်ဆောက်သူ အချက် မှား"))
            .unwrap_or_else(|| message.to_string());
    }
    // `E028`: the class has no constructor, so the fix goes inside its body
    // (the end of the header line).
    if message.starts_with("E028") && message.contains("တည်ဆောက်သူ အနေဖြင့်")
    {
        return at_end_snippet(message, source, "တည်ဆောက်သူ အလုပ် လိုအပ်")
            .unwrap_or_else(|| message.to_string());
    }
    // `E029`: the removed `အသစ်` marker is what the caret points at.
    if message.starts_with("E029") && message.contains("\"အသစ်\" မလိုအပ်တော့ပါ")
    {
        return at_needle_snippet(message, source, &["အသစ်"], "\"အသစ်\" မလိုအပ်တော့ပါ")
            .unwrap_or_else(|| message.to_string());
    }
    // --- library imports and library calls --------------------------------
    // `E072`: the `နည်းပညာများ` list is not a comma-separated list of names.
    if message.starts_with("E072") && message.contains("နည်းပညာ အမည်များကို")
    {
        return after_needle_snippet(
            message,
            source,
            "နည်းပညာများ",
            "နည်းပညာ အမည်များ \",\" ဖြင့် ခွဲရန်",
        )
        .unwrap_or_else(|| message.to_string());
    }
    // `E093`: a library alias is missing after `အဖြစ်`.
    if message.starts_with("E093") && message.contains("အမည်လွှဲ အမည် တစ်ခု လိုအပ်ပါသည်")
    {
        return after_needle_snippet(message, source, "အဖြစ်", "အမည်လွှဲ အမည် လိုအပ်")
            .unwrap_or_else(|| message.to_string());
    }
    // `E094`: the `ကို` particle in front of `သုံးမည်` is missing.
    if message.starts_with("E094") && message.contains("ကို\" ခံရေးရန် လိုအပ်ပါသည်")
    {
        return at_needle_snippet(message, source, &["သုံးမည်", "အသုံးပြုပါ"], "\"ကို\" ခံရေးရန်")
            .unwrap_or_else(|| message.to_string());
    }
    // `E075`: no function name in front of `အဖြစ်`.
    if message.starts_with("E075") && message.contains("အဖြစ်\" ၏ ရှေ့တွင်")
    {
        return at_needle_snippet(message, source, &["အဖြစ်"], "အလုပ် အမည် လိုအပ်")
            .unwrap_or_else(|| message.to_string());
    }
    // `E076`: no alias name after `အဖြစ်`.
    if message.starts_with("E076") && message.contains("အမည်လွှဲ အမည် အနည်းဆုံး")
    {
        return after_needle_snippet(message, source, "အဖြစ်", "အမည်လွှဲ အမည် လိုအပ်")
            .unwrap_or_else(|| message.to_string());
    }
    // `E077`: the alias list itself is malformed.
    if message.starts_with("E077") && message.contains("တစ်ခုချင်း ရေးပါ")
    {
        return after_needle_snippet(message, source, "အဖြစ်", "အမည်လွှဲများကို \",\" ဖြင့် ခွဲရန်")
            .unwrap_or_else(|| message.to_string());
    }
    // `E084`: a library call (`<lib> ၏ <fn> ...`) is written wrongly.
    if message.starts_with("E084") && message.contains("နည်းပညာ အလုပ် ခေါ်ရန်")
    {
        return at_needle_snippet(message, source, &["၏"], "နည်းပညာ အလုပ် ခေါ်ပုံ မှား")
            .unwrap_or_else(|| message.to_string());
    }
    // --- eng (English-spelled) declarations, blocks and values ------------
    // `E100`: an eng declaration that is not `<name> :<type> = <value>;`.
    if message.starts_with("E100") && message.contains("ကို မျှော်လင့်ပါသည်")
    {
        return at_needle_snippet(message, source, &[":"], ":<type> = <value>; လိုအပ်")
            .or_else(|| at_end_snippet(message, source, ":<type> = <value>; လိုအပ်"))
            .unwrap_or_else(|| message.to_string());
    }
    // `E101`: the declared type is not one of int/float/str/bool. The message
    // quotes the name it saw, so the caret can underline exactly that word.
    if message.starts_with("E101") && message.contains("ဆိုသော အမျိုးအစားကို မသိပါ")
    {
        let label = "အမျိုးအစား မမှန်ကန်";
        let quoted = quoted_keyword(message);
        return quoted
            .and_then(|name| at_needle_snippet(message, source, &[name], label))
            .or_else(|| after_needle_snippet(message, source, ":", label))
            .unwrap_or_else(|| message.to_string());
    }
    // `E102`: `=` with no value after it.
    if message.starts_with("E102") && message.contains("တန်ဖိုး မပါပါ") {
        return at_end_snippet(message, source, "တန်ဖိုး လိုအပ်").unwrap_or_else(|| message.to_string());
    }
    // `E106`: an eng library call written without its argument list.
    if message.starts_with("E106") && message.contains("(...);` ကို မျှော်လင့်ပါသည်")
    {
        return at_needle_snippet(message, source, &["("], "(...) ဖြင့် ခေါ်ရန်")
            .or_else(|| at_end_snippet(message, source, "(...) ဖြင့် ခေါ်ရန်"))
            .unwrap_or_else(|| message.to_string());
    }
    // `E109`: an eng block statement (`if`/`while`/`for`/...) without its
    // `{ ... }` body.
    if message.starts_with("E109") {
        return at_needle_snippet(message, source, &["{"], "{ ... } လိုအပ်")
            .or_else(|| at_end_snippet(message, source, "{ ... } လိုအပ်"))
            .unwrap_or_else(|| message.to_string());
    }
    // `E118`: a `use` statement without a library name.
    if message.starts_with("E118") && message.contains("ကို မျှော်လင့်ပါသည်")
    {
        return after_needle_snippet(message, source, "use", "library အမည် လိုအပ်")
            .or_else(|| at_needle_snippet(message, source, &["use"], "library အမည် လိုအပ်"))
            .unwrap_or_else(|| message.to_string());
    }
    // `E137`: an eng function alias without its `as <alias>` tail.
    if message.starts_with("E137") && message.contains("as alias;` ကို မျှော်လင့်ပါသည်")
    {
        return at_needle_snippet(message, source, &["as"], "as <alias> လိုအပ်")
            .or_else(|| at_end_snippet(message, source, "as <alias> လိုအပ်"))
            .unwrap_or_else(|| message.to_string());
    }
    // `E150`: a `fn` value that is not the lambda form.
    if message.starts_with("E150") && message.contains("lambda ကို မျှော်လင့်ပါသည်")
    {
        return at_needle_snippet(message, source, &["fn"], "fn(<params>) { <body> } လိုအပ်")
            .unwrap_or_else(|| message.to_string());
    }
    // `E151`: the eng ternary is missing its `:` and else branch.
    if message.starts_with("E151") && message.contains("နှစ်ခုလုံး") {
        return at_needle_snippet(message, source, &["?"], "`:` နှင့် else လိုအပ်")
            .or_else(|| at_end_snippet(message, source, "`:` နှင့် else လိုအပ်"))
            .unwrap_or_else(|| message.to_string());
    }
    // `E152`: an eng destructuring statement in the wrong shape.
    if message.starts_with("E152") && message.contains("[a, b] = <list>") {
        return at_needle_snippet(message, source, &["["], "[a, b] = <list> လိုအပ်")
            .unwrap_or_else(|| message.to_string());
    }
    // `E153`: an eng `switch` without its `{ case ... default ... }` body.
    if message.starts_with("E153") && message.contains("switch (<value>)") {
        return at_needle_snippet(message, source, &["switch"], "switch (<value>) { ... } လိုအပ်")
            .or_else(|| at_needle_snippet(message, source, &["{"], "{ ... } လိုအပ်"))
            .unwrap_or_else(|| message.to_string());
    }
    // `E156`: a Myanmar `ရွေးရန်` switch in the wrong shape.
    if message.starts_with("E156") && message.contains("ရွေးရန် (<တန်ဖိုး>)")
    {
        return at_needle_snippet(
            message,
            source,
            &["ရွေးရန်"],
            "\"ရွေးရန် (<တန်ဖိုး>) ... ပြီး။\" လိုအပ်",
        )
        .unwrap_or_else(|| message.to_string());
    }
    // `E157`: a Myanmar conditional expression without `မဟုတ်ရင်`.
    if message.starts_with("E157") && message.contains("မဟုတ်ရင်") {
        return at_needle_snippet(message, source, &["("], "\"ဖြစ်ရင် ... မဟုတ်ရင် ...\" လိုအပ်")
            .or_else(|| at_needle_snippet(message, source, &["ဖြစ်ရင်"], "\"မဟုတ်ရင်\" လိုအပ်"))
            .unwrap_or_else(|| message.to_string());
    }
    // `E158`: a Myanmar destructuring statement in the wrong shape.
    if message.starts_with("E158") && message.contains("[a, b] သည်") {
        return at_needle_snippet(message, source, &["["], "[a, b] ပုံစံဖြင့် ရေးပါ")
            .unwrap_or_else(|| message.to_string());
    }
    // --- runtime errors raised by the interpreter --------------------------
    // `E103`: a declared value does not match its type annotation. The message
    // quotes the name being assigned, so the caret sits under that name.
    if message.starts_with("E103") && message.contains("သတ်မှတ်၍မရပါ") {
        return quoted_keyword(message)
            .and_then(|name| at_needle_snippet(message, source, &[name], "အမျိုးအစား မကိုက်ညီ"))
            .unwrap_or_else(|| message.to_string());
    }
    // `E104`: a write to a name declared with `pin`.
    if message.starts_with("E104") && message.contains("ပြန်မသတ်မှတ်နိုင်ပါ")
    {
        return quoted_keyword(message)
            .and_then(|name| {
                at_needle_snippet(message, source, &[name], "'pin' ဖြင့် ကြေညာထားသော အမည်")
            })
            .unwrap_or_else(|| message.to_string());
    }
    // `E105`: a `pin` declaration that would redeclare an existing name.
    if message.starts_with("E105") && message.contains("ထပ်ကြေညာ၍ မရပါ") {
        return quoted_keyword(message)
            .and_then(|name| at_needle_snippet(message, source, &[name], "အမည် ထပ်နေသည်"))
            .unwrap_or_else(|| message.to_string());
    }
    // `E154`/`E155`: destructuring a value that has no items, or that has a
    // different number of them. The `[` is the statement to look at.
    if (message.starts_with("E154") && message.contains("ခွဲထုတ်ရန် စာရင်း"))
        || (message.starts_with("E155") && message.contains("ခွဲထုတ်ရန် ရေးထားပြီး"))
    {
        let label = "ခွဲထုတ်ရန် အရေအတွက် မကိုက်ညီ";
        return at_needle_snippet(message, source, &["["], label)
            .unwrap_or_else(|| message.to_string());
    }
    // `E073`/`E074`: floor division (`//`) by zero.
    if (message.starts_with("E073") || message.starts_with("E074"))
        && message.contains("သုညဖြင့် အပြည့်ကိန်းစား၍မရပါ")
    {
        return at_needle_snippet(message, source, &["//"], "သုညဖြင့် စား၍မရ")
            .unwrap_or_else(|| message.to_string());
    }
    // `E119`: a library imports one that is still loading (an import cycle).
    // The import statement in the importing library is the line to look at.
    if message.starts_with("E119") && message.contains("အပြန်အလှန် သွင်းယူမှု")
    {
        let label = "အပြန်အလှန် သွင်းယူမှု";
        return quoted_keyword(message)
            .and_then(|name| at_needle_snippet(message, source, &[name], label))
            .or_else(|| at_needle_snippet(message, source, &["နည်းပညာများ"], label))
            .unwrap_or_else(|| message.to_string());
    }
    let Some(Hint { line, label }) = hint(message) else {
        return message.to_string();
    };
    let Some(text) = source.lines().nth(line.saturating_sub(1)) else {
        return message.to_string();
    };
    let text = text.trim_end_matches('\r');
    let gutter = format!("line {}| ", line);
    // The caret sits one past the line's last character, where the missing
    // value or terminator belongs. `text::len` counts Myanmar syllables, so a
    // line of Myanmar script lines up the way it reads.
    let column = gutter.chars().count() + crate::text::len(text) as usize;
    let pad = " ".repeat(column);
    format!(
        "{}\n{}{}\n{}↑\n{}|\n{}|\n        {}",
        message, gutter, text, pad, pad, pad, label
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_value_points_at_the_end_of_the_line() {
        let message = "E001 လိုင်း 1 တွင် x သည် တန်ဖိုးသတ်မှတ်ထားခြင်းမရှိပါ။";
        let rendered = with_snippet(message, "x က ။\n");
        assert!(rendered.starts_with(message), "{rendered}");
        assert!(rendered.contains("line 1| x က ။\n"), "{rendered}");
        assert!(rendered.ends_with("Value တစ်ခုခုလိုအပ်"), "{rendered}");
        assert!(
            rendered.contains("↑\n") && rendered.contains("|\n        Value"),
            "{rendered}"
        );
    }

    #[test]
    fn a_missing_period_points_at_the_end_of_the_line() {
        let message = "E002 လိုင်း 2 ၏ စာကြောင်းအဆုံးတွင် '။' မရှိပါ။";
        let source = "\"a\" ကို ပြပါ။\n\"b\" ကို ပြပါ\n";
        let rendered = with_snippet(message, source);
        assert!(rendered.contains("line 2| \"b\" ကို ပြပါ\n"), "{rendered}");
        assert!(rendered.ends_with("`။` မပိတ်ထား"), "{rendered}");
    }

    #[test]
    fn an_eng_semicolon_names_the_semicolon() {
        let message = "E002 လိုင်း 1 တွင် အဆုံးသတ် ';' မရှိပါ။";
        let rendered = with_snippet(message, "print(\"x\")\n");
        assert!(rendered.contains("line 1| print(\"x\")\n"), "{rendered}");
        assert!(rendered.ends_with("`;` မပိတ်ထား"), "{rendered}");
    }

    #[test]
    fn an_unterminated_string_points_past_the_line() {
        let message = "E003 လိုင်း 1 တွင် string တန်ဖိုးအတွက် ပိတ် '\"' မရှိပါ။";
        let rendered = with_snippet(message, "\"hello\n");
        assert!(rendered.starts_with(message), "{rendered}");
        assert!(rendered.contains("line 1| \"hello\n"), "{rendered}");
        assert!(rendered.ends_with("`\"` မပိတ်ထား"), "{rendered}");
    }

    #[test]
    fn an_unclosed_bracket_keeps_its_own_message() {
        let message = "E003 လိုင်း 1 တွင် '[' ကိုပိတ်ရန် ']' မရှိပါ။";
        assert_eq!(with_snippet(message, "x မှာ [1\n"), message);
    }

    #[test]
    fn a_bracket_pair_on_one_line_marks_both_brackets() {
        let message = "E004 လိုင်း 2 တွင် ကွင်းများ ({}/[]/()) မှန်ကန်စွာ မတွဲထားပါ။";
        let rendered = with_snippet(message, "x သည် 1 ဖြစ်၏။\n(]\n");
        assert!(rendered.starts_with(message), "{rendered}");
        assert!(rendered.contains("\nline 2| (]\n"), "{rendered}");
        let carets = rendered.lines().nth(2).expect("caret row");
        assert_eq!(carets.matches('↑').count(), 2, "{rendered}");
        assert!(carets.trim_end().ends_with("↑↑"), "{carets:?}");
        assert!(rendered.ends_with("ကွင်းများမှန်ကန်စွာ မတွဲထားပါ။"), "{rendered}");
    }

    #[test]
    fn brackets_inside_strings_and_comments_do_not_count() {
        let message = "E004 လိုင်း 1 တွင် ကွင်းများ ({}/[]/()) မှန်ကန်စွာ မတွဲထားပါ။";
        assert_eq!(with_snippet(message, "print(\"]\")\n"), message);
        assert_eq!(with_snippet(message, "# ]\n"), message);
    }

    #[test]
    fn a_missing_particle_points_before_the_function_word() {
        let message = "E006 လိုင်း 1၌ \"ဖော်ပြပါ။\" လုပ်ဆောင်ရန် \"ဖော်ပြပါ။\" ၏အရှေ့၌ \"ကို\" ခံရေးရန်လိုအပ်သည်။";
        let rendered = with_snippet(message, "\"hi\" ဖော်ပြပါ။\n");
        assert!(rendered.starts_with(message), "{rendered}");
        assert!(
            rendered.contains("\n\nline 1| \"hi\" ဖော်ပြပါ။\n"),
            "{rendered}"
        );
        assert!(rendered.ends_with("\"ကို\" ခံရေးရန်"), "{rendered}");
        assert!(
            rendered.lines().nth(3).is_some_and(|row| row.contains('↑')),
            "{rendered}"
        );
    }

    #[test]
    fn a_missing_print_value_points_at_the_particle() {
        let message = "E007 လိုင်း 1၌ \"ဖော်ပြပါ။\" လုပ်ဆောင်ရန် တန်ဖိုး မရှိသဖြင့် \"ဖော်ပြပါ။\" ကို မလုပ်ဆောင်နိုင်ပါ။";
        let rendered = with_snippet(message, " ကို ဖော်ပြပါ။\n");
        assert!(rendered.starts_with(message), "{rendered}");
        assert!(rendered.contains("\n\nline 1|  ကို ဖော်ပြပါ။\n"), "{rendered}");
        assert!(rendered.ends_with("တန်ဖိုး မရှိခြင်း"), "{rendered}");
    }

    #[test]
    fn a_removed_convert_sentence_points_at_its_verb() {
        let message =
            "E008 လိုင်း 1 တွင် \"ပြောင်းပါ\" ကို ဖယ်ရှားလိုက်ပါပြီ။ <value> ကို ကိန်း(x) / int(x) ဖြင့် ပြောင်းပါ။";
        let rendered = with_snippet(message, "x ကို ကိန်း သို့ ပြောင်းပါ။\n");
        assert!(rendered.starts_with(message), "{rendered}");
        assert!(
            rendered.contains("\n\nline 1| x ကို ကိန်း သို့ ပြောင်းပါ။\n"),
            "{rendered}"
        );
        assert!(rendered.ends_with("\"ပြောင်းပါ\" မလိုအပ်တော့ပါ"), "{rendered}");
    }

    #[test]
    fn a_math_statement_without_an_amount_points_at_the_particle() {
        let message = "E010 လိုင်း 1 တွင် \"တိုးပါ\" ကိုလုပ်ဆောင်ရန် တန်ဖိုး မရှိသဖြင့် \"တိုးပါ\" ကိုမလုပ်ဆောင်နိုင်ပါ။";
        let rendered = with_snippet(message, " ကို တိုးပါ။\n");
        assert!(rendered.starts_with(message), "{rendered}");
        assert!(rendered.contains("\n\nline 1|  ကို တိုးပါ။\n"), "{rendered}");
        assert!(rendered.ends_with("တန်ဖိုး မရှိခြင်း"), "{rendered}");
    }

    #[test]
    fn a_conditionless_if_points_at_the_closing_word() {
        let message = "E011 လိုင်း 1 တွင် \"အကယ်၍\"/\"သို့မဟုတ်\" အတွက် အခြေအနေ (စည်းကမ်းချက်) လိုအပ်ပါသည်။";
        let rendered = with_snippet(message, "အကယ်၍ ဖြစ်လျှင် ဖော်ပြပါ။ ပြီး။\n");
        assert!(rendered.starts_with(message), "{rendered}");
        assert!(rendered.ends_with("အခြေအနေ (စည်းကမ်းချက်) လိုအပ်"), "{rendered}");
        let caret = rendered.lines().nth(3).expect("caret row");
        let source = "အကယ်၍ ဖြစ်လျှင် ဖော်ပြပါ။ ပြီး။";
        let want = "line 1| ".chars().count()
            + crate::text::len(&source[..source.find("ဖြစ်လျှင်").unwrap()]) as usize;
        assert_eq!(
            caret.chars().position(|c| c == '↑'),
            Some(want),
            "{rendered}"
        );
    }

    #[test]
    fn a_conditionless_while_points_at_the_closing_word() {
        let message = "E015 လိုင်း 1 တွင် \"အခြေအနေ\" အတွက် အခြေအနေ (စည်းကမ်းချက်) လိုအပ်ပါသည်။";
        let rendered = with_snippet(message, "အခြေအနေ ဖြစ်နေစဉ် ရပ်ပါ။ ပြီး။\n");
        assert!(rendered.ends_with("အခြေအနေ (စည်းကမ်းချက်) လိုအပ်"), "{rendered}");
    }

    #[test]
    fn a_bad_condition_points_at_its_bracket() {
        let message = "E012 လိုင်း 1 တွင် အခြေအနေ ရေးသားပုံ မှားနေပါသည်။ အသုံးပြုပုံ — (<value> <တန်ဖိုးနှိုင်းယှဉ်ခြင်း> <value>) (နှင့်|သို့) (...) ဖြစ်လျှင်";
        let rendered = with_snippet(message, "အကယ်၍ (ကွင်း) ဖြစ်လျှင် ဖော်ပြပါ။ ပြီး။\n");
        assert!(rendered.ends_with("အခြေအနေ ရေးသားပုံ မှား"), "{rendered}");
        let caret = rendered.lines().nth(3).expect("caret row");
        let source = "အကယ်၍ (ကွင်း) ဖြစ်လျှင် ဖော်ပြပါ။ ပြီး။";
        let want = "line 1| ".chars().count()
            + crate::text::len(&source[..source.find('(').unwrap()]) as usize;
        assert_eq!(
            caret.chars().position(|c| c == '↑'),
            Some(want),
            "{rendered}"
        );
    }

    #[test]
    fn a_missing_then_word_points_past_the_line() {
        let message = "E013 လိုင်း 1 တွင် အခြေအနေ အပြီးမှာ \"ဖြစ်လျှင်\" သို့မဟုတ် \"မဖြစ်လျှင်\" လိုအပ်ပါသည်။";
        let rendered = with_snippet(message, "အကယ်၍ (x > 1) ဖော်ပြပါ။ ပြီး။\n");
        assert!(rendered.ends_with("\"ဖြစ်လျှင်\" လိုအပ်"), "{rendered}");
    }

    #[test]
    fn a_missing_while_word_points_past_the_line() {
        let message = "E017 လိုင်း 1 တွင် အခြေအနေ အပြီးမှာ \"ဖြစ်နေစဉ်\" / \"ဖြစ်နေရင်\" သို့မဟုတ် \"မဖြစ်နေစဉ်\" / \"မဖြစ်နေရင်\" လိုအပ်ပါသည်။";
        let rendered = with_snippet(message, "အခြေအနေ (x > 1) ရပ်ပါ။ ပြီး။\n");
        assert!(rendered.ends_with("\"ဖြစ်နေစဉ်\" လိုအပ်"), "{rendered}");
    }

    #[test]
    fn the_short_print_spelling_is_found_too() {
        let message = "E006 လိုင်း 1၌ \"ဖော်ပြပါ။\" လုပ်ဆောင်ရန် \"ဖော်ပြပါ။\" ၏အရှေ့၌ \"ကို\" ခံရေးရန်လိုအပ်သည်။";
        let rendered = with_snippet(message, "\"hi\" ပြပါ။\n");
        assert!(rendered.contains("line 1| \"hi\" ပြပါ။\n"), "{rendered}");
    }

    #[test]
    fn a_function_header_without_a_name_points_after_the_keyword() {
        let message =
            "E018 လိုင်း 1 တွင် \"အလုပ်\" အတွက် အမည် လိုအပ်ပါသည်။ အသုံးပြုပုံ — အလုပ် <fn name> အတွက် <parameter> ဖြင့်";
        let source = "အလုပ် အတွက် x ဖြင့်။";
        let rendered = with_snippet(message, source);
        assert!(rendered.ends_with("အလုပ် အမည် လိုအပ်"), "{rendered}");
        let caret = rendered.lines().nth(3).expect("caret row");
        let want = "line 1| ".chars().count() + crate::text::len("အလုပ်") as usize;
        assert_eq!(
            caret.chars().position(|c| c == '↑'),
            Some(want),
            "{rendered}"
        );
    }

    #[test]
    fn a_function_header_without_its_closing_word_points_past_the_line() {
        let message = "E020 လိုင်း 1 တွင် \"အလုပ်\" ၏ ခေါင်းစီး အပြီးမှာ \"ဖြင့်\" (အချက် ပါလျှင်) သို့မဟုတ် \"သည်\" (အချက် မပါလျှင်) လိုအပ်ပါသည်။";
        let rendered = with_snippet(message, "အလုပ် greet()\n");
        assert!(rendered.ends_with("\"ဖြင့်\" / \"သည်\" လိုအပ်"), "{rendered}");
        let caret = rendered.lines().nth(3).expect("caret row");
        let want = "line 1| ".chars().count() + crate::text::len("အလုပ် greet()") as usize;
        assert_eq!(
            caret.chars().position(|c| c == '↑'),
            Some(want),
            "{rendered}"
        );
    }

    #[test]
    fn a_call_missing_its_particle_points_at_the_call_word() {
        let message = "E022 လိုင်း 1 တွင် အလုပ် ခေါ်ရန် \"<fn name> ကို လုပ်ပါ။\" သို့မဟုတ် \"<fn name> ကို လုပ်ရန် <argument> ဖြင့်\" ပုံစံဖြင့် ရေးရပါမည်။";
        let source = "greet လုပ်ပါ။";
        let rendered = with_snippet(message, source);
        assert!(rendered.ends_with("\"ကို\" ခံရေးရန်"), "{rendered}");
        let caret = rendered.lines().nth(3).expect("caret row");
        let want = "line 1| ".chars().count()
            + crate::text::len(&source[..source.find("လုပ်ပါ").unwrap()]) as usize;
        assert_eq!(
            caret.chars().position(|c| c == '↑'),
            Some(want),
            "{rendered}"
        );
    }

    #[test]
    fn a_call_missing_its_argument_points_at_the_closing_word() {
        let message = "E024 လိုင်း 1 တွင် \"လုပ်ရန်\" ရန် အချက် လိုအပ်ပါသည်။";
        let source = "greet ကို လုပ်ရန် ဖြင့်။";
        let rendered = with_snippet(message, source);
        assert!(rendered.ends_with("အချက် လိုအပ်"), "{rendered}");
        let caret = rendered.lines().nth(3).expect("caret row");
        let want = "line 1| ".chars().count()
            + crate::text::len(&source[..source.find("ဖြင့်").unwrap()]) as usize;
        assert_eq!(
            caret.chars().position(|c| c == '↑'),
            Some(want),
            "{rendered}"
        );
    }

    #[test]
    fn a_class_header_without_a_name_points_after_the_keyword() {
        let message = "E025 လိုင်း 1 တွင် \"ပုံသေ\" အတွက် အမည် လိုအပ်ပါသည်။ အသုံးပြုပုံ — ပုံသေ <class name>။";
        let rendered = with_snippet(message, "ပုံသေ\n");
        assert!(rendered.ends_with("ပုံသေ အမည် လိုအပ်"), "{rendered}");
        let caret = rendered.lines().nth(3).expect("caret row");
        let want = "line 1| ".chars().count() + crate::text::len("ပုံသေ") as usize;
        assert_eq!(
            caret.chars().position(|c| c == '↑'),
            Some(want),
            "{rendered}"
        );
    }

    #[test]
    fn the_removed_new_marker_is_pointed_at() {
        let message = "E029 လိုင်း 1 တွင် အရာဝတ္ထု ဖန်တီးသည့် ပုံသေ ရေးပုံ မှားနေပါသည်။ \"အသစ်\" မလိုအပ်တော့ပါ။ အသုံးပြုပုံ — <class name> (<argument>, ...)";
        let rendered = with_snippet(message, "Point အသစ် (1)။\n");
        assert!(rendered.ends_with("\"အသစ်\" မလိုအပ်တော့ပါ"), "{rendered}");
        let caret = rendered.lines().nth(3).expect("caret row");
        let want = "line 1| ".chars().count() + crate::text::len("Point ") as usize;
        assert_eq!(
            caret.chars().position(|c| c == '↑'),
            Some(want),
            "{rendered}"
        );
    }

    #[test]
    fn a_non_method_inside_a_class_points_at_the_statement() {
        let message = "E027 လိုင်း 2 တွင် \"ပုံသေ\" အတွင်း \"အလုပ်\" များသာ ပါဝင်နိုင်ပါသည်။";
        let rendered = with_snippet(message, "ပုံသေ Point။\nx ကို 1 တိုးပါ။\n");
        assert!(rendered.ends_with("\"အလုပ်\" များသာ ပါဝင်နိုင်"), "{rendered}");
        let caret = rendered.lines().nth(3).expect("caret row");
        assert_eq!(
            caret.chars().position(|c| c == '↑'),
            Some("line 2| ".chars().count()),
            "{rendered}"
        );
    }

    /// Is the caret row at the column `needle` starts on in `source`?
    fn points_at(rendered: &str, source: &str, needle: &str) -> bool {
        let want = "line 1| ".chars().count()
            + crate::text::len(&source[..source.find(needle).unwrap()]) as usize;
        caret_is_at(rendered, want)
    }

    /// Is the caret row one past the last character of `source`?
    fn points_past_end(rendered: &str, source: &str) -> bool {
        caret_is_at(
            rendered,
            "line 1| ".chars().count() + crate::text::len(source) as usize,
        )
    }

    fn caret_is_at(rendered: &str, column: usize) -> bool {
        rendered
            .lines()
            .nth(3)
            .and_then(|row| row.chars().position(|c| c == '↑'))
            == Some(column)
    }

    #[test]
    fn a_bad_library_list_points_after_the_keyword() {
        let message = "E072 လိုင်း 1 တွင် \"နည်းပညာများ\" ၏ နည်းပညာ အမည်များကို \",\" ဖြင့် ခွဲ၍ရေးပါ။ အသုံးပြုပုံ — နည်းပညာများ <lib1>, <lib2> ကို သုံးမည်။";
        let source = "နည်းပညာများ ၅ ကို အသုံးပြုပါ။";
        let rendered = with_snippet(message, source);
        assert!(
            rendered.ends_with("နည်းပညာ အမည်များ \",\" ဖြင့် ခွဲရန်"),
            "{rendered}"
        );
        assert!(points_at(&rendered, source, " ၅"), "{rendered}");
    }

    #[test]
    fn a_library_alias_without_a_name_points_after_the_word() {
        let message = "E093 လိုင်း 1 တွင် \"အဖြစ်\" ၏ နောက်တွင် အမည်လွှဲ အမည် တစ်ခု လိုအပ်ပါသည်။ အသုံးပြုပုံ — နည်းပညာများ <lib> အဖြစ် <alias> ကို သုံးမည်။";
        let source = "နည်းပညာများ ကျပန်း အဖြစ် ကို အသုံးပြုပါ။";
        let rendered = with_snippet(message, source);
        assert!(rendered.ends_with("အမည်လွှဲ အမည် လိုအပ်"), "{rendered}");
        assert!(points_at(&rendered, source, " ကို"), "{rendered}");
    }

    #[test]
    fn a_library_use_without_its_particle_points_at_the_verb() {
        let message = "E094 လိုင်း 1 တွင် နည်းပညာ(ကျပန်း) ကို အသုံးပြုရန် \"ကို\" ခံရေးရန် လိုအပ်ပါသည်";
        let source = "နည်းပညာများ ကျပန်း သုံးမည်။";
        let rendered = with_snippet(message, source);
        assert!(rendered.ends_with("\"ကို\" ခံရေးရန်"), "{rendered}");
        assert!(points_at(&rendered, source, "သုံးမည်"), "{rendered}");
    }

    #[test]
    fn a_function_alias_without_a_name_points_at_the_word() {
        let message = "E075 လိုင်း 1 တွင် \"အဖြစ်\" ၏ ရှေ့တွင် အမည်လွှဲ ပေးလိုသော အလုပ် အမည် တစ်ခုတည်း လိုအပ်ပါသည်။ အသုံးပြုပုံ — <fn name> အဖြစ် <alias1>, <alias2>";
        let source = "\"x\" အဖြစ် y။";
        let rendered = with_snippet(message, source);
        assert!(rendered.ends_with("အလုပ် အမည် လိုအပ်"), "{rendered}");
        assert!(points_at(&rendered, source, "အဖြစ်"), "{rendered}");
    }

    #[test]
    fn a_function_alias_without_an_alias_points_after_the_word() {
        let message = "E076 လိုင်း 1 တွင် \"အဖြစ်\" ၏ နောက်တွင် အမည်လွှဲ အမည် အနည်းဆုံး တစ်ခု လိုအပ်ပါသည်။ အသုံးပြုပုံ — <fn name> အဖြစ် <alias1>, <alias2>";
        let source = "foo အဖြစ်။";
        let rendered = with_snippet(message, source);
        assert!(rendered.ends_with("အမည်လွှဲ အမည် လိုအပ်"), "{rendered}");
        assert!(points_at(&rendered, source, "။"), "{rendered}");
    }

    #[test]
    fn a_bad_alias_list_points_after_the_word() {
        let message = "E077 လိုင်း 1 တွင် အမည်လွှဲ အမည်များကို \",\" ဖြင့် ခွဲ၍ တစ်ခုချင်း ရေးပါ။ အသုံးပြုပုံ — <fn name> အဖြစ် <alias1>, <alias2>";
        let source = "foo အဖြစ် 1, 2။";
        let rendered = with_snippet(message, source);
        assert!(rendered.ends_with("အမည်လွှဲများကို \",\" ဖြင့် ခွဲရန်"), "{rendered}");
        assert!(points_at(&rendered, source, " 1"), "{rendered}");
    }

    #[test]
    fn a_bad_library_call_points_at_the_of_particle() {
        let message = "E084 လိုင်း 1 တွင် နည်းပညာ အလုပ် ခေါ်ရန် ရေးသားပုံ မှားနေပါသည်။ အသုံးပြုပုံ — <lib> ၏ <fn>(<argument>) ကို လုပ်ပါ။ သို့မဟုတ် <lib> ၏ <fn> ကို လုပ်ရန် <argument> ဖြင့်";
        let source = "ကျပန်း ၏ ကိန်း(1)။";
        let rendered = with_snippet(message, source);
        assert!(rendered.ends_with("နည်းပညာ အလုပ် ခေါ်ပုံ မှား"), "{rendered}");
        assert!(points_at(&rendered, source, "၏"), "{rendered}");
    }

    #[test]
    fn a_bad_eng_declaration_points_at_its_colon_or_line_end() {
        let message = "E100 လိုင်း 1 တွင် `<name> :<type> = <value>;` သို့မဟုတ် `pin <name> :<type> = <value>;` ကို မျှော်လင့်ပါသည် (အမျိုးအစားများ — int, float, str, bool)။";
        let source = "x str = \"a\";";
        let rendered = with_snippet(message, source);
        assert!(rendered.ends_with(":<type> = <value>; လိုအပ်"), "{rendered}");
        assert!(points_past_end(&rendered, source), "{rendered}");
    }

    #[test]
    fn an_unknown_eng_type_points_at_the_type_name() {
        let message =
            "E101 လိုင်း 1 တွင် \"number\" ဆိုသော အမျိုးအစားကို မသိပါ (အမျိုးအစားများ — int, float, str, bool)။";
        let source = "x :number = 1;";
        let rendered = with_snippet(message, source);
        assert!(rendered.ends_with("အမျိုးအစား မမှန်ကန်"), "{rendered}");
        assert!(points_at(&rendered, source, "number"), "{rendered}");
    }

    #[test]
    fn an_eng_declaration_without_a_value_points_past_the_line() {
        let message = "E102 လိုင်း 1 တွင် '=' နောက်၌ တန်ဖိုး မပါပါ။";
        let source = "x :int = ;";
        let rendered = with_snippet(message, source);
        assert!(rendered.ends_with("တန်ဖိုး လိုအပ်"), "{rendered}");
        assert!(points_past_end(&rendered, source), "{rendered}");
    }

    #[test]
    fn an_eng_call_without_parentheses_points_past_the_line() {
        let message = "E106 လိုင်း 1 တွင် `print(...);` ကို မျှော်လင့်ပါသည်။";
        let source = "print;";
        let rendered = with_snippet(message, source);
        assert!(rendered.ends_with("(...) ဖြင့် ခေါ်ရန်"), "{rendered}");
        assert!(points_past_end(&rendered, source), "{rendered}");
    }

    #[test]
    fn an_eng_block_without_braces_points_past_the_line() {
        let message = "E109 လိုင်း 1 တွင် `if (<cond>) { ... }` (လိုအပ်လျှင် `else if`/`else` ဖြင့်)၊ `while (<cond>) { ... }`၊ `for (<init>; <cond>; <step>) { ... }`၊ `loop { ... }`၊ `switch (<value>) { case <value>: ... default: ... }` သို့မဟုတ် `fn <name>(<params>) { ... }` ကို မျှော်လင့်ပါသည်။";
        let source = "loop x;";
        let rendered = with_snippet(message, source);
        assert!(rendered.ends_with("{ ... } လိုအပ်"), "{rendered}");
        assert!(points_past_end(&rendered, source), "{rendered}");
    }

    #[test]
    fn an_eng_use_without_a_library_points_after_the_keyword() {
        let message = "E118 လိုင်း 1 တွင် `use <library> [as <alias>] [, ...];` ကို မျှော်လင့်ပါသည်။";
        let source = "use ;";
        let rendered = with_snippet(message, source);
        assert!(rendered.ends_with("library အမည် လိုအပ်"), "{rendered}");
        assert!(points_at(&rendered, source, " ;"), "{rendered}");
    }

    #[test]
    fn an_eng_alias_without_a_name_points_at_the_keyword() {
        let message =
            "E137 လိုင်း 1 တွင် `function() as alias;` သို့မဟုတ် `function as alias;` ကို မျှော်လင့်ပါသည်။";
        let source = "print as;";
        let rendered = with_snippet(message, source);
        assert!(rendered.ends_with("as <alias> လိုအပ်"), "{rendered}");
        assert!(points_at(&rendered, source, "as;"), "{rendered}");
    }

    #[test]
    fn a_bad_lambda_points_at_the_fn_keyword() {
        let message = "E150 လိုင်း 1 တွင် `fn(<params>) { <body> }` ပုံစံ lambda ကို မျှော်လင့်ပါသည်။";
        let source = "x :int = fn(1) { 2 };";
        let rendered = with_snippet(message, source);
        assert!(
            rendered.ends_with("fn(<params>) { <body> } လိုအပ်"),
            "{rendered}"
        );
        assert!(points_at(&rendered, source, "fn"), "{rendered}");
    }

    #[test]
    fn an_eng_ternary_without_its_colon_points_at_the_question() {
        let message = "E151 လိုင်း 1 တွင် `<condition> ? <then> : <else>` ကို မျှော်လင့်ပါသည် ('?' နှင့် ':' နှစ်ခုလုံး လိုအပ်ပါသည်)။";
        let source = "x :int = 1 ? 2;";
        let rendered = with_snippet(message, source);
        assert!(rendered.ends_with("`:` နှင့် else လိုအပ်"), "{rendered}");
        assert!(points_at(&rendered, source, "?"), "{rendered}");
    }

    #[test]
    fn a_bad_eng_destructuring_points_at_the_bracket() {
        let message =
            "E152 လိုင်း 1 တွင် `[a, b] = <list>` ပုံစံ ခွဲထုတ်သတ်မှတ်မှုကို မျှော်လင့်ပါသည် (အမည်များသာ၊ ကော်မာဖြင့် ခွဲပါ)။";
        let source = "[a, 1] = xs;";
        let rendered = with_snippet(message, source);
        assert!(rendered.ends_with("[a, b] = <list> လိုအပ်"), "{rendered}");
        assert!(points_at(&rendered, source, "["), "{rendered}");
    }

    #[test]
    fn a_bad_eng_switch_points_at_the_keyword() {
        let message =
            "E153 လိုင်း 1 တွင် `switch (<value>) { case <value>: ... default: ... }` ကို မျှော်လင့်ပါသည်။";
        let source = "switch x;";
        let rendered = with_snippet(message, source);
        assert!(
            rendered.ends_with("switch (<value>) { ... } လိုအပ်"),
            "{rendered}"
        );
        assert!(points_at(&rendered, source, "switch"), "{rendered}");
    }

    #[test]
    fn a_bad_myanmar_switch_points_at_the_keyword() {
        let message = "E156 လိုင်း 1 တွင် \"ရွေးရန် (<တန်ဖိုး>) ... ပြီး။\" ပုံစံဖြင့် ရေးပါ (အကိုင်းတစ်ခုစီသည် \"<တန်ဖိုး> ဖြစ်ရင် ...\" သို့မဟုတ် \"မူလ ...\")။";
        let source = "ရွေးရန် x ပြီး။";
        let rendered = with_snippet(message, source);
        assert!(
            rendered.ends_with("\"ရွေးရန် (<တန်ဖိုး>) ... ပြီး။\" လိုအပ်"),
            "{rendered}"
        );
        assert!(points_at(&rendered, source, "ရွေးရန်"), "{rendered}");
    }

    #[test]
    fn a_bad_myanmar_ternary_points_at_its_open_paren() {
        let message = "E157 လိုင်း 1 တွင် \"(<အခြေအနေ>) ဖြစ်ရင် <တန်ဖိုး> မဟုတ်ရင် <တန်ဖိုး>\" ပုံစံဖြင့် ရေးပါ။";
        let source = "x သည် (1 > 0 ဖြစ်ရင် \"a\") ဖြစ်၏။";
        let rendered = with_snippet(message, source);
        assert!(
            rendered.ends_with("\"ဖြစ်ရင် ... မဟုတ်ရင် ...\" လိုအပ်"),
            "{rendered}"
        );
        assert!(points_at(&rendered, source, "("), "{rendered}");
    }

    #[test]
    fn a_bad_myanmar_destructuring_points_at_the_bracket() {
        let message = "E158 လိုင်း 1 တွင် \"[a, b] သည် <စာရင်း> ဖြစ်၏။\" ပုံစံဖြင့် နာမည်များသာ ရေးပါ။";
        let source = "[a, 1] သည် xs ဖြစ်၏။";
        let rendered = with_snippet(message, source);
        assert!(rendered.ends_with("[a, b] ပုံစံဖြင့် ရေးပါ"), "{rendered}");
        assert!(points_at(&rendered, source, "["), "{rendered}");
    }

    #[test]
    fn a_runtime_type_mismatch_points_at_the_declared_name() {
        let message =
            "E103 လိုင်း 1 တွင် စာသား ကို \"x\" သို့ သတ်မှတ်၍မရပါ (:int ဟု ကြေညာထားသဖြင့် တန်ဖိုးသည် ကိန်း ဖြစ်ရပါမည်)။";
        let source = "x :int = \"no\";";
        let rendered = with_snippet(message, source);
        assert!(rendered.ends_with("အမျိုးအစား မကိုက်ညီ"), "{rendered}");
        assert!(points_at(&rendered, source, "x"), "{rendered}");
    }

    #[test]
    fn a_runtime_pin_reassignment_points_at_the_name() {
        let message = "E104 လိုင်း 1 တွင် \"N\" ကို 'pin' ဖြင့် ကြေညာထားသဖြင့် ပြန်မသတ်မှတ်နိုင်ပါ။";
        let source = "N = 2;";
        let rendered = with_snippet(message, source);
        assert!(
            rendered.ends_with("'pin' ဖြင့် ကြေညာထားသော အမည်"),
            "{rendered}"
        );
        assert!(points_at(&rendered, source, "N"), "{rendered}");
    }

    #[test]
    fn a_runtime_pin_redeclaration_points_at_the_name() {
        let message = "E105 လိုင်း 1 တွင် pin \"N\" သည် ရှိပြီးသား ကိန်းရှင်ကို ထပ်ကြေညာ၍ မရပါ။";
        let source = "pin N :int = 2;";
        let rendered = with_snippet(message, source);
        assert!(rendered.ends_with("အမည် ထပ်နေသည်"), "{rendered}");
        assert!(points_at(&rendered, source, "N"), "{rendered}");
    }

    #[test]
    fn a_runtime_split_of_a_non_collection_points_at_the_bracket() {
        let message = "E154 လိုင်း 1 တွင် ခွဲထုတ်ရန် စာရင်း သို့မဟုတ် tuple လိုအပ်ပါသည်၊ ကိန်း ရပါသည်။";
        let source = "[a, b] = 1;";
        let rendered = with_snippet(message, source);
        assert!(rendered.ends_with("ခွဲထုတ်ရန် အရေအတွက် မကိုက်ညီ"), "{rendered}");
        assert!(points_at(&rendered, source, "["), "{rendered}");
    }

    #[test]
    fn a_runtime_split_count_mismatch_points_at_the_bracket() {
        let message = "E155 လိုင်း 1 တွင် အမည် 2 ခု ခွဲထုတ်ရန် ရေးထားပြီး တန်ဖိုးတွင် အချက် 1 ခု ရှိပါသည်။";
        let source = "[a, b] = [1];";
        let rendered = with_snippet(message, source);
        assert!(rendered.ends_with("ခွဲထုတ်ရန် အရေအတွက် မကိုက်ညီ"), "{rendered}");
        assert!(points_at(&rendered, source, "["), "{rendered}");
    }

    #[test]
    fn a_runtime_integer_floor_division_by_zero_points_at_the_operator() {
        let message = "E073 လိုင်း 1 တွင် သုညဖြင့် အပြည့်ကိန်းစား၍မရပါ။";
        let source = "print(10 // 0);";
        let rendered = with_snippet(message, source);
        assert!(rendered.ends_with("သုညဖြင့် စား၍မရ"), "{rendered}");
        assert!(points_at(&rendered, source, "//"), "{rendered}");
    }

    #[test]
    fn a_runtime_float_floor_division_by_zero_points_at_the_operator() {
        let message = "E074 လိုင်း 1 တွင် သုညဖြင့် အပြည့်ကိန်းစား၍မရပါ။";
        let source = "print(10 // 0.0);";
        let rendered = with_snippet(message, source);
        assert!(rendered.ends_with("သုညဖြင့် စား၍မရ"), "{rendered}");
        assert!(points_at(&rendered, source, "//"), "{rendered}");
    }

    #[test]
    fn an_already_annotated_message_is_not_drawn_again() {
        let message =
            "E103 လိုင်း 1 တွင် စာသား ကို \"x\" သို့ သတ်မှတ်၍မရပါ (:int ဟု ကြေညာထားသဖြင့် တန်ဖိုးသည် ကိန်း ဖြစ်ရပါမည်)။";
        let source = "x :int = \"no\";";
        let drawn = with_snippet(message, source);
        assert_eq!(with_snippet(&drawn, source), drawn);
    }

    #[test]
    fn other_messages_and_missing_lines_are_unchanged() {
        let other = "E001 လိုင်း 1 တွင် တန်ဖိုးသတ်မှတ်ရာမှာ 'သည်' လိုအပ်ပါသည်။";
        assert_eq!(with_snippet(other, "x 1။\n"), other);
        assert_eq!(
            with_snippet("E999 no line here", "x\n"),
            "E999 no line here"
        );
        let past_end = "E002 လိုင်း 7 ၏ စာကြောင်းအဆုံးတွင် '။' မရှိပါ။";
        assert_eq!(with_snippet(past_end, "x\n"), past_end);
    }
}
