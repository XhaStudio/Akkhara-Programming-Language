//! Lowering: syntax tree -> HIR, and HIR -> MIR.
//!
//! `hir` never fails. It resolves names as it walks, so a name no declaration
//! covers stays dynamic (`HirExpr::Name`) and a construct the IR does not model
//! becomes `HirStmt::Opaque` instead of disappearing.
//!
//! `mir` turns the nested tree into basic blocks and refuses -- with `E176`,
//! naming the construct and the line -- anything it cannot represent yet, in
//! the same spirit as the bytecode compiler's `E170`. Refusing is deliberate:
//! an IR that silently approximated a construct would be a different language.

use super::builder::Builder;
use super::hir::*;
use super::mir::*;
use crate::frontend::ast::{CondAtom, CondChain, Expr, ForSource, LogicalOp, Stmt};
use crate::scope::resolver::Resolver;
use crate::scope::symbol::{SymbolKind, VarId};
use crate::types::Type;

/// Lower the parsed program to HIR.
pub(crate) fn hir(stmts: &[Stmt]) -> HirProgram {
    let mut lower = Lower::new();
    let body = lower.stmts(stmts);
    let main = HirFn {
        name: "<main>".to_string(),
        params: Vec::new(),
        body,
        line: 0,
        captures: Vec::new(),
    };
    let (symbols, unresolved) = lower.resolver.into_parts();
    HirProgram {
        main,
        functions: lower.functions,
        symbols,
        unresolved,
    }
}

struct Lower {
    resolver: Resolver,
    functions: Vec<HirFn>,
}

impl Lower {
    fn new() -> Lower {
        Lower {
            resolver: Resolver::new(),
            functions: Vec::new(),
        }
    }

    fn stmts(&mut self, list: &[Stmt]) -> Vec<HirStmt> {
        let mut out = Vec::with_capacity(list.len());
        for stmt in list {
            self.stmt(stmt, &mut out);
        }
        out
    }

