//! The Myanmar spelling of every keyword the parser recognises.
//!
//! These are the words a program is written with; the English spelling lives
//! in `keywords_en`, and both build the same syntax tree.

pub(crate) const KW_ASSIGN: &str = "သည်";

pub(crate) const KW_ASSIGN_COLLECTION: &str = "မှာ";

pub(crate) const KW_IS: &str = "ဖြစ်၏";

pub(crate) const KW_PARTICLE: &str = "ကို";

/// Short declaration: `<name> က <value> ။` -- the same statement as
/// `<name> သည် <value> ဖြစ်၏။`, with the `ဖြစ်၏` left out.
pub(crate) const KW_DECL: &str = "က";

pub(crate) const KW_PRINT: &str = "ဖော်ပြပါ";

/// Short print: `<value> ကို ပြပါ။` -- `ပြပါ` for `ဖော်ပြပါ`.
pub(crate) const KW_PRINT_ALT: &str = "ပြပါ";

pub(crate) const KW_INPUT: &str = "မေးပါ";

pub(crate) const KW_FOR: &str = "အတွက်";

/// `သို့` -- the "to" of the request library's send sentence, and the
/// Myanmar spelling of the `or` condition operator. The conversion sentence
/// it used to introduce is gone; see `KW_CONVERT` below.
pub(crate) const KW_TO: &str = "သို့";

/// `ပြောင်းပါ` -- the verb of the removed conversion sentence. Kept only so
/// that a line still using it reports its replacement (`စာ(x)`, `int(x)`, ...)
/// instead of a generic syntax error.
pub(crate) const KW_CONVERT: &str = "ပြောင်းပါ";

pub(crate) const KW_LOOP_FROM: &str = "ထဲမှ";

/// `ထဲ` -- the short loop particle (`x သည် xs ထဲ တစ်ခုစီ`) and the
/// membership operator (`(xs ထဲ x)` is `x in xs`).
pub(crate) const KW_INSIDE: &str = "ထဲ";

pub(crate) const KW_LOOP_EACH: &str = "တစ်ခုစီ";

// Type-check condition keywords: "<var/val> သည် <type>" inside if/while.
// Distinct from the conversion-target type keywords (e.g. "ကိန်းပြည့်" = int):
// here "ကိန်း" (no ပြည့်) means a broad "is this a number" check that also
// accepts numeric-looking strings, per spec.
pub(crate) const TC_STR: &str = "စာသား";

pub(crate) const TC_NUM: &str = "ကိန်း";

pub(crate) const TC_FLOAT: &str = "ဒဿမကိန်း";

pub(crate) const TC_BOOL: &str = "မှန်/မှား";

pub(crate) const KW_LOOP_END: &str = "ပြီး";

/// eng `break`, spelled as a Myanmar keyword: `ရပ်ပါ။`.
pub(crate) const KW_BREAK: &str = "ရပ်ပါ";

/// eng `continue`, spelled as a Myanmar keyword: `ဆက်ပါ။`.
pub(crate) const KW_CONTINUE: &str = "ဆက်ပါ";

/// Raise a user error: `<value> ကို <name> အမည်ဖြင့် ပစ်ပါ။` -- "throw
/// `<value>` with the name `<name>`". `ဖမ်းပါ` (catch) can select it by that
/// name, by the raised error's code `E178`, or catch anything with `E`.
pub(crate) const KW_RAISE: &str = "ပစ်ပါ";

/// The name particle in a raise sentence: `<value> ကို <name> အမည်ဖြင့်
/// ပစ်ပါ။` -- the error identity that follows it.
pub(crate) const KW_RAISE_NAME: &str = "အမည်ဖြင့်";

pub(crate) const KW_TRY: &str = "စမ်းရန်";

pub(crate) const KW_CATCH: &str = "ဖမ်းပါ";

pub(crate) const KW_FINALLY: &str = "နောက်ဆုံးတွင်";

pub(crate) const KW_IF: &str = "အကယ်၍";

pub(crate) const KW_ELIF: &str = "သို့မဟုတ်";

/// `အခြား` -- the short spelling of `သို့မဟုတ်` (the else-if branch).
pub(crate) const KW_ELIF_ALT: &str = "အခြား";

pub(crate) const KW_ELSE: &str = "မဟုတ်လျှင်";

/// `မဟုတ်ရင်` -- the short spelling of `မဟုတ်လျှင်` (the else branch).
pub(crate) const KW_ELSE_ALT: &str = "မဟုတ်ရင်";

pub(crate) const KW_THEN_POS: &str = "ဖြစ်လျှင်";

/// `ဖြစ်ရင်` -- the short spelling of `ဖြစ်လျှင်`.
pub(crate) const KW_THEN_POS_ALT: &str = "ဖြစ်ရင်";

pub(crate) const KW_THEN_NEG: &str = "မဖြစ်လျှင်";

/// `မဖြစ်ရင်` -- the short spelling of `မဖြစ်လျှင်`.
pub(crate) const KW_THEN_NEG_ALT: &str = "မဖြစ်ရင်";

pub(crate) const KW_AND: &str = "နှင့်";

pub(crate) const KW_OR: &str = "သို့";

/// `and` / `or` -- the English spellings of `(နှင့်)` / `(သို့)`.
pub(crate) const KW_AND_ALT: &str = "and";

