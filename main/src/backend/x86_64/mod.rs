//! x86-64 code generation for the native slice.
//!
//! The subset is deliberate and small: functions over `int`, `float`, `bool`
//! and string literals, with `move`, `+ - * % /`, comparisons, branches,
//! `print`, calls and `return`. Every value is one 64-bit word:
//!
//! | kind | in a slot |
//! | --- | --- |
//! | `int` | the signed value |
//! | `float` | the bits of the `f64` (SSE registers carry the same bits) |
//! | `bool` | `0` or `1` |
//! | `str` | the address of `{ length: u64, bytes... }` in `.data` |
//! | unit | `0`, and never read |
//!
//! The language has no types to read, so [`analyze`] works them out: a value's
//! kind comes from what is done with it (`%` means int, printing does not say,
//! passing it to a parameter that is used as an int means int), and the
//! answer is settled for the whole program at once so a call can teach its
//! callee's parameters. A value whose kind cannot be worked out is refused
//! rather than guessed. An int and a float mix in arithmetic and in ordering
//! comparisons the way the interpreter does it -- the int is converted to a
//! float and the result is a float -- while `==` stays the interpreter's
//! structural comparison, where `1` and `1.0` are not equal.
//!
//! Comparing two strings is in the slice: `==`/`!=` compare their bytes and an
//! ordering compares them lexicographically, both through the runtime's
//! `rt_cmp_str`. The numeric builtins `abs`, `min` and `max` are too, because
//! their result's kind is the kind of their arguments: `abs` of an int is an
//! int and of a float a float, and `min`/`max` over ints stays an int (over
//! floats a float). The candidates of one `min`/`max` have to share a kind --
//! the interpreter would compare across int and float, but then the result's
//! kind depends on the values, which the slice cannot settle statically, so
//! that is refused instead of guessed. `floor`, `ceil` and `round` are in too:
//! each takes one number (an int is converted the way the interpreter converts
//! it) and returns the `int` its saturating `as i64` cast gives, so a NaN is 0
//! and anything at or past the ends of `i64` is `i64::MIN`/`i64::MAX`. A
//! builtin called with the wrong kind or number of arguments is refused with
//! the interpreter's own `E110`, because that is a broken program rather than a
//! construct outside the slice. `sqrt` is still outside the slice: its error
//! message quotes the offending value, which compiled code has no way to
//! render into a diagnostic. Anything else -- a closure, a class, a list, a
//! dict, a float remainder -- is refused with `E177`, naming the item.
//! Refusing is what keeps the backend honest: a construct it cannot emit must
//! never become silently different code.
//!
//! A raise (`ပစ်ပါ` / `throw`) and the try/catch clause that handles it are in
//! the slice. The message has to be a string (it is what a catch clause binds
//! and what an uncaught raise prints), and a pending raise travels back through
//! the generated functions -- the runtime keeps its identity, prefix and
//! message in three data slots -- so a call that raises is handled by the same
//! clause a raise written at the call site would reach. `<main>` prints the
//! interpreter's `E178` line to stderr and exits 1 when nothing catches it.

pub(crate) mod encode;
mod gen;

pub(crate) use encode::{Code, CC_E, CC_NE, CC_NS};
pub(crate) use encode::{R10, R11, R8, R9, RAX, RBP, RCX, RDI, RDX, RSI};
pub(crate) use gen::{compile, Data, Image, Import};

use std::collections::{HashMap, HashSet};

use crate::ir::mir::*;
use crate::scope::symbol::VarId;

/// The type of one value in the slice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    Int,
    Float,
    Bool,
    Str,
}

impl Kind {
    /// Whether this kind is a number, which is what `+ - * /`, ordering
    /// comparisons and `abs`/`min`/`max` accept (mixing the two promotes to
    /// float, except in the builtins, which need one kind).
    fn is_number(self) -> bool {
        matches!(self, Kind::Int | Kind::Float)
    }
}

/// A builtin the backend emits itself, rather than one the interpreter's
/// `call_builtin` handles. Only the ones whose result kind follows from their
/// arguments are here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Builtin {
    Abs,
    Min,
    Max,
    Floor,
    Ceil,
    Round,
}

/// The builtin called `name`, if the slice emits it. A user function of the
/// same name wins over this, exactly as it does in the interpreter.
pub(crate) fn builtin(name: &str) -> Option<Builtin> {
    match name {
        "abs" => Some(Builtin::Abs),
        "min" => Some(Builtin::Min),
        "max" => Some(Builtin::Max),
        "floor" => Some(Builtin::Floor),
        "ceil" => Some(Builtin::Ceil),
        "round" => Some(Builtin::Round),
        _ => None,
    }
}

/// Refuse a construct by name. Every rejection in this module goes through
/// here, so the diagnostic always says what was not supported. The wording
/// itself lives in the shared error layer; both engines read it from there.
pub(crate) fn unsupported(what: &str) -> String {
    crate::shared::errors::native_unsupported(what)
}

/// The interpreter's `E110` for a builtin called with the wrong number or kind
/// of arguments, with the wording it uses. Compiled code carries no spans, so
/// the line number is left out, the way the `E036`/`E037`/`E038` panics are.
fn builtin_error(name: &str, detail: &str) -> String {
    crate::shared::errors::builtin_error(None, name, detail)
}

/// `E110` for a builtin that takes exactly one argument.
fn one_arg_error(name: &str, count: usize) -> String {
    builtin_error(name, &crate::shared::errors::one_arg_needed_detail(count))
}

/// `E110` for a builtin that needs a number.
fn number_error(name: &str, kind: Kind) -> String {
    builtin_error(
        name,
        &crate::shared::errors::number_needed_detail(kind.mm_name()),
    )
}

/// One function, typed: the kind of every temporary and binding, and the
/// signature its callers have to match.
#[derive(Debug, Default)]
pub(crate) struct Typed {
    pub(crate) temps: HashMap<Temp, Kind>,
    pub(crate) vars: HashMap<VarId, Kind>,
    /// What callers have told each parameter, while the fixpoint runs.
    learned: Vec<Option<Kind>>,
    /// One settled kind per declared parameter, in order.
    pub(crate) params: Vec<Kind>,
    /// `Some` when the function returns a value, `None` for unit.
    pub(crate) result: Option<Kind>,
}

