//! AST: statements, conditions and loop sources.

use super::*;

#[derive(Debug, Clone)]
pub enum Stmt {
    VarDecl {
        name: Sym,
        value: Expr,
        line: usize,
    },
    Print {
        value: Expr,
        line: usize,
    },
    InputNoAssign {
        value: Expr,
        line: usize,
    },
    InputAssign {
        name: String,
        value: Expr,
        line: usize,
    },
    RandomDecl {
        name: String,
        min: Expr,
        max: Expr,
        line: usize,
    },
    ExprStmt {
        value: Expr,
        line: usize,
    },
    MathAssign {
        target: Expr,
        op: char,
        amount: Expr,
        line: usize,
    },
    /// Assignment into a collection element:
    ///   eng:     `xs[0] = <value>;`
    ///   Myanmar: `xs[0] သည် <value> ဖြစ်၏။`
    /// The target is always an `Expr::Index`; the interpreter writes through
    /// the variable that holds the collection (a list, a table row or a dict),
    /// so no name is rebound.
    IndexAssign {
        target: Expr,
        value: Expr,
        line: usize,
    },
    ForLoop {
        var_name: String,
        source: ForSource,
        body: Vec<Stmt>,
        line: usize,
    },
    If {
        branches: Vec<IfBranch>,
    },
    While {
        cond: CondChain,
        negate: bool,
        body: Vec<Stmt>,
        line: usize,
    },
    FuncDef {
        name: String,
        params: Vec<String>,
        /// One entry per parameter: `Some(expr)` for a parameter that has a
        /// default (`fn greet(name, punct = "!")`), `None` otherwise. Default
        /// parameters come last, so a call may leave them off from the right.
        defaults: Vec<Option<Expr>>,
        body: Vec<Stmt>,
        /// The line the `fn` / `အလုပ်` header is written on. The static
        /// warning pass (`crate::sema::warnings`) points the unused-function and
        /// unused-argument warnings at it.
        line: usize,
    },
    FuncCall {
        name: String,
        args: Vec<Expr>,
        line: usize,
    },
    /// `<var> အတွက် <fn> ကို လုပ်ရန် <arg> ဖြင့်` / `<var> အတွက် <fn> ကို လုပ်ပါ။`
    /// Also accepts a multi-line, multi-argument brace form:
    /// `<var> အတွက် <fn> ကို လုပ်ရန် { <arg1>, <arg2>, ... } ဖြင့်`
    /// Calls a function and stores its return value (the function body's
    /// final bare expression statement) into `name`.
    FuncCallAssign {
        name: String,
        fn_name: String,
        args: Vec<Expr>,
        line: usize,
    },
    /// Library function call (result discarded):
    ///   `<lib> ၏ <fn>(<arg>, ...) ကို လုပ်ပါ။`
    ///   `<lib> ၏ <fn> ကို လုပ်ရန် <arg>, ... ဖြင့်။`
    ///   `<lib> ၏ <fn> ကို လုပ်ရန် (<arg>, ...) ဖြင့်။`
    ///   `<lib> ၏ <fn> ကို လုပ်ပါ။`  (no arguments)
    LibCall {
        lib: String,
        fn_name: String,
        args: Vec<Expr>,
        line: usize,
    },
    /// Library function call whose result is stored in `name`:
    ///   `<var> အတွက် <lib> ၏ <fn>(<arg>, ...) ကို လုပ်ပါ။`
    ///   `<var> အတွက် <lib> ၏ <fn> ကို လုပ်ရန် <arg>, ... ဖြင့်။`
    ///   `<var> အတွက် <lib> ၏ <fn> ကို လုပ်ရန် (<arg>, ...) ဖြင့်။`
    ///   `<var> အတွက် <lib> ၏ <fn> ကို လုပ်ပါ။`  (no arguments)
    LibCallAssign {
        name: String,
        lib: String,
        fn_name: String,
        args: Vec<Expr>,
        line: usize,
    },
    /// `<fn name> အဖြစ် <alias1>, <alias2>, ...`
    /// Registers one or more alias names for an existing function, so
    /// calling any alias calls the original function.
    FuncAlias {
        original: String,
        aliases: Vec<String>,
        line: usize,
    },
    ClassDef {
        name: String,
        /// Constructor parameters declared in the class header, e.g. the
        /// `x, y` in `ပုံသေ Point(x, y) သည် ... ပြီး။`. `None` for the
        /// older parenthesis-less header, where the constructor is the
        /// first method and its own parameters supply the constructor's.
        params: Option<Vec<String>>,
        body: Vec<Stmt>,
        line: usize,
    },
    SelfFieldSet {
        field: String,
        value: Expr,
        line: usize,
    },
    TryCatch {
        try_body: Vec<Stmt>,
        catch_err: Option<String>,
        catch_var: Option<String>,
        catch_body: Option<Vec<Stmt>>,
        finally_body: Option<Vec<Stmt>>,
    },
    /// Library import:
    /// `နည်းပညာများ <lib name>[, <lib name>, ...][ အဖြစ် <alias>] ကို သုံးမည်။`
    UseLibrary {
        /// `(library name, optional alias)` pairs, in source order. The
        /// alias (from `... အဖြစ် <alias>`) lets later statements reach the
        /// library under a shorter name: `r ၏ ...` for `ကျပန်း ၏ ...`.
        libs: Vec<(String, Option<String>)>,
        line: usize,
    },
    /// Wait (sleep): `10s ကို စောင့်ပါ။` (also `10m` / `10h`, or a bare
    /// number which defaults to seconds).
    Wait {
        amount: Expr,
        unit: WaitUnit,
        line: usize,
    },
    /// HTTP send (POST-style): `<var> အတွက် <lib> ၏ <url> သို့ <data> ကို
    /// ပို့ပါ။` -- sends `<data>` to `<url>` via `<lib>` (currently only
    /// "request" is supported) and stores the resulting response object
    /// into `<var>`.
    HttpSend {
        name: String,
        lib: String,
        url: Expr,
        data: Expr,
        line: usize,
    },
    /// eng library declaration, written in the C-style spelling (a lone '='
    /// assignment, ':' type annotation, ';' terminator) instead of the
    /// Myanmar sentence forms:
    ///
    ///   `name :int = 5;`        -- mutable typed declaration
    ///   `pin name :bool = true;` -- constant typed declaration
    ///
    /// `is_const` is set for the `pin` form. The interpreter enforces the
    /// declared type and refuses reassignment of `pin` values.
    EngDecl {
        name: Sym,
        type_name: String,
        value: Expr,
        is_const: bool,
        line: usize,
    },
    /// eng library print: `print(<value>);` -- same behavior as the
    /// Myanmar `... ကို ဖော်ပြပါ။` statement.
    EngPrint {
        value: Expr,
        line: usize,
    },
    /// eng library input (prompt only, input discarded):
    /// `input("... ");`
    EngInput {
        prompt: Expr,
        line: usize,
    },
    /// eng library input (result stored): `<name> :<type> = input("...");`
    /// -- shows the prompt, reads a line, coerces it to the declared type.
    EngInputAssign {
        name: String,
        type_name: String,
        prompt: Expr,
        line: usize,
    },
    /// eng library return: `return;` / `return <value>;` -- unwinds the
    /// enclosing function call (Myanmar functions may use it too).
    EngReturn {
        value: Option<Expr>,
        line: usize,
    },
    /// eng library function-call statement: `name(arg1, arg2);` -- calls a
    /// user-defined function (Myanmar- or eng-defined) or a eng builtin,
    /// discarding any result.
    EngExprCall {
        name: String,
        args: Vec<Expr>,
        line: usize,
    },
    /// eng library assignment: `name = <value>;` -- reassigns an existing
    /// variable (eng `pin` constants are protected).
    EngAssign {
        name: Sym,
        value: Expr,
        line: usize,
    },
    /// eng compound assignment: `name += <value>;` (also `-=`, `*=`, `/=`,
    /// `%=`, `^=`) -- applies the operator to the existing value and stores
    /// the result back into the same variable.
    EngMathAssign {
        name: Sym,
        op: char,
        amount: Expr,
        line: usize,
    },
    /// eng/Myanmar break: `break;` / `break <value>;` / `ရပ်ပါ။` /
    /// `<value> ကို ရပ်ပါ။` -- stops the innermost enclosing loop. The
    /// optional value becomes the value of a `loop { ... }` expression.
    EngBreak {
        value: Option<Expr>,
        line: usize,
    },
    /// `continue;` / `ဆက်ပါ။` -- skips the rest of the innermost loop body
    /// and starts the next iteration. Unlike `break`, it takes no value.
    Continue {
        line: usize,
    },
    /// Destructuring assignment: `[a, b] = <value>;` (or `(a, b) = <value>;`).
    /// The value must be a list or tuple with exactly one item per name, and
    /// each name takes its item in order.
    Destructure {
        names: Vec<String>,
        value: Expr,
        line: usize,
    },
    /// `switch (<value>) { case <expr>: ... default: ... }` -- runs the arm
    /// whose `case` equals the value, or `default` when none does. There is
    /// no fall-through, so arms need no `break` (a `break` inside still
    /// stops the switch, and works inside a loop the switch is in).
    Switch {
        subject: Expr,
        arms: Vec<SwitchArm>,
        line: usize,
    },
    /// eng infinite loop statement: `loop { ... }` -- runs until a `break`
    /// (or a `return` unwinds out of the enclosing function).
    EngLoop {
        body: Vec<Stmt>,
    },
    /// eng counted loop: `for (<init>; <cond>; <step>) { ... }`. `init` runs
    /// once before the loop, `cond` is tested before every iteration, and
    /// `step` runs at the end of each one -- so a `break` inside the body
    /// skips the step, exactly as in C.
    EngFor {
        init: Box<Stmt>,
        cond: CondChain,
        step: Box<Stmt>,
        body: Vec<Stmt>,
        line: usize,
    },
    /// eng loop-as-value: `<name> :<type> = loop { ... };` (or the plain
    /// assignment spelling `<name> = loop { ... };`). The variable is seeded
    /// with its type's default before the loop runs (so the body can
    /// accumulate into it) and takes the value passed to `break <value>`.
    EngLoopAssign {
        name: String,
        /// None for the assignment spelling (the name must already exist).
        type_name: Option<String>,
        is_const: bool,
        body: Vec<Stmt>,
        line: usize,
    },
}

