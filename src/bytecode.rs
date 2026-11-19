//! A real bytecode compiler and virtual machine.
//!
//! The interpreter walks the parsed statements directly. This module is the
//! other half: [`compile`] turns the same [`Stmt`] tree into a flat list of
//! [`Instr`] and [`Vm`] executes that list on a stack machine, so a program
//! runs without re-walking its syntax tree. `akk --vm <file>` selects it.
//!
//! The instruction set is deliberately small:
//!
//! * values are pushed and popped on one stack;
//! * `Jump`/`JumpIfFalse` carry absolute indices, so a loop is a backward jump
//!   and an if-chain is a run of forward jumps;
//! * every user function is compiled once into its own [`FuncDef`] and
//!   reached with `Call`, which starts a fresh local frame.
//!
//! Dictionaries are compiled too: a literal builds a value on the stack, and
//! reads, writes, membership and iteration reuse the interpreter's own value
//! helpers, so printing and the missing-key error (`E053`) are identical in
//! both backends.
//!
//! `try`/`catch`/`finally` compile to handler instructions and a stack unwind,
//! and `loop { ... break <value>; }` compiles to a loop that keeps the break
//! value in a temporary slot -- both with the unwinding and the ordering the
//! interpreter uses.
//!
//! Builtin calls compile to one `Builtin` instruction each, resolved by name
//! at compile time and run by the interpreter's own value-based builtin code,
//! so results and error messages are shared rather than reimplemented.
//!
//! Builtin libraries whose functions only read their argument values
//! (`ကျပန်း`, `အချိန်`, `json`, `file`) import and call through
//! `UseLib`/`RequireLib`/`LibCall`, again running the interpreter's own
//! dispatch. Their failures keep the interpreter's codes and ordering: `E089`
//! before the arguments are evaluated, `E086`/`E087` before them too,
//! `E117`/`E088` after the call.
//!
//! The mutating list builtins (`push`, `pop`, `sort`, `insert`, `remove`,
//! `append`) compile to `PinGuard`/`Load`/`ListArg`/`Mutate`: the list variable
//! is read, checked and written back, and the call's result is the value the
//! interpreter returns. Default parameters are a `ParamMissing` prologue in
//! the callee's own body, so each default sees the parameters before it.
//!
//! The callback builtins `map`, `filter` and `reduce` compile to `IterItems`
//! (the value turned into the items they walk, with the interpreter's `E110`)
//! followed by `HigherOrder`, which calls each item's callback. A lambda in the
//! callback position is compiled into a function body of its own, exactly as a
//! named function is, so both go through the same `E033`/`E114` checks.
//!
//! Lambdas are first-class values: `fn(x) { ... }` compiles to a [`FuncDef`]
//! and `MakeFunc` pushes it, so a lambda can be stored, passed, returned and
//! called through a variable (`CallableGuard` then `CallDyn`). Scopes are
//! dynamic, as the interpreter's are: `load` walks the frame stack, so a
//! callee reads its caller's locals, and a parameter with no previous binding
//! stays visible after the call -- the interpreter's own rule.
//!
//! Whatever the compiler cannot express yet -- the windows/server library,
//! script/package libraries, a library function used as a value -- is refused
//! with `E170` at compile time rather than silently behaving differently from
//! the interpreter.

use std::collections::HashMap;
use std::rc::Rc;

use indexmap::{IndexMap, IndexSet};

use crate::interpreter::{value_contains, value_eq, Callable, Interpreter, Key, ObjectValue, Value};
use crate::parser::{CondAtom, CondChain, Expr, ForSource, IfBranch, LogicalOp, Stmt};

/// One instruction. Every jump holds an absolute index into its own chunk
/// (the main code or one function body).
#[derive(Debug, Clone)]
pub enum Instr {
    Const(Value),
    Load(String, u32),
    Store(String),
    Pop,
    /// Pop a list or tuple and bind one name per item, in order:
    /// `[a, b] = <value>;`. The line is carried so a non-collection (`E154`)
    /// and a count mismatch (`E155`) point at the statement, as they do in the
    /// interpreter, and a `pin` name is refused (`E104`).
    Destructure(Vec<String>, u32),
    Add,
    Sub,
    Mul,
    /// Divide: integers produce a float, as the interpreter's `/` does. The
    /// line is carried so division by zero reports the interpreter's `E036`
    /// (integers) or `E037` (floats) at the line written.
    Div(u32),
    /// Remainder, Euclidean as the interpreter computes it. The line is
    /// carried for `E038`/`E039`.
    Mod(u32),
    Neg,
    /// Logical not. The line is carried so a non-bool operand reports the
    /// interpreter's `E043` at the line the operator was written on.
    Not(u32),
    And,
    Or,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    /// Pop a bool; jump when it is true.
    JumpIfTrue(usize),
    /// Pop a bool; jump when it is false.
    JumpIfFalse(usize),
    Jump(usize),
    /// Pop a value and print it, exactly as `... ကို ဖော်ပြပါ။` does.
    Print,
    /// Replace the top value with its length.
    Len,
    /// Call a user function: its index in [`Program::funcs`] and how many
    /// arguments were pushed.
    Call(usize, u8),
    /// Return from the current function, popping its value (or `null`). The
    /// line is carried so a `return` outside any function fails with the
    /// interpreter's `E108` instead of quietly ending the program.
    Return(u32),
    /// `container[k1]...[kn]` -> element: how many index expressions were
    /// written and the line they sit on, so a missing dictionary key reports
    /// `E053` and a bad index `E048`/`E049` exactly as the interpreter does.
    Index(u32, u8),
    /// Pop `count` key/value pairs and build a dictionary, keeping insertion
    /// order and a repeated key's first position.
    Dict(usize),
    /// `<needle> in <haystack>` / `<haystack> ထဲ <needle>`.
    Contains(u32),
    /// Pop a value and push what a `for` loop walks: a dictionary's keys, in
    /// insertion order; anything else unchanged.
    IterKeys,
    /// `<name>[k...] = value`: pop the value, the keys and the container,
    /// write through, and store the container back under `name`.
    IndexStore(String, u32, u8),
    /// Pop `count` values and build a set: insertion order, a repeated value
    /// kept once, at its first position.
    Set(usize),
    /// Pop `argc` arguments and construct the class at that slot: its
    /// constructor runs in a fresh frame and the object is the result. The
    /// line is carried for `E034`.
    NewObj(usize, u8, u32),
    /// `အတု <field> သည် <value> ဖြစ်၏။`: pop a value and write it to the field
    /// of the object under construction. Line carried for `E035`.
    SelfSet(String, u32),
    /// `<object>.<field>` / `<object> ၏ <field>`, with the line for `E082`.
    Field(String, u32),
    /// `<name>.<field>` written without a call. When `name` is a live variable
    /// this reads that object's field, exactly as the interpreter does; a name
    /// that is not a variable is a library reference, which the compiler does
    /// not support -- so it fails with the interpreter's `E085`.
    FieldRef(String, String, u32),
    /// Pop a value and show it as an input prompt: its text, no newline,
    /// flushed, exactly as `... ကို မေးပါ။` does before it blocks.
    Prompt,
    /// Read one line from stdin and push it as a value, inferring numbers and
    /// booleans the way `မေးပါ` does.
    Read,
    /// Read one line and coerce it to the declared record type
    /// (`int`/`float`/`bool`/`str`), refusing a bad value with `E103`.
    ReadTyped(String, u32),
    /// `<name> :<type> = <value>;` / `pin <name> :<type> = <value>;`: pop the
    /// value, check it against the declared type (`E103`), record a `pin`
    /// name so a later write is `E104`, and refuse re-declaring a `pin` name
    /// that already exists (`E105`).
    Declare(String, String, bool, u32),
    /// Refuse a write to a `pin` constant (`E104`) *before* the right-hand
    /// side runs, which is where the interpreter refuses it.
    PinGuard(String, u32),
    /// Expand the `{name}` placeholders of the string on top of the stack, the
    /// way the interpreter expands a string literal when it is evaluated.
    /// The line is carried for `E056`/`E058`/`E060`.
    Interpolate(u32),
    /// Stop the run with this message. Used where the interpreter's failure has
    /// no printed value to compare -- a `break` with no loop to stop (`E116`),
    /// for instance -- so the message is the interpreter's own text.
    Abort(String),
    /// Start a protected region: from here to the matching [`Instr::TryEnd`] an
    /// error jumps to `handler` instead of stopping the run. The handler pc is
    /// patched once the layout is known.
    Try(usize),
    /// End of a protected region on the normal path: the handler is dropped.
    TryEnd,
    /// Push the code (`E104`) of the error a handler is running for.
    ErrorCode,
    /// Re-raise the error the current handler is running for.
    ReThrow,
    /// Drop `count` handlers: how a `break`/`continue`/`return` leaves the
    /// `try` blocks it sits inside.
    PopHandlers(u8),
    /// Mark `name` as a constant (`pin`), without writing a value to it: a
    /// `pin X :<type> = loop { ... }` seeds and declares the name before the
    /// loop, so only the constant mark is left for the end.
    MarkConst(String),
    /// `E105` when `name` already exists, without declaring anything: the
    /// up-front half of a `pin` loop value, which must refuse a redeclaration
    /// before the loop body runs.
    PinDeclareCheck(String, u32),
    /// Call the builtin `name` with `argc` evaluated arguments from the stack.
    /// The name is resolved when the call is compiled, and the work is done by
    /// the interpreter's own value-based builtin code, so messages and results
    /// cannot drift between the backends. `line` points `E111` and the
    /// wrong-argument errors at the call.
    Builtin(String, u8, u32),
    /// Import the builtin library `name` (every spelling it answers to), the
    /// way `နည်းပညာများ ... ကို သုံးမည်။` does when it runs.
    UseLib(String),
    /// `E089` unless the library `name` has been imported: compiled *before*
    /// a call's arguments, matching the interpreter's order.
    RequireLib(String, u32),
    /// Call the library function `lib.fn` with `argc` evaluated arguments.
    /// `err` is the message to raise when the call produces no value or when
    /// the value is required (empty: put `null` on the stack instead).
    LibCall(String, String, u8, u32, String),
    /// Return with a value: the function's last expression, its last
    /// declaration, or an explicit `return <expr>;`.
    ReturnValue(u32),
    /// Return without one: the function's body ended on a statement that yields
    /// nothing, or on a bare `return;`. A caller that demanded a value turns
    /// this into `E071`.
    ReturnVoid(u32),
    /// Call the user function `index`, storing its result. `err` is the `E071`
    /// message to raise when the body returns nothing.
    CallValue(usize, u8, u32, String),
    /// Turn the collection on top of the stack into a list of the items
    /// `map`/`filter`/`reduce` walk: a list or tuple as it is, a set's and a
    /// dictionary's values in insertion order, a string's clusters. Anything
    /// else is the interpreter's `E110`, raised here -- before `reduce`'s
    /// starting value is evaluated, which is the interpreter's order.
    /// `kind` selects the builtin named in that message (0 map, 1 filter,
    /// 2 reduce).
    IterItems(u8, u32),
    /// `map`/`filter`/`reduce`: the items list (pushed by `IterItems`), the
    /// callback value, and for `reduce` its starting value are on the stack.
    /// The callback is an ordinary callable value, so a lambda written there,
    /// a named function and a variable all work -- the interpreter resolves
    /// the same three.
    HigherOrder(u8, u32),
    /// Check that the value on top of the stack is a list, before a mutating
    /// builtin's other arguments run. `E110` names the builtin.
    ListArg(String, u32),
    /// `push`/`append`/`pop`/`insert`/`remove`/`sort` on the variable `var`:
    /// the builtin's name, the variable, the argument count and the line. The
    /// list and the extra arguments are on the stack; the updated list is
    /// written back through the variable and the result is pushed.
    Mutate(String, String, u8, u32),
    /// Fill the parameter `name` from its default when the call left it off:
    /// jump to the given index when the frame already has it.
    ParamMissing(String, usize),
    /// Pop `count` values and build a list, keeping their order.
    List(usize),
    /// Pop `count` values and build a tuple.
    Tuple(usize),
    /// Push the compiled function `index` as a first-class value, so it can
    /// be stored, passed and returned like any other value. Calling it later
    /// creates the same fresh frame `Call` does.
    MakeFunc(usize),
    /// `Load`, except that a name which is not a variable but is a user
    /// function yields the function itself -- the interpreter's `Expr::Ident`
    /// rule, which lets `f = double;` bind a function value.
    LoadFn(String, usize, u32),
    /// Before a call's arguments run, check that `name` holds a callable
    /// value. `mode` picks the missing-name error the interpreter's path
    /// uses: 0 is `E032` (an expression), 1 is `E031` (the `... ကို လုပ်ပါ။`
    /// form) and 2 is `E113` (the eng `name(args);` statement).
    CallableGuard(String, u32, u8),
    /// Call the callable in variable `name` with `argc` arguments from the
    /// stack. `mode` is the guard's, for a name that vanished between the two
    /// instructions. A callable call always wants a value, so a body that
    /// yields none is the interpreter's `E114` with the callable's label.
    CallDyn(String, u8, u32, u8),
    /// `reverse`: reverse the value on top of the stack. With a variable, a
    /// list is reversed in place and written back (`E104` for a `pin` list);
    /// every other value is reversed without a write.
    Reverse(Option<String>, u32),
    /// Keep the top value only when it is a bool, with the interpreter's
    /// `E043` for anything else.
    ToBool(u32),
    /// Short-circuit `and`: when the left operand is false, push `false` and
    /// jump past the right operand; otherwise evaluate it.
    AndJump(usize, u32),
    /// Short-circuit `or`: when the left operand is true, push `true` and
    /// jump past the right operand; otherwise evaluate it.
    OrJump(usize, u32),
    /// End of the top-level chunk.
    Halt,
}

/// One compiled class: its constructor parameters, and the constructor body
/// (the first method) that fills the object's fields in.
#[derive(Debug, Clone)]
pub struct ClassDef {
    pub name: String,
    pub params: Vec<String>,
    pub code: Vec<Instr>,
}

/// One compiled user function.
#[derive(Debug, Clone)]
pub struct FuncDef {
    pub name: String,
    pub params: Vec<String>,
    /// How many arguments a call must supply: the parameters without a
    /// default. The rest are filled in by the body's own prologue.
    pub required: usize,
    pub code: Vec<Instr>,
}

/// A whole compiled program: the top-level chunk plus every function body.
#[derive(Debug, Clone, Default)]
pub struct Program {
    pub code: Vec<Instr>,
    pub funcs: Vec<FuncDef>,
    pub classes: Vec<ClassDef>,
}

/// A builtin the VM can run from evaluated values alone.
///
/// The mutating list builtins (`push`, `pop`, ...) reach for a variable rather
/// than a value, so they stay `E170` in bytecode mode; `reverse` is compiled
/// by [`Compiler::reverse_site`] instead. `map`/`filter`/`reduce` are not
/// value-only either, but they have their own instructions -- see
/// [`is_higher_order`].
fn is_pure_builtin(name: &str) -> bool {
    crate::interpreter::is_eng_builtin(name)
        && !crate::interpreter::is_mutating_builtin(name)
        && !matches!(name, "reverse" | "map" | "filter" | "reduce")
}

/// The callback builtins: `map`, `filter` and `reduce`. They walk a value and
/// call a function for every item, so they compile to `IterItems` plus
/// `HigherOrder` with the lambda (or named function) as its own body.
fn is_mutating_builtin(name: &str) -> bool {
    // `reverse` is handled as a value too (a string reverses into a string),
    // so it keeps its own `E170` until that is compiled.
    name != "reverse" && crate::interpreter::is_mutating_builtin(name)
}

fn is_higher_order(name: &str) -> bool {
    crate::interpreter::is_eng_builtin(name) && matches!(name, "map" | "filter" | "reduce")
}

fn higher_order_name(kind: u8) -> &'static str {
    match kind {
        0 => "map",
        1 => "filter",
        _ => "reduce",
    }
}

fn unsupported(line: usize, what: &str) -> String {
    format!(
        "E170 လိုင်း {} တွင် {} ကို bytecode ဖြင့် မထောက်ပံ့သေးပါ။ `akk` (interpreter) ဖြင့် ဖွင့်ပါ။",
        line, what
    )
}

fn bool_expected(line: usize) -> String {
    format!(
        "E171 လိုင်း {} တွင် bytecode စက်သည် အခြေအနေကို bool အဖြစ်သာ စစ်နိုင်ပါသည်။",
        line
    )
}

fn math_type_err(op: char) -> String {
    format!(
        "E172 တွင် '{}' ကို ဤ တန်ဖိုး အမျိုးအစားများဖြင့် မတွက်နိုင်ပါ။",
        op
    )
}

fn unknown_var(name: &str, line: usize) -> String {
    format!(
        "E030 လိုင်း {} တွင် \"{}\" ဆိုသော ကိန်းရှင်ကို ရှာမတွေ့ပါ။",
        line, name
    )
}

// ---------------------------------------------------------------------------
// Compiler
// ---------------------------------------------------------------------------

/// A loop being compiled: the jump sites that still have to point at the
/// loop's exit / its next iteration.
#[derive(Default)]
struct LoopCtx {
    breaks: Vec<usize>,
    continues: Vec<usize>,
    /// Set while compiling a `loop { }` used as a value: the temporary slot a
    /// `break <value>` writes its value to, and the loop statement's line
    /// (which `E115` points at when a `break` carries no value).
    break_value: Option<(String, usize)>,
    /// How many `try` handlers were live when the loop started, so a
    /// `break`/`continue` out of a `try` inside the loop drops exactly the
    /// handlers it leaves behind.
    handlers: usize,
    /// Set for a `switch`: its arms are a `break` target of their own, but a
    /// `continue` there belongs to the enclosing loop, as in the interpreter.
    is_switch: bool,
}

struct Compiler {
    /// The chunk being written (main code, or one function body).
    code: Vec<Instr>,
    funcs: Vec<FuncDef>,
    /// Slot in `funcs` by name, so a call compiles even before the callee's
    /// body is written.
    slots: HashMap<String, usize>,
    /// `(total, required)` parameters of each function, so a call with the
    /// wrong argument count can be refused the way the interpreter refuses it.
    arities: HashMap<String, (usize, usize)>,
    /// Library alias -> the library's real name (`r` -> `ကျပန်း`), from
    /// `... အဖြစ် r` and `<lib> အဖြစ် r။`.
    lib_aliases: HashMap<String, String>,
    /// Class name -> its slot in `classes`.
    class_slots: HashMap<String, usize>,
    classes: Vec<ClassDef>,
    loops: Vec<LoopCtx>,
    /// `try` handlers live in the code being compiled.
    handlers: usize,
    temps: usize,
    /// Line of the statement being compiled, for error messages.
    line: usize,
}

impl Compiler {
    fn new() -> Self {
        Compiler {
            code: Vec::new(),
            funcs: Vec::new(),
            slots: HashMap::new(),
            arities: HashMap::new(),
            lib_aliases: HashMap::new(),
            class_slots: HashMap::new(),
            classes: Vec::new(),
            loops: Vec::new(),
            handlers: 0,

            temps: 0,
            line: 0,
        }
    }

    fn here(&self) -> usize {
        self.code.len()
    }

    fn emit(&mut self, instr: Instr) -> usize {
        self.code.push(instr);
        self.code.len() - 1
    }

    fn temp(&mut self, purpose: &str) -> String {
        self.temps += 1;
        format!("#{}{}", purpose, self.temps)
    }

    /// Emit a `finally` body inline, at a point where execution leaves the
    /// `try` it belongs to.
    fn inline_finally(&mut self, body: &Option<Vec<Stmt>>) -> Result<(), String> {
        match body {
            Some(body) => self.block(body),
            None => Ok(()),
        }
    }

    /// Leave every `try` down to `target` handler depth: drop the handlers the
    /// `break`/`continue`/`return` is leaving behind.
    ///
    /// The `finally` body is deliberately *not* run here: the interpreter's
    /// statement loop stops as soon as a break/continue/return is in flight, so
    /// `finally` is skipped on those exits and only runs on a normal end or an
    /// error. Matching that is what keeps the two backends identical.
    fn leave_tries(&mut self, target: usize) -> Result<(), String> {
        let count = self.handlers.saturating_sub(target);
        if count > 0 {
            self.emit(Instr::PopHandlers(count.min(255) as u8));
        }
        Ok(())
    }

    fn patch_jump(&mut self, at: usize, target: usize) {
        match &mut self.code[at] {
            Instr::Jump(to) | Instr::JumpIfFalse(to) | Instr::JumpIfTrue(to) => *to = target,
            other => panic!("patch on a non-jump: {:?}", other),
        }
    }

    fn block(&mut self, stmts: &[Stmt]) -> Result<(), String> {
        for stmt in stmts {
            self.stmt(stmt)?;
        }
        Ok(())
    }