/// Every function of a program, typed, plus the name of each.
#[derive(Debug, Default)]
pub(crate) struct TypedProgram {
    pub(crate) functions: Vec<Typed>,
    pub(crate) names: Vec<String>,
}

impl TypedProgram {
    /// The index of the function called `name`, if this program defines it.
    pub(crate) fn index_of(&self, name: &str) -> Option<usize> {
        self.names.iter().position(|candidate| candidate == name)
    }
}

/// A name for an instruction, used when it is outside the slice.
fn name_instruction(instruction: &MirInst) -> &'static str {
    match instruction {
        MirInst::LibCall { .. } => "a library call",
        MirInst::Index { .. } => "an index",
        MirInst::Field { .. } => "a field access",
        MirInst::List { .. } => "a list",
        MirInst::Tuple { .. } => "a tuple",
        MirInst::Set { .. } => "a set",
        MirInst::Dict { .. } => "a dict",
        MirInst::IsType { .. } => "a type test",
        MirInst::Items { .. } => "iteration",
        MirInst::Len { .. } => "len()",
        MirInst::StoreIndex { .. } => "a store into an element",
        MirInst::StoreField { .. } => "a store into a field",
        MirInst::Call { .. }
        | MirInst::Binary { .. }
        | MirInst::Compare { .. }
        | MirInst::Move { .. }
        | MirInst::Not { .. }
        | MirInst::Neg { .. }
        | MirInst::StoreVar { .. }
        | MirInst::Eval(_)
        | MirInst::Print(_)
        | MirInst::SetError { .. }
        | MirInst::ErrorMatches { .. }
        | MirInst::BindError { .. }
        | MirInst::ClearError
        | MirInst::CheckError { .. } => "an expression",
    }
}

/// How many arguments one call may pass. Each one travels in a register, so
/// this is the number of registers the calling convention reserves for them.
pub(crate) const MAX_ARGS: usize = 6;

/// Work out the kind of every value in every function, or refuse the program.
///
/// The pass runs to a fixpoint: each round reads what it already knows and
/// learns more, so a call site can tell a callee's parameter what it holds.
/// Nothing is assumed from the order functions are written in, which is why
/// forward references work.
pub(crate) fn analyze(program: &MirProgram) -> Result<TypedProgram, String> {
    if program.functions.is_empty() {
        return Err(unsupported("a program with no main"));
    }
    let names: Vec<String> = program
        .functions
        .iter()
        .map(|function| function.name.clone())
        .collect();
    // Every identity the program raises. A catch clause may only select one
    // of these (or the shared `E178`); a clause for a runtime code such as
    // `E036` could never match here, because a compiled program has no
    // catchable runtime errors -- a panic exits instead.
    let raised: HashSet<&str> = program
        .functions
        .iter()
        .flat_map(|function| &function.blocks)
        .flat_map(|block| &block.insts)
        .filter_map(|instruction| match instruction {
            MirInst::SetError { identity, .. } => Some(identity.as_str()),
            _ => None,
        })
        .collect();
    let functions = program
        .functions
        .iter()
        .map(|function| Typed {
            learned: vec![None; function.params.len()],
            ..Typed::default()
        })
        .collect();
    let mut typed = TypedProgram { functions, names };
    for (index, function) in program.functions.iter().enumerate() {
        if !function.captures.is_empty() {
            return Err(unsupported("a closure (`fn` that captures a variable)"));
        }
        if index == 0 && !function.params.is_empty() {
            return Err(unsupported("a `<main>` with parameters"));
        }
        if function.params.len() > MAX_ARGS {
            return Err(unsupported(&format!(
                "the function `{}` with {} parameters (at most {} are supported)",
                function.name,
                function.params.len(),
                MAX_ARGS
            )));
        }
        for block in &function.blocks {
            for instruction in &block.insts {
                if matches!(
                    instruction,
                    MirInst::LibCall { .. }
                        | MirInst::Index { .. }
                        | MirInst::Field { .. }
                        | MirInst::List { .. }
                        | MirInst::Tuple { .. }
                        | MirInst::Set { .. }
                        | MirInst::Dict { .. }
                        | MirInst::IsType { .. }
                        | MirInst::Items { .. }
                        | MirInst::Len { .. }
                        | MirInst::StoreIndex { .. }
                        | MirInst::StoreField { .. }
                ) {
                    return Err(unsupported(name_instruction(instruction)));
                }
            }
            if index == 0 {
                // The interpreter reports `E108` when the top-level program
                // returns; compiled code would just exit. Refuse the shape
                // instead of building a program that exits differently.
                if let Some(Terminator::Return(_)) = &block.term {
                    return Err(unsupported("a return from the top-level program"));
                }
            }
        }
    }

    loop {
        // `true` once the last round learned something new.
        let mut changed = true;
        let mut rounds = 0;
        while changed {
            changed = false;
            rounds += 1;
            if rounds > 64 {
                // The rules either settle or contradict each other; a loop here
                // would mean a bug, not a program that needs more rounds.
                return Err(unsupported("this program's value kinds do not settle"));
            }
            for (index, function) in program.functions.iter().enumerate() {
                for block in &function.blocks {
                    for instruction in &block.insts {
                        let mut typed_fn = std::mem::take(&mut typed.functions[index]);
                        let learned =
                            learn_instruction(instruction, &mut typed_fn, &mut typed, index);
                        typed.functions[index] = typed_fn;
                        changed |= learned?;
                    }
                    if let Some(Terminator::Return(Some(value))) = &block.term {
                        if let Some(kind) = kind_of(value, &typed.functions[index]) {
                            let result = &mut typed.functions[index];
                            if result.result != Some(kind) {
                                // A function that returns both a value and, on
                                // another path, nothing cannot keep one contract.
                                if result.result.is_some() {
                                    return Err(unsupported(&format!(
                                        "the function `{}` returning values of two kinds",
                                        program.functions[index].name
                                    )));
                                }
                                result.result = Some(kind);
                                changed = true;
                            }
                        }
                    }
                }
            }
        }
        // Nothing else contradicts anything, so whatever is still unlabelled
        // is the default numeric kind: `a + b` with two bare names is the
        // interpreter's int arithmetic unless a float says otherwise. That
        // can unlock more learning, so go round again.
        if !default_numbers(program, &mut typed) {
            break;
        }
    }

    // Every parameter and every value the generated code will read has to have
    // a kind now, and every function has to return on every path the same way.
    for (index, function) in program.functions.iter().enumerate() {
        let typed_fn = &mut typed.functions[index];
        let mut params = Vec::with_capacity(function.params.len());
        for (position, param) in function.params.iter().enumerate() {
            let from_body = typed_fn.vars.get(param).copied();
            let from_callers = typed_fn.learned.get(position).copied().flatten();
            let kind = match (from_body, from_callers) {
                (Some(body), Some(callers)) if body != callers => {
                    return Err(unsupported(&format!(
                        "parameter #{} of `{}` being a {} in its body and a {} at its call sites",
                        position + 1,
                        function.name,
                        body.name(),
                        callers.name()
                    )))
                }
                (Some(kind), _) | (_, Some(kind)) => kind,
                (None, None) if is_read(function, *param) => {
                    return Err(unsupported(&format!(
                        "parameter #{} of `{}` (nothing shows whether it is int, float, bool or string)",
                        position + 1,
                        function.name
                    )))
                }
                // A parameter nothing reads carries no value anyone can see,
                // so one word of any kind will do.
                (None, None) => Kind::Int,
            };
            params.push(kind);
            // Record it as the binding's kind too, so the slot allocator and
            // the reads below see a settled parameter.
            typed_fn.vars.insert(*param, kind);
        }
        typed_fn.params = params;
        typed_fn.learned.clear();
        let mut returns_value = false;
        let mut returns_nothing = false;
        for block in &function.blocks {
            match &block.term {
                Some(Terminator::Return(Some(_))) => returns_value = true,
                Some(Terminator::Return(None)) => returns_nothing = true,
                _ => {}
            }
        }
        if returns_value && returns_nothing {
            return Err(unsupported(&format!(
                "the function `{}` returning a value on one path and nothing on another",
                function.name
            )));
        }
        // A caller that uses the result of a function that never returns one
        // would be reading a value that does not exist.
        if typed_fn.result.is_some() && !returns_value {
            return Err(unsupported(&format!(
                "the result of the function `{}` (it returns nothing)",
                function.name
            )));
        }
        for block in &function.blocks {
            for instruction in &block.insts {
                if let MirInst::ErrorMatches { identity, .. } = instruction {
                    if identity != "E178"
                        && is_error_code(identity)
                        && !raised.contains(identity.as_str())
                    {
                        return Err(unsupported(&format!(
                            "a catch clause for `{}` (a compiled program can only catch an error it raises itself)",
                            identity
                        )));
                    }
                }
                for operand in read_operands(instruction) {
                    // A discarded value may be the unit of a call; nothing
                    // reads it, so its kind does not matter.
                    if matches!(instruction, MirInst::Eval(_)) {
                        continue;
                    }
                    match operand {
                        Operand::Const(Const::Null) => return Err(unsupported("null")),
                        Operand::Const(_) => {}
                        Operand::Temp(temp) => {
                            if !typed_fn.temps.contains_key(temp) {
                                return Err(unsupported(&format!(
                                    "the value in t{} (its kind could not be worked out)",
                                    temp
                                )));
                            }
                        }
                        Operand::Var(var) => {
                            if !typed_fn.vars.contains_key(var) {
                                return Err(unsupported(&format!(
                                    "the binding {} (nothing shows whether it is int, float, bool or string)",
                                    var
                                )));
                            }
                        }
                        Operand::Name(name) => {
                            return Err(unsupported(&format!("the run-time name \"{}\"", name)))
                        }
                        Operand::Fn(_) => return Err(unsupported("a function value")),
                    }
                }
            }
        }
    }
    Ok(typed)
}

