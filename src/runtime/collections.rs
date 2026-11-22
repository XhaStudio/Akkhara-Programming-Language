//! The native collection runtime: arena allocation, indexing, length and
//! printing for the list / tuple / set / dict values the backend now emits.
//!
//! A collection is one word, the address of a descriptor in the runtime arena
//! or in the executable's writable data:
//!
//! ```text
//!   +0   kind     (the `runtime::value` tag vocabulary)
//!   +8   length   (number of elements, or of key/value pairs)
//!   +16  elements, each `{ tag: u64, word: u64 }`
//! ```
//!
//! The tag is the `Value` tag for the kind (`TAG_INT`, `TAG_STR`, `TAG_LIST`,
//! ...), with two descriptor-only tags: [`TAG_FLOAT`] for a raw `f64` word and
//! [`TAG_BOOL`] for a word whose `0`/`1` decides between `False` and `True`.
//! The code generator writes the tag it learned for each element, so the
//! runtime printer and index helpers dispatch on the same vocabulary the Rust
//! value layer uses. This is the codegen bridge: a collection element is not
//! yet a NaN-boxed `Value` word (an `int` still travels inline), but its tag
//! space, display rules and kind names come from `src/runtime/value.rs`.
//!
//! Allocation is a bump pointer over a zeroed arena the image reserves. There
//! is no collector yet (mark-sweep is a later part of Step 3), so a program
//! that builds collections in a long loop will reach `rt_alloc`'s honest
//! "arena exhausted" exit (E177 on stderr, exit 1) instead of corrupting
//! memory. The index/key routines take the lowered source line in `r8`, so
//! their `E049`/`E053` text is byte-identical to the interpreter's.

use crate::backend::x86_64::encode::*;
use crate::backend::x86_64::{Code, Data};
use crate::runtime::value::{
    TAG_BOOL, TAG_DICT, TAG_FALSE, TAG_FLOAT, TAG_INT, TAG_LIST, TAG_NULL, TAG_SET, TAG_STR,
    TAG_TRUE, TAG_TUPLE,
};

/// Emit the collection helpers. `arena_used` is a `.data` slot holding the
/// bump offset, `arena_start` the offset of the zeroed arena, and
/// `arena_size` its length in bytes.
pub(crate) fn emit(
    code: &mut Code,
    data: &mut Data,
    arena_used: u32,
    arena_start: u32,
    arena_size: u32,
) {
    let newline = data.intern("\n");
    let comma = data.intern(", ");
    let colon = data.intern(": ");
    let open_list = data.intern("[");
    let close_list = data.intern("]");
    let open_tuple = data.intern("(");
    let close_tuple = data.intern(")");
    let open_set = data.intern("{");
    let close_set = data.intern("}");
    let open_dict = data.intern("{");
    let close_dict = data.intern("}");
    let null = data.intern("null");
    let question = data.intern("?");
    let index_prefix = data.intern("E049 လိုင်း ");
    let index_mid = data.intern(" တွင် အညွှန်း ");
    let index_suffix = data.intern(" သည် အကွာအဝေးပြင်ပတွင် ရှိနေပါသည်။");
    let key_prefix = data.intern("E053 လိုင်း ");
    let key_mid = data.intern(" တွင် သော့ \"");
    let key_suffix = data.intern("\" ကို ရှာမတွေ့ပါ။");
    let exhausted = data.intern("E177 native runtime arena ကုန်ဆုံးသွားပါသည်။");

    emit_alloc(
        code,
        data,
        arena_used,
        arena_start,
        arena_size,
        exhausted,
        newline,
    );
    emit_index(code);
    emit_index_dict(code);
    emit_panics(
        code,
        data,
        index_prefix,
        index_mid,
        index_suffix,
        key_prefix,
        key_mid,
        key_suffix,
        newline,
    );
    emit_print(
        code,
        data,
        comma,
        colon,
        open_list,
        close_list,
        open_tuple,
        close_tuple,
        open_set,
        close_set,
        open_dict,
        close_dict,
        null,
        question,
    );
}