#[derive(Debug, Clone)]
pub struct SwitchArm {
    /// The `case` value this arm matches, or `None` for `default:`.
    pub case: Option<Expr>,
    pub body: Vec<Stmt>,
    pub line: usize,
}

#[derive(Debug, Clone)]
pub struct IfBranch {
    /// None only for the final, unconditional "မဟုတ်လျှင်" (else) branch.
    pub cond: Option<CondChain>,
    /// True if this branch used the negative form ("မဖြစ်လျှင်": run when
    /// the condition chain is FALSE) rather than the positive form
    /// ("ဖြစ်လျှင်": run when TRUE).
    pub negate: bool,
    pub body: Vec<Stmt>,
}

#[derive(Debug, Clone)]
pub enum LogicalOp {
    And,
    Or,
}

#[derive(Debug, Clone)]
pub struct CondAtom {
    pub lhs: Expr,
    /// None for a bare boolean check like "(flag)" -- just test lhs's
    /// truthiness. Some(op) for a full comparison "lhs op rhs".
    pub op: Option<String>, // "<" ">" "==" "!=" "<=" ">="
    pub rhs: Option<Expr>,
    /// True when this atom was introduced by the eng library's logical-not
    /// token `!` (e.g. `if (!flag) { ... }`) -- the interpreter negates the
    /// atom's truth value.
    pub negate: bool,
    /// Some(type_name) for a type-check condition "lhs သည် <type>", e.g.
    /// "(x သည် ကိန်း)". Mutually exclusive with `op`/`rhs`.
    pub type_check: Option<String>,
    pub line: usize,
}

#[derive(Debug, Clone)]
pub struct CondChain {
    pub first: CondAtom,
    pub rest: Vec<(LogicalOp, CondAtom)>,
}

#[derive(Debug, Clone)]
pub enum ForSource {
    Range {
        start: Option<Expr>,
        end: Expr,
        step: Expr,
        op: char,
    },
    /// `<var> အတွက် တစ်ခုစီ(<from>, <end>[, <step>])` -- the parenthesized
    /// spelling of a numeric range. `<end>` is exclusive and the sign of
    /// `<step>` (default `1`) picks the direction, so a negative step counts
    /// down.
    RangeSigned { start: Expr, end: Expr, step: Expr },
    /// No explicit step-function clause was given. Resolved at runtime:
    /// a number auto-ranges from 0 (step +1); a list/tuple/set/dict iterates
    /// its elements (keys, for dict).
    Auto(Expr),
}

/// Unit of a wait amount: `s` (seconds), `m` (minutes), `h` (hours).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaitUnit {
    Seconds,
    Minutes,
    Hours,
}
