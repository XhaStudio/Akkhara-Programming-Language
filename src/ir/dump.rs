//! A textual dump of `hir` and `mir`, for `akk ir` and the tests.
//!
//! The format is meant to be read by a person debugging a program or this
//! compiler, and to be stable enough for a test to assert on a line of it. It
//! is not an interchange format: nothing parses it back.

use std::fmt::Write;

use super::hir::*;
use super::mir::*;
use crate::frontend::ast::LogicalOp;

/// Print the resolved program: the symbol table, the names that stayed
/// dynamic, then every function's tree.
pub(crate) fn hir(out: &mut String, program: &HirProgram) {
    let _ = writeln!(out, "== symbols ==");
    if program.symbols.len() == 0 {
        let _ = writeln!(out, "  (none)");
    }
    for symbol in program.symbols.iter() {
        let ty = match symbol.ty {
            Some(ty) => format!(" :{}", ty.name_en()),
            None => String::new(),
        };
        let captured = if symbol.captured { " captured" } else { "" };
        let _ = writeln!(
            out,
            "  {} {} {} line {}{}{}",
            symbol.id,
            symbol.name,
            symbol.kind.label(),
            symbol.line,
            ty,
            captured
        );
    }

    if !program.unresolved.is_empty() {
        let _ = writeln!(out, "== names that stay dynamic ==");
        for name in &program.unresolved {
            let _ = writeln!(out, "  {}", name);
        }
    }

    let _ = writeln!(out, "== hir ==");
    function(out, &program.main);
    for (index, lambda) in program.functions.iter().enumerate() {
        let _ = writeln!(out);
        let _ = writeln!(out, "-- fn#{} --", index);
        function(out, lambda);
    }
}

fn function(out: &mut String, function: &HirFn) {
    let params: Vec<String> = function
        .params
        .iter()
        .map(|param| match &param.default {
            Some(default) => format!("{} {} = {}", param.var, param.name, expr(default)),
            None => format!("{} {}", param.var, param.name),
        })
        .collect();
    let captures: Vec<String> = function
        .captures
        .iter()
        .map(|capture| capture.to_string())
        .collect();
    let mut header = format!("fn {}({})", function.name, params.join(", "));
    if !captures.is_empty() {
        let _ = write!(header, " captures [{}]", captures.join(", "));
    }
    if function.line != 0 {
        let _ = write!(header, " line {}", function.line);
    }
    let _ = writeln!(out, "{}", header);
    body(out, &function.body, 1);
}

fn body(out: &mut String, list: &[HirStmt], depth: usize) {
    if list.is_empty() {
        let _ = writeln!(out, "{}(empty)", indent(depth));
        return;
    }
    for stmt in list {
        statement(out, stmt, depth);
    }
}

