//! Mid-level IR: basic blocks and explicit branches.
//!
//! One function is a list of blocks; each block ends in exactly one
//! terminator, so nothing is nested any more and a branch is a block id. Values
//! are already flattened into temporaries (`t0`, `t1`, ...): every instruction
//! writes at most one temporary, and a variable read is a `VarId` slot.
//!
//! These temporaries are slots, not SSA values: a loop counter is written in
//! several iterations. Turning them into SSA -- and the optimisations that
//! become possible then -- belongs to `opt`, which is still empty.
//!
//! Calls are not inlined or devirtualised: `call add(1, 2)` is a function,
//! a builtin or a class construction, which is what the interpreter decides at
//! run time. Nothing here changes what a program means.

use crate::scope::symbol::VarId;
use crate::types::Type;

/// A block id. Blocks are stored in creation order, so the numbers are labels
/// in the dump rather than a flow order.
pub(crate) type BlockId = u32;

/// A temporary: an instruction's destination.
pub(crate) type Temp = u32;

#[derive(Debug)]
pub(crate) struct MirProgram {
    /// Index 0 is the top-level statements. A `Const::Fn(i)` refers to this
    /// list, so a lambda at `HirProgram::functions[i]` is `functions[i + 1]`
    /// here.
    pub(crate) functions: Vec<MirFn>,
}

#[derive(Debug)]
pub(crate) struct MirFn {
    pub(crate) name: String,
    pub(crate) params: Vec<VarId>,
    /// Names read from enclosing functions.
    pub(crate) captures: Vec<VarId>,
    pub(crate) blocks: Vec<BasicBlock>,
    /// How many temporaries the function uses, for the dump.
    pub(crate) temps: u32,
}

#[derive(Debug)]
pub(crate) struct BasicBlock {
    pub(crate) id: BlockId,
    pub(crate) insts: Vec<MirInst>,
    /// `None` only while the block is still being built.
    pub(crate) term: Option<Terminator>,
}

/// A value an instruction reads: a literal, a variable, a temporary or a name
/// that has to be looked up when the program runs.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Operand {
    Const(Const),
    Var(VarId),
    Temp(Temp),
    /// A dynamic name (builtin, library function, class) -- the interpreter's
    /// run-time lookup, kept as a value rather than resolved here.
    Name(String),
    /// An anonymous function value, by index into `MirProgram::functions`.
    Fn(usize),
}

impl std::fmt::Display for Operand {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Operand::Const(value) => write!(f, "{}", value),
            Operand::Var(id) => write!(f, "{}", id),
            Operand::Temp(temp) => write!(f, "t{}", temp),
            Operand::Name(name) => write!(f, "{}", name),
            Operand::Fn(index) => write!(f, "fn#{}", index),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Const {
    Int(i64),
    Float(f64),
    Str(String),
    Bool(bool),
    Null,
}

impl std::fmt::Display for Const {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Const::Int(n) => write!(f, "{}", n),
            Const::Float(n) => write!(f, "{:?}", n),
            Const::Str(text) => write!(f, "{:?}", text),
            Const::Bool(value) => write!(f, "{}", value),
            Const::Null => write!(f, "null"),
        }
    }
}

