//! The bytecode compiler: AST in, instructions out.

use super::*;
use crate::frontend::parser::{CondAtom, CondChain, Expr, ForSource, IfBranch, LogicalOp, Stmt};
use crate::interp::{Key, Value};
use crate::project::library::{is_builtin_library, LibraryLoader};
use indexmap::IndexMap;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

/// A builtin the VM can run from evaluated values alone.
///
/// The mutating list builtins (`push`, `pop`, ...) reach for a variable rather
/// than a value, so they stay `E170` in bytecode mode; `reverse` is compiled
/// by [`Compiler::reverse_site`] instead. `map`/`filter`/`reduce` are not
/// value-only either, but they have their own instructions -- see
/// [`is_higher_order`].
pub(crate) fn is_pure_builtin(name: &str) -> bool {
    crate::interp::is_eng_builtin(name)
        && !crate::interp::is_mutating_builtin(name)
        && !matches!(name, "reverse" | "map" | "filter" | "reduce")
}

/// The callback builtins: `map`, `filter` and `reduce`. They walk a value and
/// call a function for every item, so they compile to `IterItems` plus
/// `HigherOrder` with the lambda (or named function) as its own body.
pub(crate) fn is_mutating_builtin(name: &str) -> bool {
    // `reverse` is handled as a value too (a string reverses into a string),
    // so it keeps its own `E170` until that is compiled.
    name != "reverse" && crate::interp::is_mutating_builtin(name)
}

pub(crate) fn is_higher_order(name: &str) -> bool {
    crate::interp::is_eng_builtin(name) && matches!(name, "map" | "filter" | "reduce")
}

pub(crate) fn higher_order_name(kind: u8) -> &'static str {
    match kind {
        0 => "map",
        1 => "filter",
        _ => "reduce",
    }
}

pub(crate) fn unsupported(line: usize, what: &str) -> String {
    format!(
        "E170 လိုင်း {} တွင် {} ကို bytecode ဖြင့် မထောက်ပံ့သေးပါ။ `akk` (interpreter) ဖြင့် ဖွင့်ပါ။",
        line, what
    )
}

pub(crate) fn bool_expected(line: usize) -> String {
    format!(
        "E171 လိုင်း {} တွင် bytecode စက်သည် အခြေအနေကို bool အဖြစ်သာ စစ်နိုင်ပါသည်။",
        line
    )
}

pub(crate) fn math_type_err(op: char) -> String {
    format!("E172 တွင် '{}' ကို ဤ တန်ဖိုး အမျိုးအစားများဖြင့် မတွက်နိုင်ပါ။", op)
}

pub(crate) fn unknown_var(name: &str, line: usize) -> String {
    format!("E030 လိုင်း {} တွင် \"{}\" ဆိုသော ကိန်းရှင်ကို ရှာမတွေ့ပါ။", line, name)
}

