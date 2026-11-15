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
//! Anything else -- classes, libraries, `input`, JSON, App -- is refused with
//! `E170` at compile time rather than silently behaving differently from the
//! interpreter.

use std::collections::HashMap;
use std::rc::Rc;

use indexmap::{IndexMap, IndexSet};

use crate::interpreter::{value_contains, value_eq, Key, ObjectValue, Value};
use crate::parser::{CondAtom, CondChain, Expr, ForSource, IfBranch, LogicalOp, Stmt};

/// One instruction. Every jump holds an absolute index into its own chunk
/// (the main code or one function body).
#[derive(Debug, Clone)]
pub enum Instr {
    Const(Value),
    Load(String),
    Store(String),
    Pop,
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Neg,
    Not,
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
    pub code: Vec<Instr>,
}

/// A whole compiled program: the top-level chunk plus every function body.
#[derive(Debug, Clone, Default)]
pub struct Program {
    pub code: Vec<Instr>,
    pub funcs: Vec<FuncDef>,
    pub classes: Vec<ClassDef>,
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

fn div_zero() -> String {
    "E173 တွင် သုညဖြင့် စား၍မရပါ။".to_string()
}

fn unknown_var(name: &str) -> String {
    format!(
        "E030 တွင် \"{}\" ဆိုသော ကိန်းရှင်ကို ရှာမတွေ့ပါ။",
        name
    )
}

// ---------------------------------------------------------------------------
// Compiler
// ---------------------------------------------------------------------------

/// A loop being compiled: where it starts, and the jump sites that still have
/// to point at the loop's exit / its next iteration.
#[derive(Default)]
struct LoopCtx {
    start: usize,
    breaks: Vec<usize>,
    continues: Vec<usize>,
    /// How many `try` handlers were live when the loop started, so a
    /// `break`/`continue` out of a `try` inside the loop drops exactly the
    /// handlers it leaves behind.
    handlers: usize,
}

struct Compiler {
    /// The chunk being written (main code, or one function body).
    code: Vec<Instr>,
    funcs: Vec<FuncDef>,
    /// Slot in `funcs` by name, so a call compiles even before the callee's
    /// body is written.
    slots: HashMap<String, usize>,
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
                    '/' => Instr::Div,
                    '%' => Instr::Mod,
                    other => {
                        return Err(unsupported(
                            *line,
                            &format!("'{}' လုပ်ဆောင်ချက်", other),
                        ))
                    }
                };
                self.emit(Instr::PinGuard(name.to_string(), *line as u32));
                self.emit(Instr::Load(name.to_string()));
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
            // `break;` -- jump past the innermost loop. With no loop to stop
            // it fails at run time with the interpreter's `E116`.
            Stmt::EngBreak { value, line } => {
                self.line = *line;
                if value.is_some() {
                    // A `break <value>` is the value of a `loop { }` used as
                    // an expression, which is not compiled yet.
                    return Err(unsupported(*line, "အဖိုးအတွက် loop ရပ်ခြင်း"));
                }
                if self.loops.is_empty() {
                    self.emit(Instr::Abort(
                        crate::interpreter::eng_break_outside_loop_err(*line),
                    ));
                    return Ok(());
                }
                let target = self.loops.last().expect("loop context").handlers;
                self.leave_tries(target)?;
                let at = self.emit(Instr::Jump(usize::MAX));
                self.loops.last_mut().expect("loop context").breaks.push(at);
                Ok(())
            }
            // `continue;` -- jump back to the top of the innermost loop.
            Stmt::Continue { line } => {
                self.line = *line;
                if self.loops.is_empty() {
                    self.emit(Instr::Abort(
                        crate::interpreter::eng_continue_outside_loop_err(*line),
                    ));
                    return Ok(());
                }
                let target = self.loops.last().expect("loop context").handlers;
                self.leave_tries(target)?;
                let at = self.emit(Instr::Jump(usize::MAX));
                self.loops
                    .last_mut()
                    .expect("loop context")
                    .continues
                    .push(at);
                Ok(())
            }
            // `loop { ... }` -- run the body until a `break` leaves it.
            Stmt::EngLoop { body } => {
                let start = self.here();
                self.loops.push(LoopCtx {
                    start,
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
            // `name(args);` -- a call whose result is discarded.
            Stmt::EngExprCall { name, args, line } => {
                self.line = *line;
                let idx = self.callee(name)?;
                for arg in args {
                    self.expr(arg)?;
                }
                self.emit(Instr::Call(idx, args.len() as u8));
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
                self.emit(Instr::Load(name.to_string()));
                self.expr(amount)?;
                self.emit(match op {
                    '+' => Instr::Add,
                    '-' => Instr::Sub,
                    '*' => Instr::Mul,
                    '/' => Instr::Div,
                    '%' => Instr::Mod,
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
            Stmt::FuncCall { name, args, .. } => {
                let idx = self.callee(name)?;
                for arg in args {
                    self.expr(arg)?;
                }
                self.emit(Instr::Call(idx, args.len() as u8));
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
                self.emit(Instr::Load(name.to_string()));
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
                match value {
                    Some(v) => self.expr(v)?,
                    None => {
                        self.emit(Instr::Const(Value::Null));
                    }
                }
                // A `return` inside a `try` runs its `finally` first and drops
                // the handlers of this function.
                self.leave_tries(0)?;
                self.emit(Instr::Return(*line as u32));
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

    fn callee(&self, name: &str) -> Result<usize, String> {
        self.slots.get(name).copied().ok_or_else(|| {
            format!(
                "E174 လိုင်း {} တွင် \"{}\" ဆိုသော အလုပ်ကို bytecode တွင် မတွေ့ပါ။",
                self.line, name
            )
        })
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
            start,
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
        self.emit(Instr::Load(step_var.clone()));
        self.emit(Instr::Const(Value::Int(0)));
        self.emit(Instr::Gt);
        let goes_down = self.emit(Instr::JumpIfFalse(usize::MAX));
        self.emit(Instr::Load(var_name.to_string()));
        self.emit(Instr::Load(end_var.clone()));
        self.emit(Instr::Lt);
        let test_done = self.emit(Instr::Jump(usize::MAX));
        let down_at = self.here();
        self.patch_jump(goes_down, down_at);
        self.emit(Instr::Load(var_name.to_string()));
        self.emit(Instr::Load(end_var.clone()));
        self.emit(Instr::Gt);
        let test_end = self.here();
        self.patch_jump(test_done, test_end);

        let exit = self.emit(Instr::JumpIfFalse(usize::MAX));
        self.loops.push(LoopCtx {
            start: start_pc,
            handlers: self.handlers,
            ..Default::default()
        });
        self.block(body)?;
        let ctx = self.loops.pop().expect("loop context");
        // A `continue` re-tests, so it lands on the step below, not on it.
        for site in ctx.continues {
            self.patch_jump(site, self.here());
        }
        self.emit(Instr::Load(var_name.to_string()));
        self.emit(Instr::Load(step_var));
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
        self.emit(Instr::Load(idx_var.clone()));
        self.emit(Instr::Load(seq_var.clone()));
        self.emit(Instr::Len);
        self.emit(Instr::Lt);
        let exit = self.emit(Instr::JumpIfFalse(usize::MAX));

        self.emit(Instr::Load(seq_var.clone()));
        self.emit(Instr::Load(idx_var.clone()));
        self.emit(Instr::Index(self.line as u32, 1));
        self.emit(Instr::Store(var_name.to_string()));

        self.loops.push(LoopCtx {
            start: start_pc,
            handlers: self.handlers,
            ..Default::default()
        });
        self.block(body)?;
        let ctx = self.loops.pop().expect("loop context");
        for site in ctx.continues {
            self.patch_jump(site, self.here());
        }

        self.emit(Instr::Load(idx_var.clone()));
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
            self.emit(Instr::Not);
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
            self.emit(Instr::Not);
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
                self.emit(Instr::Load(name.to_string()));
                Ok(())
            }
            Expr::Binary(lhs, op, rhs, line) => {
                self.expr(lhs)?;
                self.expr(rhs)?;
                self.emit(match op {
                    '+' => Instr::Add,
                    '-' => Instr::Sub,
                    '*' => Instr::Mul,
                    '/' => Instr::Div,
                    '%' => Instr::Mod,
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
            Expr::Not(inner, _) => {
                self.expr(inner)?;
                self.emit(Instr::Not);
                Ok(())
            }
            Expr::Neg(inner, _) => {
                self.expr(inner)?;
                self.emit(Instr::Neg);
                Ok(())
            }
            Expr::ListLit(items) | Expr::TupleLit(items) => {
                let mut values = Vec::with_capacity(items.len());
                for item in items {
                    values.push(literal(item, self.line)?);
                }
                let value = match expr {
                    Expr::TupleLit(_) => Value::Tuple(values),
                    _ => Value::List(values),
                };
                self.emit(Instr::Const(value));
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
            Expr::NewObj(name, args, _) if self.slots.contains_key(name) => {
                let idx = self.slots[name];
                for arg in args {
                    self.expr(arg)?;
                }
                self.emit(Instr::Call(idx, args.len() as u8));
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
    // Pass one: every function and class name gets a slot, so a call compiled
    // before the callee's body is written still finds it.
    register_functions(stmts, &mut compiler);
    register_classes(stmts, &mut compiler)?;

    // Pass two: compile each body into its slot, then the top-level chunk.
    for (index, def) in collect_defs(stmts).into_iter().enumerate() {
        let Stmt::FuncDef { body, .. } = def else {
            unreachable!("collect_defs yields only function definitions")
        };
        let saved_code = std::mem::take(&mut compiler.code);
        let saved_loops = std::mem::take(&mut compiler.loops);
        let saved_handlers = compiler.handlers;
        compiler.handlers = 0;
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
/// interpreter uses: `... ပြီး။` returns the final bare expression.
fn compile_body(compiler: &mut Compiler, body: &[Stmt]) -> Result<(), String> {
    match body.split_last() {
        Some((Stmt::ExprStmt { value, .. }, rest)) => {
            compiler.block(rest)?;
            compiler.expr(value)?;
        }
        Some((last, rest)) => {
            compiler.block(rest)?;
            compiler.stmt(last)?;
            compiler.emit(Instr::Const(Value::Null));
        }
        None => {
            compiler.emit(Instr::Const(Value::Null));
        }
    }
    compiler.emit(Instr::Return(0));
    Ok(())
}

fn register_functions(stmts: &[Stmt], compiler: &mut Compiler) {
    for stmt in stmts {
        match stmt {
            Stmt::FuncDef { name, params, .. } => {
                if !compiler.slots.contains_key(name) {
                    compiler.slots.insert(name.clone(), compiler.funcs.len());
                    compiler.funcs.push(FuncDef {
                        name: name.clone(),
                        params: params.clone(),
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
    /// The message of the error a handler is currently running for.
    pending_error: Option<String>,
    /// Set when the program has stopped (its `Halt`, or a `Return` out of the
    /// top-level chunk), so the run loop can tell that from an error.
    halted: bool,
    /// Everything the program wrote, byte for byte -- the `.akop` transcript.
    pub log: String,
    /// A scripted stdin (tests); `None` reads the process's own stdin.
    input: Option<Box<dyn std::io::BufRead>>,
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
            pending_error: None,
            halted: false,
            log: String::new(),
            input: None,
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

    fn load(&self, name: &str) -> Result<Value, String> {
        if let Some(frame) = self.frames.last() {
            if let Some(v) = frame.locals.get(name) {
                return Ok(v.clone());
            }
        }
        self.globals
            .get(name)
            .cloned()
            .ok_or_else(|| unknown_var(name))
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

    fn arith(&mut self, op: char) -> Result<(), String> {
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
        let value = match op {
            '+' => a + b,
            '-' => a - b,
            '*' => a * b,
            '/' => {
                if b == 0.0 {
                    return Err(div_zero());
                }
                a / b
            }
            '%' => {
                if b == 0.0 {
                    return Err(div_zero());
                }
                a % b
            }
            other => return Err(math_type_err(other)),
        };
        if both_int {
            self.stack.push(Value::Int(value as i64));
        } else {
            self.stack.push(Value::Float(value));
        }
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
                Instr::Load(name) => {
                    let v = self.load(&name)?;
                    self.stack.push(v);
                    self.set_pc(pc + 1);
                }
                Instr::Store(name) => {
                    let v = self.pop();
                    self.store(&name, v);
                    self.set_pc(pc + 1);
                }
                Instr::Pop => {
                    self.pop();
                    self.set_pc(pc + 1);
                }
                Instr::Add => {
                    self.arith('+')?;
                    self.set_pc(pc + 1);
                }
                Instr::Sub => {
                    self.arith('-')?;
                    self.set_pc(pc + 1);
                }
                Instr::Mul => {
                    self.arith('*')?;
                    self.set_pc(pc + 1);
                }
                Instr::Div => {
                    self.arith('/')?;
                    self.set_pc(pc + 1);
                }
                Instr::Mod => {
                    self.arith('%')?;
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
                Instr::Not => {
                    let v = self.pop();
                    self.stack.push(Value::Bool(!truthy(&v)?));
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
                            vm.load(name).map_err(|_| {
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
                    let params = self.prog.funcs[index].params.clone();
                    let mut locals = HashMap::new();
                    for (name, value) in params.iter().zip(args) {
                        locals.insert(name.clone(), value);
                    }
                    // The caller resumes at the instruction after the call;
                    // `Return` finds it there when the frame pops.
                    self.set_pc(pc + 1);
                    self.frames.push(Frame {
                        func: index,
                        pc: 0,
                        locals,
                        class: None,
                        self_fields: Vec::new(),
                    });
                }
                Instr::Return(line) => {
                    let value = self.stack.pop().unwrap_or(Value::Null);
                    if self.frames.is_empty() {
                        // `return` outside any function is `E108`, exactly as
                        // the interpreter reports it.
                        return Err(crate::interpreter::eng_return_outside_fn_err(
                            line as usize,
                        ));
                    }
                    match self.frames.pop() {
                        Some(frame) => {
                            // A constructor's result is the object it filled in,
                            // not whatever its last expression evaluated to.
                            let value = match frame.class {
                                Some(index) => Value::Object(Rc::new(ObjectValue {
                                    class: self.prog.classes[index].name.clone(),
                                    fields: frame.self_fields,
                                })),
                                None => value,
                            };
                            // Back in the caller, right after its `Call`.
                            self.stack.push(value);
                        }
                        None => self.halted = true,
                    }
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
pub fn compile_source(src: &str) -> Result<Program, String> {
    let tokens = crate::lexer::lex(src)?;
    let stmts = crate::parser::parse(&tokens)?;
    compile(&stmts)
}

/// Compile and run a source text, returning the lines it printed.
pub fn run_source(src: &str) -> Result<Vec<String>, String> {
    let program = compile_source(src)?;
    let mut vm = Vm::new(&program);
    vm.run()?;
    Ok(vm.out)
}

/// Compile and run a source text with scripted stdin, returning the lines it
/// printed -- what a program that asks for input does under a test.
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
pub const FORMAT_VERSION: u16 = 6;
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
            Instr::Load(name) => {
                out.push(1);
                put_str(out, name)?;
            }
            Instr::Store(name) => {
                out.push(2);
                put_str(out, name)?;
            }
            Instr::Pop => out.push(3),
            Instr::Add => out.push(4),
            Instr::Sub => out.push(5),
            Instr::Mul => out.push(6),
            Instr::Div => out.push(7),
            Instr::Mod => out.push(8),
            Instr::Neg => out.push(9),
            Instr::Not => out.push(10),
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
            1 => Instr::Load(reader.text()?),
            2 => Instr::Store(reader.text()?),
            3 => Instr::Pop,
            4 => Instr::Add,
            5 => Instr::Sub,
            6 => Instr::Mul,
            7 => Instr::Div,
            8 => Instr::Mod,
            9 => Instr::Neg,
            10 => Instr::Not,
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
            let code = decode_code(&mut reader)?;
            funcs.push(FuncDef {
                name,
                params,
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
        // A library call is still outside the compiler; sets, dictionaries and
        // classes are not.
        let err = run_source("ကျပန်း ၏ တန်ဖိုး([]) ကို လုပ်ပါ။\n").unwrap_err();
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
