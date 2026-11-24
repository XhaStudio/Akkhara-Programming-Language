//! High-level IR: the syntax tree with names resolved.
//!
//! The shape follows the AST closely on purpose -- HIR is where a name becomes
//! a binding and a statement becomes a node with a line, not where the tree is
//! restructured. Control flow is still nested; `mir` is what makes the branches
//! explicit. Types are attached where the program declared one (`name :int = 5;`);
//! nothing infers types yet.

use crate::frontend::ast::LogicalOp;
use crate::scope::capture::Capture;
use crate::scope::error::Unresolved;
use crate::scope::symbol::{SymbolTable, VarId};
use crate::types::Type;

#[derive(Debug)]
pub(crate) struct HirProgram {
    /// The top-level statements (frame 0).
    pub(crate) main: HirFn,
    /// Every function and lambda, in resolution order. `HirExpr::Lambda`
    /// indexes this list.
    pub(crate) functions: Vec<HirFn>,
    /// Every binding the resolver created, for the dump.
    pub(crate) symbols: SymbolTable,
    /// Names that stay dynamic: builtins, library functions, classes.
    pub(crate) unresolved: Vec<Unresolved>,
}

#[derive(Debug)]
pub(crate) struct HirFn {
    /// `<main>` for the top-level statements, `<lambda>` for an anonymous one.
    pub(crate) name: String,
    pub(crate) params: Vec<HirParam>,
    pub(crate) body: Vec<HirStmt>,
    /// The header line; `0` for the top level, which has none.
    pub(crate) line: usize,
    /// Names from enclosing functions this body reads (captured by value when
    /// the function value is created, exactly as `interp` does).
    pub(crate) captures: Vec<Capture>,
}

#[derive(Debug)]
pub(crate) struct HirParam {
    pub(crate) var: VarId,
    pub(crate) name: String,
    /// The default expression of a parameter that has one, already resolved.
    pub(crate) default: Option<HirExpr>,
}

#[derive(Debug)]
pub(crate) enum HirStmt {
    /// A declaration or an assignment. `ty` is the type an eng declaration
    /// wrote, `op` the operator of a compound assignment (`+=`, `-=`, ...);
    /// both are `None` for a plain store.
    Store {
        var: VarId,
        ty: Option<Type>,
        op: Option<char>,
        value: HirExpr,
        line: usize,
    },
    /// `xs[i] = v` / `table[r, c] = v` -- writes through the binding that holds
    /// the collection, so no name is rebound.
    SetIndex {
        target: HirExpr,
        indices: Vec<HirExpr>,
        value: HirExpr,
        line: usize,
    },
    /// `object.field = value`.
    SetField {
        object: HirExpr,
        field: String,
        value: HirExpr,
        line: usize,
    },
    Print {
        value: HirExpr,
        line: usize,
    },
    /// An expression whose value is discarded.
    Eval {
        value: HirExpr,
        line: usize,
    },
    If {
        branches: Vec<HirBranch>,
    },
    /// `switch (value) { case c: ... default: ... }`. The subject is evaluated
    /// once, then the case expressions in written order until one equals it;
    /// the first `default` arm runs only when none did. There is no
    /// fall-through, so at most one arm's body runs.
    Switch {
        subject: HirExpr,
        arms: Vec<HirArm>,
        line: usize,
    },
    /// `while`, and the C-style `for`: `step` runs after each body and before
    /// the next test, so a `continue` still runs it.
    While {
        cond: HirExpr,
        negate: bool,
        body: Vec<HirStmt>,
        step: Option<Box<HirStmt>>,
        line: usize,
    },
    Loop {
        body: Vec<HirStmt>,
        line: usize,
    },
    /// A counted loop. `op` is how the counter advances (`+`, `-`, `*`, `/`),
    /// and it also picks the comparison: `+`/`*` count up, `-`/`/` down.
    ForRange {
        var: VarId,
        start: HirExpr,
        end: HirExpr,
        step: HirExpr,
        op: char,
        body: Vec<HirStmt>,
        line: usize,
    },
    /// A loop over a collection (or a number, which auto-ranges from 0).
    ForEach {
        var: VarId,
        iter: HirExpr,
        body: Vec<HirStmt>,
        line: usize,
    },
    Return {
        value: Option<HirExpr>,
        line: usize,
    },
    Break {
        value: Option<HirExpr>,
        line: usize,
    },
    Continue {
        line: usize,
    },
    /// `use` names the libraries a program may call. The native backend does
    /// not model a library import yet, so MIR refuses it with `E176` at this
    /// line instead of letting the import disappear from the executable.
    Import {
        libs: Vec<String>,
        line: usize,
    },
    /// `<value> ကို <name> အမည်ဖြင့် ပစ်ပါ။` / `throw <value> as <name>;`
    Raise {
        identity: String,
        value: HirExpr,
        line: usize,
    },
    /// `စမ်းရန် ... ဖမ်းပါ ... နောက်ဆုံးတွင် ... ပြီး။`: a body with one optional
    /// catch clause and an optional `finally`. `catch_var` is declared by the
    /// lowering, so the clause's body reads it as an ordinary binding (the
    /// interpreter binds the raise's message there).
    TryCatch {
        try_body: Vec<HirStmt>,
        catch_err: Option<String>,
        catch_var: Option<VarId>,
        catch_body: Option<Vec<HirStmt>>,
        finally_body: Option<Vec<HirStmt>>,
        line: usize,
    },
    /// A statement the IR does not model yet, kept so nothing disappears
    /// silently. MIR refuses it with `E176`.
    Opaque {
        what: String,
        line: usize,
    },
}

