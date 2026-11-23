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
//! rather than guessed. `%` of two ints is the int remainder and a `%` with a
//! float in it is the float one. An int and a float mix in arithmetic and in ordering
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
//! render into a diagnostic. A collection literal (`list`/`tuple`/`set`/`dict`)
//! is in the slice, with indexing, `len`, printing, writing into an element and
//! iterating, all through the native collection runtime. `push`/`append` and
//! `reverse` are in too: `push` appends one value to the list a variable names
//! (its descriptor is replaced by the runtime's copy, so a second binding keeps
//! its own contents) and answers the new length, while `reverse` answers a
//! fresh list or tuple in the opposite order -- writing it back when the
//! argument is a list variable, exactly as the interpreter's in-place reversal
//! does, and leaving a tuple binding alone. Text reverse, and a `push` whose
//! value does not fit the list's element shape, are refused rather than
//! guessed. Anything else -- a closure, a class -- is refused with `E177`,
//! naming the item. Refusing is what keeps the backend
//! honest: a construct it cannot emit must never become silently different
//! code.
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
use crate::shared::errors::{
    list_needed_detail, push_value_needed_detail, reverse_arg_needed_detail,
    reverse_value_needed_detail, PUSH_NEEDS_LIST_VARIABLE,
};

/// The type of one value in the slice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    Int,
    Float,
    Bool,
    Str,
    List,
    Tuple,
    Set,
    Dict,
}

impl Kind {
    /// Whether this kind is a number, which is what `+ - * /`, ordering
    /// comparisons and `abs`/`min`/`max` accept (mixing the two promotes to
    /// float, except in the builtins, which need one kind).
    fn is_number(self) -> bool {
        matches!(self, Kind::Int | Kind::Float)
    }

