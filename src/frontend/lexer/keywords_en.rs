//! The English spelling of the parse-level keywords, and the type names.

// --- eng library (English-spelled declarations) ---
//
// The "eng" library lets the same program declare variables with either the
// Myanmar sentence forms or these English spellings:
//
//   variable:  <name> :<type> = <value>;        e.g.  name :str = "John";
//   constant:  pin <name> :<type> = <value>;    e.g.  pin control :bool = true;
//
// Types: int, float, str, bool.
pub(crate) const ENG_PIN: &str = "pin";

pub(crate) const ENG_TYPE_INT: &str = "int";

pub(crate) const ENG_TYPE_FLOAT: &str = "float";

pub(crate) const ENG_TYPE_STR: &str = "str";

pub(crate) const ENG_TYPE_BOOL: &str = "bool";

pub(crate) const ENG_PRINT: &str = "print";

pub(crate) const ENG_INPUT: &str = "input";

pub(crate) const ENG_IF: &str = "if";

pub(crate) const ENG_ELSE: &str = "else";

pub(crate) const ENG_WHILE: &str = "while";

pub(crate) const ENG_FN: &str = "fn";

pub(crate) const ENG_RETURN: &str = "return";

pub(crate) const ENG_LOOP: &str = "loop";

pub(crate) const ENG_FOR: &str = "for";

pub(crate) const ENG_IN: &str = "in";

pub(crate) const ENG_BREAK: &str = "break";

pub(crate) const ENG_CONTINUE: &str = "continue";

/// Raise a user error: `throw <value> as <name>;` -- the eng spelling of
/// `<value> ကို <name> အမည်ဖြင့် ပစ်ပါ။`.
pub(crate) const ENG_THROW: &str = "throw";

pub(crate) const ENG_USE: &str = "use";

pub(crate) const ENG_AS: &str = "as";

pub(crate) const ENG_SWITCH: &str = "switch";

pub(crate) const ENG_CASE: &str = "case";

pub(crate) const ENG_DEFAULT: &str = "default";

/// The eng library's four supported type keywords.
pub(crate) fn is_eng_type(s: &str) -> bool {
    matches!(
        s,
        ENG_TYPE_INT | ENG_TYPE_FLOAT | ENG_TYPE_STR | ENG_TYPE_BOOL
    )
}