/// `E036`-shaped: the interpreter's own codes, which a compiled program never
/// propagates (a panic exits).
fn is_error_code(name: &str) -> bool {
    match name.strip_prefix('E') {
        Some(digits) => digits.len() == 3 && digits.chars().all(|digit| digit.is_ascii_digit()),
        None => false,
    }
}

fn callee_params(program: &TypedProgram, typed: &Typed, callee: usize, index: usize) -> usize {
    if callee == index {
        typed.learned.len()
    } else {
        program.functions[callee].learned.len()
    }
}

fn callee_param(
    program: &TypedProgram,
    typed: &Typed,
    callee: usize,
    index: usize,
    position: usize,
) -> Option<Kind> {
    if callee == index {
        typed.learned.get(position).copied().flatten()
    } else {
        program.functions[callee]
            .learned
            .get(position)
            .copied()
            .flatten()
    }
}

fn set_callee_param(
    program: &mut TypedProgram,
    typed: &mut Typed,
    callee: usize,
    index: usize,
    position: usize,
    kind: Kind,
    name: &str,
) -> Result<bool, String> {
    let slot = if callee == index {
        &mut typed.learned[position]
    } else {
        &mut program.functions[callee].learned[position]
    };
    match *slot {
        Some(existing) if existing != kind => Err(unsupported(&format!(
            "calling `{}` with a {} where its parameter #{} is a {}",
            name,
            kind.name(),
            position + 1,
            existing.name()
        ))),
        Some(_) => Ok(false),
        None => {
            *slot = Some(kind);
            Ok(true)
        }
    }
}

fn callee_result(
    program: &TypedProgram,
    typed: &Typed,
    callee: usize,
    index: usize,
) -> Option<Kind> {
    if callee == index {
        typed.result
    } else {
        program.functions[callee].result
    }
}

fn set_callee_result(
    program: &mut TypedProgram,
    typed: &mut Typed,
    callee: usize,
    index: usize,
    kind: Kind,
    name: &str,
) -> Result<bool, String> {
    let slot = if callee == index {
        &mut typed.result
    } else {
        &mut program.functions[callee].result
    };
    match *slot {
        Some(existing) if existing != kind => Err(unsupported(&format!(
            "`{}` returning a {} but used as a {}",
            name,
            existing.name(),
            kind.name()
        ))),
        Some(_) => Ok(false),
        None => {
            *slot = Some(kind);
            Ok(true)
        }
    }
}