#[derive(Debug)]
pub(crate) struct HirArm {
    /// The `case` value, or `None` for `default:`.
    pub(crate) case: Option<HirExpr>,
    pub(crate) body: Vec<HirStmt>,
    pub(crate) line: usize,
}

#[derive(Debug)]
pub(crate) struct HirBranch {
    /// `None` for the final `မဟုတ်လျှင်` / `else` branch.
    pub(crate) cond: Option<HirExpr>,
    /// True when the branch runs on a *false* condition (`မဖြစ်လျှင်`).
    pub(crate) negate: bool,
    pub(crate) body: Vec<HirStmt>,
    pub(crate) line: usize,
}

#[derive(Debug)]
pub(crate) enum HirExpr {
    Int(i64),
    Float(f64),
    Str(String),
    Bool(bool),
    Null,
    /// A binding the resolver tied to a declaration.
    Var(VarId),
    /// A name that stays dynamic: a builtin, a library function, a class.
    Name(String),
    /// A numeric literal the parser could not turn into a number (`1.2.3`).
    /// The interpreter reports `E047`, but only if it is evaluated.
    BadNum(String),
    /// `lhs <op> rhs`; the `usize` is the line of the operator, which a
    /// division or remainder needs for the interpreter's `E036`-`E039` text.
    Binary(char, Box<HirExpr>, Box<HirExpr>, usize),
    Compare(String, Box<HirExpr>, Box<HirExpr>),
    /// `&&` / `||`: short-circuiting, both sides `bool`.
    Logical(LogicalOp, Box<HirExpr>, Box<HirExpr>),
    Not(Box<HirExpr>),
    Neg(Box<HirExpr>),
    List(Vec<HirExpr>),
    Tuple(Vec<HirExpr>),
    Set(Vec<HirExpr>),
    Dict(Vec<(HirExpr, HirExpr)>),
    Index(Box<HirExpr>, Vec<HirExpr>),
    Field(Box<HirExpr>, String),
    /// `name(args)`: the parser's one call-shaped node, so this is a class
    /// construction, a user function or a builtin -- decided at run time.
    Call(String, Vec<HirExpr>),
    LibCall {
        lib: String,
        func: String,
        args: Vec<HirExpr>,
    },
    /// An anonymous function, by index into `HirProgram::functions`.
    Lambda(usize),
    /// `<cond> ? <then> : <else>` (and its Myanmar spelling).
    Ternary(Box<HirExpr>, Box<HirExpr>, Box<HirExpr>),
    /// `needle in collection` -- the operator spelling of `contains`.
    Contains(Box<HirExpr>, Box<HirExpr>),
    /// `value သည် <type>` used as a value.
    IsType {
        value: Box<HirExpr>,
        ty: Type,
    },
    /// `str(value)` and one `{…}` placeholder: the value's display text as a
    /// string. This is a form of its own rather than a `Call`, so a user
    /// function named `str` cannot shadow what interpolation means.
    ToStr(Box<HirExpr>),
    /// A library function named without calling it (`Tools.add`).
    LibFnRef {
        lib: String,
        func: String,
    },
    /// `request ၏ <url>`.
    HttpGet {
        lib: String,
        url: Box<HirExpr>,
    },
    /// An expression the IR does not model yet. MIR refuses it with `E176`.
    Opaque(String),
}