    /// Whether this kind is one of the four collection descriptors the
    /// runtime emits.
    fn is_collection(self) -> bool {
        matches!(self, Kind::List | Kind::Tuple | Kind::Set | Kind::Dict)
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
    Len,
    /// `push`/`append`: append one value to a list variable's descriptor and
    /// answer the new length.
    Push,
    /// `reverse`: a fresh descriptor with the elements in the opposite order.
    Reverse,
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
        "len" => Some(Builtin::Len),
        "push" | "append" => Some(Builtin::Push),
        "reverse" => Some(Builtin::Reverse),
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
    /// The element shape of a list/tuple/set (the value shape of a dict),
    /// starting with the element itself: `[Int]` is a list of ints,
    /// `[List, Int]` a list of int lists, `[Dict, Str]` a dict of string
    /// values. Absence means the collection is empty or not settled yet.
    pub(crate) elems: HashMap<Temp, Vec<Kind>>,
    pub(crate) var_elems: HashMap<VarId, Vec<Kind>>,
    /// What callers have told each parameter, while the fixpoint runs.
    learned: Vec<Option<Kind>>,
    /// The element shape callers have told each parameter, while the fixpoint
    /// runs.
    learned_elems: Vec<Option<Vec<Kind>>>,
    /// The binding of each declared parameter, in order.
    pub(crate) param_vars: Vec<VarId>,
    /// One settled kind per declared parameter, in order.
    pub(crate) params: Vec<Kind>,
    /// One settled element shape per declared parameter, in order. `None` when
    /// the parameter is not a collection or its elements are unknown.
    pub(crate) param_elems: Vec<Option<Vec<Kind>>>,
    /// `Some` when the function returns a value, `None` for unit.
    pub(crate) result: Option<Kind>,
    /// The element shape of the returned collection, when the function returns
    /// a collection whose elements are known.
    pub(crate) result_elems: Option<Vec<Kind>>,
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
            learned_elems: vec![None; function.params.len()],
            param_vars: function.params.clone(),
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
                        | MirInst::Field { .. }
                        | MirInst::IsType { .. }
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
                        // A returned collection's element shape is what a
                        // caller reads through the result, so it settles here
                        // the same way a parameter's shape does.
                        if let Some(shape) = elems_of(value, &typed.functions[index]) {
                            let result = &mut typed.functions[index];
                            match result.result_elems.clone() {
                                Some(existing) => match merge_shapes(&existing, &shape) {
                                    Some(merged) if merged.len() > existing.len() => {
                                        result.result_elems = Some(merged);
                                        changed = true;
                                    }
                                    Some(_) => {}
                                    None => {
                                        return Err(unsupported(&format!(
                                            "the function `{}` returning a {} on one path and a {} on another",
                                            program.functions[index].name,
                                            shape_name(&existing),
                                            shape_name(&shape)
                                        )))
                                    }
                                },
                                None => {
                                    result.result_elems = Some(shape);
                                    changed = true;
                                }
                            }
                        }
                    }
                    // A branch condition is the interpreter's `bool` rule:
                    // a collection pointer or a number is E043, not truthy.
                    if let Some(Terminator::Branch { cond, .. }) = &block.term {
                        match kind_of(cond, &typed.functions[index]) {
                            Some(Kind::Bool) => {}
                            Some(other) => {
                                return Err(unsupported(&format!(
                                    "a condition that is a {}",
                                    other.name()
                                )))
                            }
                            None => {
                                let mut typed_fn = std::mem::take(&mut typed.functions[index]);
                                changed |= learn_operand(cond, Kind::Bool, &mut typed_fn)?;
                                typed.functions[index] = typed_fn;
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
        // The element shape of a collection parameter comes from both sides:
        // what the body reads or writes through it, and what its call sites
        // pass. They have to agree, and the settled shape is recorded on the
        // binding so the reads below and the slot allocator see it.
        typed_fn.param_elems.clear();
        for (position, param) in function.params.iter().enumerate() {
            let from_body = typed_fn.var_elems.get(param).cloned();
            let from_callers = typed_fn.learned_elems.get(position).cloned().flatten();
            let shape = match (from_body, from_callers) {
                (Some(body), Some(callers)) => match merge_shapes(&body, &callers) {
                    Some(merged) => Some(merged),
                    None => {
                        return Err(unsupported(&format!(
                        "parameter #{} of `{}` being a {} in its body and a {} at its call sites",
                        position + 1,
                        function.name,
                        shape_name(&body),
                        shape_name(&callers)
                    )))
                    }
                },
                (body, callers) => body.or(callers),
            };
            if let Some(shape) = &shape {
                typed_fn.var_elems.insert(*param, shape.clone());
            }
            typed_fn.param_elems.push(shape);
        }
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

/// The element shape a callee's parameter already has, from its body or from
/// an earlier call site.
fn callee_param_elems(
    program: &TypedProgram,
    typed: &Typed,
    callee: usize,
    index: usize,
    position: usize,
) -> Option<Vec<Kind>> {
    let function = if callee == index {
        Some(typed)
    } else {
        program.functions.get(callee)
    }?;
    let param = function.param_vars.get(position)?;
    function
        .var_elems
        .get(param)
        .cloned()
        .or_else(|| function.learned_elems.get(position).cloned().flatten())
}

fn set_callee_param_elems(
    program: &mut TypedProgram,
    typed: &mut Typed,
    callee: usize,
    index: usize,
    position: usize,
    shape: Vec<Kind>,
    name: &str,
) -> Result<bool, String> {
    let slot = if callee == index {
        &mut typed.learned_elems[position]
    } else {
        &mut program.functions[callee].learned_elems[position]
    };
    if let Some(existing) = slot.clone() {
        match merge_shapes(&existing, &shape) {
            Some(merged) if merged.len() > existing.len() => {
                *slot = Some(merged);
                Ok(true)
            }
            Some(_) => Ok(false),
            None => Err(unsupported(&format!(
                "calling `{}` with a {} where its parameter #{} is a {}",
                name,
                shape_name(&shape),
                position + 1,
                shape_name(&existing)
            ))),
        }
    } else {
        *slot = Some(shape);
        Ok(true)
    }
}

/// The element shape a callee's result already has.
fn callee_result_elems(
    program: &TypedProgram,
    typed: &Typed,
    callee: usize,
    index: usize,
) -> Option<Vec<Kind>> {
    if callee == index {
        typed.result_elems.clone()
    } else {
        program.functions[callee].result_elems.clone()
    }
}

fn set_callee_result_elems(
    program: &mut TypedProgram,
    typed: &mut Typed,
    callee: usize,
    index: usize,
    shape: Vec<Kind>,
    name: &str,
) -> Result<bool, String> {
    let slot = if callee == index {
        &mut typed.result_elems
    } else {
        &mut program.functions[callee].result_elems
    };
    if let Some(existing) = slot.clone() {
        match merge_shapes(&existing, &shape) {
            Some(merged) if merged.len() > existing.len() => {
                *slot = Some(merged);
                Ok(true)
            }
            Some(_) => Ok(false),
            None => Err(unsupported(&format!(
                "`{}` returning a {} on one path and a {} on another",
                name,
                shape_name(&existing),
                shape_name(&shape)
            ))),
        }
    } else {
        *slot = Some(shape);
        Ok(true)
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
            // A collection's element shape travels the same way as its kind:
            // what a caller passes describes the parameter's elements, so the
            // body's reads through them are typed on the next round.
            if let Some(shape) = typed.functions[index]
                .learned_elems
                .get(position)
                .cloned()
                .flatten()
            {
                match typed.functions[index].var_elems.get(param).cloned() {
                    Some(existing) => {
                        if let Some(merged) = merge_shapes(&existing, &shape) {
                            if merged.len() > existing.len() {
                                typed.functions[index].var_elems.insert(*param, merged);
                                changed = true;
                            }
                        }
                    }
                    None => {
                        typed.functions[index].var_elems.insert(*param, shape);
                        changed = true;
                    }
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
                        // A string (or any other non-number) operand settles
                        // the operator itself: `s + t` is concatenation, not
                        // the int default. Skipping here lets the next round
                        // learn the other side instead of contradicting it.
                        let known = [
                            kind_of(lhs, &typed.functions[index]),
                            kind_of(rhs, &typed.functions[index]),
                        ];
                        if known.into_iter().flatten().any(|kind| !kind.is_number()) {
                            continue;
                        }
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
            // `+` of two strings is the interpreter's one string arithmetic:
            // it concatenates. A string next to anything else is the `E040`
            // the interpreter reports, refused here by name.
            if *op == '+' && (left == Some(Kind::Str) || right == Some(Kind::Str)) {
                for kind in [left, right].into_iter().flatten() {
                    if kind != Kind::Str {
                        return Err(unsupported(&format!(
                            "arithmetic on a {} value",
                            kind.name()
                        )));
                    }
                }
                changed |= learn_operand(lhs, Kind::Str, typed)?;
                changed |= learn_operand(rhs, Kind::Str, typed)?;
                changed |= learn_temp(&mut typed.temps, *dest, Kind::Str)?;
                return Ok(changed);
            }
            if let Some(kind) = left.or(right) {
                if !kind.is_number() {
                    return Err(unsupported(&format!(
                        "arithmetic on a {} value",
                        kind.name()
                    )));
                }
            }
            if *op == '%' {
                // The interpreter's remainder follows the promoted kind: two
                // ints stay the Euclidean int one, and anything with a float
                // in it is the float one (its own runtime routine).
                if left == Some(Kind::Float) || right == Some(Kind::Float) {
                    changed |= learn_temp(&mut typed.temps, *dest, Kind::Float)?;
                    return Ok(changed);
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
        MirInst::Compare {
            dest, op, lhs, rhs, ..
        } => {
            if !matches!(op.as_str(), "==" | "!=" | "<" | "<=" | ">" | ">=") {
                return Err(unsupported(&format!("the comparison \"{}\"", op)));
            }
            let ordered = !matches!(op.as_str(), "==" | "!=");
            let left = kind_of(lhs, typed);
            let right = kind_of(rhs, typed);
            if left.into_iter().chain(right).any(Kind::is_collection) {
                // `==`/`!=` compare the printed forms, which the runtime does
                // structurally over the descriptors. Ordering a collection is
                // the interpreter's `E042`; both operands are evaluated first
                // and the runtime prints that line, so it compiles here.
                changed |= learn_temp(&mut typed.temps, *dest, Kind::Bool)?;
                return Ok(changed);
            }
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
            if let Some(kind) = kind_of(value, typed) {
                if kind != Kind::Bool {
                    return Err(unsupported(&format!("negating a {}", kind.name())));
                }
            }
            changed |= learn_operand(value, Kind::Bool, typed)?;
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
            // A collection's element shape travels with the binding, so
            // `ys = xs` followed by `ys[0]` is typed like `xs[0]`.
            if let Some(shape) = elems_of(value, typed) {
                match typed.var_elems.get(var) {
                    Some(existing) if *existing != shape => {
                        return Err(unsupported(&format!(
                            "the binding {} holding {} and {} elements",
                            var,
                            shape_name(existing),
                            shape_name(&shape)
                        )))
                    }
                    Some(_) => {}
                    None => {
                        typed.var_elems.insert(*var, shape);
                        changed = true;
                    }
                }
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
            // A collection's element shape travels across the call in both
            // directions: a caller that passes a list of ints tells the
            // callee's parameter what its elements are, and a callee that
            // returns one tells the caller what it can read through it.
            for (position, arg) in args.iter().enumerate() {
                if position >= arity {
                    continue;
                }
                if let Some(shape) = elems_of(arg, typed) {
                    changed |= set_callee_param_elems(
                        program, typed, callee, index, position, shape, name,
                    )?;
                }
                if let Some(shape) = callee_param_elems(program, typed, callee, index, position) {
                    changed |= refine_element(arg, shape, typed);
                }
            }
            if let Some(shape) = callee_result_elems(program, typed, callee, index) {
                changed |= refine_element(&Operand::Temp(*dest), shape, typed);
            } else if let Some(shape) = elems_of(&Operand::Temp(*dest), typed) {
                changed |= set_callee_result_elems(program, typed, callee, index, shape, name)?;
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
        MirInst::List { dest, items } => {
            let shape = items_shape("list", items, typed)?;
            changed |= learn_collection(*dest, Kind::List, shape, typed)?;
        }
        MirInst::Tuple { dest, items } => {
            let shape = items_shape("tuple", items, typed)?;
            changed |= learn_collection(*dest, Kind::Tuple, shape, typed)?;
        }
        MirInst::Set { dest, items } => {
            // A set drops duplicates, so its members have to be known here:
            // the code generator deduplicates literal members exactly as the
            // interpreter's insertion-ordered set does. A member computed at
            // run time would need the runtime's equality and is refused.
            if !items.iter().all(|item| matches!(item, Operand::Const(_))) {
                return Err(unsupported(
                    "a set built from run-time values (only literal members are in the slice)",
                ));
            }
            let shape = items_shape("set", items, typed)?;
            changed |= learn_collection(*dest, Kind::Set, shape, typed)?;
        }
        MirInst::Dict { dest, pairs } => {
            let mut value_shape: Option<Vec<Kind>> = None;
            for (key, value) in pairs {
                // Only literal keys can be deduplicated at compile time; a
                // run-time key needs the runtime's hash/equality, which is
                // later Step 3 work.
                if !matches!(key, Operand::Const(Const::Str(_))) {
                    return Err(unsupported(
                        "a dict with a key computed at run time (only string literals are in the slice)",
                    ));
                }
                match kind_of(key, typed) {
                    Some(Kind::Str) => {}
                    Some(other) => {
                        return Err(unsupported(&format!(
                            "a dict with a {} key (only string keys are in the slice)",
                            other.name()
                        )))
                    }
                    None => {
                        changed |= learn_operand(key, Kind::Str, typed)?;
                    }
                }
                if let Some(shape) = shape_of(value, typed) {
                    match &value_shape {
                        Some(existing) => match merge_shapes(existing, &shape) {
                            Some(merged) => value_shape = Some(merged),
                            None => {
                                return Err(unsupported(&format!(
                                    "a dict whose values are a {} and a {}",
                                    shape_name(existing),
                                    shape_name(&shape)
                                )))
                            }
                        },
                        None => value_shape = Some(shape),
                    }
                }
            }
            changed |= learn_collection(*dest, Kind::Dict, value_shape, typed)?;
        }
        MirInst::Index {
            dest,
            base,
            indices,
            ..
        } => {
            if indices.len() != 1 {
                return Err(unsupported(
                    "an index with more than one part (a `[row, column]` lookup)",
                ));
            }
            let index = &indices[0];
            match kind_of(base, typed) {
                Some(Kind::List | Kind::Tuple | Kind::Set) => {
                    changed |= learn_operand(index, Kind::Int, typed)?;
                    changed |= learn_indexed(*dest, base, typed)?;
                }
                Some(Kind::Dict) => {
                    changed |= learn_operand(index, Kind::Str, typed)?;
                    changed |= learn_indexed(*dest, base, typed)?;
                }
                Some(Kind::Str) => return Err(unsupported("an index into a string")),
                Some(other) => {
                    return Err(unsupported(&format!("an index into a {}", other.name())))
                }
                None => {}
            }
        }
        MirInst::Len { dest, value } => match kind_of(value, typed) {
            Some(kind) if kind.is_collection() => {
                changed |= learn_temp(&mut typed.temps, *dest, Kind::Int)?;
            }
            Some(Kind::Str) => return Err(unsupported("len() of a string")),
            Some(other) => return Err(unsupported(&format!("len() of a {}", other.name()))),
            None => {}
        },
        MirInst::StoreIndex {
            base,
            indices,
            value,
            ..
        } => {
            if indices.len() != 1 {
                return Err(unsupported(
                    "a store into an element with more than one index (a `[row, column]` write)",
                ));
            }
            // The runtime copies the descriptor with the one element changed
            // and the copy replaces the binding -- the interpreter's value
            // semantics. A temporary has no binding to replace, so only a
            // variable base is in the slice.
            if !matches!(base, Operand::Var(_)) {
                return Err(unsupported(
                    "a store into an element of a temporary (the collection has to be held in a variable)",
                ));
            }
            let index = &indices[0];
            match kind_of(base, typed) {
                Some(Kind::List | Kind::Tuple | Kind::Set) => {
                    changed |= learn_operand(index, Kind::Int, typed)?;
                    changed |= learn_stored(base, value, typed)?;
                }
                Some(Kind::Dict) => {
                    changed |= learn_operand(index, Kind::Str, typed)?;
                    changed |= learn_stored(base, value, typed)?;
                }
                Some(Kind::Str) => return Err(unsupported("a write into a string")),
                Some(other) => {
                    return Err(unsupported(&format!("a store into a {}", other.name())))
                }
                None => {}
            }
        }
        MirInst::Items { dest, value } => match kind_of(value, typed) {
            // A list, tuple or set is already the sequence the loop's
            // `Len`/`Index` read, so the loop walks the collection itself and
            // its element kind travels with it.
            Some(kind @ (Kind::List | Kind::Tuple | Kind::Set)) => {
                changed |= learn_collection(*dest, kind, elems_of(value, typed), typed)?;
            }
            // A dict yields its keys, in the order they were first inserted;
            // the native runtime copies them into a fresh list of strings.
            Some(Kind::Dict) => {
                changed |= learn_collection(*dest, Kind::List, Some(vec![Kind::Str]), typed)?;
            }
            Some(Kind::Str) => return Err(unsupported("iteration over a string")),
            // The interpreter's auto-range: a numeric source walks the ints
            // `0, 1, ...` below it, so the loop variable is always an int.
            Some(Kind::Int | Kind::Float) => {
                changed |= learn_collection(*dest, Kind::List, Some(vec![Kind::Int]), typed)?;
            }
            Some(other) => return Err(unsupported(&format!("iteration over a {}", other.name()))),
            None => {}
        },
        other => {
            if matches!(
                other,
                MirInst::LibCall { .. }
                    | MirInst::Field { .. }
                    | MirInst::IsType { .. }
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
        // `len` over a collection is the descriptor's length word. A string's
        // length is counted in grapheme clusters, which needs the text helper
        // the native runtime does not carry yet, so it is refused.
        Builtin::Len => {
            if args.len() != 1 {
                return Err(one_arg_error(name, args.len()));
            }
            match kind_of(&args[0], typed) {
                Some(kind) if kind.is_collection() => learn_temp(&mut typed.temps, dest, Kind::Int),
                Some(Kind::Str) => Err(unsupported("len() of a string")),
                Some(other) => Err(unsupported(&format!("len() of a {}", other.name()))),
                None => Ok(false),
            }
        }
        // `push`/`append` appends one value to the list the first argument
        // names and answers the new length, exactly as the interpreter does.
        // The argument has to be that variable (a value is a copy, so writing
        // back to it would be invisible), and the appended value has to fit
        // the element shape the list already has.
        Builtin::Push => {
            if args.is_empty() {
                return Err(builtin_error(name, &push_value_needed_detail(0)));
            }
            if args.len() != 2 {
                return Err(builtin_error(
                    name,
                    &push_value_needed_detail(args.len() - 1),
                ));
            }
            if !matches!(args[0], Operand::Var(_)) {
                return Err(builtin_error(name, PUSH_NEEDS_LIST_VARIABLE));
            }
            match kind_of(&args[0], typed) {
                Some(Kind::List) => {
                    // The value joins the element shape, and a value of
                    // another kind is refused by `learn_stored` with the same
                    // wording a store of it would use.
                    let mut changed = learn_stored(&args[0], &args[1], typed)?;
                    changed |= learn_temp(&mut typed.temps, dest, Kind::Int)?;
                    Ok(changed)
                }
                Some(other) => Err(builtin_error(name, &list_needed_detail(other.mm_name()))),
                None => Ok(false),
            }
        }
        // `reverse` answers a list or tuple with its elements in the opposite
        // order. The interpreter reverses a list *variable* in place and any
        // other value as a copy; the code generator writes the fresh
        // descriptor back for the first case and leaves the binding alone for
        // the second, so both match. Text is reversed by character, which
        // needs the Unicode tables the native runtime does not carry.
        Builtin::Reverse => {
            if args.len() != 1 {
                return Err(builtin_error(name, &reverse_arg_needed_detail(args.len())));
            }
            match kind_of(&args[0], typed) {
                Some(kind @ (Kind::List | Kind::Tuple)) => {
                    learn_collection(dest, kind, elems_of(&args[0], typed), typed)
                }
                Some(Kind::Str) => Err(unsupported(
                    "reverse of a string (its characters need the Unicode tables)",
                )),
                Some(other) => Err(builtin_error(
                    name,
                    &reverse_value_needed_detail(other.mm_name()),
                )),
                None => Ok(false),
            }
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
            Kind::List => "list",
            Kind::Tuple => "tuple",
            Kind::Set => "set",
            Kind::Dict => "dict",
        }
    }

    /// The runtime `Value` tag number the code generator writes for this kind
    /// as a collection element. Floats and bools use the descriptor-only tags
    /// because their word decides how to print them.
    pub(crate) fn tag(self) -> u64 {
        use crate::runtime::value::{
            TAG_BOOL, TAG_DICT, TAG_FLOAT, TAG_INT, TAG_LIST, TAG_SET, TAG_STR, TAG_TUPLE,
        };
        match self {
            Kind::Int => TAG_INT,
            Kind::Float => TAG_FLOAT,
            Kind::Bool => TAG_BOOL,
            Kind::Str => TAG_STR,
            Kind::List => TAG_LIST,
            Kind::Tuple => TAG_TUPLE,
            Kind::Set => TAG_SET,
            Kind::Dict => TAG_DICT,
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
            Kind::List => "စာရင်း",
            Kind::Tuple => "အစု",
            Kind::Set => "အုပ်စု",
            Kind::Dict => "အဘိဓာန်",
        }
    }
}

/// The shape of an operand: its kind followed by the element shape when it is
/// a collection. A list of int lists is `[List, Int]`.
fn shape_of(operand: &Operand, typed: &Typed) -> Option<Vec<Kind>> {
    let kind = kind_of(operand, typed)?;
    let mut shape = vec![kind];
    if kind.is_collection() {
        if let Some(rest) = elems_of(operand, typed) {
            shape.extend(rest);
        }
    }
    Some(shape)
}

/// Merge two shapes, allowing one to be a prefix of the other: an empty
/// literal's elements are not known yet, so `[List]` and `[List, Int]` agree.
/// A genuine disagreement (`[List, Int]` and `[List, Str]`) returns `None`.
fn merge_shapes(a: &[Kind], b: &[Kind]) -> Option<Vec<Kind>> {
    let common = a
        .iter()
        .zip(b)
        .take_while(|(left, right)| left == right)
        .count();
    if common < a.len().min(b.len()) {
        None
    } else if a.len() >= b.len() {
        Some(a.to_vec())
    } else {
        Some(b.to_vec())
    }
}

/// A shape as words: `[List, Int]` is `"list of int"`.
fn shape_name(shape: &[Kind]) -> String {
    shape
        .iter()
        .map(|kind| kind.name())
        .collect::<Vec<_>>()
        .join(" of ")
}

/// The common element shape of a sequence literal, or `None` while every
/// element's shape is still unknown. Two different known shapes are refused:
/// the code generator writes one tag per collection and cannot emit a mixed
/// one without the dynamic `Value` representation.
fn items_shape(what: &str, items: &[Operand], typed: &Typed) -> Result<Option<Vec<Kind>>, String> {
    let mut shape: Option<Vec<Kind>> = None;
    for item in items {
        let Some(item_shape) = shape_of(item, typed) else {
            continue;
        };
        match &shape {
            Some(existing) => match merge_shapes(existing, &item_shape) {
                Some(merged) => shape = Some(merged),
                None => {
                    return Err(unsupported(&format!(
                        "a {what} with a {} element and a {} element",
                        shape_name(existing),
                        shape_name(&item_shape)
                    )))
                }
            },
            None => shape = Some(item_shape),
        }
    }
    Ok(shape)
}

/// Record that a collection operand holds elements of `shape`, without
/// contradicting a shape some other instruction already settled.
fn learn_element(operand: &Operand, shape: Vec<Kind>, typed: &mut Typed) {
    match operand {
        Operand::Temp(temp) => {
            typed.elems.entry(*temp).or_insert(shape);
        }
        Operand::Var(var) => {
            typed.var_elems.entry(*var).or_insert(shape);
        }
        _ => {}
    }
}

/// The element shape recorded for a collection operand, if it is known.
fn elems_of(operand: &Operand, typed: &Typed) -> Option<Vec<Kind>> {
    match operand {
        Operand::Temp(temp) => typed.elems.get(temp).cloned(),
        Operand::Var(var) => typed.var_elems.get(var).cloned(),
        _ => None,
    }
}

/// Learn the shape of an operand that is read from: `Option` says whether
/// anything new was settled, and a contradiction is refused by name.
fn learn_shape(operand: &Operand, shape: Vec<Kind>, typed: &mut Typed) -> Result<bool, String> {
    match operand {
        Operand::Temp(temp) => match typed.elems.get(temp) {
            Some(existing) if *existing != shape => Err(unsupported(&format!(
                "the collection in t{} holding {} and {} elements",
                temp,
                shape_name(existing),
                shape_name(&shape)
            ))),
            Some(_) => Ok(false),
            None => {
                typed.elems.insert(*temp, shape);
                Ok(true)
            }
        },
        Operand::Var(var) => match typed.var_elems.get(var) {
            Some(existing) if *existing != shape => Err(unsupported(&format!(
                "the collection in {} holding {} and {} elements",
                var,
                shape_name(existing),
                shape_name(&shape)
            ))),
            Some(_) => Ok(false),
            None => {
                typed.var_elems.insert(*var, shape);
                Ok(true)
            }
        },
        _ => Ok(false),
    }
}

/// One indexing step: the destination gets the first kind of the base's
/// element shape, and the rest of the shape travels with it, so
/// `grid[0][1]` reads an int out of a list of lists.
fn learn_indexed(dest: Temp, base: &Operand, typed: &mut Typed) -> Result<bool, String> {
    let Some(shape) = elems_of(base, typed) else {
        return Ok(false);
    };
    let Some((first, rest)) = shape.split_first() else {
        return Ok(false);
    };
    let mut changed = learn_temp(&mut typed.temps, dest, *first)?;
    if !rest.is_empty() {
        changed |= learn_shape(&Operand::Temp(dest), rest.to_vec(), typed)?;
    }
    Ok(changed)
}

/// Type a value written into an element. When the target's element shape is
/// known the value must match the whole shape (a list of ints cannot take a
/// list of strings at the second level); an empty collection adopts the shape
/// of the value being written, so later reads are typed from it.
fn learn_stored(base: &Operand, value: &Operand, typed: &mut Typed) -> Result<bool, String> {
    let Some(mut target) = elems_of(base, typed) else {
        if let Some(shape) = shape_of(value, typed) {
            learn_element(base, shape, typed);
            return Ok(true);
        }
        return Ok(false);
    };
    if target.len() > 1 {
        if let Some(value_shape) = shape_of(value, typed) {
            match merge_shapes(&target, &value_shape) {
                // A value whose inner shape is known refines a target that
                // only knows the first level (`[]` then `[1]`).
                Some(merged) => target = merged,
                None => {
                    return Err(unsupported(&format!(
                        "a store of a {} into a collection of {}",
                        shape_name(&value_shape),
                        shape_name(&target)
                    )))
                }
            }
        }
    }
    let mut changed = learn_operand(value, target[0], typed)?;
    changed |= refine_element(base, target, typed);
    Ok(changed)
}

/// Tighten a recorded element shape to a longer, compatible one (`[List]`
/// becomes `[List, Int]` once a non-empty inner list is seen).
fn refine_element(operand: &Operand, shape: Vec<Kind>, typed: &mut Typed) -> bool {
    match operand {
        Operand::Temp(temp) => match typed.elems.get(temp) {
            Some(existing) if existing != &shape && shape.starts_with(existing) => {
                typed.elems.insert(*temp, shape);
                true
            }
            Some(_) => false,
            None => {
                typed.elems.insert(*temp, shape);
                true
            }
        },
        Operand::Var(var) => match typed.var_elems.get(var) {
            Some(existing) if existing != &shape && shape.starts_with(existing) => {
                typed.var_elems.insert(*var, shape);
                true
            }
            Some(_) => false,
            None => {
                typed.var_elems.insert(*var, shape);
                true
            }
        },
        _ => false,
    }
}

/// Record that `dest` is a collection of `kind` whose elements are `shape`.
fn learn_collection(
    dest: Temp,
    kind: Kind,
    shape: Option<Vec<Kind>>,
    typed: &mut Typed,
) -> Result<bool, String> {
    if let Some(existing) = typed.temps.get(&dest) {
        if *existing != kind {
            return Err(unsupported(&format!(
                "the value in t{} being used as a {} and as a {}",
                dest,
                existing.name(),
                kind.name()
            )));
        }
    }
    let mut changed = learn_temp(&mut typed.temps, dest, kind)?;
    if let Some(shape) = shape {
        match typed.elems.get(&dest) {
            Some(existing) if *existing != shape => {
                return Err(unsupported(&format!(
                    "the {} in t{} holding {} and {} elements",
                    kind.name(),
                    dest,
                    shape_name(existing),
                    shape_name(&shape)
                )))
            }
            Some(_) => {}
            None => {
                typed.elems.insert(dest, shape);
                changed = true;
            }
        }
    }
    Ok(changed)
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
        MirInst::List { items, .. } | MirInst::Tuple { items, .. } | MirInst::Set { items, .. } => {
            items.iter().collect()
        }
        MirInst::Dict { pairs, .. } => pairs.iter().flat_map(|(key, value)| [key, value]).collect(),
        MirInst::Index { base, indices, .. } => {
            std::iter::once(base).chain(indices.iter()).collect()
        }
        MirInst::Len { value, .. } | MirInst::Items { value, .. } => vec![value],
        MirInst::StoreIndex {
            base,
            indices,
            value,
            ..
        } => std::iter::once(base)
            .chain(indices.iter())
            .chain(std::iter::once(value))
            .collect(),
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
    fn a_remainder_follows_the_promoted_kind() {
        let mut mixed = None;
        let mut ints = None;
        let function = function(|builder| {
            let dest = builder.temp();
            mixed = Some(dest);
            builder.emit(MirInst::Binary {
                dest,
                op: '%',
                lhs: Operand::Const(Const::Float(5.5)),
                rhs: Operand::Const(Const::Int(2)),
            });
            let dest = builder.temp();
            ints = Some(dest);
            builder.emit(MirInst::Binary {
                dest,
                op: '%',
                lhs: Operand::Const(Const::Int(7)),
                rhs: Operand::Const(Const::Int(2)),
            });
            builder.terminate(Terminator::Exit);
        });
        let typed = analyze(&program_of(vec![function])).expect("remainders are in the slice");
        let temps = &typed.functions[0].temps;
        assert_eq!(
            temps.get(&mixed.expect("a temporary")),
            Some(&Kind::Float),
            "a remainder with a float operand is a float"
        );
        assert_eq!(
            temps.get(&ints.expect("a temporary")),
            Some(&Kind::Int),
            "two ints keep the int remainder"
        );
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
                line: 1,
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

    #[test]
    fn a_list_index_is_typed_from_its_members() {
        let mut list_id = None;
        let mut item_id = None;
        let function = function(|builder| {
            let list = builder.temp();
            list_id = Some(list);
            builder.emit(MirInst::List {
                dest: list,
                items: vec![Operand::Const(Const::Int(1)), Operand::Const(Const::Int(2))],
            });
            let index = builder.temp();
            builder.emit(MirInst::Move {
                dest: index,
                value: Operand::Const(Const::Int(0)),
            });
            let item = builder.temp();
            item_id = Some(item);
            builder.emit(MirInst::Index {
                dest: item,
                base: Operand::Temp(list),
                indices: vec![Operand::Temp(index)],
                line: 1,
            });
            builder.emit(MirInst::Print(Operand::Temp(item)));
            builder.terminate(Terminator::Exit);
        });
        let typed = analyze(&program_of(vec![function])).expect("collections are in the slice");
        let main = &typed.functions[0];
        assert_eq!(main.temps.get(&list_id.expect("a list")), Some(&Kind::List));
        assert_eq!(
            main.elems.get(&list_id.expect("a list")),
            Some(&vec![Kind::Int])
        );
        assert_eq!(main.temps.get(&item_id.expect("an item")), Some(&Kind::Int));
    }

    #[test]
    fn a_nested_index_is_typed_from_the_inner_elements() {
        let mut row_id = None;
        let mut cell_id = None;
        let function = function(|builder| {
            let inner = builder.temp();
            builder.emit(MirInst::List {
                dest: inner,
                items: vec![Operand::Const(Const::Int(1)), Operand::Const(Const::Int(2))],
            });
            let grid = builder.temp();
            builder.emit(MirInst::List {
                dest: grid,
                items: vec![Operand::Temp(inner)],
            });
            // `grid[0]` is a list of ints; `grid[0][1]` is an int.
            let row = builder.temp();
            row_id = Some(row);
            builder.emit(MirInst::Index {
                dest: row,
                base: Operand::Temp(grid),
                indices: vec![Operand::Const(Const::Int(0))],
                line: 3,
            });
            let cell = builder.temp();
            cell_id = Some(cell);
            builder.emit(MirInst::Index {
                dest: cell,
                base: Operand::Temp(row),
                indices: vec![Operand::Const(Const::Int(1))],
                line: 3,
            });
            builder.emit(MirInst::Print(Operand::Temp(cell)));
            builder.terminate(Terminator::Exit);
        });
        let typed = analyze(&program_of(vec![function])).expect("nested indexing is in the slice");
        let main = &typed.functions[0];
        let row = row_id.expect("a row");
        assert_eq!(main.temps.get(&row).copied(), Some(Kind::List));
        assert_eq!(main.elems.get(&row), Some(&vec![Kind::Int]));
        assert_eq!(
            main.temps.get(&cell_id.expect("a cell")).copied(),
            Some(Kind::Int)
        );
    }

    #[test]
    fn a_dict_loop_walks_its_keys_as_strings() {
        let mut items_id = None;
        let function = function(|builder| {
            let dict = builder.temp();
            builder.emit(MirInst::Dict {
                dest: dict,
                pairs: vec![(
                    Operand::Const(Const::Str("a".to_string())),
                    Operand::Const(Const::Int(1)),
                )],
            });
            let items = builder.temp();
            items_id = Some(items);
            builder.emit(MirInst::Items {
                dest: items,
                value: Operand::Temp(dict),
            });
            let key = builder.temp();
            builder.emit(MirInst::Index {
                dest: key,
                base: Operand::Temp(items),
                indices: vec![Operand::Const(Const::Int(0))],
                line: 1,
            });
            builder.emit(MirInst::Print(Operand::Temp(key)));
            builder.terminate(Terminator::Exit);
        });
        let typed = analyze(&program_of(vec![function])).expect("iteration is in the slice");
        let main = &typed.functions[0];
        let items = items_id.expect("the items temporary");
        assert_eq!(main.temps.get(&items).copied(), Some(Kind::List));
        assert_eq!(main.elems.get(&items).cloned(), Some(vec![Kind::Str]));
    }

    /// `+` of two strings is the interpreter's one string arithmetic and
    /// gives a string; a string beside a number stays refused.
    #[test]
    fn string_concatenation_types_as_a_string() {
        let mut dest_id = None;
        let function = function(|builder| {
            let dest = builder.temp();
            dest_id = Some(dest);
            builder.emit(MirInst::Binary {
                dest,
                op: '+',
                lhs: Operand::Const(Const::Str("မြ".to_string())),
                rhs: Operand::Const(Const::Str("န်မာ".to_string())),
            });
            builder.emit(MirInst::Print(Operand::Temp(dest)));
            builder.terminate(Terminator::Exit);
        });
        let typed = analyze(&program_of(vec![function])).expect("string `+` is in the slice");
        assert_eq!(
            typed.functions[0]
                .temps
                .get(&dest_id.expect("the join"))
                .copied(),
            Some(Kind::Str)
        );
    }

    /// Mixing a string with a number has no arithmetic in the language; the
    /// slice refuses it instead of joining their printed forms.
    #[test]
    fn joining_a_string_with_a_number_is_refused() {
        let function = function(|builder| {
            let dest = builder.temp();
            builder.emit(MirInst::Binary {
                dest,
                op: '+',
                lhs: Operand::Const(Const::Str("a".to_string())),
                rhs: Operand::Const(Const::Int(1)),
            });
            builder.emit(MirInst::Print(Operand::Temp(dest)));
            builder.terminate(Terminator::Exit);
        });
        let error = analyze(&program_of(vec![function])).expect_err("a string and a number");
        assert!(error.contains("E177"), "{error}");
        assert!(error.contains("arithmetic on a"), "{error}");
    }

    /// The interpreter's auto-range binds an `int` counter whatever the
    /// source's kind is, so both an int and a float bound walk `[Int]`.
    #[test]
    fn a_number_loop_walks_ints_from_zero() {
        for (kind, value) in [
            (Kind::Int, Operand::Const(Const::Int(5))),
            (Kind::Float, Operand::Const(Const::Float(2.5))),
        ] {
            let mut items_id = None;
            let function = function(|builder| {
                let items = builder.temp();
                items_id = Some(items);
                builder.emit(MirInst::Items { dest: items, value });
                let item = builder.temp();
                builder.emit(MirInst::Index {
                    dest: item,
                    base: Operand::Temp(items),
                    indices: vec![Operand::Const(Const::Int(0))],
                    line: 1,
                });
                builder.emit(MirInst::Print(Operand::Temp(item)));
                builder.terminate(Terminator::Exit);
            });
            let typed = analyze(&program_of(vec![function]))
                .unwrap_or_else(|error| panic!("{kind:?}: {error}"));
            let main = &typed.functions[0];
            let items = items_id.expect("the items temporary");
            assert_eq!(
                main.temps.get(&items).copied(),
                Some(Kind::List),
                "{kind:?}"
            );
            assert_eq!(
                main.elems.get(&items).cloned(),
                Some(vec![Kind::Int]),
                "{kind:?}"
            );
        }
    }

    #[test]
    fn a_store_into_a_list_is_typed_from_its_elements() {
        let function = function(|builder| {
            let var = VarId(0);
            let list = builder.temp();
            builder.emit(MirInst::List {
                dest: list,
                items: vec![Operand::Const(Const::Int(1))],
            });
            builder.emit(MirInst::StoreVar {
                var,
                op: None,
                value: Operand::Temp(list),
            });
            builder.emit(MirInst::StoreIndex {
                base: Operand::Var(var),
                indices: vec![Operand::Const(Const::Int(0))],
                value: Operand::Const(Const::Int(9)),
                line: 3,
            });
            let read = builder.temp();
            builder.emit(MirInst::Index {
                dest: read,
                base: Operand::Var(var),
                indices: vec![Operand::Const(Const::Int(0))],
                line: 4,
            });
            builder.emit(MirInst::Print(Operand::Temp(read)));
            builder.terminate(Terminator::Exit);
        });
        let typed = analyze(&program_of(vec![function])).expect("a store is in the slice");
        let main = &typed.functions[0];
        assert_eq!(main.vars.get(&VarId(0)), Some(&Kind::List));
        assert_eq!(main.var_elems.get(&VarId(0)), Some(&vec![Kind::Int]));
    }

    #[test]
    fn a_store_into_a_string_is_refused_by_name() {
        let function = function(|builder| {
            let var = VarId(0);
            builder.emit(MirInst::StoreVar {
                var,
                op: None,
                value: Operand::Const(Const::Str("text".to_string())),
            });
            builder.emit(MirInst::StoreIndex {
                base: Operand::Var(var),
                indices: vec![Operand::Const(Const::Int(0))],
                value: Operand::Const(Const::Str("x".to_string())),
                line: 2,
            });
            builder.terminate(Terminator::Exit);
        });
        let error = analyze(&program_of(vec![function])).expect_err("text is fixed");
        assert!(error.contains("E177"), "{}", error);
        assert!(error.contains("write into a string"), "{}", error);
    }

    #[test]
    fn comparing_two_lists_is_typed_as_a_bool() {
        let mut flag_id = None;
        let function = function(|builder| {
            let left = builder.temp();
            let right = builder.temp();
            builder.emit(MirInst::List {
                dest: left,
                items: vec![Operand::Const(Const::Int(1))],
            });
            builder.emit(MirInst::List {
                dest: right,
                items: vec![Operand::Const(Const::Int(1))],
            });
            let flag = builder.temp();
            flag_id = Some(flag);
            builder.emit(MirInst::Compare {
                dest: flag,
                op: "==".to_string(),
                lhs: Operand::Temp(left),
                rhs: Operand::Temp(right),
                line: 3,
            });
            builder.emit(MirInst::Print(Operand::Temp(flag)));
            builder.terminate(Terminator::Exit);
        });
        let typed = analyze(&program_of(vec![function])).expect("equality is in the slice");
        let flag = flag_id.expect("the comparison's temporary");
        assert_eq!(
            typed.functions[0].temps.get(&flag).copied(),
            Some(Kind::Bool)
        );
    }

    #[test]
    fn ordering_two_lists_compiles_to_the_interpreters_error() {
        let function = function(|builder| {
            let left = builder.temp();
            let right = builder.temp();
            builder.emit(MirInst::List {
                dest: left,
                items: vec![Operand::Const(Const::Int(1))],
            });
            builder.emit(MirInst::List {
                dest: right,
                items: vec![Operand::Const(Const::Int(2))],
            });
            let flag = builder.temp();
            builder.emit(MirInst::Compare {
                dest: flag,
                op: "<".to_string(),
                lhs: Operand::Temp(left),
                rhs: Operand::Temp(right),
                line: 3,
            });
            builder.emit(MirInst::Print(Operand::Temp(flag)));
            builder.terminate(Terminator::Exit);
        });
        // Ordering collections is E042 at run time, not a construct outside
        // the slice, so the analyzer types the comparison and codegen emits
        // the interpreter's panic with this line.
        let typed = analyze(&program_of(vec![function])).expect("E042 is the interpreter's rule");
        assert_eq!(
            typed.functions[0]
                .temps
                .values()
                .filter(|kind| **kind == Kind::Bool)
                .count(),
            1
        );
    }

    #[test]
    fn a_list_with_two_element_kinds_is_refused() {
        let function = function(|builder| {
            let list = builder.temp();
            builder.emit(MirInst::List {
                dest: list,
                items: vec![
                    Operand::Const(Const::Int(1)),
                    Operand::Const(Const::Str("two".to_string())),
                ],
            });
            builder.emit(MirInst::Print(Operand::Temp(list)));
            builder.terminate(Terminator::Exit);
        });
        let error = analyze(&program_of(vec![function])).expect_err("mixed elements");
        assert!(error.contains("E177"), "{}", error);
        assert!(error.contains("list"), "{}", error);
    }

    #[test]
    fn a_set_built_from_a_run_time_member_is_refused() {
        let function = function(|builder| {
            let member = builder.temp();
            builder.emit(MirInst::Move {
                dest: member,
                value: Operand::Const(Const::Int(2)),
            });
            let set = builder.temp();
            builder.emit(MirInst::Set {
                dest: set,
                items: vec![Operand::Const(Const::Int(1)), Operand::Temp(member)],
            });
            builder.emit(MirInst::Print(Operand::Temp(set)));
            builder.terminate(Terminator::Exit);
        });
        let error = analyze(&program_of(vec![function])).expect_err("run-time set member");
        assert!(error.contains("E177"), "{}", error);
        assert!(
            error.contains("set built from run-time values"),
            "{}",
            error
        );
    }

    #[test]
    fn a_collection_shape_crosses_a_call_in_both_directions() {
        // fn first(xs) { return xs[0] }  called as first([1, 2]) from main.
        let param = VarId(9);
        let mut callee = Builder::new();
        let first = callee.temp();
        callee.emit(MirInst::Index {
            dest: first,
            base: Operand::Var(param),
            indices: vec![Operand::Const(Const::Int(0))],
            line: 1,
        });
        callee.terminate(Terminator::Return(Some(Operand::Temp(first))));
        let callee = callee.finish("first".to_string(), vec![param], Vec::new());

        let mut main = Builder::new();
        let list = main.temp();
        main.emit(MirInst::List {
            dest: list,
            items: vec![Operand::Const(Const::Int(1)), Operand::Const(Const::Int(2))],
        });
        let dest = main.temp();
        main.emit(MirInst::Call {
            dest,
            name: "first".to_string(),
            args: vec![Operand::Temp(list)],
        });
        main.emit(MirInst::Print(Operand::Temp(dest)));
        main.terminate(Terminator::Exit);
        let main = main.finish("<main>".to_string(), Vec::new(), Vec::new());

        let typed = analyze(&program_of(vec![main, callee])).expect("shapes cross calls");
        // The caller's list describes the parameter, and the read through it
        // types both the scalar result and the call's destination.
        assert_eq!(typed.functions[1].param_elems, vec![Some(vec![Kind::Int])]);
        assert_eq!(typed.functions[1].result, Some(Kind::Int));
        assert_eq!(typed.functions[1].result_elems, None);
        assert_eq!(typed.functions[0].temps.get(&dest), Some(&Kind::Int));
    }
}