    fn stmt(&mut self, stmt: &Stmt) -> Result<(), String> {
        self.line = stmt_line(stmt);
        match stmt {
            Stmt::VarDecl { name, value, .. } => {
                self.expr(value)?;
                self.emit(Instr::Store(name.to_string()));
                Ok(())
            }
            // `[a, b] = <value>;` -- the value runs once, then one name per
            // item, in order.
            Stmt::Destructure { names, value, line } => {
                self.line = *line;
                self.expr(value)?;
                self.emit(Instr::Destructure(names.clone(), *line as u32));
                Ok(())
            }
            // `x :int = 5;`: the declaration's own type check and `pin`
            // protection, done by the runtime with the interpreter's helpers.
            Stmt::EngDecl {
                name,
                type_name,
                value,
                is_const,
                line,
            } => {
                self.line = *line;
                self.expr(value)?;
                self.emit(Instr::Declare(
                    name.to_string(),
                    type_name.clone(),
                    *is_const,
                    *line as u32,
                ));
                Ok(())
            }
            // `x = <value>;`: a `pin` target is refused before the value runs.
            Stmt::EngAssign { name, value, line } => {
                self.line = *line;
                self.emit(Instr::PinGuard(name.to_string(), *line as u32));
                self.expr(value)?;
                self.emit(Instr::Store(name.to_string()));
                Ok(())
            }
            // `x += <value>;` and friends: guard, read, apply, write back.
            Stmt::EngMathAssign {
                name,
                op,
                amount,
                line,
            } => {
                self.line = *line;
                let instr = match op {
                    '+' => Instr::Add,
                    '-' => Instr::Sub,
                    '*' => Instr::Mul,
                    '/' => Instr::Div(*line as u32),
                    '%' => Instr::Mod(*line as u32),
                    other => {
                        return Err(unsupported(
                            *line,
                            &format!("'{}' လုပ်ဆောင်ချက်", other),
                        ))
                    }
                };
                self.emit(Instr::PinGuard(name.to_string(), *line as u32));
                self.emit(Instr::Load(name.to_string(), self.line as u32));
                self.expr(amount)?;
                self.emit(instr);
                self.emit(Instr::Store(name.to_string()));
                Ok(())
            }
            // `print(<value>);` -- the same instruction as `... ကို ဖော်ပြပါ။`.
            Stmt::EngPrint { value, line } => {
                self.line = *line;
                self.expr(value)?;
                self.emit(Instr::Print);
                Ok(())
            }
            // `break;` / `break <value>;` -- leave the innermost loop. With no
            // loop to stop it fails at run time with the interpreter's `E116`
            // (before the value would be evaluated, as there).
            Stmt::EngBreak { value, line } => {
                self.line = *line;
                if self.loops.is_empty() {
                    self.emit(Instr::Abort(
                        crate::interpreter::eng_break_outside_loop_err(*line),
                    ));
                    return Ok(());
                }
                let ctx = self.loops.last().expect("loop context");
                let target = ctx.handlers;
                let into = ctx.break_value.clone();
                match value {
                    Some(expr) => {
                        self.expr(expr)?;
                        match &into {
                            Some((slot, _)) => self.emit(Instr::Store(slot.clone())),
                            // A value handed to a loop that is not used as one
                            // is evaluated and dropped, exactly as there.
                            None => self.emit(Instr::Pop),
                        };
                    }
                    None => {
                        if let Some((_, loop_line)) = into {
                            // `loop { ... }` as a value can only end through a
                            // `break <value>;` -- `E115`, at the loop's line.
                            self.leave_tries(target)?;
                            self.emit(Instr::Abort(crate::interpreter::eng_loop_no_value_err(
                                loop_line,
                            )));
                            return Ok(());
                        }
                    }
                }
                self.leave_tries(target)?;
                let at = self.emit(Instr::Jump(usize::MAX));
                self.loops.last_mut().expect("loop context").breaks.push(at);
                Ok(())
            }
            // `continue;` -- jump back to the top of the innermost loop.
            Stmt::Continue { line } => {
                self.line = *line;
                // A `switch` counts as a break target but not as a loop for
                // `continue`: the interpreter records the continue and lets it
                // reach the enclosing loop, so the jump is patched there.
                let Some(index) = self.loops.iter().rposition(|ctx| !ctx.is_switch) else {
                    self.emit(Instr::Abort(
                        crate::interpreter::eng_continue_outside_loop_err(*line),
                    ));
                    return Ok(());
                };
                let target = self.loops[index].handlers;
                self.leave_tries(target)?;
                let at = self.emit(Instr::Jump(usize::MAX));
                self.loops[index].continues.push(at);
                Ok(())
            }
            // `loop { ... }` -- run the body until a `break` leaves it.
            Stmt::EngLoop { body } => {
                let start = self.here();
                self.loops.push(LoopCtx {
                    handlers: self.handlers,
                    ..Default::default()
                });
                self.block(body)?;
                let ctx = self.loops.pop().expect("loop context");
                self.emit(Instr::Jump(start));
                let end = self.here();
                for site in ctx.breaks {
                    self.patch_jump(site, end);
                }
                for site in ctx.continues {
                    self.patch_jump(site, start);
                }
                Ok(())
            }
            // `နည်းပညာများ <lib> [အဖြစ် <alias>] ကို သုံးမည်။` -- import a
            // builtin library. A script/package library has no bytecode
            // equivalent, so importing one is refused rather than run.
            Stmt::UseLibrary { libs, line } => {
                for (name, alias) in libs {
                    if !crate::library::is_builtin_library(name) {
                        return Err(unsupported(
                            *line,
                            &format!("\"{}\" နည်းပညာများ တင်သွင်းခြင်း", name),
                        ));
                    }
                    self.emit(Instr::UseLib(name.clone()));
                    if let Some(alias) = alias {
                        self.lib_aliases.insert(alias.clone(), name.clone());
                    }
                }
                Ok(())
            }
            // `<lib> အဖြစ် <alias>။` -- an alias for a library (which also
            // imports it), or for an existing function. A name that is
            // neither is `E031` when it runs, as there.
            Stmt::FuncAlias {
                original,
                aliases,
                line,
            } => {
                if let Some(&slot) = self.slots.get(original) {
                    let arity = self.arities.get(original).copied();
                    for alias in aliases {
                        self.slots.insert(alias.clone(), slot);
                        if let Some(arity) = arity {
                            self.arities.insert(alias.clone(), arity);
                        }
                    }
                    return Ok(());
                }
                if crate::library::is_builtin_library(original) {
                    for alias in aliases {
                        self.lib_aliases.insert(alias.clone(), original.clone());
                    }
                    self.emit(Instr::UseLib(original.clone()));
                    return Ok(());
                }
                self.emit(Instr::Abort(
                    crate::interpreter::eng_missing_function_err(*line, original),
                ));
                Ok(())
            }
            // `<lib> ၏ <fn>(...) ကို လုပ်ပါ။` -- result discarded.
            Stmt::LibCall {
                lib,
                fn_name,
                args,
                line,
            } => {
                self.line = *line;
                self.lib_call_site(lib, fn_name, args, *line, None)?;
                self.emit(Instr::Pop);
                Ok(())
            }
            // `<var> အတွက် <lib> ၏ <fn>(...) ကို လုပ်ပါ။` -- result stored.
            Stmt::LibCallAssign {
                name,
                lib,
                fn_name,
                args,
                line,
            } => {
                self.line = *line;
                let err = format!(
                    "E088 လိုင်း {} တွင် \"{}\" ၏ \"{}\" အလုပ်သည် တန်ဖိုး ပြန်မပေးသဖြင့် \"{}\" ကို သိမ်းဆည်း၍မရပါ။",
                    line, lib, fn_name, name
                );
                self.lib_call_site(lib, fn_name, args, *line, Some(err))?;
                self.emit(Instr::Store(name.to_string()));
                Ok(())
            }
            // `<name> [:<type>] = loop { ... };` -- a loop used as a value.
            //
            // The interpreter seeds a typed name with its type's default
            // before the loop runs (so the body can accumulate into it), keeps
            // the value passed to `break <value>`, then checks that value
            // against the type and stores it. The same steps are compiled in
            // the same order here, so a `pin` redeclaration is refused before
            // the body runs and a bad break value is refused after it.
            Stmt::EngLoopAssign {
                name,
                type_name,
                is_const,
                body,
                line,
            } => {
                self.line = *line;
                // A `pin` target is refused before the loop runs, exactly as
                // the declaration and assignment statements refuse it.
                self.emit(Instr::PinGuard(name.to_string(), *line as u32));
                if *is_const {
                    self.emit(Instr::PinDeclareCheck(name.to_string(), *line as u32));
                }
                if let Some(t) = type_name {
                    self.emit(Instr::Const(crate::interpreter::eng_type_default(t)));
                    self.emit(Instr::Declare(
                        name.to_string(),
                        t.clone(),
                        *is_const,
                        *line as u32,
                    ));
                }
                let slot = self.temp("loopvalue");
                let start = self.here();
                self.loops.push(LoopCtx {
                    handlers: self.handlers,
                    break_value: Some((slot.clone(), *line)),
                    ..Default::default()
                });
                self.block(body)?;
                let ctx = self.loops.pop().expect("loop context");
                self.emit(Instr::Jump(start));
                let end = self.here();
                for site in ctx.breaks {
                    self.patch_jump(site, end);
                }
                for site in ctx.continues {
                    self.patch_jump(site, start);
                }
                self.emit(Instr::Load(slot, self.line as u32));
                match type_name {
                    Some(t) => self.emit(Instr::Declare(
                        name.to_string(),
                        t.clone(),
                        false,
                        *line as u32,
                    )),
                    None => self.emit(Instr::Store(name.to_string())),
                };
                if *is_const && type_name.is_none() {
                    // `pin X = loop { ... }`: the pre-check above only looked,
                    // so the name becomes a constant once it holds a value.
                    self.emit(Instr::MarkConst(name.to_string()));
                }
                Ok(())
            }
            // `name(args);` -- a call whose result is discarded, but whose
            // callable resolution is the eng statement's (`E113`, and a
            // callable that yields nothing is still `E114`).
            Stmt::EngExprCall { name, args, line } => {
                self.line = *line;
                self.call_site(name, args, *line, 2)?;
                self.emit(Instr::Pop);
                Ok(())
            }
            Stmt::Print { value, .. } => {
                self.expr(value)?;
                self.emit(Instr::Print);
                Ok(())
            }
            Stmt::ExprStmt { value, .. } => {
                self.expr(value)?;
                self.emit(Instr::Pop);
                Ok(())
            }
            Stmt::MathAssign {
                target,
                op,
                amount,
                ..
            } => {
                let Expr::Ident(name) = target else {
                    return Err(unsupported(self.line, "ဤ နေရာသို့ ပေါင်းထည့်ခြင်း"));
                };
                self.emit(Instr::Load(name.to_string(), self.line as u32));
                self.expr(amount)?;
                self.emit(match op {
                    '+' => Instr::Add,
                    '-' => Instr::Sub,
                    '*' => Instr::Mul,
                    '/' => Instr::Div(self.line as u32),
                    '%' => Instr::Mod(self.line as u32),
                    other => {
                        return Err(unsupported(
                            self.line,
                            &format!("'{}' လုပ်ဆောင်ချက်", other),
                        ))
                    }
                });
                self.emit(Instr::Store(name.to_string()));
                Ok(())
            }
            Stmt::If { branches } => self.if_chain(branches),
            Stmt::While {
                cond,
                negate,
                body,
                ..
            } => self.while_loop(cond, *negate, body),
            Stmt::ForLoop {
                var_name,
                source,
                body,
                ..
            } => self.for_loop(var_name, source, body),
            // `switch (<value>) { case ... default: ... }` and the Myanmar
            // `ရွေးရန်`: the subject runs once, only the first matching case's
            // body runs, and `default` runs only when no case matched, wherever
            // it is written. A `break` in an arm stops the switch, so the arms
            // are compiled with a loop context of their own.
            Stmt::Switch {
                subject,
                arms,
                line,
            } => {
                self.line = *line;
                let slot = self.temp("switch");
                self.expr(subject)?;
                self.emit(Instr::Store(slot.clone()));
                self.loops.push(LoopCtx {
                    handlers: self.handlers,
                    is_switch: true,
                    ..Default::default()
                });
                let mut to_end: Vec<usize> = Vec::new();
                let mut default_body: Option<&Vec<Stmt>> = None;
                for arm in arms {
                    match &arm.case {
                        Some(case) => {
                            self.emit(Instr::Load(slot.clone(), arm.line as u32));
                            self.expr(case)?;
                            self.emit(Instr::Eq);
                            let skip = self.emit(Instr::JumpIfFalse(usize::MAX));
                            self.block(&arm.body)?;
                            to_end.push(self.emit(Instr::Jump(usize::MAX)));
                            let next = self.here();
                            self.patch_jump(skip, next);
                        }
                        None => {
                            if default_body.is_none() {
                                default_body = Some(&arm.body);
                            }
                        }
                    }
                }
                if let Some(body) = default_body {
                    self.block(body)?;
                }
                let end = self.here();
                for site in to_end {
                    self.patch_jump(site, end);
                }
                let ctx = self.loops.pop().expect("switch context");
                for site in ctx.breaks {
                    self.patch_jump(site, end);
                }
                Ok(())
            }
            // Slots were registered in the first pass; the body is compiled
            // there too, so the statement itself emits nothing here.
            Stmt::FuncDef { .. } => Ok(()),
            // The class was registered and its constructor compiled in the
            // earlier passes; the statement itself emits nothing.
            Stmt::ClassDef { .. } => Ok(()),
            // Input: show the prompt (no newline), read one line, and either
            // discard it or bind the inferred value. The interpreter does the
            // same three steps in the same order.
            Stmt::InputNoAssign { value, .. } | Stmt::EngInput { prompt: value, .. } => {
                self.expr(value)?;
                self.emit(Instr::Prompt);
                self.emit(Instr::Read);
                self.emit(Instr::Pop);
                Ok(())
            }
            Stmt::InputAssign { name, value, .. } => {
                self.expr(value)?;
                self.emit(Instr::Prompt);
                self.emit(Instr::Read);
                self.emit(Instr::Store(name.to_string()));
                Ok(())
            }
            Stmt::EngInputAssign {
                name,
                type_name,
                prompt,
                line,
                ..
            } => {
                // The conversion error carries this statement's line, the way
                // the interpreter's does.
                self.line = *line;
                self.expr(prompt)?;
                self.emit(Instr::Prompt);
                self.emit(Instr::ReadTyped(type_name.clone(), self.line as u32));
                self.emit(Instr::Store(name.to_string()));
                Ok(())
            }
            Stmt::SelfFieldSet {
                field,
                value,
                line,
            } => {
                self.expr(value)?;
                self.emit(Instr::SelfSet(field.clone(), *line as u32));
                Ok(())
            }
            Stmt::FuncCall { name, args, line } => {
                self.line = *line;
                self.call_site(name, args, *line, 1)?;
                self.emit(Instr::Pop);
                Ok(())
            }
            Stmt::IndexAssign {
                target,
                value,
                line,
            } => {
                let Expr::Index(base, indices) = target else {
                    return Err(unsupported(*line, "အညွှန်းသို့ သတ်မှတ်ခြင်း"));
                };
                let Expr::Ident(name) = &**base else {
                    return Err(unsupported(*line, "ဤ နေရာသို့ အညွှန်း သတ်မှတ်ခြင်း"));
                };
                self.emit(Instr::Load(name.to_string(), self.line as u32));
                for key in indices {
                    self.expr(key)?;
                }
                self.expr(value)?;
                self.emit(Instr::IndexStore(
                    name.to_string(),
                    *line as u32,
                    indices.len() as u8,
                ));
                Ok(())
            }
            Stmt::EngReturn { value, line } => {
                let produced = match value {
                    Some(v) => {
                        self.expr(v)?;
                        true
                    }
                    None => {
                        // `return;` carries no value, so a caller that wanted
                        // one gets the interpreter's `E071`.
                        self.emit(Instr::Const(Value::Null));
                        false
                    }
                };
                // A `return` inside a `try` runs its `finally` first and drops
                // the handlers of this function.
                self.leave_tries(0)?;
                if produced {
                    self.emit(Instr::ReturnValue(*line as u32));
                } else {
                    self.emit(Instr::ReturnVoid(*line as u32));
                }
                Ok(())
            }
            // `<var> အတွက် <fn>(args) ကို လုပ်ပါ။` -- call and keep the result.
            // A body that returns nothing is `E071`, so the value is asked for
            // by the call itself rather than assumed from a pushed `null`.
            Stmt::FuncCallAssign {
                name,
                fn_name,
                args,
                line,
            } => {
                self.line = *line;
                self.assign_call_site(name, fn_name, args, *line)?;
                self.emit(Instr::Store(name.to_string()));
                Ok(())
            }
            // `try { } catch (E) { } finally { }`, laid out as the interpreter
            // runs it: the body protected by a handler, the `finally` body on
            // the normal path, the catch path guarded so its own errors still
            // run the `finally`, and a finally+rethrow exit for an unmatched
            // error.
            Stmt::TryCatch {
                try_body,
                catch_err,
                catch_var,
                catch_body,
                finally_body,
            } => {
                let has_catch = catch_err.is_some() && catch_body.is_some();
                let mut to_after: Vec<usize> = Vec::new();

                let guard = self.emit(Instr::Try(usize::MAX));
                self.handlers += 1;
                self.block(try_body)?;
                self.handlers -= 1;
                self.emit(Instr::TryEnd);
                self.inline_finally(finally_body)?;
                to_after.push(self.emit(Instr::Jump(usize::MAX)));

                // Handler entry: the error's message is in `pending_error`.
                let handler_pc = self.here();
                match &mut self.code[guard] {
                    Instr::Try(target) => *target = handler_pc,
                    _ => unreachable!("the guard is a Try"),
                }
                if has_catch {
                    let mut to_rethrow: Vec<usize> = Vec::new();
                    if let Some(code) = catch_err {
                        // `E` catches anything; otherwise the code must match,
                        // exactly as the interpreter decides it.
                        if code != "E" {
                            self.emit(Instr::ErrorCode);
                            self.emit(Instr::Const(Value::Str(code.clone())));
                            self.emit(Instr::Eq);
                            to_rethrow.push(self.emit(Instr::JumpIfFalse(usize::MAX)));
                        }
                    }
                    if let Some(var) = catch_var {
                        // The catch name holds the error's code, as there.
                        self.emit(Instr::ErrorCode);
                        self.emit(Instr::Store(var.clone()));
                    }
                    let catch_guard = self.emit(Instr::Try(usize::MAX));
                    self.handlers += 1;
                    if let Some(body) = catch_body {
                        self.block(body)?;
                    }
                    self.handlers -= 1;
                    self.emit(Instr::TryEnd);
                    self.inline_finally(finally_body)?;
                    to_after.push(self.emit(Instr::Jump(usize::MAX)));

                    // A failed filter, or an error inside the catch body, runs
                    // the `finally` and re-raises.
                    let rethrow_pc = self.here();
                    match &mut self.code[catch_guard] {
                        Instr::Try(target) => *target = rethrow_pc,
                        _ => unreachable!("the guard is a Try"),
                    }
                    for site in to_rethrow {
                        self.patch_jump(site, rethrow_pc);
                    }
                    self.inline_finally(finally_body)?;
                    self.emit(Instr::ReThrow);
                } else {
                    // No catch to take the error: run the finally and raise it
                    // again, to whatever handler is outside.
                    self.inline_finally(finally_body)?;
                    self.emit(Instr::ReThrow);
                }
                let after = self.here();
                for site in to_after {
                    self.patch_jump(site, after);
                }
                Ok(())
            }
            other => Err(unsupported(self.line, stmt_name(other))),
        }
    }

    /// Rewrite a `<lib>` written through an alias (`r` -> `ကျပန်း`).
    fn resolve_lib(&self, lib: &str) -> String {
        self.lib_aliases
            .get(lib)
            .cloned()
            .unwrap_or_else(|| lib.to_string())
    }

    /// Compile one library call.
    ///
    /// The interpreter checks, in this order: the library is imported
    /// (`E089`), the function exists (`E086`), the argument count fits
    /// (`E087`), and only then evaluates the arguments. The same order is
    /// compiled here, so a call that fails raises before its arguments run.
    /// `err` is the message for a call that must produce a value but does not
    /// (empty string: a missing value is fine, `null` is pushed instead).
    fn lib_call_site(
        &mut self,
        lib: &str,
        fn_name: &str,
        args: &[Expr],
        line: usize,
        err: Option<String>,
    ) -> Result<(), String> {
        let lib = self.resolve_lib(lib);
        if !crate::library::is_builtin_library(&lib) {
            // Not a library this backend knows: `E085` when it runs, and the
            // arguments are not evaluated, exactly as there.
            self.emit(Instr::Abort(crate::interpreter::lib_unknown_err(
                &lib, line,
            )));
            self.emit(Instr::Const(Value::Null));
            return Ok(());
        }
        if !crate::interpreter::lib_is_value_library(&lib) {
            return Err(unsupported(
                line,
                &format!("\"{}\" နည်းပညာ ခေါ်ခြင်း", lib),
            ));
        }
        self.emit(Instr::RequireLib(lib.clone(), line as u32));
        match crate::interpreter::lib_value_arity(&lib, fn_name, line) {
            Err(message) => {
                // Unknown function in a known library: `E086`, before the
                // arguments would run.
                self.emit(Instr::Abort(message));
                self.emit(Instr::Const(Value::Null));
                return Ok(());
            }
            Ok(want) => {
                if args.len() != want {
                    let message = crate::interpreter::lib_fn_argc_count(
                        &lib,
                        fn_name,
                        args.len(),
                        want,
                        line,
                    )
                    .unwrap_err();
                    self.emit(Instr::Abort(message));
                    self.emit(Instr::Const(Value::Null));
                    return Ok(());
                }
            }
        }
        for arg in args {
            self.expr(arg)?;
        }
        self.emit(Instr::LibCall(
            lib,
            fn_name.to_string(),
            args.len() as u8,
            line as u32,
            err.unwrap_or_default(),
        ));
        Ok(())
    }

    /// Compile the call half of `<var> အတွက် <fn>(args) ကို လုပ်ပါ။`, leaving
    /// exactly one value on the stack. The interpreter resolves a class first,
    /// then a user function, then a builtin, then a callable value, and only
    /// then reports `E031` -- the same order is used here.
    fn assign_call_site(
        &mut self,
        target: &str,
        fn_name: &str,
        args: &[Expr],
        line: usize,
    ) -> Result<(), String> {
        if let Some(&index) = self.class_slots.get(fn_name) {
            for arg in args {
                self.expr(arg)?;
            }
            self.emit(Instr::NewObj(index, args.len() as u8, line as u32));
            return Ok(());
        }
        if self.slots.contains_key(fn_name) {
            if let Some(&(total, required)) = self.arities.get(fn_name) {
                let argc = args.len();
                if argc < required || argc > total {
                    self.emit(Instr::Abort(crate::interpreter::eng_arity_err(
                        fn_name, argc, required, total, line,
                    )));
                    self.emit(Instr::Const(Value::Null));
                    return Ok(());
                }
            }
            let index = self.slots[fn_name];
            for arg in args {
                self.expr(arg)?;
            }
            let err = format!(
                "E071 လိုင်း {} တွင် \"{}\" အလုပ်သည် တန်ဖိုး ပြန်မပေးသဖြင့် \"{}\" ကို သိမ်းဆည်း၍မရပါ။ အလုပ်၏ နောက်ဆုံး ညွှန်ကြားချက်သည် တန်ဖိုး တစ်ခု ဖြစ်ရပါမည်။",
                line, fn_name, target
            );
            self.emit(Instr::CallValue(index, args.len() as u8, line as u32, err));
            return Ok(());
        }
        if fn_name == "reverse" {
            // The caller stores the value, as it does for any call.
            self.reverse_site(args, line)?;
            return Ok(());
        }
        if is_higher_order(fn_name) {
            // The caller stores the value, as it does for any call.
            self.higher_order_site(fn_name, args, line)?;
            return Ok(());
        }
        if is_mutating_builtin(fn_name) {
            // The caller stores the value, as it does for any call.
            self.mutate_site(fn_name, args, line)?;
            return Ok(());
        }
        if is_pure_builtin(fn_name) {
            for arg in args {
                self.expr(arg)?;
            }
            self.emit(Instr::Builtin(fn_name.to_string(), args.len() as u8, line as u32));
            return Ok(());
        }
        if crate::interpreter::is_eng_builtin(fn_name) {
            return Err(unsupported(line, "builtin အလုပ်"));
        }
        // Neither a function nor a builtin: `E031` when it runs, before the
        // arguments would be evaluated -- or a variable holding a callable,
        // which the interpreter calls instead of reporting `E031`.
        self.callable_call_site(fn_name, args, line, 1)?;
        Ok(())
    }

    /// `map`/`filter`/`reduce`: the collection is turned into its items (with
    /// the interpreter's `E110` for a value that cannot be walked), then the
    /// callback is evaluated as an ordinary value -- a lambda, a named
    /// function or a variable all work, because the interpreter resolves the
    /// same three -- and then `reduce`'s starting value. A library function
    /// reference stays `E170`: the VM cannot represent one.
    fn higher_order_site(&mut self, name: &str, args: &[Expr], line: usize) -> Result<(), String> {
        let kind = match name {
            "map" => 0u8,
            "filter" => 1,
            _ => 2,
        };
        let want = if kind == 2 { 3 } else { 2 };
        if args.len() != want {
            self.emit(Instr::Abort(crate::interpreter::eng_builtin_err(
                line,
                name,
                &format!(
                    "အချက် {want} ခု လိုအပ်ပါသည်၊ {} ရပါသည်",
                    args.len()
                ),
            )));
            self.emit(Instr::Const(Value::Null));
            return Ok(());
        }
        self.expr(&args[0])?;
        self.emit(Instr::IterItems(kind, line as u32));
        if matches!(&args[1], Expr::LibFnRef(..)) {
            return Err(unsupported(
                self.line,
                "နည်းပညာ အလုပ် ကို ပြန်ခေါ် အလုပ်အဖြစ်",
            ));
        }
        self.expr(&args[1])?;
        if kind == 2 {
            self.expr(&args[2])?;
        }
        self.emit(Instr::HigherOrder(kind, line as u32));
        Ok(())
    }

    /// Compile a call resolved at run time from a variable holding a callable
    /// value: `f(1)`, `apply(fn(x) { x * 2 }, 2)`.
    ///
    /// The interpreter checks that the name is a callable *before* it
    /// evaluates the arguments, and so does [`Instr::CallableGuard`]; the
    /// arguments then run, and [`Instr::CallDyn`] enters the callable's frame.
    fn callable_call_site(
        &mut self,
        name: &str,
        args: &[Expr],
        line: usize,
        missing: u8,
    ) -> Result<(), String> {
        self.emit(Instr::CallableGuard(name.to_string(), line as u32, missing));
        for arg in args {
            self.expr(arg)?;
        }
        self.emit(Instr::CallDyn(
            name.to_string(),
            args.len() as u8,
            line as u32,
            missing,
        ));
        Ok(())
    }

    /// `reverse(xs)`: an identifier argument reverses a list variable in place
    /// (the interpreter's rule, `pin` included); any other value is reversed
    /// without a write.
    fn reverse_site(&mut self, args: &[Expr], line: usize) -> Result<(), String> {
        if args.len() != 1 {
            self.emit(Instr::Abort(crate::interpreter::eng_builtin_err(
                line,
                "reverse",
                &format!(
                    "စာရင်း သို့မဟုတ် စာသား တစ်ခု လိုအပ်ပါသည်၊ {} ရပါသည်",
                    args.len()
                ),
            )));
            self.emit(Instr::Const(Value::Null));
            return Ok(());
        }
        match &args[0] {
            Expr::Ident(name) => {
                self.emit(Instr::Load(name.to_string(), self.line as u32));
                self.emit(Instr::Reverse(Some(name.to_string()), line as u32));
            }
            other => {
                self.expr(other)?;
                self.emit(Instr::Reverse(None, line as u32));
            }
        }
        Ok(())
    }

    /// `push(xs, 4)`, `sort(xs)`, ...: the first argument must name a list
    /// variable. The interpreter checks, in order, that the name is not a
    /// `pin` constant, that it exists (`E030`), that it holds a list (`E110`),
    /// and only then evaluates the other arguments -- compiled here in the same
    /// order. A first argument that is not a variable is the interpreter's own
    /// `E110`, raised before anything runs.
    fn mutate_site(&mut self, name: &str, args: &[Expr], line: usize) -> Result<(), String> {
        let Some(Expr::Ident(var)) = args.first() else {
            let message = crate::interpreter::eng_builtin_err(
                line,
                name,
                "ပထမ အချက်သည် စာရင်း ကိန်းရှင် ဖြစ်ရပါမည်၊ ဥပမာ — push(xs, 4)",
            );
            self.emit(Instr::Abort(message));
            self.emit(Instr::Const(Value::Null));
            return Ok(());
        };
        let var = var.to_string();
        self.emit(Instr::PinGuard(var.clone(), line as u32));
        self.emit(Instr::Load(var.clone(), line as u32));
        self.emit(Instr::ListArg(name.to_string(), line as u32));
        for arg in &args[1..] {
            self.expr(arg)?;
        }
        self.emit(Instr::Mutate(
            name.to_string(),
            var,
            args.len() as u8,
            line as u32,
        ));
        Ok(())
    }

    /// Compile an anonymous function into a function slot of its own, the way
    /// a named definition is compiled: its own chunk, no handlers inherited,
    /// and the last expression as its value. Nested lambdas append more slots
    /// while this one is compiled, so the index is kept from before.
    fn compile_lambda(
        &mut self,
        params: &[String],
        defaults: &[Option<Expr>],
        body: &[Stmt],
        _line: usize,
    ) -> Result<usize, String> {
        let index = self.funcs.len();
        self.funcs.push(FuncDef {
            name: format!("fn({})", params.join(", ")),
            params: params.to_vec(),
            required: defaults.iter().filter(|d| d.is_none()).count(),
            code: Vec::new(),
        });
        let saved_code = std::mem::take(&mut self.code);
        let saved_loops = std::mem::take(&mut self.loops);
        let saved_handlers = self.handlers;
        let saved_line = self.line;
        self.handlers = 0;
        let result = self
            .fill_defaults(params, defaults)
            .and_then(|()| compile_body(self, body));
        let code = std::mem::replace(&mut self.code, saved_code);
        self.loops = saved_loops;
        self.handlers = saved_handlers;
        self.line = saved_line;
        result?;
        self.funcs[index].code = code;
        Ok(index)
    }

    /// A parameter the call left off is filled from its default, in the
    /// callee's own frame, so a later default sees the ones before it.
    fn fill_defaults(
        &mut self,
        params: &[String],
        defaults: &[Option<Expr>],
    ) -> Result<(), String> {
        for (index, default) in defaults.iter().enumerate() {
            let Some(expr) = default else { continue };
            let Some(param) = params.get(index) else { continue };
            let check = self.code.len();
            self.emit(Instr::ParamMissing(param.clone(), 0));
            self.expr(expr)?;
            self.emit(Instr::Store(param.clone()));
            let target = self.code.len();
            if let Instr::ParamMissing(_, jump) = &mut self.code[check] {
                *jump = target;
            }
        }
        Ok(())
    }