fn statement(out: &mut String, stmt: &HirStmt, depth: usize) {
    let pad = indent(depth);
    match stmt {
        HirStmt::Store {
            var,
            ty,
            op,
            value,
            line,
        } => {
            let ty = match ty {
                Some(ty) => format!(" :{} ({})", ty.name_en(), ty.name_mm()),
                None => String::new(),
            };
            let store = match op {
                Some(op) => format!("{}{} {}= {}", var, ty, op, expr(value)),
                None => format!("{}{} = {}", var, ty, expr(value)),
            };
            let _ = writeln!(out, "{}store {}  (line {})", pad, store, line);
        }
        HirStmt::SetIndex {
            target,
            indices,
            value,
            line,
        } => {
            let _ = writeln!(
                out,
                "{}store {}[{}] = {}  (line {})",
                pad,
                expr(target),
                join(indices),
                expr(value),
                line
            );
        }
        HirStmt::SetField {
            object,
            field,
            value,
            line,
        } => {
            let _ = writeln!(
                out,
                "{}store {}.{} = {}  (line {})",
                pad,
                expr(object),
                field,
                expr(value),
                line
            );
        }
        HirStmt::Print { value, line } => {
            let _ = writeln!(out, "{}print {}  (line {})", pad, expr(value), line);
        }
        HirStmt::Eval { value, line } => {
            let _ = writeln!(out, "{}eval {}  (line {})", pad, expr(value), line);
        }
        HirStmt::If { branches } => {
            let _ = writeln!(out, "{}if", pad);
            for branch in branches {
                let head = match (&branch.cond, branch.negate) {
                    (Some(cond), false) => format!("if {}", expr(cond)),
                    (Some(cond), true) => format!("unless {}", expr(cond)),
                    (None, _) => "else".to_string(),
                };
                let _ = writeln!(out, "{}  {} {{", pad, head);
                body(out, &branch.body, depth + 2);
                let _ = writeln!(out, "{}  }}", pad);
            }
        }
        HirStmt::While {
            cond,
            negate,
            body: loop_body,
            step,
            line,
        } => {
            let word = if *negate { "until" } else { "while" };
            let step = match step {
                Some(step) => format!(" step {}", statement_text(step)),
                None => String::new(),
            };
            let _ = writeln!(
                out,
                "{}{} {} {{  (line {}){}",
                pad,
                word,
                expr(cond),
                line,
                step
            );
            body(out, loop_body, depth + 1);
            let _ = writeln!(out, "{}}}", pad);
        }
        HirStmt::Loop {
            body: loop_body,
            line,
        } => {
            let _ = writeln!(out, "{}loop {{  (line {})", pad, line);
            body(out, loop_body, depth + 1);
            let _ = writeln!(out, "{}}}", pad);
        }
        HirStmt::ForRange {
            var,
            start,
            end,
            step,
            op,
            body: loop_body,
            line,
        } => {
            let _ = writeln!(
                out,
                "{}for {} = {}; {} {} {}; {} {} {{  (line {})",
                pad,
                var,
                expr(start),
                var,
                if matches!(op, '+' | '*') { "<" } else { ">" },
                expr(end),
                var,
                format_args!("{}= {}", op, expr(step)),
                line
            );
            body(out, loop_body, depth + 1);
            let _ = writeln!(out, "{}}}", pad);
        }
        HirStmt::ForEach {
            var,
            iter,
            body: loop_body,
            line,
        } => {
            let _ = writeln!(
                out,
                "{}for {} in {} {{  (line {})",
                pad,
                var,
                expr(iter),
                line
            );
            body(out, loop_body, depth + 1);
            let _ = writeln!(out, "{}}}", pad);
        }
        HirStmt::Return { value, line } => {
            let value = match value {
                Some(value) => format!(" {}", expr(value)),
                None => String::new(),
            };
            let _ = writeln!(out, "{}return{}  (line {})", pad, value, line);
        }
        HirStmt::Break { value, line } => {
            let value = match value {
                Some(value) => format!(" {}", expr(value)),
                None => String::new(),
            };
            let _ = writeln!(out, "{}break{}  (line {})", pad, value, line);
        }
        HirStmt::Continue { line } => {
            let _ = writeln!(out, "{}continue  (line {})", pad, line);
        }
        HirStmt::Import(libs) => {
            let _ = writeln!(out, "{}import {}", pad, libs.join(", "));
        }
        HirStmt::Opaque { what, line } => {
            let _ = writeln!(out, "{}todo {}  (line {})", pad, what, line);
        }
    }
}

/// A one-line rendering of a statement, for the step of a C-style `for`.
fn statement_text(stmt: &HirStmt) -> String {
    let mut text = String::new();
    statement(&mut text, stmt, 0);
    text.trim().to_string()
}

fn indent(depth: usize) -> String {
    " ".repeat(depth * 2)
}

fn join(items: &[HirExpr]) -> String {
    items.iter().map(expr).collect::<Vec<String>>().join(", ")
}

fn expr(value: &HirExpr) -> String {
    match value {
        HirExpr::Int(number) => number.to_string(),
        HirExpr::Float(number) => format!("{:?}", number),
        HirExpr::Str(text) => format!("{:?}", text),
        HirExpr::Bool(flag) => flag.to_string(),
        HirExpr::Null => "null".to_string(),
        HirExpr::Var(var) => var.to_string(),
        HirExpr::Name(name) => name.clone(),
        HirExpr::BadNum(text) => format!("<bad number {:?}>", text),
        HirExpr::Binary(op, left, right) => {
            format!("({} {} {})", expr(left), op, expr(right))
        }
        HirExpr::Compare(op, left, right) => {
            format!("({} {} {})", expr(left), op, expr(right))
        }
        HirExpr::Logical(op, left, right) => {
            let op = match op {
                LogicalOp::And => "&&",
                LogicalOp::Or => "||",
            };
            format!("({} {} {})", expr(left), op, expr(right))
        }
        HirExpr::Not(inner) => format!("!{}", expr(inner)),
        HirExpr::Neg(inner) => format!("-{}", expr(inner)),
        HirExpr::List(items) => format!("[{}]", join(items)),
        HirExpr::Tuple(items) => format!("({})", join(items)),
        HirExpr::Set(items) => format!("{{{}}}", join(items)),
        HirExpr::Dict(pairs) => {
            let pairs: Vec<String> = pairs
                .iter()
                .map(|(key, value)| format!("{}: {}", expr(key), expr(value)))
                .collect();
            format!("{{{}}}", pairs.join(", "))
        }
        HirExpr::Index(base, indices) => format!("{}[{}]", expr(base), join(indices)),
        HirExpr::Field(object, field) => format!("{}.{}", expr(object), field),
        HirExpr::Call(name, args) => format!("{}({})", name, join(args)),
        HirExpr::LibCall { lib, func, args } => {
            format!("{}.{}({})", lib, func, join(args))
        }
        HirExpr::Lambda(index) => format!("fn#{}", index),
        HirExpr::Ternary(cond, then_value, else_value) => format!(
            "({} ? {} : {})",
            expr(cond),
            expr(then_value),
            expr(else_value)
        ),
        HirExpr::Contains(haystack, needle) => {
            format!("({} in {})", expr(needle), expr(haystack))
        }
        HirExpr::IsType { value, ty } => format!("({} is {})", expr(value), ty.name_en()),
        HirExpr::LibFnRef { lib, func } => format!("{}.{}", lib, func),
        HirExpr::HttpGet { lib, url } => format!("{} get {}", lib, expr(url)),
        HirExpr::Opaque(what) => format!("<todo {}>", what),
    }
}