    /// Lower one statement into zero or more HIR nodes. Only a function
    /// definition produces none: the body goes to `functions`, because a
    /// definition has no run-time effect where it is written.
    fn stmt(&mut self, stmt: &Stmt, out: &mut Vec<HirStmt>) {
        match stmt {
            Stmt::VarDecl { name, value, line } => {
                let var = self.declare(name, SymbolKind::Var, *line, None);
                out.push(HirStmt::Store {
                    var,
                    ty: None,
                    op: None,
                    value: self.expr(value),
                    line: *line,
                });
            }
            Stmt::EngDecl {
                name,
                type_name,
                value,
                line,
                ..
            } => {
                // `pin` constness is a run-time rule today, so the IR records
                // the declared type and nothing about constness.
                let ty = Type::named(type_name);
                let var = self.declare(name, SymbolKind::Var, *line, ty);
                out.push(HirStmt::Store {
                    var,
                    ty,
                    op: None,
                    value: self.expr(value),
                    line: *line,
                });
            }
            Stmt::Print { value, line } | Stmt::EngPrint { value, line } => {
                out.push(HirStmt::Print {
                    value: self.expr(value),
                    line: *line,
                });
            }
            Stmt::InputNoAssign { value, line }
            | Stmt::EngInput {
                prompt: value,
                line,
            } => {
                out.push(HirStmt::Eval {
                    value: call("input", vec![self.expr(value)]),
                    line: *line,
                });
            }
            Stmt::InputAssign { name, value, line } => {
                let var = self.declare(name, SymbolKind::Var, *line, None);
                out.push(HirStmt::Store {
                    var,
                    ty: None,
                    op: None,
                    value: call("input", vec![self.expr(value)]),
                    line: *line,
                });
            }
            Stmt::EngInputAssign {
                name,
                type_name,
                prompt,
                line,
            } => {
                let ty = Type::named(type_name);
                let var = self.declare(name, SymbolKind::Var, *line, ty);
                out.push(HirStmt::Store {
                    var,
                    ty,
                    op: None,
                    value: call("input", vec![self.expr(prompt)]),
                    line: *line,
                });
            }
            Stmt::RandomDecl {
                name,
                min,
                max,
                line,
            } => {
                let var = self.declare(name, SymbolKind::Var, *line, None);
                out.push(HirStmt::Store {
                    var,
                    ty: None,
                    op: None,
                    value: call("random", vec![self.expr(min), self.expr(max)]),
                    line: *line,
                });
            }
            Stmt::ExprStmt { value, line } => out.push(HirStmt::Eval {
                value: self.expr(value),
                line: *line,
            }),
            Stmt::MathAssign {
                target,
                op,
                amount,
                line,
            } => match target {
                Expr::Ident(name) => {
                    let var = self.resolver.declare_or_resolve(name, *line);
                    out.push(HirStmt::Store {
                        var,
                        ty: None,
                        op: Some(*op),
                        value: self.expr(amount),
                        line: *line,
                    });
                }
                _ => out.push(opaque("a compound assignment into an element", *line)),
            },
            Stmt::IndexAssign {
                target,
                value,
                line,
            } => match target {
                Expr::Index(base, indices) => {
                    let target = self.expr(base);
                    let indices = self.exprs(indices);
                    out.push(HirStmt::SetIndex {
                        target,
                        indices,
                        value: self.expr(value),
                        line: *line,
                    });
                }
                _ => out.push(opaque("an assignment that is not into an element", *line)),
            },
            Stmt::If { branches } => {
                let mut lowered = Vec::with_capacity(branches.len());
                for branch in branches {
                    let cond = branch.cond.as_ref().map(|chain| self.cond(chain));
                    lowered.push(HirBranch {
                        cond,
                        negate: branch.negate,
                        body: self.stmts(&branch.body),
                        line: 0,
                    });
                }
                out.push(HirStmt::If { branches: lowered });
            }
            Stmt::While {
                cond,
                negate,
                body,
                line,
            } => out.push(HirStmt::While {
                cond: self.cond(cond),
                negate: *negate,
                body: self.stmts(body),
                step: None,
                line: *line,
            }),
            Stmt::EngLoop { body } => out.push(HirStmt::Loop {
                body: self.stmts(body),
                line: 0,
            }),
            Stmt::ForLoop {
                var_name,
                source,
                body,
                line,
            } => {
                let var = self.declare(var_name, SymbolKind::LoopVar, *line, None);
                match source {
                    ForSource::Range {
                        start,
                        end,
                        step,
                        op,
                    } => {
                        let start = match start {
                            Some(value) => self.expr(value),
                            None => HirExpr::Int(0),
                        };
                        let end = self.expr(end);
                        let step = self.expr(step);
                        out.push(HirStmt::ForRange {
                            var,
                            start,
                            end,
                            step,
                            op: *op,
                            body: self.stmts(body),
                            line: *line,
                        });
                    }
                    ForSource::RangeSigned { start, end, step } => {
                        // The step's sign picks the direction, so the IR needs
                        // to know it before it can choose the comparison; a
                        // computed step keeps that at run time and is refused.
                        let start = self.expr(start);
                        let end = self.expr(end);
                        let step = self.expr(step);
                        let op = match &step {
                            HirExpr::Int(value) if *value < 0 => '-',
                            HirExpr::Int(_) => '+',
                            _ => {
                                out.push(opaque(
                                    "a signed range whose step is not a literal",
                                    *line,
                                ));
                                let _ = var;
                                return;
                            }
                        };
                        out.push(HirStmt::ForRange {
                            var,
                            start,
                            end,
                            step,
                            op,
                            body: self.stmts(body),
                            line: *line,
                        });
                    }
                    ForSource::Auto(iter) => out.push(HirStmt::ForEach {
                        var,
                        iter: self.expr(iter),
                        body: self.stmts(body),
                        line: *line,
                    }),
                }
            }
            Stmt::FuncDef {
                name,
                params,
                defaults,
                body,
                line,
            } => {
                self.function(name, params, defaults, body, *line);
            }
            Stmt::FuncCall { name, args, line } | Stmt::EngExprCall { name, args, line } => {
                out.push(HirStmt::Eval {
                    value: call(name, self.exprs(args)),
                    line: *line,
                });
            }
            Stmt::FuncCallAssign {
                name,
                fn_name,
                args,
                line,
            } => {
                let var = self.declare(name, SymbolKind::Var, *line, None);
                out.push(HirStmt::Store {
                    var,
                    ty: None,
                    op: None,
                    value: call(fn_name, self.exprs(args)),
                    line: *line,
                });
            }
            Stmt::LibCall {
                lib,
                fn_name,
                args,
                line,
            } => out.push(HirStmt::Eval {
                value: HirExpr::LibCall {
                    lib: lib.clone(),
                    func: fn_name.clone(),
                    args: self.exprs(args),
                },
                line: *line,
            }),
            Stmt::LibCallAssign {
                name,
                lib,
                fn_name,
                args,
                line,
            } => {
                let var = self.declare(name, SymbolKind::Var, *line, None);
                let value = HirExpr::LibCall {
                    lib: lib.clone(),
                    func: fn_name.clone(),
                    args: self.exprs(args),
                };
                out.push(HirStmt::Store {
                    var,
                    ty: None,
                    op: None,
                    value,
                    line: *line,
                });
            }
            Stmt::FuncAlias {
                original,
                aliases,
                line,
            } => out.push(opaque(
                &format!("the aliases {} of \"{}\"", aliases.join(", "), original),
                *line,
            )),
            Stmt::ClassDef { name, line, .. } => {
                out.push(opaque(&format!("class \"{}\"", name), *line))
            }
            Stmt::SelfFieldSet { field, value, line } => out.push(HirStmt::SetField {
                object: HirExpr::Name("self".to_string()),
                field: field.clone(),
                value: self.expr(value),
                line: *line,
            }),
            Stmt::Raise {
                identity,
                value,
                line,
            } => out.push(HirStmt::Raise {
                identity: identity.clone(),
                value: self.expr(value),
                line: *line,
            }),
            Stmt::TryCatch {
                try_body,
                catch_err,
                catch_var,
                catch_body,
                finally_body,
            } => {
                // The try body is lowered first: the catch clause's binding is
                // not in scope inside it, exactly as the interpreter has it.
                let try_body = self.stmts(try_body);
                let catch_var = match (catch_var, catch_body.is_some()) {
                    (Some(name), true) => {
                        Some(self.declare(name, SymbolKind::Var, 0, Some(Type::Str)))
                    }
                    _ => None,
                };
                let catch_body = catch_body.as_ref().map(|body| self.stmts(body));
                let finally_body = finally_body.as_ref().map(|body| self.stmts(body));
                out.push(HirStmt::TryCatch {
                    try_body,
                    catch_err: catch_err.clone(),
                    catch_var,
                    catch_body,
                    finally_body,
                    line: 0,
                });
            }
            Stmt::UseLibrary { libs, line } => out.push(HirStmt::Import {
                libs: libs.iter().map(|(name, _)| name.clone()).collect(),
                line: *line,
            }),
            Stmt::Wait { .. } => out.push(HirStmt::Opaque {
                what: "wait".to_string(),
                line: 0,
            }),
            Stmt::HttpSend { line, .. } => out.push(opaque("an HTTP send", *line)),
            Stmt::EngReturn { value, line } => out.push(HirStmt::Return {
                value: value.as_ref().map(|value| self.expr(value)),
                line: *line,
            }),
            Stmt::EngAssign { name, value, line } => {
                let var = self.resolver.declare_or_resolve(name, *line);
                out.push(HirStmt::Store {
                    var,
                    ty: None,
                    op: None,
                    value: self.expr(value),
                    line: *line,
                });
            }
            Stmt::EngMathAssign {
                name,
                op,
                amount,
                line,
            } => {
                let var = self.resolver.declare_or_resolve(name, *line);
                out.push(HirStmt::Store {
                    var,
                    ty: None,
                    op: Some(*op),
                    value: self.expr(amount),
                    line: *line,
                });
            }
            Stmt::EngBreak { value, line } => out.push(HirStmt::Break {
                value: value.as_ref().map(|value| self.expr(value)),
                line: *line,
            }),
            Stmt::Continue { line } => out.push(HirStmt::Continue { line: *line }),
            Stmt::Destructure { names, line, .. } => out.push(opaque(
                &format!("destructuring into {}", names.join(", ")),
                *line,
            )),
            Stmt::Switch { line, .. } => out.push(opaque("switch", *line)),
            Stmt::EngFor {
                init,
                cond,
                step,
                body,
                line,
            } => {
                self.stmt(init, out);
                let step = self.single(step);
                match step {
                    Some(step) => out.push(HirStmt::While {
                        cond: self.cond(cond),
                        negate: false,
                        body: self.stmts(body),
                        step: Some(step),
                        line: *line,
                    }),
                    None => out.push(opaque("a for loop whose step is not one statement", *line)),
                }
            }
            Stmt::EngLoopAssign { line, .. } => out.push(opaque("a loop used as a value", *line)),
        }
    }