    /// Compile a call to the user function `name`.
    ///
    /// The interpreter resolves a callee when it runs a call: an undefined name
    /// is `E031` and a wrong argument count is `E033`, both raised *before* the
    /// arguments are evaluated and both catchable by an enclosing `try`. So an
    /// undefined name and a wrong count compile to an `Abort` carrying the very
    /// same message instead of failing the build -- `runtime_uncaught.akk` must
    /// print its line before the error under `--vm` too. A name that is a
    /// builtin stays `E170`: the VM does not implement builtins, and pretending
    /// the name is unknown would be a lie.
    fn call_site(
        &mut self,
        name: &str,
        args: &[Expr],
        line: usize,
        missing: u8,
    ) -> Result<(), String> {
        let Some(idx) = self.slots.get(name).copied() else {
            if name == "reverse" {
                // The caller discards the value, as it does for any call.
                self.reverse_site(args, line)?;
                return Ok(());
            }
            if is_mutating_builtin(name) {
                // The caller discards the value, as it does for any call.
                self.mutate_site(name, args, line)?;
                return Ok(());
            }
            if is_higher_order(name) {
                // The caller discards the value, as it does for any call.
                self.higher_order_site(name, args, line)?;
                return Ok(());
            }
            if is_pure_builtin(name) {
                for arg in args {
                    self.expr(arg)?;
                }
                self.emit(Instr::Builtin(name.to_string(), args.len() as u8, line as u32));
                return Ok(());
            }
            if crate::interpreter::is_eng_builtin(name) {
                return Err(unsupported(line, "builtin အလုပ်"));
            }
            // No callee by that name: a variable may hold a callable value
            // (a lambda), which the interpreter looks up before reporting a
            // missing function. The guard runs before the arguments, and the
            // call happens after them, exactly as the interpreter orders the
            // two -- and like its `invoke_callable`, the call wants a value.
            self.callable_call_site(name, args, line, missing)?;
            return Ok(());
        };
        if let Some(&(total, required)) = self.arities.get(name) {
            let argc = args.len();
            if argc < required || argc > total {
                self.emit(Instr::Abort(crate::interpreter::eng_arity_err(
                    name, argc, required, total, line,
                )));
                self.emit(Instr::Const(Value::Null));
                return Ok(());
            }
        }
        for arg in args {
            self.expr(arg)?;
        }
        self.emit(Instr::Call(idx, args.len() as u8));
        Ok(())
    }

    fn if_chain(&mut self, branches: &[IfBranch]) -> Result<(), String> {
        let mut exits: Vec<usize> = Vec::new();
        let last = branches.len().saturating_sub(1);
        for (i, branch) in branches.iter().enumerate() {
            match &branch.cond {
                Some(cond) => {
                    self.cond(cond, branch.negate)?;
                    let skip = self.emit(Instr::JumpIfFalse(usize::MAX));
                    self.block(&branch.body)?;
                    if i != last {
                        exits.push(self.emit(Instr::Jump(usize::MAX)));
                    }
                    let next = self.here();
                    self.patch_jump(skip, next);
                }
                None => self.block(&branch.body)?,
            }
        }
        let end = self.here();
        for site in exits {
            self.patch_jump(site, end);
        }
        Ok(())
    }

    fn while_loop(&mut self, cond: &CondChain, negate: bool, body: &[Stmt]) -> Result<(), String> {
        let start = self.here();
        self.cond(cond, negate)?;
        let exit = self.emit(Instr::JumpIfFalse(usize::MAX));
        self.loops.push(LoopCtx {
            handlers: self.handlers,
            ..Default::default()
        });
        self.block(body)?;
        let ctx = self.loops.pop().expect("loop context");
        self.emit(Instr::Jump(start));
        let end = self.here();
        self.patch_jump(exit, end);
        for site in ctx.breaks {
            self.patch_jump(site, end);
        }
        for site in ctx.continues {
            self.patch_jump(site, start);
        }
        Ok(())
    }

    fn for_loop(
        &mut self,
        var_name: &str,
        source: &ForSource,
        body: &[Stmt],
    ) -> Result<(), String> {
        match source {
            ForSource::RangeSigned { start, end, step } => {
                self.numeric_range(var_name, start, end, step, body)
            }
            ForSource::Range {
                start,
                end,
                step,
                op,
            } => {
                let start_expr = start.clone().unwrap_or(Expr::Int(0));
                let step_expr = match op {
                    '-' => Expr::Neg(Box::new(step.clone()), self.line),
                    _ => step.clone(),
                };
                self.numeric_range(var_name, &start_expr, end, &step_expr, body)
            }
            ForSource::Auto(expr) => self.collection_loop(var_name, expr, body),
        }
    }

    /// `i သည် တစ်ခုစီ(<start>, <end>[, <step>])` -- a counted loop. A negative
    /// step counts down, exactly as the interpreter does it.
    fn numeric_range(
        &mut self,
        var_name: &str,
        start: &Expr,
        end: &Expr,
        step: &Expr,
        body: &[Stmt],
    ) -> Result<(), String> {
        let end_var = self.temp("end");
        let step_var = self.temp("step");
        self.expr(start)?;
        self.emit(Instr::Store(var_name.to_string()));
        self.expr(end)?;
        self.emit(Instr::Store(end_var.clone()));
        self.expr(step)?;
        self.emit(Instr::Store(step_var.clone()));

        let start_pc = self.here();
        // step > 0 ? i < end : i > end
        self.emit(Instr::Load(step_var.clone(), self.line as u32));
        self.emit(Instr::Const(Value::Int(0)));
        self.emit(Instr::Gt);
        let goes_down = self.emit(Instr::JumpIfFalse(usize::MAX));
        self.emit(Instr::Load(var_name.to_string(), self.line as u32));
        self.emit(Instr::Load(end_var.clone(), self.line as u32));
        self.emit(Instr::Lt);
        let test_done = self.emit(Instr::Jump(usize::MAX));
        let down_at = self.here();
        self.patch_jump(goes_down, down_at);
        self.emit(Instr::Load(var_name.to_string(), self.line as u32));
        self.emit(Instr::Load(end_var.clone(), self.line as u32));
        self.emit(Instr::Gt);
        let test_end = self.here();
        self.patch_jump(test_done, test_end);

        let exit = self.emit(Instr::JumpIfFalse(usize::MAX));
        self.loops.push(LoopCtx {
            handlers: self.handlers,
            ..Default::default()
        });
        self.block(body)?;
        let ctx = self.loops.pop().expect("loop context");
        // A `continue` re-tests, so it lands on the step below, not on it.
        for site in ctx.continues {
            self.patch_jump(site, self.here());
        }
        self.emit(Instr::Load(var_name.to_string(), self.line as u32));
        self.emit(Instr::Load(step_var, self.line as u32));
        self.emit(Instr::Add);
        self.emit(Instr::Store(var_name.to_string()));
        self.emit(Instr::Jump(start_pc));
        let end_pc = self.here();
        self.patch_jump(exit, end_pc);
        for site in ctx.breaks {
            self.patch_jump(site, end_pc);
        }
        Ok(())
    }

    /// `x သည် တစ်ခုစီ(xs)` -- walk a list or tuple by index.
    fn collection_loop(
        &mut self,
        var_name: &str,
        source: &Expr,
        body: &[Stmt],
    ) -> Result<(), String> {
        let seq_var = self.temp("seq");
        let idx_var = self.temp("idx");
        self.expr(source)?;
        // A dictionary yields its keys, exactly as the interpreter's `for`
        // loop does; every other collection is walked as it stands.
        self.emit(Instr::IterKeys);
        self.emit(Instr::Store(seq_var.clone()));
        self.emit(Instr::Const(Value::Int(0)));
        self.emit(Instr::Store(idx_var.clone()));

        let start_pc = self.here();
        self.emit(Instr::Load(idx_var.clone(), self.line as u32));
        self.emit(Instr::Load(seq_var.clone(), self.line as u32));
        self.emit(Instr::Len);
        self.emit(Instr::Lt);
        let exit = self.emit(Instr::JumpIfFalse(usize::MAX));

        self.emit(Instr::Load(seq_var.clone(), self.line as u32));
        self.emit(Instr::Load(idx_var.clone(), self.line as u32));
        self.emit(Instr::Index(self.line as u32, 1));
        self.emit(Instr::Store(var_name.to_string()));

        self.loops.push(LoopCtx {
            handlers: self.handlers,
            ..Default::default()
        });
        self.block(body)?;
        let ctx = self.loops.pop().expect("loop context");
        for site in ctx.continues {
            self.patch_jump(site, self.here());
        }

        self.emit(Instr::Load(idx_var.clone(), self.line as u32));
        self.emit(Instr::Const(Value::Int(1)));
        self.emit(Instr::Add);
        self.emit(Instr::Store(idx_var));
        self.emit(Instr::Jump(start_pc));
        let end_pc = self.here();
        self.patch_jump(exit, end_pc);
        for site in ctx.breaks {
            self.patch_jump(site, end_pc);
        }
        Ok(())
    }

    /// A condition chain. Its atoms are pure comparisons, so they are all
    /// evaluated and then combined with `And`/`Or`; the result is a bool.
    fn cond(&mut self, chain: &CondChain, negate: bool) -> Result<(), String> {
        self.atom(&chain.first)?;
        for (op, atom) in &chain.rest {
            self.atom(atom)?;
            self.emit(match op {
                LogicalOp::And => Instr::And,
                LogicalOp::Or => Instr::Or,
            });
        }
        if negate {
            self.emit(Instr::Not(self.line as u32));
        }
        Ok(())
    }

    fn atom(&mut self, atom: &CondAtom) -> Result<(), String> {
        if atom.type_check.is_some() {
            return Err(unsupported(self.line, "အမျိုးအစား စစ်ဆေးခြင်း"));
        }
        self.expr(&atom.lhs)?;
        match (&atom.op, &atom.rhs) {
            (Some(op), Some(rhs)) => {
                self.expr(rhs)?;
                self.emit(match op.as_str() {
                    "<" => Instr::Lt,
                    "<=" => Instr::Le,
                    ">" => Instr::Gt,
                    ">=" => Instr::Ge,
                    "==" => Instr::Eq,
                    "!=" => Instr::Ne,
                    other => return Err(unsupported(self.line, other)),
                });
            }
            (None, None) => {
                // A bare check like `(flag)`: the value itself must be a bool.
                self.emit(Instr::Const(Value::Bool(true)));
                self.emit(Instr::Eq);
            }
            _ => return Err(bool_expected(self.line)),
        }
        if atom.negate {
            self.emit(Instr::Not(self.line as u32));
        }
        Ok(())
    }

    fn expr(&mut self, expr: &Expr) -> Result<(), String> {
        match expr {
            Expr::Int(n) => {
                self.emit(Instr::Const(Value::Int(*n)));
                Ok(())
            }
            Expr::Float(n) => {
                self.emit(Instr::Const(Value::Float(*n)));
                Ok(())
            }
            Expr::StrLit(s) => {
                self.emit(Instr::Const(Value::Str(s.clone())));
                // A literal with a brace or an escape in it is a template: the
                // interpreter expands it where it is evaluated, so the VM must
                // expand it at exactly that point too.
                if s.contains(['{', '\\']) {
                    self.emit(Instr::Interpolate(self.line as u32));
                }
                Ok(())
            }
            Expr::BoolLit(b) => {
                self.emit(Instr::Const(Value::Bool(*b)));
                Ok(())
            }
            Expr::NullLit => {
                self.emit(Instr::Const(Value::Null));
                Ok(())
            }
            Expr::Ident(name) => {
                // A name that is not a variable but is a function is the
                // function itself (`f = double;`), as the interpreter's own
                // `Expr::Ident` rule says.
                match self.slots.get(&**name).copied() {
                    Some(index) => {
                        self.emit(Instr::LoadFn(name.to_string(), index, self.line as u32));
                    }
                    None => {
                        self.emit(Instr::Load(name.to_string(), self.line as u32));
                    }
                }
                Ok(())
            }
            Expr::Binary(lhs, op, rhs, line) => {
                self.expr(lhs)?;
                self.expr(rhs)?;
                self.emit(match op {
                    '+' => Instr::Add,
                    '-' => Instr::Sub,
                    '*' => Instr::Mul,
                    '/' => Instr::Div(*line as u32),
                    '%' => Instr::Mod(*line as u32),
                    other => return Err(unsupported(*line, &format!("'{}'", other))),
                });
                Ok(())
            }
            Expr::Compare(lhs, op, rhs, line) => {
                self.expr(lhs)?;
                self.expr(rhs)?;
                self.emit(match op.as_str() {
                    "<" => Instr::Lt,
                    "<=" => Instr::Le,
                    ">" => Instr::Gt,
                    ">=" => Instr::Ge,
                    "==" => Instr::Eq,
                    "!=" => Instr::Ne,
                    other => return Err(unsupported(*line, other)),
                });
                Ok(())
            }
            Expr::Not(inner, line) => {
                self.expr(inner)?;
                self.emit(Instr::Not(*line as u32));
                Ok(())
            }
            // `a and b` / `a or b` as a value: short-circuit, then a bool.
            Expr::Logical(lhs, op, rhs, line) => {
                self.expr(lhs)?;
                let at = self.emit(match op {
                    LogicalOp::And => Instr::AndJump(usize::MAX, *line as u32),
                    LogicalOp::Or => Instr::OrJump(usize::MAX, *line as u32),
                });
                self.expr(rhs)?;
                self.emit(Instr::ToBool(*line as u32));
                let end = self.here();
                match &mut self.code[at] {
                    Instr::AndJump(target, _) | Instr::OrJump(target, _) => *target = end,
                    other => panic!("patch on a non-branch: {:?}", other),
                }
                Ok(())
            }
            // `<cond> ? <then> : <else>`: only the chosen branch runs, so the
            // other one may be a call with side effects or a division by zero.
            Expr::Ternary(cond, then_expr, else_expr, line) => {
                self.expr(cond)?;
                self.emit(Instr::ToBool(*line as u32));
                let to_else = self.emit(Instr::JumpIfFalse(usize::MAX));
                self.expr(then_expr)?;
                let to_end = self.emit(Instr::Jump(usize::MAX));
                let else_at = self.here();
                self.patch_jump(to_else, else_at);
                self.expr(else_expr)?;
                let end = self.here();
                self.patch_jump(to_end, end);
                Ok(())
            }
            Expr::Neg(inner, _) => {
                self.expr(inner)?;
                self.emit(Instr::Neg);
                Ok(())
            }
            Expr::ListLit(items) | Expr::TupleLit(items) => {
                // Every element a literal: fold into one constant. Otherwise
                // evaluate the elements and build the collection at run time,
                // so `[a, b + 1]` works as it does in the interpreter.
                let mut values = Vec::with_capacity(items.len());
                let mut folded = true;
                for item in items {
                    match literal(item, self.line) {
                        Ok(value) => values.push(value),
                        Err(_) => {
                            folded = false;
                            break;
                        }
                    }
                }
                if folded {
                    let value = match expr {
                        Expr::TupleLit(_) => Value::Tuple(values),
                        _ => Value::List(values),
                    };
                    self.emit(Instr::Const(value));
                    return Ok(());
                }
                for item in items {
                    self.expr(item)?;
                }
                self.emit(match expr {
                    Expr::TupleLit(_) => Instr::Tuple(items.len()),
                    _ => Instr::List(items.len()),
                });
                Ok(())
            }
            // `reverse(xs)` reverses a list variable in place, and a string,
            // list or tuple value without a write -- the interpreter's rule.
            Expr::NewObj(name, args, line) if name == "reverse" => {
                self.reverse_site(args, *line)?;
                Ok(())
            }
            // An anonymous function used as a value: compile its body once and
            // push it as a callable. 
            Expr::Lambda {
                params,
                defaults,
                body,
                line,
            } => {
                let index = self.compile_lambda(params, defaults, body, *line)?;
                self.emit(Instr::MakeFunc(index));
                Ok(())
            }
            // `<name>(args)` reaches the compiler as an instantiation. The
            // interpreter gives a class priority over a function of the same
            // name, so the compiler resolves them in that order too.
            Expr::NewObj(name, args, _) if self.class_slots.contains_key(name) => {
                let idx = self.class_slots[name];
                for arg in args {
                    self.expr(arg)?;
                }
                self.emit(Instr::NewObj(idx, args.len() as u8, self.line as u32));
                Ok(())
            }
            Expr::NewObj(name, args, line) if self.slots.contains_key(name) => {
                self.call_site(name, args, *line, 1)?;
                Ok(())
            }
            // `<name>(args)` where the name is a builtin: the call becomes one
            // instruction, and the value comes from the interpreter's own
            // builtin code when it runs.
            // `<lib> ၏ <fn>(args)` used as a value: `E117` when the function
            // acts without producing one.
            Expr::LibCall(lib, fn_name, args, line) => {
                let resolved = self.resolve_lib(lib);
                let err = crate::interpreter::eng_lib_no_value_err(&resolved, fn_name, *line);
                self.lib_call_site(lib, fn_name, args, *line, Some(err))?;
                Ok(())
            }
            Expr::NewObj(name, args, line) if is_higher_order(name) => {
                self.higher_order_site(name, args, *line)?;
                Ok(())
            }
            // `push(xs, 4)` and the rest: the list variable is read, checked,
            // updated and written back, exactly as the interpreter stores it.
            Expr::NewObj(name, args, line) if is_mutating_builtin(name) => {
                self.mutate_site(name, args, *line)?;
                Ok(())
            }
            Expr::NewObj(name, args, line) if is_pure_builtin(name) => {
                for arg in args {
                    self.expr(arg)?;
                }
                self.emit(Instr::Builtin(name.clone(), args.len() as u8, *line as u32));
                Ok(())
            }
            // Any other `<name>(args)` used as a value: the name may be a
            // variable holding a lambda, so the call is resolved at run time.
            // A name that resolves to nothing is the interpreter's `E032`.
            Expr::NewObj(name, args, line) => {
                self.callable_call_site(name, args, *line, 0)?;
                Ok(())
            }
            Expr::FieldAccess(target, field, line) => {
                self.expr(target)?;
                self.emit(Instr::Field(field.clone(), *line as u32));
                Ok(())
            }
            Expr::SetLit(items) => {
                for item in items {
                    self.expr(item)?;
                }
                self.emit(Instr::Set(items.len()));
                Ok(())
            }
            Expr::LibFnRef(name, field, line) => {
                self.emit(Instr::FieldRef(
                    name.to_string(),
                    field.clone(),
                    *line as u32,
                ));
                Ok(())
            }
            Expr::Index(target, indices) => {
                self.expr(target)?;
                for index in indices {
                    self.expr(index)?;
                }
                self.emit(Instr::Index(self.line as u32, indices.len() as u8));
                Ok(())
            }
            Expr::DictLit(pairs) => {
                for (key, value) in pairs {
                    self.expr(key)?;
                    self.expr(value)?;
                }
                self.emit(Instr::Dict(pairs.len()));
                Ok(())
            }
            Expr::Contains(haystack, needle, line) => {
                self.expr(haystack)?;
                self.expr(needle)?;
                self.emit(Instr::Contains(*line as u32));
                Ok(())
            }
            other => Err(unsupported(self.line, expr_name(other))),
        }
    }
}

/// A list element must be a literal the compiler can fold into a constant.
/// Nested lists, tuples and dictionaries fold too, so `[{"a": 1}]` is one
/// constant rather than a construct the compiler has to refuse.
fn literal(expr: &Expr, line: usize) -> Result<Value, String> {
    match expr {
        Expr::Int(n) => Ok(Value::Int(*n)),
        Expr::Float(n) => Ok(Value::Float(*n)),
        // A folded string never goes through `Interpolate`, so a template
        // (a `{name}` placeholder, or an escaped brace) is refused loudly
        // rather than printed with its braces.
        Expr::StrLit(s) if s.contains(['{', '\\']) => {
            Err(unsupported(line, "စာသား ထည့်သွင်းရေးသားမှု (ဤ နေရာတွင်)"))
        }
        Expr::StrLit(s) => Ok(Value::Str(s.clone())),
        Expr::BoolLit(b) => Ok(Value::Bool(*b)),
        Expr::NullLit => Ok(Value::Null),
        Expr::ListLit(items) => {
            let mut values = Vec::with_capacity(items.len());
            for item in items {
                values.push(literal(item, line)?);
            }
            Ok(Value::List(values))
        }
        Expr::TupleLit(items) => {
            let mut values = Vec::with_capacity(items.len());
            for item in items {
                values.push(literal(item, line)?);
            }
            Ok(Value::Tuple(values))
        }
        Expr::DictLit(pairs) => {
            let mut map: IndexMap<Key, Value> = IndexMap::with_capacity(pairs.len());
            for (key, value) in pairs {
                map.insert(Key::new(literal(key, line)?), literal(value, line)?);
            }
            Ok(Value::Dict(Rc::new(map)))
        }
        other => Err(unsupported(line, expr_name(other))),
    }
}

fn stmt_name(stmt: &Stmt) -> &'static str {
    match stmt {
        Stmt::InputNoAssign { .. } | Stmt::InputAssign { .. } => "input",
        Stmt::RandomDecl { .. } => "ကျပန်း ကိန်းရှင်",
        Stmt::IndexAssign { .. } => "အညွှန်းသို့ သတ်မှတ်ခြင်း",
        Stmt::FuncCallAssign { .. } => "အလုပ် ရလဒ် သိမ်းခြင်း",
        Stmt::LibCall { .. } | Stmt::LibCallAssign { .. } => "နည်းပညာ ခေါ်ခြင်း",
        Stmt::FuncAlias { .. } => "အလုပ် အမည်လွှဲ",
        Stmt::UseLibrary { .. } => "နည်းပညာများ တင်သွင်းခြင်း",
        Stmt::EngDecl { .. } => "eng ကြေညာချက်",
        Stmt::EngPrint { .. } => "eng print",
        Stmt::EngInput { .. } | Stmt::EngInputAssign { .. } => "eng input",
        Stmt::EngExprCall { .. } => "eng အလုပ် ခေါ်ခြင်း",
        Stmt::EngAssign { .. } => "eng သတ်မှတ်ခြင်း",
        Stmt::EngMathAssign { .. } => "eng ပေါင်းထည့်ခြင်း",
        Stmt::Switch { .. } => "switch ရွေးချယ်ခြင်း",
        _ => "ဤ ဖော်ပြချက်",
    }
}

fn stmt_line(stmt: &Stmt) -> usize {
    match stmt {
        Stmt::VarDecl { line, .. }
        | Stmt::Print { line, .. }
        | Stmt::ExprStmt { line, .. }
        | Stmt::MathAssign { line, .. }
        | Stmt::IndexAssign { line, .. }
        | Stmt::FuncCall { line, .. }
        | Stmt::ForLoop { line, .. }
        | Stmt::While { line, .. }
        | Stmt::Switch { line, .. }
        | Stmt::FuncDef { line, .. }
        | Stmt::InputNoAssign { line, .. }
        | Stmt::InputAssign { line, .. }
        | Stmt::UseLibrary { line, .. }
        | Stmt::ClassDef { line, .. }
        | Stmt::SelfFieldSet { line, .. } => *line,
        _ => 0,
    }
}

fn expr_name(expr: &Expr) -> &'static str {
    match expr {
        Expr::Ternary(..) => "သုံးမျိုး ရွေးချယ်မှု (? :)",
        Expr::Lambda { .. } => "lambda",
        Expr::NewObj(..) => "class",
        Expr::LibCall(..) | Expr::LibFnRef(..) => "နည်းပညာ ခေါ်ခြင်း",
        Expr::FieldAccess(..) => "အကွက် ဖတ်ခြင်း",
        Expr::DictLit(..) => "အဘိဓာန်",
        Expr::SetLit(..) => "အစု (set)",
        Expr::Contains(..) => "ပါဝင်မှု စစ်ဆေးခြင်း",
        Expr::Logical(..) => "တန်ဖိုး and/or",
        _ => "ဤ ဖော်ပြချက်",
    }
}

fn e096(line: usize, name: &str) -> String {
    format!(
        "E096 လိုင်း {} တွင် \"{}\" ပုံသေ၏ ခေါင်းစီးတွင် တည်ဆောက်သူ အချက် များ ရေးထားပြီးဖြစ်၍ ပထမ လုပ်ဆောင်ချက်သည် အချက် များ ထပ်မံမရေးရပါ။",
        line, name
    )
}

/// Register every class by name: its constructor parameters come from the
/// header's `(params)`, or from the first method when the header has none --
/// exactly the rule the interpreter applies.
fn register_classes(stmts: &[Stmt], compiler: &mut Compiler) -> Result<(), String> {
    for stmt in collect_classes(stmts) {
        let Stmt::ClassDef {
            name,
            params,
            body,
            line,
        } = stmt
        else {
            unreachable!("collect_classes yields only class definitions")
        };
        let Some(Stmt::FuncDef {
            params: first_params,
            ..
        }) = body.first()
        else {
            return Err(format!(
                "E028 လိုင်း {} တွင် \"{}\" ပုံသေသည် တည်ဆောက်သူ လုပ်ဆောင်ချက် အနည်းဆုံး တစ်ခု လိုအပ်ပါသည်။",
                line, name
            ));
        };
        if params.is_some() && !first_params.is_empty() {
            return Err(e096(*line, name));
        }
        if compiler.class_slots.contains_key(name) {
            continue;
        }
        let ctor_params = params.clone().unwrap_or_else(|| first_params.clone());
        compiler.class_slots.insert(name.clone(), compiler.classes.len());
        compiler.classes.push(ClassDef {
            name: name.clone(),
            params: ctor_params,
            code: Vec::new(),
        });
    }
    Ok(())
}

/// Every class definition, in source order (including ones written inside a
/// block).
fn collect_classes(stmts: &[Stmt]) -> Vec<&Stmt> {
    fn walk<'a>(stmts: &'a [Stmt], out: &mut Vec<&'a Stmt>) {
        for stmt in stmts {
            match stmt {
                Stmt::ClassDef { .. } => out.push(stmt),
                Stmt::If { branches } => {
                    for branch in branches {
                        walk(&branch.body, out);
                    }
                }
                Stmt::While { body, .. } | Stmt::ForLoop { body, .. } => walk(body, out),
                Stmt::Switch { arms, .. } => {
                    for arm in arms {
                        walk(&arm.body, out);
                    }
                }
                _ => {}
            }
        }
    }
    let mut out = Vec::new();
    walk(stmts, &mut out);
    out
}

