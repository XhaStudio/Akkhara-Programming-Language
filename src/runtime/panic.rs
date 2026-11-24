//! Arithmetic run-time failures the slice can still hit directly. A zero
//! divisor for `/` or `%` is a `guard_zero` in the generated code, which raises
//! the interpreter's `E036`-`E039` like a raise: catchable, with the operator's
//! line, and reported by `rt_report_error` when nothing catches it. Only a
//! compound remainder assignment (`x %= 0`) keeps its check here, because it has
//! no guard in front of it; that reports `E038` and exits 1. The collection
//! runtime's `E049`/`E053` panics live in `collections.rs`, next to the indexing
//! code that raises them.

use crate::backend::x86_64::{Code, Data, RAX, RBP, RCX};

/// The message the interpreter prints for a zero remainder divisor, without
/// the line number a compiled program has no span for.
const MESSAGE: &str = "E038 သုညဖြင့် ကြွင်းကိန်းရှာ၍မရပါ။";

pub(crate) fn emit_zero(code: &mut Code, data: &mut Data) {
    let message = data.intern(MESSAGE);
    let newline = data.intern("\n");
    code.label("rt_panic_zero");
    code.push(RBP);
    code.mov_rbp_rsp();
    code.mov_data_address(RAX, message + 8);
    code.mov_imm32(RCX, MESSAGE.len() as i32);
    code.call("rt_write_err");
    code.mov_data_address(RAX, newline + 8);
    code.mov_imm32(RCX, 1);
    code.call("rt_write_err");
    code.mov_imm32_zero(RAX, 1);
    code.call("rt_exit");
    code.ud2();
}
