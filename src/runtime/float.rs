//! `rt_print_float`: the interpreter's float formatting, in machine code.
//!
//! `interp` prints a float with `{:.1}` when it is a whole number and with
//! `{}` -- the shortest decimal that reads back as the same double -- otherwise,
//! always in positional notation and always followed by the print newline.
//! Reproducing that exactly needs the digits of the value rather than an
//! approximation of them, so this routine works in exact decimal integers.
//!
//! A `f64` is `m * 2^e`:
//!
//! * `e >= 0` makes it a whole number, and `m * 2^e` is computed exactly by
//!   doubling a decimal bignum `e` times;
//! * `e < 0` gives `m * 5^-e * 10^e`, so multiplying by 5 exactly as many
//!   times produces the exact decimal expansion with a known scale.
//!
//! The shortest form is found the way its definition says: take the exact
//! expansion and, for `p` from 1 to 17, round it to `p` significant digits and
//! ask whether that decimal still lies inside the interval of reals that round
//! to this double -- between `(2m-1) * 2^(e-1)` and `(2m+1) * 2^(e-1)`, whose
//! expansions are built the same way. The first `p` that does is the shortest
//! representation, and `p` never has to pass 17.
//!
//! A decimal here is a little-endian byte array of digits 0-9 (one digit per
//! byte) plus a length and a scale, so no division or digit packing is needed
//! anywhere: multiplying by 2 or 5 is one pass with a carry, and comparing two
//! decimals is a compare of `length + scale` (the decimal-point position)
//! followed by a digit-by-digit compare with zero padding.

use crate::backend::x86_64::encode::*;

/// Scratch slots and buffers. The buffers hold 800 digits, which covers the
/// 767-digit exact expansion of the smallest subnormal.
const S_BITS: i32 = -8;
const S_SIGN: i32 = -16;
const S_EXP: i32 = -24;
const S_M: i32 = -32;
const S_E: i32 = -40;
const S_S: i32 = -48;
const S_LENV: i32 = -56;
const S_SCALEV: i32 = -64;
const S_LENLO: i32 = -72;
const S_SCALELO: i32 = -80;
const S_LENHI: i32 = -88;
const S_SCALEHI: i32 = -96;
const S_P: i32 = -104;
const S_CLEN: i32 = -112;
const S_CSCALE: i32 = -120;
const S_INCL: i32 = -128;
const S_T1: i32 = -136;
const S_T2: i32 = -144;
const S_T3: i32 = -152;
const S_T4: i32 = -184;
/// Which buffer holds the digits to render: 0 is the exact expansion, 1 the
/// rounded candidate.
const S_WHICH: i32 = -192;
const S_STRIP: i32 = -200;
const S_INTEGRAL: i32 = -208;

const DIG_V: i32 = -1000;
const DIG_LO: i32 = -1800;
const DIG_HI: i32 = -2600;
const CAND: i32 = -3400;
const OUT: i32 = -4300;
const FRAME: u32 = 4320;

/// Append the byte in `al` to the output buffer: `rdi` is its base, `r9` the
/// write index.
fn append_out(code: &mut Code) {
    code.store_byte_index(RDI, R9, RAX);
    code.inc(R9);
}

/// Append the digit at `[rsi + r11]`, as the byte that prints it.
fn append_digit(code: &mut Code) {
    code.load_byte_index(RAX, RSI, R11);
    code.add_imm32(RAX, b'0' as i32);
    append_out(code);
}