/// Compile a parsed program into bytecode.
pub fn compile(stmts: &[Stmt]) -> Result<Program, String> {
    let mut compiler = Compiler::new();
    // A default parameter is filled in by the interpreter when the call runs,
    // in the callee's own frame, and this backend binds parameters only from
    // the arguments the call supplies. Refuse the program instead of running
    // it with a parameter left unbound.
    // Pass one: every function and class name gets a slot, so a call compiled
    // before the callee's body is written still finds it.
    register_functions(stmts, &mut compiler);
    register_classes(stmts, &mut compiler)?;

    // Pass two: compile each body into its slot, then the top-level chunk.
    for (index, def) in collect_defs(stmts).into_iter().enumerate() {
        let Stmt::FuncDef {
            params,
            defaults,
            body,
            ..
        } = def
        else {
            unreachable!("collect_defs yields only function definitions")
        };
        let saved_code = std::mem::take(&mut compiler.code);
        let saved_loops = std::mem::take(&mut compiler.loops);
        let saved_handlers = compiler.handlers;
        compiler.handlers = 0;
        compiler.fill_defaults(params, defaults)?;
        compile_body(&mut compiler, body)?;
        let code = std::mem::replace(&mut compiler.code, saved_code);
        compiler.loops = saved_loops;
        compiler.handlers = saved_handlers;
        compiler.funcs[index].code = code;
    }

    // Pass three: each class's constructor body (its first method).
    for (index, class) in collect_classes(stmts).into_iter().enumerate() {
        let Stmt::ClassDef { body, .. } = class else {
            unreachable!("collect_classes yields only class definitions")
        };
        let Some(Stmt::FuncDef {
            body: ctor_body, ..
        }) = body.first()
        else {
            continue;
        };
        let saved_code = std::mem::take(&mut compiler.code);
        let saved_loops = std::mem::take(&mut compiler.loops);
        let saved_handlers = compiler.handlers;
        compiler.handlers = 0;
        compile_body(&mut compiler, ctor_body)?;
        let code = std::mem::replace(&mut compiler.code, saved_code);
        compiler.loops = saved_loops;
        compiler.handlers = saved_handlers;
        compiler.classes[index].code = code;
    }

    compiler.block(stmts)?;
    compiler.emit(Instr::Halt);
    Ok(Program {
        code: compiler.code,
        funcs: compiler.funcs,
        classes: compiler.classes,
    })
}

/// A function body's value is its last expression, the same rule the
/// interpreter uses: `... ပြီး။` returns the final bare expression, and a
/// body that ends in a declaration returns the value it just declared.
/// Anything else returns *no* value, which `ReturnVoid` records so a caller
/// that asked for one can raise `E071`.
fn compile_body(compiler: &mut Compiler, body: &[Stmt]) -> Result<(), String> {
    match body.split_last() {
        Some((Stmt::ExprStmt { value, .. }, rest)) => {
            compiler.block(rest)?;
            compiler.expr(value)?;
            compiler.emit(Instr::ReturnValue(0));
        }
        // `x သည် <expr> ဖြစ်၏။` as the last line: the name is assigned as
        // usual *and* its value is returned, as the interpreter does.
        Some((Stmt::VarDecl { name, value, .. }, rest)) => {
            compiler.block(rest)?;
            compiler.expr(value)?;
            compiler.emit(Instr::Store(name.to_string()));
            compiler.emit(Instr::Load(name.to_string(), compiler.line as u32));
            compiler.emit(Instr::ReturnValue(0));
        }
        Some((last, rest)) => {
            compiler.block(rest)?;
            compiler.stmt(last)?;
            compiler.emit(Instr::Const(Value::Null));
            compiler.emit(Instr::ReturnVoid(0));
        }
        None => {
            compiler.emit(Instr::Const(Value::Null));
            compiler.emit(Instr::ReturnVoid(0));
        }
    }
    Ok(())
}


fn register_functions(stmts: &[Stmt], compiler: &mut Compiler) {
    for stmt in stmts {
        match stmt {
            Stmt::FuncDef {
                name,
                params,
                defaults,
                ..
            } => {
                if !compiler.slots.contains_key(name) {
                    compiler.slots.insert(name.clone(), compiler.funcs.len());
                    let required = defaults.iter().filter(|d| d.is_none()).count();
                    compiler
                        .arities
                        .insert(name.clone(), (params.len(), required));
                    compiler.funcs.push(FuncDef {
                        name: name.clone(),
                        params: params.clone(),
                        required,
                        code: Vec::new(),
                    });
                }
            }
            Stmt::If { branches } => {
                for branch in branches {
                    register_functions(&branch.body, compiler);
                }
            }
            Stmt::While { body, .. } | Stmt::ForLoop { body, .. } => {
                register_functions(body, compiler);
            }
            Stmt::Switch { arms, .. } => {
                for arm in arms {
                    register_functions(&arm.body, compiler);
                }
            }
            _ => {}
        }
    }
}

/// Every `FuncDef` in source order, so pass two fills the slots registered in
/// pass one in the same order.
fn collect_defs(stmts: &[Stmt]) -> Vec<&Stmt> {
    fn walk<'a>(stmts: &'a [Stmt], out: &mut Vec<&'a Stmt>) {
        for stmt in stmts {
            match stmt {
                Stmt::FuncDef { .. } => out.push(stmt),
                Stmt::If { branches } => {
                    for branch in branches {
                        walk(&branch.body, out);
                    }
                }
                Stmt::While { body, .. } | Stmt::ForLoop { body, .. } => walk(body, out),
                Stmt::Switch { arms, .. } => {
                    for arm in arms {
                        walk(&arm.body, out);
                    }
                }
                _ => {}
            }
        }
    }
    let mut out = Vec::new();
    walk(stmts, &mut out);
    out
}

// ---------------------------------------------------------------------------
// Virtual machine
// ---------------------------------------------------------------------------

/// One live `try` region: where to resume when an error happens, and the shape
/// of the world at the moment the region started, so unwinding can restore it.
struct Handler {
    frames: usize,
    stack_len: usize,
    pc: usize,
}

struct Frame {
    func: usize,
    pc: usize,
    locals: HashMap<String, Value>,
    /// `Some(slot)` when this frame is a class constructor: `အတု <field> သည်
    /// ... ဖြစ်၏။` writes into `self_fields`, and the frame's `Return`
    /// produces the object.
    class: Option<usize>,
    self_fields: Vec<(String, Value)>,
    /// Set by `CallValue`: the `E071` message to raise if this body returns
    /// without a value.
    value_err: Option<String>,
}

/// One mutating builtin on an already-read list: the interpreter's own rules,
/// including its `E110`/`E049` messages and the value each call returns.
fn mutate_list(
    name: &str,
    items: &mut Vec<Value>,
    mut extra: Vec<Value>,
    line: usize,
) -> Result<Value, String> {
    use crate::interpreter::{eng_builtin_err, index_as_int, seq_slot, sort_items};
    match name {
        "push" | "append" => {
            if extra.len() != 1 {
                return Err(eng_builtin_err(
                    line,
                    name,
                    &format!(
                        "စာရင်း နှင့် တန်ဖိုး တစ်ခု လိုအပ်ပါသည်၊ တန်ဖိုး {} ခု ရပါသည်",
                        extra.len()
                    ),
                ));
            }
            items.push(extra.pop().expect("one value"));
            Ok(Value::Int(items.len() as i64))
        }
        "pop" => {
            if !extra.is_empty() {
                return Err(eng_builtin_err(
                    line,
                    name,
                    "စာရင်း တစ်ခုသာ လိုအပ်ပြီး အခြားအချက် မလိုပါ",
                ));
            }
            match items.pop() {
                Some(v) => Ok(v),
                None => Err(eng_builtin_err(line, name, "စာရင်းအလွတ်မှ ဖယ်၍မရပါ")),
            }
        }
        "insert" => {
            if extra.len() != 2 {
                return Err(eng_builtin_err(
                    line,
                    name,
                    &format!(
                        "စာရင်း၊ အညွှန်း နှင့် တန်ဖိုး တစ်ခု လိုအပ်ပါသည်၊ စာရင်းနောက်တွင် အချက် {} ခု ရပါသည်",
                        extra.len()
                    ),
                ));
            }
            let value = extra.pop().expect("the value");
            let at = index_as_int(&extra[0], line)?;
            if at < 0 || at as usize > items.len() {
                return Err(format!(
                    "E049 လိုင်း {} တွင် အညွှန်း {} သည် အကွာအဝေးပြင်ပတွင် ရှိနေပါသည်။",
                    line, at
                ));
            }
            items.insert(at as usize, value);
            Ok(Value::Int(items.len() as i64))
        }
        "remove" => {
            if extra.len() != 1 {
                return Err(eng_builtin_err(
                    line,
                    name,
                    &format!(
                        "စာရင်း နှင့် အညွှန်း တစ်ခု လိုအပ်ပါသည်၊ အညွှန်း {} ခု ရပါသည်",
                        extra.len()
                    ),
                ));
            }
            let at = index_as_int(&extra[0], line)?;
            let slot = seq_slot(items.len(), at, line)?;
            Ok(items.remove(slot))
        }
        "sort" => {
            if !extra.is_empty() {
                return Err(eng_builtin_err(
                    line,
                    name,
                    "စာရင်း တစ်ခုသာ လိုအပ်ပြီး အခြားအချက် မလိုပါ",
                ));
            }
            sort_items(items, line, name)?;
            Ok(Value::List(items.clone()))
        }
        other => Err(crate::interpreter::lib_fn_unknown_err(other, other, line)),
    }
}

/// What a callback call did: returned a value, or left the call some other
/// way (an error was handed to a handler below it, or the program halted).
enum CallbackOutcome {
    Value(Value),
    Left,
}

/// One line of program output, rendered by the interpreter's own `display`,
/// so a printed dictionary (or list) reads exactly as it does there.
fn render(value: &Value) -> String {
    crate::interpreter::display(value)
}

fn truthy(value: &Value) -> Result<bool, String> {
    match value {
        Value::Bool(b) => Ok(*b),
        _ => Err(bool_expected(0)),
    }
}

/// `bool_operand` from the interpreter, with the same `E043` text: a value
/// used where a condition is required must be a bool.
fn bool_operand_vm(value: &Value, line: usize) -> Result<bool, String> {
    match value {
        Value::Bool(b) => Ok(*b),
        other => Err(format!(
            "E043 လိုင်း {} တွင် {} တန်ဖိုးကို အခြေအနေအဖြစ် (မှန်/မှား စစ်ရန်) သုံး၍မရပါ။",
            line,
            crate::interpreter::type_name_mm(other)
        )),
    }
}

/// The error for a call whose name resolves to nothing. Each of the
/// interpreter's call paths has its own code, so the mode picks it.
fn missing_callee(name: &str, line: usize, mode: u8) -> String {
    match mode {
        // `... ကို လုပ်ပါ။` falls through to the missing-function error.
        1 => crate::interpreter::eng_missing_function_err(line, name),
        // The eng `name(args);` statement has its own error code.
        2 => format!(
            "E113 လိုင်း {} တွင် \"{}\" သည် အလုပ် သို့မဟုတ် builtin မဟုတ်ပါ။",
            line, name
        ),
        // An expression ends up in the class-not-found path.
        _ => format!(
            "E032 လိုင်း {} တွင် \"{}\" ဆိုသော ပုံသေကို ရှာမတွေ့ပါ။",
            line, name
        ),
    }
}

/// Reverse a value the way the interpreter's `reverse` does: a string by
/// clusters, a list or tuple by element. Anything else is `E111`.
fn reverse_value(value: Value, line: usize) -> Result<Value, String> {
    match value {
        Value::Str(s) => Ok(Value::Str(crate::text::reverse(&s))),
        Value::List(mut items) => {
            items.reverse();
            Ok(Value::List(items))
        }
        Value::Tuple(mut items) => {
            items.reverse();
            Ok(Value::Tuple(items))
        }
        other => Err(crate::interpreter::eng_builtin_err(
            line,
            "reverse",
            &format!(
                "စာရင်း သို့မဟုတ် စာသား လိုအပ်ပါသည်၊ {} ရပါသည်",
                crate::interpreter::type_name_mm(&other)
            ),
        )),
    }
}

/// The result of `+ - *`: an integer when both operands were integers, a float
/// otherwise -- the type the interpreter keeps for those operations.
fn number(value: f64, both_int: bool) -> Value {
    if both_int {
        Value::Int(value as i64)
    } else {
        Value::Float(value)
    }
}

fn numbers(a: &Value, b: &Value) -> Option<(f64, f64)> {
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => Some((*x as f64, *y as f64)),
        (Value::Int(x), Value::Float(y)) => Some((*x as f64, *y)),
        (Value::Float(x), Value::Int(y)) => Some((*x, *y as f64)),
        (Value::Float(x), Value::Float(y)) => Some((*x, *y)),
        _ => None,
    }
}

fn values_equal(a: &Value, b: &Value) -> bool {
    value_eq(a, b)
}

/// Indexed writes and reads go through the interpreter's own `set_index` and
/// `index_value` (see [`Instr::Index`] / [`Instr::IndexStore`]), so a dict
/// write, a missing key and an out-of-range list index all fail with the very
/// same `E0xx` text in both backends.
/// An indexed write runs the interpreter's own `set_index`, so a dict write, a
/// wrong-arity key path and an out-of-range list slot fail with the very same
/// `E0xx` text in both backends.
fn set_index_vm(base: &mut Value, keys: &[Value], value: Value, line: usize) -> Result<(), String> {
    crate::interpreter::set_index(base, keys, value, line)
}

/// The stack machine. [`Vm::run`] executes until `Halt`.
pub struct Vm<'p> {
    prog: &'p Program,
    stack: Vec<Value>,
    globals: HashMap<String, Value>,
    frames: Vec<Frame>,
    main_pc: usize,
    /// Everything the program printed, one entry per line.
    pub out: Vec<String>,
    /// Write output to stdout as it is produced, instead of collecting it in
    /// `out` for after the run. A real run streams, so a prompt is on screen
    /// before the read blocks.
    stream: bool,
    /// The names declared with `pin`, so a later write is refused with the
    /// interpreter's `E104`. The interpreter keeps one set for the whole run,
    /// so this does too.
    decls: HashMap<String, ()>,
    /// The live `try` regions, innermost last.
    handlers: Vec<Handler>,
    /// The libraries imported so far, by every name they answer to.
    libs: std::collections::HashSet<String>,
    /// The message of the error a handler is currently running for.
    pending_error: Option<String>,
    /// Set when the program has stopped (its `Halt`, or a `Return` out of the
    /// top-level chunk), so the run loop can tell that from an error.
    halted: bool,
    /// Everything the program wrote, byte for byte -- the `.akop` transcript.
    pub log: String,
    /// A scripted stdin (tests); `None` reads the process's own stdin.
    input: Option<Box<dyn std::io::BufRead>>,
    /// Set while a nested run is going: it stops as soon as the frame stack
    /// drops back to this depth, so the callback returns to its caller.
    stop_depth: Option<usize>,
}

impl<'p> Vm<'p> {
    pub fn new(prog: &'p Program) -> Self {
        Vm {
            prog,
            stack: Vec::new(),
            globals: HashMap::new(),
            frames: Vec::new(),
            main_pc: 0,
            out: Vec::new(),
            stream: false,
            decls: HashMap::new(),
            handlers: Vec::new(),
            libs: std::collections::HashSet::new(),
            pending_error: None,
            halted: false,
            log: String::new(),
            input: None,
            stop_depth: None,
        }
    }

    /// A VM that writes to stdout as the program runs, for `akk --vm` and for
    /// a built bytecode artifact.
    pub fn streaming(prog: &'p Program) -> Self {
        let mut vm = Vm::new(prog);
        vm.stream = true;
        vm
    }

    /// Feed this VM scripted stdin instead of the process's own.
    #[cfg(test)]
    pub fn feed(&mut self, text: &str) {
        self.input = Some(Box::new(std::io::Cursor::new(text.as_bytes().to_vec())));
    }

    /// Show program text: always recorded in `log`, and written to stdout now
    /// when the VM is streaming (a prompt must precede the read it belongs to).
    fn write_out(&mut self, text: &str) {
        self.log.push_str(text);
        if self.stream {
            use std::io::Write;
            let stdout = std::io::stdout();
            let mut lock = stdout.lock();
            let _ = lock.write_all(text.as_bytes());
            let _ = lock.flush();
        }
    }

    /// One line of stdin, without its line ending. EOF reads as an empty line,
    /// which is what the interpreter's `read_line` leaves behind.
    fn read_input(&mut self) -> String {
        let mut buf = String::new();
        match self.input.as_mut() {
            Some(reader) => {
                let _ = reader.read_line(&mut buf);
            }
            None => {
                let _ = std::io::stdin().read_line(&mut buf);
            }
        }
        buf
    }

    fn pop(&mut self) -> Value {
        self.stack.pop().expect("stack underflow")
    }

    fn load(&self, name: &str, line: usize) -> Result<Value, String> {
        // The interpreter's scope chain is dynamic: a callee sees the frames
        // of its callers, so a function may read a caller's local. Walk the
        // frames from innermost outward, then the globals.
        for frame in self.frames.iter().rev() {
            if let Some(v) = frame.locals.get(name) {
                return Ok(v.clone());
            }
        }
        self.globals
            .get(name)
            .cloned()
            .ok_or_else(|| unknown_var(name, line))
    }

    /// Pop the frame a `return` is leaving and hand `value` back to the
    /// caller. `produced` says whether the body actually yielded a value; when
    /// the call demanded one (`CallValue`) and it did not, that is `E071`,
    /// raised exactly where the interpreter raises it.
    fn leave_frame(&mut self, value: Value, line: u32, produced: bool) -> Result<(), String> {
        if self.frames.is_empty() {
            // `return` outside any function is `E108`, exactly as the
            // interpreter reports it.
            return Err(crate::interpreter::eng_return_outside_fn_err(
                line as usize,
            ));
        }
        let frame = self.frames.pop().expect("a frame was just checked");
        if !produced {
            if let Some(err) = frame.value_err {
                return Err(err);
            }
        }
        // A constructor's result is the object it filled in, not whatever its
        // last expression evaluated to.
        let value = match frame.class {
            Some(index) => Value::Object(Rc::new(ObjectValue {
                class: self.prog.classes[index].name.clone(),
                fields: frame.self_fields,
            })),
            None => value,
        };
        // Back in the caller, right after its call.
        self.stack.push(value);
        Ok(())
    }

    /// The items `map`/`filter`/`reduce` walk, with the interpreter's own
    /// `E110` for a value that cannot be walked.
    fn iter_items(kind: u8, collection: &Value, line: usize) -> Result<Vec<Value>, String> {
        match collection {
            Value::List(items) | Value::Tuple(items) => Ok(items.clone()),
            Value::Set(items) => Ok(items.iter().map(|k| k.value().clone()).collect()),
            Value::Dict(pairs) => Ok(pairs.keys().map(|k| k.value().clone()).collect()),
            Value::Str(text) => Ok(crate::text::clusters(text)
                .into_iter()
                .map(|c| Value::Str(c.to_string()))
                .collect()),
            other => Err(crate::interpreter::eng_builtin_err(
                line,
                higher_order_name(kind),
                &format!(
                    "စုစည်းမှု သို့မဟုတ် စာသား လိုအပ်ပါသည်၊ {} ရပါသည်",
                    crate::interpreter::type_name_mm(other)
                ),
            )),
        }
    }

    /// Run the callback function `index` with `args`, the way the interpreter's
    /// `call_callback` does: a fresh frame, the interpreter's `E033` for the
    /// wrong argument count, and `err` (its `E114`) when the body returns
    /// nothing. `Left` says control did not come back from the call -- an
    /// error was handed to a `try` handler below it, or the program halted --
    /// so the caller must not touch the stack.
    /// Start a call: check the argument count, push a fresh frame and bind the
    /// parameters. `next_pc` is where the caller resumes once the callee
    /// returns; `value_err` (when present) turns a body that yields nothing
    /// into the interpreter's `E114`.
    fn enter_call(
        &mut self,
        index: usize,
        args: Vec<Value>,
        value_err: Option<String>,
        line: u32,
        next_pc: usize,
    ) -> Result<(), String> {
        let func = &self.prog.funcs[index];
        let name = func.name.clone();
        let params = func.params.clone();
        let required = func.required;
        if args.len() < required || args.len() > params.len() {
            return Err(crate::interpreter::eng_arity_err(
                &name,
                args.len(),
                required,
                params.len(),
                line as usize,
            ));
        }
        self.set_pc(next_pc);
        self.frames.push(Frame {
            func: index,
            pc: 0,
            locals: HashMap::new(),
            class: None,
            self_fields: Vec::new(),
            value_err,
        });
        self.bind_params(&params, args);
        Ok(())
    }

    /// Bind a call's parameters with the interpreter's `bind_param` rule: a
    /// name that has no binding yet is written into the shared environment
    /// without a frame log entry, so it outlives the call (the interpreter's
    /// own quirk, which makes a returned lambda that reads a parameter work);
    /// a name that already has a binding is restored when the frame pops.
    fn bind_params(&mut self, params: &[String], args: Vec<Value>) {
        for (name, value) in params.iter().zip(args) {
            let existed = self
                .frames
                .iter()
                .rev()
                .any(|frame| frame.locals.contains_key(name))
                || self.globals.contains_key(name);
            if !existed {
                self.globals.insert(name.clone(), value.clone());
            }
            self.frames
                .last_mut()
                .expect("the frame was just pushed")
                .locals
                .insert(name.clone(), value);
        }
    }

    /// Call the callable `name` holds, after [`Instr::CallableGuard`] checked
    /// that it is one. The arguments are already evaluated, as the
    /// interpreter's `invoke_callable` has them by this point.
    fn call_dynamic(
        &mut self,
        name: &str,
        args: Vec<Value>,
        line: u32,
        mode: u8,
    ) -> Result<(), String> {
        let value = match self.load(name, line as usize) {
            Ok(value) => value,
            Err(_) => return Err(missing_callee(name, line as usize, mode)),
        };
        let callable = match value {
            Value::Callable(kind) => kind,
            _ => return Err(missing_callee(name, line as usize, mode)),
        };
        let (index, label) = match &*callable {
            Callable::Compiled { index, label } => (*index, label.clone()),
            other => {
                return Err(format!(
                    "E170 လိုင်း {} တွင် \"{}\" ကို bytecode စက် ဖြင့် ခေါ်၍မရပါ။",
                    line,
                    other.label()
                ))
            }
        };
        // Every callable call wants a value: the interpreter's
        // `invoke_callable` raises `E114` when the body yields none, even
        // for a statement whose result is discarded.
        let value_err = Some(crate::interpreter::eng_no_return_err(
            &label,
            line as usize,
        ));
        // The arm already moved past the call instruction, so the caller
        // resumes exactly where the pc points now.
        let next_pc = self.current_pc();
        self.enter_call(index, args, value_err, line, next_pc)
    }

    fn call_callback(
        &mut self,
        kind: &Callable,
        args: Vec<Value>,
        line: u32,
    ) -> Result<CallbackOutcome, String> {
        let (index, label) = match kind {
            Callable::Compiled { index, label } => (*index, label.clone()),
            other => {
                return Err(format!(
                    "E170 လိုင်း {} တွင် \"{}\" ကို bytecode စက် ဖြင့် ခေါ်၍မရပါ။",
                    line,
                    other.label()
                ))
            }
        };
        let func = &self.prog.funcs[index];
        let params = func.params.clone();
        let required = func.required;
        if args.len() < required || args.len() > params.len() {
            return Err(crate::interpreter::eng_arity_err(
                &label,
                args.len(),
                required,
                params.len(),
                line as usize,
            ));
        }
        let base = self.frames.len();
        // The stack before the frame: a normal return leaves exactly the one
        // value it produced on top of it, which is how a callback's own
        // return is told from control leaving the call another way (an error
        // handed to a `try` handler below it, or a halt).
        let stack_base = self.stack.len();
        self.frames.push(Frame {
            func: index,
            pc: 0,
            locals: HashMap::new(),
            class: None,
            self_fields: Vec::new(),
            value_err: Some(crate::interpreter::eng_no_return_err(
                &label,
                line as usize,
            )),
        });
        self.bind_params(&params, args);
        let saved_stop = self.stop_depth;
        self.stop_depth = Some(base);
        let result = self.run();
        self.stop_depth = saved_stop;
        result?;
        if self.frames.len() != base || self.stack.len() != stack_base + 1 {
            return Ok(CallbackOutcome::Left);
        }
        Ok(CallbackOutcome::Value(self.pop()))
    }

    fn store(&mut self, name: &str, value: Value) {
        match self.frames.last_mut() {
            Some(frame) => {
                frame.locals.insert(name.to_string(), value);
            }
            None => {
                self.globals.insert(name.to_string(), value);
            }
        }
    }

    /// Hand `message` to the innermost live `try` handler.
    ///
    /// Returns `true` when a handler took over: the frames opened inside the
    /// region are dropped, the value stack is cut back to its depth at the
    /// `try`, the message is kept for `catch`/`ReThrow`, and execution resumes
    /// at the handler. Returns `false` when no handler is left, and the caller
    /// reports the error as uncaught.
    fn unwind(&mut self, message: &str) -> bool {
        let Some(handler) = self.handlers.pop() else {
            return false;
        };
        while self.frames.len() > handler.frames {
            self.frames.pop();
        }
        self.stack.truncate(handler.stack_len);
        self.pending_error = Some(message.to_string());
        self.set_pc(handler.pc);
        true
    }

    fn current_pc(&self) -> usize {
        match self.frames.last() {
            Some(frame) => frame.pc,
            None => self.main_pc,
        }
    }

    fn set_pc(&mut self, pc: usize) {
        match self.frames.last_mut() {
            Some(frame) => frame.pc = pc,
            None => self.main_pc = pc,
        }
    }

    fn arith(&mut self, op: char, line: u32) -> Result<(), String> {
        let rhs = self.pop();
        let lhs = self.pop();
        if op == '+' {
            if let (Value::Str(a), Value::Str(b)) = (&lhs, &rhs) {
                self.stack.push(Value::Str(format!("{}{}", a, b)));
                return Ok(());
            }
        }
        let Some((a, b)) = numbers(&lhs, &rhs) else {
            return Err(math_type_err(op));
        };
        let both_int = matches!(lhs, Value::Int(_)) && matches!(rhs, Value::Int(_));
        let line = line as usize;
        // The interpreter's own rules: integer `+ - *` stay integers, `/`
        // always yields a float, and `%` is the Euclidean remainder. A zero
        // divisor is `E036`/`E038` for integers and `E037`/`E039` for floats,
        // each with the line the operation was written on.
        let value = match op {
            '+' => number(a + b, both_int),
            '-' => number(a - b, both_int),
            '*' => number(a * b, both_int),
            '/' => {
                if b == 0.0 {
                    return Err(if both_int {
                        format!("E036 လိုင်း {} တွင် သုညဖြင့် စား၍မရပါ။", line)
                    } else {
                        format!("E037 လိုင်း {} တွင် သုညဖြင့် စား၍မရပါ။", line)
                    });
                }
                Value::Float(a / b)
            }
            '%' => {
                if b == 0.0 {
                    return Err(if both_int {
                        format!(
                            "E038 လိုင်း {} တွင် သုညဖြင့် ကြွင်းကိန်းရှာ၍မရပါ။",
                            line
                        )
                    } else {
                        format!(
                            "E039 လိုင်း {} တွင် သုညဖြင့် ကြွင်းကိန်းရှာ၍မရပါ။",
                            line
                        )
                    });
                }
                if both_int {
                    Value::Int((a as i64).rem_euclid(b as i64))
                } else {
                    Value::Float(a.rem_euclid(b))
                }
            }
            other => return Err(math_type_err(other)),
        };
        self.stack.push(value);
        Ok(())
    }