/// Label what is still unconstrained with `Kind::Int`.
///
/// Only reached once the fixpoint has settled, and it never contradicts a kind
/// that is already known, so it cannot make two sources disagree. Returns
/// `true` when it filled something in, which means another round may learn
/// more.
fn default_numbers(program: &MirProgram, typed: &mut TypedProgram) -> bool {
    let mut changed = false;
    // A parameter only its callers describe is still known: move the callers'
    // kind into the body before anything is defaulted, so `x * k` with two
    // such parameters is not mistaken for int arithmetic.
    for (index, function) in program.functions.iter().enumerate() {
        for (position, param) in function.params.iter().enumerate() {
            let from_callers = typed.functions[index]
                .learned
                .get(position)
                .copied()
                .flatten();
            if let Some(kind) = from_callers {
                if !typed.functions[index].vars.contains_key(param) {
                    typed.functions[index].vars.insert(*param, kind);
                    changed = true;
                }
            }
        }
    }
    for (index, function) in program.functions.iter().enumerate() {
        for block in &function.blocks {
            for instruction in &block.insts {
                match instruction {
                    // `/` and `%` settle themselves even from nothing.
                    MirInst::Binary { dest, op, lhs, rhs }
                        if !matches!(op, '/' | '%')
                            && !typed.functions[index].temps.contains_key(dest) =>
                    {
                        let typed_fn = &mut typed.functions[index];
                        let float = [kind_of(lhs, typed_fn), kind_of(rhs, typed_fn)]
                            .into_iter()
                            .any(|kind| kind == Some(Kind::Float));
                        if float {
                            continue;
                        }
                        // The destination is missing here, so recording it is
                        // always news; the operands may or may not be.
                        fill_unknown(lhs, Kind::Int, typed_fn);
                        fill_unknown(rhs, Kind::Int, typed_fn);
                        typed_fn.temps.insert(*dest, Kind::Int);
                        changed = true;
                    }
                    // A builtin's result is never defaulted: its kind is the
                    // kind of its arguments, and defaulting it to int before
                    // a caller has told a parameter it is a float would
                    // contradict that. `learn_builtin` settles it as soon as
                    // the arguments are known.
                    MirInst::Call { dest, name, .. }
                        if !typed.functions[index].temps.contains_key(dest) =>
                    {
                        let result = typed
                            .names
                            .iter()
                            .position(|candidate| candidate == name)
                            .and_then(|callee| typed.functions[callee].result);
                        if let Some(kind) = result {
                            typed.functions[index].temps.insert(*dest, kind);
                            changed = true;
                        }
                    }
                    _ => {}
                }
            }
            // A returned value is never defaulted: a function's result is
            // what its callers read, so guessing int here would contradict a
            // float body that the next round is about to work out.
        }
    }
    changed
}

/// Give `operand` `kind` if nothing has labelled it yet.
fn fill_unknown(operand: &Operand, kind: Kind, typed: &mut Typed) -> bool {
    match operand {
        Operand::Temp(temp) => {
            if typed.temps.contains_key(temp) {
                false
            } else {
                typed.temps.insert(*temp, kind);
                true
            }
        }
        Operand::Var(var) => {
            if typed.vars.contains_key(var) {
                false
            } else {
                typed.vars.insert(*var, kind);
                true
            }
        }
        _ => false,
    }
}

/// Is this binding read anywhere in `function`?
fn is_read(function: &MirFn, var: VarId) -> bool {
    let mut read = false;
    for block in &function.blocks {
        for instruction in &block.insts {
            read |= read_operands(instruction)
                .iter()
                .any(|operand| matches!(operand, Operand::Var(candidate) if *candidate == var));
        }
        if let Some(Terminator::Branch { cond, .. }) = &block.term {
            read |= matches!(cond, Operand::Var(candidate) if *candidate == var);
        }
    }
    read
}