    /// Lower one statement into a single node, for the places that need one
    /// (the step of a C-style `for`).
    fn single(&mut self, stmt: &Stmt) -> Option<Box<HirStmt>> {
        let mut nodes = Vec::new();
        self.stmt(stmt, &mut nodes);
        if nodes.len() == 1 {
            nodes.pop().map(Box::new)
        } else {
            None
        }
    }

    fn declare(&mut self, name: &str, kind: SymbolKind, line: usize, ty: Option<Type>) -> VarId {
        self.resolver.declare(name, kind, line, ty)
    }

    fn expr(&mut self, expr: &Expr) -> HirExpr {
        match expr {
            Expr::NumLit(text) => HirExpr::BadNum(text.clone()),
            Expr::Int(value) => HirExpr::Int(*value),
            Expr::Float(value) => HirExpr::Float(*value),
            // The interpreter expands `{Name}` placeholders when it evaluates a
            // string literal. The native backend does not model that yet, so a
            // string with a placeholder (or an escaped brace, which the
            // interpreter unescapes) is refused by name rather than printed
            // with its braces intact.
            Expr::StrLit(text) if text.contains('{') || text.contains("\\}") => {
                HirExpr::Opaque("a string with {…} interpolation".to_string())
            }
            Expr::StrLit(text) => HirExpr::Str(text.clone()),
            Expr::BoolLit(value) => HirExpr::Bool(*value),
            Expr::NullLit => HirExpr::Null,
            Expr::Ident(name) => match self.resolver.resolve(name) {
                Some(var) => HirExpr::Var(var),
                None => {
                    // `Ident` carries no line, so the note has none either.
                    self.resolver.note_unresolved(name, 0);
                    HirExpr::Name(name.to_string())
                }
            },
            Expr::Binary(left, op, right, _) => {
                HirExpr::Binary(*op, Box::new(self.expr(left)), Box::new(self.expr(right)))
            }
            Expr::Compare(left, op, right, _) => HirExpr::Compare(
                op.clone(),
                Box::new(self.expr(left)),
                Box::new(self.expr(right)),
            ),
            Expr::Contains(haystack, needle, _) => {
                HirExpr::Contains(Box::new(self.expr(haystack)), Box::new(self.expr(needle)))
            }
            Expr::Logical(left, op, right, _) => HirExpr::Logical(
                op.clone(),
                Box::new(self.expr(left)),
                Box::new(self.expr(right)),
            ),
            Expr::Ternary(cond, then_value, else_value, _) => HirExpr::Ternary(
                Box::new(self.expr(cond)),
                Box::new(self.expr(then_value)),
                Box::new(self.expr(else_value)),
            ),
            Expr::Not(value, _) => HirExpr::Not(Box::new(self.expr(value))),
            Expr::Neg(value, _) => HirExpr::Neg(Box::new(self.expr(value))),
            Expr::ListLit(items) => HirExpr::List(self.exprs(items)),
            Expr::TupleLit(items) => HirExpr::Tuple(self.exprs(items)),
            Expr::SetLit(items) => HirExpr::Set(self.exprs(items)),
            Expr::DictLit(pairs) => HirExpr::Dict(
                pairs
                    .iter()
                    .map(|(key, value)| (self.expr(key), self.expr(value)))
                    .collect(),
            ),
            Expr::Index(base, indices) => {
                HirExpr::Index(Box::new(self.expr(base)), self.exprs(indices))
            }
            Expr::NewObj(name, args, _) => call(name, self.exprs(args)),
            Expr::Lambda {
                params,
                defaults,
                body,
                line,
            } => HirExpr::Lambda(self.function("<lambda>", params, defaults, body, *line)),
            Expr::LibCall(lib, func, args, _) => HirExpr::LibCall {
                lib: lib.clone(),
                func: func.clone(),
                args: self.exprs(args),
            },
            Expr::LibFnRef(lib, func, _) => HirExpr::LibFnRef {
                lib: lib.clone(),
                func: func.clone(),
            },
            Expr::FieldAccess(object, field, _) => {
                HirExpr::Field(Box::new(self.expr(object)), field.clone())
            }
            Expr::HttpGet(lib, url, _) => HirExpr::HttpGet {
                lib: lib.clone(),
                url: Box::new(self.expr(url)),
            },
        }
    }

