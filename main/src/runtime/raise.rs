//! The raise runtime: the routine that reports a raise nothing caught.
//!
//! A pending raise lives in three slots the image reserves for it: the text
//! before its message (`E178 လိုင်း N တွင် [name] `), its identity and the
//! message a catch clause binds. Both strings are ordinary descriptors,
//! `{ length: u64, bytes... }`, so writing one is the same `write` the printer
//! uses -- on stderr, and then `exit 1`, exactly what the interpreter does.

use crate::backend::x86_64::{Code, Data, RAX, RBP, RCX};

/// `rt_report_error`: print the pending raise the way the interpreter prints
/// an uncaught one and exit 1.
pub(crate) fn emit_report(code: &mut Code, data: &mut Data, prefix: u32, payload: u32) {
    let newline = data.intern("\n");
    code.label("rt_report_error");
    code.push(RBP);
    code.mov_rbp_rsp();
    for slot in [prefix, payload] {
        code.mov_data_address(RCX, slot);
        code.load_ptr(RAX, RCX);
        code.load_ptr(RCX, RAX);
        code.lea_disp8(RAX, RAX, 8);
        code.call("rt_write_err");
    }
    code.mov_data_address(RAX, newline + 8);
    code.mov_imm32(RCX, 1);
    code.call("rt_write_err");
    code.mov_imm32_zero(RAX, 1);
    code.call("rt_exit");
    code.ud2();
}