/// One round of learning from one instruction.
fn learn_instruction(
    instruction: &MirInst,
    typed: &mut Typed,
    program: &mut TypedProgram,
    index: usize,
) -> Result<bool, String> {
    let mut changed = false;
    match instruction {
        MirInst::Move { dest, value } => {
            if let Some(kind) = kind_of(value, typed) {
                changed |= learn_temp(&mut typed.temps, *dest, kind)?;
            }
            // The other direction: a destination that is already used as an
            // int says the value moved into it is an int.
            if let Some(kind) = typed.temps.get(dest).copied() {
                changed |= learn_operand(value, kind, typed)?;
            }
        }
        MirInst::Binary { dest, op, lhs, rhs } => {
            if !matches!(op, '+' | '-' | '*' | '%' | '/') {
                return Err(unsupported("that arithmetic operator"));
            }
            let left = kind_of(lhs, typed);
            let right = kind_of(rhs, typed);
            if let Some(kind) = left.or(right) {
                if !kind.is_number() {
                    return Err(unsupported(&format!(
                        "arithmetic on a {} value",
                        kind.name()
                    )));
                }
            }
            if *op == '%' {
                // The interpreter has a float remainder too; the slice keeps
                // only the int one, so the refusal names it instead of
                // quietly becoming an int operation.
                if left == Some(Kind::Float) || right == Some(Kind::Float) {
                    return Err(unsupported("a remainder (`%`) on a float"));
                }
                changed |= learn_operand(lhs, Kind::Int, typed)?;
                changed |= learn_operand(rhs, Kind::Int, typed)?;
                changed |= learn_temp(&mut typed.temps, *dest, Kind::Int)?;
                return Ok(changed);
            }
            // `+ - *` of two ints stays an int, anything with a float in it
            // is a float, and `/` is a float even for two ints (the
            // interpreter divides them as doubles). Mixed operands are left
            // alone: `Int + Float` promotes at run time, so neither side has
            // to be relabelled.
            let known_float = left == Some(Kind::Float) || right == Some(Kind::Float);
            let both_int = left == Some(Kind::Int) && right == Some(Kind::Int);
            if *op == '/' || known_float {
                changed |= learn_temp(&mut typed.temps, *dest, Kind::Float)?;
            } else if both_int {
                changed |= learn_temp(&mut typed.temps, *dest, Kind::Int)?;
            }
        }
        MirInst::Compare { dest, op, lhs, rhs } => {
            if !matches!(op.as_str(), "==" | "!=" | "<" | "<=" | ">" | ">=") {
                return Err(unsupported(&format!("the comparison \"{}\"", op)));
            }
            let ordered = !matches!(op.as_str(), "==" | "!=");
            let left = kind_of(lhs, typed);
            let right = kind_of(rhs, typed);
            let string = left == Some(Kind::Str) || right == Some(Kind::Str);
            if string {
                // Strings compare with strings: byte-wise for equality and
                // lexicographically for an ordering. Mixing a string with any
                // other kind is not a comparison the interpreter has, so both
                // sides are learned as strings and a contradiction is refused.
                changed |= learn_operand(lhs, Kind::Str, typed)?;
                changed |= learn_operand(rhs, Kind::Str, typed)?;
            } else if ordered {
                // Ordering needs two numbers (mixing promotes).
                for kind in [left, right].into_iter().flatten() {
                    if !kind.is_number() {
                        return Err(unsupported(&format!("ordering a {} value", kind.name())));
                    }
                }
            } else if let (Some(left), Some(right)) = (left, right) {
                if left != right && !(left.is_number() && right.is_number()) {
                    return Err(unsupported(&format!(
                        "comparing a {} with a {}",
                        left.name(),
                        right.name()
                    )));
                }
            } else if let Some(kind) = left.or(right) {
                changed |= learn_operand(lhs, kind, typed)?;
                changed |= learn_operand(rhs, kind, typed)?;
            }
            changed |= learn_temp(&mut typed.temps, *dest, Kind::Bool)?;
        }
        MirInst::Not { dest, value } => {
            if let Some(Kind::Str) = kind_of(value, typed) {
                return Err(unsupported("negating a string"));
            }
            changed |= learn_temp(&mut typed.temps, *dest, Kind::Bool)?;
        }
        MirInst::Neg { dest, value } => {
            let held = kind_of(value, typed).or_else(|| typed.temps.get(dest).copied());
            match held {
                Some(kind) if !kind.is_number() => {
                    return Err(unsupported(&format!("negating a {} value", kind.name())))
                }
                Some(kind) => {
                    changed |= learn_operand(value, kind, typed)?;
                    changed |= learn_temp(&mut typed.temps, *dest, kind)?;
                }
                None => {}
            }
        }
        MirInst::StoreVar { var, op, value } => {
            if matches!(op, Some(op) if !matches!(op, '+' | '-' | '*' | '%')) {
                return Err(unsupported("that compound assignment operator"));
            }
            if op.is_some() {
                let held = typed
                    .vars
                    .get(var)
                    .copied()
                    .or_else(|| kind_of(value, typed));
                match held {
                    Some(kind) if !kind.is_number() => {
                        return Err(unsupported(&format!(
                            "that compound assignment on a {}",
                            kind.name()
                        )))
                    }
                    Some(kind) => {
                        // `x += 1` on a float keeps the binding a float.
                        changed |= learn_operand(value, kind, typed)?;
                        changed |= learn_var(&mut typed.vars, *var, kind)?;
                    }
                    None => {}
                }
                return Ok(changed);
            }
            if let Some(kind) = kind_of(value, typed) {
                changed |= learn_var(&mut typed.vars, *var, kind)?;
            }
            if let Some(kind) = typed.vars.get(var).copied() {
                changed |= learn_operand(value, kind, typed)?;
            }
        }
        MirInst::Call { dest, name, args } => {
            let Some(callee) = program.index_of(name) else {
                // No user function of that name, so the interpreter would fall
                // back to a builtin.
                if let Some(builtin) = builtin(name) {
                    return learn_builtin(builtin, name, args, *dest, typed);
                }
                return Err(unsupported(&format!(
                    "a function call to \"{}\" (no function with that name in this file; libraries are outside the slice)",
                    name
                )));
            };
            if args.len() > MAX_ARGS {
                return Err(unsupported(&format!(
                    "a call to `{}` with {} arguments (at most {} are supported)",
                    name,
                    args.len(),
                    MAX_ARGS
                )));
            }
            // A call with the wrong number of arguments cannot run at all (the
            // interpreter reports E033), so it is left to the oracle instead of
            // inventing a different diagnostic here.
            let arity = callee_params(program, typed, callee, index);
            for (position, arg) in args.iter().enumerate() {
                if position >= arity {
                    continue;
                }
                if let Some(kind) = kind_of(arg, typed) {
                    changed |=
                        set_callee_param(program, typed, callee, index, position, kind, name)?;
                }
                // The other direction: a parameter that is already used as an
                // int types the argument passed to it.
                if let Some(kind) = callee_param(program, typed, callee, index, position) {
                    changed |= learn_operand(arg, kind, typed)?;
                }
            }
            if let Some(result) = callee_result(program, typed, callee, index) {
                changed |= learn_temp(&mut typed.temps, *dest, result)?;
            } else if let Some(kind) = typed.temps.get(dest).copied() {
                // What the caller does with the result tells the callee what
                // it returns.
                changed |= set_callee_result(program, typed, callee, index, kind, name)?;
            }
        }
        // A raise's message is what a catch clause binds and what an uncaught
        // raise prints, so it is a string; anything else is refused by name
        // rather than turned into a different message.
        MirInst::SetError { value, .. } => {
            match kind_of(value, typed) {
                Some(Kind::Str) => {}
                Some(other) => {
                    return Err(unsupported(&format!(
                        "a raise whose message is a {}",
                        other.name()
                    )))
                }
                None => {}
            }
            changed |= learn_operand(value, Kind::Str, typed)?;
        }
        MirInst::ErrorMatches { dest, .. } => {
            changed |= learn_temp(&mut typed.temps, *dest, Kind::Bool)?;
        }
        MirInst::BindError { var } => {
            changed |= learn_var(&mut typed.vars, *var, Kind::Str)?;
        }
        MirInst::ClearError | MirInst::CheckError { .. } => {}
        // A discarded value is computed and thrown away; nothing to learn.
        MirInst::Eval(_) => {}
        MirInst::Print(_) => {}
        other => {
            if matches!(
                other,
                MirInst::LibCall { .. }
                    | MirInst::Index { .. }
                    | MirInst::Field { .. }
                    | MirInst::List { .. }
                    | MirInst::Tuple { .. }
                    | MirInst::Set { .. }
                    | MirInst::Dict { .. }
                    | MirInst::IsType { .. }
                    | MirInst::Items { .. }
                    | MirInst::Len { .. }
                    | MirInst::StoreIndex { .. }
                    | MirInst::StoreField { .. }
            ) {
                return Err(unsupported(name_instruction(other)));
            }
        }
    }
    Ok(changed)
}

