//! A very small x86-64 encoder: exactly the instructions the native backend
//! emits, nothing more.
//!
//! There is no assembler on the build machine, so the bytes are written here.
//! Every helper emits one instruction with explicit REX/ModRM bytes; a name
//! that is not declared below simply does not exist, which keeps the encoder
//! honest about the subset it claims.

/// Registers, in x86 encoding order.
pub(crate) const RAX: u8 = 0;
pub(crate) const RCX: u8 = 1;
pub(crate) const RDX: u8 = 2;
pub(crate) const RBP: u8 = 5;
pub(crate) const RSI: u8 = 6;
pub(crate) const RDI: u8 = 7;
pub(crate) const R8: u8 = 8;
pub(crate) const R9: u8 = 9;
pub(crate) const R10: u8 = 10;
pub(crate) const R11: u8 = 11;

/// SSE registers, in x86 encoding order. The native slice uses two: the
/// accumulator and the second operand of a float operation.
pub(crate) const XMM0: u8 = 0;
pub(crate) const XMM1: u8 = 1;
pub(crate) const XMM2: u8 = 2;

// Condition codes for `setcc` / `jcc`.
pub(crate) const CC_E: u8 = 0x4;
pub(crate) const CC_NE: u8 = 0x5;
pub(crate) const CC_L: u8 = 0xC;
pub(crate) const CC_GE: u8 = 0xD;
pub(crate) const CC_LE: u8 = 0xE;
pub(crate) const CC_G: u8 = 0xF;
pub(crate) const CC_NS: u8 = 0x9;
pub(crate) const CC_B: u8 = 0x2;
pub(crate) const CC_BE: u8 = 0x6;
pub(crate) const CC_A: u8 = 0x7;
pub(crate) const CC_AE: u8 = 0x3;
/// Parity: set when the comparison was unordered (a NaN was involved).
pub(crate) const CC_P: u8 = 0xA;

/// A code buffer with labels, so a call to a routine emitted later can be
/// patched once every byte exists.
pub(crate) struct Code {
    bytes: Vec<u8>,
    labels: Vec<(String, usize)>,
    /// `(position of the rel32, label, is a conditional jump's cc or None)`
    branches: Vec<(usize, String)>,
    /// Positions of `mov reg, imm64` immediates whose value is a data address.
    fixups: Vec<(usize, u32)>,
}

impl Code {
    pub(crate) fn new() -> Code {
        Code {
            bytes: Vec::new(),
            labels: Vec::new(),
            branches: Vec::new(),
            fixups: Vec::new(),
        }
    }

    pub(crate) fn position(&self) -> usize {
        self.bytes.len()
    }

    fn byte(&mut self, value: u8) {
        self.bytes.push(value);
    }

    fn imm32(&mut self, value: i32) {
        self.bytes.extend_from_slice(&value.to_le_bytes());
    }

    fn imm64(&mut self, value: u64) {
        self.bytes.extend_from_slice(&value.to_le_bytes());
    }

    /// REX with the W bit, and the R/B extension bits for the two registers.
    fn rex(&mut self, wide: bool, reg: u8, rm: u8) {
        let mut rex = 0x40;
        if wide {
            rex |= 0x08;
        }
        if reg & 0x08 != 0 {
            rex |= 0x04;
        }
        if rm & 0x08 != 0 {
            rex |= 0x01;
        }
        if rex != 0x40 {
            self.byte(rex);
        }
    }

    fn modrm(&mut self, mode: u8, reg: u8, rm: u8) {
        self.byte((mode << 6) | ((reg & 7) << 3) | (rm & 7));
    }

    /// `mov reg, imm64`
    pub(crate) fn mov_imm64(&mut self, reg: u8, value: u64) {
        self.rex(true, 0, reg);
        self.byte(0xB8 + (reg & 7));
        self.imm64(value);
    }

