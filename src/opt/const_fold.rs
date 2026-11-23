//! Constant folding over MIR.
//!
//! A fresh look at the values the interpreter's own arithmetic produces, so a
//! folded result is byte-for-byte what running the program would have given:
//! two ints divided give a float, `%` is `rem_euclid`, `^` is a power, and two
//! strings joined with `+` concatenate. Anything else -- a division by zero, an
//! int overflow, a mixed int/float pair -- is left exactly as it was, because
//! folding it would either change the result or hide a run-time error.

use std::collections::{HashMap, HashSet};

use crate::ir::mir::*;
use crate::scope::symbol::VarId;
use crate::types::Type;

/// What one pass of folding changed.
#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct Folds {
    pub(crate) instructions: usize,
    pub(crate) branches: usize,
}

impl Folds {
    pub(crate) fn is_empty(self) -> bool {
        self.instructions == 0 && self.branches == 0
    }
}

/// Fold one function until nothing changes.
///
/// Instructions and branch conditions are folded in the same loop, because
/// folding a comparison into a constant is what makes the branch that reads it
/// foldable on the next round.
pub(crate) fn run(function: &mut MirFn) -> Folds {
    let mut folds = Folds::default();
    for _ in 0..32 {
        let known = constants(function);
        let mut changed = false;
        for block in &mut function.blocks {
            for instruction in &mut block.insts {
                let folded = fold_instruction(instruction, &known);
                if let (Some(value), Some(dest)) = (folded, destination(instruction)) {
                    *instruction = MirInst::Move {
                        dest,
                        value: Operand::Const(value),
                    };
                    folds.instructions += 1;
                    changed = true;
                }
            }
        }
        changed |= fold_branches(function, &known, &mut folds);
        if !changed {
            break;
        }
    }
    folds
}

/// One `branch` whose condition is a constant becomes a `goto`. The side it
/// cannot reach is left for `dce` to delete.
fn fold_branches(function: &mut MirFn, known: &Known, folds: &mut Folds) -> bool {
    let mut changed = false;
    for block in &mut function.blocks {
        let target = match &block.term {
            Some(Terminator::Branch {
                cond,
                then_block,
                else_block,
            }) => match resolve(cond, known) {
                Some(Const::Bool(true)) => Some(*then_block),
                Some(Const::Bool(false)) => Some(*else_block),
                _ => None,
            },
            _ => None,
        };
        if let Some(target) = target {
            block.term = Some(Terminator::Goto(target));
            folds.branches += 1;
            changed = true;
        }
    }
    changed
}

/// The temporaries and bindings that hold a constant for the whole function:
/// exactly one definition, and that definition is a constant.
#[derive(Default)]
pub(crate) struct Known {
    temps: HashMap<Temp, Const>,
    vars: HashMap<VarId, Const>,
}

pub(crate) fn constants(function: &MirFn) -> Known {
    let mut temps: HashMap<Temp, (u32, Option<Const>)> = HashMap::new();
    let mut vars: HashMap<VarId, (u32, Option<Const>)> = HashMap::new();
    for block in &function.blocks {
        for instruction in &block.insts {
            if let Some(temp) = destination(instruction) {
                let value = match instruction {
                    MirInst::Move {
                        value: Operand::Const(value),
                        ..
                    } => Some(value.clone()),
                    _ => None,
                };
                record(&mut temps, temp, value);
            }
            match instruction {
                MirInst::StoreVar {
                    var,
                    op: None,
                    value: Operand::Const(value),
                } => record(&mut vars, *var, Some(value.clone())),
                MirInst::StoreVar { var, .. } => record(&mut vars, *var, None),
                _ => {}
            }
        }
    }
    // A parameter or a captured binding is written by the caller, not by a
    // store inside this function, so its value is not known here.
    let written: HashSet<VarId> = function
        .params
        .iter()
        .chain(function.captures.iter())
        .copied()
        .collect();
    Known {
        temps: single_constants(temps),
        vars: single_constants(vars)
            .into_iter()
            .filter(|(var, _)| !written.contains(var))
            .collect(),
    }
}

fn record<K: std::hash::Hash + Eq>(
    seen: &mut HashMap<K, (u32, Option<Const>)>,
    key: K,
    value: Option<Const>,
) {
    let entry = seen.entry(key).or_insert((0, None));
    entry.0 += 1;
    entry.1 = if entry.0 == 1 { value } else { None };
}

fn single_constants<K: std::hash::Hash + Eq>(
    seen: HashMap<K, (u32, Option<Const>)>,
) -> HashMap<K, Const> {
    seen.into_iter()
        .filter_map(|(key, (count, value))| match (count, value) {
            (1, Some(value)) => Some((key, value)),
            _ => None,
        })
        .collect()
}

pub(crate) fn resolve(operand: &Operand, known: &Known) -> Option<Const> {
    match operand {
        Operand::Const(value) => Some(value.clone()),
        Operand::Temp(temp) => known.temps.get(temp).cloned(),
        Operand::Var(var) => known.vars.get(var).cloned(),
        _ => None,
    }
}