/// One round of learning from a call to a builtin the slice emits.
fn learn_builtin(
    builtin: Builtin,
    name: &str,
    args: &[Operand],
    dest: Temp,
    typed: &mut Typed,
) -> Result<bool, String> {
    match builtin {
        Builtin::Abs => {
            if args.len() != 1 {
                return Err(one_arg_error(name, args.len()));
            }
            let Some(kind) = kind_of(&args[0], typed) else {
                // Nothing says yet. The kind settles once whatever produces
                // the argument is typed (a caller tells a parameter, say);
                // it is never defaulted here, because a float argument would
                // then contradict that default.
                return Ok(false);
            };
            if !kind.is_number() {
                return Err(number_error(name, kind));
            }
            learn_temp(&mut typed.temps, dest, kind)
        }
        // The rounding builtins take one number and give an int, whatever the
        // argument is: the interpreter converts it to a double, rounds it and
        // casts the result with `as i64`.
        Builtin::Floor | Builtin::Ceil | Builtin::Round => {
            if args.len() != 1 {
                return Err(one_arg_error(name, args.len()));
            }
            if let Some(kind) = kind_of(&args[0], typed) {
                if !kind.is_number() {
                    return Err(number_error(name, kind));
                }
            }
            learn_temp(&mut typed.temps, dest, Kind::Int)
        }
        Builtin::Min | Builtin::Max => {
            if args.is_empty() {
                // The interpreter's wording, which it uses for both names.
                return Err(builtin_error(
                    name,
                    crate::shared::errors::MIN_MAX_NEEDS_VALUE,
                ));
            }
            if args.len() > MAX_ARGS {
                return Err(unsupported(&format!(
                    "`{}` with {} arguments (at most {} are supported)",
                    name,
                    args.len(),
                    MAX_ARGS
                )));
            }
            // All the candidates have to be one numeric kind. The interpreter
            // compares across int and float, but then the result is whichever
            // argument won, so its kind depends on the values; the slice
            // settles kinds statically and refuses instead of guessing.
            let mut kind: Option<Kind> = None;
            for arg in args {
                let Some(known) = kind_of(arg, typed) else {
                    continue;
                };
                match kind {
                    Some(existing) if existing != known => {
                        return Err(unsupported(&format!(
                            "`{}` over a {} and a {} (the slice needs one kind)",
                            name,
                            existing.name(),
                            known.name()
                        )))
                    }
                    _ => kind = Some(known),
                }
            }
            let Some(kind) = kind else {
                return Ok(false);
            };
            if !kind.is_number() {
                return Err(unsupported(&format!(
                    "`{}` over {} values (only numbers are in the slice)",
                    name,
                    kind.name()
                )));
            }
            let mut changed = false;
            for arg in args {
                changed |= learn_operand(arg, kind, typed)?;
            }
            changed |= learn_temp(&mut typed.temps, dest, kind)?;
            Ok(changed)
        }
    }
}

impl Kind {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Kind::Int => "int",
            Kind::Float => "float",
            Kind::Bool => "bool",
            Kind::Str => "string",
        }
    }

    /// The kind's name as the interpreter's `E110` spells it. The text comes
    /// from the shared error layer, so both engines use the same words.
    pub(crate) fn mm_name(self) -> &'static str {
        match self {
            Kind::Int => crate::shared::errors::KIND_INT_MM,
            Kind::Float => crate::shared::errors::KIND_FLOAT_MM,
            Kind::Bool => crate::shared::errors::KIND_BOOL_MM,
            Kind::Str => crate::shared::errors::KIND_STR_MM,
        }
    }
}

fn kind_of(operand: &Operand, typed: &Typed) -> Option<Kind> {
    match operand {
        Operand::Const(Const::Int(_)) => Some(Kind::Int),
        Operand::Const(Const::Float(_)) => Some(Kind::Float),
        Operand::Const(Const::Bool(_)) => Some(Kind::Bool),
        Operand::Const(Const::Str(_)) => Some(Kind::Str),
        Operand::Temp(temp) => typed.temps.get(temp).copied(),
        Operand::Var(var) => typed.vars.get(var).copied(),
        _ => None,
    }
}

/// Record that `operand` holds `kind`, refusing a contradiction.
fn learn_operand(operand: &Operand, kind: Kind, typed: &mut Typed) -> Result<bool, String> {
    match operand {
        Operand::Const(constant) => {
            let actual = match constant {
                Const::Int(_) => Kind::Int,
                Const::Bool(_) => Kind::Bool,
                Const::Str(_) => Kind::Str,
                Const::Float(_) => Kind::Float,
                Const::Null => return Err(unsupported("null")),
            };
            if actual == kind {
                Ok(false)
            } else {
                Err(unsupported(&format!(
                    "using a {} where a {} is needed",
                    actual.name(),
                    kind.name()
                )))
            }
        }
        Operand::Temp(temp) => learn_temp(&mut typed.temps, *temp, kind),
        Operand::Var(var) => learn_var(&mut typed.vars, *var, kind),
        Operand::Name(name) => Err(unsupported(&format!("the run-time name \"{}\"", name))),
        Operand::Fn(_) => Err(unsupported("a function value")),
    }
}

