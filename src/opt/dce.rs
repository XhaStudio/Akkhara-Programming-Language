//! Dead-code elimination over MIR.
//!
//! Two things go: a block nothing branches to, and an instruction whose result
//! nobody reads. Only instructions that cannot fail and have no effect are
//! removed -- `Move`, a list/tuple/set/dict construction and a type test -- so
//! arithmetic that could raise a type error, an index that could be out of
//! range, a call, a print and a store all stay, even when their value is never
//! used. Removing one of those would change what the program does or which
//! error it reports, which no optimisation here is allowed to do.

use std::collections::{HashMap, HashSet};

use crate::ir::mir::*;

/// What one pass of elimination removed.
#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct Removals {
    pub(crate) blocks: usize,
    pub(crate) instructions: usize,
}

impl Removals {
    pub(crate) fn is_empty(self) -> bool {
        self.blocks == 0 && self.instructions == 0
    }
}

/// Remove unreachable blocks, then unused pure definitions, repeating until
/// nothing is left to remove.
pub(crate) fn run(function: &mut MirFn) -> Removals {
    let mut removals = Removals::default();

    let reachable = reachable(function);
    if reachable.len() < function.blocks.len() {
        removals.blocks = function.blocks.len() - reachable.len();
        prune(function, &reachable);
    }

    loop {
        let used = used_temps(function);
        let mut removed = 0;
        for block in &mut function.blocks {
            let before = block.insts.len();
            block
                .insts
                .retain(|instruction| match removable(instruction) {
                    Some(temp) => used.contains(&temp),
                    None => true,
                });
            removed += before - block.insts.len();
        }
        if removed == 0 {
            break;
        }
        removals.instructions += removed;
    }

    // `temps` is the number the dump prints: report what is left, not what was
    // allocated before elimination.
    let live = all_temps(function);
    function.temps = live.iter().copied().max().map_or(0, |temp| temp + 1);
    removals
}

/// Every block a `goto`/`branch` from block 0 can reach.
fn reachable(function: &MirFn) -> HashSet<usize> {
    let mut seen = HashSet::new();
    let mut pending = vec![0usize];
    while let Some(index) = pending.pop() {
        if !seen.insert(index) {
            continue;
        }
        let Some(block) = function.blocks.get(index) else {
            continue;
        };
        // A call that raises jumps to its check's target, and a zero-divisor
        // guard jumps to its handler, so those blocks are reachable even
        // though the block's terminator does not name them.
        for instruction in &block.insts {
            match instruction {
                MirInst::CheckError { target } | MirInst::GuardZero { target, .. } => {
                    pending.push(*target as usize)
                }
                _ => {}
            }
        }
        match &block.term {
            Some(Terminator::Goto(next)) => pending.push(*next as usize),
            Some(Terminator::Branch {
                then_block,
                else_block,
                ..
            }) => {
                pending.push(*then_block as usize);
                pending.push(*else_block as usize);
            }
            _ => {}
        }
    }
    seen
}

/// Keep only the reachable blocks, renumbering them in the order they had and
/// rewriting every jump that pointed at one.
fn prune(function: &mut MirFn, reachable: &HashSet<usize>) {
    let old = std::mem::take(&mut function.blocks);
    let mut numbers = HashMap::with_capacity(reachable.len());
    for index in 0..old.len() {
        if reachable.contains(&index) {
            numbers.insert(index as BlockId, numbers.len() as BlockId);
        }
    }
    let mut kept = Vec::with_capacity(reachable.len());
    for (index, mut block) in old.into_iter().enumerate() {
        let Some(&number) = numbers.get(&(index as BlockId)) else {
            continue;
        };
        block.id = number;
        block.term = block.term.map(|term| remap(term, &numbers));
        for instruction in &mut block.insts {
            match instruction {
                MirInst::CheckError { target } | MirInst::GuardZero { target, .. } => {
                    *target = numbers.get(target).copied().unwrap_or(*target);
                }
                _ => {}
            }
        }
        kept.push(block);
    }
    function.blocks = kept;
}

fn remap(term: Terminator, numbers: &HashMap<BlockId, BlockId>) -> Terminator {
    let number = |id: BlockId| numbers.get(&id).copied().unwrap_or(id);
    match term {
        Terminator::Goto(next) => Terminator::Goto(number(next)),
        Terminator::Branch {
            cond,
            then_block,
            else_block,
        } => Terminator::Branch {
            cond,
            then_block: number(then_block),
            else_block: number(else_block),
        },
        other => other,
    }
}

/// The instructions whose removal is always safe when their result is unused:
/// they cannot fail and have no side effect.
fn removable(instruction: &MirInst) -> Option<Temp> {
    match instruction {
        MirInst::Move { dest, .. }
        | MirInst::List { dest, .. }
        | MirInst::Tuple { dest, .. }
        | MirInst::Set { dest, .. }
        | MirInst::Dict { dest, .. }
        | MirInst::IsType { dest, .. } => Some(*dest),
        _ => None,
    }
}

