//! `rt_str_int` and `rt_str_bool`: a value's display text as a fresh string
//! descriptor, for `str(x)` and for a `{x}` placeholder.
//!
//! An int is written in decimal into a stack buffer and then copied into the
//! collection arena as `{ length: u64, bytes... }`; a bool answers one of the
//! two interned descriptors the printer already uses, so `str(true)` and
//! `print(true)` cannot disagree. `rt_str_float` lives in `float.rs`, where the
//! formatter is: it renders into the same buffer a `print` would write and
//! copies it into a string instead.

use crate::backend::x86_64::encode::*;
use crate::backend::x86_64::{Code, Data};

pub(crate) fn emit(code: &mut Code, data: &mut Data) {
    let yes = data.intern("True");
    let no = data.intern("False");

    code.label("rt_str_bool");
    code.test_rr(RAX, RAX);
    code.jcc(CC_E, "str_bool_false");
    code.mov_data_address(RAX, yes);
    code.ret();
    code.label("str_bool_false");
    code.mov_data_address(RAX, no);
    code.ret();

    // The digits are built backwards from `rbp - 31`, just as the int printer
    // does; slots at -8, -16 and -24 sit above them.
    code.label("rt_str_int");
    code.push(RBP);
    code.mov_rbp_rsp();
    code.sub_rsp(128);
    code.lea_disp8(RSI, RBP, -31);
    code.mov_rr(R10, RAX);
    code.mov_imm32_zero(R11, 0);
    code.test_rr(R10, R10);
    code.jcc(CC_NS, "str_int_abs");
    code.neg(R10);
    code.mov_imm32_zero(R11, 1);
    code.label("str_int_abs");
    code.mov_rr(RAX, R10);
    code.mov_imm32(RCX, 10);
    code.label("str_int_loop");
    code.xor_rr(RDX, RDX);
    code.div(RCX);
    code.add_imm8(RDX, b'0' as i8);
    code.store_byte(RSI, RDX);
    code.dec(RSI);
    code.test_rr(RAX, RAX);
    code.jcc(CC_NE, "str_int_loop");
    code.test_rr(R11, R11);
    code.jcc(CC_E, "str_int_digits");
    code.store_byte_imm(RSI, b'-');
    code.dec(RSI);
    code.label("str_int_digits");
    // `rsi` is one byte before the first one, so the text starts at `rsi + 1`
    // and its length is `rbp - 30 - start`.
    code.lea_disp8(RAX, RSI, 1);
    code.store(-16, RAX);
    code.mov_rr(RCX, RBP);
    code.sub_rr(RCX, RAX);
    code.sub_imm8(RCX, 30);
    code.store(-8, RCX);
    code.mov_rr(RDI, RCX);
    code.add_imm8(RDI, 8);
    code.call("rt_alloc");
    code.store(-24, RAX);
    code.load(RCX, -8);
    code.store_ptr_disp32(RAX, 0, RCX);
    code.mov_rr(RDI, RAX);
    code.add_imm8(RDI, 8);
    code.load(RSI, -16);
    code.rep_movsb();
    code.load(RAX, -24);
    code.leave();
    code.ret();
}
