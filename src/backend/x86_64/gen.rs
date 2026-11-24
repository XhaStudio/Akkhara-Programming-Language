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
use super::{builtin, Builtin, Kind, TypedProgram, MAX_ARGS};
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

/// The zeroed arena the collection runtime bump-allocates from. It lives in
/// the image's writable data and is never grown or collected in this slice.
pub(crate) const ARENA_SIZE: u32 = 256 * 1024;

impl Data {
    /// Reserve `size` zeroed bytes and return their offset. The collection
    /// runtime writes descriptors here at run time.
    pub(crate) fn reserve_arena(&mut self, size: usize) -> u32 {
        while self.bytes.len() % 8 != 0 {
            self.bytes.push(0);
        }
        let offset = self.bytes.len() as u32;
        self.bytes.resize(self.bytes.len() + size, 0);
        offset
    }

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
    // A raise in flight: the text before its message, its identity and the
    // message itself. A zero prefix means "no raise pending".
    let error_prefix = data.reserve_slot();
    let error_identity = data.reserve_slot();
    let error_payload = data.reserve_slot();
    // The collection arena: its bump offset and the zeroed bytes themselves.
    let arena_used = data.reserve_slot();
    // Non-zero while `rt_str_float` runs: the float formatter then builds the
    // string instead of writing it to stdout.
    let str_mode = data.reserve_slot();
    let arena_start = data.reserve_arena(ARENA_SIZE as usize);
    let mut imports = Vec::new();
    // Label names must be unique across the whole image: `finish` resolves a
    // branch to the first label that matches, so a per-function counter would
    // let a jump in one function land in another.
    let mut site = 0u32;

    for (index, function) in program.functions.iter().enumerate() {
        let typed_fn = &typed.functions[index];
        let slots = slots(function);
        let mut gen = Gen {
            kinds: typed_fn.temps.clone(),
            var_kinds: typed_fn.vars.clone(),
            elems: typed_fn.elems.clone(),
            var_elems: typed_fn.var_elems.clone(),
            slots: slots.temps,
            var_slots: slots.vars,
            callees: typed.clone_indexes(),
            site,
            function: index,
            error_prefix,
            error_identity,
            error_payload,
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
                // An uncaught raise: the runtime prints the interpreter's
                // `E178` line and exits 1.
                Some(Terminator::ReportError) => {
                    code.call("rt_report_error");
                    code.ud2();
                }
                // Leave with the raise pending; the caller's check picks it up.
                Some(Terminator::Propagate) => {
                    code.mov_imm32_zero(RAX, 0);
                    code.leave();
                    code.ret();
                }
                None => {}
            }
        }
        site = gen.site;
    }

    runtime::emit(
        &mut code,
        &mut data,
        target,
        &mut imports,
        error_prefix,
        error_payload,
        str_mode,
        arena_used,
        arena_start,
        ARENA_SIZE,
    );
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