    fn exprs(&mut self, list: &[Expr]) -> Vec<HirExpr> {
        let mut out = Vec::with_capacity(list.len());
        for expr in list {
            out.push(self.expr(expr));
        }
        out
    }

    /// Lower a condition chain, `a && !b || c`, into one expression: each atom
    /// is a comparison (or a bare truthiness test), and the chain's operators
    /// join them.
    fn cond(&mut self, chain: &CondChain) -> HirExpr {
        let mut value = self.atom(&chain.first);
        for (op, atom) in &chain.rest {
            let right = self.atom(atom);
            value = HirExpr::Logical(op.clone(), Box::new(value), Box::new(right));
        }
        value
    }

    fn atom(&mut self, atom: &CondAtom) -> HirExpr {
        let mut value = match (&atom.type_check, &atom.op) {
            (Some(type_name), _) => match Type::named(type_name) {
                Some(ty) => HirExpr::IsType {
                    value: Box::new(self.expr(&atom.lhs)),
                    ty,
                },
                None => HirExpr::Opaque(format!("a check against the type \"{}\"", type_name)),
            },
            (None, Some(op)) => match &atom.rhs {
                Some(rhs) => HirExpr::Compare(
                    op.clone(),
                    Box::new(self.expr(&atom.lhs)),
                    Box::new(self.expr(rhs)),
                ),
                None => HirExpr::Opaque("a comparison without a right side".to_string()),
            },
            (None, None) => self.expr(&atom.lhs),
        };
        if atom.negate {
            value = HirExpr::Not(Box::new(value));
        }
        value
    }

    /// Lower a function or lambda body under its own frame, and return its
    /// index in `functions`.
    fn function(
        &mut self,
        name: &str,
        params: &[String],
        defaults: &[Option<Expr>],
        body: &[Stmt],
        line: usize,
    ) -> usize {
        // Defaults are expressions of the enclosing scope: they are written in
        // the caller's world, not the callee's.
        let defaults: Vec<Option<HirExpr>> = defaults
            .iter()
            .map(|default| default.as_ref().map(|value| self.expr(value)))
            .collect();
        self.resolver.enter(params, line);
        let body = self.stmts(body);
        let frame = self.resolver.leave();
        let params = frame
            .params
            .iter()
            .zip(params.iter())
            .zip(defaults)
            .map(|((var, name), default)| HirParam {
                var: *var,
                name: name.clone(),
                default,
            })
            .collect();
        self.functions.push(HirFn {
            name: name.to_string(),
            params,
            body,
            line,
            captures: frame.captures,
        });
        self.functions.len() - 1
    }
}

fn call(name: &str, args: Vec<HirExpr>) -> HirExpr {
    HirExpr::Call(name.to_string(), args)
}

fn opaque(what: &str, line: usize) -> HirStmt {
    HirStmt::Opaque {
        what: what.to_string(),
        line,
    }
}

// --- HIR -> MIR ------------------------------------------------------------

/// Lower every HIR function to basic blocks, top-level statements first.
pub(crate) fn mir(program: &HirProgram) -> Result<MirProgram, String> {
    let mut functions = Vec::with_capacity(program.functions.len() + 1);
    functions.push(lower_fn(&program.main, true)?);
    for function in &program.functions {
        functions.push(lower_fn(function, false)?);
    }
    Ok(MirProgram { functions })
}

fn lower_fn(function: &HirFn, is_main: bool) -> Result<MirFn, String> {
    let mut builder = Builder::new();
    let mut ctx = Ctx {
        loops: Vec::new(),
        handlers: Vec::new(),
        escape: None,
        is_main,
        finally_depth: 0,
    };
    stmts(&mut builder, &mut ctx, &function.body)?;
    // Close the body explicitly. Leaving it open would let `finish` give it a
    // fall-through to the next block, and a statement after an `if` is not
    // the last block that was created -- it would fall into the `if` body.
    if !builder.is_terminated() {
        builder.terminate(if is_main {
            Terminator::Exit
        } else {
            Terminator::Return(None)
        });
    }
    Ok(builder.finish(
        function.name.clone(),
        function.params.iter().map(|param| param.var).collect(),
        function
            .captures
            .iter()
            .map(|capture| capture.var)
            .collect(),
    ))
}

fn unsupported(line: usize, what: &str) -> String {
    format!("E176 လိုင်း {} တွင် {} ကို MIR ဖြင့် မထောက်ပံ့သေးပါ။", line, what)
}

/// What a `raise` has to unwind through while one function is being lowered.
struct Ctx {
    /// The blocks a `break` and a `continue` jump to, innermost loop last,
    /// with the `finally` depth the loop was entered at (a `break` that would
    /// leave a `finally` is refused instead of skipping it).
    loops: Vec<(BlockId, BlockId, usize)>,
    /// The dispatch block of every enclosing try, innermost last. A raise
    /// jumps to the innermost one; a call is followed by a check that does.
    handlers: Vec<BlockId>,
    /// The block a raise goes to when no try in this function catches it,
    /// created on first use.
    escape: Option<BlockId>,
    /// `<main>` reports an uncaught raise; a function returns with it pending
    /// so its caller's own check deals with it.
    is_main: bool,
    /// How many enclosing `finally` blocks have to run before control leaves.
    finally_depth: usize,
}

/// Where a raise (or a call that may raise) goes when no try in this function
/// catches it.
fn escape_block(builder: &mut Builder, ctx: &mut Ctx) -> BlockId {
    if let Some(block) = ctx.escape {
        return block;
    }
    let block = builder.block();
    ctx.escape = Some(block);
    let current = builder.current();
    builder.switch(block);
    builder.terminate(if ctx.is_main {
        Terminator::ReportError
    } else {
        Terminator::Propagate
    });
    builder.switch(current);
    block
}