    /// `mov reg, imm32`, sign-extended into the 64-bit register.
    pub(crate) fn mov_imm32(&mut self, reg: u8, value: i32) {
        self.rex(true, 0, reg);
        self.byte(0xC7);
        self.modrm(3, 0, reg);
        self.imm32(value);
    }

    /// `mov reg32, imm32` -- zero-extends into the full register.
    pub(crate) fn mov_imm32_zero(&mut self, reg: u8, value: u32) {
        self.rex(false, 0, reg);
        self.byte(0xB8 + (reg & 7));
        self.imm32(value as i32);
    }

    /// `mov dst, src`
    pub(crate) fn mov_rr(&mut self, dst: u8, src: u8) {
        self.rex(true, src, dst);
        self.byte(0x89);
        self.modrm(3, src, dst);
    }

    /// `mov dst, [rbp + disp]`
    pub(crate) fn load(&mut self, dst: u8, disp: i32) {
        self.rex(true, dst, RBP);
        self.byte(0x8B);
        self.modrm(2, dst, RBP);
        self.imm32(disp);
    }

    /// `mov [rbp + disp], src`
    pub(crate) fn store(&mut self, disp: i32, src: u8) {
        self.rex(true, src, RBP);
        self.byte(0x89);
        self.modrm(2, src, RBP);
        self.imm32(disp);
    }

    /// `mov dst, [rsp + disp8]`
    pub(crate) fn load_rsp(&mut self, dst: u8, disp: u8) {
        self.rex(true, dst, 4);
        self.byte(0x8B);
        self.modrm(1, dst, 4);
        self.byte(0x24);
        self.byte(disp);
    }

    /// `mov [rsp + disp8], src`
    pub(crate) fn store_rsp(&mut self, disp: u8, src: u8) {
        self.rex(true, src, 4);
        self.byte(0x89);
        self.modrm(1, src, 4);
        self.byte(0x24);
        self.byte(disp);
    }

    /// `lea dst, [base + disp8]`
    pub(crate) fn lea_disp8(&mut self, dst: u8, base: u8, disp: i8) {
        self.rex(true, dst, base);
        self.byte(0x8D);
        self.modrm(1, dst, base);
        self.byte(disp as u8);
    }

    /// `mov dst, [src]` (a full 64-bit load)
    pub(crate) fn load_ptr(&mut self, dst: u8, src: u8) {
        self.rex(true, dst, src);
        self.byte(0x8B);
        self.modrm(0, dst, src);
    }

    /// `mov [ptr], src8`
    pub(crate) fn store_byte(&mut self, ptr: u8, src: u8) {
        self.rex(false, src, ptr);
        self.byte(0x88);
        self.modrm(0, src, ptr);
    }

    /// `mov byte [ptr], imm8`
    pub(crate) fn store_byte_imm(&mut self, ptr: u8, value: u8) {
        self.rex(false, 0, ptr);
        self.byte(0xC6);
        self.modrm(0, 0, ptr);
        self.byte(value);
    }

    pub(crate) fn add_rr(&mut self, dst: u8, src: u8) {
        self.rex(true, src, dst);
        self.byte(0x01);
        self.modrm(3, src, dst);
    }

    pub(crate) fn sub_rr(&mut self, dst: u8, src: u8) {
        self.rex(true, src, dst);
        self.byte(0x29);
        self.modrm(3, src, dst);
    }

    pub(crate) fn imul_rr(&mut self, dst: u8, src: u8) {
        self.rex(true, dst, src);
        self.byte(0x0F);
        self.byte(0xAF);
        self.modrm(3, dst, src);
    }

    /// `add dst, imm8`
    pub(crate) fn add_imm8(&mut self, dst: u8, value: i8) {
        self.rex(true, 0, dst);
        self.byte(0x83);
        self.modrm(3, 0, dst);
        self.byte(value as u8);
    }