/// `rt_alloc`: `rdi` bytes, eight-byte aligned, returns the address in `rax`.
fn emit_alloc(
    code: &mut Code,
    data: &mut Data,
    arena_used: u32,
    arena_start: u32,
    arena_size: u32,
    exhausted: u32,
    newline: u32,
) {
    code.label("rt_alloc");
    code.push(RBP);
    code.mov_rbp_rsp();
    code.sub_rsp(32);
    code.store(-8, RDI);
    code.mov_data_address(RCX, arena_used);
    code.load_ptr(RAX, RCX);
    code.add_imm8(RAX, 7);
    code.and_imm32(RAX, -8);
    code.store(-16, RAX);
    code.add_rr(RAX, RDI);
    code.store(-24, RAX);
    code.cmp_imm32(RAX, arena_size as i32);
    code.jcc(CC_A, "alloc_exhausted");
    code.mov_data_address(RCX, arena_used);
    code.load(RDX, -24);
    code.store_ptr(RCX, RDX);
    code.mov_data_address(RCX, arena_start);
    code.load(RDX, -16);
    code.add_rr(RDX, RCX);
    code.mov_rr(RAX, RDX);
    code.leave();
    code.ret();

    code.label("alloc_exhausted");
    write(
        code,
        exhausted,
        "E177 native runtime arena ကုန်ဆုံးသွားပါသည်။".len(),
        true,
    );
    write(code, newline, 1, true);
    code.mov_imm32_zero(RAX, 1);
    code.call("rt_exit");
    code.ud2();
    let _ = data;
}

/// `rt_index_seq`: `rax` is the collection descriptor, `rcx` the index and
/// `r8` the source line; the element word comes back in `rax`. Negative and
/// past-the-end indices are the interpreter's `E049`, line included.
fn emit_index(code: &mut Code) {
    code.label("rt_index_seq");
    code.push(RBP);
    code.mov_rbp_rsp();
    code.sub_rsp(32);
    code.store(-8, RAX);
    code.store(-16, RCX);
    code.store(-24, R8);
    code.load_ptr_disp32(RDX, RAX, 8);
    code.test_rr(RCX, RCX);
    code.jcc(CC_NS, "index_non_negative");
    code.mov_rr(RAX, RCX);
    code.load(RDI, -24);
    code.call("rt_panic_index");
    code.ud2();
    code.label("index_non_negative");
    code.cmp_rr(RCX, RDX);
    code.jcc(CC_AE, "index_out_of_range");
    code.shl_imm(RCX, 4);
    code.add_imm8(RCX, 24);
    code.add_rr(RCX, RAX);
    code.load_ptr(RAX, RCX);
    code.leave();
    code.ret();
    code.label("index_out_of_range");
    code.load(RAX, -16);
    code.load(RDI, -24);
    code.call("rt_panic_index");
    code.ud2();
}

/// `rt_index_dict`: `rax` is the dict descriptor, `rdx` the key string and
/// `r8` the source line; the matching value word comes back in `rax`. A missing
/// key is the interpreter's `E053`, line included.
fn emit_index_dict(code: &mut Code) {
    code.label("rt_index_dict");
    code.push(RBP);
    code.mov_rbp_rsp();
    code.sub_rsp(32);
    code.store(-8, RAX);
    code.store(-16, RDX);
    code.store(-24, R8);
    code.mov_imm32_zero(RCX, 0);
    code.store(-32, RCX);
    code.label("dict_index_loop");
    code.load(RAX, -8);
    code.load_ptr_disp32(RDX, RAX, 8);
    code.load(RCX, -32);
    code.cmp_rr(RCX, RDX);
    code.jcc(CC_AE, "dict_index_missing");
    code.shl_imm(RCX, 5);
    code.add_imm8(RCX, 24);
    code.add_rr(RCX, RAX);
    code.load_ptr(RAX, RCX);
    code.load(RCX, -16);
    code.call("rt_cmp_str");
    code.test_rr(RAX, RAX);
    code.jcc(CC_E, "dict_index_found");
    code.load(RCX, -32);
    code.inc(RCX);
    code.store(-32, RCX);
    code.jmp("dict_index_loop");
    code.label("dict_index_found");
    code.load(RAX, -8);
    code.load(RCX, -32);
    code.shl_imm(RCX, 5);
    code.add_imm8(RCX, 40);
    code.add_rr(RCX, RAX);
    code.load_ptr(RAX, RCX);
    code.leave();
    code.ret();
    code.label("dict_index_missing");
    code.load(RAX, -16);
    code.load(RDI, -24);
    code.call("rt_panic_key");
    code.ud2();
}