pub(crate) const KW_OR_ALT: &str = "or";

pub(crate) const KW_WHILE: &str = "အခြေအနေ";

pub(crate) const KW_WHILE_POS: &str = "ဖြစ်နေစဉ်";

/// `ဖြစ်နေစဥ်` -- the same word with `ဥ` instead of `ဉ`.
pub(crate) const KW_WHILE_POS_ALT: &str = "ဖြစ်နေစဥ်";

/// `ဖြစ်နေရင်` -- the positive while header written with the `ရင်`
/// terminator instead of `စဉ်`: it loops while the condition is true, the
/// same as `ဖြစ်နေစဉ်`.
pub(crate) const KW_WHILE_POS_ALT2: &str = "ဖြစ်နေရင်";

pub(crate) const KW_WHILE_NEG: &str = "မဖြစ်နေစဉ်";

/// `မဖြစ်နေစဥ်` -- `မဖြစ်နေစဉ်` with `ဥ`.
pub(crate) const KW_WHILE_NEG_ALT: &str = "မဖြစ်နေစဥ်";

/// `မဖြစ်နေရင်` -- the negative while header written with the `ရင်`
/// terminator instead of `စဉ်`: it loops while the condition is not true,
/// the same as `မဖြစ်နေစဉ်`.
pub(crate) const KW_WHILE_NEG_ALT2: &str = "မဖြစ်နေရင်";

pub(crate) const KW_FUNC_DEF: &str = "အလုပ်";

/// `လုပ်ငန်း` -- the older spelling of the function keyword, still used by
/// the README and by existing programs. Both spellings open a definition.
pub(crate) const KW_FUNC_DEF_ALT: &str = "လုပ်ငန်း";

/// `ရွေးရန်` -- the Myanmar switch header: `ရွေးရန် (<value>) ... ပြီး။`.
pub(crate) const KW_SWITCH: &str = "ရွေးရန်";

/// `မူလ` -- the `default:` arm of a `ရွေးရန်` switch, run only when no
/// `<value> ဖြစ်ရင်` arm matched.
pub(crate) const KW_SWITCH_DEFAULT: &str = "မူလ";

pub(crate) const KW_BY: &str = "ဖြင့်";

pub(crate) const KW_CALL: &str = "လုပ်ပါ";

pub(crate) const KW_CALL_WITH: &str = "လုပ်ရန်";

pub(crate) const KW_CLASS_DEF: &str = "ပုံသေ";

/// `အတု` -- the self keyword inside a class method:
/// `အတု <field> သည် <value> ဖြစ်၏။` (the old `တန်ဖိုး` spelling is gone).
pub(crate) const KW_SELF: &str = "အတု";

/// `အသစ်` -- the removed marker of the object-creation expression, which is
/// now just `<class name>(<argument>, ...)`. It stays a known word only so a
/// line still using it reports its replacement (`E029`) instead of a generic
/// syntax error, the same way `KW_CONVERT` reports the removed conversion
/// sentence.
pub(crate) const KW_NEW: &str = "အသစ်";

// Library system keywords.
pub(crate) const KW_USE: &str = "သုံးမည်";

/// `အသုံးပြုပါ` -- the other verb the import sentence is written with (the
/// spelling the README and the library READMEs use).
pub(crate) const KW_USE_ALT: &str = "အသုံးပြုပါ";

pub(crate) const KW_LIBRARIES: &str = "နည်းပညာများ";

pub(crate) const KW_WAIT: &str = "စောင့်ပါ";

pub(crate) const KW_RANDOM: &str = "ကျပန်းကိန်း";

pub(crate) const KW_BETWEEN: &str = "အကြား";

/// The type keywords a condition can name (`<expr> သည် <type>`). The ones
/// `parse_typecheck_name` cannot check are listed so `(x သည် ကိန်းပြည့်)`
/// keeps reporting a condition-syntax error instead of comparing `x` to a
/// variable that happens to share the type word.
pub(crate) const TYPE_WORDS: &[&str] = &[
    "စာသား",
    "ကိန်း",
    "ကိန်းပြည့်",
    "ဒဿမ",
    "ဒဿမကိန်း",
    "စာရင်း",
    "အစု",
    "အုပ်စု",
    "အဘိဓာန်",
    "ဇယား",
];

pub(crate) const KW_AS: &str = "အဖြစ်";

// Member access / network library keywords.
pub(crate) const KW_OF: &str = "၏";

pub(crate) const KW_SEND: &str = "ပို့ပါ";

pub(crate) const LIB_REQUEST: &str = "request";

pub const MATH_FN_INC: &str = "တိုးပါ";

pub const MATH_FN_DEC: &str = "လျော့ပါ";

pub const MATH_FN_MUL: &str = "မြှောက်ပါ";

pub const MATH_FN_DIV: &str = "စားပါ";

pub const LOOP_FN_INC: &str = "တိုးခြင်းဖြင့်";

pub const LOOP_FN_DEC: &str = "လျော့ခြင်းဖြင့်";

pub const LOOP_FN_MUL: &str = "မြှောက်ခြင်းဖြင့်";

pub const LOOP_FN_DIV: &str = "စားခြင်းဖြင့်";