    /// `sub dst, imm8`
    pub(crate) fn sub_imm8(&mut self, dst: u8, value: i8) {
        self.rex(true, 0, dst);
        self.byte(0x83);
        self.modrm(3, 5, dst);
        self.byte(value as u8);
    }

    /// `or dst, src`
    pub(crate) fn or_rr(&mut self, dst: u8, src: u8) {
        self.rex(true, src, dst);
        self.byte(0x09);
        self.modrm(3, src, dst);
    }

    /// REX for an instruction whose ModRM carries a SIB byte: the R bit for
    /// the register operand, the X bit for the index and the B bit for the
    /// base. Dropping X would silently index with a different register.
    fn rex_sib(&mut self, reg: u8, index: u8, base: u8) {
        let mut rex = 0x40;
        if reg & 0x08 != 0 {
            rex |= 0x04;
        }
        if index & 0x08 != 0 {
            rex |= 0x02;
        }
        if base & 0x08 != 0 {
            rex |= 0x01;
        }
        if rex != 0x40 {
            self.byte(rex);
        }
    }

    /// `movzx dst, byte [base + index]` -- neither register may be `rsp`, and
    /// the index may not be `rsp`/`r12` either (those mean "no index").
    pub(crate) fn load_byte_index(&mut self, dst: u8, base: u8, index: u8) {
        self.rex_sib(dst, index, base);
        self.byte(0x0F);
        self.byte(0xB6);
        self.modrm(0, dst, 4);
        self.sib(0, index, base);
    }

    /// `mov byte [base + index], src`
    pub(crate) fn store_byte_index(&mut self, base: u8, index: u8, src: u8) {
        self.rex_sib(src, index, base);
        self.byte(0x88);
        self.modrm(0, src, 4);
        self.sib(0, index, base);
    }

    fn sib(&mut self, scale: u8, index: u8, base: u8) {
        self.byte((scale << 6) | ((index & 7) << 3) | (base & 7));
    }

    /// `and dst, src`
    pub(crate) fn and_rr(&mut self, dst: u8, src: u8) {
        self.rex(true, src, dst);
        self.byte(0x21);
        self.modrm(3, src, dst);
    }

    pub(crate) fn cmp_rr(&mut self, lhs: u8, rhs: u8) {
        self.rex(true, rhs, lhs);
        self.byte(0x39);
        self.modrm(3, rhs, lhs);
    }

    pub(crate) fn test_rr(&mut self, lhs: u8, rhs: u8) {
        self.rex(true, rhs, lhs);
        self.byte(0x85);
        self.modrm(3, rhs, lhs);
    }

    pub(crate) fn xor_rr(&mut self, lhs: u8, rhs: u8) {
        self.rex(true, rhs, lhs);
        self.byte(0x31);
        self.modrm(3, rhs, lhs);
    }

    /// `neg reg`
    pub(crate) fn neg(&mut self, reg: u8) {
        self.rex(true, 0, reg);
        self.byte(0xF7);
        self.modrm(3, 3, reg);
    }

    /// `cqo` -- sign-extend rax into rdx:rax.
    pub(crate) fn cqo(&mut self) {
        self.byte(0x48);
        self.byte(0x99);
    }

    /// `idiv reg`
    pub(crate) fn idiv(&mut self, reg: u8) {
        self.rex(true, 0, reg);
        self.byte(0xF7);
        self.modrm(3, 7, reg);
    }

    /// `div reg` -- unsigned.
    pub(crate) fn div(&mut self, reg: u8) {
        self.rex(true, 0, reg);
        self.byte(0xF7);
        self.modrm(3, 6, reg);
    }

    /// `setcc al` followed by `movzx eax, al`
    pub(crate) fn setcc(&mut self, cc: u8) {
        self.byte(0x0F);
        self.byte(0x90 + cc);
        self.modrm(3, 0, RAX);
        // movzx eax, al -- clears the rest of rax so the value is 0 or 1.
        self.byte(0x0F);
        self.byte(0xB6);
        self.modrm(3, 0, RAX);
    }

