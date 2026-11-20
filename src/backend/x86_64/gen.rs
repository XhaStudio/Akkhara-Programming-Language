//! MIR to x86-64 for the native slice.
//!
//! Every function is emitted as its own routine with its own frame: one 64-bit
//! slot per temporary and per binding, `[rbp - 8 * (n + 1)]`, and the frame
//! rounded up to a multiple of 16 so `rsp` is 16-byte aligned at every `call`.
//!
//! Calling convention (private to the generated code):
//!
//! * arguments 1-6 in `rdi`, `rsi`, `rdx`, `rcx`, `r8`, `r9`; more is refused;
//! * the result comes back in `rax`;
//! * the callee keeps nothing else: `rbp` is pushed and restored, everything
//!   else is caller-saved, so a caller cannot rely on any register over a call;
//! * `rsp` is 16-byte aligned before every `call`, and the callee's frame is a
//!   multiple of 16, so the guarantee holds all the way down;
//! * the caller removes its arguments (they live in registers, so there is
//!   nothing to remove beyond the scratch area it reserved before evaluating
//!   them).
//!
//! Arguments are evaluated left to right, the order the interpreter uses, into
//! a scratch area on the stack; only then are they loaded into registers. That
//! keeps a nested call inside an argument expression 16-byte aligned.

use std::collections::{HashMap, HashSet};

use super::analyze;
use super::encode::*;
use super::unsupported;
use super::{Kind, Typed, TypedProgram, MAX_ARGS};
use crate::ir::mir::*;
use crate::runtime::{self, Target};
use crate::scope::symbol::VarId;

/// The registers arguments travel in, in order.
const ARGS: [u8; MAX_ARGS] = [RDI, RSI, RDX, RCX, R8, R9];

/// A function this executable imports. Windows needs its table; the Linux
/// image has none.
#[derive(Debug, Clone)]
pub(crate) struct Import {
    pub(crate) dll: &'static str,
    pub(crate) name: &'static str,
    /// Offset in `.data` of the eight-byte slot the loader fills in.
    pub(crate) slot: u32,
}

/// A finished program image, before a platform writer turns it into a file.
pub(crate) struct Image {
    /// Entry point: an offset inside `text` (`<main>` is emitted first, so 0).
    pub(crate) entry: u64,
    pub(crate) text: Vec<u8>,
    pub(crate) data: Vec<u8>,
    /// `(position of an imm64 in text, offset in data)`
    pub(crate) data_fixups: Vec<(usize, u32)>,
    pub(crate) imports: Vec<Import>,
}

impl Image {
    /// Fill in every immediate that names a data address, once the writer
    /// knows where the data lands in memory.
    pub(crate) fn patch(&mut self, data_address: u64) {
        for (position, offset) in &self.data_fixups {
            let value = data_address + *offset as u64;
            self.text[*position..*position + 8].copy_from_slice(&value.to_le_bytes());
        }
    }
}

/// The constant pool: every string literal as `{ length: u64, bytes... }`, so
/// printing only needs the address.
#[derive(Default)]
pub(crate) struct Data {
    bytes: Vec<u8>,
    interned: HashMap<String, u32>,
}

impl Data {
    pub(crate) fn intern(&mut self, text: &str) -> u32 {
        if let Some(offset) = self.interned.get(text) {
            return *offset;
        }
        while self.bytes.len() % 8 != 0 {
            self.bytes.push(0);
        }
        let offset = self.bytes.len() as u32;
        self.bytes
            .extend_from_slice(&(text.len() as u64).to_le_bytes());
        self.bytes.extend_from_slice(text.as_bytes());
        self.interned.insert(text.to_string(), offset);
        offset
    }

    /// Reserve an eight-byte slot for a value the loader fills in.
    pub(crate) fn reserve_slot(&mut self) -> u32 {
        while self.bytes.len() % 8 != 0 {
            self.bytes.push(0);
        }
        let offset = self.bytes.len() as u32;
        self.bytes.extend_from_slice(&[0; 8]);
        offset
    }
}

