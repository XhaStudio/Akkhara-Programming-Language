//! AST: expressions.

use super::*;

#[derive(Debug, Clone)]
pub enum Expr {
    /// A numeric literal that could not be parsed into `Int`/`Float`
    /// (e.g. `1.2.3`). Kept as text so the interpreter still reports it as
    /// `E047` when -- and only when -- it is evaluated.
    NumLit(String),
    /// A numeric literal already parsed at parse time. The interpreter used
    /// to run `parse::<i64>()` / `parse::<f64>()` on the literal text on
    /// every single evaluation; the value is the same either way, and the
    /// `E047` behavior above is unchanged because unparseable literals fall
    /// back to `NumLit`.
    Int(i64),
    Float(f64),
    StrLit(String),
    BoolLit(bool),
    /// `null` / `ဗလာ` -- JSON's null, now writable as a literal as well as
    /// returned by `json.parse`.
    NullLit,
    Ident(Sym),
    Binary(Box<Expr>, char, Box<Expr>, usize), // line of the operator
    /// A comparison used as a value: `x > y`, `name == "Aung"`. The same
    /// operators a condition accepts, producing a `bool`.
    Compare(Box<Expr>, String, Box<Expr>, usize), // line of the operator
    /// Membership, the operator spelling of `contains(<haystack>, <needle>)`:
    /// `x in xs` in the eng syntax and `xs ထဲ x` in the Myanmar one. The
    /// fields are (haystack, needle), so both spellings share one node.
    Contains(Box<Expr>, Box<Expr>, usize), // line of the operator
    /// The eng library's logical-and / logical-or used as a value:
    /// `a && b`, `a || b`. Both sides must be `bool`. Expressions
    /// short-circuit, so the right side is not evaluated when the left one
    /// already decides the result -- `i < len(xs) && xs[i] > 0` is safe.
    Logical(Box<Expr>, LogicalOp, Box<Expr>, usize), // line of the operator
    /// The eng library's conditional expression: `<cond> ? <then> : <else>`.
    /// Only the branch the condition picks is evaluated, so the other side
    /// may be anything (a call with side effects, a division by zero).
    Ternary(Box<Expr>, Box<Expr>, Box<Expr>, usize), // line of the '?'
    /// The eng library's logical-not used as a value: `!done`. The operand
    /// must be `bool`.
    Not(Box<Expr>, usize), // line of the operator
    Neg(Box<Expr>, usize),                     // unary minus, line
    ListLit(Vec<Expr>),
    TupleLit(Vec<Expr>),
    SetLit(Vec<Expr>),
    DictLit(Vec<(Expr, Expr)>),
    /// Postfix index: `xs[i]` or table `xs[row, col]` (or dict `xs[key]`).
    Index(Box<Expr>, Vec<Expr>),
    /// Object instantiation: `<ClassName>(arg1, arg2, ...)`. This is the only
    /// spelling; the old `အသစ်` marker (on either side of the argument list)
    /// is refused with `E029`.
    NewObj(String, Vec<Expr>, usize),
    /// An anonymous function value: `fn(x) { x * 2 }`. It is the same pair a
    /// named function definition holds -- parameters and a body -- so a
    /// callback can be written where it is used instead of being defined and
    /// then named: `map(xs, fn(x) { x * 2 })`. Like a function body, the last
    /// expression in the block is its value.
    Lambda {
        params: Vec<String>,
        /// Defaults, parallel to `params` -- see `Stmt::FuncDef::defaults`.
        defaults: Vec<Option<Expr>>,
        body: Vec<Stmt>,
        line: usize,
    },
    /// Library call in the eng spelling: `<lib>.<fn>(arg1, arg2, ...)` --
    /// the same call as the Myanmar `<lib> ၏ <fn>(args) ကို လုပ်ပါ။`.
    LibCall(String, String, Vec<Expr>, usize),
    /// A library function named without calling it: `<lib>.<fn>`. Its
    /// value is a callable, so `TOOL = Tools.add;` binds `add` and a later
    /// `TOOL(1, 100)` invokes it.
    LibFnRef(String, String, usize),
    /// Member/field access: `<expr> ၏ <field>` -- reads a named field off
    /// an Akkhara object (e.g. `response ၏ အခြေအနေကုဒ်` reads the status
    /// code field off a "request" library response object).
    FieldAccess(Box<Expr>, String, usize),
    /// HTTP GET via a network-capable native library: `<lib> ၏ <url>`,
    /// e.g. `request ၏ "https://api.example.com/data"`. `<lib>` must name
    /// a recognized library (currently only "request"); the interpreter
    /// checks that it has actually been imported before calling out.
    HttpGet(String, Box<Expr>, usize),
}
