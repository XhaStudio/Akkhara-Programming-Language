//! Arithmetic run-time failures the slice can hit: `%` by zero (`E038`) and a
//! zero divisor for `/` (`E036`/`E037`). The interpreter reports them and exits
//! 1; a computed `idiv` would trap the process instead, so the generated code
//! checks first and calls here. The collection runtime's `E049`/`E053` panics
//! live in `collections.rs`, next to the indexing code that raises them.

use crate::backend::x86_64::{Code, Data, RAX, RBP, RCX};

/// The message the interpreter prints for a zero remainder divisor, without
/// the line number a compiled program has no span for.
const MESSAGE: &str = "E038 သုညဖြင့် ကြွင်းကိန်းရှာ၍မရပါ။";

/// The interpreter's two division-by-zero messages, without the line number.
/// `E036` is two ints divided (`/` makes a float, but the check is on the
/// divisor before the conversion), `E037` is a division with a float in it.
const MESSAGE_INT_DIV: &str = "E036 သုညဖြင့် စား၍မရပါ။";
const MESSAGE_FLOAT_DIV: &str = "E037 သုညဖြင့် စား၍မရပါ။";

pub(crate) fn emit_div(code: &mut Code, data: &mut Data) {
    let messages = [
        ("rt_panic_div_int", MESSAGE_INT_DIV),
        ("rt_panic_div_float", MESSAGE_FLOAT_DIV),
    ];
    let newline = data.intern("\n");
    for (label, text) in messages {
        let message = data.intern(text);
        code.label(label);
        code.push(RBP);
        code.mov_rbp_rsp();
        code.mov_data_address(RAX, message + 8);
        code.mov_imm32(RCX, text.len() as i32);
        code.call("rt_write_err");
        code.mov_data_address(RAX, newline + 8);
        code.mov_imm32(RCX, 1);
        code.call("rt_write_err");
        code.mov_imm32_zero(RAX, 1);
        code.call("rt_exit");
        code.ud2();
    }
}

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