/// Print the blocks of every function.
pub(crate) fn mir(out: &mut String, program: &MirProgram) {
    let _ = writeln!(out, "== mir ==");
    functions(out, program);
}

/// Print the same blocks after `opt::optimize`, so a reader can compare the
/// two sections line by line.
pub(crate) fn mir_optimised(out: &mut String, program: &MirProgram, report: &crate::opt::Report) {
    let _ = writeln!(out, "== mir (optimised) ==");
    if report.is_empty() {
        let _ = writeln!(out, "  nothing to optimise");
    } else {
        let _ = writeln!(
            out,
            "  folded {} instructions and {} branches; removed {} blocks and {} instructions",
            report.instructions_folded,
            report.branches_folded,
            report.blocks_removed,
            report.instructions_removed
        );
    }
    functions(out, program);
}

fn functions(out: &mut String, program: &MirProgram) {
    for function in &program.functions {
        let params: Vec<String> = function.params.iter().map(|var| var.to_string()).collect();
        let captures: Vec<String> = function
            .captures
            .iter()
            .map(|var| var.to_string())
            .collect();
        let mut header = format!(
            "fn {}({}) [{} temps]",
            function.name,
            params.join(", "),
            function.temps
        );
        if !captures.is_empty() {
            let _ = write!(header, " captures [{}]", captures.join(", "));
        }
        let _ = writeln!(out, "{}", header);
        for block in &function.blocks {
            let _ = writeln!(out, "  bb{}:", block.id);
            for inst in &block.insts {
                let _ = writeln!(out, "    {}", inst_text(inst));
            }
            if let Some(term) = &block.term {
                let _ = writeln!(out, "    {}", term);
            }
        }
    }
}

fn inst_text(inst: &MirInst) -> String {
    match inst {
        MirInst::Binary { dest, op, lhs, rhs } => format!("t{} = {} {} {}", dest, lhs, op, rhs),
        MirInst::Compare { dest, op, lhs, rhs } => format!("t{} = ({} {} {})", dest, lhs, op, rhs),
        MirInst::Not { dest, value } => format!("t{} = !{}", dest, value),
        MirInst::Neg { dest, value } => format!("t{} = -{}", dest, value),
        MirInst::Move { dest, value } => format!("t{} = {}", dest, value),
        MirInst::Call { dest, name, args } => {
            format!("t{} = call {}({})", dest, name, operands(args))
        }
        MirInst::LibCall {
            dest,
            lib,
            func,
            args,
        } => format!("t{} = call {}.{}({})", dest, lib, func, operands(args)),
        MirInst::Index {
            dest,
            base,
            indices,
        } => format!("t{} = {}[{}]", dest, base, operands(indices)),
        MirInst::Field { dest, base, field } => format!("t{} = {}.{}", dest, base, field),
        MirInst::List { dest, items } => format!("t{} = [{}]", dest, operands(items)),
        MirInst::Tuple { dest, items } => format!("t{} = ({})", dest, operands(items)),
        MirInst::Set { dest, items } => format!("t{} = {{{}}}", dest, operands(items)),
        MirInst::Dict { dest, pairs } => {
            let pairs: Vec<String> = pairs
                .iter()
                .map(|(key, value)| format!("{}: {}", key, value))
                .collect();
            format!("t{} = {{{}}}", dest, pairs.join(", "))
        }
        MirInst::IsType { dest, value, ty } => {
            format!("t{} = ({} is {})", dest, value, ty.name_en())
        }
        MirInst::Items { dest, value } => format!("t{} = items({})", dest, value),
        MirInst::Len { dest, value } => format!("t{} = len({})", dest, value),
        MirInst::StoreVar { var, op, value } => match op {
            Some(op) => format!("{} {}= {}", var, op, value),
            None => format!("{} = {}", var, value),
        },
        MirInst::StoreIndex {
            base,
            indices,
            value,
        } => format!("{}[{}] = {}", base, operands(indices), value),
        MirInst::StoreField { base, field, value } => {
            format!("{}.{} = {}", base, field, value)
        }
        MirInst::Print(value) => format!("print {}", value),
        MirInst::Eval(value) => format!("eval {}", value),
    }
}

fn operands(list: &[Operand]) -> String {
    list.iter()
        .map(|operand| operand.to_string())
        .collect::<Vec<String>>()
        .join(", ")
}
