//! User-facing error text that more than one engine must produce.
//!
//! A diagnostic is part of the compiler's contract, not of one execution
//! strategy: a raised error or a builtin misuse has to read the same whether
//! the interpreter, the VM or a native executable ran the program. The text
//! lives here so the engines cannot drift apart. The native backend keeps its
//! own `E177` wording here too, next to everything else users read.

/// The code every user-raised error carries. A catch clause can select it
/// (`E178 ကို ဖမ်းပါ။`), but the identity it was raised with is what makes the
/// message readable.
pub(crate) const RAISED_CODE: &str = "E178";

/// Everything a raise's message says before its payload, e.g.
/// `E178 လိုင်း 3 တွင် [InsufficientFunds] `.
///
/// The native backend builds its stderr line from this same text, so an
/// uncaught raise reads identically however the program was run.
pub(crate) fn raised_prefix(line: usize, identity: &str) -> String {
    format!("{} လိုင်း {} တွင် [{}] ", RAISED_CODE, line, identity)
}

/// The message a `ပစ်ပါ` raise produces: the code, the line, the identity in
/// brackets and then the message text, e.g.
/// `E178 လိုင်း 3 တွင် [InsufficientFunds] လက်ကျန်ငွေ မလုံလောက်ပါ။`.
pub(crate) fn raised_error(line: usize, identity: &str, payload: &str) -> String {
    format!("{}{}", raised_prefix(line, identity), payload)
}

/// The identity a raised error was given, or `None` for any other error.
pub(crate) fn raised_identity(msg: &str) -> Option<&str> {
    let rest = msg.strip_prefix(RAISED_CODE)?.strip_prefix(' ')?;
    let open = rest.find('[')?;
    let close = rest[open + 1..].find(']')? + open + 1;
    Some(&rest[open + 1..close])
}

/// The message a raised error carries, or `None` for any other error.
pub(crate) fn raised_payload(msg: &str) -> Option<&str> {
    let rest = msg.strip_prefix(RAISED_CODE)?.strip_prefix(' ')?;
    let open = rest.find('[')?;
    let close = rest[open + 1..].find(']')? + open + 1;
    let after = &rest[close + 1..];
    Some(after.strip_prefix(' ').unwrap_or(after))
}

/// Error-code helpers for try/catch. All runtime errors are formatted as
/// "E### <message>". `<error_name> ကို ဖမ်းပါ` binds the code (e.g. "E030")
/// into a string variable so scripts can match on it.
pub(crate) fn extract_error_code(msg: &str) -> String {
    let code = msg.split_whitespace().next().unwrap_or("E");
    code.to_string()
}

/// The interpreter's `E110` for a builtin called with the wrong number or kind
/// of arguments. Compiled code carries no spans, so it passes `None` and the
/// line number is left out, the way the `E036`/`E037`/`E038` panics are.
pub(crate) fn builtin_error(line: Option<usize>, name: &str, detail: &str) -> String {
    match line {
        Some(line) => format!("E110 လိုင်း {} တွင် {}() {}", line, name, detail),
        None => format!("E110 {}() {}", name, detail),
    }
}

/// The interpreter's wording for a builtin that takes exactly one argument.
pub(crate) fn one_arg_needed_detail(count: usize) -> String {
    format!("အချက် တစ်ခုတည်း လိုအပ်ပါသည်၊ {} ရပါသည်", count)
}

/// The interpreter's wording for a builtin that needs a number.
pub(crate) fn number_needed_detail(kind: &str) -> String {
    format!("ကိန်း လိုအပ်ပါသည်၊ {} ရပါသည်", kind)
}

/// The interpreter's wording for `push`/`append` when the value count is
/// wrong: the first argument is the list, so `count` is the number of extra
/// arguments it was given.
pub(crate) fn push_value_needed_detail(count: usize) -> String {
    format!("စာရင်း နှင့် တန်ဖိုး တစ်ခု လိုအပ်ပါသည်၊ တန်ဖိုး {} ခု ရပါသည်", count)
}

/// The interpreter's wording for `push`/`append` when the first argument is
/// not a variable that names the list. It is one fixed sentence, written for
/// both names, and it is what compiled code has to say too.
pub(crate) const PUSH_NEEDS_LIST_VARIABLE: &str =
    "ပထမ အချက်သည် စာရင်း ကိန်းရှင် ဖြစ်ရပါမည်၊ ဥပမာ — push(xs, 4)";

