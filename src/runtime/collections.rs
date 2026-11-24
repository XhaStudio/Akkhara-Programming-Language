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
    let pop_empty = data.intern("E110 pop() စာရင်းအလွတ်မှ ဖယ်၍မရပါ");
    let index_plain_prefix = data.intern("E049 အညွှန်း ");
    let index_plain_suffix = data.intern(" သည် အကွာအဝေးပြင်ပတွင် ရှိနေပါသည်။");

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
    emit_items_dict(code);
    emit_items_range(code);
    emit_store(code);
    emit_reverse(code);
    emit_pop(code, pop_empty, newline);
    emit_insert(code);
    emit_remove(code);
    emit_sort(code);
    emit_panic_message(code, newline);
    emit_panics(
        code,
        data,
        index_prefix,
        index_mid,
        index_suffix,
        key_prefix,
        key_mid,
        key_suffix,
        index_plain_prefix,
        index_plain_suffix,
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

/// `rt_items_range_int`: `rax` is the exclusive upper bound of a numeric
/// `for` loop, already a whole number; `rax` leaves as a fresh list descriptor
/// holding `0, 1, ..., n-1` as ints. That is exactly the sequence the
/// interpreter's auto-range walks, so `Len`/`Index` type the loop variable as
/// an int and nothing else in the slice changes.
///
/// A bound at or below zero gives an empty list, which is why `for i in 0` and
/// `for i in -3` print nothing. A bound past what the arena can hold is capped
/// at 16384 elements -- itself too large for the 256 KiB arena -- so `rt_alloc`
/// takes its documented `E177` exhaustion path instead of the size arithmetic
/// wrapping.
fn emit_items_range(code: &mut Code) {
    code.label("rt_items_range_int");
    code.push(RBP);
    code.mov_rbp_rsp();
    code.sub_rsp(32);
    code.test_rr(RAX, RAX);
    code.jcc(CC_NS, "range_non_negative");
    code.mov_imm32_zero(RAX, 0);
    code.label("range_non_negative");
    code.cmp_imm32(RAX, 16384);
    code.jcc(CC_LE, "range_size_ok");
    code.mov_imm32(RAX, 16384);
    code.label("range_size_ok");
    code.store(-8, RAX);
    // `16 + 16 * n`: the descriptor header and one `{tag, word}` per element.
    code.mov_rr(RCX, RAX);
    code.shl_imm(RCX, 4);
    code.add_imm8(RCX, 16);
    code.mov_rr(RDI, RCX);
    code.call("rt_alloc");
    code.mov_rr(R11, RAX);
    code.mov_imm32(RCX, TAG_LIST as i32);
    code.store_ptr_disp32(R11, 0, RCX);
    code.load(RDX, -8);
    code.store_ptr_disp32(R11, 8, RDX);
    code.mov_imm32_zero(RCX, 0);
    code.label("range_fill");
    code.cmp_rr(RCX, RDX);
    code.jcc(CC_AE, "range_done");
    code.mov_rr(RAX, RCX);
    code.shl_imm(RAX, 4);
    code.add_rr(RAX, R11);
    code.mov_imm32(RDI, TAG_INT as i32);
    code.store_ptr_disp32(RAX, 16, RDI);
    code.store_ptr_disp32(RAX, 24, RCX);
    code.inc(RCX);
    code.jmp("range_fill");
    code.label("range_done");
    code.mov_rr(RAX, R11);
    code.leave();
    code.ret();
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

/// `rt_items_dict`: `rax` is a dict descriptor and the result is a fresh list
/// descriptor holding its keys, in insertion order. A `for` loop over a dict
/// yields exactly those keys, so collecting them into a list lets the loop's
/// existing `Len`/`Index` read them without a second iteration shape.
fn emit_items_dict(code: &mut Code) {
    code.label("rt_items_dict");
    code.push(RBP);
    code.mov_rbp_rsp();
    code.sub_rsp(64);
    code.store(-8, RAX);
    code.load_ptr_disp32(RDX, RAX, 8);
    code.store(-16, RDX);
    // `16 + 16 * len`: the list header plus one tag/word pair per key.
    code.mov_rr(RDI, RDX);
    code.shl_imm(RDI, 4);
    code.add_imm8(RDI, 16);
    code.call("rt_alloc");
    code.store(-32, RAX);
    code.mov_imm32(RCX, TAG_LIST as i32);
    code.store_ptr_disp32(RAX, 0, RCX);
    code.load(RCX, -16);
    code.store_ptr_disp32(RAX, 8, RCX);
    code.mov_imm32_zero(RCX, 0);
    code.store(-40, RCX);
    code.label("items_dict_loop");
    code.load(RCX, -40);
    code.load(RDX, -16);
    code.cmp_rr(RCX, RDX);
    code.jcc(CC_AE, "items_dict_done");
    // The key word of pair `i` sits at `dict + 16 + 32 * i + 8`.
    code.load(RAX, -8);
    code.load(RCX, -40);
    code.shl_imm(RCX, 5);
    code.add_imm8(RCX, 24);
    code.add_rr(RCX, RAX);
    code.load_ptr(RDX, RCX);
    // The destination element sits at `list + 16 + 16 * i`.
    code.load(RAX, -32);
    code.load(RCX, -40);
    code.shl_imm(RCX, 4);
    code.add_imm8(RCX, 16);
    code.add_rr(RCX, RAX);
    code.store_ptr_disp32(RCX, 8, RDX);
    // Every key in the slice is a string literal.
    code.mov_imm32(R8, TAG_STR as i32);
    code.store_ptr_disp32(RCX, 0, R8);
    code.load(RCX, -40);
    code.inc(RCX);
    code.store(-40, RCX);
    code.jmp("items_dict_loop");
    code.label("items_dict_done");
    code.load(RAX, -32);
    code.leave();
    code.ret();
}

/// The indexed-write helpers. Every one of them returns a **new** descriptor
/// and leaves the old one untouched: the interpreter's collections have value
/// semantics (`ys = xs` gives `ys` its own contents), so the generated code
/// writes the result back into the binding instead of mutating a shared
/// descriptor. Growing a list, adding a dict key or dropping a duplicate set
/// member therefore costs one copy, and the arena keeps the old descriptor.
fn emit_store(code: &mut Code) {
    emit_value_eq(code);
    emit_collection_eq(code);
    emit_store_seq(code);
    emit_store_dict(code);
    emit_store_set(code);
}

/// `rt_value_eq`: `rdi` is the element tag, `rax` and `rdx` are the two words;
/// the answer is 1 or 0 in `rax`. It follows the interpreter's equality for
/// one set of a single element kind: strings compare by bytes, floats by bits
/// (with `NaN` equal to `NaN`), everything else by the word.
fn emit_value_eq(code: &mut Code) {
    code.label("rt_value_eq");
    code.push(RBP);
    code.mov_rbp_rsp();
    code.sub_rsp(32);
    code.store(-8, RDI);
    code.store(-16, RAX);
    code.store(-24, RDX);
    // A nested collection compares by its contents, the way its printed form
    // does; `rt_collection_eq` checks the kind first, so a list never equals a
    // tuple even with the same elements.
    for tag in [TAG_LIST, TAG_TUPLE, TAG_SET, TAG_DICT] {
        code.cmp_imm32(RDI, tag as i32);
        code.jcc(CC_E, "veq_collection");
    }
    code.cmp_imm32(RDI, TAG_STR as i32);
    code.jcc(CC_E, "veq_str");
    code.cmp_imm32(RDI, TAG_FLOAT as i32);
    code.jcc(CC_E, "veq_float");
    code.load(RAX, -16);
    code.load(RCX, -24);
    code.cmp_rr(RAX, RCX);
    code.jcc(CC_E, "veq_yes");
    code.jmp("veq_no");

    code.label("veq_str");
    code.load(RAX, -16);
    code.load(RCX, -24);
    code.call("rt_cmp_str");
    code.test_rr(RAX, RAX);
    code.jcc(CC_E, "veq_yes");
    code.jmp("veq_no");

    code.label("veq_float");
    code.load(RAX, -16);
    code.load(RCX, -24);
    code.cmp_rr(RAX, RCX);
    code.jcc(CC_E, "veq_yes");
    // Different bits: equal only when both are NaN, as the interpreter's
    // printed-form comparison answers.
    code.mov_imm64(R9, 0x7fff_ffff_ffff_ffff);
    code.mov_rr(R8, RAX);
    code.and_rr(R8, R9);
    code.mov_imm64(R10, 0x7ff0_0000_0000_0000);
    code.cmp_rr(R8, R10);
    code.jcc(CC_LE, "veq_no");
    code.mov_rr(R8, RCX);
    code.and_rr(R8, R9);
    code.cmp_rr(R8, R10);
    code.jcc(CC_LE, "veq_no");

    code.label("veq_collection");
    code.load(RAX, -16);
    code.load(RDX, -24);
    code.call("rt_collection_eq");
    code.leave();
    code.ret();

    code.label("veq_yes");
    code.mov_imm32_zero(RAX, 1);
    code.leave();
    code.ret();
    code.label("veq_no");
    code.mov_imm32_zero(RAX, 0);
    code.leave();
    code.ret();
}

/// `rt_collection_eq`: `rax` and `rdx` are two descriptors; the answer is 1 or
/// 0 in `rax`. It is the interpreter's `repr(a) == repr(b)` rule: the kinds
/// must be the same, the lengths must match, and every element (every key and
/// value for a dict) must be equal, in the order stored. That order is what the
/// printed form shows, so two dicts with the same pairs in a different
/// insertion order are not equal, exactly as in the interpreter.
fn emit_collection_eq(code: &mut Code) {
    code.label("rt_collection_eq");
    code.push(RBP);
    code.mov_rbp_rsp();
    code.sub_rsp(64);
    code.store(-8, RAX);
    code.store(-16, RDX);
    code.cmp_rr(RAX, RDX);
    code.jcc(CC_E, "ceq_yes");
    // Same kind? A list, tuple, set or dict each prints its own brackets.
    code.load(RAX, -8);
    code.load_ptr_disp32(RCX, RAX, 0);
    code.store(-24, RCX);
    code.load(RAX, -16);
    code.load_ptr_disp32(R9, RAX, 0);
    code.cmp_rr(RCX, R9);
    code.jcc(CC_NE, "ceq_no");
    // Same length?
    code.load(RAX, -8);
    code.load_ptr_disp32(RCX, RAX, 8);
    code.store(-32, RCX);
    code.load(RAX, -16);
    code.load_ptr_disp32(R9, RAX, 8);
    code.cmp_rr(RCX, R9);
    code.jcc(CC_NE, "ceq_no");
    code.mov_imm32_zero(RCX, 0);
    code.store(-40, RCX);
    code.label("ceq_loop");
    code.load(RCX, -40);
    code.load(RDX, -32);
    code.cmp_rr(RCX, RDX);
    code.jcc(CC_AE, "ceq_yes");
    code.load(RAX, -24);
    code.cmp_imm32(RAX, TAG_DICT as i32);
    code.jcc(CC_E, "ceq_dict_item");
    // One tag/word pair at `16 + 16 * i`.
    code.load(RAX, -8);
    code.load(RCX, -40);
    code.shl_imm(RCX, 4);
    code.add_imm8(RCX, 16);
    code.add_rr(RCX, RAX);
    code.load_ptr_disp32(RDI, RCX, 0);
    code.load_ptr_disp32(RAX, RCX, 8);
    code.load(RDX, -16);
    code.load(R11, -40);
    code.shl_imm(R11, 4);
    code.add_imm8(R11, 16);
    code.add_rr(R11, RDX);
    code.load_ptr_disp32(RDX, R11, 8);
    code.call("rt_value_eq");
    code.test_rr(RAX, RAX);
    code.jcc(CC_E, "ceq_no");
    code.jmp("ceq_next");
    // A dict pair compares its key and then its value, at 32-byte strides.
    code.label("ceq_dict_item");
    code.load(RAX, -8);
    code.load(RCX, -40);
    code.shl_imm(RCX, 5);
    code.add_imm8(RCX, 16);
    code.add_rr(RCX, RAX);
    code.load_ptr_disp32(RDI, RCX, 0);
    code.load_ptr_disp32(RAX, RCX, 8);
    code.load(RDX, -16);
    code.load(R11, -40);
    code.shl_imm(R11, 5);
    code.add_imm8(R11, 16);
    code.add_rr(R11, RDX);
    code.load_ptr_disp32(RDX, R11, 8);
    code.call("rt_value_eq");
    code.test_rr(RAX, RAX);
    code.jcc(CC_E, "ceq_no");
    code.load(RAX, -8);
    code.load(RCX, -40);
    code.shl_imm(RCX, 5);
    code.add_imm8(RCX, 16);
    code.add_rr(RCX, RAX);
    code.load_ptr_disp32(RDI, RCX, 16);
    code.load_ptr_disp32(RAX, RCX, 24);
    code.load(RDX, -16);
    code.load(R11, -40);
    code.shl_imm(R11, 5);
    code.add_imm8(R11, 16);
    code.add_rr(R11, RDX);
    code.load_ptr_disp32(RDX, R11, 24);
    code.call("rt_value_eq");
    code.test_rr(RAX, RAX);
    code.jcc(CC_E, "ceq_no");
    code.label("ceq_next");
    code.load(RCX, -40);
    code.inc(RCX);
    code.store(-40, RCX);
    code.jmp("ceq_loop");
    code.label("ceq_yes");
    code.mov_imm32_zero(RAX, 1);
    code.leave();
    code.ret();
    code.label("ceq_no");
    code.mov_imm32_zero(RAX, 0);
    code.leave();
    code.ret();
}

/// `rt_reverse_seq`: `rax` is a list/tuple descriptor and the result is a
/// fresh descriptor of the same kind and length with the elements in the
/// opposite order. The source descriptor is never written, so a second binding
/// that still points at it keeps its own contents -- the interpreter's value
/// semantics, where only the reversed binding takes the new list.
fn emit_reverse(code: &mut Code) {
    code.label("rt_reverse_seq");
    code.push(RBP);
    code.mov_rbp_rsp();
    code.sub_rsp(48);
    code.store(-8, RAX);
    code.load_ptr_disp32(RCX, RAX, 8);
    code.store(-16, RCX);
    // `16 + 16 * len`: the header and one tag/word pair per element.
    code.mov_rr(RDI, RCX);
    code.shl_imm(RDI, 4);
    code.add_imm8(RDI, 16);
    code.call("rt_alloc");
    code.store(-24, RAX);
    // Header: the source's kind tag, then its length.
    code.load(RCX, -8);
    code.load_ptr(RDX, RCX);
    code.store_ptr_disp32(RAX, 0, RDX);
    code.load(RDX, -16);
    code.store_ptr_disp32(RAX, 8, RDX);
    code.mov_imm32_zero(RCX, 0);
    code.store(-32, RCX);
    code.label("reverse_loop");
    code.load(RCX, -32);
    code.load(RDX, -16);
    code.cmp_rr(RCX, RDX);
    code.jcc(CC_AE, "reverse_done");
    // Source element `i` sits at `source + 16 + 16 * i`.
    code.load(RAX, -8);
    code.mov_rr(R8, RCX);
    code.shl_imm(R8, 4);
    code.add_imm8(R8, 16);
    code.add_rr(R8, RAX);
    // It lands at `dest + 16 + 16 * (len - 1 - i)`.
    code.load(R9, -16);
    code.sub_rr(R9, RCX);
    code.dec(R9);
    code.shl_imm(R9, 4);
    code.add_imm8(R9, 16);
    code.load(R10, -24);
    code.add_rr(R9, R10);
    code.load_ptr(R10, R8);
    code.store_ptr_disp32(R9, 0, R10);
    code.load_ptr_disp32(R10, R8, 8);
    code.store_ptr_disp32(R9, 8, R10);
    code.load(RCX, -32);
    code.inc(RCX);
    code.store(-32, RCX);
    code.jmp("reverse_loop");
    code.label("reverse_done");
    code.load(RAX, -24);
    code.leave();
    code.ret();
}

/// `rt_pop_seq`: `rax` is a list descriptor. The result is a fresh descriptor
/// without its last element in `rax` and the dropped element's word in `rdx`.
/// An empty list is the interpreter's `E110` panic (with no line number, since
/// a call carries no span).
fn emit_pop(code: &mut Code, pop_empty: u32, newline: u32) {
    code.label("rt_pop_seq");
    code.push(RBP);
    code.mov_rbp_rsp();
    code.sub_rsp(48);
    code.store(-8, RAX);
    code.load_ptr_disp32(RCX, RAX, 8);
    code.test_rr(RCX, RCX);
    code.jcc(CC_NE, "pop_nonempty");
    write(code, pop_empty, "E110 pop() စာရင်းအလွတ်မှ ဖယ်၍မရပါ".len(), true);
    write(code, newline, 1, true);
    code.mov_imm32_zero(RAX, 1);
    code.call("rt_exit");
    code.ud2();
    code.label("pop_nonempty");
    code.dec(RCX);
    code.store(-16, RCX);
    code.mov_rr(RDI, RCX);
    code.shl_imm(RDI, 4);
    code.add_imm8(RDI, 16);
    code.call("rt_alloc");
    code.store(-24, RAX);
    // Header: the source's kind tag, then the shorter length.
    code.load(RCX, -8);
    code.load_ptr(RDX, RCX);
    code.store_ptr_disp32(RAX, 0, RDX);
    code.load(RDX, -16);
    code.store_ptr_disp32(RAX, 8, RDX);
    code.mov_imm32_zero(RCX, 0);
    code.store(-32, RCX);
    code.label("pop_copy");
    code.load(RCX, -32);
    code.load(RDX, -16);
    code.cmp_rr(RCX, RDX);
    code.jcc(CC_AE, "pop_copied");
    code.load(RAX, -8);
    code.mov_rr(R8, RCX);
    code.shl_imm(R8, 4);
    code.add_imm8(R8, 16);
    code.add_rr(R8, RAX);
    code.load(R9, -24);
    code.mov_rr(R10, RCX);
    code.shl_imm(R10, 4);
    code.add_imm8(R10, 16);
    code.add_rr(R10, R9);
    code.load_ptr(R11, R8);
    code.store_ptr_disp32(R10, 0, R11);
    code.load_ptr_disp32(R11, R8, 8);
    code.store_ptr_disp32(R10, 8, R11);
    code.load(RCX, -32);
    code.inc(RCX);
    code.store(-32, RCX);
    code.jmp("pop_copy");
    code.label("pop_copied");
    // The dropped word is element `newlen` of the source: 24 + 16 * newlen.
    code.load(RAX, -8);
    code.load(RCX, -16);
    code.shl_imm(RCX, 4);
    code.add_imm8(RCX, 24);
    code.add_rr(RCX, RAX);
    code.load_ptr(RDX, RCX);
    code.load(RAX, -24);
    code.leave();
    code.ret();
}

/// `rt_insert_seq`: `rax` is a list descriptor, `rcx` the position, `rdx` the
/// value word and `rdi` its tag. The position may equal the length, which
/// appends; anything outside `0..=len` is the interpreter's `E049` (without
/// the line clause a call cannot carry). The result is a new descriptor in
/// `rax`.
fn emit_insert(code: &mut Code) {
    code.label("rt_insert_seq");
    code.push(RBP);
    code.mov_rbp_rsp();
    code.sub_rsp(96);
    code.store(-8, RAX);
    code.store(-16, RCX);
    code.store(-24, RDX);
    code.store(-32, RDI);
    code.load_ptr_disp32(R9, RAX, 8);
    code.store(-40, R9);
    code.load(RCX, -16);
    code.cmp_imm32(RCX, 0);
    code.jcc(CC_L, "insert_bad");
    code.load(RDX, -40);
    code.load(RCX, -16);
    code.cmp_rr(RCX, RDX);
    code.jcc(CC_A, "insert_bad");
    code.load(R9, -40);
    code.inc(R9);
    code.store(-48, R9);
    code.mov_rr(RDI, R9);
    code.shl_imm(RDI, 4);
    code.add_imm8(RDI, 16);
    code.call("rt_alloc");
    code.store(-56, RAX);
    code.load(RCX, -8);
    code.load_ptr(RDX, RCX);
    code.store_ptr_disp32(RAX, 0, RDX);
    code.load(RDX, -48);
    code.store_ptr_disp32(RAX, 8, RDX);
    code.mov_imm32_zero(RCX, 0);
    code.store(-64, RCX);
    code.label("insert_copy");
    code.load(RCX, -64);
    code.load(RDX, -40);
    code.cmp_rr(RCX, RDX);
    code.jcc(CC_AE, "insert_copied");
    // The source element.
    code.load(RAX, -8);
    code.mov_rr(R8, RCX);
    code.shl_imm(R8, 4);
    code.add_imm8(R8, 16);
    code.add_rr(R8, RAX);
    // Everything from the position on moves one place right.
    code.load(R9, -64);
    code.load(R10, -16);
    code.cmp_rr(R9, R10);
    code.jcc(CC_B, "insert_keep_index");
    code.inc(R9);
    code.label("insert_keep_index");
    code.shl_imm(R9, 4);
    code.add_imm8(R9, 16);
    code.load(R10, -56);
    code.add_rr(R9, R10);
    code.load_ptr(R10, R8);
    code.store_ptr_disp32(R9, 0, R10);
    code.load_ptr_disp32(R10, R8, 8);
    code.store_ptr_disp32(R9, 8, R10);
    code.load(RCX, -64);
    code.inc(RCX);
    code.store(-64, RCX);
    code.jmp("insert_copy");
    code.label("insert_copied");
    // The new element at the position.
    code.load(RAX, -56);
    code.load(RCX, -16);
    code.shl_imm(RCX, 4);
    code.add_imm8(RCX, 16);
    code.add_rr(RCX, RAX);
    code.load(RDX, -24);
    code.store_ptr_disp32(RCX, 8, RDX);
    code.load(RDX, -32);
    code.store_ptr_disp32(RCX, 0, RDX);
    code.load(RAX, -56);
    code.leave();
    code.ret();
    code.label("insert_bad");
    code.load(RAX, -16);
    code.call("rt_panic_index_plain");
    code.ud2();
}

/// `rt_remove_seq`: `rax` is a list descriptor and `rcx` the position. The
/// result is a fresh descriptor without that element in `rax` and the removed
/// element's word in `rdx`. A position outside `0..len` is `E049` without the
/// line clause.
fn emit_remove(code: &mut Code) {
    code.label("rt_remove_seq");
    code.push(RBP);
    code.mov_rbp_rsp();
    code.sub_rsp(80);
    code.store(-8, RAX);
    code.store(-16, RCX);
    code.load_ptr_disp32(R9, RAX, 8);
    code.store(-24, R9);
    code.load(RCX, -16);
    code.cmp_imm32(RCX, 0);
    code.jcc(CC_L, "remove_bad");
    code.load(RDX, -24);
    code.load(RCX, -16);
    code.cmp_rr(RCX, RDX);
    code.jcc(CC_AE, "remove_bad");
    code.load(R9, -24);
    code.dec(R9);
    code.store(-32, R9);
    code.mov_rr(RDI, R9);
    code.shl_imm(RDI, 4);
    code.add_imm8(RDI, 16);
    code.call("rt_alloc");
    code.store(-40, RAX);
    code.load(RCX, -8);
    code.load_ptr(RDX, RCX);
    code.store_ptr_disp32(RAX, 0, RDX);
    code.load(RDX, -32);
    code.store_ptr_disp32(RAX, 8, RDX);
    code.mov_imm32_zero(RCX, 0);
    code.store(-48, RCX);
    code.label("remove_copy");
    code.load(RCX, -48);
    code.load(RDX, -32);
    code.cmp_rr(RCX, RDX);
    code.jcc(CC_AE, "remove_copied");
    // Everything after the position comes from one place further on.
    code.load(R9, -48);
    code.load(R10, -16);
    code.cmp_rr(R9, R10);
    code.jcc(CC_B, "remove_keep_source");
    code.inc(R9);
    code.label("remove_keep_source");
    code.load(RAX, -8);
    code.shl_imm(R9, 4);
    code.add_imm8(R9, 16);
    code.add_rr(R9, RAX);
    code.load(R10, -40);
    code.mov_rr(R8, RCX);
    code.shl_imm(R8, 4);
    code.add_imm8(R8, 16);
    code.add_rr(R8, R10);
    code.load_ptr(R11, R9);
    code.store_ptr_disp32(R8, 0, R11);
    code.load_ptr_disp32(R11, R9, 8);
    code.store_ptr_disp32(R8, 8, R11);
    code.load(RCX, -48);
    code.inc(RCX);
    code.store(-48, RCX);
    code.jmp("remove_copy");
    code.label("remove_copied");
    // The removed word is the source element at the position.
    code.load(RAX, -8);
    code.load(RCX, -16);
    code.shl_imm(RCX, 4);
    code.add_imm8(RCX, 24);
    code.add_rr(RCX, RAX);
    code.load_ptr(RDX, RCX);
    code.load(RAX, -40);
    code.leave();
    code.ret();
    code.label("remove_bad");
    code.load(RAX, -16);
    code.call("rt_panic_index_plain");
    code.ud2();
}

/// `rt_sort_seq`: `rax` is a list descriptor and `rdi` picks the comparison --
/// `0` ints, `1` floats, `2` strings. The result is a fresh, stably sorted
/// descriptor in `rax`; the source is never written. Insertion sort keeps the
/// interpreter's stable order for equal elements, and every element of a
/// native list shares one kind (the analyzer's element shape), so only the
/// words move.
fn emit_sort(code: &mut Code) {
    code.label("rt_sort_seq");
    code.push(RBP);
    code.mov_rbp_rsp();
    code.sub_rsp(96);
    code.store(-8, RAX);
    code.store(-16, RDI);
    code.load_ptr_disp32(RCX, RAX, 8);
    code.store(-24, RCX);
    code.mov_rr(RDI, RCX);
    code.shl_imm(RDI, 4);
    code.add_imm8(RDI, 16);
    code.call("rt_alloc");
    code.store(-32, RAX);
    code.load(RCX, -8);
    code.load_ptr(RDX, RCX);
    code.store_ptr_disp32(RAX, 0, RDX);
    code.load(RDX, -24);
    code.store_ptr_disp32(RAX, 8, RDX);
    // Copy every element, then sort the copy.
    code.mov_imm32_zero(RCX, 0);
    code.store(-40, RCX);
    code.label("sort_copy");
    code.load(RCX, -40);
    code.load(RDX, -24);
    code.cmp_rr(RCX, RDX);
    code.jcc(CC_AE, "sort_copied");
    code.load(RAX, -8);
    code.mov_rr(R8, RCX);
    code.shl_imm(R8, 4);
    code.add_imm8(R8, 16);
    code.add_rr(R8, RAX);
    code.load(R9, -32);
    code.mov_rr(R10, RCX);
    code.shl_imm(R10, 4);
    code.add_imm8(R10, 16);
    code.add_rr(R10, R9);
    code.load_ptr(R11, R8);
    code.store_ptr_disp32(R10, 0, R11);
    code.load_ptr_disp32(R11, R8, 8);
    code.store_ptr_disp32(R10, 8, R11);
    code.load(RCX, -40);
    code.inc(RCX);
    code.store(-40, RCX);
    code.jmp("sort_copy");
    code.label("sort_copied");
    code.mov_imm32(RCX, 1);
    code.store(-48, RCX);
    code.label("sort_outer");
    code.load(RCX, -48);
    code.load(RDX, -24);
    code.cmp_rr(RCX, RDX);
    code.jcc(CC_AE, "sort_done");
    code.store(-56, RCX);
    code.label("sort_inner");
    code.load(RCX, -56);
    code.test_rr(RCX, RCX);
    code.jcc(CC_E, "sort_next_i");
    // Element `j - 1` is at `dest + 8 + 16 * j`, element `j` at `dest + 24 +
    // 16 * j`; only the words move, because the tags are all the same kind.
    code.load(RAX, -32);
    code.mov_rr(R8, RCX);
    code.shl_imm(R8, 4);
    code.mov_rr(R9, R8);
    code.add_imm8(R9, 8);
    code.add_rr(R9, RAX);
    code.mov_rr(R10, R8);
    code.add_imm8(R10, 24);
    code.add_rr(R10, RAX);
    code.store(-64, R9);
    code.store(-72, R10);
    code.load(RDI, -16);
    code.cmp_imm32(RDI, 0);
    code.jcc(CC_NE, "sort_not_int");
    code.load_ptr(RDX, R9);
    code.load_ptr(R11, R10);
    code.cmp_rr(RDX, R11);
    code.jcc(CC_LE, "sort_next_i");
    code.jmp("sort_swap");
    code.label("sort_not_int");
    code.cmp_imm32(RDI, 1);
    code.jcc(CC_NE, "sort_strings");
    code.load_ptr(RDX, R9);
    code.movq_xmm_gpr(XMM0, RDX);
    code.load_ptr(R11, R10);
    code.movq_xmm_gpr(XMM1, R11);
    code.ucomisd(XMM0, XMM1);
    code.jcc(CC_BE, "sort_next_i");
    code.jmp("sort_swap");
    code.label("sort_strings");
    code.load_ptr(RAX, R9);
    code.load_ptr(RCX, R10);
    code.call("rt_cmp_str");
    code.test_rr(RAX, RAX);
    code.jcc(CC_LE, "sort_next_i");
    code.label("sort_swap");
    code.load(R9, -64);
    code.load(R10, -72);
    code.load_ptr(RDX, R9);
    code.load_ptr(R11, R10);
    code.store_ptr(R9, R11);
    code.store_ptr(R10, RDX);
    code.load(RCX, -56);
    code.dec(RCX);
    code.store(-56, RCX);
    code.jmp("sort_inner");
    code.label("sort_next_i");
    code.load(RCX, -48);
    code.inc(RCX);
    code.store(-48, RCX);
    code.jmp("sort_outer");
    code.label("sort_done");
    code.load(RAX, -32);
    code.leave();
    code.ret();
}

/// `rt_store_seq`: `rax` is a list/tuple descriptor, `rcx` the index, `rdx` the
/// value word, `rdi` its tag and `r8` the source line. The index may equal the
/// length, which appends, and anything else outside `0..=len` is the
/// interpreter's `E049`. The result is a new descriptor in `rax`.
fn emit_store_seq(code: &mut Code) {
    code.label("rt_store_seq");
    code.push(RBP);
    code.mov_rbp_rsp();
    code.sub_rsp(96);
    code.store(-8, RAX);
    code.store(-16, RCX);
    code.store(-24, RDX);
    code.store(-32, RDI);
    code.store(-40, R8);
    code.load_ptr_disp32(R9, RAX, 8);
    code.store(-48, R9);
    // A write never leaves a gap: before 0 or past the end is E049.
    code.load(RCX, -16);
    code.cmp_imm32(RCX, 0);
    code.jcc(CC_L, "store_seq_bad");
    code.load(RDX, -48);
    code.cmp_rr(RCX, RDX);
    code.jcc(CC_A, "store_seq_bad");
    // The new length grows only when the position is just past the end.
    code.mov_rr(R9, RDX);
    code.cmp_rr(RCX, RDX);
    code.jcc(CC_NE, "store_seq_same_length");
    code.inc(R9);
    code.label("store_seq_same_length");
    code.store(-56, R9);
    code.mov_rr(RDI, R9);
    code.shl_imm(RDI, 4);
    code.add_imm8(RDI, 16);
    code.call("rt_alloc");
    code.store(-64, RAX);
    // Header: the collection's tag, then the new length.
    code.load(RCX, -8);
    code.load_ptr(RDX, RCX);
    code.store_ptr_disp32(RAX, 0, RDX);
    code.load(RDX, -56);
    code.store_ptr_disp32(RAX, 8, RDX);
    // Copy every old element as its tag/word pair.
    code.mov_imm32_zero(RCX, 0);
    code.store(-72, RCX);
    code.label("store_seq_copy");
    code.load(RCX, -72);
    code.load(RDX, -48);
    code.cmp_rr(RCX, RDX);
    code.jcc(CC_AE, "store_seq_copied");
    code.load(RAX, -8);
    code.load(RCX, -72);
    code.shl_imm(RCX, 4);
    code.add_imm8(RCX, 16);
    code.add_rr(RCX, RAX);
    code.load(RAX, -64);
    code.load(RDX, -72);
    code.shl_imm(RDX, 4);
    code.add_imm8(RDX, 16);
    code.add_rr(RDX, RAX);
    code.load_ptr(R8, RCX);
    code.store_ptr_disp32(RDX, 0, R8);
    code.load_ptr_disp32(R8, RCX, 8);
    code.store_ptr_disp32(RDX, 8, R8);
    code.load(RCX, -72);
    code.inc(RCX);
    code.store(-72, RCX);
    code.jmp("store_seq_copy");
    code.label("store_seq_copied");
    // The written element, whether it replaced one or was appended.
    code.load(RAX, -64);
    code.load(RCX, -16);
    code.shl_imm(RCX, 4);
    code.add_imm8(RCX, 16);
    code.add_rr(RCX, RAX);
    code.load(RDX, -24);
    code.store_ptr_disp32(RCX, 8, RDX);
    code.load(RDX, -32);
    code.store_ptr_disp32(RCX, 0, RDX);
    code.load(RAX, -64);
    code.leave();
    code.ret();
    code.label("store_seq_bad");
    code.load(RAX, -16);
    code.load(RDI, -40);
    code.call("rt_panic_index");
    code.ud2();
}

/// `rt_store_dict`: `rax` is a dict descriptor, `rcx` the key string, `rdx` the
/// value word, `rdi` its tag and `r8` the source line (a write never fails on a
/// missing key: it appends, exactly as `IndexMap::insert` does). Keys keep the
/// position they first had; a new key goes last. The result is a new
/// descriptor in `rax`.
fn emit_store_dict(code: &mut Code) {
    code.label("rt_store_dict");
    code.push(RBP);
    code.mov_rbp_rsp();
    code.sub_rsp(128);
    code.store(-8, RAX);
    code.store(-16, RCX);
    code.store(-24, RDX);
    code.store(-32, RDI);
    code.store(-40, R8);
    code.load_ptr_disp32(R9, RAX, 8);
    code.store(-48, R9);
    // Find the key by content: a match keeps its slot.
    code.mov_imm32_zero(RCX, 0);
    code.store(-56, RCX);
    code.mov_imm64(RCX, u64::MAX);
    code.store(-64, RCX);
    code.label("store_dict_find");
    code.load(RCX, -56);
    code.load(RDX, -48);
    code.cmp_rr(RCX, RDX);
    code.jcc(CC_AE, "store_dict_found_done");
    code.load(RAX, -8);
    code.load(RCX, -56);
    code.shl_imm(RCX, 5);
    code.add_imm8(RCX, 24);
    code.add_rr(RCX, RAX);
    code.load_ptr(RAX, RCX);
    code.load(RCX, -16);
    code.call("rt_cmp_str");
    code.test_rr(RAX, RAX);
    code.jcc(CC_E, "store_dict_found");
    code.load(RCX, -56);
    code.inc(RCX);
    code.store(-56, RCX);
    code.jmp("store_dict_find");
    code.label("store_dict_found");
    code.load(RCX, -56);
    code.store(-64, RCX);
    code.label("store_dict_found_done");
    // A missing key appends one pair.
    code.load(R9, -48);
    code.load(RCX, -64);
    code.cmp_imm32(RCX, -1);
    code.jcc(CC_NE, "store_dict_length_done");
    code.inc(R9);
    code.label("store_dict_length_done");
    code.store(-72, R9);
    code.mov_rr(RDI, R9);
    code.shl_imm(RDI, 5);
    code.add_imm8(RDI, 16);
    code.call("rt_alloc");
    code.store(-80, RAX);
    // Header.
    code.load(RCX, -8);
    code.load_ptr(RDX, RCX);
    code.store_ptr_disp32(RAX, 0, RDX);
    code.load(RDX, -72);
    code.store_ptr_disp32(RAX, 8, RDX);
    // Copy every old pair.
    code.mov_imm32_zero(RCX, 0);
    code.store(-88, RCX);
    code.label("store_dict_copy");
    code.load(RCX, -88);
    code.load(RDX, -48);
    code.cmp_rr(RCX, RDX);
    code.jcc(CC_AE, "store_dict_copied");
    code.load(RAX, -8);
    code.load(RCX, -88);
    code.shl_imm(RCX, 5);
    code.add_imm8(RCX, 16);
    code.add_rr(RCX, RAX);
    code.load(RAX, -80);
    code.load(RDX, -88);
    code.shl_imm(RDX, 5);
    code.add_imm8(RDX, 16);
    code.add_rr(RDX, RAX);
    for offset in [0, 8, 16, 24] {
        code.load_ptr_disp32(R8, RCX, offset);
        code.store_ptr_disp32(RDX, offset, R8);
    }
    code.load(RCX, -88);
    code.inc(RCX);
    code.store(-88, RCX);
    code.jmp("store_dict_copy");
    code.label("store_dict_copied");
    code.load(RCX, -64);
    code.cmp_imm32(RCX, -1);
    code.jcc(CC_E, "store_dict_append");
    // An existing key keeps its position and takes the new value.
    code.load(RAX, -80);
    code.load(RCX, -64);
    code.shl_imm(RCX, 5);
    code.add_imm8(RCX, 16);
    code.add_rr(RCX, RAX);
    code.load(RDX, -24);
    code.store_ptr_disp32(RCX, 24, RDX);
    code.load(RDX, -32);
    code.store_ptr_disp32(RCX, 16, RDX);
    code.load(RAX, -80);
    code.leave();
    code.ret();
    code.label("store_dict_append");
    code.load(RAX, -80);
    code.load(RCX, -48);
    code.shl_imm(RCX, 5);
    code.add_imm8(RCX, 16);
    code.add_rr(RCX, RAX);
    code.load(RDX, -16);
    code.store_ptr_disp32(RCX, 8, RDX);
    code.mov_imm32(R8, TAG_STR as i32);
    code.store_ptr_disp32(RCX, 0, R8);
    code.load(RDX, -24);
    code.store_ptr_disp32(RCX, 24, RDX);
    code.load(RDX, -32);
    code.store_ptr_disp32(RCX, 16, RDX);
    code.load(RAX, -80);
    code.leave();
    code.ret();
}

/// `rt_store_set`: `rax` is a set descriptor, `rcx` the position, `rdx` the
/// value word, `rdi` its tag and `r8` the source line. A position outside
/// `0..=len` is `E049`. The interpreter's rules are kept exactly: writing the
/// member that already sits at the position changes nothing; writing a member
/// that lives elsewhere drops the written position (the member keeps the place
/// it first had); a position just past the end appends; any other position is
/// replaced. The result is a new descriptor in `rax`.
fn emit_store_set(code: &mut Code) {
    code.label("rt_store_set");
    code.push(RBP);
    code.mov_rbp_rsp();
    code.sub_rsp(128);
    code.store(-8, RAX);
    code.store(-16, RCX);
    code.store(-24, RDX);
    code.store(-32, RDI);
    code.store(-40, R8);
    code.load_ptr_disp32(R9, RAX, 8);
    code.store(-48, R9);
    code.load(RCX, -16);
    code.cmp_imm32(RCX, 0);
    code.jcc(CC_L, "store_set_bad");
    code.load(RDX, -48);
    code.cmp_rr(RCX, RDX);
    code.jcc(CC_A, "store_set_bad");
    // Where else does this member already live, if anywhere?
    code.mov_imm32_zero(RCX, 0);
    code.store(-56, RCX);
    code.mov_imm64(RCX, u64::MAX);
    code.store(-64, RCX);
    code.label("store_set_find");
    code.load(RCX, -56);
    code.load(RDX, -48);
    code.cmp_rr(RCX, RDX);
    code.jcc(CC_AE, "store_set_found_done");
    code.load(RAX, -8);
    code.load(RCX, -56);
    code.shl_imm(RCX, 4);
    code.add_imm8(RCX, 16);
    code.add_rr(RCX, RAX);
    code.load_ptr_disp32(RDI, RCX, 0);
    code.load_ptr_disp32(RAX, RCX, 8);
    code.load(RDX, -24);
    code.call("rt_value_eq");
    code.test_rr(RAX, RAX);
    code.jcc(CC_NE, "store_set_duplicate");
    code.load(RCX, -56);
    code.inc(RCX);
    code.store(-56, RCX);
    code.jmp("store_set_find");
    code.label("store_set_duplicate");
    code.load(RCX, -56);
    code.store(-64, RCX);
    code.label("store_set_found_done");
    // The member already at the written position: nothing changes.
    code.load(RCX, -64);
    code.load(RDX, -16);
    code.cmp_rr(RCX, RDX);
    code.jcc(CC_E, "store_set_unchanged");
    // A duplicate elsewhere and the position just past the end: dropping the
    // written position would drop nothing, so the set stays as it is.
    code.load(RCX, -64);
    code.cmp_imm32(RCX, -1);
    code.jcc(CC_E, "store_set_replace_or_append");
    code.load(RDX, -16);
    code.load(RCX, -48);
    code.cmp_rr(RDX, RCX);
    code.jcc(CC_E, "store_set_unchanged");
    // Drop the written position.
    code.load(RCX, -48);
    code.dec(RCX);
    code.store(-72, RCX);
    code.mov_imm32(RCX, 1);
    code.store(-80, RCX);
    code.jmp("store_set_alloc");
    code.label("store_set_replace_or_append");
    code.load(RCX, -48);
    code.load(RDX, -16);
    code.cmp_rr(RDX, RCX);
    code.jcc(CC_NE, "store_set_replace");
    code.inc(RCX);
    code.label("store_set_replace");
    code.store(-72, RCX);
    code.mov_imm32_zero(RCX, 0);
    code.store(-80, RCX);
    code.label("store_set_alloc");
    code.load(RDI, -72);
    code.shl_imm(RDI, 4);
    code.add_imm8(RDI, 16);
    code.call("rt_alloc");
    code.store(-88, RAX);
    // Header: the set's tag, then the new length.
    code.load(RCX, -8);
    code.load_ptr(RDX, RCX);
    code.store_ptr_disp32(RAX, 0, RDX);
    code.load(RDX, -72);
    code.store_ptr_disp32(RAX, 8, RDX);
    code.mov_imm32_zero(RCX, 0);
    code.store(-96, RCX);
    code.label("store_set_copy");
    code.load(RCX, -96);
    code.load(RDX, -72);
    code.cmp_rr(RCX, RDX);
    code.jcc(CC_AE, "store_set_copied");
    // A removal copies from one position further on past the dropped slot.
    code.load(RDX, -80);
    code.test_rr(RDX, RDX);
    code.jcc(CC_E, "store_set_same_position");
    code.load(RCX, -96);
    code.load(RDX, -16);
    code.cmp_rr(RCX, RDX);
    code.jcc(CC_B, "store_set_copy_element");
    code.inc(RCX);
    code.jmp("store_set_copy_element");
    code.label("store_set_same_position");
    code.load(RCX, -96);
    code.load(RDX, -16);
    code.cmp_rr(RCX, RDX);
    code.jcc(CC_E, "store_set_write_value");
    code.label("store_set_copy_element");
    code.load(RAX, -8);
    code.shl_imm(RCX, 4);
    code.add_imm8(RCX, 16);
    code.add_rr(RCX, RAX);
    code.load(RAX, -88);
    code.load(RDX, -96);
    code.shl_imm(RDX, 4);
    code.add_imm8(RDX, 16);
    code.add_rr(RDX, RAX);
    code.load_ptr(R8, RCX);
    code.store_ptr_disp32(RDX, 0, R8);
    code.load_ptr_disp32(R8, RCX, 8);
    code.store_ptr_disp32(RDX, 8, R8);
    code.jmp("store_set_next");
    code.label("store_set_write_value");
    code.load(RAX, -88);
    code.load(RCX, -96);
    code.shl_imm(RCX, 4);
    code.add_imm8(RCX, 16);
    code.add_rr(RCX, RAX);
    code.load(RDX, -24);
    code.store_ptr_disp32(RCX, 8, RDX);
    code.load(RDX, -32);
    code.store_ptr_disp32(RCX, 0, RDX);
    code.label("store_set_next");
    code.load(RCX, -96);
    code.inc(RCX);
    code.store(-96, RCX);
    code.jmp("store_set_copy");
    code.label("store_set_copied");
    code.load(RAX, -88);
    code.leave();
    code.ret();
    code.label("store_set_unchanged");
    code.load(RAX, -8);
    code.leave();
    code.ret();
    code.label("store_set_bad");
    code.load(RAX, -16);
    code.load(RDI, -40);
    code.call("rt_panic_index");
    code.ud2();
}

/// `rt_panic_message`: `rax` is an interned `{ length, bytes }` descriptor;
/// write it and a newline to stderr, then exit 1. Codegen uses it for a
/// diagnostic it can build completely at compile time (the interpreter's
/// `E042` for ordering a collection), so the text is byte-identical.
fn emit_panic_message(code: &mut Code, newline: u32) {
    code.label("rt_panic_message");
    code.push(RBP);
    code.mov_rbp_rsp();
    code.sub_rsp(32);
    code.store(-8, RAX);
    code.load_ptr_disp32(RCX, RAX, 0);
    code.store(-16, RCX);
    code.load(RAX, -8);
    code.add_imm8(RAX, 8);
    code.load(RCX, -16);
    code.call("rt_write_err");
    write(code, newline, 1, true);
    code.mov_imm32_zero(RAX, 1);
    code.call("rt_exit");
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
    index_plain_prefix: u32,
    index_plain_suffix: u32,
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

    // `rax` is the offending index of a builtin call. A call carries no span,
    // so the line clause is left out -- the same rule the arithmetic panics
    // follow -- and the interpreter's sentence is otherwise unchanged.
    code.label("rt_panic_index_plain");
    code.push(RBP);
    code.mov_rbp_rsp();
    code.sub_rsp(16);
    code.store(-16, RAX);
    write(code, index_plain_prefix, "E049 အညွှန်း ".len(), true);
    code.load(RAX, -16);
    code.call("rt_write_int_err");
    write(
        code,
        index_plain_suffix,
        " သည် အကွာအဝေးပြင်ပတွင် ရှိနေပါသည်။".len(),
        true,
    );
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