/// Write one interned descriptor's bytes to stdout (`false`) or stderr.
fn write(code: &mut Code, offset: u32, length: usize, stderr: bool) {
    code.mov_data_address(RAX, offset + 8);
    code.mov_imm32(RCX, length as i32);
    code.call(if stderr { "rt_write_err" } else { "rt_write" });
}

fn emit_panics(
    code: &mut Code,
    data: &mut Data,
    index_prefix: u32,
    index_mid: u32,
    index_suffix: u32,
    key_prefix: u32,
    key_mid: u32,
    key_suffix: u32,
    newline: u32,
) {
    let index_prefix_len = "E049 လိုင်း ".len();
    let index_mid_len = " တွင် အညွှန်း ".len();
    let index_suffix_len = " သည် အကွာအဝေးပြင်ပတွင် ရှိနေပါသည်။".len();
    let key_prefix_len = "E053 လိုင်း ".len();
    let key_mid_len = " တွင် သော့ \"".len();
    let key_suffix_len = "\" ကို ရှာမတွေ့ပါ။".len();

    // `rdi` is the source line and `rax` the offending index.
    code.label("rt_panic_index");
    code.push(RBP);
    code.mov_rbp_rsp();
    code.sub_rsp(16);
    code.store(-8, RDI);
    code.store(-16, RAX);
    write(code, index_prefix, index_prefix_len, true);
    code.load(RAX, -8);
    code.call("rt_write_int_err");
    write(code, index_mid, index_mid_len, true);
    code.load(RAX, -16);
    code.call("rt_write_int_err");
    write(code, index_suffix, index_suffix_len, true);
    write(code, newline, 1, true);
    code.mov_imm32_zero(RAX, 1);
    code.call("rt_exit");
    code.ud2();

    // `rdi` is the source line and `rax` the missing key descriptor.
    code.label("rt_panic_key");
    code.push(RBP);
    code.mov_rbp_rsp();
    code.sub_rsp(16);
    code.store(-8, RDI);
    code.store(-16, RAX);
    write(code, key_prefix, key_prefix_len, true);
    code.load(RAX, -8);
    code.call("rt_write_int_err");
    write(code, key_mid, key_mid_len, true);
    code.load(RAX, -16);
    code.call("rt_write_str_err");
    write(code, key_suffix, key_suffix_len, true);
    write(code, newline, 1, true);
    code.mov_imm32_zero(RAX, 1);
    code.call("rt_exit");
    code.ud2();
    let _ = data;
}

/// `rt_print_value`: `rdi` is the element tag and `rax` the element word.
/// Writes the interpreter's `repr` form, with no trailing newline.
fn emit_print_value(code: &mut Code, null: u32, question: u32) {
    code.label("rt_print_value");
    code.push(RBP);
    code.mov_rbp_rsp();
    code.sub_rsp(16);
    code.store(-8, RDI);
    code.store(-16, RAX);
    for (tag, label) in [
        (TAG_INT, "pv_int"),
        (TAG_FLOAT, "pv_float"),
        (TAG_BOOL, "pv_bool"),
        (TAG_TRUE, "pv_bool"),
        (TAG_FALSE, "pv_bool"),
        (TAG_STR, "pv_str"),
        (TAG_LIST, "pv_list"),
        (TAG_TUPLE, "pv_tuple"),
        (TAG_SET, "pv_set"),
        (TAG_DICT, "pv_dict"),
        (TAG_NULL, "pv_null"),
    ] {
        code.load(RDI, -8);
        code.cmp_imm32(RDI, tag as i32);
        code.jcc(CC_E, label);
    }
    write(code, question, 1, false);
    code.leave();
    code.ret();

    code.label("pv_null");
    write(code, null, 4, false);
    code.leave();
    code.ret();
    for label in ["pv_list", "pv_tuple", "pv_set", "pv_dict"] {
        code.label(label);
        code.load(RDI, -8);
        code.load(RAX, -16);
        code.call("rt_print_collection");
        code.leave();
        code.ret();
    }
    code.label("pv_int");
    code.load(RAX, -16);
    code.call("rt_write_int");
    code.leave();
    code.ret();
    code.label("pv_float");
    code.load(RAX, -16);
    code.call("rt_write_float");
    code.leave();
    code.ret();
    code.label("pv_bool");
    code.load(RAX, -16);
    code.call("rt_write_bool");
    code.leave();
    code.ret();
    code.label("pv_str");
    code.load(RAX, -16);
    code.call("rt_write_str_quoted");
    code.leave();
    code.ret();
}