/// The interpreter's wording for `push`/`append` over a value that is not a
/// list.
pub(crate) fn list_needed_detail(kind: &str) -> String {
    format!("စာရင်း လိုအပ်ပါသည်၊ {} ရပါသည်", kind)
}

/// The interpreter's wording for a list builtin that takes no extra argument
/// (`pop`, `sort`).
pub(crate) const LIST_ONLY_NEEDED: &str = "စာရင်း တစ်ခုသာ လိုအပ်ပြီး အခြားအချက် မလိုပါ";

/// The interpreter's wording for `insert` when it does not get a list, an
/// index and one value.
pub(crate) fn insert_args_needed_detail(count: usize) -> String {
    format!(
        "စာရင်း၊ အညွှန်း နှင့် တန်ဖိုး တစ်ခု လိုအပ်ပါသည်၊ စာရင်းနောက်တွင် အချက် {} ခု ရပါသည်",
        count
    )
}

/// The interpreter's wording for `remove` when it does not get a list and one
/// index.
pub(crate) fn remove_index_needed_detail(count: usize) -> String {
    format!("စာရင်း နှင့် အညွှန်း တစ်ခု လိုအပ်ပါသည်၊ အညွှန်း {} ခု ရပါသည်", count)
}

/// The interpreter's wording for `slice` when it is not given a collection, a
/// start and (optionally) an end.
pub(crate) fn slice_args_needed_detail(count: usize) -> String {
    format!(
        "စုစည်းမှု၊ စတင်ချက် နှင့် ရွေးချယ်နိုင်သော အဆုံးချက် လိုအပ်ပါသည်၊ အချက် {} ခု ရပါသည်",
        count
    )
}

/// The interpreter's wording for `reverse` when it is not given exactly one
/// argument.
pub(crate) fn reverse_arg_needed_detail(count: usize) -> String {
    format!("စာရင်း သို့မဟုတ် စာသား တစ်ခု လိုအပ်ပါသည်၊ {} ရပါသည်", count)
}

/// The interpreter's wording for `reverse` over a value that is neither a
/// list nor text.
pub(crate) fn reverse_value_needed_detail(kind: &str) -> String {
    format!("စာရင်း သို့မဟုတ် စာသား လိုအပ်ပါသည်၊ {} ရပါသည်", kind)
}

/// The wording `min`/`max` use when called with no candidates.
pub(crate) const MIN_MAX_NEEDS_VALUE: &str = "အနည်းဆုံး တန်ဖိုး တစ်ခု လိုအပ်ပါသည်";

/// The Myanmar name of an `int` value, as the diagnostics spell it.
pub(crate) const KIND_INT_MM: &str = "ကိန်းပြည့်";
/// The Myanmar name of a `float` value.
pub(crate) const KIND_FLOAT_MM: &str = "ဒဿမကိန်း";
/// The Myanmar name of a `bool` value.
pub(crate) const KIND_BOOL_MM: &str = "မှန်/မှား";
/// The Myanmar name of a `string` value.
pub(crate) const KIND_STR_MM: &str = "စာသား";

/// Refuse a construct the native backend cannot compile, naming it. Every
/// rejection in the native backend goes through this wording, so the
/// diagnostic always says what was not supported.
pub(crate) fn native_unsupported(what: &str) -> String {
    format!(
        "E177 native backend တွင် {} ကို မထောက်ပံ့သေးပါ။ (int, float, bool, string literal, list/tuple/set/dict literal နှင့် ၎င်းတို့၏ index/len/print/iteration/store/equality, အခြေခံ တွက်ချက်မှု, နှိုင်းယှဉ်ချက်, abs/min/max/floor/ceil/round, push/append, reverse, pop/insert/remove/sort, slice, str conversion နှင့် {{…}} interpolation, print, လုပ်ဆောင်ချက် ခေါ်ခြင်း, return, ပစ်ပါ နှင့် try/catch သာ ရပါသည်)",
        what
    )
}

/// The native build's wrapper when the middle end refused the program with
/// `E176`: `akk build` reports it with the backend's own code and quotes the
/// reason, so the construct is still named.
pub(crate) fn native_refusal(reason: &str) -> String {
    format!("E177 native backend တွင် ဤပရောဂျက်ကို မထောက်ပံ့သေးပါ။ ({})", reason)
}