/// `pf_digits`: the decimal digits of `rax` go to `[rdi]`, then the digits are
/// multiplied by `rsi` (2 or 5) `r11` times. The length comes back in `rax`.
fn emit_digits(code: &mut Code) {
    code.label("pf_digits");
    code.mov_imm32_zero(R8, 0);
    code.label("pf_d_set");
    code.xor_rr(RDX, RDX);
    code.mov_imm32_zero(RCX, 10);
    code.div(RCX);
    code.store_byte_index(RDI, R8, RDX);
    code.inc(R8);
    code.test_rr(RAX, RAX);
    code.jcc(CC_NE, "pf_d_set");

    code.label("pf_d_mul");
    code.test_rr(R11, R11);
    code.jcc(CC_E, "pf_d_done");
    code.mov_imm32_zero(R9, 0);
    code.mov_imm32_zero(RCX, 0);
    code.label("pf_d_each");
    code.cmp_rr(RCX, R8);
    code.jcc(CC_AE, "pf_d_tail");
    code.load_byte_index(RDX, RDI, RCX);
    code.imul_rr(RDX, RSI);
    code.add_rr(RDX, R9);
    code.mov_imm32_zero(R9, 0);
    code.label("pf_d_carry");
    code.cmp_imm8(RDX, 10);
    code.jcc(CC_B, "pf_d_store");
    code.sub_imm32(RDX, 10);
    code.inc(R9);
    code.jmp("pf_d_carry");
    code.label("pf_d_store");
    code.store_byte_index(RDI, RCX, RDX);
    code.inc(RCX);
    code.jmp("pf_d_each");
    code.label("pf_d_tail");
    code.test_rr(R9, R9);
    code.jcc(CC_E, "pf_d_next");
    code.store_byte_index(RDI, R8, R9);
    code.inc(R8);
    code.label("pf_d_next");
    code.dec(R11);
    code.jmp("pf_d_mul");
    code.label("pf_d_done");
    code.mov_rr(RAX, R8);
    code.ret();
}

/// `pf_cmp`: compare two decimals. `rdi`/`rsi`/`rdx` are the first one's
/// digits, length and scale; `rcx`/`r8`/`r9` the second's. Leaves -1, 0 or 1
/// in `rax`.
fn emit_compare(code: &mut Code) {
    code.label("pf_cmp");
    code.mov_rr(RAX, RSI);
    code.add_rr(RAX, RDX);
    code.store(S_T1, RAX);
    code.mov_rr(RAX, R8);
    code.add_rr(RAX, R9);
    code.load(R10, S_T1);
    code.cmp_rr(R10, RAX);
    code.jcc(CC_L, "pf_cmp_less");
    code.jcc(CC_G, "pf_cmp_greater");
    // The same decimal-point position: compare digits from the top, treating
    // the shorter number as if it were padded with zeros.
    // Walk the decimal places from the most significant down to the last
    // place either number has, reading a zero where a number has no digit.
    // Digit `i` of a decimal sits at place `i + scale`.
    code.mov_rr(RAX, RDX);
    code.cmp_rr(RAX, R9);
    code.jcc(CC_LE, "pf_cmp_base_ready");
    code.mov_rr(RAX, R9);
    code.label("pf_cmp_base_ready");
    code.store(S_T2, RAX);
    code.load(RAX, S_T1);
    code.dec(RAX);
    code.store(S_T3, RAX);
    code.label("pf_cmp_loop");
    code.load(R10, S_T3);
    code.load(RAX, S_T2);
    code.cmp_rr(R10, RAX);
    code.jcc(CC_L, "pf_cmp_equal");
    // a's digit at this place
    code.mov_rr(RAX, R10);
    code.sub_rr(RAX, RDX);
    code.mov_imm32_zero(R11, 0);
    code.cmp_imm8(RAX, 0);
    code.jcc(CC_L, "pf_cmp_a_done");
    code.cmp_rr(RAX, RSI);
    code.jcc(CC_AE, "pf_cmp_a_done");
    code.load_byte_index(R11, RDI, RAX);
    code.label("pf_cmp_a_done");
    code.store(S_T4, R11);
    // b's digit at this place
    code.mov_rr(RAX, R10);
    code.sub_rr(RAX, R9);
    code.mov_imm32_zero(R11, 0);
    code.cmp_imm8(RAX, 0);
    code.jcc(CC_L, "pf_cmp_b_done");
    code.cmp_rr(RAX, R8);
    code.jcc(CC_AE, "pf_cmp_b_done");
    code.load_byte_index(R11, RCX, RAX);
    code.label("pf_cmp_b_done");
    code.load(RAX, S_T4);
    code.cmp_rr(RAX, R11);
    code.jcc(CC_B, "pf_cmp_less");
    code.jcc(CC_A, "pf_cmp_greater");
    code.dec(R10);
    code.store(S_T3, R10);
    code.jmp("pf_cmp_loop");
    code.label("pf_cmp_equal");
    code.mov_imm32_zero(RAX, 0);
    code.ret();
    code.label("pf_cmp_less");
    code.mov_imm64(RAX, u64::MAX);
    code.ret();
    code.label("pf_cmp_greater");
    code.mov_imm32_zero(RAX, 1);
    code.ret();
}