    fn compare(&mut self, want: fn(std::cmp::Ordering) -> bool) -> Result<(), String> {
        let rhs = self.pop();
        let lhs = self.pop();
        let ordering = match (&lhs, &rhs) {
            (Value::Str(a), Value::Str(b)) => a.cmp(b),
            _ => {
                let Some((a, b)) = numbers(&lhs, &rhs) else {
                    return Err("E172 တွင် နှိုင်းယှဉ်၍မရပါ။".to_string());
                };
                a.partial_cmp(&b).unwrap_or(std::cmp::Ordering::Equal)
            }
        };
        self.stack.push(Value::Bool(want(ordering)));
        Ok(())
    }

    /// Run to `Halt` (or off the end of the main chunk).
    pub fn run(&mut self) -> Result<(), String> {
        loop {
            if let Some(depth) = self.stop_depth {
                if self.frames.len() <= depth {
                    return Ok(());
                }
            }
            let pc = self.current_pc();
            let instr = {
                let code = match self.frames.last() {
                    Some(frame) => match frame.class {
                        Some(slot) => &self.prog.classes[slot].code,
                        None => &self.prog.funcs[frame.func].code,
                    },
                    None => &self.prog.code,
                };
                if pc >= code.len() {
                    return Ok(());
                }
                code[pc].clone()
            };
            // Each instruction runs in its own closure, so an error it
            // returns can be handed to a `try` handler instead of ending
            // the run. The arms keep using `?` and `return Err`, which is
            // what makes them report the interpreter's own text.
            let outcome: Result<(), String> = (|| {
                match instr {
                Instr::Const(v) => {
                    self.stack.push(v);
                    self.set_pc(pc + 1);
                }
                Instr::Load(name, line) => {
                    let v = self.load(&name, line as usize)?;
                    self.stack.push(v);
                    self.set_pc(pc + 1);
                }
                Instr::Store(name) => {
                    let v = self.pop();
                    self.store(&name, v);
                    self.set_pc(pc + 1);
                }
                Instr::Destructure(names, line) => {
                    let value = self.pop();
                    let items = match &value {
                        Value::List(items) | Value::Tuple(items) => items.clone(),
                        other => {
                            return Err(crate::interpreter::destructure_not_collection_err(
                                line as usize,
                                crate::interpreter::type_name_mm(other),
                            ))
                        }
                    };
                    if items.len() != names.len() {
                        return Err(crate::interpreter::destructure_count_err(
                            line as usize,
                            names.len(),
                            items.len(),
                        ));
                    }
                    for (name, item) in names.iter().zip(items.into_iter()) {
                        if self.decls.contains_key(name) {
                            return Err(crate::interpreter::eng_const_reassign_err(
                                name,
                                line as usize,
                            ));
                        }
                        self.store(name, item);
                    }
                    self.set_pc(pc + 1);
                }
                Instr::Pop => {
                    self.pop();
                    self.set_pc(pc + 1);
                }
                Instr::Add => {
                    self.arith('+', 0)?;
                    self.set_pc(pc + 1);
                }
                Instr::Sub => {
                    self.arith('-', 0)?;
                    self.set_pc(pc + 1);
                }
                Instr::Mul => {
                    self.arith('*', 0)?;
                    self.set_pc(pc + 1);
                }
                Instr::Div(line) => {
                    self.arith('/', line)?;
                    self.set_pc(pc + 1);
                }
                Instr::Mod(line) => {
                    self.arith('%', line)?;
                    self.set_pc(pc + 1);
                }
                Instr::Neg => {
                    let v = self.pop();
                    let value = match v {
                        Value::Int(n) => Value::Int(-n),
                        Value::Float(f) => Value::Float(-f),
                        _ => return Err("E172 တွင် အနုတ်လက္ခဏာ မထည့်နိုင်ပါ။".to_string()),
                    };
                    self.stack.push(value);
                    self.set_pc(pc + 1);
                }
                Instr::Not(line) => {
                    let v = self.pop();
                    let b = bool_operand_vm(&v, line as usize)?;
                    self.stack.push(Value::Bool(!b));
                    self.set_pc(pc + 1);
                }
                Instr::ToBool(line) => {
                    let v = self.pop();
                    let b = bool_operand_vm(&v, line as usize)?;
                    self.stack.push(Value::Bool(b));
                    self.set_pc(pc + 1);
                }
                Instr::AndJump(target, line) => {
                    let v = self.pop();
                    if bool_operand_vm(&v, line as usize)? {
                        self.set_pc(pc + 1);
                    } else {
                        self.stack.push(Value::Bool(false));
                        self.set_pc(target);
                    }
                }
                Instr::OrJump(target, line) => {
                    let v = self.pop();
                    if bool_operand_vm(&v, line as usize)? {
                        self.stack.push(Value::Bool(true));
                        self.set_pc(target);
                    } else {
                        self.set_pc(pc + 1);
                    }
                }
                Instr::MakeFunc(index) => {
                    let label = self.prog.funcs[index].name.clone();
                    self.stack.push(Value::Callable(Rc::new(Callable::Compiled {
                        index,
                        label,
                    })));
                    self.set_pc(pc + 1);
                }
                Instr::LoadFn(name, index, line) => {
                    // A live variable wins over the function of the same name,
                    // exactly as the interpreter's `Expr::Ident` says.
                    let value = match self.load(&name, line as usize) {
                        Ok(value) => value,
                        Err(_) => Value::Callable(Rc::new(Callable::Compiled {
                            index,
                            label: name,
                        })),
                    };
                    self.stack.push(value);
                    self.set_pc(pc + 1);
                }
                Instr::CallableGuard(name, line, mode) => {
                    let value = match self.load(&name, line as usize) {
                        Ok(value) => value,
                        Err(_) => return Err(missing_callee(&name, line as usize, mode)),
                    };
                    if !matches!(value, Value::Callable(_)) {
                        return Err(missing_callee(&name, line as usize, mode));
                    }
                    self.set_pc(pc + 1);
                }
                Instr::CallDyn(name, argc, line, mode) => {
                    let mut args = Vec::with_capacity(argc as usize);
                    for _ in 0..argc {
                        args.push(self.pop());
                    }
                    args.reverse();
                    self.set_pc(pc + 1);
                    self.call_dynamic(&name, args, line, mode)?;
                }
                Instr::Reverse(var, line) => {
                    let value = self.pop();
                    let in_place = matches!(value, Value::List(_));
                    let reversed = reverse_value(value, line as usize)?;
                    if in_place {
                        if let Some(name) = var {
                            if self.decls.contains_key(&name) {
                                return Err(crate::interpreter::eng_const_reassign_err(
                                    &name,
                                    line as usize,
                                ));
                            }
                            self.store(&name, reversed.clone());
                        }
                    }
                    self.stack.push(reversed);
                    self.set_pc(pc + 1);
                }
                Instr::And => {
                    let b = truthy(&self.pop())?;
                    let a = truthy(&self.pop())?;
                    self.stack.push(Value::Bool(a && b));
                    self.set_pc(pc + 1);
                }
                Instr::Or => {
                    let b = truthy(&self.pop())?;
                    let a = truthy(&self.pop())?;
                    self.stack.push(Value::Bool(a || b));
                    self.set_pc(pc + 1);
                }
                Instr::Eq => {
                    let b = self.pop();
                    let a = self.pop();
                    self.stack.push(Value::Bool(values_equal(&a, &b)));
                    self.set_pc(pc + 1);
                }
                Instr::Ne => {
                    let b = self.pop();
                    let a = self.pop();
                    self.stack.push(Value::Bool(!values_equal(&a, &b)));
                    self.set_pc(pc + 1);
                }
                Instr::Lt => {
                    self.compare(|o| o == std::cmp::Ordering::Less)?;
                    self.set_pc(pc + 1);
                }
                Instr::Le => {
                    self.compare(|o| o != std::cmp::Ordering::Greater)?;
                    self.set_pc(pc + 1);
                }
                Instr::Gt => {
                    self.compare(|o| o == std::cmp::Ordering::Greater)?;
                    self.set_pc(pc + 1);
                }
                Instr::Ge => {
                    self.compare(|o| o != std::cmp::Ordering::Less)?;
                    self.set_pc(pc + 1);
                }
                Instr::JumpIfTrue(target) => {
                    let v = self.pop();
                    self.set_pc(if truthy(&v)? { target } else { pc + 1 });
                }
                Instr::JumpIfFalse(target) => {
                    let v = self.pop();
                    self.set_pc(if truthy(&v)? { pc + 1 } else { target });
                }
                Instr::Jump(target) => self.set_pc(target),
                Instr::Print => {
                    let v = self.pop();
                    let line = render(&v);
                    self.out.push(line.clone());
                    self.write_out(&format!("{}\n", line));
                    self.set_pc(pc + 1);
                }
                Instr::Prompt => {
                    let v = self.pop();
                    let text = render(&v);
                    self.write_out(&text);
                    self.set_pc(pc + 1);
                }
                Instr::Read => {
                    let line = self.read_input();
                    let value = crate::interpreter::infer_value(line.trim());
                    self.stack.push(value);
                    self.set_pc(pc + 1);
                }
                Instr::ReadTyped(type_name, line) => {
                    let text = self.read_input();
                    let value = crate::interpreter::coerce_eng_input(
                        text.trim(),
                        &type_name,
                        line as usize,
                    )?;
                    self.stack.push(value);
                    self.set_pc(pc + 1);
                }
                Instr::Declare(name, type_name, is_const, line) => {
                    let value = self.pop();
                    let value = crate::interpreter::check_eng_value(
                        &name,
                        &type_name,
                        value,
                        line as usize,
                    )?;
                    if is_const {
                        let exists = self.decls.contains_key(&name)
                            || self.globals.contains_key(&name)
                            || self
                                .frames
                                .last()
                                .map(|frame| frame.locals.contains_key(&name))
                                .unwrap_or(false);
                        if exists {
                            return Err(crate::interpreter::eng_pin_redeclare_err(
                                &name,
                                line as usize,
                            ));
                        }
                        self.decls.insert(name.clone(), ());
                    }
                    self.store(&name, value);
                    self.set_pc(pc + 1);
                }
                Instr::Builtin(name, argc, line) => {
                    let mut args = Vec::with_capacity(argc as usize);
                    for _ in 0..argc {
                        args.push(self.pop());
                    }
                    args.reverse();
                    let value = Interpreter::builtin_value_result(&name, &args, line as usize)?;
                    self.stack.push(value);
                    self.set_pc(pc + 1);
                }
                Instr::UseLib(name) => {
                    match crate::library::builtin_names(&name) {
                        Some(names) => {
                            for spelling in names {
                                self.libs.insert((*spelling).to_string());
                            }
                        }
                        None => {
                            self.libs.insert(name);
                        }
                    }
                    self.set_pc(pc + 1);
                }
                Instr::RequireLib(name, line) => {
                    if !self.libs.contains(&name) {
                        return Err(crate::interpreter::lib_not_imported_err(
                            &name,
                            line as usize,
                        ));
                    }
                    self.set_pc(pc + 1);
                }
                Instr::LibCall(lib, fn_name, argc, line, err) => {
                    let mut args = Vec::with_capacity(argc as usize);
                    for _ in 0..argc {
                        args.push(self.pop());
                    }
                    args.reverse();
                    match crate::interpreter::lib_value_call(
                        &lib,
                        &fn_name,
                        &args,
                        line as usize,
                    )? {
                        Some(value) => self.stack.push(value),
                        None if err.is_empty() => self.stack.push(Value::Null),
                        None => return Err(err.clone()),
                    }
                    self.set_pc(pc + 1);
                }
                Instr::MarkConst(name) => {
                    self.decls.insert(name, ());
                    self.set_pc(pc + 1);
                }
                Instr::PinDeclareCheck(name, line) => {
                    let exists = self.decls.contains_key(&name)
                        || self.globals.contains_key(&name)
                        || self
                            .frames
                            .last()
                            .map(|frame| frame.locals.contains_key(&name))
                            .unwrap_or(false);
                    if exists {
                        return Err(crate::interpreter::eng_pin_redeclare_err(
                            &name,
                            line as usize,
                        ));
                    }
                    self.set_pc(pc + 1);
                }
                Instr::PinGuard(name, line) => {
                    if self.decls.contains_key(&name) {
                        return Err(crate::interpreter::eng_const_reassign_err(
                            &name,
                            line as usize,
                        ));
                    }
                    self.set_pc(pc + 1);
                }
                Instr::Abort(message) => return Err(message),
                Instr::Try(handler) => {
                    self.handlers.push(Handler {
                        frames: self.frames.len(),
                        stack_len: self.stack.len(),
                        pc: handler,
                    });
                    self.set_pc(pc + 1);
                }
                Instr::TryEnd => {
                    self.handlers.pop();
                    self.set_pc(pc + 1);
                }
                Instr::ErrorCode => {
                    let code = crate::interpreter::extract_error_code(
                        self.pending_error.as_deref().unwrap_or("E"),
                    );
                    self.stack.push(Value::Str(code));
                    self.set_pc(pc + 1);
                }
                Instr::ReThrow => {
                    let message = self.pending_error.take().unwrap_or_else(|| {
                        "E170 လိုင်း 0 တွင် မိမိဘာသာ ပြန်မတင်ႇနိူင်သော အမှားတစ်ခု ရှိနေပါသည်။"
                            .to_string()
                    });
                    return Err(message);
                }
                Instr::PopHandlers(count) => {
                    let keep = self.handlers.len().saturating_sub(count as usize);
                    self.handlers.truncate(keep);
                    self.set_pc(pc + 1);
                }
                Instr::Interpolate(line) => {
                    let value = self.pop();
                    let text = match value {
                        Value::Str(text) => text,
                        other => render(&other),
                    };
                    // The lookups only read the VM, so the borrow ends before
                    // the expanded string is pushed back on the stack.
                    let vm: &Vm = self;
                    let expanded = crate::interpreter::interpolate_text(
                        &text,
                        line as usize,
                        |name| {
                            vm.load(name, line as usize).map_err(|_| {
                                crate::interpreter::interpolate_missing_var_err(
                                    name,
                                    line as usize,
                                )
                            })
                        },
                        |field_name, whole| {
                            crate::interpreter::self_field_lookup(
                                vm.frames
                                    .last()
                                    .filter(|frame| frame.class.is_some())
                                    .map(|frame| frame.self_fields.as_slice()),
                                field_name,
                                whole,
                                line as usize,
                            )
                        },
                    )?;
                    self.stack.push(Value::Str(expanded));
                    self.set_pc(pc + 1);
                }
                Instr::Len => {
                    let v = self.pop();
                    let n = match &v {
                        Value::Str(s) => crate::text::len(s),
                        Value::List(items) | Value::Tuple(items) => items.len() as i64,
                        Value::Dict(pairs) => pairs.len() as i64,
                        Value::Set(items) => items.len() as i64,
                        _ => return Err("E172 တွင် အရှည် မတွက်နိုင်ပါ။".to_string()),
                    };
                    self.stack.push(Value::Int(n));
                    self.set_pc(pc + 1);
                }
                Instr::Index(line, count) => {
                    let mut keys = Vec::with_capacity(count as usize);
                    for _ in 0..count {
                        keys.push(self.pop());
                    }
                    keys.reverse();
                    let base = self.pop();
                    let item = crate::interpreter::index_value(&base, &keys, line as usize)?;
                    self.stack.push(item);
                    self.set_pc(pc + 1);
                }
                Instr::Dict(count) => {
                    let mut pairs: IndexMap<Key, Value> = IndexMap::with_capacity(count);
                    let mut items = Vec::with_capacity(count * 2);
                    for _ in 0..count * 2 {
                        items.push(self.pop());
                    }
                    items.reverse();
                    let mut it = items.into_iter();
                    while let (Some(key), Some(value)) = (it.next(), it.next()) {
                        pairs.insert(Key::new(key), value);
                    }
                    self.stack.push(Value::Dict(Rc::new(pairs)));
                    self.set_pc(pc + 1);
                }
                Instr::Contains(line) => {
                    let needle = self.pop();
                    let haystack = self.pop();
                    let found = value_contains(&haystack, &needle, line as usize)?;
                    self.stack.push(Value::Bool(found));
                    self.set_pc(pc + 1);
                }
                Instr::IterKeys => {
                    let v = self.pop();
                    let walked = match &v {
                        Value::Dict(pairs) => {
                            Value::List(pairs.keys().map(|k| k.value().clone()).collect())
                        }
                        Value::Set(items) => {
                            Value::List(items.iter().map(|k| k.value().clone()).collect())
                        }
                        _ => v,
                    };
                    self.stack.push(walked);
                    self.set_pc(pc + 1);
                }
                Instr::Set(count) => {
                    let mut items: IndexSet<Key> = IndexSet::with_capacity(count);
                    let mut values = Vec::with_capacity(count);
                    for _ in 0..count {
                        values.push(self.pop());
                    }
                    values.reverse();
                    for value in values {
                        items.insert(Key::new(value));
                    }
                    self.stack.push(Value::Set(Rc::new(items)));
                    self.set_pc(pc + 1);
                }
                Instr::NewObj(index, argc, line) => {
                    let mut args = Vec::with_capacity(argc as usize);
                    for _ in 0..argc {
                        args.push(self.pop());
                    }
                    args.reverse();
                    let params = self.prog.classes[index].params.clone();
                    if args.len() != params.len() {
                        return Err(format!(
                            "E034 လိုင်း {} တွင် \"{}\" ပုံသေ၏ တည်ဆောက်သူသည် အချက် {} ခု လိုအပ်ပါသည်၊ {} ခု ပေးထားပါသည်။",
                            line,
                            self.prog.classes[index].name,
                            params.len(),
                            args.len()
                        ));
                    }
                    let mut locals = HashMap::new();
                    for (name, value) in params.iter().zip(args) {
                        locals.insert(name.clone(), value);
                    }
                    self.set_pc(pc + 1);
                    self.frames.push(Frame {
                        func: 0,
                        pc: 0,
                        locals,
                        class: Some(index),
                        self_fields: Vec::new(),
                        value_err: None,
                    });
                }
                Instr::SelfSet(field, line) => {
                    let value = self.pop();
                    let Some(frame) = self.frames.last_mut() else {
                        return Err(format!(
                            "E035 လိုင်း {} တွင် \"အတု\" ကို ပုံသေ တည်ဆောက်သူ အတွင်းမှာသာ သုံးနိုင်ပါသည်။",
                            line
                        ));
                    };
                    if frame.class.is_none() {
                        return Err(format!(
                            "E035 လိုင်း {} တွင် \"အတု\" ကို ပုံသေ တည်ဆောက်သူ အတွင်းမှာသာ သုံးနိုင်ပါသည်။",
                            line
                        ));
                    }
                    match frame.self_fields.iter_mut().find(|(k, _)| *k == field) {
                        Some(slot) => slot.1 = value,
                        None => frame.self_fields.push((field, value)),
                    }
                    self.set_pc(pc + 1);
                }
                Instr::Field(field, line) => {
                    let target = self.pop();
                    let value = crate::interpreter::field_of(&target, &field, line as usize)?;
                    self.stack.push(value);
                    self.set_pc(pc + 1);
                }
                Instr::FieldRef(name, field, line) => {
                    // A variable wins over a library name, as in the
                    // interpreter: `<var>.<field>` is a field read.
                    let bound = match self.frames.last() {
                        Some(frame) => frame.locals.get(&name).cloned(),
                        None => None,
                    }
                    .or_else(|| self.globals.get(&name).cloned());
                    let value = match bound {
                        Some(target) => {
                            crate::interpreter::field_of(&target, &field, line as usize)?
                        }
                        None => {
                            return Err(format!(
                                "E085 လိုင်း {} တွင် \"{}\" ဆိုသော နည်းပညာများကို ရှာမတွေ့ပါ။",
                                line, name
                            ))
                        }
                    };
                    self.stack.push(value);
                    self.set_pc(pc + 1);
                }
                Instr::IndexStore(name, line, count) => {
                    let value = self.pop();
                    let mut keys = Vec::with_capacity(count as usize);
                    for _ in 0..count {
                        keys.push(self.pop());
                    }
                    keys.reverse();
                    let mut base = self.pop();
                    set_index_vm(&mut base, &keys, value, line as usize)?;
                    self.store(&name, base);
                    self.set_pc(pc + 1);
                }
                Instr::Call(index, argc) => {
                    let mut args = Vec::with_capacity(argc as usize);
                    for _ in 0..argc {
                        args.push(self.pop());
                    }
                    args.reverse();
                    // The caller resumes at the instruction after the call;
                    // `Return` finds it there when the frame pops.
                    self.enter_call(index, args, None, 0, pc + 1)?;
                }
                Instr::CallValue(index, argc, line, err) => {
                    let mut args = Vec::with_capacity(argc as usize);
                    for _ in 0..argc {
                        args.push(self.pop());
                    }
                    args.reverse();
                    // The caller wants a value: a body that ends without one
                    // is the interpreter's `E071`.
                    self.enter_call(index, args, Some(err), line, pc + 1)?;
                }
                Instr::IterItems(kind, line) => {
                    let collection = self.pop();
                    let items = Vm::iter_items(kind, &collection, line as usize)?;
                    self.stack.push(Value::List(items));
                    self.set_pc(pc + 1);
                }
                Instr::HigherOrder(kind, line) => {
                    // Stack: items, the callback value, and for `reduce` its
                    // starting value -- the order the interpreter evaluates
                    // them in.
                    let initial = if kind == 2 { Some(self.pop()) } else { None };
                    let callback = self.pop();
                    let items = match self.pop() {
                        Value::List(items) => items,
                        other => vec![other],
                    };
                    let callable = match callback {
                        Value::Callable(kind) => kind,
                        other => {
                            return Err(crate::interpreter::eng_builtin_err(
                                line as usize,
                                higher_order_name(kind),
                                &format!(
                                    "ပြန်ခေါ် အလုပ်အဖြစ် အလုပ် တစ်ခု လိုအပ်ပါသည်၊ {} ရပါသည်",
                                    crate::interpreter::type_name_mm(&other)
                                ),
                            ))
                        }
                    };
                    // The caller resumes at the next instruction; a callback
                    // that leaves through a handler must find it that way.
                    self.set_pc(pc + 1);
                    let mut out: Vec<Value> = Vec::with_capacity(items.len());
                    let mut acc = initial;
                    for item in items {
                        let args = match kind {
                            2 => vec![acc.take().expect("reduce keeps an accumulator"), item.clone()],
                            _ => vec![item.clone()],
                        };
                        let value = match self.call_callback(&callable, args, line)? {
                            CallbackOutcome::Value(value) => value,
                            CallbackOutcome::Left => return Ok(()),
                        };
                        match kind {
                            0 => out.push(value),
                            1 => match value {
                                Value::Bool(true) => out.push(item),
                                Value::Bool(false) => {}
                                other => {
                                    return Err(crate::interpreter::eng_builtin_err(
                                        line as usize,
                                        higher_order_name(kind),
                                        &format!(
                                            "True သို့မဟုတ် False ပြန်ပေးသော ပြန်ခေါ် အလုပ် လိုအပ်ပါသည်၊ {} ရပါသည်",
                                            crate::interpreter::type_name_mm(&other)
                                        ),
                                    ))
                                }
                            },
                            _ => acc = Some(value),
                        }
                    }
                    match kind {
                        2 => self.stack.push(acc.expect("reduce has its result")),
                        _ => self.stack.push(Value::List(out)),
                    }
                }
                Instr::ListArg(name, line) => {
                    if let Some(other) = self.stack.last() {
                        if !matches!(other, Value::List(_)) {
                            let got = crate::interpreter::type_name_mm(other);
                            return Err(crate::interpreter::eng_builtin_err(
                                line as usize,
                                &name,
                                &format!("စာရင်း လိုအပ်ပါသည်၊ {} ရပါသည်", got),
                            ));
                        }
                    }
                    self.set_pc(pc + 1);
                }
                Instr::Mutate(name, var, argc, line) => {
                    let mut extra = Vec::with_capacity(argc.saturating_sub(1) as usize);
                    for _ in 0..argc.saturating_sub(1) {
                        extra.push(self.pop());
                    }
                    extra.reverse();
                    let current = self.pop();
                    let mut items = match current {
                        Value::List(items) => items,
                        other => {
                            return Err(crate::interpreter::eng_builtin_err(
                                line as usize,
                                &name,
                                &format!(
                                    "စာရင်း လိုအပ်ပါသည်၊ {} ရပါသည်",
                                    crate::interpreter::type_name_mm(&other)
                                ),
                            ))
                        }
                    };
                    let result = mutate_list(&name, &mut items, extra, line as usize)?;
                    self.store(&var, Value::List(items));
                    self.stack.push(result);
                    self.set_pc(pc + 1);
                }
                Instr::List(count) => {
                    let mut items = Vec::with_capacity(count.min(1024));
                    for _ in 0..count {
                        items.push(self.pop());
                    }
                    items.reverse();
                    self.stack.push(Value::List(items));
                    self.set_pc(pc + 1);
                }
                Instr::Tuple(count) => {
                    let mut items = Vec::with_capacity(count.min(1024));
                    for _ in 0..count {
                        items.push(self.pop());
                    }
                    items.reverse();
                    self.stack.push(Value::Tuple(items));
                    self.set_pc(pc + 1);
                }
                Instr::ParamMissing(name, target) => {
                    let bound = self
                        .frames
                        .last()
                        .map(|f| f.locals.contains_key(&name))
                        .unwrap_or(false);
                    self.set_pc(if bound { target } else { pc + 1 });
                }
                Instr::Return(line) | Instr::ReturnValue(line) => {
                    let value = self.stack.pop().unwrap_or(Value::Null);
                    self.leave_frame(value, line, true)?;
                }
                Instr::ReturnVoid(line) => {
                    let value = self.stack.pop().unwrap_or(Value::Null);
                    self.leave_frame(value, line, false)?;
                }
                Instr::Halt => self.halted = true,
                }
                Ok(())
            })();
            if self.halted {
                return Ok(());
            }
            if let Err(message) = outcome {
                if !self.unwind(&message) {
                    return Err(message);
                }
            }
        }
    }
}

