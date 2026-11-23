//! `rt_rem_float`: the interpreter's float `%`, which is `f64::rem_euclid` --
//! the truncated remainder (`fmod`) made non-negative.
//!
//! x86-64 has no SSE remainder instruction, so the routine uses the x87
//! `fprem`, which computes the same truncated remainder exactly, and repeats
//! while it reports an incomplete reduction (C2). The x87 registers are wider
//! than a double, but the true remainder is exactly representable as one, so
//! storing it loses nothing.
//!
//! Entry: the dividend's bits in `rax`, the divisor's in `rcx`. Exit: the
//! result's bits in `rax`. A zero divisor is the interpreter's `E039`, which
//! `rt_panic_rem_float` writes (without the line number compiled code has no
//! span for) before exiting 1.

use crate::backend::x86_64::encode::*;
use crate::backend::x86_64::Data;

const MESSAGE: &str = "E039 သုညဖြင့် ကြွင်းကိန်းရှာ၍မရပါ။";

pub(crate) fn emit(code: &mut Code, data: &mut Data) {
    let message = data.intern(MESSAGE);
    let newline = data.intern("\n");

    code.label("rt_rem_float");
    code.push(RBP);
    code.mov_rbp_rsp();

    // A zero divisor (or `-0.0`) is `E039`; a NaN divisor is not zero --
    // parity is set, so it takes the ordinary path and comes back NaN.
    code.movq_xmm_gpr(XMM1, RCX);
    code.xorpd(XMM2, XMM2);
    code.ucomisd(XMM1, XMM2);
    code.jcc(CC_P, "rf_nonzero");
    code.jcc(CC_NE, "rf_nonzero");
    code.call("rt_panic_rem_float");
    code.ud2();
    code.label("rf_nonzero");

    code.sub_rsp(16);
    code.store_rsp(0, RAX);
    code.store_rsp(8, RCX);
    // `fprem` reads ST(0) as the dividend and ST(1) as the divisor, so the
    // divisor is pushed first.
    code.fld_rsp(8);
    code.fld_rsp(0);
    code.label("rf_reduce");
    code.fprem();
    code.fnstsw_ax();
    code.test_ah_imm8(4);
    code.jcc(CC_NE, "rf_reduce");
    code.fstp_rsp(0);
    code.fstp_st0();
    code.load_rsp(RAX, 0);
    code.add_rsp(16);

    // `rem_euclid`: a negative remainder gains |divisor|; zero reproduces the
    // sign it was given, NaN stays NaN, and a positive remainder is returned
    // unchanged.
    code.movq_xmm_gpr(XMM0, RAX);
    code.xorpd(XMM1, XMM1);
    code.ucomisd(XMM0, XMM1);
    code.jcc(CC_P, "rf_done");
    code.jcc(CC_AE, "rf_done");
    code.mov_imm64(R11, 0x7fff_ffff_ffff_ffff);
    code.and_rr(RCX, R11);
    code.movq_xmm_gpr(XMM1, RCX);
    code.addsd(XMM0, XMM1);
    code.movq_gpr_xmm(RAX, XMM0);
    code.label("rf_done");
    code.leave();
    code.ret();

    code.label("rt_panic_rem_float");
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
