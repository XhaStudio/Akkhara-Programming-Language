//! The language's types, named once.
//!
//! The interpreter is dynamically typed: a value carries its own type and
//! nothing is checked before a program runs. This module names those types so
//! the lexer, the warnings, and a future type checker can all refer to the
//! same set, and so the spelling a program writes (`ကိန်းပြည့်`, `int`) maps
//! to exactly one `Type`.

pub(crate) mod bool;
pub(crate) mod dict;
pub(crate) mod float;
pub(crate) mod func;
pub(crate) mod int;
pub(crate) mod list;
pub(crate) mod null;
pub(crate) mod object;
pub(crate) mod set;
pub(crate) mod str;
pub(crate) mod tuple;
pub(crate) mod uint;

/// One Akkhara type.
///
/// `Int` and `Uint` are separate even though the interpreter has one integer
/// value, and `Float` is separate from both: a language that writes the type
/// names in a declaration needs them distinguishable (`ကိန်းပြည့်`,
/// `ဒဿမကိန်း`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Type {
    Int,
    Uint,
    Float,
    Bool,
    Str,
    Null,
    List,
    Tuple,
    Dict,
    Set,
    Object,
    Func,
}

impl Type {
    /// The type a program names with `name`, in either spelling.
    ///
    /// Both spellings are accepted because both are written in real
    /// programs; `ကိန်း` ("number") is the one name that does not map to a
    /// single type -- it covers `Int`, `Uint` and `Float`.
    pub(crate) fn named(name: &str) -> Option<Type> {
        Some(match name {
            "ကိန်းပြည့်" | "int" => Type::Int,
            "အနုတ်လက္ခဏာမဲ့ကိန်း" | "uint" => Type::Uint,
            "ဒဿမကိန်း" | "float" => Type::Float,
            "မှန်/မှား" | "bool" => Type::Bool,
            "စာသား" | "str" | "string" => Type::Str,
            "null" | "ဗလာ" => Type::Null,
            "စာရင်း" | "list" => Type::List,
            "tuple" | "အစု" => Type::Tuple,
            "dict" | "အဘိဓာန်" => Type::Dict,
            "set" | "အစုအဝေး" => Type::Set,
            "object" | "အရာဝတ္ထု" => Type::Object,
            "func" | "အလုပ်" => Type::Func,
            _ => return None,
        })
    }

    /// The Myanmar spelling, as a program writes it.
    pub(crate) fn name_mm(self) -> &'static str {
        match self {
            Type::Int => "ကိန်းပြည့်",
            Type::Uint => "အနုတ်လက္ခဏာမဲ့ကိန်း",
            Type::Float => "ဒဿမကိန်း",
            Type::Bool => "မှန်/မှား",
            Type::Str => "စာသား",
            Type::Null => "ဗလာ",
            Type::List => "စာရင်း",
            Type::Tuple => "tuple",
            Type::Dict => "အဘိဓာန်",
            Type::Set => "အစုအဝေး",
            Type::Object => "အရာဝတ္ထု",
            Type::Func => "အလုပ်",
        }
    }

    /// The English spelling, as a declaration writes it (`int`, `float`, ...).
    pub(crate) fn name_en(self) -> &'static str {
        match self {
            Type::Int => "int",
            Type::Uint => "uint",
            Type::Float => "float",
            Type::Bool => "bool",
            Type::Str => "str",
            Type::Null => "null",
            Type::List => "list",
            Type::Tuple => "tuple",
            Type::Dict => "dict",
            Type::Set => "set",
            Type::Object => "object",
            Type::Func => "func",
        }
    }

    /// Every type, in the order a lookup table lists them.
    ///
    /// The type checker and the compiler's own tables will read this; today
    /// only the round-trip test does, so it is allowed to be unused at run
    /// time for now.
    #[allow(dead_code)]
    pub(crate) const ALL: &'static [Type] = &[
        Type::Int,
        Type::Uint,
        Type::Float,
        Type::Bool,
        Type::Str,
        Type::Null,
        Type::List,
        Type::Tuple,
        Type::Dict,
        Type::Set,
        Type::Object,
        Type::Func,
    ];
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_type_round_trips_through_its_myanmar_name() {
        for kind in Type::ALL {
            assert_eq!(Type::named(kind.name_mm()), Some(*kind));
        }
    }

    #[test]
    fn english_spellings_name_the_same_types() {
        assert_eq!(Type::named("int"), Some(Type::Int));
        assert_eq!(Type::named("float"), Some(Type::Float));
        assert_eq!(Type::named("str"), Some(Type::Str));
        assert_eq!(Type::named("bool"), Some(Type::Bool));
        assert_eq!(Type::named("list"), Some(Type::List));
        assert_eq!(Type::named("dict"), Some(Type::Dict));
        assert_eq!(Type::named("set"), Some(Type::Set));
        assert_eq!(Type::named("null"), Some(Type::Null));
        assert_eq!(Type::named("func"), Some(Type::Func));
        assert_eq!(Type::named("object"), Some(Type::Object));
        assert_eq!(Type::named("uint"), Some(Type::Uint));
    }

    #[test]
    fn an_unknown_name_is_not_a_type() {
        assert_eq!(Type::named(""), None);
        assert_eq!(Type::named("ကိန်း"), None);
        assert_eq!(Type::named("integer"), None);
    }
}