/// Compile a whole MIR program into one image.
pub(crate) fn compile(program: &MirProgram, target: Target) -> Result<Image, String> {
    let typed = analyze(program)?;

    let mut code = Code::new();
    let mut data = Data::default();
    let mut imports = Vec::new();
    // Label names must be unique across the whole image: `finish` resolves a
    // branch to the first label that matches, so a per-function counter would
    // let a jump in one function land in another.
    let mut site = 0u32;

    for (index, function) in program.functions.iter().enumerate() {
        let typed_fn = &typed.functions[index];
        let slots = slots(function, typed_fn);
        let mut gen = Gen {
            kinds: typed_fn.temps.clone(),
            var_kinds: typed_fn.vars.clone(),
            slots: slots.temps,
            var_slots: slots.vars,
            callees: typed.clone_indexes(),
            site,
        };

        code.label(&function_label(index));
        code.push(RBP);
        code.mov_rbp_rsp();
        code.sub_rsp(slots.frame);
        // The parameters arrive in registers and live in frame slots from the
        // first instruction on.
        for (position, param) in function.params.iter().enumerate() {
            if let Some(slot) = gen.var_slots.get(param) {
                code.store(*slot, ARGS[position]);
            }
        }

        for block in &function.blocks {
            code.label(&block_label(index, block.id));
            for instruction in &block.insts {
                gen.instruction(&mut code, &mut data, instruction)?;
            }
            match &block.term {
                Some(Terminator::Goto(next)) => code.jmp(&block_label(index, *next)),
                Some(Terminator::Branch {
                    cond,
                    then_block,
                    else_block,
                }) => {
                    gen.load(&mut code, &mut data, cond);
                    code.test_rr(RAX, RAX);
                    code.jcc(CC_NE, &block_label(index, *then_block));
                    code.jmp(&block_label(index, *else_block));
                }
                Some(Terminator::Return(Some(value))) => {
                    gen.load(&mut code, &mut data, value);
                    code.leave();
                    code.ret();
                }
                Some(Terminator::Return(None)) => {
                    code.mov_imm32_zero(RAX, 0);
                    code.leave();
                    code.ret();
                }
                Some(Terminator::Exit) => {
                    code.mov_imm32_zero(RAX, 0);
                    code.call("rt_exit");
                }
                None => {}
            }
        }
        site = gen.site;
    }

    runtime::emit(&mut code, &mut data, target, &mut imports);
    let (text, data_fixups) = code.finish()?;
    Ok(Image {
        entry: 0,
        text,
        data: data.bytes,
        data_fixups,
        imports,
    })
}

fn function_label(index: usize) -> String {
    format!("fn{index}")
}

fn block_label(index: usize, block: BlockId) -> String {
    format!("fn{index}.bb{block}")
}

impl TypedProgram {
    /// Call targets by name, for the code generator.
    fn clone_indexes(&self) -> HashMap<String, usize> {
        self.names
            .iter()
            .enumerate()
            .map(|(index, name)| (name.clone(), index))
            .collect()
    }
}

struct Slots {
    temps: HashMap<Temp, i32>,
    vars: HashMap<VarId, i32>,
    frame: u32,
}

fn slots(function: &MirFn, typed: &Typed) -> Slots {
    let mut temps = HashMap::new();
    let mut vars = HashMap::new();
    let mut used_temps = HashSet::new();
    let mut used_vars = HashSet::new();
    for param in &function.params {
        used_vars.insert(*param);
    }
    for block in &function.blocks {
        for instruction in &block.insts {
            if let Some(dest) = dest_of(instruction) {
                used_temps.insert(dest);
            }
            if let MirInst::StoreVar { var, .. } = instruction {
                used_vars.insert(*var);
            }
            for operand in operands_of(instruction) {
                match operand {
                    Operand::Temp(temp) => {
                        used_temps.insert(*temp);
                    }
                    Operand::Var(var) => {
                        used_vars.insert(*var);
                    }
                    _ => {}
                }
            }
        }
        if let Some(Terminator::Branch { cond, .. }) = &block.term {
            if let Operand::Temp(temp) = cond {
                used_temps.insert(*temp);
            }
            if let Operand::Var(var) = cond {
                used_vars.insert(*var);
            }
        }
        if let Some(Terminator::Return(Some(Operand::Temp(temp)))) = &block.term {
            used_temps.insert(*temp);
        }
        if let Some(Terminator::Return(Some(Operand::Var(var)))) = &block.term {
            used_vars.insert(*var);
        }
    }

    let mut frame = 0i32;
    let mut sorted: Vec<Temp> = used_temps.into_iter().collect();
    sorted.sort_unstable();
    for temp in sorted {
        if typed.temps.contains_key(&temp) {
            frame += 8;
            temps.insert(temp, -frame);
        }
    }
    let mut sorted: Vec<VarId> = used_vars.into_iter().collect();
    sorted.sort_unstable();
    for var in sorted {
        if typed.vars.contains_key(&var) {
            frame += 8;
            vars.insert(var, -frame);
        }
    }

    // Keep rsp 16-byte aligned at every `call`.
    let frame = ((frame as u32 + 15) / 16) * 16;
    Slots {
        temps,
        vars,
        frame: frame.max(16),
    }
}

