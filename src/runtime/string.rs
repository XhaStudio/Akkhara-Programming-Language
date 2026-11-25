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

/// `rt_trim_str`: the interpreter's `str::trim` -- the text without the
/// Unicode whitespace at either end. `rax` holds the string's address; the
/// answer is a fresh string in the arena, or the same string when nothing was
/// trimmed (strings are immutable here, so sharing one is exact).
///
/// The ends are walked as UTF-8: forwards one scalar at a time, and backwards
/// from the last byte to the start of the scalar that ends the text, so a
/// Myanmar syllable (several scalar values) is only ever dropped whole. A
/// scalar is whitespace when its bytes match one of the sequences
/// [`emit_ws_len`] compares against, which are generated from the host's
/// `char::is_whitespace` -- the same table `str::trim` uses, so no script can
/// disagree.
pub(crate) fn emit_trim(code: &mut Code) {
    code.label("rt_trim_str");
    code.push(RBP);
    code.mov_rbp_rsp();
    code.sub_rsp(64);
    // -8 descriptor, -16 first byte, -24 original length, -32 lo, -40 hi,
    // -48 the new length while the copy runs, -56 the scalar the backward
    // walk is testing (`rt_ws_len` clobbers `r8`).
    code.store(-8, RAX);
    code.load_ptr(RCX, RAX);
    code.store(-24, RCX);
    code.lea_disp8(RSI, RAX, 8);
    code.store(-16, RSI);
    code.mov_rr(RDI, RSI);
    code.add_rr(RDI, RCX);
    code.store(-32, RSI);
    code.store(-40, RDI);

    // Forwards: drop one whitespace scalar at a time from the front.
    code.label("trim_fwd");
    code.load(RSI, -32);
    code.load(RDI, -40);
    code.cmp_rr(RSI, RDI);
    code.jcc(CC_AE, "trim_bwd");
    code.call("rt_ws_len");
    code.test_rr(RAX, RAX);
    code.jcc(CC_E, "trim_bwd");
    code.load(RSI, -32);
    code.add_rr(RSI, RAX);
    code.store(-32, RSI);
    code.jmp("trim_fwd");

    // Backwards: find the start of the scalar that ends the text -- a byte
    // below `0xC0` is not a continuation byte, so it starts one -- then drop
    // it when it is whitespace.
    code.label("trim_bwd");
    code.load(RSI, -32);
    code.load(RDI, -40);
    code.cmp_rr(RDI, RSI);
    code.jcc(CC_BE, "trim_build");
    code.mov_imm32_zero(RCX, 0);
    code.mov_rr(R8, RDI);
    code.dec(R8);
    code.label("trim_bwd_scan");
    code.cmp_rr(R8, RSI);
    code.jcc(CC_BE, "trim_bwd_try");
    code.load_byte_index(R9, R8, RCX);
    code.mov_imm32(R10, 0x80);
    code.cmp_rr(R9, R10);
    code.jcc(CC_B, "trim_bwd_try");
    code.mov_imm32(R10, 0xC0);
    code.cmp_rr(R9, R10);
    code.jcc(CC_AE, "trim_bwd_try");
    code.dec(R8);
    code.jmp("trim_bwd_scan");
    code.label("trim_bwd_try");
    code.store(-56, R8);
    code.mov_rr(RSI, R8);
    code.call("rt_ws_len");
    code.test_rr(RAX, RAX);
    code.jcc(CC_E, "trim_build");
    // The scalar has to end exactly at the old hi, or it is not the last one.
    code.load(R8, -56);
    code.load(RDI, -40);
    code.mov_rr(RCX, R8);
    code.add_rr(RCX, RAX);
    code.cmp_rr(RCX, RDI);
    code.jcc(CC_NE, "trim_build");
    code.mov_rr(RDI, R8);
    code.store(-40, RDI);
    code.jmp("trim_bwd");

    // Nothing trimmed from either end: the same string is the answer.
    code.label("trim_build");
    code.load(RSI, -32);
    code.load(RDI, -40);
    code.mov_rr(RDX, RDI);
    code.sub_rr(RDX, RSI);
    code.load(RCX, -24);
    code.cmp_rr(RDX, RCX);
    code.jcc(CC_E, "trim_original");
    code.store(-48, RDX);
    code.mov_rr(RDI, RDX);
    code.add_imm8(RDI, 8);
    code.call("rt_alloc");
    code.store(-8, RAX);
    code.load(RDX, -48);
    code.store_ptr_disp32(RAX, 0, RDX);
    code.load(RSI, -32);
    code.mov_rr(RDI, RAX);
    code.add_imm8(RDI, 8);
    code.load(RCX, -48);
    code.rep_movsb();
    code.load(RAX, -8);
    code.leave();
    code.ret();

    code.label("trim_original");
    code.load(RAX, -8);
    code.leave();
    code.ret();

    emit_ws_len(code);
}

/// `rt_ws_len`: `rsi` points at a UTF-8 scalar inside a string, `rdi` is the
/// end of the text. `rax` is that scalar's byte length when it is Unicode
/// whitespace and `0` otherwise.
///
/// The set is written out as byte comparisons generated from the host's
/// `char::is_whitespace`, so it is the interpreter's `str::trim` table exactly
/// -- about two dozen scalars, from the ASCII space to U+3000 -- and the
/// routine needs no Unicode data of its own. A sequence that would run past
/// `rdi` is not a candidate, so the walk never reads outside the text.
fn emit_ws_len(code: &mut Code) {
    let whitespace: Vec<char> = (0u32..=0x10FFFF)
        .filter_map(char::from_u32)
        .filter(|c| c.is_whitespace())
        .collect();

    code.label("rt_ws_len");
    for (index, ch) in whitespace.iter().enumerate() {
        let mut buf = [0u8; 4];
        let bytes = ch.encode_utf8(&mut buf).as_bytes().to_vec();
        let next = format!("ws_next_{index}");
        // The whole sequence has to fit between the scalar and the end.
        code.lea_disp32(RAX, RSI, bytes.len() as i32);
        code.cmp_rr(RAX, RDI);
        code.jcc(CC_A, &next);
        for (offset, byte) in bytes.iter().enumerate() {
            code.mov_imm32_zero(RDX, offset as u32);
            code.load_byte_index(R8, RSI, RDX);
            code.mov_imm32(R9, *byte as i32);
            code.cmp_rr(R8, R9);
            code.jcc(CC_NE, &next);
        }
        code.mov_imm32_zero(RAX, bytes.len() as u32);
        code.ret();
        code.label(&next);
    }
    code.mov_imm32_zero(RAX, 0);
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