/// Copy `rcx` digits from `[rbp + from + r10]` to `[rbp + to]`.
fn emit_copy(code: &mut Code, from: i32, to: i32) {
    code.lea_disp32(RSI, RBP, from);
    code.lea_disp32(RDI, RBP, to);
    code.add_rr(RSI, R10);
    code.mov_imm32_zero(R11, 0);
    code.label("pf_copy_loop");
    code.cmp_rr(R11, RCX);
    code.jcc(CC_AE, "pf_copy_done");
    code.load_byte_index(RAX, RSI, R11);
    code.store_byte_index(RDI, R11, RAX);
    code.inc(R11);
    code.jmp("pf_copy_loop");
    code.label("pf_copy_done");
}

pub(crate) fn emit(code: &mut Code) {
    code.label("rt_write_float");
    code.push(RBP);
    code.mov_rbp_rsp();
    code.sub_rsp(FRAME);

    // sign, exponent field and mantissa
    code.store(S_BITS, RAX);
    code.mov_rr(RCX, RAX);
    code.shr_imm(RCX, 63);
    code.store(S_SIGN, RCX);
    code.mov_rr(RCX, RAX);
    code.shr_imm(RCX, 52);
    code.and_imm32(RCX, 0x7ff);
    code.store(S_EXP, RCX);
    code.mov_rr(RCX, RAX);
    code.shl_imm(RCX, 12);
    code.shr_imm(RCX, 12);
    code.store(S_M, RCX);

    // inf and NaN have no digits.
    code.load(R8, S_EXP);
    code.mov_imm32(RCX, 0x7ff);
    code.cmp_rr(R8, RCX);
    code.jcc(CC_E, "pf_special");
    code.load(R8, S_BITS);
    code.shl_imm(R8, 1);
    code.test_rr(R8, R8);
    code.jcc(CC_E, "pf_zero");

    // the value is m * 2^e
    code.load(R8, S_M);
    code.load(R9, S_EXP);
    code.test_rr(R9, R9);
    code.jcc(CC_E, "pf_subnormal");
    code.mov_imm64(RCX, 1 << 52);
    code.or_rr(R8, RCX);
    code.sub_imm32(R9, 1075);
    code.jmp("pf_have");
    code.label("pf_subnormal");
    code.mov_imm64(R9, (-1074i64) as u64);
    code.label("pf_have");
    code.store(S_M, R8);
    code.store(S_E, R9);

    // A whole number has every fractional bit clear.
    code.test_rr(R9, R9);
    code.jcc(CC_NS, "pf_whole_shift");
    code.mov_rr(RCX, R9);
    code.neg(RCX);
    code.store(S_S, RCX);
    code.cmp_imm8(RCX, 53);
    code.jcc(CC_AE, "pf_fractional");
    code.mov_imm64(R10, 1);
    code.shl_cl(R10);
    code.dec(R10);
    code.mov_rr(R11, R8);
    code.and_rr(R11, R10);
    code.test_rr(R11, R11);
    code.jcc(CC_NE, "pf_fractional");

    // Whole with a negative exponent: shift the mantissa down.
    code.mov_rr(RAX, R8);
    code.mov_rr(RCX, R9);
    code.neg(RCX);
    code.shr_cl(RAX);
    code.lea_disp32(RDI, RBP, DIG_V);
    code.mov_imm32_zero(RSI, 2);
    code.mov_imm32_zero(R11, 0);
    code.call("pf_digits");
    code.store(S_LENV, RAX);
    code.jmp("pf_whole_ready");

    // Whole with a non-negative exponent: double the mantissa e times.
    code.label("pf_whole_shift");
    code.mov_rr(RAX, R8);
    code.lea_disp32(RDI, RBP, DIG_V);
    code.mov_imm32_zero(RSI, 2);
    code.mov_rr(R11, R9);
    code.call("pf_digits");
    code.store(S_LENV, RAX);

    code.label("pf_whole_ready");
    code.mov_imm32_zero(RAX, 0);
    code.store(S_WHICH, RAX);
    code.store(S_STRIP, RAX);
    code.load(RAX, S_LENV);
    code.store(S_T2, RAX);
    code.mov_imm32_zero(RAX, 0);
    code.store(S_T3, RAX);
    code.mov_imm32_zero(RAX, 1);
    code.store(S_INTEGRAL, RAX);
    code.jmp("pf_render");

    // The exact expansions of the value and of the two boundaries.
    code.label("pf_fractional");
    code.load(RAX, S_M);
    code.lea_disp32(RDI, RBP, DIG_V);
    code.mov_imm32_zero(RSI, 5);
    code.load(R11, S_S);
    code.call("pf_digits");
    code.store(S_LENV, RAX);
    code.load(RAX, S_E);
    code.store(S_SCALEV, RAX);

    code.load(RAX, S_M);
    code.add_rr(RAX, RAX);
    code.inc(RAX);
    code.lea_disp32(RDI, RBP, DIG_HI);
    code.mov_imm32_zero(RSI, 5);
    code.load(R11, S_S);
    code.inc(R11);
    code.call("pf_digits");
    code.store(S_LENHI, RAX);
    code.load(RAX, S_E);
    code.dec(RAX);
    code.store(S_SCALEHI, RAX);

    code.load(RAX, S_M);
    code.add_rr(RAX, RAX);
    code.dec(RAX);
    code.lea_disp32(RDI, RBP, DIG_LO);
    code.mov_imm32_zero(RSI, 5);
    code.load(R11, S_S);
    code.inc(R11);
    code.call("pf_digits");
    code.store(S_LENLO, RAX);
    code.load(RAX, S_E);
    code.dec(RAX);
    code.store(S_SCALELO, RAX);

    // A value with an even mantissa owns its rounding boundaries.
    code.load(RAX, S_M);
    code.and_imm32(RAX, 1);
    code.store(S_INCL, RAX);

    code.mov_imm32_zero(RAX, 1);
    code.store(S_P, RAX);
    code.label("pf_p_loop");
    code.load(RCX, S_P);
    code.load(RDX, S_LENV);
    code.cmp_rr(RCX, RDX);
    code.jcc(CC_AE, "pf_accept_exact");

    // The candidate: the top p digits of the exact expansion.
    code.mov_rr(R10, RDX);
    code.sub_rr(R10, RCX);
    emit_copy(code, DIG_V, CAND);
    code.store(S_CLEN, RCX);
    code.load(RAX, S_SCALEV);
    code.load(R11, S_P);
    code.load(RDX, S_LENV);
    code.sub_rr(RDX, R11);
    code.add_rr(RAX, RDX);
    code.store(S_CSCALE, RAX);

    // Does the next digit round it up?
    code.lea_disp32(RSI, RBP, DIG_V);
    code.lea_disp32(RDI, RBP, CAND);
    code.load(R11, S_LENV);
    code.load(RCX, S_P);
    code.sub_rr(R11, RCX);
    code.dec(R11);
    code.load_byte_index(RAX, RSI, R11);
    code.cmp_imm8(RAX, 5);
    code.jcc(CC_A, "pf_round_up");
    code.jcc(CC_B, "pf_test_interval");
    // Exactly half: a remaining digit decides, otherwise round to even.
    code.load(RDX, S_LENV);
    code.load(RCX, S_P);
    code.sub_rr(RDX, RCX);
    code.mov_imm32_zero(R11, 0);
    code.label("pf_sticky_loop");
    code.cmp_rr(R11, RDX);
    code.jcc(CC_AE, "pf_tie");
    code.load_byte_index(RAX, RSI, R11);
    code.test_rr(RAX, RAX);
    code.jcc(CC_NE, "pf_round_up");
    code.inc(R11);
    code.jmp("pf_sticky_loop");
    code.label("pf_tie");
    code.mov_imm32_zero(R11, 0);
    code.load_byte_index(RAX, RDI, R11);
    code.and_imm32(RAX, 1);
    code.test_rr(RAX, RAX);
    code.jcc(CC_NE, "pf_round_up");
    code.jmp("pf_test_interval");

    code.label("pf_round_up");
    code.mov_imm32_zero(R11, 0);
    code.label("pf_inc_loop");
    code.load_byte_index(RAX, RDI, R11);
    code.inc(RAX);
    code.cmp_imm8(RAX, 10);
    code.jcc(CC_B, "pf_inc_done");
    code.mov_imm32_zero(RAX, 0);
    code.store_byte_index(RDI, R11, RAX);
    code.inc(R11);
    code.load(RCX, S_CLEN);
    code.cmp_rr(R11, RCX);
    code.jcc(CC_B, "pf_inc_loop");
    // Every digit carried: the candidate gained a digit.
    code.mov_imm32_zero(RAX, 1);
    code.store_byte_index(RDI, R11, RAX);
    code.inc(R11);
    code.store(S_CLEN, R11);
    code.jmp("pf_test_interval");
    code.label("pf_inc_done");
    code.store_byte_index(RDI, R11, RAX);

    // Inside (lo, hi), or on a boundary this value owns?
    code.label("pf_test_interval");
    code.lea_disp32(RDI, RBP, CAND);
    code.load(RSI, S_CLEN);
    code.load(RDX, S_CSCALE);
    code.lea_disp32(RCX, RBP, DIG_LO);
    code.load(R8, S_LENLO);
    code.load(R9, S_SCALELO);
    code.call("pf_cmp");
    code.call("pf_cmp");
    code.test_rr(RAX, RAX);
    code.jcc(CC_L, "pf_reject");
    code.jcc(CC_NE, "pf_check_hi");
    code.load(R10, S_INCL);
    code.test_rr(R10, R10);
    code.jcc(CC_E, "pf_reject");
    code.label("pf_check_hi");
    code.lea_disp32(RDI, RBP, CAND);
    code.load(RSI, S_CLEN);
    code.load(RDX, S_CSCALE);
    code.lea_disp32(RCX, RBP, DIG_HI);
    code.load(R8, S_LENHI);
    code.load(R9, S_SCALEHI);
    code.call("pf_cmp");
    code.test_rr(RAX, RAX);
    code.jcc(CC_G, "pf_reject");
    code.jcc(CC_NE, "pf_accept_candidate");
    code.load(R10, S_INCL);
    code.test_rr(R10, R10);
    code.jcc(CC_E, "pf_reject");
    code.jmp("pf_accept_candidate");

    code.label("pf_reject");
    code.load(RAX, S_P);
    code.inc(RAX);
    code.store(S_P, RAX);
    code.cmp_imm8(RAX, 17);
    code.jcc(CC_LE, "pf_p_loop");
    // 17 digits always read back as the same double, so this is unreachable.
    code.jmp("pf_accept_candidate");

    code.label("pf_accept_exact");
    code.mov_imm32_zero(RAX, 0);
    code.store(S_WHICH, RAX);
    code.load(RAX, S_LENV);
    code.store(S_T2, RAX);
    code.load(RAX, S_SCALEV);
    code.store(S_T3, RAX);
    code.jmp("pf_fractional_render");

    code.label("pf_accept_candidate");
    code.mov_imm32_zero(RAX, 1);
    code.store(S_WHICH, RAX);
    code.load(RAX, S_CLEN);
    code.store(S_T2, RAX);
    code.load(RAX, S_CSCALE);
    code.store(S_T3, RAX);

    code.label("pf_fractional_render");
    code.mov_imm32_zero(RAX, 0);
    code.store(S_INTEGRAL, RAX);
    code.mov_imm32_zero(RAX, 1);
    code.store(S_STRIP, RAX);

    // Render: `rsi` digits, `rcx` length, `r8` decimal-point position, with the
    // output built in `out` and written by one call.
    code.label("pf_render");
    code.load(R10, S_WHICH);
    code.test_rr(R10, R10);
    code.jcc(CC_NE, "pf_render_candidate");
    code.lea_disp32(RSI, RBP, DIG_V);
    code.jmp("pf_render_digits");
    code.label("pf_render_candidate");
    code.lea_disp32(RSI, RBP, CAND);
    code.label("pf_render_digits");
    code.load(RCX, S_T2);

    // A shortest form can end in zeros; they carry nothing, so drop them.
    code.load(R10, S_STRIP);
    code.test_rr(R10, R10);
    code.jcc(CC_E, "pf_render_k");
    code.label("pf_strip_loop");
    code.cmp_imm8(RCX, 1);
    code.jcc(CC_BE, "pf_render_k");
    code.mov_imm32_zero(R11, 0);
    code.load_byte_index(RAX, RSI, R11);
    code.test_rr(RAX, RAX);
    code.jcc(CC_NE, "pf_render_k");
    // Dropping a zero digit divides the integer by ten, so the scale grows to
    // keep the value (and the decimal point) where it was.
    code.inc(RSI);
    code.dec(RCX);
    code.load(RAX, S_T3);
    code.inc(RAX);
    code.store(S_T3, RAX);
    code.jmp("pf_strip_loop");

    code.label("pf_render_k");
    code.lea_disp32(RDI, RBP, OUT);
    code.mov_imm32_zero(R9, 0);
    code.load(R8, S_T3);
    code.add_rr(R8, RCX);
    code.store(S_T1, R8);

    // Sign first.
    code.load(R10, S_SIGN);
    code.test_rr(R10, R10);
    code.jcc(CC_E, "pf_render_body");
    code.mov_imm32_zero(RAX, b'-' as u32);
    append_out(code);

    code.label("pf_render_body");
    code.load(R8, S_T1);
    code.test_rr(R8, R8);
    code.jcc(CC_LE, "pf_render_small");
    code.cmp_rr(R8, RCX);
    code.jcc(CC_GE, "pf_render_large");

    // 0 < k < len: k digits, a point, then the rest.
    code.mov_rr(R10, RCX);
    code.sub_rr(R10, R8);
    code.mov_rr(R11, RCX);
    code.dec(R11);
    code.label("pf_render_head");
    append_digit(code);
    code.dec(R11);
    code.cmp_rr(R11, R10);
    code.jcc(CC_GE, "pf_render_head");
    code.mov_imm32_zero(RAX, b'.' as u32);
    append_out(code);
    code.label("pf_render_tail");
    append_digit(code);
    code.dec(R11);
    code.cmp_imm8(R11, 0);
    code.jcc(CC_GE, "pf_render_tail");
    code.jmp("pf_render_end");

    // k >= len: every digit, then the zeros that hold the value's magnitude.
    code.label("pf_render_large");
    code.mov_rr(R11, RCX);
    code.dec(R11);
    code.label("pf_render_all");
    append_digit(code);
    code.dec(R11);
    code.cmp_imm8(R11, 0);
    code.jcc(CC_GE, "pf_render_all");
    code.mov_rr(R10, R8);
    code.sub_rr(R10, RCX);
    code.label("pf_render_zeros");
    code.test_rr(R10, R10);
    code.jcc(CC_E, "pf_render_end");
    code.mov_imm32_zero(RAX, b'0' as u32);
    append_out(code);
    code.dec(R10);
    code.jmp("pf_render_zeros");

    // k <= 0: "0.", the zeros before the first digit, then every digit.
    code.label("pf_render_small");
    code.mov_imm32_zero(RAX, b'0' as u32);
    append_out(code);
    code.mov_imm32_zero(RAX, b'.' as u32);
    append_out(code);
    code.mov_rr(R10, R8);
    code.neg(R10);
    code.label("pf_render_leading");
    code.test_rr(R10, R10);
    code.jcc(CC_E, "pf_render_small_digits");
    code.mov_imm32_zero(RAX, b'0' as u32);
    append_out(code);
    code.dec(R10);
    code.jmp("pf_render_leading");
    code.label("pf_render_small_digits");
    code.mov_rr(R11, RCX);
    code.dec(R11);
    code.label("pf_render_small_loop");
    append_digit(code);
    code.dec(R11);
    code.cmp_imm8(R11, 0);
    code.jcc(CC_GE, "pf_render_small_loop");

    code.label("pf_render_end");
    // A whole number prints with one decimal place, as `{:.1}` does.
    code.load(R10, S_INTEGRAL);
    code.test_rr(R10, R10);
    code.jcc(CC_E, "pf_write");
    code.mov_imm32_zero(RAX, b'.' as u32);
    append_out(code);
    code.mov_imm32_zero(RAX, b'0' as u32);
    append_out(code);

    code.label("pf_write");
    code.mov_rr(RCX, R9);
    code.lea_disp32(RAX, RBP, OUT);
    code.call("rt_write");
    code.leave();
    code.ret();

    // `rt_write_float` is the body without the print newline, so a float
    // inside a collection is followed by `, ` rather than a line break.
    code.label("rt_print_float");
    code.push(RBP);
    code.mov_rbp_rsp();
    code.call("rt_write_float");
    code.call("rt_write_newline");
    code.leave();
    code.ret();

    // inf, NaN and zero are written straight out: no digits, no rounding.
    code.label("pf_special");
    code.load(R8, S_M);
    code.test_rr(R8, R8);
    code.jcc(CC_NE, "pf_nan");
    code.lea_disp32(RDI, RBP, OUT);
    code.mov_imm32_zero(R9, 0);
    code.load(R10, S_SIGN);
    code.test_rr(R10, R10);
    code.jcc(CC_E, "pf_inf_body");
    code.mov_imm32_zero(RAX, b'-' as u32);
    append_out(code);
    code.label("pf_inf_body");
    for ch in b"inf" {
        code.mov_imm32_zero(RAX, *ch as u32);
        append_out(code);
    }
    code.jmp("pf_write");

    code.label("pf_nan");
    code.lea_disp32(RDI, RBP, OUT);
    code.mov_imm32_zero(R9, 0);
    for ch in b"NaN" {
        code.mov_imm32_zero(RAX, *ch as u32);
        append_out(code);
    }
    code.jmp("pf_write");

    code.label("pf_zero");
    code.lea_disp32(RDI, RBP, OUT);
    code.mov_imm32_zero(R9, 0);
    code.load(R10, S_SIGN);
    code.test_rr(R10, R10);
    code.jcc(CC_E, "pf_zero_body");
    code.mov_imm32_zero(RAX, b'-' as u32);
    append_out(code);
    code.label("pf_zero_body");
    for ch in b"0.0" {
        code.mov_imm32_zero(RAX, *ch as u32);
        append_out(code);
    }
    code.jmp("pf_write");

    emit_digits(code);
    emit_compare(code);
}