fn dest_of(instruction: &MirInst) -> Option<Temp> {
    match instruction {
        MirInst::Binary { dest, .. }
        | MirInst::Compare { dest, .. }
        | MirInst::Not { dest, .. }
        | MirInst::Neg { dest, .. }
        | MirInst::Move { dest, .. }
        | MirInst::Call { dest, .. } => Some(*dest),
        _ => None,
    }
}

fn operands_of(instruction: &MirInst) -> Vec<&Operand> {
    match instruction {
        MirInst::Binary { lhs, rhs, .. } | MirInst::Compare { lhs, rhs, .. } => vec![lhs, rhs],
        MirInst::Call { args, .. } => args.iter().collect(),
        MirInst::Not { value, .. }
        | MirInst::Neg { value, .. }
        | MirInst::Move { value, .. }
        | MirInst::StoreVar { value, .. } => vec![value],
        MirInst::Print(value) | MirInst::Eval(value) => vec![value],
        _ => Vec::new(),
    }
}

/// The condition code for a float comparison, taken after `ucomisd lhs, rhs`
/// with the unordered case already branched away.
fn float_cc(op: &str) -> u8 {
    match op {
        "==" => CC_E,
        "!=" => CC_NE,
        "<" => CC_B,
        "<=" => CC_BE,
        ">" => CC_A,
        ">=" => CC_AE,
        _ => CC_E,
    }
}

fn compare_cc(op: &str) -> Option<u8> {
    Some(match op {
        "==" => CC_E,
        "!=" => CC_NE,
        "<" => CC_L,
        "<=" => CC_LE,
        ">" => CC_G,
        ">=" => CC_GE,
        _ => return None,
    })
}

struct Gen {
    kinds: HashMap<Temp, Kind>,
    var_kinds: HashMap<VarId, Kind>,
    slots: HashMap<Temp, i32>,
    var_slots: HashMap<VarId, i32>,
    callees: HashMap<String, usize>,
    /// A counter for labels only one instruction uses.
    site: u32,
}

impl Gen {
    fn kind(&self, operand: &Operand) -> Result<Kind, String> {
        match operand {
            Operand::Const(Const::Int(_)) => Ok(Kind::Int),
            Operand::Const(Const::Bool(_)) => Ok(Kind::Bool),
            Operand::Const(Const::Str(_)) => Ok(Kind::Str),
            Operand::Const(Const::Float(_)) => Ok(Kind::Float),
            Operand::Const(Const::Null) => Err(unsupported("null")),
            Operand::Temp(temp) => self
                .kinds
                .get(temp)
                .copied()
                .ok_or_else(|| unsupported(&format!("the value in t{}", temp))),
            Operand::Var(var) => self
                .var_kinds
                .get(var)
                .copied()
                .ok_or_else(|| unsupported(&format!("the binding {}", var))),
            Operand::Name(name) => Err(unsupported(&format!("the run-time name \"{}\"", name))),
            Operand::Fn(_) => Err(unsupported("a function value")),
        }
    }

    /// Load `operand` into `rax`.
    fn load(&self, code: &mut Code, data: &mut Data, operand: &Operand) {
        match operand {
            Operand::Const(Const::Int(value)) => code.mov_imm64(RAX, *value as u64),
            Operand::Const(Const::Bool(value)) => code.mov_imm64(RAX, u64::from(*value)),
            Operand::Const(Const::Str(text)) => {
                let descriptor = data.intern(text);
                code.mov_data_address(RAX, descriptor);
            }
            Operand::Const(Const::Float(value)) => code.mov_imm64(RAX, value.to_bits()),
            Operand::Temp(temp) => code.load(RAX, self.slots[temp]),
            Operand::Var(var) => code.load(RAX, self.var_slots[var]),
            // `analyze` refuses this before code generation runs.
            Operand::Const(Const::Null) => {}
            Operand::Name(_) | Operand::Fn(_) => {}
        }
    }

