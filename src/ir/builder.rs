//! The MIR builder: blocks, instructions and temporaries.
//!
//! Lowering needs three things from it: somewhere to put an instruction, a
//! fresh block to branch to, and a fresh temporary to write. A block is closed
//! by its terminator, so `emit` after a `return` or `break` starts a new,
//! unreachable block instead of appending past the end of the old one -- dead
//! code stays visible in the dump rather than corrupting the block.

use super::mir::*;
use crate::scope::symbol::VarId;

pub(crate) struct Builder {
    blocks: Vec<BasicBlock>,
    current: BlockId,
    next_temp: Temp,
}

impl Builder {
    pub(crate) fn new() -> Builder {
        Builder {
            blocks: vec![BasicBlock {
                id: 0,
                insts: Vec::new(),
                term: None,
            }],
            current: 0,
            next_temp: 0,
        }
    }

    /// A fresh temporary: the destination of the instruction about to be
    /// emitted.
    pub(crate) fn temp(&mut self) -> Temp {
        let temp = self.next_temp;
        self.next_temp += 1;
        temp
    }

    /// A fresh block. It is not made current; call [`Builder::switch`].
    pub(crate) fn block(&mut self) -> BlockId {
        let id = self.blocks.len() as BlockId;
        self.blocks.push(BasicBlock {
            id,
            insts: Vec::new(),
            term: None,
        });
        id
    }

    pub(crate) fn switch(&mut self, id: BlockId) {
        self.current = id;
    }

    pub(crate) fn emit(&mut self, inst: MirInst) {
        if self.blocks[self.current as usize].term.is_some() {
            let id = self.block();
            self.current = id;
        }
        self.blocks[self.current as usize].insts.push(inst);
    }

    /// Close the current block. A block that is already closed keeps the
    /// terminator it has.
    pub(crate) fn terminate(&mut self, term: Terminator) {
        let block = &mut self.blocks[self.current as usize];
        if block.term.is_none() {
            block.term = Some(term);
        }
    }

    pub(crate) fn is_terminated(&self) -> bool {
        self.blocks[self.current as usize].term.is_some()
    }

    /// Fill in the blocks that were left open -- a block with no terminator
    /// falls through to the block created after it, and the last one exits.
    pub(crate) fn finish(
        mut self,
        name: String,
        params: Vec<VarId>,
        captures: Vec<VarId>,
    ) -> MirFn {
        let count = self.blocks.len();
        for index in 0..count {
            if self.blocks[index].term.is_some() {
                continue;
            }
            let term = if index + 1 < count {
                Terminator::Goto((index + 1) as BlockId)
            } else {
                Terminator::Exit
            };
            self.blocks[index].term = Some(term);
        }
        MirFn {
            name,
            params,
            captures,
            blocks: self.blocks,
            temps: self.next_temp,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn temporaries_and_blocks_are_distinct() {
        let mut builder = Builder::new();
        let first = builder.temp();
        let second = builder.temp();
        assert_ne!(first, second);
        let block = builder.block();
        assert_ne!(block, 0);
    }

    #[test]
    fn an_open_block_falls_through_to_the_next_one() {
        let mut builder = Builder::new();
        let _ = builder.block();
        let function = builder.finish("f".to_string(), Vec::new(), Vec::new());
        assert!(matches!(function.blocks[0].term, Some(Terminator::Goto(1))));
        assert!(matches!(function.blocks[1].term, Some(Terminator::Exit)));
    }

    #[test]
    fn emitting_after_a_terminator_opens_a_new_block() {
        let mut builder = Builder::new();
        builder.terminate(Terminator::Exit);
        builder.emit(MirInst::Print(Operand::Const(Const::Int(1))));
        let function = builder.finish("f".to_string(), Vec::new(), Vec::new());
        assert_eq!(function.blocks.len(), 2);
        assert!(matches!(function.blocks[0].term, Some(Terminator::Exit)));
        assert_eq!(function.blocks[1].insts.len(), 1);
    }
}