fn fold_instruction(instruction: &MirInst, known: &Known) -> Option<Const> {
    match instruction {
        MirInst::Binary { op, lhs, rhs, .. } => {
            eval_binary(*op, &resolve(lhs, known)?, &resolve(rhs, known)?)
        }
        MirInst::Compare { op, lhs, rhs, .. } => {
            eval_compare(op, &resolve(lhs, known)?, &resolve(rhs, known)?)
        }
        MirInst::Not { value, .. } => match resolve(value, known)? {
            Const::Bool(flag) => Some(Const::Bool(!flag)),
            _ => None,
        },
        MirInst::Neg { value, .. } => match resolve(value, known)? {
            Const::Int(number) => number.checked_neg().map(Const::Int),
            Const::Float(number) => Some(Const::Float(-number)),
            _ => None,
        },
        MirInst::IsType { value, ty, .. } => {
            let value = resolve(value, known)?;
            Some(Const::Bool(const_type(&value) == *ty))
        }
        _ => None,
    }
}

fn destination(instruction: &MirInst) -> Option<Temp> {
    match instruction {
        MirInst::Binary { dest, .. }
        | MirInst::Compare { dest, .. }
        | MirInst::Not { dest, .. }
        | MirInst::Neg { dest, .. }
        | MirInst::Move { dest, .. }
        | MirInst::Call { dest, .. }
        | MirInst::LibCall { dest, .. }
        | MirInst::Index { dest, .. }
        | MirInst::Field { dest, .. }
        | MirInst::List { dest, .. }
        | MirInst::Tuple { dest, .. }
        | MirInst::Set { dest, .. }
        | MirInst::Dict { dest, .. }
        | MirInst::IsType { dest, .. }
        | MirInst::Items { dest, .. }
        | MirInst::Len { dest, .. }
        | MirInst::ErrorMatches { dest, .. } => Some(*dest),
        MirInst::StoreVar { .. }
        | MirInst::StoreIndex { .. }
        | MirInst::StoreField { .. }
        | MirInst::Print(_)
        | MirInst::Eval(_)
        | MirInst::SetError { .. }
        | MirInst::BindError { .. }
        | MirInst::ClearError
        | MirInst::CheckError { .. } => None,
    }
}

/// What a constant of each kind is, so `value is <type>` can be answered
/// without running the program.
fn const_type(value: &Const) -> Type {
    match value {
        Const::Int(_) => Type::Int,
        Const::Float(_) => Type::Float,
        Const::Str(_) => Type::Str,
        Const::Bool(_) => Type::Bool,
        Const::Null => Type::Null,
    }
}

/// Two constants under one arithmetic operator, with the interpreter's own
/// rules -- or `None`, which leaves the instruction alone.
fn eval_binary(op: char, left: &Const, right: &Const) -> Option<Const> {
    match (left, right) {
        (Const::Int(a), Const::Int(b)) => match op {
            // `+`, `-`, `*` are checked rather than wrapping: the interpreter
            // panics on an overflowing int in a debug build, and a folded
            // value would hide that.
            '+' => a.checked_add(*b).map(Const::Int),
            '-' => a.checked_sub(*b).map(Const::Int),
            '*' => a.checked_mul(*b).map(Const::Int),
            // Two ints divided give a float, as `arith` does.
            '/' => {
                if *b == 0 {
                    None
                } else {
                    Some(Const::Float(*a as f64 / *b as f64))
                }
            }
            '%' => {
                if *b == 0 {
                    None
                } else {
                    a.checked_rem_euclid(*b).map(Const::Int)
                }
            }
            '^' => {
                if *b < 0 || *b > u32::MAX as i64 {
                    None
                } else {
                    a.checked_pow(*b as u32).map(Const::Int)
                }
            }
            _ => None,
        },
        (Const::Float(a), Const::Float(b)) => match op {
            '+' => Some(Const::Float(a + b)),
            '-' => Some(Const::Float(a - b)),
            '*' => Some(Const::Float(a * b)),
            _ => None,
        },
        (Const::Str(a), Const::Str(b)) => match op {
            '+' => Some(Const::Str(format!("{}{}", a, b))),
            _ => None,
        },
        _ => None,
    }
}

/// Two constants under one comparison, following `compare_values`: numbers
/// order as `f64` (which is what the interpreter does for ints too), strings
/// order lexicographically, and equality is only folded between two values of
/// the same kind.
fn eval_compare(op: &str, left: &Const, right: &Const) -> Option<Const> {
    match (left, right) {
        // Equality is exact for two ints; only the ordering goes through
        // `f64`, which is what the interpreter compares.
        (Const::Int(a), Const::Int(b)) => Some(Const::Bool(match op {
            "==" => a == b,
            "!=" => a != b,
            _ => ord(*a as f64, *b as f64, op)?,
        })),
        (Const::Float(a), Const::Float(b)) => Some(Const::Bool(match op {
            "==" => a == b,
            "!=" => a != b,
            _ => ord(*a, *b, op)?,
        })),
        (Const::Str(a), Const::Str(b)) => Some(Const::Bool(match op {
            "==" => a == b,
            "!=" => a != b,
            "<" => a < b,
            ">" => a > b,
            "<=" => a <= b,
            ">=" => a >= b,
            _ => return None,
        })),
        (Const::Bool(a), Const::Bool(b)) => match op {
            "==" => Some(Const::Bool(a == b)),
            "!=" => Some(Const::Bool(a != b)),
            _ => None,
        },
        (Const::Null, Const::Null) => match op {
            "==" => Some(Const::Bool(true)),
            "!=" => Some(Const::Bool(false)),
            _ => None,
        },
        _ => None,
    }
}