fn slots(function: &MirFn) -> Slots {
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
            if let MirInst::BindError { var } = instruction {
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
    // Every temporary a destination writes needs a slot even when nothing
    // ever reads it (the result of a discarded call has no kind): the store
    // happens regardless, and a missing slot used to panic the compiler.
    let mut sorted: Vec<Temp> = used_temps.into_iter().collect();
    sorted.sort_unstable();
    for temp in sorted {
        frame += 8;
        temps.insert(temp, -frame);
    }
    let mut sorted: Vec<VarId> = used_vars.into_iter().collect();
    sorted.sort_unstable();
    for var in sorted {
        frame += 8;
        vars.insert(var, -frame);
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
        | MirInst::Call { dest, .. }
        | MirInst::List { dest, .. }
        | MirInst::Tuple { dest, .. }
        | MirInst::Set { dest, .. }
        | MirInst::Dict { dest, .. }
        | MirInst::Index { dest, .. }
        | MirInst::Len { dest, .. }
        | MirInst::Items { dest, .. }
        | MirInst::ErrorMatches { dest, .. } => Some(*dest),
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
        | MirInst::StoreVar { value, .. }
        | MirInst::SetError { value, .. } => vec![value],
        MirInst::Print(value) | MirInst::Eval(value) => vec![value],
        MirInst::List { items, .. } | MirInst::Tuple { items, .. } | MirInst::Set { items, .. } => {
            items.iter().collect()
        }
        MirInst::Dict { pairs, .. } => pairs.iter().flat_map(|(key, value)| [key, value]).collect(),
        MirInst::Index { base, indices, .. } => {
            std::iter::once(base).chain(indices.iter()).collect()
        }
        MirInst::Len { value, .. } | MirInst::Items { value, .. } => vec![value],
        MirInst::StoreIndex {
            base,
            indices,
            value,
            ..
        } => std::iter::once(base)
            .chain(indices.iter())
            .chain(std::iter::once(value))
            .collect(),
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

/// A set member or dict key in literal form, for the compile-time
/// deduplication the interpreter's insertion-ordered containers need.
#[derive(PartialEq, Eq)]
enum DedupKey {
    Int(i64),
    Float(u64),
    Bool(bool),
    Str(String),
}

fn dedup_key(operand: &Operand) -> Result<DedupKey, String> {
    match operand {
        Operand::Const(Const::Int(value)) => Ok(DedupKey::Int(*value)),
        Operand::Const(Const::Float(value)) => Ok(DedupKey::Float(value.to_bits())),
        Operand::Const(Const::Bool(value)) => Ok(DedupKey::Bool(*value)),
        Operand::Const(Const::Str(value)) => Ok(DedupKey::Str(value.clone())),
        _ => Err(unsupported(
            "a set built from run-time values (only literal members are in the slice)",
        )),
    }
}

/// Deduplicate a set literal the way the interpreter's insertion-ordered set
/// does: a value keeps the position where it first appeared.
fn dedup_set(items: &[Operand]) -> Result<Vec<Operand>, String> {
    let mut out = Vec::new();
    let mut seen = Vec::new();
    for item in items {
        let key = dedup_key(item)?;
        if !seen.contains(&key) {
            seen.push(key);
            out.push(item.clone());
        }
    }
    Ok(out)
}

/// Deduplicate a dict literal: an existing key keeps its first position and
/// takes the last value, as `IndexMap::insert` does.
fn dedup_dict(pairs: &[(Operand, Operand)]) -> Result<Vec<(Operand, Operand)>, String> {
    let mut out: Vec<(Operand, Operand)> = Vec::new();
    for (key, value) in pairs {
        let Operand::Const(Const::Str(text)) = key else {
            return Err(unsupported(
                "a dict with a key computed at run time (only string literals are in the slice)",
            ));
        };
        if let Some(slot) = out.iter_mut().find(
            |(existing, _)| matches!(existing, Operand::Const(Const::Str(other)) if other == text),
        ) {
            slot.1 = value.clone();
        } else {
            out.push((key.clone(), value.clone()));
        }
    }
    Ok(out)
}

struct Gen {
    kinds: HashMap<Temp, Kind>,
    var_kinds: HashMap<VarId, Kind>,
    /// The element shape of a collection temporary (see [`super::Typed`]).
    elems: HashMap<Temp, Vec<Kind>>,
    /// The element shape of a collection binding.
    var_elems: HashMap<VarId, Vec<Kind>>,
    slots: HashMap<Temp, i32>,
    var_slots: HashMap<VarId, i32>,
    callees: HashMap<String, usize>,
    /// A counter for labels only one instruction uses.
    site: u32,
    /// The function being emitted, for a `check_error` jump's label.
    function: usize,
    /// The `.data` slots the raise runtime keeps its state in.
    error_prefix: u32,
    error_identity: u32,
    error_payload: u32,
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

    /// The first element kind of a collection operand, when the analyzer
    /// settled one. `sort` needs it to pick the runtime's comparison.
    fn elem_kind(&self, operand: &Operand) -> Option<Kind> {
        match operand {
            Operand::Temp(temp) => self
                .elems
                .get(temp)
                .and_then(|shape| shape.first().copied()),
            Operand::Var(var) => self
                .var_elems
                .get(var)
                .and_then(|shape| shape.first().copied()),
            _ => None,
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

    /// Load `operand` as one collection element: its word in `rdx` and its
    /// tag in `rdi`. A dynamic read already holds the address of the element's
    /// own `{ tag, word }` pair, so both come from that pair; otherwise the
    /// tag is the static kind. One register pair serves every literal and
    /// runtime write, so the tag always reaches the descriptor unchanged.
    fn load_tagged(
        &self,
        code: &mut Code,
        data: &mut Data,
        operand: &Operand,
    ) -> Result<(), String> {
        let kind = self.kind(operand)?;
        self.load(code, data, operand);
        if kind == Kind::Dynamic {
            code.mov_rr(R10, RAX);
            code.load_ptr_disp32(RDI, R10, 0);
            code.load_ptr_disp32(RDX, R10, 8);
        } else {
            code.mov_rr(RDX, RAX);
            code.mov_imm32(RDI, kind.tag() as i32);
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

    /// Load the value of one raise state slot into `rax`.
    fn load_error(&self, code: &mut Code, slot: u32) {
        code.mov_data_address(RCX, slot);
        code.load_ptr(RAX, RCX);
    }

    /// Store `rax` into one raise state slot.
    fn store_error(&self, code: &mut Code, slot: u32) {
        code.mov_data_address(RCX, slot);
        code.store_ptr(RCX, RAX);
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
                // Two strings join with `+`: the runtime copies both byte
                // ranges into a fresh arena string. Constant pairs are folded
                // when the optimiser runs, so this is the `--no-opt` path too,
                // and a run-time operand always comes here.
                if *op == '+' && result == Kind::Str {
                    self.load(code, data, lhs);
                    code.push(RAX);
                    self.load(code, data, rhs);
                    code.mov_rr(RCX, RAX);
                    code.pop(RAX);
                    code.call("rt_concat_str");
                    self.store(code, &Operand::Temp(*dest));
                    return Ok(());
                }
                if *op == '/' || result == Kind::Float {
                    self.load_number(code, data, lhs, XMM0)?;
                    self.load_number(code, data, rhs, XMM1)?;
                    match op {
                        '+' => code.addsd(XMM0, XMM1),
                        '-' => code.subsd(XMM0, XMM1),
                        '*' => code.mulsd(XMM0, XMM1),
                        '%' => {
                            // No SSE remainder: the runtime's x87 `fprem`
                            // routine computes the interpreter's `rem_euclid`
                            // over the two promoted doubles. Its own zero check
                            // never fires: the `guard_zero` emitted before this
                            // instruction already raised `E039`.
                            code.movq_gpr_xmm(RAX, XMM0);
                            code.movq_gpr_xmm(RCX, XMM1);
                            code.call("rt_rem_float");
                            self.store(code, &Operand::Temp(*dest));
                            return Ok(());
                        }
                        '/' => code.divsd(XMM0, XMM1),
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
                    '%' => self.remainder(code, false),
                    '/' => {
                        // The zero divisor was already refused by the
                        // `guard_zero` before this instruction.
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
                dest,
                op,
                lhs,
                rhs,
                line,
            } => {
                let cc = compare_cc(op)
                    .ok_or_else(|| unsupported(&format!("the comparison \"{}\"", op)))?;
                let left = self.kind(lhs)?;
                let right = self.kind(rhs)?;
                let ordered = !matches!(op.as_str(), "==" | "!=");
                if left.is_collection() || right.is_collection() {
                    // Equality compares the printed forms: two collections are
                    // equal when they have the same kind, length and elements
                    // (in order). A collection against any other kind prints
                    // differently every time, so it is simply unequal.
                    if ordered {
                        // The interpreter evaluates both operands and then
                        // reports E042. Do the same, with its exact text (the
                        // kinds are settled here, and the line traveled in the
                        // instruction), then exit 1.
                        self.load(code, data, lhs);
                        code.push(RAX);
                        self.load(code, data, rhs);
                        code.pop(RAX);
                        let message = format!(
                            "E042 လိုင်း {} တွင် {} နှင့် {} ကို \"{}\" ဖြင့် နှိုင်းယှဉ်၍မရပါ။",
                            line,
                            left.mm_name(),
                            right.mm_name(),
                            op
                        );
                        let offset = data.intern(&message);
                        code.mov_data_address(RAX, offset);
                        code.call("rt_panic_message");
                        code.ud2();
                        return Ok(());
                    }
                    if left.is_collection() && right.is_collection() {
                        self.load(code, data, lhs);
                        code.push(RAX);
                        self.load(code, data, rhs);
                        code.mov_rr(RDX, RAX);
                        code.pop(RAX);
                        code.call("rt_collection_eq");
                        if op == "!=" {
                            code.mov_imm32(RCX, 1);
                            code.xor_rr(RAX, RCX);
                        }
                    } else {
                        code.mov_imm32_zero(RAX, u32::from(op == "!="));
                    }
                    self.store(code, &Operand::Temp(*dest));
                    return Ok(());
                }
                if left == Kind::Str && right == Kind::Str {
                    // Two strings compare byte-wise; `rt_cmp_str` answers -1,
                    // 0 or 1, which the comparison's condition code reads
                    // against zero.
                    self.load(code, data, lhs);
                    code.push(RAX);
                    self.load(code, data, rhs);
                    code.mov_rr(RCX, RAX);
                    code.pop(RAX);
                    code.call("rt_cmp_str");
                    code.cmp_imm8(RAX, 0);
                    code.setcc(cc);
                    self.store(code, &Operand::Temp(*dest));
                    return Ok(());
                }
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
                        '%' => {
                            code.movq_gpr_xmm(RAX, XMM0);
                            code.movq_gpr_xmm(RCX, XMM1);
                            code.call("rt_rem_float");
                            self.store(code, &Operand::Var(*var));
                            return Ok(());
                        }
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
                    // A compound assignment has no `guard_zero` in front of
                    // it, so the remainder keeps its own check and exits with
                    // the interpreter's `E038` (without the line it has no
                    // span for here).
                    '%' => self.remainder(code, true),
                    other => return Err(unsupported(&format!("the operator '{}='", other))),
                }
                self.store(code, &Operand::Var(*var));
            }
            MirInst::Call { dest, name, args } => {
                // A user function wins over a builtin of the same name, as in
                // the interpreter; `analyze` has already refused a call to a
                // name that is neither.
                if let Some(callee) = self.callees.get(name).copied() {
                    self.call(code, data, callee, args);
                } else if let Some(builtin) = builtin(name) {
                    self.builtin_call(code, data, builtin, name, args)?;
                } else {
                    return Err(unsupported(&format!("a function call to \"{}\"", name)));
                }
                self.store(code, &Operand::Temp(*dest));
            }
            // A zero divisor: the interpreter's arithmetic code, chosen by the
            // operand kinds. It unwinds like a raise -- the identity slot holds
            // the code, the payload the exact message with the operator's line
            // -- but it has no prefix, because a panic is not a named raise.
            MirInst::GuardZero {
                lhs,
                rhs,
                op,
                line,
                target,
            } => {
                let left = self.kind(lhs)?;
                let right = self.kind(rhs)?;
                if !matches!(left, Kind::Int | Kind::Float)
                    || !matches!(right, Kind::Int | Kind::Float)
                {
                    // Nothing numeric to divide: the `binary` that follows
                    // refuses the program.
                    return Ok(());
                }
                let both_int = left == Kind::Int && right == Kind::Int;
                let (name, text) = match (op, both_int) {
                    ('/', true) => ("E036", format!("E036 လိုင်း {} တွင် သုညဖြင့် စား၍မရပါ။", line)),
                    ('/', false) => ("E037", format!("E037 လိုင်း {} တွင် သုညဖြင့် စား၍မရပါ။", line)),
                    (_, true) => (
                        "E038",
                        format!("E038 လိုင်း {} တွင် သုညဖြင့် ကြွင်းကိန်းရှာ၍မရပါ။", line),
                    ),
                    (_, false) => (
                        "E039",
                        format!("E039 လိုင်း {} တွင် သုညဖြင့် ကြွင်းကိန်းရှာ၍မရပါ။", line),
                    ),
                };
                let site = self.next_site();
                let nonzero = format!("guard_nz_{site}");
                self.load(code, data, rhs);
                if right == Kind::Float {
                    // A NaN divisor is not zero: parity is set, so it is
                    // skipped before the equality test is read.
                    code.movq_xmm_gpr(XMM0, RAX);
                    code.xorpd(XMM1, XMM1);
                    code.ucomisd(XMM0, XMM1);
                    code.jcc(CC_P, &nonzero);
                    code.jcc(CC_NE, &nonzero);
                } else {
                    code.test_rr(RAX, RAX);
                    code.jcc(CC_NE, &nonzero);
                }
                let identity = data.intern(name);
                code.mov_data_address(RAX, identity);
                self.store_error(code, self.error_identity);
                code.mov_imm32_zero(RAX, 0);
                self.store_error(code, self.error_prefix);
                let message = data.intern(&text);
                code.mov_data_address(RAX, message);
                self.store_error(code, self.error_payload);
                code.jmp(&format!("fn{}.bb{}", self.function, *target));
                code.label(&nonzero);
            }
            MirInst::SetError {
                identity,
                prefix,
                value,
            } => {
                // The message first: formatting it can use registers, but it
                // is already a string in the slice.
                self.load(code, data, value);
                self.store_error(code, self.error_payload);
                let prefix = data.intern(prefix);
                code.mov_data_address(RAX, prefix);
                self.store_error(code, self.error_prefix);
                let identity = data.intern(identity);
                code.mov_data_address(RAX, identity);
                self.store_error(code, self.error_identity);
            }
            // A catch filter: `E178` takes every raise, any other name only
            // the raise whose identity is exactly that text. The identities
            // are interned, so equal text is the same pointer.
            MirInst::ErrorMatches { dest, identity } => {
                if identity == "E178" {
                    // `E178` selects a raise, never a run-time panic: only a
                    // raise leaves a prefix behind.
                    self.load_error(code, self.error_prefix);
                    code.test_rr(RAX, RAX);
                    code.setcc(CC_NE);
                } else {
                    self.load_error(code, self.error_identity);
                    let name = data.intern(identity);
                    code.mov_data_address(RCX, name);
                    code.cmp_rr(RAX, RCX);
                    code.setcc(CC_E);
                }
                self.store(code, &Operand::Temp(*dest));
            }
            // What a catch clause binds: a raise's message, or -- for a
            // run-time panic, which has no prefix -- its code (`E036`).
            MirInst::BindError { var } => {
                let site = self.next_site();
                let panic = format!("bind_code_{site}");
                let done = format!("bind_done_{site}");
                self.load_error(code, self.error_prefix);
                code.test_rr(RAX, RAX);
                code.jcc(CC_E, &panic);
                self.load_error(code, self.error_payload);
                code.jmp(&done);
                code.label(&panic);
                self.load_error(code, self.error_identity);
                code.label(&done);
                self.store(code, &Operand::Var(*var));
            }
            // The clause has taken the error; the next check must not think it
            // is still pending. The identity is the flag a check reads.
            MirInst::ClearError => {
                code.mov_imm32_zero(RAX, 0);
                self.store_error(code, self.error_identity);
                self.store_error(code, self.error_prefix);
            }
            // A call that raised: unwind the way a raise written here does. The
            // identity slot is non-zero for a raise and for a run-time panic
            // alike, so both are caught by the same check.
            MirInst::CheckError { target } => {
                self.load_error(code, self.error_identity);
                code.test_rr(RAX, RAX);
                code.jcc(CC_NE, &format!("fn{}.bb{}", self.function, *target));
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
                    Kind::List | Kind::Tuple | Kind::Set | Kind::Dict => {
                        // The collection printer writes the `repr` form; the
                        // top-level `print` adds the statement newline.
                        code.mov_imm32_zero(RDI, kind.tag() as u32);
                        code.call("rt_print_collection");
                        code.call("rt_write_newline");
                    }
                    // A dynamic read holds the address of the element's own
                    // `{ tag, word }` pair, so the display dispatches on the
                    // tag the collection stored.
                    Kind::Dynamic => {
                        code.mov_rr(R11, RAX);
                        code.load_ptr_disp32(RDI, R11, 0);
                        code.load_ptr_disp32(RAX, R11, 8);
                        code.call("rt_print_dynamic");
                    }
                }
            }
            MirInst::ToStr { dest, value } => {
                self.to_str(code, data, value)?;
                self.store(code, &Operand::Temp(*dest));
            }
            MirInst::List { dest, items } => {
                self.sequence(code, data, *dest, Kind::List, items)?;
            }
            MirInst::Tuple { dest, items } => {
                self.sequence(code, data, *dest, Kind::Tuple, items)?;
            }
            MirInst::Set { dest, items } => {
                if items.iter().all(|item| matches!(item, Operand::Const(_))) {
                    let unique = dedup_set(items)?;
                    self.sequence(code, data, *dest, Kind::Set, &unique)?;
                } else {
                    self.runtime_set(code, data, *dest, items)?;
                }
            }
            MirInst::Dict { dest, pairs } => {
                if pairs
                    .iter()
                    .all(|(key, _)| matches!(key, Operand::Const(Const::Str(_))))
                {
                    let unique = dedup_dict(pairs)?;
                    self.dictionary(code, data, *dest, &unique)?;
                } else {
                    self.runtime_dict(code, data, *dest, pairs)?;
                }
            }
            MirInst::Index {
                dest,
                base,
                indices,
                line,
            } => {
                let base_kind = self.kind(base)?;
                let index = indices
                    .first()
                    .ok_or_else(|| unsupported("an index with no part"))?;
                // Evaluate the base, then the index, left to right.
                self.load(code, data, base);
                code.mov_rr(R11, RAX);
                self.load(code, data, index);
                // `r8` carries the source line for the E049/E053 text.
                // A dynamic read keeps the element's `{ tag, word }` address,
                // so the tag it was stored with reaches every later use; any
                // other read takes the word alone.
                let dynamic = self.kind(&Operand::Temp(*dest))? == Kind::Dynamic;
                code.mov_imm32(R8, *line as i32);
                if base_kind == Kind::Dict {
                    code.mov_rr(RDX, RAX);
                    code.mov_rr(RAX, R11);
                    code.call(if dynamic {
                        "rt_index_dict_ref"
                    } else {
                        "rt_index_dict"
                    });
                } else {
                    code.mov_rr(RCX, RAX);
                    code.mov_rr(RAX, R11);
                    code.call(if dynamic {
                        "rt_index_seq_ref"
                    } else {
                        "rt_index_seq"
                    });
                }
                self.store(code, &Operand::Temp(*dest));
            }
            MirInst::Len { dest, value } => {
                // A collection descriptor keeps its length in the second word.
                self.load(code, data, value);
                code.load_ptr_disp32(RAX, RAX, 8);
                self.store(code, &Operand::Temp(*dest));
            }
            MirInst::StoreIndex {
                base,
                indices,
                value,
                line,
            } => {
                let base_kind = self.kind(base)?;
                // The write replaces the binding's descriptor with a copy
                // carrying the change, which is the interpreter's value
                // semantics: a second binding of the same collection keeps its
                // own contents. That needs a variable to write the copy back
                // into; the analyzer refused anything else.
                let Operand::Var(var) = base else {
                    return Err(unsupported(
                        "a store into an element of a temporary (the collection has to be held in a variable)",
                    ));
                };
                let index = indices
                    .first()
                    .ok_or_else(|| unsupported("an index with no part"))?;
                let value_kind = self.kind(value)?;
                // `rax` base, `rcx` index/key, `rdx` value word, `rdi` value
                // tag, `r8` source line (for the E049 text).
                self.load(code, data, base);
                code.mov_rr(R11, RAX);
                self.load(code, data, index);
                code.mov_rr(RCX, RAX);
                self.load(code, data, value);
                code.mov_rr(RDX, RAX);
                code.mov_imm32(RDI, value_kind.tag() as i32);
                code.mov_imm32(R8, *line as i32);
                code.mov_rr(RAX, R11);
                match base_kind {
                    Kind::List | Kind::Tuple => code.call("rt_store_seq"),
                    Kind::Set => code.call("rt_store_set"),
                    Kind::Dict => code.call("rt_store_dict"),
                    other => return Err(unsupported(&format!("a store into a {}", other.name()))),
                }
                // The copy is now what the name holds.
                let slot = self.var_slots[var];
                code.store(slot, RAX);
            }
            MirInst::Items { dest, value } => {
                // What a `for` loop walks has to be a sequence `Len`/`Index`
                // can read. A list, tuple or set already is one; a dict's keys
                // are copied into a fresh list by the runtime, which is the
                // order the interpreter's dict iteration yields.
                let kind = self.kind(value)?;
                self.load(code, data, value);
                match kind {
                    Kind::Dict => code.call("rt_items_dict"),
                    Kind::List | Kind::Tuple | Kind::Set => {}
                    // A numeric source is the interpreter's auto-range: the
                    // loop walks the ints below it. A float bound is rounded
                    // up, so `for i in 2.5` runs for 0, 1 and 2.
                    Kind::Int => code.call("rt_items_range_int"),
                    Kind::Float => {
                        self.round_call(code, data, value, Builtin::Ceil)?;
                        code.call("rt_items_range_int");
                    }
                    other => {
                        return Err(unsupported(&format!("iteration over a {}", other.name())))
                    }
                }
                self.store(code, &Operand::Temp(*dest));
            }
            other => return Err(unsupported(&format!("{:?}", other))),
        }
        Ok(())
    }

    /// Emit a list or tuple descriptor: `{ kind, len, [tag, word] * len }`.
    /// The tag for each element is the static kind the analyzer settled, so a
    /// homogeneous literal needs no per-element dispatch at construction.
    fn sequence(
        &mut self,
        code: &mut Code,
        data: &mut Data,
        dest: Temp,
        kind: Kind,
        items: &[Operand],
    ) -> Result<(), String> {
        let size = 16 + 16 * items.len() as i32;
        code.mov_imm32(RDI, size);
        code.call("rt_alloc");
        code.mov_rr(R11, RAX);
        code.mov_imm32(RCX, kind.tag() as i32);
        code.store_ptr_disp32(R11, 0, RCX);
        code.mov_imm32(RCX, items.len() as i32);
        code.store_ptr_disp32(R11, 8, RCX);
        for (position, item) in items.iter().enumerate() {
            let offset = 16 + 16 * position as i32;
            self.load_tagged(code, data, item)?;
            code.store_ptr_disp32(R11, offset, RDI);
            code.store_ptr_disp32(R11, offset + 8, RDX);
        }
        code.mov_rr(RAX, R11);
        self.store(code, &Operand::Temp(dest));
        Ok(())
    }

    /// Emit a fresh descriptor of `kind` with room for `elements` members
    /// (a dict pair counts as one) and length 0. One allocation covers the
    /// whole literal, so the runtime can fill it in place.
    fn sized_collection(&self, code: &mut Code, kind: Kind, elements: i32, dest: Temp) {
        let stride = if kind == Kind::Dict { 32 } else { 16 };
        code.mov_imm32(RDI, 16 + stride * elements);
        code.call("rt_alloc");
        code.mov_imm32(RCX, kind.tag() as i32);
        code.store_ptr_disp32(RAX, 0, RCX);
        code.mov_imm32_zero(RCX, 0);
        code.store_ptr_disp32(RAX, 8, RCX);
        self.store(code, &Operand::Temp(dest));
    }

    /// Build a set in one descriptor: the runtime's `rt_fill_set` appends the
    /// member that is not already present, which is the interpreter's
    /// insertion-ordered deduplication -- a member that already lives in the
    /// set keeps the position it first had. This is the path for a literal
    /// with a run-time member, where `dedup_set` cannot see the value before
    /// the program runs. A loop that rebuilds the literal therefore allocates
    /// once per evaluation instead of once per member.
    fn runtime_set(
        &mut self,
        code: &mut Code,
        data: &mut Data,
        dest: Temp,
        items: &[Operand],
    ) -> Result<(), String> {
        self.sized_collection(code, Kind::Set, items.len() as i32, dest);
        for item in items {
            // `rax` base, `rdx` the member word and `rdi` its tag. Dedup can
            // only drop a member, so the literal's capacity is never exceeded.
            self.load_tagged(code, data, item)?;
            self.load(code, data, &Operand::Temp(dest));
            code.call("rt_fill_set");
            self.store(code, &Operand::Temp(dest));
        }
        Ok(())
    }

    /// Build a dict in one descriptor: `rt_fill_dict` appends a key it has not
    /// seen and updates the value of one it has, so a key keeps the position
    /// it first had and takes the last value. This is the path for a literal
    /// with a run-time key, where `dedup_dict` cannot compare the key before
    /// the program runs. Like a set, a dict literal rebuilt in a loop costs one
    /// allocation per evaluation.
    fn runtime_dict(
        &mut self,
        code: &mut Code,
        data: &mut Data,
        dest: Temp,
        pairs: &[(Operand, Operand)],
    ) -> Result<(), String> {
        self.sized_collection(code, Kind::Dict, pairs.len() as i32, dest);
        for (key, value) in pairs {
            // `rax` base, `rcx` the key, `rdx` the value word and `rdi` its
            // tag. A repeated key overwrites in place, so the capacity is
            // never exceeded.
            self.load(code, data, key);
            code.mov_rr(RCX, RAX);
            self.load_tagged(code, data, value)?;
            self.load(code, data, &Operand::Temp(dest));
            code.call("rt_fill_dict");
            self.store(code, &Operand::Temp(dest));
        }
        Ok(())
    }

    /// Emit a dict descriptor: `{ TAG_DICT, len, [key_tag, key, val_tag, val] }
    /// * len`. Keys are string literals, deduplicated by the analyzer/codegen;
    /// a run-time key needs the runtime's hash/equality and is refused.
    fn dictionary(
        &mut self,
        code: &mut Code,
        data: &mut Data,
        dest: Temp,
        pairs: &[(Operand, Operand)],
    ) -> Result<(), String> {
        let size = 16 + 32 * pairs.len() as i32;
        code.mov_imm32(RDI, size);
        code.call("rt_alloc");
        code.mov_rr(R11, RAX);
        code.mov_imm32(RCX, Kind::Dict.tag() as i32);
        code.store_ptr_disp32(R11, 0, RCX);
        code.mov_imm32(RCX, pairs.len() as i32);
        code.store_ptr_disp32(R11, 8, RCX);
        for (position, (key, value)) in pairs.iter().enumerate() {
            let offset = 16 + 32 * position as i32;
            self.load(code, data, key);
            code.store_ptr_disp32(R11, offset + 8, RAX);
            code.mov_imm32(RCX, Kind::Str.tag() as i32);
            code.store_ptr_disp32(R11, offset, RCX);
            self.load_tagged(code, data, value)?;
            code.store_ptr_disp32(R11, offset + 16, RDI);
            code.store_ptr_disp32(R11, offset + 24, RDX);
        }
        code.mov_rr(RAX, R11);
        self.store(code, &Operand::Temp(dest));
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

    /// Emit `str(value)`, leaving a string descriptor in `rax`: a string is
    /// itself, an int its decimal text, a float its print form and a bool
    /// `True`/`False` (the interpreter's display). The fresh descriptors come
    /// from the collection arena.
    fn to_str(&self, code: &mut Code, data: &mut Data, value: &Operand) -> Result<(), String> {
        let kind = self.kind(value)?;
        self.load(code, data, value);
        match kind {
            Kind::Str => {}
            Kind::Int => code.call("rt_str_int"),
            Kind::Float => code.call("rt_str_float"),
            Kind::Bool => code.call("rt_str_bool"),
            other => return Err(unsupported(&format!("a `str` of a {}", other.name()))),
        }
        Ok(())
    }

    /// Convert the double in `xmm0` to its saturating `as i64`: NaN is 0,
    /// anything at or past 2^63 is `i64::MAX`, and anything below -2^63 is
    /// `i64::MIN`. That is the interpreter's cast, and `sum` uses it for its
    /// integer answer.
    fn float_to_int(&mut self, code: &mut Code) {
        let site = self.next_site();
        let nan = format!("f2i_nan_{site}");
        let big = format!("f2i_big_{site}");
        let small = format!("f2i_small_{site}");
        let done = format!("f2i_done_{site}");
        code.ucomisd(XMM0, XMM0);
        code.jcc(CC_P, &nan);
        code.mov_imm64(RCX, 0x43E0_0000_0000_0000);
        code.movq_xmm_gpr(XMM1, RCX);
        code.ucomisd(XMM0, XMM1);
        code.jcc(CC_AE, &big);
        // `-2^63` itself converts, so only a strictly smaller value saturates.
        code.mov_imm64(RCX, 0xC3E0_0000_0000_0000);
        code.movq_xmm_gpr(XMM1, RCX);
        code.ucomisd(XMM0, XMM1);
        code.jcc(CC_B, &small);
        code.cvttsd2si(RAX, XMM0);
        code.jmp(&done);
        code.label(&nan);
        code.mov_imm32_zero(RAX, 0);
        code.jmp(&done);
        code.label(&big);
        code.mov_imm64(RAX, i64::MAX as u64);
        code.jmp(&done);
        code.label(&small);
        code.mov_imm64(RAX, i64::MIN as u64);
        code.label(&done);
    }

    /// Emit `abs`, `min` or `max`, leaving the result in `rax`.
    ///
    /// `abs` of an int is the value or its negation; of a float it is the
    /// bits with the sign bit cleared, so `-0.0` becomes `0.0` and a NaN stays
    /// a NaN. `min`/`max` evaluate every argument left to right (so a call in
    /// an argument runs in order) into a scratch area, then keep the first
    /// value and replace it only when a later one is strictly smaller or
    /// larger. The interpreter's ordering treats a NaN pair as equal, so an
    /// unordered comparison never replaces the value.
    fn builtin_call(
        &mut self,
        code: &mut Code,
        data: &mut Data,
        builtin: Builtin,
        name: &str,
        args: &[Operand],
    ) -> Result<(), String> {
        match builtin {
            Builtin::Abs => {
                let arg = args
                    .first()
                    .ok_or_else(|| unsupported("`abs` with no argument"))?;
                let kind = self.kind(arg)?;
                self.load(code, data, arg);
                match kind {
                    Kind::Int => {
                        // A sign test keeps every value but the minimum.
                        let site = self.next_site();
                        let done = format!("abs_{site}");
                        code.test_rr(RAX, RAX);
                        code.jcc(CC_NS, &done);
                        code.neg(RAX);
                        code.label(&done);
                    }
                    Kind::Float => {
                        // Clearing bit 63 of the `f64` is `f64::abs`; a shift
                        // left then right does it without a mask register.
                        code.shl_imm(RAX, 1);
                        code.shr_imm(RAX, 1);
                    }
                    other => {
                        return Err(unsupported(&format!("`abs` on a {} value", other.name())))
                    }
                }
            }
            Builtin::Floor | Builtin::Ceil | Builtin::Round => {
                let arg = args
                    .first()
                    .ok_or_else(|| unsupported(&format!("`{}` with no argument", name)))?;
                self.round_call(code, data, arg, builtin)?;
            }
            Builtin::Len => {
                let arg = args
                    .first()
                    .ok_or_else(|| unsupported("`len` with no argument"))?;
                self.load(code, data, arg);
                code.load_ptr_disp32(RAX, RAX, 8);
            }
            Builtin::Str => {
                let arg = args
                    .first()
                    .ok_or_else(|| unsupported("`str` with no argument"))?;
                self.to_str(code, data, arg)?;
            }
            // `keys(d)`: the runtime already builds exactly that list for a
            // `for` loop over a dict, so the builtin is that call.
            Builtin::Keys => {
                let arg = args
                    .first()
                    .ok_or_else(|| unsupported("`keys` with no argument"))?;
                self.load(code, data, arg);
                code.call("rt_items_dict");
            }
            // `values(d)` copies each pair's value word with its own tag into
            // a fresh list, in the dict's insertion order.
            Builtin::Values => {
                let arg = args
                    .first()
                    .ok_or_else(|| unsupported("`values` with no argument"))?;
                self.load(code, data, arg);
                code.call("rt_values_dict");
            }
            // `entries(d)` is a fresh list holding one `(key, value)` tuple
            // per pair, built by the runtime with the stored tags.
            Builtin::Entries => {
                let arg = args
                    .first()
                    .ok_or_else(|| unsupported("`entries` with no argument"))?;
                self.load(code, data, arg);
                code.call("rt_entries_dict");
            }
            // `sum(xs)`: walk the descriptor left to right, adding every
            // element as a double -- the interpreter accumulates in `f64` --
            // and answer an int through its saturating `as i64` cast, or the
            // double itself when the elements are floats.
            Builtin::Sum => {
                let arg = args
                    .first()
                    .ok_or_else(|| unsupported("`sum` with no argument"))?;
                let Some(shape) = self.elem_kind(arg) else {
                    // An empty collection: the interpreter's answer is the int 0.
                    code.mov_imm32_zero(RAX, 0);
                    return Ok(());
                };
                if !matches!(shape, Kind::Int | Kind::Float) {
                    return Err(unsupported(&format!(
                        "`sum` of a collection of {}",
                        shape.name()
                    )));
                }
                let site = self.next_site();
                let loop_label = format!("sum_loop_{site}");
                let done_label = format!("sum_done_{site}");
                self.load(code, data, arg);
                // `r10` is the descriptor, `r11` its length, `rcx` the index.
                code.mov_rr(R10, RAX);
                code.load_ptr_disp32(R11, RAX, 8);
                code.mov_imm32_zero(RCX, 0);
                code.xorpd(XMM0, XMM0);
                code.label(&loop_label);
                code.cmp_rr(RCX, R11);
                code.jcc(CC_AE, &done_label);
                // The element word of item `i` is at `base + 24 + 16 * i`.
                code.mov_rr(RDX, RCX);
                code.shl_imm(RDX, 4);
                code.add_imm8(RDX, 24);
                code.add_rr(RDX, R10);
                code.load_ptr(RAX, RDX);
                if shape == Kind::Float {
                    code.movq_xmm_gpr(XMM1, RAX);
                } else {
                    code.cvtsi2sd(XMM1, RAX);
                }
                code.addsd(XMM0, XMM1);
                code.inc(RCX);
                code.jmp(&loop_label);
                code.label(&done_label);
                if shape == Kind::Float {
                    code.movq_gpr_xmm(RAX, XMM0);
                } else {
                    self.float_to_int(code);
                }
            }
            // `slice(xs, start[, end])`: the runtime clamps both ends, copies
            // the selected `{ tag, word }` pairs into a fresh list and answers
            // it. The end defaults to the descriptor's length, which the
            // runtime would clamp to anyway, but passing it keeps the helper's
            // calling convention one shape.
            Builtin::Slice => {
                let base = args
                    .first()
                    .ok_or_else(|| unsupported("`slice` with no argument"))?;
                let start = args
                    .get(1)
                    .ok_or_else(|| unsupported("`slice` with no start"))?;
                self.load(code, data, base);
                code.mov_rr(R11, RAX);
                self.load(code, data, start);
                code.mov_rr(RCX, RAX);
                match args.get(2) {
                    Some(end) => {
                        self.load(code, data, end);
                        code.mov_rr(RDX, RAX);
                    }
                    None => code.load_ptr_disp32(RDX, R11, 8),
                }
                code.mov_rr(RAX, R11);
                code.call("rt_slice_seq");
            }
            // `push`/`append`: the runtime's indexed write with the index set
            // to the current length is exactly "append at the end", and it
            // answers a fresh descriptor. That descriptor replaces what the
            // variable holds -- the interpreter's value semantics, where a
            // second binding keeps its own contents -- and the new length is
            // what the builtin returns.
            Builtin::Push => {
                let base = args
                    .first()
                    .ok_or_else(|| unsupported("`push` with no argument"))?;
                let value = args
                    .get(1)
                    .ok_or_else(|| unsupported("`push` with no value"))?;
                // `analyze` refuses anything else before this runs.
                let Operand::Var(var) = base else {
                    return Err(unsupported("a push whose list is not a variable"));
                };
                let value_kind = self.kind(value)?;
                self.load(code, data, base);
                code.mov_rr(R11, RAX);
                // `rcx` is the index: the descriptor's length word, so the
                // write lands just past the last element.
                code.load_ptr_disp32(RCX, RAX, 8);
                self.load(code, data, value);
                code.mov_rr(RDX, RAX);
                code.mov_imm32(RDI, value_kind.tag() as i32);
                // The index is never out of range, so no `E049` line is read.
                code.mov_imm32_zero(R8, 0);
                code.mov_rr(RAX, R11);
                code.call("rt_store_seq");
                let slot = self.var_slots[var];
                code.store(slot, RAX);
                code.load_ptr_disp32(RAX, RAX, 8);
            }
            // `pop`: the runtime copies the list without its last element
            // (answering the copy in `rax` and the dropped word in `rdx`), so
            // the binding takes the copy and the popped value is the result.
            // An empty list is the interpreter's `E110` panic.
            Builtin::Pop => {
                let base = args
                    .first()
                    .ok_or_else(|| unsupported("`pop` with no argument"))?;
                let Operand::Var(var) = base else {
                    return Err(unsupported("a pop whose list is not a variable"));
                };
                self.load(code, data, base);
                code.call("rt_pop_seq");
                let slot = self.var_slots[var];
                code.store(slot, RAX);
                code.mov_rr(RAX, RDX);
            }
            // `insert`: the runtime copies the list with `at` shifted by one
            // and the value written into the gap; the binding takes the copy
            // and the new length (its second word) is the result.
            Builtin::Insert => {
                let base = args
                    .first()
                    .ok_or_else(|| unsupported("`insert` with no argument"))?;
                let index = args
                    .get(1)
                    .ok_or_else(|| unsupported("`insert` with no index"))?;
                let value = args
                    .get(2)
                    .ok_or_else(|| unsupported("`insert` with no value"))?;
                // `analyze` refuses anything else before this runs.
                let Operand::Var(var) = base else {
                    return Err(unsupported("an insert whose list is not a variable"));
                };
                let value_kind = self.kind(value)?;
                self.load(code, data, base);
                code.mov_rr(R11, RAX);
                self.load(code, data, index);
                code.mov_rr(RCX, RAX);
                self.load(code, data, value);
                code.mov_rr(RDX, RAX);
                code.mov_imm32(RDI, value_kind.tag() as i32);
                code.mov_rr(RAX, R11);
                code.call("rt_insert_seq");
                let slot = self.var_slots[var];
                code.store(slot, RAX);
                code.load_ptr_disp32(RAX, RAX, 8);
            }
            // `remove`: the runtime copies the list without the element at
            // `at` and answers that element word in `rdx`.
            Builtin::Remove => {
                let base = args
                    .first()
                    .ok_or_else(|| unsupported("`remove` with no argument"))?;
                let index = args
                    .get(1)
                    .ok_or_else(|| unsupported("`remove` with no index"))?;
                let Operand::Var(var) = base else {
                    return Err(unsupported("a remove whose list is not a variable"));
                };
                self.load(code, data, base);
                code.mov_rr(R11, RAX);
                self.load(code, data, index);
                code.mov_rr(RCX, RAX);
                code.mov_rr(RAX, R11);
                code.call("rt_remove_seq");
                let slot = self.var_slots[var];
                code.store(slot, RAX);
                code.mov_rr(RAX, RDX);
            }
            // `sort`: a stable insertion sort in the runtime, over a copy.
            // The selector tells it which comparison to use: an int, a float
            // or a string comparison (`rt_cmp_str`).
            Builtin::Sort => {
                let base = args
                    .first()
                    .ok_or_else(|| unsupported("`sort` with no argument"))?;
                let Operand::Var(var) = base else {
                    return Err(unsupported("a sort whose list is not a variable"));
                };
                let selector = match self.elem_kind(base) {
                    Some(Kind::Int) => 0,
                    Some(Kind::Float) => 1,
                    Some(Kind::Str) => 2,
                    other => {
                        return Err(unsupported(&format!(
                            "sort of a list of {}",
                            other.map_or("unknown elements", |kind| kind.name())
                        )))
                    }
                };
                self.load(code, data, base);
                code.mov_imm32(RDI, selector);
                code.call("rt_sort_seq");
                let slot = self.var_slots[var];
                code.store(slot, RAX);
            }
            // `reverse` answers a fresh descriptor with the elements in the
            // opposite order, through the runtime's `rt_reverse_seq`. A list
            // held in a variable is reversed in place (the binding takes the
            // fresh descriptor), while a tuple or any other expression is
            // reversed as a value and its binding is left alone -- which is
            // exactly what the interpreter's `call_reverse` does.
            Builtin::Reverse => {
                let arg = args
                    .first()
                    .ok_or_else(|| unsupported("`reverse` with no argument"))?;
                let kind = self.kind(arg)?;
                self.load(code, data, arg);
                code.call("rt_reverse_seq");
                if kind == Kind::List {
                    if let Operand::Var(var) = arg {
                        let slot = self.var_slots[var];
                        code.store(slot, RAX);
                    }
                }
            }
            Builtin::Min | Builtin::Max => {
                let keep = match builtin {
                    Builtin::Min => false,
                    _ => true,
                };
                let first = args
                    .first()
                    .ok_or_else(|| unsupported(&format!("`{}` with no argument", name)))?;
                let kind = self.kind(first)?;
                if !kind.is_number() {
                    return Err(unsupported(&format!(
                        "`{}` over {} values",
                        name,
                        kind.name()
                    )));
                }
                let area = (((args.len() as u32) * 8 + 15) / 16) * 16;
                code.sub_rsp(area);
                for (position, arg) in args.iter().enumerate() {
                    self.load(code, data, arg);
                    code.store_rsp((position * 8) as u8, RAX);
                }
                code.load_rsp(RAX, 0);
                let site = self.next_site();
                for position in 1..args.len() {
                    let replace = format!("minmax_{site}_{position}");
                    let skip = format!("minmax_skip_{site}_{position}");
                    code.load_rsp(RCX, (position * 8) as u8);
                    if kind == Kind::Float {
                        code.movq_xmm_gpr(XMM0, RAX);
                        code.movq_xmm_gpr(XMM1, RCX);
                        code.ucomisd(XMM0, XMM1);
                        if keep {
                            // `max`: replace when the kept value is below the
                            // candidate. Unordered (NaN) must not replace.
                            code.jcc(CC_P, &skip);
                            code.jcc(CC_B, &replace);
                        } else {
                            // `min`: replace when it is above the candidate;
                            // unordered is neither above nor below.
                            code.jcc(CC_A, &replace);
                        }
                    } else {
                        code.cmp_rr(RAX, RCX);
                        code.jcc(if keep { CC_L } else { CC_G }, &replace);
                    }
                    code.jmp(&skip);
                    code.label(&replace);
                    code.mov_rr(RAX, RCX);
                    code.label(&skip);
                }
                code.add_rsp(area);
            }
        }
        Ok(())
    }

    /// Emit `floor`, `ceil` or `round`, leaving the int in `rax`.
    ///
    /// The interpreter converts the argument to a double, rounds it, and casts
    /// with `as i64`, which saturates: a NaN is 0, anything at or past `2^63`
    /// is `i64::MAX` and anything below `-2^63` is `i64::MIN`. Converting an
    /// int to a double first (rather than rounding the int directly) is what
    /// keeps a value beyond 2^53 on the interpreter's side of the rounding.
    ///
    /// Truncation is `cvttsd2si`, exact because the out-of-range values were
    /// branched away; the difference between the value and its truncation is
    /// then compared against zero (for floor/ceil) or half away from zero
    /// (for the interpreter's `f64::round`).
    fn round_call(
        &mut self,
        code: &mut Code,
        data: &mut Data,
        arg: &Operand,
        mode: Builtin,
    ) -> Result<(), String> {
        match self.kind(arg)? {
            Kind::Int => {
                self.load(code, data, arg);
                code.cvtsi2sd(XMM0, RAX);
            }
            Kind::Float => {
                self.load(code, data, arg);
                code.movq_xmm_gpr(XMM0, RAX);
            }
            // `analyze` refuses a non-numeric argument before this runs.
            other => return Err(unsupported(&format!("rounding a {} value", other.name()))),
        }
        let site = self.next_site();
        let nan = format!("rnd_nan_{site}");
        let family = format!("rnd_family_{site}");
        let big = format!("rnd_big_{site}");
        let small = format!("rnd_small_{site}");
        let inc = format!("rnd_inc_{site}");
        let dec = format!("rnd_dec_{site}");
        let done = format!("rnd_done_{site}");

        // NaN is the only unordered pair against itself; it casts to 0.
        code.ucomisd(XMM0, XMM0);
        code.jcc(CC_P, &nan);
        // At or past 2^63 saturates to the maximum.
        code.mov_imm64(RCX, 0x43E0_0000_0000_0000);
        code.movq_xmm_gpr(XMM1, RCX);
        code.ucomisd(XMM0, XMM1);
        code.jcc(CC_B, &family);
        code.jmp(&big);
        code.label(&family);
        // Below -2^63 saturates to the minimum (exactly -2^63 converts).
        code.mov_imm64(RCX, 0xC3E0_0000_0000_0000);
        code.movq_xmm_gpr(XMM1, RCX);
        code.ucomisd(XMM0, XMM1);
        code.jcc(CC_B, &small);

        code.cvttsd2si(RAX, XMM0);
        // fraction = value - truncation, in `xmm0`, compared with 0.
        code.cvtsi2sd(XMM1, RAX);
        code.subsd(XMM0, XMM1);
        code.xorpd(XMM2, XMM2);
        match mode {
            Builtin::Floor => {
                // A negative fraction means the truncation went up.
                code.ucomisd(XMM0, XMM2);
                code.jcc(CC_B, &dec);
            }
            Builtin::Ceil => {
                code.ucomisd(XMM0, XMM2);
                code.jcc(CC_A, &inc);
            }
            _ => {
                // `f64::round` is half away from zero: 0.5 and above rounds
                // up, -0.5 and below rounds down.
                code.mov_imm64(RCX, 0x3FE0_0000_0000_0000);
                code.movq_xmm_gpr(XMM1, RCX);
                code.ucomisd(XMM0, XMM1);
                code.jcc(CC_AE, &inc);
                code.mov_imm64(RCX, 0xBFE0_0000_0000_0000);
                code.movq_xmm_gpr(XMM1, RCX);
                code.ucomisd(XMM0, XMM1);
                code.jcc(CC_BE, &dec);
            }
        }
        code.jmp(&done);
        code.label(&inc);
        code.inc(RAX);
        code.jmp(&done);
        code.label(&dec);
        code.dec(RAX);
        code.jmp(&done);
        code.label(&nan);
        code.mov_imm32_zero(RAX, 0);
        code.jmp(&done);
        code.label(&big);
        code.mov_imm64(RAX, i64::MAX as u64);
        code.jmp(&done);
        code.label(&small);
        code.mov_imm64(RAX, i64::MIN as u64);
        code.label(&done);
        Ok(())
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
    fn remainder(&mut self, code: &mut Code, guard: bool) {
        let site = self.site;
        self.site += 1;
        let add = format!("rem_a_{}", site);
        let done = format!("rem_d_{}", site);
        let nonzero = format!("rem_n_{}", site);

        if guard {
            // A zero divisor panics. It has to be a `call`, not a jump, or the
            // panic routine would run with the stack 8 bytes off its aligned
            // position.
            code.test_rr(RCX, RCX);
            code.jcc(CC_NE, &nonzero);
            code.call("rt_panic_zero");
            code.ud2();
            code.label(&nonzero);
        }
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