fn learn_temp(map: &mut HashMap<Temp, Kind>, temp: Temp, kind: Kind) -> Result<bool, String> {
    learn(map, temp, kind, |existing, kind| {
        format!(
            "the value in t{} being used as a {} and as a {}",
            existing.0,
            existing.1.name(),
            kind.name()
        )
    })
}

fn learn_var(map: &mut HashMap<VarId, Kind>, var: VarId, kind: Kind) -> Result<bool, String> {
    learn(map, var, kind, |existing, kind| {
        format!(
            "the binding {} being used as a {} and as a {}",
            existing.0,
            existing.1.name(),
            kind.name()
        )
    })
}

/// Shared "learn or confirm" step: a second, different kind is a refusal.
fn learn<K: std::hash::Hash + Eq + Copy>(
    map: &mut HashMap<K, Kind>,
    key: K,
    kind: Kind,
    contradiction: impl Fn((K, Kind), Kind) -> String,
) -> Result<bool, String> {
    match map.get(&key) {
        Some(existing) if *existing == kind => Ok(false),
        Some(existing) => Err(unsupported(&contradiction((key, *existing), kind))),
        None => {
            map.insert(key, kind);
            Ok(true)
        }
    }
}

fn read_operands(instruction: &MirInst) -> Vec<&Operand> {
    match instruction {
        MirInst::Binary { lhs, rhs, .. } | MirInst::Compare { lhs, rhs, .. } => vec![lhs, rhs],
        MirInst::Call { args, .. } => args.iter().collect(),
        MirInst::Not { value, .. }
        | MirInst::Neg { value, .. }
        | MirInst::Move { value, .. }
        | MirInst::StoreVar { value, .. }
        | MirInst::SetError { value, .. } => vec![value],
        MirInst::Print(value) | MirInst::Eval(value) => vec![value],
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::builder::Builder;

    fn function(body: impl FnOnce(&mut Builder)) -> MirFn {
        let mut builder = Builder::new();
        body(&mut builder);
        builder.finish("<main>".to_string(), Vec::new(), Vec::new())
    }

    fn program_of(functions: Vec<MirFn>) -> MirProgram {
        MirProgram { functions }
    }

    #[test]
    fn an_int_and_a_bool_have_their_own_kinds() {
        let function = function(|builder| {
            let int = builder.temp();
            let flag = builder.temp();
            builder.emit(MirInst::Move {
                dest: int,
                value: Operand::Const(Const::Int(3)),
            });
            builder.emit(MirInst::Move {
                dest: flag,
                value: Operand::Const(Const::Bool(true)),
            });
            builder.terminate(Terminator::Exit);
        });
        let typed = analyze(&program_of(vec![function])).expect("kinds");
        let main = &typed.functions[0];
        assert_eq!(main.temps.values().filter(|k| **k == Kind::Int).count(), 1);
        assert_eq!(main.temps.values().filter(|k| **k == Kind::Bool).count(), 1);
    }

    #[test]
    fn a_call_to_a_missing_function_is_refused_by_name() {
        let function = function(|builder| {
            let dest = builder.temp();
            builder.emit(MirInst::Call {
                dest,
                name: "f".to_string(),
                args: Vec::new(),
            });
            builder.terminate(Terminator::Exit);
        });
        let error = analyze(&program_of(vec![function])).expect_err("no such function");
        assert!(error.contains("E177"), "{}", error);
        assert!(error.contains("function call"), "{}", error);
    }

    #[test]
    fn dividing_two_ints_learns_a_float() {
        let function = function(|builder| {
            let dest = builder.temp();
            builder.emit(MirInst::Binary {
                dest,
                op: '/',
                lhs: Operand::Const(Const::Int(7)),
                rhs: Operand::Const(Const::Int(2)),
            });
            builder.terminate(Terminator::Exit);
        });
        let typed = analyze(&program_of(vec![function])).expect("division is in the slice");
        assert!(
            typed.functions[0]
                .temps
                .values()
                .any(|kind| *kind == Kind::Float),
            "the quotient of two ints is a float"
        );
    }

    #[test]
    fn a_remainder_on_a_float_is_refused_with_its_reason() {
        let function = function(|builder| {
            let dest = builder.temp();
            builder.emit(MirInst::Binary {
                dest,
                op: '%',
                lhs: Operand::Const(Const::Float(5.5)),
                rhs: Operand::Const(Const::Int(2)),
            });
            builder.terminate(Terminator::Exit);
        });
        let error = analyze(&program_of(vec![function])).expect_err("float % is out of the slice");
        assert!(error.contains("E177"), "{}", error);
        assert!(error.contains("float"), "{}", error);
    }

    #[test]
    fn comparing_two_strings_is_in_the_slice() {
        let mut dest_id = None;
        let function = function(|builder| {
            let dest = builder.temp();
            dest_id = Some(dest);
            builder.emit(MirInst::Compare {
                dest,
                op: "==".to_string(),
                lhs: Operand::Const(Const::Str("a".to_string())),
                rhs: Operand::Const(Const::Str("b".to_string())),
            });
            builder.terminate(Terminator::Exit);
        });
        let typed =
            analyze(&program_of(vec![function])).expect("string comparison is in the slice");
        assert_eq!(
            typed.functions[0].temps.get(&dest_id.expect("a temporary")),
            Some(&Kind::Bool)
        );
    }

    #[test]
    fn abs_keeps_the_kind_of_its_argument() {
        let function = function(|builder| {
            let int = builder.temp();
            let float = builder.temp();
            builder.emit(MirInst::Call {
                dest: int,
                name: "abs".to_string(),
                args: vec![Operand::Const(Const::Int(-3))],
            });
            builder.emit(MirInst::Call {
                dest: float,
                name: "abs".to_string(),
                args: vec![Operand::Const(Const::Float(-1.5))],
            });
            builder.terminate(Terminator::Exit);
        });
        let typed = analyze(&program_of(vec![function])).expect("abs is in the slice");
        let kinds: Vec<Kind> = typed.functions[0].temps.values().copied().collect();
        assert!(kinds.contains(&Kind::Int), "abs(-3) is an int");
        assert!(kinds.contains(&Kind::Float), "abs(-1.5) is a float");
    }

    #[test]
    fn min_and_max_take_one_numeric_kind() {
        let function = function(|builder| {
            let low = builder.temp();
            let high = builder.temp();
            builder.emit(MirInst::Call {
                dest: low,
                name: "min".to_string(),
                args: vec![
                    Operand::Const(Const::Int(3)),
                    Operand::Const(Const::Int(1)),
                    Operand::Const(Const::Int(2)),
                ],
            });
            builder.emit(MirInst::Call {
                dest: high,
                name: "max".to_string(),
                args: vec![
                    Operand::Const(Const::Float(1.5)),
                    Operand::Const(Const::Float(2.5)),
                ],
            });
            builder.terminate(Terminator::Exit);
        });
        let typed = analyze(&program_of(vec![function])).expect("min/max are in the slice");
        let kinds: Vec<Kind> = typed.functions[0].temps.values().copied().collect();
        assert!(kinds.contains(&Kind::Int), "min of ints is an int");
        assert!(kinds.contains(&Kind::Float), "max of floats is a float");
    }

    #[test]
    fn min_mixing_an_int_and_a_float_is_refused() {
        let function = function(|builder| {
            let dest = builder.temp();
            builder.emit(MirInst::Call {
                dest,
                name: "min".to_string(),
                args: vec![
                    Operand::Const(Const::Int(1)),
                    Operand::Const(Const::Float(2.5)),
                ],
            });
            builder.terminate(Terminator::Exit);
        });
        let error = analyze(&program_of(vec![function])).expect_err("one kind per min");
        assert!(error.contains("E177"), "{}", error);
        assert!(error.contains("min"), "{}", error);
    }

    #[test]
    fn rounding_builtins_give_an_int_for_any_number() {
        let function = function(|builder| {
            let from_float = builder.temp();
            let from_int = builder.temp();
            builder.emit(MirInst::Call {
                dest: from_float,
                name: "floor".to_string(),
                args: vec![Operand::Const(Const::Float(2.5))],
            });
            builder.emit(MirInst::Call {
                dest: from_int,
                name: "ceil".to_string(),
                args: vec![Operand::Const(Const::Int(3))],
            });
            builder.terminate(Terminator::Exit);
        });
        let typed = analyze(&program_of(vec![function])).expect("rounding is in the slice");
        assert_eq!(typed.functions[0].temps.len(), 2);
        assert!(typed.functions[0]
            .temps
            .values()
            .all(|kind| *kind == Kind::Int));
    }

    #[test]
    fn rounding_a_string_is_refused_with_the_interpreters_code() {
        let function = function(|builder| {
            let dest = builder.temp();
            builder.emit(MirInst::Call {
                dest,
                name: "round".to_string(),
                args: vec![Operand::Const(Const::Str("a".to_string()))],
            });
            builder.terminate(Terminator::Exit);
        });
        let error = analyze(&program_of(vec![function])).expect_err("round needs a number");
        assert!(error.starts_with("E110"), "{}", error);
        assert!(error.contains("round"), "{}", error);
        assert!(error.contains("စာသား"), "{}", error);
    }

    #[test]
    fn a_parameter_learns_its_kind_from_the_caller() {
        // fn callee(x) { return x + 1 }  called as callee(2) from main.
        let param = VarId(7);
        let mut callee = Builder::new();
        let x = callee.temp();
        let sum = callee.temp();
        callee.emit(MirInst::Binary {
            dest: sum,
            op: '+',
            lhs: Operand::Var(param),
            rhs: Operand::Const(Const::Int(1)),
        });
        callee.emit(MirInst::Move {
            dest: x,
            value: Operand::Var(param),
        });
        callee.terminate(Terminator::Return(Some(Operand::Temp(sum))));
        let callee = callee.finish("callee".to_string(), vec![param], Vec::new());

        let mut main = Builder::new();
        let dest = main.temp();
        main.emit(MirInst::Call {
            dest,
            name: "callee".to_string(),
            args: vec![Operand::Const(Const::Int(2))],
        });
        main.terminate(Terminator::Exit);
        let main = main.finish("<main>".to_string(), Vec::new(), Vec::new());

        let typed = analyze(&program_of(vec![main, callee])).expect("kinds");
        assert_eq!(typed.functions[1].params, vec![Kind::Int]);
        assert_eq!(typed.functions[1].result, Some(Kind::Int));
    }

    #[test]
    fn a_result_used_by_a_caller_does_not_type_a_void_function() {
        let mut f = Builder::new();
        f.emit(MirInst::Print(Operand::Const(Const::Int(1))));
        f.terminate(Terminator::Return(None));
        let f = f.finish("f".to_string(), Vec::new(), Vec::new());

        let main = function(|builder| {
            let dest = builder.temp();
            builder.emit(MirInst::Call {
                dest,
                name: "f".to_string(),
                args: Vec::new(),
            });
            let sum = builder.temp();
            builder.emit(MirInst::Binary {
                dest: sum,
                op: '+',
                lhs: Operand::Temp(dest),
                rhs: Operand::Const(Const::Int(1)),
            });
            builder.terminate(Terminator::Exit);
        });
        let error = analyze(&program_of(vec![main, f])).expect_err("f returns nothing");
        assert!(error.contains("returns nothing"), "{}", error);
    }

    #[test]
    fn a_parameter_nothing_reveals_is_refused() {
        // fn f(x) { print x } called as f(1) would be fine; here nothing is
        // known because the function is never called and `print` says nothing.
        let param = VarId(3);
        let mut f = Builder::new();
        let x = f.temp();
        f.emit(MirInst::Move {
            dest: x,
            value: Operand::Var(param),
        });
        f.emit(MirInst::Print(Operand::Var(param)));
        f.terminate(Terminator::Return(None));
        let f = f.finish("f".to_string(), vec![param], Vec::new());
        let main = function(|builder| builder.terminate(Terminator::Exit));
        let error = analyze(&program_of(vec![main, f])).expect_err("kind is unknown");
        assert!(error.contains("parameter"), "{}", error);
    }
}
