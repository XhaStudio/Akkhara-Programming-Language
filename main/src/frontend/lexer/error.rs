//! The lexer's own errors: an unterminated string literal (`E003`).

/// `E003`: a `"""` triple-quoted literal was opened and never closed.
pub(crate) fn unterminated_triple_string(line: usize) -> String {
    format!("E003 လိုင်း {} တွင် စာသား တန်ဖိုးအတွက် ပိတ် '\"\"\"' မရှိပါ။", line)
}

/// `E003`: an ordinary string literal was opened and never closed.
pub(crate) fn unterminated_string(line: usize) -> String {
    format!("E003 လိုင်း {} တွင် string တန်ဖိုးအတွက် ပိတ် '\"' မရှိပါ။", line)
}
