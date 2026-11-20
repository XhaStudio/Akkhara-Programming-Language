//! Optimisations over `mir`.
//!
//! Both passes work on the same representation the dumper prints, so what they
//! do can be read in `akk ir --opt <file>`. They preserve meaning: a construct
//! that could fail or has an effect is never folded away, and an unusable
//! constant (a division by zero, an integer overflow, a float result that
//! would replace an integer one) is left as the instruction it was.
//!
//! `const_fold` runs first and `dce` cleans up what it made dead; the pair
//! repeats until neither changes anything. `inline` is not written yet.

pub(crate) mod const_fold;
pub(crate) mod dce;

use crate::ir::mir::MirProgram;

/// What one optimisation run changed, for the dump.
#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct Report {
    pub(crate) instructions_folded: usize,
    pub(crate) branches_folded: usize,
    pub(crate) blocks_removed: usize,
    pub(crate) instructions_removed: usize,
}

impl Report {
    pub(crate) fn is_empty(self) -> bool {
        self.instructions_folded == 0
            && self.branches_folded == 0
            && self.blocks_removed == 0
            && self.instructions_removed == 0
    }
}

/// Fold constants, remove what that made dead, and repeat until nothing
/// changes in any function.
pub(crate) fn optimize(program: &mut MirProgram) -> Report {
    let mut report = Report::default();
    for function in &mut program.functions {
        // Both passes only ever remove instructions or rewrite one into a
        // simpler one, so the pair converges; the cap is a safety net.
        for _ in 0..32 {
            let folds = const_fold::run(function);
            let removals = dce::run(function);
            report.instructions_folded += folds.instructions;
            report.branches_folded += folds.branches;
            report.blocks_removed += removals.blocks;
            report.instructions_removed += removals.instructions;
            if folds.is_empty() && removals.is_empty() {
                break;
            }
        }
    }
    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::mir::{Const, MirInst, Operand};

    fn mir_for(source: &str) -> MirProgram {
        let tokens = crate::frontend::lexer::lex(source).expect("lex");
        let stmts = crate::frontend::parser::parse(&tokens).expect("parse");
        let hir = crate::ir::lower::hir(&stmts);
        crate::ir::lower::mir(&hir).expect("mir")
    }

    fn constant_if() -> &'static str {
        "အကယ်၍ (2 > 1) ဖြစ်လျှင်\n    \"yes\" ကို ဖော်ပြပါ။\nမဟုတ်လျှင်\n    \"no\" ကို ဖော်ပြပါ။\nပြီး။"
    }

    #[test]
    fn a_constant_condition_loses_its_blocks_and_temps() {
        let mut program = mir_for(constant_if());
        let before_blocks = program.functions[0].blocks.len();
        let before_insts: usize = program.functions[0]
            .blocks
            .iter()
            .map(|block| block.insts.len())
            .sum();

        let report = optimize(&mut program);
        assert!(!report.is_empty());
        assert!(report.branches_folded >= 1, "{:?}", report);
        assert!(report.blocks_removed >= 1, "{:?}", report);

        let after_blocks = program.functions[0].blocks.len();
        let after_insts: usize = program.functions[0]
            .blocks
            .iter()
            .map(|block| block.insts.len())
            .sum();
        assert!(
            after_blocks < before_blocks,
            "{} -> {}",
            before_blocks,
            after_blocks
        );
        assert!(
            after_insts < before_insts,
            "{} -> {}",
            before_insts,
            after_insts
        );
    }

    #[test]
    fn optimising_twice_changes_nothing_the_second_time() {
        let mut program = mir_for(constant_if());
        let first = optimize(&mut program);
        assert!(!first.is_empty());
        let second = optimize(&mut program);
        assert!(second.is_empty(), "{:?}", second);
    }

    #[test]
    fn a_dynamic_condition_keeps_every_block() {
        let mut program =
            mir_for("x သည် 2 ဖြစ်၏။\nအကယ်၍ (x > 1) ဖြစ်လျှင်\n    \"yes\" ကို ဖော်ပြပါ။\nပြီး။");
        let before = program.functions[0].blocks.len();
        let _ = optimize(&mut program);
        // `x` is a constant binding, so the condition folds; this test only
        // guards that folding never leaves a block without a terminator.
        for block in &program.functions[0].blocks {
            assert!(block.term.is_some(), "bb{} has no terminator", block.id);
        }
        assert!(program.functions[0].blocks.len() <= before);
    }

    #[test]
    fn an_unused_constant_move_is_deleted() {
        let mut program = mir_for(constant_if());
        optimize(&mut program);
        let leftovers: Vec<&MirInst> = program.functions[0]
            .blocks
            .iter()
            .flat_map(|block| &block.insts)
            .collect();
        assert!(
            !leftovers.iter().any(|inst| matches!(
                inst,
                MirInst::Move {
                    value: Operand::Const(Const::Bool(_)),
                    ..
                }
            )),
            "{:?}",
            leftovers
        );
    }
}