#[derive(Debug)]
pub(crate) enum MirInst {
    /// `dest = lhs <op> rhs`
    Binary {
        dest: Temp,
        op: char,
        lhs: Operand,
        rhs: Operand,
    },
    /// `dest = lhs <op> rhs`, producing a `bool`.
    Compare {
        dest: Temp,
        /// `<`, `<=`, `>`, `>=`, `==`, `!=`
        op: String,
        lhs: Operand,
        rhs: Operand,
        /// The source line, so ordering a collection reports the
        /// interpreter's `E042` with the same line.
        line: usize,
    },
    Not {
        dest: Temp,
        value: Operand,
    },
    Neg {
        dest: Temp,
        value: Operand,
    },
    /// `dest = value` -- only where a value has to move into a temporary
    /// chosen before the instructions that produce it (a `?:` arm, a
    /// short-circuit side).
    Move {
        dest: Temp,
        value: Operand,
    },
    /// `dest = name(args)`: a user function, a builtin or a class.
    Call {
        dest: Temp,
        name: String,
        args: Vec<Operand>,
    },
    LibCall {
        dest: Temp,
        lib: String,
        func: String,
        args: Vec<Operand>,
    },
    Index {
        dest: Temp,
        base: Operand,
        indices: Vec<Operand>,
        /// The source line, so the native runtime's `E049`/`E053` text is
        /// byte-identical to the interpreter's (the other compiled panics
        /// still have no span).
        line: usize,
    },
    Field {
        dest: Temp,
        base: Operand,
        field: String,
    },
    List {
        dest: Temp,
        items: Vec<Operand>,
    },
    Tuple {
        dest: Temp,
        items: Vec<Operand>,
    },
    Set {
        dest: Temp,
        items: Vec<Operand>,
    },
    Dict {
        dest: Temp,
        pairs: Vec<(Operand, Operand)>,
    },
    /// `dest = value is <type>`
    IsType {
        dest: Temp,
        value: Operand,
        ty: Type,
    },
    /// `dest = items(value)`: the elements of a collection, or `0..value` for a
    /// number -- the same dispatch `ForSource::Auto` does at run time.
    Items {
        dest: Temp,
        value: Operand,
    },
    /// `dest = len(value)`
    Len {
        dest: Temp,
        value: Operand,
    },
    StoreVar {
        var: VarId,
        /// The operator of a compound assignment, `None` for a plain store.
        op: Option<char>,
        value: Operand,
    },
    StoreIndex {
        base: Operand,
        indices: Vec<Operand>,
        value: Operand,
        /// The source line, so the native runtime's `E049` text matches the
        /// interpreter's for a write out of range.
        line: usize,
    },
    StoreField {
        base: Operand,
        field: String,
        value: Operand,
    },
    Print(Operand),
    Eval(Operand),
    /// `set_error <identity> <prefix>`: remember the raise that is on its way
    /// to a handler. `prefix` is the whole `E178 လိုင်း N တွင် [name] ` text,
    /// built once here, so reporting an uncaught raise writes it unchanged.
    SetError {
        identity: String,
        prefix: String,
        value: Operand,
    },
    /// `dest = the pending raise has this identity` -- the catch filter.
    ErrorMatches {
        dest: Temp,
        identity: String,
    },
    /// `var = the pending raise's message`; the catch clause's binding.
    BindError {
        var: VarId,
    },
    /// Forget the pending raise, once a catch clause has taken it.
    ClearError,
    /// Leave the block for `target` when a raise is pending. Emitted after a
    /// call, so an error raised in a callee unwinds to this function's
    /// handler (or back to its own caller) with the same code a raise written
    /// here uses.
    CheckError {
        target: BlockId,
    },
}

/// How a block ends.
#[derive(Debug, Clone)]
pub(crate) enum Terminator {
    Goto(BlockId),
    /// `branch cond, bb_then, bb_else`
    Branch {
        cond: Operand,
        then_block: BlockId,
        else_block: BlockId,
    },
    Return(Option<Operand>),
    /// The top-level block of a function that just runs out.
    Exit,
    /// Report the pending raise on stderr and exit 1: what `<main>` does
    /// with a raise no clause caught.
    ReportError,
    /// Leave the function with a raise still pending, so its caller's own
    /// check (or handler) deals with it.
    Propagate,
}

impl std::fmt::Display for Terminator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Terminator::Goto(id) => write!(f, "goto bb{}", id),
            Terminator::Branch {
                cond,
                then_block,
                else_block,
            } => write!(f, "branch {}, bb{}, bb{}", cond, then_block, else_block),
            Terminator::Return(Some(value)) => write!(f, "return {}", value),
            Terminator::Return(None) => write!(f, "return"),
            Terminator::Exit => write!(f, "exit"),
            Terminator::ReportError => write!(f, "report_error"),
            Terminator::Propagate => write!(f, "propagate"),
        }
    }
}