/// The values one instruction reads.
fn operands(instruction: &MirInst) -> Vec<&Operand> {
    match instruction {
        MirInst::Binary { lhs, rhs, .. } | MirInst::Compare { lhs, rhs, .. } => vec![lhs, rhs],
        MirInst::Not { value, .. }
        | MirInst::Neg { value, .. }
        | MirInst::Move { value, .. }
        | MirInst::IsType { value, .. }
        | MirInst::Items { value, .. }
        | MirInst::Len { value, .. }
        | MirInst::ToStr { value, .. }
        | MirInst::StoreVar { value, .. }
        | MirInst::SetError { value, .. } => vec![value],
        MirInst::Call { args, .. }
        | MirInst::LibCall { args, .. }
        | MirInst::List { items: args, .. }
        | MirInst::Tuple { items: args, .. }
        | MirInst::Set { items: args, .. } => args.iter().collect(),
        MirInst::Index { base, indices, .. } => {
            std::iter::once(base).chain(indices.iter()).collect()
        }
        MirInst::Field { base, .. } => vec![base],
        MirInst::Dict { pairs, .. } => pairs.iter().flat_map(|(key, value)| [key, value]).collect(),
        MirInst::StoreIndex {
            base,
            indices,
            value,
            ..
        } => std::iter::once(base)
            .chain(indices.iter())
            .chain(std::iter::once(value))
            .collect(),
        MirInst::StoreField { base, value, .. } => vec![base, value],
        MirInst::Print(value) | MirInst::Eval(value) => vec![value],
        MirInst::GuardZero { lhs, rhs, .. } => vec![lhs, rhs],
        MirInst::ErrorMatches { .. }
        | MirInst::BindError { .. }
        | MirInst::ClearError
        | MirInst::CheckError { .. } => Vec::new(),
    }
}

fn used_temps(function: &MirFn) -> HashSet<Temp> {
    let mut used = HashSet::new();
    for block in &function.blocks {
        for instruction in &block.insts {
            for operand in operands(instruction) {
                if let Operand::Temp(temp) = operand {
                    used.insert(*temp);
                }
            }
        }
        if let Some(term) = &block.term {
            let condition = match term {
                Terminator::Branch { cond, .. } => Some(cond),
                Terminator::Return(value) => value.as_ref(),
                _ => None,
            };
            if let Some(Operand::Temp(temp)) = condition {
                used.insert(*temp);
            }
        }
    }
    used
}

fn all_temps(function: &MirFn) -> HashSet<Temp> {
    let mut temps = used_temps(function);
    for block in &function.blocks {
        for instruction in &block.insts {
            if let Some(dest) = dest(instruction) {
                temps.insert(dest);
            }
        }
    }
    temps
}

fn dest(instruction: &MirInst) -> Option<Temp> {
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
        | MirInst::ToStr { dest, .. }
        | MirInst::ErrorMatches { dest, .. } => Some(*dest),
        MirInst::StoreVar { .. }
        | MirInst::StoreIndex { .. }
        | MirInst::StoreField { .. }
        | MirInst::Print(_)
        | MirInst::Eval(_)
        | MirInst::SetError { .. }
        | MirInst::GuardZero { .. }
        | MirInst::BindError { .. }
        | MirInst::ClearError
        | MirInst::CheckError { .. } => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::builder::Builder;

    fn function(body: impl FnOnce(&mut Builder)) -> MirFn {
        let mut builder = Builder::new();
        body(&mut builder);
        builder.finish("f".to_string(), Vec::new(), Vec::new())
    }

    #[test]
    fn an_unused_move_is_removed() {
        let mut function = function(|builder| {
            let dest = builder.temp();
            builder.emit(MirInst::Move {
                dest,
                value: Operand::Const(Const::Int(1)),
            });
            builder.terminate(Terminator::Exit);
        });
        let removals = run(&mut function);
        assert_eq!(removals.instructions, 1);
        assert!(function.blocks[0].insts.is_empty());
    }

    #[test]
    fn an_unused_index_is_kept_because_it_can_fail() {
        let mut function = function(|builder| {
            let dest = builder.temp();
            builder.emit(MirInst::Index {
                dest,
                base: Operand::Const(Const::Str("a".to_string())),
                indices: vec![Operand::Const(Const::Int(9))],
                line: 1,
            });
            builder.terminate(Terminator::Exit);
        });
        let removals = run(&mut function);
        assert_eq!(removals.instructions, 0);
        assert_eq!(function.blocks[0].insts.len(), 1);
    }

    #[test]
    fn a_call_is_never_removed() {
        let mut function = function(|builder| {
            let dest = builder.temp();
            builder.emit(MirInst::Call {
                dest,
                name: "now".to_string(),
                args: Vec::new(),
            });
            builder.terminate(Terminator::Exit);
        });
        run(&mut function);
        assert_eq!(function.blocks[0].insts.len(), 1);
    }

    #[test]
    fn an_unreachable_block_is_removed_and_the_rest_renumbered() {
        let mut function = function(|builder| {
            let dead = builder.block();
            let end = builder.block();
            builder.terminate(Terminator::Goto(end));
            builder.switch(dead);
            builder.emit(MirInst::Print(Operand::Const(Const::Int(1))));
            builder.terminate(Terminator::Goto(end));
            builder.switch(end);
            builder.terminate(Terminator::Exit);
        });
        let removals = run(&mut function);
        assert_eq!(removals.blocks, 1);
        assert_eq!(function.blocks.len(), 2);
        assert_eq!(function.blocks[0].id, 0);
        assert_eq!(function.blocks[1].id, 1);
        assert!(matches!(function.blocks[0].term, Some(Terminator::Goto(1))));
    }

    #[test]
    fn an_operand_that_becomes_unused_is_removed_in_the_next_round() {
        let mut function = function(|builder| {
            let source = builder.temp();
            let copy = builder.temp();
            builder.emit(MirInst::Move {
                dest: source,
                value: Operand::Const(Const::Int(1)),
            });
            builder.emit(MirInst::Move {
                dest: copy,
                value: Operand::Temp(source),
            });
            builder.terminate(Terminator::Exit);
        });
        let removals = run(&mut function);
        assert_eq!(removals.instructions, 2);
        assert!(function.blocks[0].insts.is_empty());
    }
}