/// Compile a source text all the way to instructions.
#[cfg(test)]
pub fn compile_source(src: &str) -> Result<Program, String> {
    let tokens = crate::lexer::lex(src)?;
    let stmts = crate::parser::parse(&tokens)?;
    compile(&stmts)
}

/// Compile and run a source text, returning the lines it printed.
#[cfg(test)]
pub fn run_source(src: &str) -> Result<Vec<String>, String> {
    let program = compile_source(src)?;
    let mut vm = Vm::new(&program);
    vm.run()?;
    Ok(vm.out)
}

/// Compile and run a source text with scripted stdin, returning the lines it
/// printed -- what a program that asks for input does under a test.
#[cfg(test)]
pub fn run_source_with_input(src: &str, input: &str) -> Result<Vec<String>, String> {
    let program = compile_source(src)?;
    let mut vm = Vm::new(&program);
    vm.feed(input);
    vm.run()?;
    Ok(vm.out)
}

// ---------------------------------------------------------------------------
// Serialization
// ---------------------------------------------------------------------------

/// Version of the container [`Program::encode`] writes. `decode` refuses any
/// other version with `E175`, so a built file and the reader always agree on
/// the instruction set.
///
/// Revision 2 added dictionaries: value tag 7, the `Dict`/`Contains`/
/// `IterKeys`/`IndexStore` instructions, and a source line on `Index`.
/// Revision 3 added sets (value tag 8, `Set`), classes (`Program::classes`
/// with `NewObj`/`SelfSet`/`Field`) and the object-building constructor frame.
/// Revision 4 added `input`: `Prompt`, `Read` and `ReadTyped`.
/// Revision 5 added the eng declaration forms: `Declare` and `PinGuard`.
/// Revision 6 added `try`/`catch`/`finally`: `Try`, `TryEnd`, `ErrorCode`,
/// `ReThrow` and `PopHandlers`.
/// Revision 11 carried the line of a missing-variable `E030` on `Load`, and
/// revision 12 the callback builtins: `IterItems` and `HigherOrder`, which run
/// `map`/`filter`/`reduce` with a lambda or named callback. Revision 13 gave
/// `Div`/`Mod` their line, so division by zero reports `E036`/`E037` and a zero
/// remainder `E038`/`E039` exactly where the interpreter does.
/// Revision 15 made lambdas first-class values: `MakeFunc`, `LoadFn`,
/// `CallableGuard`, `CallDyn`, `Reverse`, `ToBool`, `AndJump`, `OrJump`, a
/// line on `Not`, and a value callback on `HigherOrder`.
/// Revision 16 added destructuring: `Destructure`, which binds one name per
/// item of a list or tuple.
pub const FORMAT_VERSION: u16 = 16;
/// Container magic: `AKKBC` + the digits of the original container layout.
/// The revision that decides compatibility is [`FORMAT_VERSION`], below the
/// magic; older magic digits are kept so a built file stays recognisable.
const MAGIC: [u8; 8] = *b"AKKBC001";

fn malformed(why: &str) -> String {
    format!("E175 bytecode ကို ဖတ်၍မရပါ - {}.", why)
}

fn put_u16(out: &mut Vec<u8>, n: u16) {
    out.extend_from_slice(&n.to_le_bytes());
}

fn put_u32(out: &mut Vec<u8>, n: u32) {
    out.extend_from_slice(&n.to_le_bytes());
}

fn put_str(out: &mut Vec<u8>, text: &str) -> Result<(), String> {
    let len = u32::try_from(text.len()).map_err(|_| "E175 စာသား ရှည်လွန်းပါသည်။".to_string())?;
    put_u32(out, len);
    out.extend_from_slice(text.as_bytes());
    Ok(())
}

/// Reads little-endian fields out of a byte slice, refusing to run off its end.
struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Reader { bytes, at: 0 }
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8], String> {
        let end = self
            .at
            .checked_add(n)
            .filter(|end| *end <= self.bytes.len())
            .ok_or_else(|| malformed("အချက်အလက် မလုံလောက်ပါ"))?;
        let slice = &self.bytes[self.at..end];
        self.at = end;
        Ok(slice)
    }

    fn u8(&mut self) -> Result<u8, String> {
        Ok(self.take(1)?[0])
    }

    fn u16(&mut self) -> Result<u16, String> {
        let b = self.take(2)?;
        Ok(u16::from_le_bytes([b[0], b[1]]))
    }

    fn u32(&mut self) -> Result<u32, String> {
        let b = self.take(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    fn u64(&mut self) -> Result<u64, String> {
        let b = self.take(8)?;
        Ok(u64::from_le_bytes([
            b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7],
        ]))
    }

    fn text(&mut self) -> Result<String, String> {
        let len = self.u32()? as usize;
        let raw = self.take(len)?;
        String::from_utf8(raw.to_vec()).map_err(|_| malformed("စာသားသည် UTF-8 မဟုတ်ပါ"))
    }

    fn done(&self) -> bool {
        self.at == self.bytes.len()
    }
}

fn encode_value(value: &Value, out: &mut Vec<u8>) -> Result<(), String> {
    match value {
        Value::Null => out.push(0),
        Value::Bool(b) => {
            out.push(1);
            out.push(u8::from(*b));
        }
        Value::Int(n) => {
            out.push(2);
            out.extend_from_slice(&n.to_le_bytes());
        }
        Value::Float(f) => {
            out.push(3);
            out.extend_from_slice(&f.to_le_bytes());
        }
        Value::Str(s) => {
            out.push(4);
            put_str(out, s)?;
        }
        Value::List(items) => {
            out.push(5);
            put_u32(out, items.len() as u32);
            for item in items {
                encode_value(item, out)?;
            }
        }
        Value::Tuple(items) => {
            out.push(6);
            put_u32(out, items.len() as u32);
            for item in items {
                encode_value(item, out)?;
            }
        }
        Value::Dict(pairs) => {
            out.push(7);
            put_u32(out, pairs.len() as u32);
            for (key, value) in pairs.iter() {
                encode_value(key.value(), out)?;
                encode_value(value, out)?;
            }
        }
        Value::Set(items) => {
            out.push(8);
            put_u32(out, items.len() as u32);
            for item in items.iter() {
                encode_value(item.value(), out)?;
            }
        }
        other => {
            return Err(format!(
                "E175 {} ကို bytecode တွင် သိမ်း၍မရပါ။",
                render(other)
            ))
        }
    }
    Ok(())
}

fn decode_value(reader: &mut Reader<'_>) -> Result<Value, String> {
    let tag = reader.u8()?;
    match tag {
        0 => Ok(Value::Null),
        1 => Ok(Value::Bool(reader.u8()? != 0)),
        2 => Ok(Value::Int(reader.u64()? as i64)),
        3 => Ok(Value::Float(f64::from_bits(reader.u64()?))),
        4 => Ok(Value::Str(reader.text()?)),
        5 | 6 => {
            let count = reader.u32()? as usize;
            let mut items = Vec::with_capacity(count.min(1024));
            for _ in 0..count {
                items.push(decode_value(reader)?);
            }
            if tag == 6 {
                Ok(Value::Tuple(items))
            } else {
                Ok(Value::List(items))
            }
        }
        7 => {
            let count = reader.u32()? as usize;
            let mut pairs: IndexMap<Key, Value> = IndexMap::with_capacity(count.min(1024));
            for _ in 0..count {
                let key = decode_value(reader)?;
                let value = decode_value(reader)?;
                pairs.insert(Key::new(key), value);
            }
            Ok(Value::Dict(Rc::new(pairs)))
        }
        8 => {
            let count = reader.u32()? as usize;
            let mut items: IndexSet<Key> = IndexSet::with_capacity(count.min(1024));
            for _ in 0..count {
                items.insert(Key::new(decode_value(reader)?));
            }
            Ok(Value::Set(Rc::new(items)))
        }
        tag => Err(malformed(&format!("မသိသော တန်ဖိုး အမျိုးအစား {}", tag))),
    }
}

fn encode_code(code: &[Instr], out: &mut Vec<u8>) -> Result<(), String> {
    put_u32(out, code.len() as u32);
    for instr in code {
        match instr {
            Instr::Const(v) => {
                out.push(0);
                encode_value(v, out)?;
            }
            Instr::Load(name, line) => {
                out.push(1);
                put_str(out, name)?;
                put_u32(out, *line);
            }
            Instr::Store(name) => {
                out.push(2);
                put_str(out, name)?;
            }
            Instr::Pop => out.push(3),
            Instr::Destructure(names, line) => {
                out.push(73);
                put_u32(out, names.len() as u32);
                for name in names {
                    put_str(out, name)?;
                }
                put_u32(out, *line);
            }
            Instr::Add => out.push(4),
            Instr::Sub => out.push(5),
            Instr::Mul => out.push(6),
            Instr::Div(line) => {
                out.push(7);
                put_u32(out, *line);
            }
            Instr::Mod(line) => {
                out.push(8);
                put_u32(out, *line);
            }
            Instr::Neg => out.push(9),
            Instr::Not(line) => {
                out.push(10);
                put_u32(out, *line);
            }
            Instr::And => out.push(11),
            Instr::Or => out.push(12),
            Instr::Eq => out.push(13),
            Instr::Ne => out.push(14),
            Instr::Lt => out.push(15),
            Instr::Le => out.push(16),
            Instr::Gt => out.push(17),
            Instr::Ge => out.push(18),
            Instr::JumpIfTrue(at) => {
                out.push(19);
                put_u32(out, *at as u32);
            }
            Instr::JumpIfFalse(at) => {
                out.push(20);
                put_u32(out, *at as u32);
            }
            Instr::Jump(at) => {
                out.push(21);
                put_u32(out, *at as u32);
            }
            Instr::Print => out.push(22),
            Instr::Len => out.push(23),
            Instr::Call(index, argc) => {
                out.push(24);
                put_u32(out, *index as u32);
                out.push(*argc);
            }
            Instr::Return(line) => {
                out.push(25);
                put_u32(out, *line);
            }
            Instr::Index(line, count) => {
                out.push(26);
                put_u32(out, *line);
                out.push(*count);
            }
            Instr::Halt => out.push(27),
            Instr::Dict(count) => {
                out.push(28);
                put_u32(out, *count as u32);
            }
            Instr::Contains(line) => {
                out.push(30);
                put_u32(out, *line);
            }
            Instr::IterKeys => out.push(29),
            Instr::IndexStore(name, line, count) => {
                out.push(31);
                put_str(out, name)?;
                put_u32(out, *line);
                out.push(*count);
            }
            Instr::Set(count) => {
                out.push(32);
                put_u32(out, *count as u32);
            }
            Instr::NewObj(index, argc, line) => {
                out.push(33);
                put_u32(out, *index as u32);
                out.push(*argc);
                put_u32(out, *line);
            }
            Instr::SelfSet(field, line) => {
                out.push(34);
                put_str(out, field)?;
                put_u32(out, *line);
            }
            Instr::Field(field, line) => {
                out.push(35);
                put_str(out, field)?;
                put_u32(out, *line);
            }
            Instr::FieldRef(name, field, line) => {
                out.push(36);
                put_str(out, name)?;
                put_str(out, field)?;
                put_u32(out, *line);
            }
            Instr::Prompt => out.push(37),
            Instr::Read => out.push(38),
            Instr::ReadTyped(type_name, line) => {
                out.push(39);
                put_str(out, type_name)?;
                put_u32(out, *line);
            }
            Instr::Declare(name, type_name, is_const, line) => {
                out.push(40);
                put_str(out, name)?;
                put_str(out, type_name)?;
                out.push(if *is_const { 1 } else { 0 });
                put_u32(out, *line);
            }
            Instr::PinGuard(name, line) => {
                out.push(41);
                put_str(out, name)?;
                put_u32(out, *line);
            }
            Instr::Interpolate(line) => {
                out.push(42);
                put_u32(out, *line);
            }
            Instr::Abort(message) => {
                out.push(43);
                put_str(out, message)?;
            }
            Instr::Try(handler) => {
                out.push(44);
                put_u32(out, *handler as u32);
            }
            Instr::TryEnd => out.push(45),
            Instr::ErrorCode => out.push(46),
            Instr::ReThrow => out.push(47),
            Instr::PopHandlers(count) => {
                out.push(48);
                out.push(*count);
            }
            Instr::MarkConst(name) => {
                out.push(49);
                put_str(out, name)?;
            }
            Instr::PinDeclareCheck(name, line) => {
                out.push(50);
                put_str(out, name)?;
                put_u32(out, *line);
            }
            Instr::Builtin(name, argc, line) => {
                out.push(51);
                put_str(out, name)?;
                out.push(*argc);
                put_u32(out, *line);
            }
            Instr::UseLib(name) => {
                out.push(52);
                put_str(out, name)?;
            }
            Instr::RequireLib(name, line) => {
                out.push(53);
                put_str(out, name)?;
                put_u32(out, *line);
            }
            Instr::LibCall(lib, fn_name, argc, line, err) => {
                out.push(54);
                put_str(out, lib)?;
                put_str(out, fn_name)?;
                out.push(*argc);
                put_u32(out, *line);
                put_str(out, err)?;
            }
            Instr::ReturnValue(line) => {
                out.push(55);
                put_u32(out, *line);
            }
            Instr::ReturnVoid(line) => {
                out.push(56);
                put_u32(out, *line);
            }
            Instr::CallValue(index, argc, line, err) => {
                out.push(57);
                put_u32(out, *index as u32);
                out.push(*argc);
                put_u32(out, *line);
                put_str(out, err)?;
            }
            Instr::IterItems(kind, line) => {
                out.push(58);
                out.push(*kind);
                put_u32(out, *line);
            }
            Instr::HigherOrder(kind, line) => {
                out.push(59);
                out.push(*kind);
                put_u32(out, *line);
            }
            Instr::ListArg(name, line) => {
                out.push(60);
                put_str(out, name)?;
                put_u32(out, *line);
            }
            Instr::Mutate(name, var, argc, line) => {
                out.push(61);
                put_str(out, name)?;
                put_str(out, var)?;
                out.push(*argc);
                put_u32(out, *line);
            }
            Instr::ParamMissing(name, target) => {
                out.push(62);
                put_str(out, name)?;
                put_u32(out, *target as u32);
            }
            Instr::List(count) => {
                out.push(63);
                put_u32(out, *count as u32);
            }
            Instr::Tuple(count) => {
                out.push(64);
                put_u32(out, *count as u32);
            }
            Instr::MakeFunc(index) => {
                out.push(65);
                put_u32(out, *index as u32);
            }
            Instr::LoadFn(name, index, line) => {
                out.push(66);
                put_str(out, name)?;
                put_u32(out, *index as u32);
                put_u32(out, *line);
            }
            Instr::CallableGuard(name, line, flags) => {
                out.push(67);
                put_str(out, name)?;
                put_u32(out, *line);
                out.push(*flags);
            }
            Instr::CallDyn(name, argc, line, flags) => {
                out.push(68);
                put_str(out, name)?;
                out.push(*argc);
                put_u32(out, *line);
                out.push(*flags);
            }
            Instr::Reverse(var, line) => {
                out.push(69);
                match var {
                    Some(name) => {
                        out.push(1);
                        put_str(out, name)?;
                    }
                    None => out.push(0),
                }
                put_u32(out, *line);
            }
            Instr::ToBool(line) => {
                out.push(70);
                put_u32(out, *line);
            }
            Instr::AndJump(target, line) => {
                out.push(71);
                put_u32(out, *target as u32);
                put_u32(out, *line);
            }
            Instr::OrJump(target, line) => {
                out.push(72);
                put_u32(out, *target as u32);
                put_u32(out, *line);
            }
        }
    }
    Ok(())
}

fn decode_code(reader: &mut Reader<'_>) -> Result<Vec<Instr>, String> {
    let count = reader.u32()? as usize;
    let mut code = Vec::with_capacity(count.min(4096));
    for _ in 0..count {
        let instr = match reader.u8()? {
            0 => Instr::Const(decode_value(reader)?),
            1 => {
                let name = reader.text()?;
                let line = reader.u32()?;
                Instr::Load(name, line)
            }
            2 => Instr::Store(reader.text()?),
            3 => Instr::Pop,
            4 => Instr::Add,
            5 => Instr::Sub,
            6 => Instr::Mul,
            7 => Instr::Div(reader.u32()?),
            8 => Instr::Mod(reader.u32()?),
            9 => Instr::Neg,
            10 => Instr::Not(reader.u32()?),
            11 => Instr::And,
            12 => Instr::Or,
            13 => Instr::Eq,
            14 => Instr::Ne,
            15 => Instr::Lt,
            16 => Instr::Le,
            17 => Instr::Gt,
            18 => Instr::Ge,
            19 => Instr::JumpIfTrue(reader.u32()? as usize),
            20 => Instr::JumpIfFalse(reader.u32()? as usize),
            21 => Instr::Jump(reader.u32()? as usize),
            22 => Instr::Print,
            23 => Instr::Len,
            24 => {
                let index = reader.u32()? as usize;
                let argc = reader.u8()?;
                Instr::Call(index, argc)
            }
            25 => Instr::Return(reader.u32()?),
            26 => {
                let line = reader.u32()?;
                let count = reader.u8()?;
                Instr::Index(line, count)
            }
            27 => Instr::Halt,
            28 => Instr::Dict(reader.u32()? as usize),
            29 => Instr::IterKeys,
            30 => Instr::Contains(reader.u32()?),
            31 => {
                let name = reader.text()?;
                let line = reader.u32()?;
                let count = reader.u8()?;
                Instr::IndexStore(name, line, count)
            }
            32 => Instr::Set(reader.u32()? as usize),
            33 => {
                let index = reader.u32()? as usize;
                let argc = reader.u8()?;
                let line = reader.u32()?;
                Instr::NewObj(index, argc, line)
            }
            34 => {
                let field = reader.text()?;
                let line = reader.u32()?;
                Instr::SelfSet(field, line)
            }
            35 => {
                let field = reader.text()?;
                let line = reader.u32()?;
                Instr::Field(field, line)
            }
            36 => {
                let name = reader.text()?;
                let field = reader.text()?;
                let line = reader.u32()?;
                Instr::FieldRef(name, field, line)
            }
            37 => Instr::Prompt,
            38 => Instr::Read,
            39 => {
                let type_name = reader.text()?;
                let line = reader.u32()?;
                Instr::ReadTyped(type_name, line)
            }
            40 => {
                let name = reader.text()?;
                let type_name = reader.text()?;
                let is_const = reader.u8()? != 0;
                let line = reader.u32()?;
                Instr::Declare(name, type_name, is_const, line)
            }
            41 => {
                let name = reader.text()?;
                let line = reader.u32()?;
                Instr::PinGuard(name, line)
            }
            42 => Instr::Interpolate(reader.u32()?),
            43 => Instr::Abort(reader.text()?),
            44 => Instr::Try(reader.u32()? as usize),
            45 => Instr::TryEnd,
            46 => Instr::ErrorCode,
            47 => Instr::ReThrow,
            48 => Instr::PopHandlers(reader.u8()?),
            49 => Instr::MarkConst(reader.text()?),
            50 => {
                let name = reader.text()?;
                let line = reader.u32()?;
                Instr::PinDeclareCheck(name, line)
            }
            51 => {
                let name = reader.text()?;
                let argc = reader.u8()?;
                let line = reader.u32()?;
                Instr::Builtin(name, argc, line)
            }
            52 => Instr::UseLib(reader.text()?),
            53 => {
                let name = reader.text()?;
                let line = reader.u32()?;
                Instr::RequireLib(name, line)
            }
            54 => {
                let lib = reader.text()?;
                let fn_name = reader.text()?;
                let argc = reader.u8()?;
                let line = reader.u32()?;
                let err = reader.text()?;
                Instr::LibCall(lib, fn_name, argc, line, err)
            }
            55 => Instr::ReturnValue(reader.u32()?),
            56 => Instr::ReturnVoid(reader.u32()?),
            57 => {
                let index = reader.u32()? as usize;
                let argc = reader.u8()?;
                let line = reader.u32()?;
                let err = reader.text()?;
                Instr::CallValue(index, argc, line, err)
            }
            58 => {
                let kind = reader.u8()?;
                let line = reader.u32()?;
                Instr::IterItems(kind, line)
            }
            59 => {
                let kind = reader.u8()?;
                let line = reader.u32()?;
                Instr::HigherOrder(kind, line)
            }
            60 => {
                let name = reader.text()?;
                let line = reader.u32()?;
                Instr::ListArg(name, line)
            }
            61 => {
                let name = reader.text()?;
                let var = reader.text()?;
                let argc = reader.u8()?;
                let line = reader.u32()?;
                Instr::Mutate(name, var, argc, line)
            }
            62 => {
                let name = reader.text()?;
                let target = reader.u32()? as usize;
                Instr::ParamMissing(name, target)
            }
            63 => Instr::List(reader.u32()? as usize),
            64 => Instr::Tuple(reader.u32()? as usize),
            65 => Instr::MakeFunc(reader.u32()? as usize),
            66 => {
                let name = reader.text()?;
                let index = reader.u32()? as usize;
                let line = reader.u32()?;
                Instr::LoadFn(name, index, line)
            }
            67 => {
                let name = reader.text()?;
                let line = reader.u32()?;
                let flags = reader.u8()?;
                Instr::CallableGuard(name, line, flags)
            }
            68 => {
                let name = reader.text()?;
                let argc = reader.u8()?;
                let line = reader.u32()?;
                let flags = reader.u8()?;
                Instr::CallDyn(name, argc, line, flags)
            }
            69 => {
                let var = if reader.u8()? != 0 {
                    Some(reader.text()?)
                } else {
                    None
                };
                let line = reader.u32()?;
                Instr::Reverse(var, line)
            }
            70 => Instr::ToBool(reader.u32()?),
            71 => {
                let target = reader.u32()? as usize;
                let line = reader.u32()?;
                Instr::AndJump(target, line)
            }
            72 => {
                let target = reader.u32()? as usize;
                let line = reader.u32()?;
                Instr::OrJump(target, line)
            }
            73 => {
                let count = reader.u32()? as usize;
                let mut names = Vec::with_capacity(count.min(1024));
                for _ in 0..count {
                    names.push(reader.text()?);
                }
                let line = reader.u32()?;
                Instr::Destructure(names, line)
            }
            op => return Err(malformed(&format!("မသိသော instruction {}", op))),
        };
        code.push(instr);
    }
    Ok(code)
}

impl Program {
    /// Write this program to the byte container: magic, [`FORMAT_VERSION`],
    /// every function body, then the top-level chunk.
    pub fn encode(&self) -> Result<Vec<u8>, String> {
        let mut out = Vec::new();
        out.extend_from_slice(&MAGIC);
        put_u16(&mut out, FORMAT_VERSION);
        put_u32(&mut out, self.funcs.len() as u32);
        for func in &self.funcs {
            put_str(&mut out, &func.name)?;
            put_u32(&mut out, func.params.len() as u32);
            for param in &func.params {
                put_str(&mut out, param)?;
            }
            put_u32(&mut out, func.required as u32);
            encode_code(&func.code, &mut out)?;
        }
        put_u32(&mut out, self.classes.len() as u32);
        for class in &self.classes {
            put_str(&mut out, &class.name)?;
            put_u32(&mut out, class.params.len() as u32);
            for param in &class.params {
                put_str(&mut out, param)?;
            }
            encode_code(&class.code, &mut out)?;
        }
        encode_code(&self.code, &mut out)?;
        Ok(out)
    }