    /// `lea dst, [rbp + disp]` with a 32-bit displacement.
    pub(crate) fn lea_disp32(&mut self, dst: u8, base: u8, disp: i32) {
        self.rex(true, dst, base);
        self.byte(0x8D);
        self.modrm(2, dst, base);
        self.imm32(disp);
    }

    /// `add dst, imm32`
    pub(crate) fn add_imm32(&mut self, dst: u8, value: i32) {
        self.rex(true, 0, dst);
        self.byte(0x81);
        self.modrm(3, 0, dst);
        self.imm32(value);
    }

    /// `and dst, imm32`
    pub(crate) fn and_imm32(&mut self, dst: u8, value: i32) {
        self.rex(true, 0, dst);
        self.byte(0x81);
        self.modrm(3, 4, dst);
        self.imm32(value);
    }

    /// `sub dst, imm32`
    pub(crate) fn sub_imm32(&mut self, dst: u8, value: i32) {
        self.rex(true, 0, dst);
        self.byte(0x81);
        self.modrm(3, 5, dst);
        self.imm32(value);
    }

    /// `cmp dst, imm8`
    pub(crate) fn cmp_imm8(&mut self, dst: u8, value: i8) {
        self.rex(true, 0, dst);
        self.byte(0x83);
        self.modrm(3, 7, dst);
        self.byte(value as u8);
    }

    /// `inc dst`
    pub(crate) fn inc(&mut self, dst: u8) {
        self.rex(true, 0, dst);
        self.byte(0xFF);
        self.modrm(3, 0, dst);
    }

    /// `shl dst, imm8`
    pub(crate) fn shl_imm(&mut self, dst: u8, value: u8) {
        self.rex(true, 0, dst);
        self.byte(0xC1);
        self.modrm(3, 4, dst);
        self.byte(value);
    }

    /// `shr dst, imm8`
    pub(crate) fn shr_imm(&mut self, dst: u8, value: u8) {
        self.rex(true, 0, dst);
        self.byte(0xC1);
        self.modrm(3, 5, dst);
        self.byte(value);
    }

    /// `shl dst, cl`
    pub(crate) fn shl_cl(&mut self, dst: u8) {
        self.rex(true, 0, dst);
        self.byte(0xD3);
        self.modrm(3, 4, dst);
    }

    /// `shr dst, cl`
    pub(crate) fn shr_cl(&mut self, dst: u8) {
        self.rex(true, 0, dst);
        self.byte(0xD3);
        self.modrm(3, 5, dst);
    }

    /// `movq xmm, reg` -- the low 64 bits of a general register move into an
    /// SSE register, which is how a float travels in a frame slot.
    pub(crate) fn movq_xmm_gpr(&mut self, xmm: u8, reg: u8) {
        self.byte(0x66);
        self.rex(true, xmm, reg);
        self.byte(0x0F);
        self.byte(0x6E);
        self.modrm(3, xmm, reg);
    }

    /// `movq reg, xmm`
    pub(crate) fn movq_gpr_xmm(&mut self, reg: u8, xmm: u8) {
        self.byte(0x66);
        self.rex(true, xmm, reg);
        self.byte(0x0F);
        self.byte(0x7E);
        self.modrm(3, xmm, reg);
    }

    fn sse_arith(&mut self, opcode: u8, dst: u8, src: u8) {
        self.byte(0xF2);
        self.rex(false, dst, src);
        self.byte(0x0F);
        self.byte(opcode);
        self.modrm(3, dst, src);
    }

    /// `addsd dst, src`
    pub(crate) fn addsd(&mut self, dst: u8, src: u8) {
        self.sse_arith(0x58, dst, src);
    }

    /// `subsd dst, src`
    pub(crate) fn subsd(&mut self, dst: u8, src: u8) {
        self.sse_arith(0x5C, dst, src);
    }

