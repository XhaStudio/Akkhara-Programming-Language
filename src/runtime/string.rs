//! `rt_cmp_str` and `rt_concat_str`: comparing and joining strings.
//!
//! A string value is the address of `{ length: u64, bytes... }`. Comparing
//! two of them is the interpreter's `a < b` on Rust `String`s: byte-wise over
//! the common prefix, and the shorter one is smaller when one is a proper
//! prefix of the other. UTF-8 keeps code-point order in its byte order, so a
//! byte compare is the same ordering the interpreter uses.
//!
//! `rax` is the first string's address and `rcx` the second's on entry; the
//! result is `-1`, `0` or `1` in `rax`. The routine calls nothing, so it does
//! not care about stack alignment.

use crate::backend::x86_64::encode::*;

/// `rt_concat_str`: `rax` is the first string's address and `rcx` the
/// second's; the result in `rax` is a fresh string holding both byte ranges in
/// order. The interpreter's `+` on two strings is exactly this join, so
/// `"foo" + "bar"` prints `foobar` and a Myanmar syllable pair keeps its bytes
/// intact.
///
/// The new string is `{ length, bytes... }` in the collection arena, so a long
/// loop of joins reaches the documented `E177` exhaustion rather than growing
/// without bound. Both ranges are copied with `rep movsb`, which is correct for
/// any length, including zero.
pub(crate) fn emit_cat(code: &mut Code) {
    code.label("rt_concat_str");
    code.push(RBP);
    code.mov_rbp_rsp();
    // Four slots, and `rsp` stays 16-byte aligned for the `rt_alloc` call.
    code.sub_rsp(32);
    code.store(-8, RAX);
    code.store(-16, RCX);
    // The byte length of each side, then the new header plus both ranges.
    code.load_ptr(RDX, RAX);
    code.load_ptr(R8, RCX);
    code.store(-24, RDX);
    code.mov_rr(RDI, RDX);
    code.add_rr(RDI, R8);
    code.add_imm8(RDI, 8);
    code.call("rt_alloc");
    code.store(-32, RAX);
    // `length = la + lb`.
    code.load(RDX, -24);
    code.load(RCX, -16);
    code.load_ptr(R8, RCX);
    code.add_rr(RDX, R8);
    code.store_ptr_disp32(RAX, 0, RDX);
    // First string: bytes at `a + 8` into `dst + 8`.
    code.load(RSI, -8);
    code.lea_disp8(RSI, RSI, 8);
    code.load(RDI, -32);
    code.lea_disp8(RDI, RDI, 8);
    code.load(RCX, -24);
    code.rep_movsb();
    // `rep movsb` left `rdi` just past the first range, where the second goes.
    code.load(RSI, -16);
    code.lea_disp8(RSI, RSI, 8);
    code.load(RCX, -16);
    code.load_ptr(RCX, RCX);
    code.rep_movsb();
    code.load(RAX, -32);
    code.leave();
    code.ret();
}

pub(crate) fn emit_cmp(code: &mut Code) {
    code.label("rt_cmp_str");

    // Lengths, then the byte pointers (the descriptor's header is 8 bytes).
    code.load_ptr(RSI, RAX);
    code.load_ptr(RDI, RCX);
    code.lea_disp8(RAX, RAX, 8);
    code.lea_disp8(RCX, RCX, 8);

    // n = min(len_a, len_b), so the loop never reads past either string.
    code.mov_rr(RDX, RSI);
    code.cmp_rr(RDX, RDI);
    code.jcc(CC_BE, "strcmp_min");
    code.mov_rr(RDX, RDI);
    code.label("strcmp_min");

    code.mov_imm32_zero(R8, 0);
    code.label("strcmp_loop");
    code.cmp_rr(R8, RDX);
    code.jcc(CC_AE, "strcmp_tail");
    code.load_byte_index(R9, RAX, R8);
    code.load_byte_index(R10, RCX, R8);
    code.cmp_rr(R9, R10);
    code.jcc(CC_B, "strcmp_less");
    code.jcc(CC_A, "strcmp_greater");
    code.inc(R8);
    code.jmp("strcmp_loop");

    // The common prefix is equal: the shorter string is the smaller one.
    code.label("strcmp_tail");
    code.cmp_rr(RSI, RDI);
    code.jcc(CC_B, "strcmp_less");
    code.jcc(CC_A, "strcmp_greater");
    code.mov_imm32_zero(RAX, 0);
    code.ret();

    code.label("strcmp_less");
    code.mov_imm32(RAX, -1);
    code.ret();

    code.label("strcmp_greater");
    code.mov_imm32(RAX, 1);
    code.ret();
}