    /// Read a container written by [`Program::encode`]. The format is fixed by
    /// [`FORMAT_VERSION`]; anything else is `E175`.
    pub fn decode(bytes: &[u8]) -> Result<Program, String> {
        let mut reader = Reader::new(bytes);
        let magic = reader.take(MAGIC.len())?;
        if magic != MAGIC {
            return Err(malformed("AKKBC ခေါင်းစီး မတွေ့ပါ"));
        }
        let version = reader.u16()?;
        if version != FORMAT_VERSION {
            return Err(format!(
                "E175 bytecode ဗားရှင်း {} ကို မသိပါ (ဤ akk သည် {} ကို ဖတ်သည်)။",
                version, FORMAT_VERSION
            ));
        }
        let func_count = reader.u32()? as usize;
        let mut funcs = Vec::with_capacity(func_count.min(4096));
        for _ in 0..func_count {
            let name = reader.text()?;
            let param_count = reader.u32()? as usize;
            let mut params = Vec::with_capacity(param_count.min(256));
            for _ in 0..param_count {
                params.push(reader.text()?);
            }
            let required = reader.u32()? as usize;
            let code = decode_code(&mut reader)?;
            funcs.push(FuncDef {
                name,
                params,
                required,
                code,
            });
        }
        let class_count = reader.u32()? as usize;
        let mut classes = Vec::with_capacity(class_count.min(4096));
        for _ in 0..class_count {
            let name = reader.text()?;
            let param_count = reader.u32()? as usize;
            let mut params = Vec::with_capacity(param_count.min(256));
            for _ in 0..param_count {
                params.push(reader.text()?);
            }
            let code = decode_code(&mut reader)?;
            classes.push(ClassDef {
                name,
                params,
                code,
            });
        }
        let code = decode_code(&mut reader)?;
        if !reader.done() {
            return Err(malformed("အပိုဘိုက်များ ပါနေပါသည်"));
        }
        Ok(Program {
            code,
            funcs,
            classes,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn out(src: &str) -> Vec<String> {
        run_source(src).expect("the program runs on the VM")
    }

    #[test]
    fn if_chain_picks_one_branch() {
        let src = r#"x သည် 5 ဖြစ်၏။
(x > 3) ဖြစ်ရင်
    "big" ကို ဖော်ပြပါ။
အခြား (x > 10) မဖြစ်ရင်
    "huge" ကို ဖော်ပြပါ။
မဖြစ်ရင်
    "small" ကို ဖော်ပြပါ။
ပြီး။
"#;
        assert_eq!(out(src), vec!["big"]);
    }

    #[test]
    fn else_branch_runs_when_the_condition_is_false() {
        let src = r#"x သည် 1 ဖြစ်၏။
(x > 3) ဖြစ်ရင်
    "big" ကို ဖော်ပြပါ။
မဖြစ်ရင်
    "small" ကို ဖော်ပြပါ။
ပြီး။
"#;
        assert_eq!(out(src), vec!["small"]);
    }

    #[test]
    fn while_loop_counts_up() {
        let src = r#"i သည် 0 ဖြစ်၏။
(i < 3) ဖြစ်နေရင်
    i ကို ဖော်ပြပါ။
    i သည် i + 1 ဖြစ်၏။
ပြီး။
"#;
        assert_eq!(out(src), vec!["0", "1", "2"]);
    }

    #[test]
    fn for_range_and_collection_loops() {
        let src = r#"i သည် တစ်ခုစီ(0, 3)
    i ကို ဖော်ပြပါ။
ပြီး။
xs မှာ [7, 8, 9] ဖြစ်၏။
x သည် တစ်ခုစီ(xs)
    x ကို ဖော်ပြပါ။
ပြီး။
"#;
        assert_eq!(out(src), vec!["0", "1", "2", "7", "8", "9"]);
    }

    #[test]
    fn function_call_runs_its_body() {
        let src = r#"လုပ်ငန်း main()သည်
    "Hello, world" ကို ပြပါ။
ပြီး။
main ကို လုပ်ပါ။
"#;
        assert_eq!(out(src), vec!["Hello, world"]);
    }

    #[test]
    fn function_returns_its_last_expression() {
        let src = r#"လုပ်ငန်း double(n)သည်
    n * 2
ပြီး။
x သည် double(21) ဖြစ်၏။
x ကို ဖော်ပြပါ။
"#;
        assert_eq!(out(src), vec!["42"]);
    }

    #[test]
    fn arithmetic_and_string_concatenation() {
        let src = r#"a သည် 7 ဖြစ်၏။
a ကို 3 တိုးပါ။
a ကို ဖော်ပြပါ။
s မှာ "မြန်မာ" ဖြစ်၏။
s သည် s + " " + "စာ" ဖြစ်၏။
s ကို ဖော်ပြပါ။
"#;
        assert_eq!(out(src), vec!["10", "မြန်မာ စာ"]);
    }

    #[test]
    fn unsupported_statements_are_refused_with_e170() {
        // A library the VM cannot lower (`App` paints windows) is refused at
        // compile time; the value-only libraries compile.
        let err = run_source("use App;\nw = App.screen(400, 300);\n").unwrap_err();
        assert!(err.contains("E170"), "{err}");
        // A script/package library has no bytecode equivalent either.
        let err = run_source("နည်းပညာများ Tools ကို သုံးမည်။\n").unwrap_err();
        assert!(err.contains("E170"), "{err}");
    }

    /// Runs a program with scripted stdin and hands back the VM itself, so a
    /// test can look at the exact bytes the run wrote (`log`) as well as the
    /// lines it printed (`out`).
    fn run_with_input(src: &str, input: &str) -> Vm<'static> {
        let program: &'static Program = Box::leak(Box::new(
            compile_source(src).expect("the program compiles to bytecode"),
        ));
        let mut vm = Vm::new(program);
        vm.feed(input);
        vm.run().expect("the program runs on the VM");
        vm
    }

    #[test]
    fn input_single_read_keeps_the_prompt_and_the_value_on_one_line() {
        let src = "name :str = input(\"Name: \");\nname ကို ဖော်ပြပါ။\n";
        let vm = run_with_input(src, "David\n");
        assert_eq!(vm.out, vec!["David"]);
        // The prompt has no newline, so the answer lands on the same line.
        assert_eq!(vm.log, "Name: David\n");
    }

    #[test]
    fn input_inside_a_loop_reads_one_line_per_pass() {
        let src = r#"i သည် 0 ဖြစ်၏။
(i < 3) ဖြစ်နေရင်
    word :str = input("word: ");
    word ကို ဖော်ပြပါ။
    i သည် i + 1 ဖြစ်၏။
ပြီး။
"#;
        let vm = run_with_input(src, "a\nb\nc\n");
        assert_eq!(vm.out, vec!["a", "b", "c"]);
        assert_eq!(vm.log, "word: a\nword: b\nword: c\n");
    }

    #[test]
    fn input_feeding_a_numeric_comparison_is_converted() {
        let src = r#"n :int = input("n: ");
(n > 10) ဖြစ်ရင်
    "big" ကို ပြပါ။
မဖြစ်ရင်
    "small" ကို ပြပါ။
ပြီး။
"#;
        let vm = run_with_input(src, "42\n");
        assert_eq!(vm.out, vec!["big"]);
        assert_eq!(vm.log, "n: big\n");
    }

    #[test]
    fn input_is_consumed_by_a_function_call() {
        // The read binds a variable, and that value is handed to a function.
        let src = r#"လုပ်ငန်း shout(s)သည်
    s + "!"
ပြီး။
who :str = input("who? ");
msg သည် shout(who) ဖြစ်၏။
msg ကို ဖော်ပြပါ။
"#;
        let vm = run_with_input(src, "Zaw\n");
        assert_eq!(vm.out, vec!["Zaw!"]);
        assert_eq!(vm.log, "who? Zaw!\n");
    }
    #[test]
    fn prompts_are_written_as_the_program_reaches_them() {
        // Two reads with a print in between: the interleaving in `log` is the
        // program's own, not one the VM adds at the end.
        let src = "\"start\" ကို ပြပါ။\na :str = input(\"a: \");\na ကို ဖော်ပြပါ။\nb :str = input(\"b: \");\nb ကို ဖော်ပြပါ။\n";
        let vm = run_with_input(src, "one\ntwo\n");
        assert_eq!(vm.out, vec!["start", "one", "two"]);
        assert_eq!(vm.log, "start\na: one\nb: two\n");
    }

    #[test]
    fn a_typed_read_past_the_end_of_stdin_fails_loudly() {
        // An empty line cannot become a number, and the interpreter fails the
        // same way on the same line: the two must not diverge at end of input.
        let src = "n :int = input(\"n: \");\nn ကို ဖော်ပြပါ။\n";
        let err = run_source_with_input(src, "").unwrap_err();
        assert!(err.contains("E107"), "{err}");
        assert!(err.contains("လိုင်း 1"), "{err}");
    }

    // -----------------------------------------------------------------
    // eng spellings: declarations, assignments, print, calls, loop control
    // -----------------------------------------------------------------

    #[test]
    fn eng_declarations_and_assignments_run() {
        let src = "x :int = 5;\nx = x + 1;\nprint(x);\ns :str = \"a\";\ns += \"b\";\nprint(s);\ny :int = 10;\ny -= 4;\nprint(y);\nprint(true);\n";
        assert_eq!(out(src), vec!["6", "ab", "6", "True"]);
    }

    #[test]
    fn a_typed_declaration_refuses_the_wrong_value() {
        let err = run_source("x :int = \"no\";\n").unwrap_err();
        assert!(err.contains("E103"), "{err}");
        assert!(err.contains("လိုင်း 1"), "{err}");
    }

    #[test]
    fn a_pin_is_never_reassigned_and_never_redeclared() {
        let err = run_source("pin N :int = 1;\nN = 2;\n").unwrap_err();
        assert!(err.contains("E104"), "{err}");

        let err = run_source("pin N :int = 1;\npin N :int = 2;\n").unwrap_err();
        assert!(err.contains("E105"), "{err}");

        // A compound assignment is a write too.
        let err = run_source("pin N :int = 1;\nN += 1;\n").unwrap_err();
        assert!(err.contains("E104"), "{err}");
    }

    #[test]
    fn eng_print_and_a_discarded_call_run() {
        let src = "fn twice(n :int) {\n    return n * 2;\n}\ntwice(21);\nprint(twice(3));\n";
        assert_eq!(out(src), vec!["6"]);
    }

    #[test]
    fn interpolation_expands_at_the_literal_and_escapes_braces() {
        let src = "name = \"Zaw\";\nprint(\"Hi, {name}!\");\nprint(\"brace: \\{x\\}\");\nn = 3;\nprint(\"n={n}\");\n";
        assert_eq!(out(src), vec!["Hi, Zaw!", "brace: {x}", "n=3"]);
    }

    #[test]
    fn a_missing_interpolation_name_fails_like_the_interpreter() {
        let err = run_source("print(\"missing {zz}\");\n").unwrap_err();
        assert!(err.contains("E058"), "{err}");
        assert!(err.contains("zz"), "{err}");
    }

    #[test]
    fn eng_break_and_continue_inside_loops() {
        let src = "i သည် 0 ဖြစ်၏။\nwhile (i < 5) {\n    i = i + 1;\n    if (i == 2) {\n        continue;\n    }\n    if (i == 4) {\n        break;\n    }\n    print(i);\n}\nloop {\n    break;\n}\nprint(i);\n";
        assert_eq!(out(src), vec!["1", "3", "4"]);
    }

    #[test]
    fn loop_control_outside_a_loop_fails_with_the_interpreter_text() {
        let err = run_source("print(\"before\");\nbreak;\n").unwrap_err();
        assert!(err.contains("E116"), "{err}");

        let err = run_source("continue;\n").unwrap_err();
        assert!(err.contains("E134"), "{err}");
    }

    // -----------------------------------------------------------------
    // try / catch / finally
    // -----------------------------------------------------------------

    #[test]
    fn try_catches_the_error_by_code_and_runs_finally() {
        let src = "print(\"before\");\nစမ်းရန်\n    n :int = \"no\";\nE103 ကို ဖမ်းပါ။\n    print(\"caught\");\nနောက်ဆုံးတွင်\n    print(\"fin\");\nပြီး။\nprint(\"after\");\n";
        assert_eq!(
            out(src),
            vec!["before", "caught", "fin", "after"]
        );
    }

    #[test]
    fn a_try_without_an_error_still_runs_finally() {
        let src = "စမ်းရန်\n    print(\"try\");\nE103 ကို ဖမ်းပါ။\n    print(\"catch\");\nနောက်ဆုံးတွင်\n    print(\"fin\");\nပြီး။\nprint(\"after\");\n";
        assert_eq!(out(src), vec!["try", "fin", "after"]);
    }

    #[test]
    fn an_unmatched_error_runs_finally_and_then_escapes() {
        let src = "စမ်းရန်\n    n :int = \"no\";\nE999 ကို ဖမ်းပါ။\n    print(\"wrong\");\nနောက်ဆုံးတွင်\n    print(\"fin\");\nပြီး။\nprint(\"unreachable\");\n";
        let err = run_source(src).unwrap_err();
        assert!(err.contains("E103"), "{err}");
    }

    #[test]
    fn a_catch_for_any_error_catches_any_code() {
        let src = "စမ်းရန်\n    n :int = \"no\";\nE ကို ဖမ်းပါ။\n    print(\"any\");\nပြီး။\nprint(\"done\");\n";
        assert_eq!(out(src), vec!["any", "done"]);
    }

    #[test]
    fn an_error_from_a_called_function_is_caught_by_the_caller() {
        let src = "fn boom() {\n    x :int = \"no\";\n}\nစမ်းရန်\n    boom();\nE103 ကို ဖမ်းပါ။\n    print(\"caught a call\");\nပြီး။\nprint(\"done\");\n";
        assert_eq!(out(src), vec!["caught a call", "done"]);
    }

    #[test]
    fn an_error_inside_the_catch_body_runs_finally_and_escapes() {
        let src = "စမ်းရန်\n    n :int = \"no\";\nE103 ကို ဖမ်းပါ။\n    print(\"catch start\");\n    y :int = \"no\";\nနောက်ဆုံးတွင်\n    print(\"fin\");\nပြီး။\nprint(\"unreachable\");\n";
        let err = run_source(src).unwrap_err();
        assert!(err.contains("E103"), "{err}");
    }

    #[test]
    fn a_loop_inside_a_try_breaks_out_without_running_finally() {
        // The interpreter stops a statement list as soon as a break is in
        // flight, so `finally` is skipped on that exit -- and so is it here.
        let src = "i မှာ 0 ဖြစ်၏။\n(i < 5) ဖြစ်နေရင်\n    စမ်းရန်\n        i ကို 1 တိုးပါ။\n        (i == 2) ဖြစ်ရင်\n            ရပ်ပါ။\n        ပြီး။\n        \"body\" ကို ပြပါ။\n    နောက်ဆုံးတွင်\n        \"fin\" ကို ပြပါ။\n    ပြီး။\nပြီး။\n\"done\" ကို ပြပါ။\n";
        // The first pass ends normally (so the finally body runs); the second
        // one
        // breaks out of the loop, which skips it.
        assert_eq!(out(src), vec!["body", "fin", "done"]);
    }

    #[test]
    fn a_caught_error_inside_a_loop_is_caught_every_pass() {
        let src = "i မှာ 0 ဖြစ်၏။\n(i < 3) ဖြစ်နေရင်\n    စမ်းရန်\n        n :int = \"no\";\n    E103 ကို ဖမ်းပါ။\n        print(\"caught\");\n    ပြီး။\n    i ကို 1 တိုးပါ။\nပြီး။\nprint(\"done\");\n";
        assert_eq!(out(src), vec!["caught", "caught", "caught", "done"]);
    }

    #[test]
    fn a_nested_try_lets_the_outer_one_catch_an_unmatched_error() {
        let src = "စမ်းရန်\n    စမ်းရန်\n        n :int = \"no\";\n    E999 ကို ဖမ်းပါ။\n        print(\"inner wrong\");\n    ပြီး။\n    print(\"unreachable inner\");\nE103 ကို ဖမ်းပါ။\n    print(\"outer caught\");\nပြီး။\nprint(\"done\");\n";
        assert_eq!(out(src), vec!["outer caught", "done"]);
    }

    #[test]
    fn a_return_out_of_a_try_leaves_the_handler_behind() {
        // The return skips `finally` (the interpreter's signal rule), and the
        // error after the call must not be caught by the dead try.
        let src = "fn f(n :int) {\n    စမ်းရန်\n        if (n > 0) {\n            return \"early\";\n        }\n        return \"late\";\n    နောက်ဆုံးတွင်\n        print(\"fin\");\n    ပြီး။\n}\nprint(f(1));\nprint(f(0));\n";
        assert_eq!(out(src), vec!["early", "late"]);
    }

    /// An undefined callee is a *run-time* `E031` in the interpreter (after
    /// the lines before it have printed), so it must be one here too -- not a
    /// compile-time refusal.
    #[test]
    fn an_undefined_function_is_e031_at_run_time() {
        let src = "print(\"start\");\nnosuchfunction ကို လုပ်ပါ။\n";
        let err = run_source(src).unwrap_err();
        assert_eq!(
            err,
            "E031 လိုင်း 2 တွင် \"nosuchfunction\" ဆိုသော အလုပ်ကို ရှာမတွေ့ပါ။"
        );
    }

    #[test]
    fn an_undefined_function_inside_a_try_is_catchable() {
        let src = "စမ်းရန်\n    nosuchthing ကို လုပ်ပါ။\nE031 ကို ဖမ်းပါ။\n    print(\"caught\");\nပြီး။\nprint(\"after\");\n";
        assert_eq!(out(src), vec!["caught", "after"]);
    }

    /// A wrong argument count is `E033` at run time, before the arguments run.
    #[test]
    fn a_wrong_argument_count_is_e033() {
        let src = "fn f(a :int) {\n    print(a);\n}\nprint(\"start\");\nf();\n";
        let err = run_source(src).unwrap_err();
        assert_eq!(
            err,
            "E033 လိုင်း 5 တွင် \"f\" အလုပ်သည် အချက် 1 ခု လိုအပ်ပါသည်၊ 0 ခု ပေးထားပါသည်။"
        );
        let src = "fn f(a :int) {\n    print(a);\n}\nf(1, 2);\n";
        let err = run_source(src).unwrap_err();
        assert_eq!(
            err,
            "E033 လိုင်း 4 တွင် \"f\" အလုပ်သည် အချက် 1 ခု လိုအပ်ပါသည်၊ 2 ခု ပေးထားပါသည်။"
        );
    }

    // -----------------------------------------------------------------
    // builtins
    // -----------------------------------------------------------------

    #[test]
    fn builtin_calls_compile_and_run() {
        let src = "print(len(\"abc\"));\nprint(abs(-5));\nprint(max(3, 7));\nprint(sqrt(6.25));\nprint(floor(2.9));\nprint(ceil(8.1));\nprint(round(1.4));\nprint(find(\"abcd\", \"d\"));\nprint(sqrt(16));\nprint(min(2.5, 3.5));\nprint(len([1, 2, 3, 4]));\nprint(sum([1, 1, 2]));\nprint(upper(\"eng\"));\nprint(trim(\"  Akkhara  \"));\nprint(contains(\"abc\", \"b\"));\nprint(starts_with(\"abc\", \"ab\"));\nprint(len(split(\"a,b,c,d\", \",\")));\n";
        assert_eq!(
            out(src),
            vec![
                "3", "5", "7", "2.5", "2", "9", "1", "3", "4.0", "2.5", "4", "4", "ENG",
                "Akkhara", "True", "True", "4"
            ]
        );
    }

    #[test]
    fn a_builtin_error_matches_the_interpreter() {
        // `sqrt(-1)` is `E111`, raised when the call runs -- and catchable.
        let src = "စမ်းရန်\n    sqrt(-1);\nE111 ကို ဖမ်းပါ။\n    print(\"sqrt ok\");\nပြီး။\nprint(\"done\");\n";
        assert_eq!(out(src), vec!["sqrt ok", "done"]);
        let err = run_source("print(\"before\");\nsqrt(-1);\n").unwrap_err();
        assert!(err.contains("E111"), "{err}");
    }

    #[test]
    fn a_builtin_is_checked_for_the_right_argument_type() {
        // The interpreter's own `E110` for `abs("x")`, not a VM-specific one.
        let err = run_source("print(abs(\"x\"));\n").unwrap_err();
        assert!(err.contains("E110"), "{err}");
    }

    #[test]
    fn reverse_compiles_as_a_statement_and_as_a_value() {
        // `reverse(xs);` discards the result, `print(reverse(xs))` keeps it;
        // both compile, and the list-mutating builtins still do too.
        assert!(compile_source("xs မှာ [1, 2] ဖြစ်၏။\nreverse(xs);\nprint(xs);\n").is_ok());
        assert!(compile_source("print(reverse([1, 2]));\n").is_ok());
        assert!(compile_source("xs မှာ [1] ဖြစ်၏။\nprint(push(xs, 2));\n").is_ok());
    }

    // -----------------------------------------------------------------
    // mutating builtins
    // -----------------------------------------------------------------

    #[test]
    fn the_mutating_builtins_change_their_list_and_return_the_useful_value() {
        let src = r#"xs မှာ [3, 1, 2] ဖြစ်၏။
print(push(xs, 4));
print(xs);
print(pop(xs));
print(sort(xs));
print(insert(xs, 0, 9));
print(remove(xs, 0));
print(append(xs, 7));
print(xs);
"#;
        assert_eq!(
            out(src),
            vec![
                "4",
                "[3, 1, 2, 4]",
                "4",
                "[1, 2, 3]",
                "4",
                "9",
                "4",
                "[1, 2, 3, 7]",
            ]
        );
        // As a statement and through the assigning call form.
        let src = "xs မှာ [2, 1] ဖြစ်၏။\nsort(xs);\nprint(xs);\ny အတွက် push(xs, 3) ကို လုပ်ပါ။\nprint(y);\n";
        assert_eq!(out(src), vec!["[1, 2]", "3"]);
    }

    #[test]
    fn a_mutating_builtin_keeps_the_interpreters_errors() {
        // A first argument that is not a variable: `E110`, catchable.
        let src = "စမ်းရန်\n    print(push(5, 1));\nE110 ကို ဖမ်းပါ။\n    print(\"caught\");\nပြီး။\n";
        assert_eq!(out(src), vec!["caught"]);
        // A missing variable: `E030`.
        let err = run_source("print(push(nosuch, 1));\n").unwrap_err();
        assert!(err.contains("E030"), "{err}");
        // Wrong argument count and an index out of range: `E110`/`E049`.
        let err = run_source("xs မှာ [1] ဖြစ်၏။\nprint(pop(xs, 2));\n").unwrap_err();
        assert!(err.contains("E110"), "{err}");
        let err = run_source("xs မှာ [1] ဖြစ်၏။\nprint(insert(xs, 9, 5));\n").unwrap_err();
        assert!(err.contains("E049"), "{err}");
        // Popping an empty list.
        let err = run_source("xs မှာ [] ဖြစ်၏။\nprint(pop(xs));\n").unwrap_err();
        assert!(err.contains("E110"), "{err}");
    }

    // -----------------------------------------------------------------
    // default parameters and the interpreter's dynamic scope
    // -----------------------------------------------------------------