    /// `mulsd dst, src`
    pub(crate) fn mulsd(&mut self, dst: u8, src: u8) {
        self.sse_arith(0x59, dst, src);
    }

    /// `divsd dst, src`
    pub(crate) fn divsd(&mut self, dst: u8, src: u8) {
        self.sse_arith(0x5E, dst, src);
    }

    /// `xorpd dst, src` -- a register against itself clears it.
    pub(crate) fn xorpd(&mut self, dst: u8, src: u8) {
        self.byte(0x66);
        self.rex(false, dst, src);
        self.byte(0x0F);
        self.byte(0x57);
        self.modrm(3, dst, src);
    }

    /// `ucomisd lhs, rhs` -- sets ZF/CF/PF the way an unsigned compare would.
    pub(crate) fn ucomisd(&mut self, lhs: u8, rhs: u8) {
        self.byte(0x66);
        self.rex(false, lhs, rhs);
        self.byte(0x0F);
        self.byte(0x2E);
        self.modrm(3, lhs, rhs);
    }

    /// `cvtsi2sd xmm, reg` -- a signed 64-bit integer as a double.
    pub(crate) fn cvtsi2sd(&mut self, xmm: u8, reg: u8) {
        self.byte(0xF2);
        self.rex(true, xmm, reg);
        self.byte(0x0F);
        self.byte(0x2A);
        self.modrm(3, xmm, reg);
    }

    pub(crate) fn push(&mut self, reg: u8) {
        if reg & 0x08 != 0 {
            self.byte(0x41);
        }
        self.byte(0x50 + (reg & 7));
    }

    pub(crate) fn pop(&mut self, reg: u8) {
        if reg & 0x08 != 0 {
            self.byte(0x41);
        }
        self.byte(0x58 + (reg & 7));
    }

    /// `mov rbp, rsp`
    pub(crate) fn mov_rbp_rsp(&mut self) {
        self.byte(0x48);
        self.byte(0x89);
        self.byte(0xE5);
    }

    /// `leave`
    pub(crate) fn leave(&mut self) {
        self.byte(0xC9);
    }

    pub(crate) fn ret(&mut self) {
        self.byte(0xC3);
    }

    /// `sub rsp, imm32`
    pub(crate) fn sub_rsp(&mut self, amount: u32) {
        self.byte(0x48);
        self.byte(0x81);
        self.byte(0xEC);
        self.imm32(amount as i32);
    }

    /// `add rsp, imm32`
    pub(crate) fn add_rsp(&mut self, amount: u32) {
        self.bytes.extend_from_slice(&[0x48, 0x81, 0xC4]);
        self.imm32(amount as i32);
    }

    /// `call rax`
    pub(crate) fn call_rax(&mut self) {
        self.byte(0xFF);
        self.byte(0xD0);
    }

    pub(crate) fn syscall(&mut self) {
        self.byte(0x0F);
        self.byte(0x05);
    }

    /// `dec reg`
    pub(crate) fn dec(&mut self, reg: u8) {
        self.rex(true, 0, reg);
        self.byte(0xFF);
        self.modrm(3, 1, reg);
    }

    /// `mov qword [rsp + disp8], 0`
    pub(crate) fn store_zero_rsp(&mut self, disp: u8) {
        self.byte(0x48);
        self.byte(0xC7);
        self.modrm(1, 0, 4);
        self.byte(0x24);
        self.byte(disp);
        self.imm32(0);
    }

    /// `ud2` -- what a ruturned-from `exit` would hit.
    pub(crate) fn ud2(&mut self) {
        self.byte(0x0F);
        self.byte(0x0B);
    }

    /// Declare a label at the current position.
    pub(crate) fn label(&mut self, name: &str) {
        self.labels.push((name.to_string(), self.position()));
    }

    /// `jmp label`
    pub(crate) fn jmp(&mut self, name: &str) {
        self.byte(0xE9);
        self.branches.push((self.position(), name.to_string()));
        self.imm32(0);
    }