    /// The next label number this instruction may need.
    fn next_site(&mut self) -> u32 {
        let site = self.site;
        self.site += 1;
        site
    }

    /// Load `operand` into SSE register `xmm` as a double, converting an int
    /// the way the interpreter promotes one.
    fn load_number(
        &self,
        code: &mut Code,
        data: &mut Data,
        operand: &Operand,
        xmm: u8,
    ) -> Result<(), String> {
        match self.kind(operand)? {
            Kind::Float => {
                self.load(code, data, operand);
                code.movq_xmm_gpr(xmm, RAX);
            }
            Kind::Int => {
                self.load(code, data, operand);
                code.cvtsi2sd(xmm, RAX);
            }
            other => {
                return Err(unsupported(&format!(
                    "arithmetic on a {} value",
                    other.name()
                )))
            }
        }
        Ok(())
    }

    fn store(&self, code: &mut Code, operand: &Operand) {
        match operand {
            Operand::Temp(temp) => code.store(self.slots[temp], RAX),
            Operand::Var(var) => code.store(self.var_slots[var], RAX),
            _ => {}
        }
    }

    fn instruction(
        &mut self,
        code: &mut Code,
        data: &mut Data,
        instruction: &MirInst,
    ) -> Result<(), String> {
        match instruction {
            MirInst::Move { dest, value } => {
                self.load(code, data, value);
                self.store(code, &Operand::Temp(*dest));
            }
            MirInst::Binary {
                dest, op, lhs, rhs, ..
            } => {
                let result = self.kinds.get(dest).copied().ok_or_else(|| {
                    unsupported(&format!(
                        "the value in t{} (its kind could not be worked out)",
                        dest
                    ))
                })?;
                if *op == '/' || result == Kind::Float {
                    self.load_number(code, data, lhs, XMM0)?;
                    self.load_number(code, data, rhs, XMM1)?;
                    match op {
                        '+' => code.addsd(XMM0, XMM1),
                        '-' => code.subsd(XMM0, XMM1),
                        '*' => code.mulsd(XMM0, XMM1),
                        '/' => {
                            // The interpreter reports E037 for a zero float
                            // divisor. A NaN divisor is not zero (parity is
                            // set, so the panic is skipped).
                            let site = self.next_site();
                            let nonzero = format!("fdiv_nz_{site}");
                            // A zero divisor is E037; a NaN one merely comes
                            // out unordered, so parity skips the panic.
                            code.xorpd(XMM2, XMM2);
                            code.ucomisd(XMM1, XMM2);
                            code.jcc(CC_P, &nonzero);
                            code.jcc(CC_NE, &nonzero);
                            code.call("rt_panic_div_float");
                            code.ud2();
                            code.label(&nonzero);
                            code.divsd(XMM0, XMM1);
                        }
                        other => return Err(unsupported(&format!("the operator '{}'", other))),
                    }
                    code.movq_gpr_xmm(RAX, XMM0);
                    self.store(code, &Operand::Temp(*dest));
                    return Ok(());
                }
                self.load(code, data, lhs);
                code.push(RAX);
                self.load(code, data, rhs);
                code.mov_rr(RCX, RAX);
                code.pop(RAX);
                match op {
                    '+' => code.add_rr(RAX, RCX),
                    '-' => code.sub_rr(RAX, RCX),
                    '*' => code.imul_rr(RAX, RCX),
                    '%' => self.remainder(code),
                    '/' => {
                        // Two ints divide as doubles, and a zero divisor is
                        // the interpreter's E036 rather than a hardware trap.
                        let site = self.next_site();
                        let nonzero = format!("idiv_nz_{site}");
                        code.test_rr(RCX, RCX);
                        code.jcc(CC_NE, &nonzero);
                        code.call("rt_panic_div_int");
                        code.ud2();
                        code.label(&nonzero);
                        code.cvtsi2sd(XMM0, RAX);
                        code.cvtsi2sd(XMM1, RCX);
                        code.divsd(XMM0, XMM1);
                        code.movq_gpr_xmm(RAX, XMM0);
                    }
                    other => return Err(unsupported(&format!("the operator '{}'", other))),
                }
                self.store(code, &Operand::Temp(*dest));
            }
            MirInst::Compare {
                dest, op, lhs, rhs, ..
            } => {
                let cc = compare_cc(op)
                    .ok_or_else(|| unsupported(&format!("the comparison \"{}\"", op)))?;
                let left = self.kind(lhs)?;
                let right = self.kind(rhs)?;
                let ordered = !matches!(op.as_str(), "==" | "!=");
                let mixed_numbers = left != right && left.is_number() && right.is_number();
                if !ordered && mixed_numbers {
                    // `==` is the interpreter's structural comparison, where
                    // an int and a float are never equal (their printed forms
                    // always differ).
                    code.mov_imm32_zero(RAX, u32::from(op == "!="));
                    self.store(code, &Operand::Temp(*dest));
                    return Ok(());
                }
                if left == Kind::Float || right == Kind::Float {
                    if ordered {
                        self.load_number(code, data, lhs, XMM0)?;
                        self.load_number(code, data, rhs, XMM1)?;
                        let site = self.next_site();
                        let unordered = format!("fcmp_nan_{site}");
                        let done = format!("fcmp_done_{site}");
                        code.ucomisd(XMM0, XMM1);
                        code.jcc(CC_P, &unordered);
                        code.setcc(float_cc(op));
                        code.jmp(&done);
                        code.label(&unordered);
                        // Every ordering is false when a NaN is involved.
                        code.mov_imm32_zero(RAX, 0);
                        code.label(&done);
                    } else {
                        self.load(code, data, lhs);
                        code.push(RAX);
                        self.load(code, data, rhs);
                        code.mov_rr(RCX, RAX);
                        code.pop(RAX);
                        self.float_equality(code, op);
                    }
                    self.store(code, &Operand::Temp(*dest));
                    return Ok(());
                }
                self.load(code, data, lhs);
                code.push(RAX);
                self.load(code, data, rhs);
                code.mov_rr(RCX, RAX);
                code.pop(RAX);
                code.cmp_rr(RAX, RCX);
                code.setcc(cc);
                self.store(code, &Operand::Temp(*dest));
            }
            MirInst::Not { dest, value } => {
                self.load(code, data, value);
                code.mov_imm32(RCX, 1);
                code.xor_rr(RAX, RCX);
                self.store(code, &Operand::Temp(*dest));
            }
            MirInst::Neg { dest, value } => {
                if self.kind(value)? == Kind::Float {
                    // Flip the sign bit: `-0.0` has to stay a float.
                    self.load(code, data, value);
                    code.mov_imm64(RCX, 0x8000_0000_0000_0000);
                    code.xor_rr(RAX, RCX);
                } else {
                    self.load(code, data, value);
                    code.neg(RAX);
                }
                self.store(code, &Operand::Temp(*dest));
            }
            MirInst::StoreVar {
                var,
                op: None,
                value,
            } => {
                self.load(code, data, value);
                self.store(code, &Operand::Var(*var));
            }
            MirInst::StoreVar {
                var,
                op: Some(op),
                value,
            } => {
                let held = self.var_kinds[var];
                if held == Kind::Float {
                    // `x += 1` on a float adds the int as a double.
                    self.load_number(code, data, value, XMM1)?;
                    code.load(RAX, self.var_slots[var]);
                    code.movq_xmm_gpr(XMM0, RAX);
                    match op {
                        '+' => code.addsd(XMM0, XMM1),
                        '-' => code.subsd(XMM0, XMM1),
                        '*' => code.mulsd(XMM0, XMM1),
                        other => return Err(unsupported(&format!("the operator '{}='", other))),
                    }
                    code.movq_gpr_xmm(RAX, XMM0);
                    self.store(code, &Operand::Var(*var));
                    return Ok(());
                }
                self.load(code, data, value);
                code.mov_rr(RCX, RAX);
                code.load(RAX, self.var_slots[var]);
                match op {
                    '+' => code.add_rr(RAX, RCX),
                    '-' => code.sub_rr(RAX, RCX),
                    '*' => code.imul_rr(RAX, RCX),
                    other => return Err(unsupported(&format!("the operator '{}='", other))),
                }
                self.store(code, &Operand::Var(*var));
            }
            MirInst::Call { dest, name, args } => {
                let callee = *self
                    .callees
                    .get(name)
                    .ok_or_else(|| unsupported(&format!("a function call to \"{}\"", name)))?;
                self.call(code, data, callee, args);
                self.store(code, &Operand::Temp(*dest));
            }
            // The value was computed for its effect; nothing to emit.
            MirInst::Eval(_) => {}
            MirInst::Print(value) => {
                let kind = self.kind(value)?;
                self.load(code, data, value);
                match kind {
                    Kind::Int => code.call("rt_print_int"),
                    Kind::Float => code.call("rt_print_float"),
                    Kind::Bool => code.call("rt_print_bool"),
                    Kind::Str => code.call("rt_print_str"),
                }
            }
            other => return Err(unsupported(&format!("{:?}", other))),
        }
        Ok(())
    }