#[allow(clippy::too_many_arguments)]
fn emit_print(
    code: &mut Code,
    data: &mut Data,
    comma: u32,
    colon: u32,
    open_list: u32,
    close_list: u32,
    open_tuple: u32,
    close_tuple: u32,
    open_set: u32,
    close_set: u32,
    open_dict: u32,
    close_dict: u32,
    null: u32,
    question: u32,
) {
    emit_print_value(code, null, question);

    // `rt_print_collection`: `rdi` is the collection tag, `rax` its address.
    code.label("rt_print_collection");
    code.push(RBP);
    code.mov_rbp_rsp();
    code.sub_rsp(64);
    code.store(-8, RAX);
    code.store(-16, RDI);
    code.load_ptr_disp32(RCX, RAX, 8);
    code.store(-24, RCX);
    code.mov_imm32_zero(RCX, 0);
    code.store(-32, RCX);

    for (tag, offset) in [
        (TAG_LIST, open_list),
        (TAG_TUPLE, open_tuple),
        (TAG_SET, open_set),
        (TAG_DICT, open_dict),
    ] {
        let skip = format!("pc_open_skip_{tag}");
        code.load(RDI, -16);
        code.cmp_imm32(RDI, tag as i32);
        code.jcc(CC_NE, &skip);
        write(code, offset, 1, false);
        code.jmp("pc_opened");
        code.label(&skip);
        let _ = data;
    }
    code.label("pc_opened");

    code.label("pc_loop");
    code.load(RCX, -32);
    code.load(RDX, -24);
    code.cmp_rr(RCX, RDX);
    code.jcc(CC_AE, "pc_done");
    code.test_rr(RCX, RCX);
    code.jcc(CC_E, "pc_no_separator");
    write(code, comma, 2, false);
    code.label("pc_no_separator");
    code.load(RDI, -16);
    code.cmp_imm32(RDI, TAG_DICT as i32);
    code.jcc(CC_E, "pc_dict_item");
    code.load(RCX, -32);
    code.shl_imm(RCX, 4);
    code.add_imm8(RCX, 16);
    code.load(RDX, -8);
    code.add_rr(RDX, RCX);
    code.load_ptr_disp32(RDI, RDX, 0);
    code.load_ptr_disp32(RAX, RDX, 8);
    code.call("rt_print_value");
    code.jmp("pc_next");
    code.label("pc_dict_item");
    code.load(RCX, -32);
    code.shl_imm(RCX, 5);
    code.add_imm8(RCX, 16);
    code.load(RDX, -8);
    code.add_rr(RDX, RCX);
    code.load_ptr_disp32(RDI, RDX, 0);
    code.load_ptr_disp32(RAX, RDX, 8);
    code.call("rt_print_value");
    write(code, colon, 2, false);
    code.load(RCX, -32);
    code.shl_imm(RCX, 5);
    code.add_imm8(RCX, 16);
    code.load(RDX, -8);
    code.add_rr(RDX, RCX);
    code.load_ptr_disp32(RDI, RDX, 16);
    code.load_ptr_disp32(RAX, RDX, 24);
    code.call("rt_print_value");
    code.label("pc_next");
    code.load(RCX, -32);
    code.inc(RCX);
    code.store(-32, RCX);
    code.jmp("pc_loop");

    code.label("pc_done");
    for (tag, offset) in [
        (TAG_LIST, close_list),
        (TAG_TUPLE, close_tuple),
        (TAG_SET, close_set),
        (TAG_DICT, close_dict),
    ] {
        let skip = format!("pc_close_skip_{tag}");
        code.load(RDI, -16);
        code.cmp_imm32(RDI, tag as i32);
        code.jcc(CC_NE, &skip);
        write(code, offset, 1, false);
        code.jmp("pc_closed");
        code.label(&skip);
    }
    code.label("pc_closed");
    code.leave();
    code.ret();
    let _ = data;
}