/// The innermost enclosing handler, or this function's escape block.
fn raise_target(builder: &mut Builder, ctx: &mut Ctx) -> BlockId {
    match ctx.handlers.last() {
        Some(handler) => *handler,
        None => escape_block(builder, ctx),
    }
}

fn stmts(builder: &mut Builder, ctx: &mut Ctx, list: &[HirStmt]) -> Result<(), String> {
    for stmt in list {
        statement(builder, ctx, stmt)?;
    }
    Ok(())
}

fn statement(builder: &mut Builder, ctx: &mut Ctx, stmt: &HirStmt) -> Result<(), String> {
    match stmt {
        HirStmt::Store {
            var,
            op,
            value,
            line,
            ..
        } => {
            let value = operand(builder, ctx, value, *line)?;
            builder.emit(MirInst::StoreVar {
                var: *var,
                op: *op,
                value,
            });
        }
        HirStmt::SetIndex {
            target,
            indices,
            value,
            line,
        } => {
            let base = operand(builder, ctx, target, *line)?;
            let indices = operands(builder, ctx, indices, *line)?;
            let value = operand(builder, ctx, value, *line)?;
            builder.emit(MirInst::StoreIndex {
                base,
                indices,
                value,
                line: *line,
            });
        }
        HirStmt::SetField {
            object,
            field,
            value,
            line,
        } => {
            let base = operand(builder, ctx, object, *line)?;
            let value = operand(builder, ctx, value, *line)?;
            builder.emit(MirInst::StoreField {
                base,
                field: field.clone(),
                value,
            });
        }
        HirStmt::Print { value, line } => {
            let value = operand(builder, ctx, value, *line)?;
            builder.emit(MirInst::Print(value));
        }
        HirStmt::Eval { value, line } => {
            let value = operand(builder, ctx, value, *line)?;
            builder.emit(MirInst::Eval(value));
        }
        HirStmt::If { branches } => {
            let end = builder.block();
            for branch in branches {
                match &branch.cond {
                    Some(cond) => {
                        let body = builder.block();
                        let next = builder.block();
                        let cond = operand(builder, ctx, cond, branch.line)?;
                        let (then_block, else_block) = if branch.negate {
                            (next, body)
                        } else {
                            (body, next)
                        };
                        builder.terminate(Terminator::Branch {
                            cond,
                            then_block,
                            else_block,
                        });
                        builder.switch(body);
                        stmts(builder, ctx, &branch.body)?;
                        builder.terminate(Terminator::Goto(end));
                        builder.switch(next);
                    }
                    None => {
                        stmts(builder, ctx, &branch.body)?;
                        builder.terminate(Terminator::Goto(end));
                    }
                }
            }
            if !builder.is_terminated() {
                builder.terminate(Terminator::Goto(end));
            }
            builder.switch(end);
        }
        HirStmt::While {
            cond,
            negate,
            body,
            step,
            line,
        } => {
            let head = builder.block();
            let body_block = builder.block();
            let step_block = if step.is_some() {
                builder.block()
            } else {
                head
            };
            let end = builder.block();
            builder.terminate(Terminator::Goto(head));
            builder.switch(head);
            let cond = operand(builder, ctx, cond, *line)?;
            let (then_block, else_block) = if *negate {
                (end, body_block)
            } else {
                (body_block, end)
            };
            builder.terminate(Terminator::Branch {
                cond,
                then_block,
                else_block,
            });
            builder.switch(body_block);
            ctx.loops.push((end, step_block, ctx.finally_depth));
            stmts(builder, ctx, body)?;
            ctx.loops.pop();
            builder.terminate(Terminator::Goto(step_block));
            if let Some(step) = step {
                builder.switch(step_block);
                statement(builder, ctx, step)?;
                builder.terminate(Terminator::Goto(head));
            }
            builder.switch(end);
        }
        HirStmt::Loop { body, .. } => {
            let body_block = builder.block();
            let end = builder.block();
            builder.terminate(Terminator::Goto(body_block));
            builder.switch(body_block);
            ctx.loops.push((end, body_block, ctx.finally_depth));
            stmts(builder, ctx, body)?;
            ctx.loops.pop();
            builder.terminate(Terminator::Goto(body_block));
            builder.switch(end);
        }
        HirStmt::ForRange {
            var,
            start,
            end,
            step,
            op,
            body,
            line,
        } => {
            let start = operand(builder, ctx, start, *line)?;
            builder.emit(MirInst::StoreVar {
                var: *var,
                op: None,
                value: start,
            });
            // `interp` evaluates the bound and the step once, before the loop.
            let end = operand(builder, ctx, end, *line)?;
            let step = operand(builder, ctx, step, *line)?;
            let head = builder.block();
            let body_block = builder.block();
            let next = builder.block();
            let exit = builder.block();
            builder.terminate(Terminator::Goto(head));
            builder.switch(head);
            let dest = builder.temp();
            let comparison = if matches!(op, '+' | '*') { "<" } else { ">" };
            builder.emit(MirInst::Compare {
                dest,
                op: comparison.to_string(),
                lhs: Operand::Var(*var),
                rhs: end,
                line: *line,
            });
            builder.terminate(Terminator::Branch {
                cond: Operand::Temp(dest),
                then_block: body_block,
                else_block: exit,
            });
            builder.switch(body_block);
            ctx.loops.push((exit, next, ctx.finally_depth));
            stmts(builder, ctx, body)?;
            ctx.loops.pop();
            builder.terminate(Terminator::Goto(next));
            builder.switch(next);
            builder.emit(MirInst::StoreVar {
                var: *var,
                op: Some(*op),
                value: step,
            });
            builder.terminate(Terminator::Goto(head));
            builder.switch(exit);
        }
        HirStmt::ForEach {
            var,
            iter,
            body,
            line,
        } => {
            let source = operand(builder, ctx, iter, *line)?;
            let items = builder.temp();
            builder.emit(MirInst::Items {
                dest: items,
                value: source,
            });
            let index = builder.temp();
            builder.emit(MirInst::Move {
                dest: index,
                value: Operand::Const(Const::Int(0)),
            });
            let head = builder.block();
            let body_block = builder.block();
            let next = builder.block();
            let exit = builder.block();
            builder.terminate(Terminator::Goto(head));
            builder.switch(head);
            let length = builder.temp();
            builder.emit(MirInst::Len {
                dest: length,
                value: Operand::Temp(items),
            });
            let dest = builder.temp();
            builder.emit(MirInst::Compare {
                dest,
                op: "<".to_string(),
                lhs: Operand::Temp(index),
                rhs: Operand::Temp(length),
                line: *line,
            });
            builder.terminate(Terminator::Branch {
                cond: Operand::Temp(dest),
                then_block: body_block,
                else_block: exit,
            });
            builder.switch(body_block);
            let item = builder.temp();
            builder.emit(MirInst::Index {
                dest: item,
                base: Operand::Temp(items),
                indices: vec![Operand::Temp(index)],
                line: *line,
            });
            builder.emit(MirInst::StoreVar {
                var: *var,
                op: None,
                value: Operand::Temp(item),
            });
            ctx.loops.push((exit, next, ctx.finally_depth));
            stmts(builder, ctx, body)?;
            ctx.loops.pop();
            builder.terminate(Terminator::Goto(next));
            builder.switch(next);
            builder.emit(MirInst::Binary {
                dest: index,
                op: '+',
                lhs: Operand::Temp(index),
                rhs: Operand::Const(Const::Int(1)),
            });
            builder.terminate(Terminator::Goto(head));
            builder.switch(exit);
        }
        HirStmt::Return { value, line } => {
            if ctx.finally_depth > 0 {
                return Err(unsupported(*line, "a return that would skip a finally"));
            }
            let value = match value {
                Some(value) => Some(operand(builder, ctx, value, *line)?),
                None => None,
            };
            builder.terminate(Terminator::Return(value));
        }
        HirStmt::Break { value, line } => {
            if value.is_some() {
                return Err(unsupported(*line, "a break that carries a value"));
            }
            match ctx.loops.last() {
                Some((exit, _, depth)) if *depth >= ctx.finally_depth => {
                    builder.terminate(Terminator::Goto(*exit))
                }
                Some(_) => return Err(unsupported(*line, "a break that would skip a finally")),
                None => return Err(unsupported(*line, "a break outside a loop")),
            }
        }
        HirStmt::Continue { line } => match ctx.loops.last() {
            Some((_, next, depth)) if *depth >= ctx.finally_depth => {
                builder.terminate(Terminator::Goto(*next))
            }
            Some(_) => return Err(unsupported(*line, "a continue that would skip a finally")),
            None => return Err(unsupported(*line, "a continue outside a loop")),
        },
        HirStmt::Raise {
            identity,
            value,
            line,
        } => {
            if ctx.finally_depth > 0 {
                // The finally body still runs: the handler the raise jumps to
                // is the dispatch block, which runs it on every path.
            }
            let value = operand(builder, ctx, value, *line)?;
            builder.emit(MirInst::SetError {
                identity: identity.clone(),
                prefix: crate::shared::errors::raised_prefix(*line, identity),
                value,
            });
            let target = raise_target(builder, ctx);
            builder.terminate(Terminator::Goto(target));
        }
        HirStmt::TryCatch {
            try_body,
            catch_err,
            catch_var,
            catch_body,
            finally_body,
            line,
        } => {
            let dispatch = builder.block();
            let catch_block = builder.block();
            let finally_caught = builder.block();
            let no_match = builder.block();
            let finally_ok = builder.block();
            let after = builder.block();
            let caught = catch_err.is_some() && catch_body.is_some();

            ctx.handlers.push(dispatch);
            if finally_body.is_some() {
                ctx.finally_depth += 1;
            }
            stmts(builder, ctx, try_body)?;
            if finally_body.is_some() {
                ctx.finally_depth -= 1;
            }
            ctx.handlers.pop();
            builder.terminate(Terminator::Goto(finally_ok));

            // The body finished without a raise: `finally`, then on.
            builder.switch(finally_ok);
            if let Some(finally) = finally_body {
                stmts(builder, ctx, finally)?;
            }
            builder.terminate(Terminator::Goto(after));

            // A raise arrived: run the clause's filter.
            builder.switch(dispatch);
            if caught {
                let name = catch_err.as_deref().unwrap_or("E");
                if name == "E" {
                    builder.terminate(Terminator::Goto(catch_block));
                } else {
                    let dest = builder.temp();
                    builder.emit(MirInst::ErrorMatches {
                        dest,
                        identity: name.to_string(),
                    });
                    builder.terminate(Terminator::Branch {
                        cond: Operand::Temp(dest),
                        then_block: catch_block,
                        else_block: no_match,
                    });
                }
            } else {
                builder.terminate(Terminator::Goto(no_match));
            }

            builder.switch(catch_block);
            if let Some(var) = catch_var {
                builder.emit(MirInst::BindError { var: *var });
            }
            builder.emit(MirInst::ClearError);
            if let Some(body) = catch_body {
                stmts(builder, ctx, body)?;
            }
            builder.terminate(Terminator::Goto(finally_caught));

            builder.switch(finally_caught);
            if let Some(finally) = finally_body {
                stmts(builder, ctx, finally)?;
            }
            builder.terminate(Terminator::Goto(after));

            // Not this clause's raise: `finally` runs, then it keeps going to
            // the enclosing handler (or out of the function).
            builder.switch(no_match);
            if let Some(finally) = finally_body {
                stmts(builder, ctx, finally)?;
            }
            let target = raise_target(builder, ctx);
            builder.terminate(Terminator::Goto(target));

            builder.switch(after);
            let _ = line;
        }
        // A library import is the interpreter's job today: its top-level
        // statements run when the import executes, and the native backend has
        // no library loader. Refusing here keeps a build from silently
        // dropping those side effects.
        HirStmt::Import { line, .. } => return Err(unsupported(*line, "a library import")),
        HirStmt::Opaque { what, line } => return Err(unsupported(*line, what)),
    }
    Ok(())
}