    /// `jcc label`
    pub(crate) fn jcc(&mut self, cc: u8, name: &str) {
        self.byte(0x0F);
        self.byte(0x80 + cc);
        self.branches.push((self.position(), name.to_string()));
        self.imm32(0);
    }

    /// `call label`
    pub(crate) fn call(&mut self, name: &str) {
        self.byte(0xE8);
        self.branches.push((self.position(), name.to_string()));
        self.imm32(0);
    }

    /// Record that the `imm64` at `position` is the address of the data at
    /// `offset`, which only the image writer can compute.
    pub(crate) fn data_address(&mut self, position: usize, offset: u32) {
        self.fixups.push((position, offset));
    }

    /// The last `mov reg, imm64` was `mov` at this offset, so callers can
    /// record a data fixup without recomputing the position.
    pub(crate) fn mov_data_address(&mut self, reg: u8, offset: u32) {
        let immediate = self.position() + 2;
        self.mov_imm64(reg, 0);
        self.data_address(immediate, offset);
    }

    /// Resolve every label and hand back the code plus the data fixups as
    /// `(position of the immediate, data offset)`.
    pub(crate) fn finish(mut self) -> Result<(Vec<u8>, Vec<(usize, u32)>), String> {
        for (position, name) in &self.branches {
            let Some((_, target)) = self.labels.iter().find(|(label, _)| label == name) else {
                return Err(format!(
                    "internal: label \"{}\" is used but never defined",
                    name
                ));
            };
            let next = position + 4;
            let rel = *target as i64 - next as i64;
            let rel = i32::try_from(rel)
                .map_err(|_| format!("internal: jump to \"{}\" is out of range", name))?;
            self.bytes[*position..*position + 4].copy_from_slice(&rel.to_le_bytes());
        }
        Ok((self.bytes, self.fixups))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mov_imm64_rax_is_forty_eight_b8() {
        let mut code = Code::new();
        code.mov_imm64(RAX, 0x1122_3344_5566_7788);
        let (bytes, _) = code.finish().expect("labels");
        assert_eq!(
            bytes,
            vec![0x48, 0xB8, 0x88, 0x77, 0x66, 0x55, 0x44, 0x33, 0x22, 0x11]
        );
    }

    #[test]
    fn moving_into_an_extended_register_sets_rex_r() {
        let mut code = Code::new();
        code.mov_rr(R11, RAX);
        let (bytes, _) = code.finish().expect("labels");
        assert_eq!(bytes, vec![0x49, 0x89, 0xC3]);
    }

    #[test]
    fn a_load_from_a_slot_is_rbp_relative() {
        let mut code = Code::new();
        code.load(RAX, -8);
        let (bytes, _) = code.finish().expect("labels");
        assert_eq!(bytes, vec![0x48, 0x8B, 0x85, 0xF8, 0xFF, 0xFF, 0xFF]);
    }

    #[test]
    fn a_label_fixes_up_a_relative_jump() {
        let mut code = Code::new();
        code.jmp("later");
        code.label("later");
        code.ret();
        let (bytes, _) = code.finish().expect("labels");
        // `E9` then `rel32`; the label is the `ret` right after the jump, and
        // `rel32` counts from the end of the instruction, so it is zero.
        assert_eq!(bytes, vec![0xE9, 0x00, 0x00, 0x00, 0x00, 0xC3]);
    }

    #[test]
    fn an_undefined_label_is_an_error() {
        let mut code = Code::new();
        code.call("nowhere");
        assert!(code.finish().is_err());
    }

    #[test]
    fn a_conditional_jump_uses_the_two_byte_opcode() {
        let mut code = Code::new();
        code.jcc(CC_E, "end");
        code.label("end");
        code.ret();
        let (bytes, _) = code.finish().expect("labels");
        assert_eq!(&bytes[0..2], &[0x0F, 0x84]);
    }
}