fn ord(a: f64, b: f64, op: &str) -> Option<bool> {
    Some(match op {
        "<" => a < b,
        ">" => a > b,
        "<=" => a <= b,
        ">=" => a >= b,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn function(source: &str) -> MirFn {
        let tokens = crate::frontend::lexer::lex(source).expect("lex");
        let stmts = crate::frontend::parser::parse(&tokens).expect("parse");
        let hir = crate::ir::lower::hir(&stmts);
        let mut program = crate::ir::lower::mir(&hir).expect("mir");
        program.functions.remove(0)
    }

    fn instructions(function: &MirFn) -> Vec<&MirInst> {
        function
            .blocks
            .iter()
            .flat_map(|block| &block.insts)
            .collect()
    }

    fn has_constant(function: &MirFn, expected: Const) -> bool {
        instructions(function).iter().any(|instruction| {
            matches!(
                instruction,
                MirInst::Move {
                    value: Operand::Const(value),
                    ..
                } if *value == expected
            )
        })
    }

    #[test]
    fn integer_addition_folds() {
        let mut function = function("x သည် 2 + 3 ဖြစ်၏။\nx ကို ဖော်ပြပါ။");
        let folds = run(&mut function);
        assert!(folds.instructions >= 1, "{:?}", folds);
        assert!(has_constant(&function, Const::Int(5)));
    }

    #[test]
    fn two_ints_divided_fold_into_a_float() {
        let mut function = function("x သည် 7 / 2 ဖြစ်၏။\nx ကို ဖော်ပြပါ။");
        run(&mut function);
        assert!(has_constant(&function, Const::Float(3.5)));
    }

    #[test]
    fn strings_concatenate() {
        let mut function = function("x သည် \"akk\" + \"hara\" ဖြစ်၏။\nx ကို ဖော်ပြပါ။");
        run(&mut function);
        assert!(has_constant(&function, Const::Str("akkhara".to_string())));
    }

    #[test]
    fn division_by_zero_is_not_folded() {
        let mut function = function("x သည် 1 / 0 ဖြစ်၏။\nx ကို ဖော်ပြပါ။");
        run(&mut function);
        assert!(instructions(&function)
            .iter()
            .any(|instruction| matches!(instruction, MirInst::Binary { op: '/', .. })));
    }

    #[test]
    fn an_overflowing_addition_is_not_folded() {
        let mut function = function("x သည် 9223372036854775807 + 1 ဖြစ်၏။\nx ကို ဖော်ပြပါ။");
        run(&mut function);
        assert!(instructions(&function)
            .iter()
            .any(|instruction| matches!(instruction, MirInst::Binary { op: '+', .. })));
    }

    #[test]
    fn a_comparison_folds_into_a_constant_branch() {
        let mut function = function("အကယ်၍ (2 > 1) ဖြစ်လျှင်\n    \"yes\" ကို ဖော်ပြပါ။\nပြီး။");
        let folds = run(&mut function);
        assert!(folds.branches >= 1, "{:?}", folds);
        assert!(!function
            .blocks
            .iter()
            .any(|block| matches!(block.term, Some(Terminator::Branch { .. }))));
    }

    #[test]
    fn a_constant_binding_makes_its_uses_constant() {
        let mut function =
            function("n သည် 2 ဖြစ်၏။\nအကယ်၍ (n == 1) ဖြစ်လျှင်\n    \"one\" ကို ဖော်ပြပါ။\nပြီး။");
        let folds = run(&mut function);
        assert!(folds.branches >= 1, "{:?}", folds);
    }

    #[test]
    fn a_reassigned_binding_is_not_a_constant() {
        let mut function = function(
            "n သည် 1 ဖြစ်၏။\nn သည် 2 ဖြစ်၏။\nအကယ်၍ (n == 1) ဖြစ်လျှင်\n    \"one\" ကို ဖော်ပြပါ။\nပြီး။",
        );
        let folds = run(&mut function);
        assert_eq!(folds.branches, 0, "{:?}", folds);
    }

    #[test]
    fn a_type_test_on_a_literal_folds() {
        use crate::ir::builder::Builder;
        use crate::types::Type;

        let mut builder = Builder::new();
        let dest = builder.temp();
        builder.emit(MirInst::IsType {
            dest,
            value: Operand::Const(Const::Int(1)),
            ty: Type::Int,
        });
        builder.terminate(Terminator::Exit);
        let mut function = builder.finish("f".to_string(), Vec::new(), Vec::new());

        run(&mut function);
        assert!(has_constant(&function, Const::Bool(true)));
    }
}