    /// Call `callee(index)`: evaluate the arguments left to right into a
    /// scratch area, load them into the argument registers, and call.
    fn call(&mut self, code: &mut Code, data: &mut Data, index: usize, args: &[Operand]) {
        // The scratch area is a multiple of 16 so a call made while an earlier
        // argument is being evaluated still sees an aligned stack.
        let area = (((args.len() as u32) * 8 + 15) / 16) * 16;
        if area > 0 {
            code.sub_rsp(area);
        }
        for (position, arg) in args.iter().enumerate() {
            self.load(code, data, arg);
            code.store_rsp((position * 8) as u8, RAX);
        }
        for (position, _) in args.iter().enumerate() {
            code.load_rsp(ARGS[position], (position * 8) as u8);
        }
        if area > 0 {
            code.add_rsp(area);
        }
        code.call(&function_label(index));
    }

    /// Equality between two floats, with the bits of both in `rax` and `rcx`.
    /// The interpreter compares the printed forms, so `0.0` and `-0.0` differ
    /// (their bits differ too) while `NaN` equals `NaN` (both print `NaN`,
    /// although their bits need not match). Leaves 0 or 1 in `rax`.
    fn float_equality(&mut self, code: &mut Code, op: &str) {
        let site = self.next_site();
        let equal = format!("feq_eq_{site}");
        let different = format!("feq_ne_{site}");
        let done = format!("feq_done_{site}");
        code.cmp_rr(RAX, RCX);
        code.jcc(CC_E, &equal);
        // Not the same bits: equal only if both are NaN.
        code.mov_imm64(R11, 0x7fff_ffff_ffff_ffff);
        code.mov_rr(R10, RAX);
        code.and_rr(R10, R11);
        code.mov_imm64(R11, 0x7ff0_0000_0000_0000);
        code.cmp_rr(R10, R11);
        code.jcc(CC_LE, &different);
        code.mov_rr(R10, RCX);
        code.mov_imm64(R11, 0x7fff_ffff_ffff_ffff);
        code.and_rr(R10, R11);
        code.mov_imm64(R11, 0x7ff0_0000_0000_0000);
        code.cmp_rr(R10, R11);
        code.jcc(CC_G, &equal);
        code.jmp(&different);
        code.label(&equal);
        code.mov_imm32_zero(RAX, 1);
        code.jmp(&done);
        code.label(&different);
        code.mov_imm32_zero(RAX, 0);
        code.label(&done);
        if op == "!=" {
            code.mov_imm32(RCX, 1);
            code.xor_rr(RAX, RCX);
        }
    }

    /// `%` as the interpreter's `rem_euclid`: a non-negative remainder. The
    /// divisor is in `rcx` and the dividend in `rax` on entry; the result is
    /// left in `rax`.
    fn remainder(&mut self, code: &mut Code) {
        let site = self.site;
        self.site += 1;
        let add = format!("rem_a_{}", site);
        let done = format!("rem_d_{}", site);
        let nonzero = format!("rem_n_{}", site);

        // A zero divisor panics. It has to be a `call`, not a jump, or the
        // panic routine would run with the stack 8 bytes off its aligned
        // position.
        code.test_rr(RCX, RCX);
        code.jcc(CC_NE, &nonzero);
        code.call("rt_panic_zero");
        code.ud2();
        code.label(&nonzero);
        code.cqo();
        code.idiv(RCX);
        // rdx = a % b; make it non-negative the way `rem_euclid` does.
        code.test_rr(RDX, RDX);
        code.jcc(CC_NS, &done);
        code.mov_rr(R10, RCX);
        code.test_rr(R10, R10);
        code.jcc(CC_NS, &add);
        code.neg(R10);
        code.label(&add);
        code.add_rr(RDX, R10);
        code.label(&done);
        code.mov_rr(RAX, RDX);
    }
}
