//! `print` for the native slice: an int in decimal, a bool as `true`/`false`,
//! a string as its bytes, each followed by the newline the interpreter's
//! `print` statement adds.
//!
//! Values are one word: a string is the address of `{ length: u64, bytes... }`
//! in the constant pool, so printing a string is one `write` of a known length.

use crate::backend::x86_64::{Code, Data, Import, R10, R11, R8, R9, RAX, RBP, RCX, RDI, RDX, RSI};
use crate::backend::x86_64::{CC_E, CC_NE, CC_NS};
use crate::runtime::Target;

/// Where the platform shims live: offsets in `.data` of the loader-filled
/// slots, or `None` on Linux where the shims are syscalls.
#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct Platform {
    pub(crate) handle: Option<u32>,
    pub(crate) write: Option<u32>,
    pub(crate) exit: Option<u32>,
}

/// Reserve the slots the Windows shims call through. They are reserved first
/// and contiguously, because the image writer builds one import descriptor
/// whose `FirstThunk` array is exactly these slots.
pub(crate) fn declare_imports(
    data: &mut Data,
    imports: &mut Vec<Import>,
    target: Target,
) -> Platform {
    match target {
        Target::LinuxX86_64 => Platform::default(),
        Target::WindowsX86_64 => {
            let handle = data.reserve_slot();
            imports.push(Import {
                dll: "kernel32.dll",
                name: "GetStdHandle",
                slot: handle,
            });
            let write = data.reserve_slot();
            imports.push(Import {
                dll: "kernel32.dll",
                name: "WriteFile",
                slot: write,
            });
            let exit = data.reserve_slot();
            imports.push(Import {
                dll: "kernel32.dll",
                name: "ExitProcess",
                slot: exit,
            });
            Platform {
                handle: Some(handle),
                write: Some(write),
                exit: Some(exit),
            }
        }
    }
}

/// `STD_OUTPUT_HANDLE` and `STD_ERROR_HANDLE` for `GetStdHandle`, and the
/// matching Linux file descriptors.
const STDOUT: (u32, u32) = (1, -11i32 as u32);
const STDERR: (u32, u32) = (2, -12i32 as u32);

/// `write(ptr = rax, length = rcx)` to stdout and to stderr, then
/// `exit(code = rax)`.
pub(crate) fn emit_write_and_exit(code: &mut Code, platform: &Platform) {
    emit_write(code, platform, "rt_write", STDOUT);
    emit_write(code, platform, "rt_write_err", STDERR);

    code.label("rt_exit");
    match platform.exit {
        None => {
            code.mov_rr(RDI, RAX);
            code.mov_imm32_zero(RAX, 60);
            code.syscall();
            code.ud2();
        }
        Some(exit) => {
            code.push(RBP);
            code.mov_rbp_rsp();
            code.mov_rr(RCX, RAX);
            call_import(code, exit);
            code.ud2();
        }
    }
}

/// One `write` routine: `write(pointer = rax, length = rcx)` to `stream`.
fn emit_write(code: &mut Code, platform: &Platform, label: &str, stream: (u32, u32)) {
    code.label(label);
    match platform.handle {
        None => {
            // Linux: write(fd, ptr, len), by syscall.
            code.mov_rr(RDX, RCX);
            code.mov_rr(RSI, RAX);
            code.mov_imm32_zero(RDI, stream.0);
            code.mov_imm32_zero(RAX, 1);
            code.syscall();
            code.ret();
        }
        Some(handle) => {
            // Windows x64: the arguments go in rcx/rdx/r8/r9, the fifth on the
            // stack. The frame is 0x30 so both calls stay 16-byte aligned.
            code.push(RBP);
            code.mov_rbp_rsp();
            code.sub_rsp(0x30);
            code.mov_rr(R8, RCX);
            code.mov_rr(RDX, RAX);
            code.mov_imm32_zero(RCX, stream.1);
            call_import(code, handle);
            code.mov_rr(RCX, RAX);
            code.lea_disp8(R9, RBP, -8);
            code.store_zero_rsp(0x20);
            if let Some(write) = platform.write {
                call_import(code, write);
            }
            code.leave();
            code.ret();
        }
    }
}

fn call_import(code: &mut Code, slot: u32) {
    code.mov_data_address(RAX, slot);
    code.load_ptr(RAX, RAX);
    code.call_rax();
}

/// `rt_print_int`: decimal, with a leading `-` and the print newline.
pub(crate) fn emit_int(code: &mut Code, data: &mut Data) {
    let newline = data.intern("\n");
    code.label("rt_print_int");
    code.push(RBP);
    code.mov_rbp_rsp();
    code.sub_rsp(64);
    code.lea_disp8(RSI, RBP, -1);
    code.mov_rr(R10, RAX);
    code.mov_imm32_zero(R11, 0);
    code.test_rr(R10, R10);
    code.jcc(CC_NS, "int_abs");
    code.neg(R10);
    code.mov_imm32_zero(R11, 1);
    code.label("int_abs");
    code.mov_rr(RAX, R10);
    code.mov_imm32(RCX, 10);
    code.label("int_loop");
    code.xor_rr(RDX, RDX);
    code.div(RCX);
    code.add_imm8(RDX, b'0' as i8);
    code.store_byte(RSI, RDX);
    code.dec(RSI);
    code.test_rr(RAX, RAX);
    code.jcc(CC_NE, "int_loop");
    code.test_rr(R11, R11);
    code.jcc(CC_E, "int_done");
    code.store_byte_imm(RSI, b'-');
    code.dec(RSI);
    code.label("int_done");
    code.lea_disp8(RAX, RSI, 1);
    code.mov_rr(RCX, RBP);
    code.sub_rr(RCX, RSI);
    code.sub_imm8(RCX, 1);
    code.call("rt_write");
    write_newline(code, newline);
    code.leave();
    code.ret();
}

pub(crate) fn emit_bool(code: &mut Code, data: &mut Data) {
    // The interpreter prints booleans capitalized, so the generated program does too.
    let yes = data.intern("True");
    let no = data.intern("False");
    let newline = data.intern("\n");
    code.label("rt_print_bool");
    code.push(RBP);
    code.mov_rbp_rsp();
    code.test_rr(RAX, RAX);
    code.jcc(CC_E, "bool_false");
    code.mov_data_address(RAX, yes + 8);
    code.mov_imm32(RCX, 4);
    code.call("rt_write");
    code.jmp("bool_end");
    code.label("bool_false");
    code.mov_data_address(RAX, no + 8);
    code.mov_imm32(RCX, 5);
    code.call("rt_write");
    code.label("bool_end");
    write_newline(code, newline);
    code.pop(RBP);
    code.ret();
}

pub(crate) fn emit_str(code: &mut Code, data: &mut Data) {
    let newline = data.intern("\n");
    code.label("rt_print_str");
    code.push(RBP);
    code.mov_rbp_rsp();
    code.load_ptr(RCX, RAX);
    code.lea_disp8(RAX, RAX, 8);
    code.call("rt_write");
    write_newline(code, newline);
    code.pop(RBP);
    code.ret();
}

fn write_newline(code: &mut Code, newline: u32) {
    code.mov_data_address(RAX, newline + 8);
    code.mov_imm32(RCX, 1);
    code.call("rt_write");
}