    #[test]
    fn a_default_parameter_fills_in_before_the_body_runs() {
        let src = r#"fn greet(name, punct = "!") {
    return name + punct;
}
print(greet("A"));
print(greet("B", "?"));
fn f(a, b = a + 1, c = b * 2) {
    return [a, b, c];
}
print(f(1));
print(f(1, 5));
print(f(1, 5, 9));
"#;
        assert_eq!(
            out(src),
            vec!["A!", "B?", "[1, 2, 4]", "[1, 5, 10]", "[1, 5, 9]"]
        );
        // Too few arguments is still the interpreter's `E033`.
        let err = run_source("fn f(a, b = a + 1) {\n    return a;\n}\nf();\n").unwrap_err();
        assert!(err.contains("E033"), "{err}");
        // A lambda in a callback position may have one too.
        // An arrow lambda takes no defaults; the `fn` spelling does.
        let src = "print(map([1, 2], fn(x, k = 10) { x * k }));\n";
        assert_eq!(out(src), vec!["[10, 20]"]);
    }

    /// The interpreter's scope chain is dynamic: a callee sees its caller's
    /// frames, so `f()` may read a local of the function that called it.
    #[test]
    fn a_callee_reads_a_callers_local() {
        let src = "fn f() {\n    print(x);\n}\nfn g() {\n    x = 5;\n    f();\n}\ng();\nprint(\"after\");\n";
        assert_eq!(out(src), vec!["5", "after"]);
    }

    #[test]
    fn a_list_literal_may_hold_expressions() {
        let src = "a = 2;\nprint([a, a + 1, \"x\"]);\nprint((a, a));\n";
        assert_eq!(out(src), vec!["[2, 3, \"x\"]", "(2, 2)"]);
    }

    // -----------------------------------------------------------------
    // json and file libraries
    // -----------------------------------------------------------------

    #[test]
    fn the_json_library_runs_on_values() {
        let src = r#"use json;
text :str = json.stringify([1, 2, 3]);
print(text);
print(json.parse(text));
print(json.parse("1"));
print(json.get(json.parse("\{\"a\": 1\}"), "a", "none"));
print(json.get(json.parse("[1]"), "missing", "fallback"));
"#;
        assert_eq!(
            out(src),
            vec![
                "[1,2,3]",
                "[1, 2, 3]",
                "1",
                "1",
                "fallback"
            ]
        );
        // A parse failure is the interpreter's `E132`, catchable by code.
        let src = "use json;\nစမ်းရန်\n    print(json.parse(\"\\{bad\"));\nE132 ကို ဖမ်းပါ။\n    print(\"caught\");\nပြီး။\n";
        assert_eq!(out(src), vec!["caught"]);
        // An unimported library still fails with `E089`.
        let err = run_source("print(json.parse(\"1\"));\n").unwrap_err();
        assert!(err.contains("E089"), "{err}");
    }

    #[test]
    fn the_file_library_runs_on_values() {
        let path = "akk_bytecode_file_test.txt";
        let src = format!(
            "use file;\nfile.write(\"{path}\", \"one\\ntwo\\n\");\nprint(file.exists(\"{path}\"));\nprint(file.lines(\"{path}\"));\nfile.append(\"{path}\", \"three\");\nprint(file.read(\"{path}\"));\nfile.delete(\"{path}\");\nprint(file.exists(\"{path}\"));\n"
        );
        assert_eq!(
            out(&src),
            vec![
                "True",
                "[\"one\", \"two\"]",
                "one\ntwo\nthree",
                "False"
            ]
        );
        // A missing file is a catchable error, not empty text.
        let src = format!(
            "use file;\nစမ်းရန်\n    print(file.read(\"{path}\"));\nE140 ကို ဖမ်းပါ။\n    print(\"caught\");\nပြီး။\n"
        );
        let out_lines = out(&src);
        assert_eq!(out_lines.last().map(String::as_str), Some("caught"));
    }

    // -----------------------------------------------------------------
    // division and remainder
    // -----------------------------------------------------------------

    #[test]
    fn division_and_remainder_match_the_interpreter() {
        // Integer `/` yields a float and `%` is the Euclidean remainder, the
        // rules the interpreter's `numeric` follows.
        let src = "print(7 / 2);\nprint(-7 % 3);\nprint(7 % -3);\nprint(7 / 2.0);\nprint(-7.5 % 2.0);\n";
        assert_eq!(out(src), vec!["3.5", "2", "1", "3.5", "0.5"]);
    }

    /// Regression: a zero divisor reports the interpreter's code, text and
    /// line -- `E036`/`E038` for integers, `E037`/`E039` for floats -- from
    /// the line the operation was written on, wherever it runs.
    #[test]
    fn a_zero_divisor_reports_the_interpreters_code_and_line() {
        assert_eq!(
            run_source("print(1);\nprint(4 / 0);\n").unwrap_err(),
            "E036 လိုင်း 2 တွင် သုညဖြင့် စား၍မရပါ။"
        );
        assert_eq!(
            run_source("print(4 / 0.0);\n").unwrap_err(),
            "E037 လိုင်း 1 တွင် သုညဖြင့် စား၍မရပါ။"
        );
        assert_eq!(
            run_source("print(4 % 0);\n").unwrap_err(),
            "E038 လိုင်း 1 တွင် သုညဖြင့် ကြွင်းကိန်းရှာ၍မရပါ။"
        );
        assert_eq!(
            run_source("print(4 % 0.0);\n").unwrap_err(),
            "E039 လိုင်း 1 တွင် သုညဖြင့် ကြွင်းကိန်းရှာ၍မရပါ။"
        );
        // Inside a function the line is the one written there...
        assert_eq!(
            run_source("fn f(n :int) {\n    return n / 0;\n}\nf(1);\n").unwrap_err(),
            "E036 လိုင်း 2 တွင် သုညဖြင့် စား၍မရပါ။"
        );
        // ...and the error is a normal run-time one: catchable by its code.
        let src = "စမ်းရန်\n    print(1 % 0);\nE038 ကို ဖမ်းပါ။\n    print(\"caught\");\nပြီး။\n";
        assert_eq!(out(src), vec!["caught"]);
    }

    // -----------------------------------------------------------------
    // map / filter / reduce
    // -----------------------------------------------------------------

    #[test]
    fn the_callback_builtins_run_lambdas_and_named_functions() {
        let src = r#"xs မှာ [1, 2, 3] ဖြစ်၏။
print(map(xs, x => x * 2));
print(filter(xs, x => x > 1));
print(reduce(xs, (a, b) => a + b, 0));
fn double(n :int) {
    return n * 2;
}
print(map(xs, double));
print(reduce(xs, (a, b) => a * b, 1));
print(map("ab", c => c + "!"));
print(map([], x => x));
print(reduce([], (a, b) => a - b, 9));
print(map({3, 1, 2}, x => x + 1));
print(filter({3, 1, 2}, x => x > 1));
"#;
        assert_eq!(
            out(src),
            vec![
                "[2, 4, 6]",
                "[2, 3]",
                "6",
                "[2, 4, 6]",
                "6",
                "[\"a!\", \"b!\"]",
                "[]",
                "9",
                "[4, 2, 3]",
                "[3, 2]",
            ]
        );
    }

    #[test]
    fn a_lambda_sees_globals_and_may_call_another_callback_builtin() {
        let src = "xs မှာ [1, 2] ဖြစ်၏။\nk = 3;\nprint(map(xs, x => x * k));\nprint(map(xs, x => len(map(xs, y => y))));\n";
        assert_eq!(out(src), vec!["[3, 6]", "[2, 2]"]);
    }

    #[test]
    fn a_callback_builtin_keeps_the_interpreters_errors() {
        // A callback that returns something other than a bool: `E110`.
        let err = run_source("print(filter([1], x => x + 1));\n").unwrap_err();
        assert!(err.contains("E110"), "{err}");
        // A callback of the wrong arity: `E033`.
        let err = run_source("print(map([1], (a, b) => a));\n").unwrap_err();
        assert!(err.contains("E033"), "{err}");
        // A callback that returns nothing: `E114`.
        let src = "xs မှာ [1] ဖြစ်၏။\nfn shout(s :int) {\n    print(s);\n    return;\n}\nprint(map(xs, shout));\n";
        let err = run_source(src).unwrap_err();
        assert!(err.contains("E114"), "{err}");
        // A value that cannot be walked: `E110`.
        let err = run_source("print(map(5, x => x));\n").unwrap_err();
        assert!(err.contains("E110"), "{err}");
        // ...and `reduce`'s starting value is not evaluated before that check.
        let src = "fn noise() {\n    print(\"noise\");\n    return 0;\n}\nစမ်းရန်\n    print(reduce(5, (a, b) => a + b, noise()));\nE110 ကို ဖမ်းပါ။\n    print(\"caught\");\nပြီး။\nprint(\"done\");\n";
        assert_eq!(out(src), vec!["caught", "done"]);
    }

    #[test]
    fn an_error_inside_a_callback_is_caught_like_any_other() {
        // A handler outside the call takes over, and the loop does not resume.
        let src = "xs မှာ [1, 2] ဖြစ်၏။\nစမ်းရန်\n    print(map(xs, x => nosuchvar));\nE030 ကို ဖမ်းပါ။\n    print(\"caught\");\nပြီး။\nprint(\"after\");\n";
        assert_eq!(out(src), vec!["caught", "after"]);
        // A handler inside the lambda body keeps the walk going.
        let src = "xs မှာ [1, 2] ဖြစ်၏။\nprint(map(xs, fn(x) {\n    စမ်းရန်\n        return nosuchvar;\n    E030 ကို ဖမ်းပါ။\n        return -1;\n    ပြီး။\n}));\n";
        assert_eq!(out(src), vec!["[-1, -1]"]);
    }

    #[test]
    fn a_callback_builtin_compiles_as_a_statement_and_in_an_assignment() {
        let src = "xs မှာ [1, 2] ဖြစ်၏။\nmap(xs, x => x);\ny အတွက် map(xs, x => x + 1) ကို လုပ်ပါ။\nprint(y);\n";
        assert_eq!(out(src), vec!["[2, 3]"]);
    }

    #[test]
    fn a_callback_that_is_not_callable_is_e111() {
        // A plain value in the callback position now reaches the run, where
        // the interpreter's own E111 refuses it.
        let err = run_source("print(map([1], 3));\n").unwrap_err();
        assert!(err.contains("E110"), "{err}");
        assert!(err.contains("ပြန်ခေါ် အလုပ်"), "{err}");
    }

    #[test]
    fn a_library_callback_the_vm_cannot_represent_is_e170() {
        let err = compile_source("print(map([1], Tools.add));\n").unwrap_err();
        assert!(err.contains("E170"), "{err}");
    }

    // -----------------------------------------------------------------
    // a function's result, stored in a variable
    // -----------------------------------------------------------------

    /// Regression: `<var> အတွက် <fn>(...) ကို လုပ်ပါ။` compiles and the value is
    /// usable afterwards, exactly as under the interpreter.
    #[test]
    fn a_functions_result_is_stored_and_then_used() {
        let src = "လုပ်ငန်း twice အတွက် x ဖြင့်\n    x * 2\nပြီး။\ny အတွက် twice ကို လုပ်ရန် 21 ဖြင့်။\ny ကို ဖော်ပြပါ။\nprint(y + 1);\n";
        assert_eq!(out(src), vec!["42", "43"]);
    }

    #[test]
    fn a_body_that_ends_in_a_declaration_returns_that_value() {
        let src = "လုပ်ငန်း f အတွက် n ဖြင့်\n    r သည် n + 1 ဖြစ်၏။\nပြီးပါပြီ။\nx အတွက် f ကို လုပ်ရန် 41 ဖြင့်။\nx ကို ဖော်ပြပါ။\n".replace("ပြီးပါပြီ။", "ပြီး။");
        assert_eq!(out(&src), vec!["42"]);
    }

    #[test]
    fn a_call_that_returns_nothing_is_e071_when_stored() {
        // A body whose last statement is a print has no value.
        let src = "လုပ်ငန်း shout အတွက် s ဖြင့်\n    \"noise\" ကို ဖော်ပြပါ။\nပြီး။\nx အတွက် shout ကို လုပ်ရန် \"a\" ဖြင့်။\n";
        let err = run_source(src).unwrap_err();
        assert!(err.contains("E071"), "{err}");
        // A bare `return;` is the same.
        let err = run_source("fn v() {\n    print(\"side\");\n    return;\n}\ny အတွက် v ကို လုပ်ပါ။\n").unwrap_err();
        assert!(err.contains("E071"), "{err}");
        // And it is catchable, as any run-time error is.
        let src = "fn v() {\n    print(\"side\");\n}\nစမ်းရန်\n    y အတွက် v ကို လုပ်ပါ။\nE071 ကို ဖမ်းပါ။\n    print(\"E071 ok\");\nပြီး။\n";
        assert_eq!(out(src), vec!["side", "E071 ok"]);
    }

    #[test]
    fn assigning_a_constructor_or_builtin_result_works() {
        let src = "ပုံသေ Point(x, y) သည်\n    လုပ်ငန်း Point()သည်\n        အတု px သည် x ဖြစ်၏။\n    ပြီး။\nပြီး။\np အတွက် Point(3, 4) ကို လုပ်ပါ။\np ၏ px ကို ဖော်ပြပါ။\nn အတွက် len(\"abcd\") ကို လုပ်ပါ။\nprint(n + 1);\n";
        assert_eq!(out(src), vec!["3", "5"]);
    }

    #[test]
    fn a_bad_callee_in_an_assigning_call_is_checked_first() {
        // Unknown function: E031, before the arguments would run.
        let err = run_source("z အတွက် nosuchfn(1) ကို လုပ်ပါ။\n").unwrap_err();
        assert!(err.contains("E031"), "{err}");
        // Wrong argument count: E033, before the arguments would run.
        let err = run_source("fn f(a :int) {\n    return a;\n}\nz အတွက် f() ကို လုပ်ပါ။\n")
            .unwrap_err();
        assert!(err.contains("E033"), "{err}");
    }

    /// A missing variable reports `E030` with its line, as the interpreter
    /// does -- the value a stored call result leaves behind is no exception.
    #[test]
    fn a_missing_variable_reports_its_line() {
        let err = run_source("print(\"a\");\nprint(nosuchvar);\n").unwrap_err();
        assert_eq!(
            err,
            "E030 လိုင်း 2 တွင် \"nosuchvar\" ဆိုသော ကိန်းရှင်ကို ရှာမတွေ့ပါ။"
        );
    }

    // -----------------------------------------------------------------
    // libraries
    // -----------------------------------------------------------------

    #[test]
    fn library_calls_compile_and_run() {
        let src = "use ကျပန်း;\nprint(ကျပန်း.ကိန်း(5, 6));\nprint(ကျပန်း.တန်ဖိုး([\"A\"]));\nprint(ကျပန်း.random_int(1, 2));\n";
        assert_eq!(out(src), vec!["5", "A", "1"]);
    }

    #[test]
    fn a_library_must_be_imported_before_it_is_called() {
        let err = run_source("print(ကျပန်း.ကိန်း(1, 2));\n").unwrap_err();
        assert!(err.contains("E089"), "{err}");
        assert_eq!(out("use ကျပန်း;\nprint(ကျပန်း.ကိန်း(1, 2));\n"), vec!["1"]);
    }

    #[test]
    fn library_errors_match_the_interpreter() {
        for (src, code) in [
            // unknown library
            ("nosuchlib.get(\"x\");\n", "E085"),
            // unknown function in a known library
            ("use ကျပန်း;\nprint(ကျပန်း.nosuchfn(1));\n", "E086"),
            // wrong argument count
            ("use ကျပန်း;\nprint(ကျပန်း.ကိန်း(1));\n", "E087"),
            // a call that acts without a value, used as one
            ("use အချိန်;\nprint(အချိန်.စောင့်(0));\n", "E117"),
            // a call that acts without a value, stored into a name
            ("use အချိန်;\nx အတွက် အချိန် ၏ စောင့်(0) ကို လုပ်ပါ။\n", "E088"),
        ] {
            let err = run_source(src).unwrap_err();
            assert!(err.contains(code), "{src} => {err}");
        }
    }

    #[test]
    fn a_library_alias_spelling_resolves_to_the_library() {
        let src = "နည်းပညာများ ကျပန်း အဖြစ် r ကို အသုံးပြုပါ။\nprint(r.ကိန်း(5, 6));\n";
        assert_eq!(out(src), vec!["5"]);
    }

    #[test]
    fn a_library_error_is_catchable_by_code() {
        let src = "စမ်းရန်\n    ကျပန်း.ကိန်း(1, 2);\nE089 ကို ဖမ်းပါ။\n    print(\"E089 ok\");\nပြီး။\nprint(\"done\");\n";
        assert_eq!(out(src), vec!["E089 ok", "done"]);
    }

    // -----------------------------------------------------------------
    // loop-as-value
    // -----------------------------------------------------------------

    #[test]
    fn a_loop_value_is_the_value_passed_to_break() {
        let src = "x :int = loop {\n    x += 1;\n    if (x == 3) {\n        break x * 10;\n    }\n};\nprint(x);\ny :str = loop {\n    break \"text\";\n};\nprint(y);\n";
        assert_eq!(out(src), vec!["30", "text"]);
    }

    #[test]
    fn a_break_without_a_value_in_a_value_loop_is_e115() {
        let src = "print(\"start\");\nv :int = loop {\n    break;\n};\nprint(v);\n";
        let err = run_source(src).unwrap_err();
        assert!(err.contains("E115"), "{err}");
        assert!(err.contains("လိုင်း 2"), "{err}");
    }

    #[test]
    fn a_nested_loop_value_targets_the_innermost_loop() {
        let src = "total :int = 0;\ni :int = 0;\nwhile (i < 3) {\n    i += 1;\n    inner :int = loop {\n        if (i == 2) {\n            break 100;\n        }\n        break i;\n    };\n    total += inner;\n}\nprint(total);\n";
        assert_eq!(out(src), vec!["104"]);
    }

    #[test]
    fn a_plain_loop_ignores_a_break_value() {
        let src = "i :int = 0;\nloop {\n    i += 1;\n    if (i == 3) {\n        break 99;\n    }\n}\nprint(i);\nloop {\n    if (i == 5) {\n        break;\n    }\n    i += 1;\n}\nprint(i);\n";
        assert_eq!(out(src), vec!["3", "5"]);
    }

    #[test]
    fn a_break_value_leaves_a_try_without_running_finally() {
        // A `break` is a signal in the interpreter, so the `finally` of a `try`
        // it is leaving does not run -- the loop value must skip it too.
        let src = "n :int = loop {\n    စမ်းရန်\n        print(\"body\");\n        break 7;\n    နောက်ဆုံးတွင်\n        print(\"fin\");\n    ပြီး။\n};\nprint(n);\n";
        assert_eq!(out(src), vec!["body", "7"]);
    }

    #[test]
    fn a_break_value_from_a_catch_body_is_the_loop_value() {
        let src = "r :int = loop {\n    စမ်းရန်\n        n :int = \"no\";\n    E ကို ဖမ်းပါ။\n        print(\"caught\");\n        break 2;\n    နောက်ဆုံးတွင်\n        print(\"fin\");\n    ပြီး။\n};\nprint(r);\n";
        assert_eq!(out(src), vec!["caught", "2"]);
    }

    #[test]
    fn a_value_loop_can_continue_and_then_break() {
        let src = "k :int = 0;\nv :int = loop {\n    k += 1;\n    if (k < 3) {\n        continue;\n    }\n    break k;\n};\nprint(v);\nprint(k);\n";
        assert_eq!(out(src), vec!["3", "3"]);
    }

    #[test]
    fn a_pin_loop_value_is_a_constant_afterwards() {
        let src = "pin P :int = loop {\n    break 5;\n};\nprint(P);\nP = 6;\n";
        let err = run_source(src).unwrap_err();
        assert!(err.contains("E104"), "{err}");
    }

    #[test]
    fn a_loop_value_is_checked_against_the_declared_type() {
        let src = "n :str = loop {\n    break 1;\n};\nprint(n);\n";
        let err = run_source(src).unwrap_err();
        assert!(err.contains("E103"), "{err}");
    }

    #[test]
    fn return_outside_a_function_is_e108() {
        let err = run_source("print(\"before\");\nreturn;\n").unwrap_err();
        assert!(err.contains("E108"), "{err}");
        assert!(err.contains("လိုင်း 2"), "{err}");
    }

    /// Sets: literal (a repeated member kept once), membership, iteration and
    /// printing.
    #[test]
    fn sets_compile_and_run() {
        let src = r#"s မှာ {1, 2, 2, 3} ဖြစ်၏။
            s ကို ဖော်ပြပါ။
            (s ထဲ 2) ဖြစ်ရင်
                "has2" ကို ပြပါ။
            မဖြစ်ရင်
                "no2" ကို ပြပါ။
            ပြီး။
            (s ထဲ 9) ဖြစ်ရင်
                "has9" ကို ပြပါ။
            မဖြစ်ရင်
                "no9" ကို ပြပါ။
            ပြီး။
            m သည် တစ်ခုစီ(s)
                m ကို ပြပါ။
            ပြီး။
"#;
        assert_eq!(
            out(src),
            vec!["{1, 2, 3}", "has2", "no9", "1", "2", "3"]
        );
    }

    /// A class: header parameters, `အတု` field writes in the constructor, the
    /// object printed, and both field-read spellings.
    #[test]
    fn classes_construct_objects_with_fields() {
        let src = r#"ပုံသေ Point(x, y) သည်
                လုပ်ငန်း Point()သည်
                    အတု px သည် x ဖြစ်၏။
                    အတု py သည် y ဖြစ်၏။
                ပြီး။
            ပြီး။
            p သည် Point(3, 4) ဖြစ်၏။
            p ကို ဖော်ပြပါ။
            p.px ကို ပြပါ။
            p ၏ py ကို ပြပါ။
"#;
        assert_eq!(out(src), vec!["Point {px: 3, py: 4}", "3", "4"]);
    }

    /// A field that is not there fails with the interpreter's `E082`.
    #[test]
    fn a_missing_field_reports_e082() {
        let err = run_source(
            r#"ပုံသေ P(x) သည်
                လုပ်ငန်း P()သည်
                    အတု a သည် x ဖြစ်၏။
                ပြီး။
            ပြီး။
            p သည် P(1) ဖြစ်၏။
            p.zz ကို ပြပါ။
"#,
        )
        .unwrap_err();
        assert_eq!(
            err,
            "E082 လိုင်း 7 တွင် အရာဝတ္ထု ၌ \"zz\" ဆိုသော အကွက် မရှိပါ။"
        );
    }

    /// Sets and classes survive the container round trip unchanged.
    #[test]
    fn sets_and_classes_round_trip_through_bytes() {
        let src = r#"ပုံသေ P(v) သည်
                လုပ်ငန်း P()သည်
                    အတု v သည် v ဖြစ်၏။
                ပြီး။
            ပြီး။
            s မှာ {1, 2, 3} ဖြစ်၏။
            s ကို ဖော်ပြပါ။
            obj သည် P(s) ဖြစ်၏။
            obj ကို ဖော်ပြပါ။
            obj.v ကို ပြပါ။
"#;
        let program = compiled(src);
        assert_eq!(program.classes.len(), 1);
        let bytes = program.encode().expect("encodes");
        let decoded = Program::decode(&bytes).expect("decodes");
        assert_eq!(decoded.classes.len(), 1);
        assert_eq!(decoded.classes[0].params, vec!["v"]);
        assert_eq!(
            run(&decoded),
            vec!["{1, 2, 3}", "P {v: {1, 2, 3}}", "{1, 2, 3}"]
        );
        assert_eq!(run(&decoded), run(&program));
        assert_eq!(
            decoded.encode().expect("re-encodes"),
            bytes,
            "sets and classes encode byte for byte"
        );
    }

    /// Dictionaries: literal, read, write, membership, iteration (keys), a
    /// nested dictionary and printing -- the whole surface the mission lists.
    #[test]
    fn dictionaries_compile_and_run() {
        let src = r#"d မှာ { "a" သည် 1 ဖြစ်၏။ "b" သည် 2 ဖြစ်၏။ } ဖြစ်၏။
            d ကို ဖော်ပြပါ။
            d["b"] ကို ဖော်ပြပါ။
            d["c"] မှာ 3 ဖြစ်၏။
            d ကို ဖော်ပြပါ။
            (d ထဲ "c") ဖြစ်ရင်
                "has-c" ကို ပြပါ။
            မဖြစ်ရင်
                "no-c" ကို ပြပါ။
            ပြီး။
            (d ထဲ "zz") ဖြစ်ရင်
                "has-zz" ကို ပြပါ။
            မဖြစ်ရင်
                "no-zz" ကို ပြပါ။
            ပြီး။
            k သည် တစ်ခုစီ(d)
                k ကို ဖော်ပြပါ။
            ပြီး။
            nested မှာ { "inner" သည် { "x" သည် 10 ဖြစ်၏။ } ဖြစ်၏။ } ဖြစ်၏။
            nested ကို ဖော်ပြပါ။
            inner သည် nested["inner"] ဖြစ်၏။
            inner["x"] ကို ဖော်ပြပါ။
            total သည် 0 ဖြစ်၏။
            key သည် တစ်ခုစီ(d)
                total သည် total + d[key] ဖြစ်၏။
            ပြီး။
            total ကို ဖော်ပြပါ။
"#;
        assert_eq!(
            out(src),
            vec![
                "{\"a\": 1, \"b\": 2}",
                "2",
                "{\"a\": 1, \"b\": 2, \"c\": 3}",
                "has-c",
                "no-zz",
                "a",
                "b",
                "c",
                "{\"inner\": {\"x\": 10}}",
                "10",
                "6",
            ]
        );
    }

    /// A missing key fails with the interpreter's `E053`, key included.
    #[test]
    fn a_missing_dictionary_key_reports_e053() {
        let err = run_source(
            "d မှာ { \"a\" သည် 1 ဖြစ်၏။ } ဖြစ်၏။\nd[\"nope\"] ကို ဖော်ပြပါ။\n",
        )
        .unwrap_err();
        assert_eq!(err, "E053 လိုင်း 2 တွင် သော့ \"nope\" ကို ရှာမတွေ့ပါ။");
    }

    /// A dictionary constant survives the container round trip unchanged,
    /// inside a function body as well as at the top level.
    #[test]
    fn dictionaries_round_trip_through_bytes() {
        let src = r#"လုပ်ငန်း show()သည်
                inner မှာ { "a" သည် 1 ဖြစ်၏။ "b" သည် [1, 2] ဖြစ်၏။ } ဖြစ်၏။
                inner ကို ဖော်ပြပါ။
            ပြီး။
            show ကို လုပ်ပါ။
            m မှာ { "n" သည် { "x" သည် 1 ဖြစ်၏။ } ဖြစ်၏။ } ဖြစ်၏။
            m ကို ဖော်ပြပါ။
"#;
        let program = compiled(src);
        let bytes = program.encode().expect("encodes");
        let decoded = Program::decode(&bytes).expect("decodes");
        assert_eq!(run(&decoded), run(&program));
        assert_eq!(
            run(&decoded),
            vec!["{\"a\": 1, \"b\": [1, 2]}", "{\"n\": {\"x\": 1}}"]
        );
        assert_eq!(
            decoded.encode().expect("re-encodes"),
            bytes,
            "the dictionary encodes byte for byte"
        );
    }

    fn compiled(src: &str) -> Program {
        compile_source(src).expect("compiles")
    }

    fn run(program: &Program) -> Vec<String> {
        let mut vm = Vm::new(program);
        vm.run().expect("runs");
        vm.out
    }

    /// A program with a lambda value survives encode -> decode unchanged and
    /// runs the same, so the new call instructions are part of the container.
    #[test]
    fn a_lambda_program_round_trips_through_bytes() {
        let src = r#"fn adder(n) { return fn(x) { return x + n; }; }
add5 = adder(5);
print(add5(1));
print(map([1, 2], add5));
"#;
        let program = compiled(src);
        let bytes = program.encode().expect("encodes");
        let decoded = Program::decode(&bytes).expect("decodes");
        assert_eq!(run(&decoded), vec!["6", "[6, 7]"]);
        assert_eq!(run(&decoded), run(&program));
        assert_eq!(
            decoded.encode().expect("re-encodes"),
            bytes,
            "the lambda instructions encode byte for byte"
        );
    }

    /// A program with a function, a loop and a string constant survives
    /// encode -> decode unchanged, runs the same, and re-encodes byte for byte.
    #[test]
    fn program_round_trips_through_bytes() {
        let src = r#"i သည် တစ်ခုစီ(0, 3)
    i ကို ဖော်ပြပါ။
ပြီး။
လုပ်ငန်း double(n)သည်
    n * 2
ပြီး။
x သည် double(21) ဖြစ်၏။
x ကို ဖော်ပြပါ။
"မြန်မာ" ကို ပြပါ။
"#;
        let program = compiled(src);
        let bytes = program.encode().expect("encodes");
        assert!(bytes.starts_with(&MAGIC), "container magic first");
        let decoded = Program::decode(&bytes).expect("decodes");

        assert_eq!(decoded.funcs.len(), program.funcs.len());
        assert_eq!(decoded.code.len(), program.code.len());
        assert_eq!(run(&decoded), run(&program));
        assert_eq!(
            decoded.encode().expect("re-encodes"),
            bytes,
            "encoding is stable"
        );
    }

    /// A container from another format revision is refused loudly.
    #[test]
    fn a_foreign_format_version_is_refused() {
        let bytes = compiled("\"hi\" ကို ပြပါ။\n").encode().expect("encodes");
        let mut broken = bytes.clone();
        let next = (FORMAT_VERSION + 7).to_le_bytes();
        broken[8..10].copy_from_slice(&next);
        let err = Program::decode(&broken).unwrap_err();
        assert!(err.starts_with("E175"), "{err}");
        assert!(err.contains("ဗားရှင်း"), "{err}");
    }

    /// Half a container is a read error, never a partial program.
    /// A container written by another revision is refused with `E175`, naming
    /// both versions, so a stale artifact never runs with the wrong layout.
    #[test]
    fn a_container_from_another_version_is_refused() {
        let program = compile_source("print(1);\n").expect("compiles");
        let mut bytes = program.encode().expect("encodes");
        bytes[8] = 13;
        bytes[9] = 0;
        let err = Program::decode(&bytes).unwrap_err();
        assert!(err.contains("E175"), "{err}");
        assert!(err.contains("13"), "{err}");
        assert!(err.contains(&FORMAT_VERSION.to_string()), "{err}");
        // Revision 14 (the format before lambdas became values) is stale too.
        let mut bytes = program.encode().expect("encodes");
        bytes[8] = 14;
        bytes[9] = 0;
        let err = Program::decode(&bytes).unwrap_err();
        assert!(err.contains("E175"), "{err}");
        assert!(err.contains("14"), "{err}");
        assert!(err.contains(&FORMAT_VERSION.to_string()), "{err}");
    }

    #[test]
    fn a_truncated_container_is_refused() {
        let bytes = compiled("\"hi\" ကို ပြပါ။\n").encode().expect("encodes");
        let err = Program::decode(&bytes[..bytes.len() / 2]).unwrap_err();
        assert!(err.starts_with("E175"), "{err}");
    }

    /// Junk after the top-level chunk is rejected rather than ignored.
    #[test]
    fn trailing_bytes_are_refused() {
        let mut bytes = compiled("\"hi\" ကို ပြပါ။\n").encode().expect("encodes");
        bytes.push(0xAA);
        let err = Program::decode(&bytes).unwrap_err();
        assert!(err.contains("အပို"), "{err}");
    }
}