/// A loop being compiled: the jump sites that still have to point at the
/// loop's exit / its next iteration.
#[derive(Default)]
pub(crate) struct LoopCtx {
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

pub(crate) struct Compiler {
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
    /// The file this compilation is reading, copied into every chunk it
    /// produces so a runtime error can name the right file.
    source: Rc<str>,
    /// The script/package libraries pulled in before compiling, when a
    /// loader was supplied. `None` keeps the old refusal: a library import
    /// that is not built in is `E170`.
    libraries: Option<Rc<LibraryTable>>,
    /// Library name -> the slot of the function that runs its top-level
    /// statements. Goes into `Program::script_libs`.
    script_init: HashMap<String, usize>,
    /// The library chunk being written, when this is a library function or
    /// init function. `None` for the main chunk, where an import cannot
    /// close a cycle.
    chunk: Option<usize>,
    /// (chunk slot, pc) of each library import -> its source line, so a
    /// cycle found at run time is reported at the importing line.
    import_lines: HashMap<(usize, usize), usize>,
}

/// A script or package library resolved before compiling: its own source
/// text, its parsed top-level statements, and the functions it defines. The
/// functions are what `<lib> ၏ <fn>` may call; the statements are compiled
/// into the library's init function.
struct ScriptLibrary {
    source: Rc<str>,
    stmts: Vec<Stmt>,
    functions: HashSet<String>,
}

/// Every script/package library a program (transitively) imports, in the
/// order their init functions must be laid down, plus the names the loader
/// could not find (their import becomes the interpreter's `E062`).
#[derive(Default)]
struct LibraryTable {
    units: HashMap<String, ScriptLibrary>,
    order: Vec<String>,
    unresolved: HashSet<String>,
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
            source: Rc::from(""),
            libraries: None,
            script_init: HashMap::new(),
            chunk: None,
            import_lines: HashMap::new(),
        }
    }

    /// The library `name` refers to, when it was compiled in.
    fn script_library(&self, name: &str) -> Option<&ScriptLibrary> {
        self.libraries.as_ref()?.units.get(name)
    }

    /// `Some(true)` when `name` is a compiled library that defines
    /// `fn_name`, `Some(false)` when it is a compiled library that does not.
    fn script_function(&self, name: &str, fn_name: &str) -> Option<bool> {
        self.script_library(name)
            .map(|unit| unit.functions.contains(fn_name))
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
                    '\\' => Instr::FloorDiv(*line as u32),
                    '%' => Instr::Mod(*line as u32),
                    other => return Err(unsupported(*line, &format!("'{}' လုပ်ဆောင်ချက်", other))),
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
                    self.emit(Instr::Abort(crate::interp::eng_break_outside_loop_err(
                        *line,
                    )));
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
                            self.emit(Instr::Abort(crate::interp::eng_loop_no_value_err(
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
                    self.emit(Instr::Abort(crate::interp::eng_continue_outside_loop_err(
                        *line,
                    )));
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
            // builtin library, or a script/package library that was resolved
            // and compiled in. Without a loader (an `akk build` compile) a
            // script library is still refused rather than run half-loaded.
            Stmt::UseLibrary { libs, line } => {
                for (name, alias) in libs {
                    if !is_builtin_library(name) {
                        if self.script_library(name).is_none() {
                            let table = self.libraries.as_ref();
                            let Some(table) = table else {
                                return Err(unsupported(
                                    *line,
                                    &format!("\"{}\" နည်းပညာများ တင်သွင်းခြင်း", name),
                                ));
                            };
                            if table.unresolved.contains(name) {
                                // The loader looked and found no such source:
                                // the interpreter's E062, raised when the
                                // import runs, before anything else.
                                self.emit(Instr::Abort(crate::interp::lib_not_found_err(
                                    name, *line,
                                )));
                                self.emit(Instr::Const(Value::Null));
                                continue;
                            }
                            // A library still loading (an import cycle): the
                            // import is a no-op run, as it is for a library
                            // already brought in.
                        }
                    }
                    let pc = self.emit(Instr::UseLib(name.clone()));
                    if !is_builtin_library(name) {
                        if let Some(chunk) = self.chunk {
                            self.import_lines.insert((chunk, pc), *line);
                        }
                    }
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
                if is_builtin_library(original) || self.script_library(original).is_some() {
                    for alias in aliases {
                        self.lib_aliases.insert(alias.clone(), original.clone());
                    }
                    let use_pc = self.emit(Instr::UseLib(original.clone()));
                    if !is_builtin_library(original) {
                        if let Some(chunk) = self.chunk {
                            self.import_lines.insert((chunk, use_pc), *line);
                        }
                    }
                    return Ok(());
                }
                self.emit(Instr::Abort(crate::interp::eng_missing_function_err(
                    *line, original,
                )));
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
                    self.emit(Instr::Const(crate::interp::eng_type_default(t)));
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
                target, op, amount, ..
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
                    '\\' => Instr::FloorDiv(self.line as u32),
                    '%' => Instr::Mod(self.line as u32),
                    other => return Err(unsupported(self.line, &format!("'{}' လုပ်ဆောင်ချက်", other))),
                });
                self.emit(Instr::Store(name.to_string()));
                Ok(())
            }
            Stmt::If { branches } => self.if_chain(branches),
            Stmt::While {
                cond, negate, body, ..
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
            Stmt::SelfFieldSet { field, value, line } => {
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
            // `<value> ကို <name> အမည်ဖြင့် ပစ်ပါ။` / `throw <value> as
            // <name>;` -- evaluate the message, then raise it with its
            // identity. The VM's run loop hands the formatted error to the
            // innermost handler, or ends the run with it.
            Stmt::Raise {
                identity,
                value,
                line,
            } => {
                self.line = *line;
                self.expr(value)?;
                self.emit(Instr::Throw(identity.clone(), *line as u32));
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
                        // `E` catches anything; otherwise the code -- or a
                        // raised error's identity -- must match, exactly as
                        // the interpreter decides it.
                        if code != "E" {
                            self.emit(Instr::ErrorMatches(code.clone()));
                            to_rethrow.push(self.emit(Instr::JumpIfFalse(usize::MAX)));
                        }
                    }
                    if let Some(var) = catch_var {
                        // The catch name holds the raised error's message, or
                        // the code for every other error, as there.
                        self.emit(Instr::ErrorBind);
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
        if let Some(defined) = self.script_function(&lib, fn_name) {
            // A script/package library function: import it (running its
            // top-level statements once), then call it like any other user
            // function. Its body was compiled with the library's own source,
            // so an error inside it names the library file and line.
            self.emit(Instr::RequireLib(lib.clone(), line as u32));
            if !defined {
                self.emit(Instr::Abort(crate::interp::lib_fn_unknown_err(
                    &lib, fn_name, line,
                )));
                self.emit(Instr::Const(Value::Null));
                return Ok(());
            }
            let (total, required) = self.arities[fn_name];
            let argc = args.len();
            if argc < required || argc > total {
                self.emit(Instr::Abort(crate::interp::eng_arity_err(
                    fn_name, argc, required, total, line,
                )));
                self.emit(Instr::Const(Value::Null));
                return Ok(());
            }
            let index = self.slots[fn_name];
            for arg in args {
                self.expr(arg)?;
            }
            match err {
                Some(message) => {
                    self.emit(Instr::CallValue(index, argc as u8, line as u32, message))
                }
                None => self.emit(Instr::Call(index, argc as u8)),
            };
            return Ok(());
        }
        if !is_builtin_library(&lib) {
            // Not a library this backend knows: `E085` when it runs, and the
            // arguments are not evaluated, exactly as there.
            self.emit(Instr::Abort(crate::interp::lib_unknown_err(&lib, line)));
            self.emit(Instr::Const(Value::Null));
            return Ok(());
        }
        if !crate::interp::lib_is_value_library(&lib) {
            return Err(unsupported(line, &format!("\"{}\" နည်းပညာ ခေါ်ခြင်း", lib)));
        }
        self.emit(Instr::RequireLib(lib.clone(), line as u32));
        match crate::interp::lib_value_arity(&lib, fn_name, line) {
            Err(message) => {
                // Unknown function in a known library: `E086`, before the
                // arguments would run.
                self.emit(Instr::Abort(message));
                self.emit(Instr::Const(Value::Null));
                return Ok(());
            }
            Ok(want) => {
                if args.len() != want {
                    let message =
                        crate::interp::lib_fn_argc_count(&lib, fn_name, args.len(), want, line)
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
                    self.emit(Instr::Abort(crate::interp::eng_arity_err(
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
            self.emit(Instr::Builtin(
                fn_name.to_string(),
                args.len() as u8,
                line as u32,
            ));
            return Ok(());
        }
        if crate::interp::is_eng_builtin(fn_name) {
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
            self.emit(Instr::Abort(crate::interp::eng_builtin_err(
                line,
                name,
                &format!("အချက် {want} ခု လိုအပ်ပါသည်၊ {} ရပါသည်", args.len()),
            )));
            self.emit(Instr::Const(Value::Null));
            return Ok(());
        }
        self.expr(&args[0])?;
        self.emit(Instr::IterItems(kind, line as u32));
        if matches!(&args[1], Expr::LibFnRef(..)) {
            return Err(unsupported(self.line, "နည်းပညာ အလုပ် ကို ပြန်ခေါ် အလုပ်အဖြစ်"));
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
            self.emit(Instr::Abort(crate::interp::eng_builtin_err(
                line,
                "reverse",
                &format!("စာရင်း သို့မဟုတ် စာသား တစ်ခု လိုအပ်ပါသည်၊ {} ရပါသည်", args.len()),
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
            let message = crate::interp::eng_builtin_err(
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
            source: Rc::clone(&self.source),
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
            let Some(param) = params.get(index) else {
                continue;
            };
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
                self.emit(Instr::Builtin(
                    name.to_string(),
                    args.len() as u8,
                    line as u32,
                ));
                return Ok(());
            }
            if crate::interp::is_eng_builtin(name) {
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
                self.emit(Instr::Abort(crate::interp::eng_arity_err(
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
                    '\\' => Instr::FloorDiv(*line as u32),
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
                let err = crate::interp::eng_lib_no_value_err(&resolved, fn_name, *line);
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
pub(crate) fn literal(expr: &Expr, line: usize) -> Result<Value, String> {
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

pub(crate) fn stmt_name(stmt: &Stmt) -> &'static str {
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
        Stmt::Raise { .. } => "အမှား ပစ်ခြင်း",
        _ => "ဤ ဖော်ပြချက်",
    }
}

pub(crate) fn stmt_line(stmt: &Stmt) -> usize {
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
        | Stmt::Raise { line, .. }
        | Stmt::SelfFieldSet { line, .. } => *line,
        _ => 0,
    }
}

pub(crate) fn expr_name(expr: &Expr) -> &'static str {
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

pub(crate) fn e096(line: usize, name: &str) -> String {
    format!(
        "E096 လိုင်း {} တွင် \"{}\" ပုံသေ၏ ခေါင်းစီးတွင် တည်ဆောက်သူ အချက် များ ရေးထားပြီးဖြစ်၍ ပထမ လုပ်ဆောင်ချက်သည် အချက် များ ထပ်မံမရေးရပါ။",
        line, name
    )
}

/// Register every class by name: its constructor parameters come from the
/// header's `(params)`, or from the first method when the header has none --
/// exactly the rule the interpreter applies.
pub(crate) fn register_classes(stmts: &[Stmt], compiler: &mut Compiler) -> Result<(), String> {
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
        compiler
            .class_slots
            .insert(name.clone(), compiler.classes.len());
        compiler.classes.push(ClassDef {
            name: name.clone(),
            params: ctor_params,
            code: Vec::new(),
            source: Rc::clone(&compiler.source),
        });
    }
    Ok(())
}

/// Every class definition, in source order (including ones written inside a
/// block).
pub(crate) fn collect_classes(stmts: &[Stmt]) -> Vec<&Stmt> {
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

/// Compile a parsed program into bytecode. `source` is the text the
/// statements came from: every chunk remembers it, so a runtime error is
/// reported against the file it was written in.
///
/// This is the `akk build` entry point and the tests' one: without a loader a
/// script/package library import stays `E170`, since a byte container does
/// not embed library code.
pub fn compile_with_source(stmts: &[Stmt], source: &str) -> Result<Program, String> {
    compile_impl(stmts, source, None)
}

/// Compile a program that may import script/package libraries. `loader`
/// resolves each imported name the way the interpreter does; the libraries'
/// top-level statements and function bodies are compiled into this program,
/// each carrying its own source for error reports.
pub fn compile_with_libraries(
    stmts: &[Stmt],
    source: &str,
    loader: &mut LibraryLoader,
) -> Result<Program, String> {
    let table = resolve_libraries(stmts, loader)?;
    compile_impl(stmts, source, Some(Rc::new(table)))
}

fn compile_impl(
    stmts: &[Stmt],
    source: &str,
    libraries: Option<Rc<LibraryTable>>,
) -> Result<Program, String> {
    let mut compiler = Compiler::new();
    compiler.source = Rc::from(source);
    compiler.libraries = libraries.clone();
    // A default parameter is filled in by the interpreter when the call runs,
    // in the callee's own frame, and this backend binds parameters only from
    // the arguments the call supplies. Refuse the program instead of running
    // it with a parameter left unbound.
    // Pass one: every function and class name gets a slot, so a call compiled
    // before the callee's body is written still finds it. The importing file
    // comes first; library definitions fill slots after it.
    register_functions(stmts, &mut compiler);
    register_classes(stmts, &mut compiler)?;
    if let Some(table) = &libraries {
        for name in &table.order {
            let unit = &table.units[name];
            // Slot registration also stamps each chunk with the source it is
            // written in, so a library's functions point at the library file.
            let saved_source = std::mem::replace(&mut compiler.source, Rc::clone(&unit.source));
            register_functions(&unit.stmts, &mut compiler);
            register_classes(&unit.stmts, &mut compiler)?;
            compiler.source = saved_source;
        }
    }

    // Pass two: compile each body into its slot, then each library's
    // top-level statements into its init function, then the main chunk.
    compile_function_bodies(&mut compiler, stmts)?;
    compile_class_bodies(&mut compiler, stmts)?;
    if let Some(table) = &libraries {
        for name in &table.order {
            let unit = &table.units[name];
            let saved_source = std::mem::replace(&mut compiler.source, Rc::clone(&unit.source));
            compile_function_bodies(&mut compiler, &unit.stmts)?;
            compile_class_bodies(&mut compiler, &unit.stmts)?;
            compiler.source = saved_source;
        }
        for name in &table.order {
            let unit = &table.units[name];
            let index = compiler.funcs.len();
            compiler.funcs.push(FuncDef {
                name: format!("<library {}>", name),
                params: Vec::new(),
                required: 0,
                code: Vec::new(),
                source: Rc::clone(&unit.source),
            });
            compiler.script_init.insert(name.clone(), index);
            let saved_code = std::mem::take(&mut compiler.code);
            let saved_loops = std::mem::take(&mut compiler.loops);
            let saved_handlers = compiler.handlers;
            let saved_source = std::mem::replace(&mut compiler.source, Rc::clone(&unit.source));
            let saved_chunk = compiler.chunk;
            compiler.chunk = Some(index);
            compiler.handlers = 0;
            compiler.block(&unit.stmts)?;
            compiler.emit(Instr::Const(Value::Null));
            compiler.emit(Instr::ReturnVoid(0));
            let code = std::mem::replace(&mut compiler.code, saved_code);
            compiler.loops = saved_loops;
            compiler.handlers = saved_handlers;
            compiler.chunk = saved_chunk;
            compiler.source = saved_source;
            compiler.funcs[index].code = code;
        }
    }

    compiler.block(stmts)?;
    compiler.emit(Instr::Halt);
    Ok(Program {
        code: compiler.code,
        funcs: compiler.funcs,
        classes: compiler.classes,
        source: compiler.source,
        script_libs: compiler.script_init,
        lib_imports: compiler.import_lines,
    })
}

/// Compile every function definition in `stmts` into the slot its name was
/// registered under. Looking the slot up by name keeps this working for a
/// library's statements as well as the main file's.
fn compile_function_bodies(compiler: &mut Compiler, stmts: &[Stmt]) -> Result<(), String> {
    for def in collect_defs(stmts) {
        let Stmt::FuncDef {
            name,
            params,
            defaults,
            body,
            ..
        } = def
        else {
            unreachable!("collect_defs yields only function definitions")
        };
        let index = compiler.slots[name];
        let saved_code = std::mem::take(&mut compiler.code);
        let saved_loops = std::mem::take(&mut compiler.loops);
        let saved_handlers = compiler.handlers;
        let saved_chunk = compiler.chunk;
        compiler.chunk = Some(index);
        compiler.handlers = 0;
        compiler.fill_defaults(params, defaults)?;
        compile_body(compiler, body)?;
        let code = std::mem::replace(&mut compiler.code, saved_code);
        compiler.loops = saved_loops;
        compiler.handlers = saved_handlers;
        compiler.chunk = saved_chunk;
        compiler.funcs[index].code = code;
    }
    Ok(())
}

/// Compile each class's constructor body (its first method) into the slot its
/// name was registered under.
fn compile_class_bodies(compiler: &mut Compiler, stmts: &[Stmt]) -> Result<(), String> {
    for class in collect_classes(stmts) {
        let Stmt::ClassDef { name, body, .. } = class else {
            unreachable!("collect_classes yields only class definitions")
        };
        let Some(Stmt::FuncDef {
            body: ctor_body, ..
        }) = body.first()
        else {
            continue;
        };
        let index = compiler.class_slots[name];
        let saved_code = std::mem::take(&mut compiler.code);
        let saved_loops = std::mem::take(&mut compiler.loops);
        let saved_handlers = compiler.handlers;
        // A constructor frame is identified by its class slot, not a function
        // slot; an import there is not cycle-recorded (its line falls back to
        // the statement itself being refused elsewhere).
        let saved_chunk = compiler.chunk;
        compiler.chunk = None;
        compiler.handlers = 0;
        compile_body(compiler, ctor_body)?;
        let code = std::mem::replace(&mut compiler.code, saved_code);
        compiler.loops = saved_loops;
        compiler.handlers = saved_handlers;
        compiler.chunk = saved_chunk;
        compiler.classes[index].code = code;
    }
    Ok(())
}

/// Find and parse every script/package library `stmts` imports, directly or
/// through another library. The libraries' own sources come from `loader`,
/// the same lookup the interpreter uses.
fn resolve_libraries(stmts: &[Stmt], loader: &mut LibraryLoader) -> Result<LibraryTable, String> {
    let mut table = LibraryTable::default();
    let mut loading = HashSet::new();
    load_imports(stmts, loader, &mut table, &mut loading)?;
    Ok(table)
}

fn load_imports(
    stmts: &[Stmt],
    loader: &mut LibraryLoader,
    table: &mut LibraryTable,
    loading: &mut HashSet<String>,
) -> Result<(), String> {
    for stmt in stmts {
        match stmt {
            Stmt::UseLibrary { libs, line } => {
                for (name, _) in libs {
                    if !is_builtin_library(name) {
                        load_library(name, *line, loader, table, loading)?;
                    }
                }
            }
            // `<lib> အဖြစ် <alias>။` imports the library too, so a library
            // that only appears this way still has to be compiled in.
            Stmt::FuncAlias { original, line, .. } => {
                if !is_builtin_library(original)
                    && !table.units.contains_key(original)
                    && loader.find_source(original).is_some()
                {
                    load_library(original, *line, loader, table, loading)?;
                }
            }
            Stmt::If { branches } => {
                for branch in branches {
                    load_imports(&branch.body, loader, table, loading)?;
                }
            }
            Stmt::While { body, .. } | Stmt::ForLoop { body, .. } => {
                load_imports(body, loader, table, loading)?;
            }
            Stmt::Switch { arms, .. } => {
                for arm in arms {
                    load_imports(&arm.body, loader, table, loading)?;
                }
            }
            // An import inside `try` still has to be compiled in, so the
            // handler above it catches a library that fails to load.
            Stmt::TryCatch {
                try_body,
                catch_body,
                finally_body,
                ..
            } => {
                load_imports(try_body, loader, table, loading)?;
                if let Some(body) = catch_body {
                    load_imports(body, loader, table, loading)?;
                }
                if let Some(body) = finally_body {
                    load_imports(body, loader, table, loading)?;
                }
            }
            _ => {}
        }
    }
    Ok(())
}

fn load_library(
    name: &str,
    line: usize,
    loader: &mut LibraryLoader,
    table: &mut LibraryTable,
    loading: &mut HashSet<String>,
) -> Result<(), String> {
    if table.units.contains_key(name) || !loading.insert(name.to_string()) {
        // Already compiled, or an import cycle: the second import of a
        // library that is still loading runs nothing (the interpreter's
        // cycle guard turns that into E119 at run time).
        return Ok(());
    }
    let Some((_path, src)) = loader.find_source(name) else {
        loading.remove(name);
        table.unresolved.insert(name.to_string());
        return Ok(());
    };
    let tokens = crate::frontend::lexer::lex(&src).map_err(|e| {
        format!(
            "E066 လိုင်း {} တွင် \"{}\" နည်းပညာကို ဖတ်ရာတွင် အမှား ဖြစ်ပေါ်ခဲ့ပါသည် - {}",
            line, name, e
        )
    })?;
    let stmts = crate::frontend::parser::parse(&tokens).map_err(|e| {
        format!(
            "E067 လိုင်း {} တွင် \"{}\" နည်းပညာကို ခွဲခြမ်းစိတ်ဖြာ လုပ်ရာတွင် အမှား ဖြစ်ပေါ်ခဲ့ပါသည် - {}",
            line, name, e
        )
    })?;
    load_imports(&stmts, loader, table, loading)?;
    loading.remove(name);
    let functions = function_names(&stmts);
    table.order.push(name.to_string());
    table.units.insert(
        name.to_string(),
        ScriptLibrary {
            source: Rc::from(src),
            stmts,
            functions,
        },
    );
    Ok(())
}

/// The function names `register_functions` will register for `stmts`.
fn function_names(stmts: &[Stmt]) -> HashSet<String> {
    let mut names = HashSet::new();
    fn walk(stmts: &[Stmt], names: &mut HashSet<String>) {
        for stmt in stmts {
            match stmt {
                Stmt::FuncDef { name, .. } => {
                    names.insert(name.clone());
                }
                Stmt::If { branches } => {
                    for branch in branches {
                        walk(&branch.body, names);
                    }
                }
                Stmt::While { body, .. } | Stmt::ForLoop { body, .. } => walk(body, names),
                Stmt::Switch { arms, .. } => {
                    for arm in arms {
                        walk(&arm.body, names);
                    }
                }
                _ => {}
            }
        }
    }
    walk(stmts, &mut names);
    names
}

/// A function body's value is its last expression, the same rule the
/// interpreter uses: `... ပြီး။` returns the final bare expression, and a
/// body that ends in a declaration returns the value it just declared.
/// Anything else returns *no* value, which `ReturnVoid` records so a caller
/// that asked for one can raise `E071`.
pub(crate) fn compile_body(compiler: &mut Compiler, body: &[Stmt]) -> Result<(), String> {
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

pub(crate) fn register_functions(stmts: &[Stmt], compiler: &mut Compiler) {
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
                        source: Rc::clone(&compiler.source),
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
pub(crate) fn collect_defs(stmts: &[Stmt]) -> Vec<&Stmt> {
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

/// Compile a source text all the way to instructions.
#[cfg(test)]
pub fn compile_source(src: &str) -> Result<Program, String> {
    let tokens = crate::frontend::lexer::lex(src)?;
    let stmts = crate::frontend::parser::parse(&tokens)?;
    compile_with_source(&stmts, src)
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