fn operands(
    builder: &mut Builder,
    ctx: &mut Ctx,
    list: &[HirExpr],
    line: usize,
) -> Result<Vec<Operand>, String> {
    let mut out = Vec::with_capacity(list.len());
    for expr in list {
        out.push(operand(builder, ctx, expr, line)?);
    }
    Ok(out)
}

/// Flatten one HIR expression into instructions, returning the operand that
/// holds its value. A `?:` or a short-circuit side opens blocks, so after this
/// returns the builder's current block is the join block.
fn operand(
    builder: &mut Builder,
    ctx: &mut Ctx,
    expr: &HirExpr,
    line: usize,
) -> Result<Operand, String> {
    Ok(match expr {
        HirExpr::Int(value) => Operand::Const(Const::Int(*value)),
        HirExpr::Float(value) => Operand::Const(Const::Float(*value)),
        HirExpr::Str(text) => Operand::Const(Const::Str(text.clone())),
        HirExpr::Bool(value) => Operand::Const(Const::Bool(*value)),
        HirExpr::Null => Operand::Const(Const::Null),
        HirExpr::Var(var) => Operand::Var(*var),
        HirExpr::Name(name) => Operand::Name(name.clone()),
        HirExpr::BadNum(text) => {
            return Err(unsupported(line, &format!("the number \"{}\"", text)))
        }
        HirExpr::Binary(op, left, right) => {
            let lhs = operand(builder, ctx, left, line)?;
            let rhs = operand(builder, ctx, right, line)?;
            let dest = builder.temp();
            builder.emit(MirInst::Binary {
                dest,
                op: *op,
                lhs,
                rhs,
            });
            Operand::Temp(dest)
        }
        HirExpr::Compare(op, left, right) => {
            let lhs = operand(builder, ctx, left, line)?;
            let rhs = operand(builder, ctx, right, line)?;
            let dest = builder.temp();
            builder.emit(MirInst::Compare {
                dest,
                op: op.clone(),
                lhs,
                rhs,
                line,
            });
            Operand::Temp(dest)
        }
        HirExpr::Logical(op, left, right) => {
            let lhs = operand(builder, ctx, left, line)?;
            let dest = builder.temp();
            let rhs_block = builder.block();
            let short_block = builder.block();
            let end = builder.block();
            let (then_block, else_block, short_value) = match op {
                LogicalOp::And => (rhs_block, short_block, Const::Bool(false)),
                LogicalOp::Or => (short_block, rhs_block, Const::Bool(true)),
            };
            builder.terminate(Terminator::Branch {
                cond: lhs,
                then_block,
                else_block,
            });
            builder.switch(rhs_block);
            let rhs = operand(builder, ctx, right, line)?;
            builder.emit(MirInst::Move { dest, value: rhs });
            builder.terminate(Terminator::Goto(end));
            builder.switch(short_block);
            builder.emit(MirInst::Move {
                dest,
                value: Operand::Const(short_value),
            });
            builder.terminate(Terminator::Goto(end));
            builder.switch(end);
            Operand::Temp(dest)
        }
        HirExpr::Not(value) => {
            let value = operand(builder, ctx, value, line)?;
            let dest = builder.temp();
            builder.emit(MirInst::Not { dest, value });
            Operand::Temp(dest)
        }
        HirExpr::Neg(value) => {
            let value = operand(builder, ctx, value, line)?;
            let dest = builder.temp();
            builder.emit(MirInst::Neg { dest, value });
            Operand::Temp(dest)
        }
        HirExpr::List(items) => {
            let items = operands(builder, ctx, items, line)?;
            let dest = builder.temp();
            builder.emit(MirInst::List { dest, items });
            Operand::Temp(dest)
        }
        HirExpr::Tuple(items) => {
            let items = operands(builder, ctx, items, line)?;
            let dest = builder.temp();
            builder.emit(MirInst::Tuple { dest, items });
            Operand::Temp(dest)
        }
        HirExpr::Set(items) => {
            let items = operands(builder, ctx, items, line)?;
            let dest = builder.temp();
            builder.emit(MirInst::Set { dest, items });
            Operand::Temp(dest)
        }
        HirExpr::Dict(pairs) => {
            let mut lowered = Vec::with_capacity(pairs.len());
            for (key, value) in pairs {
                let key = operand(builder, ctx, key, line)?;
                let value = operand(builder, ctx, value, line)?;
                lowered.push((key, value));
            }
            let dest = builder.temp();
            builder.emit(MirInst::Dict {
                dest,
                pairs: lowered,
            });
            Operand::Temp(dest)
        }
        HirExpr::Index(base, indices) => {
            let base = operand(builder, ctx, base, line)?;
            let indices = operands(builder, ctx, indices, line)?;
            let dest = builder.temp();
            builder.emit(MirInst::Index {
                dest,
                base,
                indices,
                line,
            });
            Operand::Temp(dest)
        }
        HirExpr::Field(object, field) => {
            let base = operand(builder, ctx, object, line)?;
            let dest = builder.temp();
            builder.emit(MirInst::Field {
                dest,
                base,
                field: field.clone(),
            });
            Operand::Temp(dest)
        }
        HirExpr::Call(name, args) => {
            let args = operands(builder, ctx, args, line)?;
            let dest = builder.temp();
            builder.emit(MirInst::Call {
                dest,
                name: name.clone(),
                args,
            });
            // A raise inside the callee left its error pending; this function
            // either handles it here or passes it on exactly as its own raise.
            let target = raise_target(builder, ctx);
            builder.emit(MirInst::CheckError { target });
            Operand::Temp(dest)
        }
        HirExpr::LibCall { lib, func, args } => {
            let args = operands(builder, ctx, args, line)?;
            let dest = builder.temp();
            builder.emit(MirInst::LibCall {
                dest,
                lib: lib.clone(),
                func: func.clone(),
                args,
            });
            Operand::Temp(dest)
        }
        HirExpr::Lambda(index) => Operand::Fn(index + 1),
        HirExpr::Ternary(cond, then_value, else_value) => {
            let dest = builder.temp();
            let cond = operand(builder, ctx, cond, line)?;
            let then_block = builder.block();
            let else_block = builder.block();
            let end = builder.block();
            builder.terminate(Terminator::Branch {
                cond,
                then_block,
                else_block,
            });
            builder.switch(then_block);
            let value = operand(builder, ctx, then_value, line)?;
            builder.emit(MirInst::Move { dest, value });
            builder.terminate(Terminator::Goto(end));
            builder.switch(else_block);
            let value = operand(builder, ctx, else_value, line)?;
            builder.emit(MirInst::Move { dest, value });
            builder.terminate(Terminator::Goto(end));
            builder.switch(end);
            Operand::Temp(dest)
        }
        HirExpr::Contains(haystack, needle) => {
            // The operator spelling of `contains(<haystack>, <needle>)`.
            let haystack = operand(builder, ctx, haystack, line)?;
            let needle = operand(builder, ctx, needle, line)?;
            let dest = builder.temp();
            builder.emit(MirInst::Call {
                dest,
                name: "contains".to_string(),
                args: vec![haystack, needle],
            });
            Operand::Temp(dest)
        }
        HirExpr::IsType { value, ty } => {
            let value = operand(builder, ctx, value, line)?;
            let dest = builder.temp();
            builder.emit(MirInst::IsType {
                dest,
                value,
                ty: *ty,
            });
            Operand::Temp(dest)
        }
        HirExpr::LibFnRef { lib, func } => {
            return Err(unsupported(
                line,
                &format!("the function value \"{}.{}\"", lib, func),
            ))
        }
        HirExpr::HttpGet { lib, .. } => {
            return Err(unsupported(
                line,
                &format!("an HTTP get through \"{}\"", lib),
            ))
        }
        HirExpr::Opaque(what) => return Err(unsupported(line, what)),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn program(source: &str) -> HirProgram {
        let tokens = crate::frontend::lexer::lex(source).expect("lex");
        let stmts = crate::frontend::parser::parse(&tokens).expect("parse");
        hir(&stmts)
    }

    #[test]
    fn a_name_becomes_a_binding() {
        let program = program("x သည် 1 ဖြစ်၏။\nx ကို ဖော်ပြပါ။");
        assert_eq!(program.symbols.len(), 1);
        let symbol = program.symbols.iter().next().expect("one symbol");
        assert_eq!(symbol.name, "x");
        match &program.main.body[1] {
            HirStmt::Print {
                value: HirExpr::Var(var),
                ..
            } => assert_eq!(*var, symbol.id),
            other => panic!("expected a print of a binding, got {:?}", other),
        }
    }

    #[test]
    fn an_unknown_name_stays_dynamic() {
        let program = program("y သည် some_name ဖြစ်၏။");
        assert!(program
            .unresolved
            .iter()
            .any(|name| name.name == "some_name"));
    }

    #[test]
    fn a_range_loop_is_a_counted_loop() {
        let program = program("i သည် 0, 3 ထဲမှ တစ်ခုစီ 1 တိုးခြင်းဖြင့်\n    i ကို ဖော်ပြပါ။\nပြီး။");
        assert!(matches!(program.main.body[0], HirStmt::ForRange { .. }));
    }

    #[test]
    fn a_lambda_gets_its_own_frame_and_capture() {
        let program = program("n သည် 2 ဖြစ်၏။\nf သည် fn(x) { x * n } ဖြစ်၏။");
        assert_eq!(program.functions.len(), 1);
        let lambda = &program.functions[0];
        assert_eq!(lambda.params.len(), 1);
        assert_eq!(lambda.captures.len(), 1);
    }

    #[test]
    fn mir_builds_blocks_for_an_if() {
        let program = program("x သည် 1 ဖြစ်၏။\nအကယ်၍ (x == 1) ဖြစ်လျှင်\n    \"one\" ကို ဖော်ပြပါ။\nပြီး။");
        let mir = mir(&program).expect("lower to mir");
        assert!(mir.functions[0].blocks.len() >= 3);
        assert!(mir.functions[0]
            .blocks
            .iter()
            .any(|block| matches!(block.term, Some(Terminator::Branch { .. }))));
    }

    #[test]
    fn a_construct_outside_mir_is_refused() {
        // `1.2.3` is kept as text because the parser cannot make a number of
        // it, and MIR has no instruction for it yet.
        let program = program("1.2.3 ကို ဖော်ပြပါ။");
        let error = mir(&program).expect_err("a bad number is not in MIR yet");
        assert!(error.contains("E176"), "{}", error);
    }
}
