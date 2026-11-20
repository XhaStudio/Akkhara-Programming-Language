//! The instruction set and the on-disk program format.

use super::*;
use crate::interp::{Key, Value};
use indexmap::{IndexMap, IndexSet};
use std::rc::Rc;

/// One instruction. Every jump holds an absolute index into its own chunk
/// (the main code or one function body).
#[derive(Debug, Clone)]
pub enum Instr {
    Const(Value),
    Load(String, u32),
    Store(String),
    Pop,
    /// Pop a list or tuple and bind one name per item, in order:
    /// `[a, b] = <value>;`. The line is carried so a non-collection (`E154`)
    /// and a count mismatch (`E155`) point at the statement, as they do in the
    /// interpreter, and a `pin` name is refused (`E104`).
    Destructure(Vec<String>, u32),
    Add,
    Sub,
    Mul,
    /// Divide: integers produce a float, as the interpreter's `/` does. The
    /// line is carried so division by zero reports the interpreter's `E036`
    /// (integers) or `E037` (floats) at the line written.
    Div(u32),
    /// Remainder, Euclidean as the interpreter computes it. The line is
    /// carried for `E038`/`E039`.
    Mod(u32),
    Neg,
    /// Logical not. The line is carried so a non-bool operand reports the
    /// interpreter's `E043` at the line the operator was written on.
    Not(u32),
    And,
    Or,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    /// Pop a bool; jump when it is true.
    JumpIfTrue(usize),
    /// Pop a bool; jump when it is false.
    JumpIfFalse(usize),
    Jump(usize),
    /// Pop a value and print it, exactly as `... ကို ဖော်ပြပါ။` does.
    Print,
    /// Replace the top value with its length.
    Len,
    /// Call a user function: its index in [`Program::funcs`] and how many
    /// arguments were pushed.
    Call(usize, u8),
    /// Return from the current function, popping its value (or `null`). The
    /// line is carried so a `return` outside any function fails with the
    /// interpreter's `E108` instead of quietly ending the program.
    Return(u32),
    /// `container[k1]...[kn]` -> element: how many index expressions were
    /// written and the line they sit on, so a missing dictionary key reports
    /// `E053` and a bad index `E048`/`E049` exactly as the interpreter does.
    Index(u32, u8),
    /// Pop `count` key/value pairs and build a dictionary, keeping insertion
    /// order and a repeated key's first position.
    Dict(usize),
    /// `<needle> in <haystack>` / `<haystack> ထဲ <needle>`.
    Contains(u32),
    /// Pop a value and push what a `for` loop walks: a dictionary's keys, in
    /// insertion order; anything else unchanged.
    IterKeys,
    /// `<name>[k...] = value`: pop the value, the keys and the container,
    /// write through, and store the container back under `name`.
    IndexStore(String, u32, u8),
    /// Pop `count` values and build a set: insertion order, a repeated value
    /// kept once, at its first position.
    Set(usize),
    /// Pop `argc` arguments and construct the class at that slot: its
    /// constructor runs in a fresh frame and the object is the result. The
    /// line is carried for `E034`.
    NewObj(usize, u8, u32),
    /// `အတု <field> သည် <value> ဖြစ်၏။`: pop a value and write it to the field
    /// of the object under construction. Line carried for `E035`.
    SelfSet(String, u32),
    /// `<object>.<field>` / `<object> ၏ <field>`, with the line for `E082`.
    Field(String, u32),
    /// `<name>.<field>` written without a call. When `name` is a live variable
    /// this reads that object's field, exactly as the interpreter does; a name
    /// that is not a variable is a library reference, which the compiler does
    /// not support -- so it fails with the interpreter's `E085`.
    FieldRef(String, String, u32),
    /// Pop a value and show it as an input prompt: its text, no newline,
    /// flushed, exactly as `... ကို မေးပါ။` does before it blocks.
    Prompt,
    /// Read one line from stdin and push it as a value, inferring numbers and
    /// booleans the way `မေးပါ` does.
    Read,
    /// Read one line and coerce it to the declared record type
    /// (`int`/`float`/`bool`/`str`), refusing a bad value with `E103`.
    ReadTyped(String, u32),
    /// `<name> :<type> = <value>;` / `pin <name> :<type> = <value>;`: pop the
    /// value, check it against the declared type (`E103`), record a `pin`
    /// name so a later write is `E104`, and refuse re-declaring a `pin` name
    /// that already exists (`E105`).
    Declare(String, String, bool, u32),
    /// Refuse a write to a `pin` constant (`E104`) *before* the right-hand
    /// side runs, which is where the interpreter refuses it.
    PinGuard(String, u32),
    /// Expand the `{name}` placeholders of the string on top of the stack, the
    /// way the interpreter expands a string literal when it is evaluated.
    /// The line is carried for `E056`/`E058`/`E060`.
    Interpolate(u32),
    /// Stop the run with this message. Used where the interpreter's failure has
    /// no printed value to compare -- a `break` with no loop to stop (`E116`),
    /// for instance -- so the message is the interpreter's own text.
    Abort(String),
    /// Start a protected region: from here to the matching [`Instr::TryEnd`] an
    /// error jumps to `handler` instead of stopping the run. The handler pc is
    /// patched once the layout is known.
    Try(usize),
    /// End of a protected region on the normal path: the handler is dropped.
    TryEnd,
    /// Push the code (`E104`) of the error a handler is running for.
    ErrorCode,
    /// Re-raise the error the current handler is running for.
    ReThrow,
    /// Drop `count` handlers: how a `break`/`continue`/`return` leaves the
    /// `try` blocks it sits inside.
    PopHandlers(u8),
    /// Mark `name` as a constant (`pin`), without writing a value to it: a
    /// `pin X :<type> = loop { ... }` seeds and declares the name before the
    /// loop, so only the constant mark is left for the end.
    MarkConst(String),
    /// `E105` when `name` already exists, without declaring anything: the
    /// up-front half of a `pin` loop value, which must refuse a redeclaration
    /// before the loop body runs.
    PinDeclareCheck(String, u32),
    /// Call the builtin `name` with `argc` evaluated arguments from the stack.
    /// The name is resolved when the call is compiled, and the work is done by
    /// the interpreter's own value-based builtin code, so messages and results
    /// cannot drift between the backends. `line` points `E111` and the
    /// wrong-argument errors at the call.
    Builtin(String, u8, u32),
    /// Import the builtin library `name` (every spelling it answers to), the
    /// way `နည်းပညာများ ... ကို သုံးမည်။` does when it runs.
    UseLib(String),
    /// `E089` unless the library `name` has been imported: compiled *before*
    /// a call's arguments, matching the interpreter's order.
    RequireLib(String, u32),
    /// Call the library function `lib.fn` with `argc` evaluated arguments.
    /// `err` is the message to raise when the call produces no value or when
    /// the value is required (empty: put `null` on the stack instead).
    LibCall(String, String, u8, u32, String),
    /// Return with a value: the function's last expression, its last
    /// declaration, or an explicit `return <expr>;`.
    ReturnValue(u32),
    /// Return without one: the function's body ended on a statement that yields
    /// nothing, or on a bare `return;`. A caller that demanded a value turns
    /// this into `E071`.
    ReturnVoid(u32),
    /// Call the user function `index`, storing its result. `err` is the `E071`
    /// message to raise when the body returns nothing.
    CallValue(usize, u8, u32, String),
    /// Turn the collection on top of the stack into a list of the items
    /// `map`/`filter`/`reduce` walk: a list or tuple as it is, a set's and a
    /// dictionary's values in insertion order, a string's clusters. Anything
    /// else is the interpreter's `E110`, raised here -- before `reduce`'s
    /// starting value is evaluated, which is the interpreter's order.
    /// `kind` selects the builtin named in that message (0 map, 1 filter,
    /// 2 reduce).
    IterItems(u8, u32),
    /// `map`/`filter`/`reduce`: the items list (pushed by `IterItems`), the
    /// callback value, and for `reduce` its starting value are on the stack.
    /// The callback is an ordinary callable value, so a lambda written there,
    /// a named function and a variable all work -- the interpreter resolves
    /// the same three.
    HigherOrder(u8, u32),
    /// Check that the value on top of the stack is a list, before a mutating
    /// builtin's other arguments run. `E110` names the builtin.
    ListArg(String, u32),
    /// `push`/`append`/`pop`/`insert`/`remove`/`sort` on the variable `var`:
    /// the builtin's name, the variable, the argument count and the line. The
    /// list and the extra arguments are on the stack; the updated list is
    /// written back through the variable and the result is pushed.
    Mutate(String, String, u8, u32),
    /// Fill the parameter `name` from its default when the call left it off:
    /// jump to the given index when the frame already has it.
    ParamMissing(String, usize),
    /// Pop `count` values and build a list, keeping their order.
    List(usize),
    /// Pop `count` values and build a tuple.
    Tuple(usize),
    /// Push the compiled function `index` as a first-class value, so it can
    /// be stored, passed and returned like any other value. Calling it later
    /// creates the same fresh frame `Call` does.
    MakeFunc(usize),
    /// `Load`, except that a name which is not a variable but is a user
    /// function yields the function itself -- the interpreter's `Expr::Ident`
    /// rule, which lets `f = double;` bind a function value.
    LoadFn(String, usize, u32),
    /// Before a call's arguments run, check that `name` holds a callable
    /// value. `mode` picks the missing-name error the interpreter's path
    /// uses: 0 is `E032` (an expression), 1 is `E031` (the `... ကို လုပ်ပါ။`
    /// form) and 2 is `E113` (the eng `name(args);` statement).
    CallableGuard(String, u32, u8),
    /// Call the callable in variable `name` with `argc` arguments from the
    /// stack. `mode` is the guard's, for a name that vanished between the two
    /// instructions. A callable call always wants a value, so a body that
    /// yields none is the interpreter's `E114` with the callable's label.
    CallDyn(String, u8, u32, u8),
    /// `reverse`: reverse the value on top of the stack. With a variable, a
    /// list is reversed in place and written back (`E104` for a `pin` list);
    /// every other value is reversed without a write.
    Reverse(Option<String>, u32),
    /// Keep the top value only when it is a bool, with the interpreter's
    /// `E043` for anything else.
    ToBool(u32),
    /// Short-circuit `and`: when the left operand is false, push `false` and
    /// jump past the right operand; otherwise evaluate it.
    AndJump(usize, u32),
    /// Short-circuit `or`: when the left operand is true, push `true` and
    /// jump past the right operand; otherwise evaluate it.
    OrJump(usize, u32),
    /// End of the top-level chunk.
    Halt,
}

/// One compiled class: its constructor parameters, and the constructor body
/// (the first method) that fills the object's fields in.
#[derive(Debug, Clone)]
pub struct ClassDef {
    pub name: String,
    pub params: Vec<String>,
    pub code: Vec<Instr>,
}

/// One compiled user function.
#[derive(Debug, Clone)]
pub struct FuncDef {
    pub name: String,
    pub params: Vec<String>,
    /// How many arguments a call must supply: the parameters without a
    /// default. The rest are filled in by the body's own prologue.
    pub required: usize,
    pub code: Vec<Instr>,
}

/// A whole compiled program: the top-level chunk plus every function body.
#[derive(Debug, Clone, Default)]
pub struct Program {
    pub code: Vec<Instr>,
    pub funcs: Vec<FuncDef>,
    pub classes: Vec<ClassDef>,
}

/// Version of the container [`Program::encode`] writes. `decode` refuses any
/// other version with `E175`, so a built file and the reader always agree on
/// the instruction set.
///
/// Revision 2 added dictionaries: value tag 7, the `Dict`/`Contains`/
/// `IterKeys`/`IndexStore` instructions, and a source line on `Index`.
/// Revision 3 added sets (value tag 8, `Set`), classes (`Program::classes`
/// with `NewObj`/`SelfSet`/`Field`) and the object-building constructor frame.
/// Revision 4 added `input`: `Prompt`, `Read` and `ReadTyped`.
/// Revision 5 added the eng declaration forms: `Declare` and `PinGuard`.
/// Revision 6 added `try`/`catch`/`finally`: `Try`, `TryEnd`, `ErrorCode`,
/// `ReThrow` and `PopHandlers`.
/// Revision 11 carried the line of a missing-variable `E030` on `Load`, and
/// revision 12 the callback builtins: `IterItems` and `HigherOrder`, which run
/// `map`/`filter`/`reduce` with a lambda or named callback. Revision 13 gave
/// `Div`/`Mod` their line, so division by zero reports `E036`/`E037` and a zero
/// remainder `E038`/`E039` exactly where the interpreter does.
/// Revision 15 made lambdas first-class values: `MakeFunc`, `LoadFn`,
/// `CallableGuard`, `CallDyn`, `Reverse`, `ToBool`, `AndJump`, `OrJump`, a
/// line on `Not`, and a value callback on `HigherOrder`.
/// Revision 16 added destructuring: `Destructure`, which binds one name per
/// item of a list or tuple.
pub const FORMAT_VERSION: u16 = 16;

/// Container magic: `AKKBC` + the digits of the original container layout.
/// The revision that decides compatibility is [`FORMAT_VERSION`], below the
/// magic; older magic digits are kept so a built file stays recognisable.
pub(crate) const MAGIC: [u8; 8] = *b"AKKBC001";

pub(crate) fn malformed(why: &str) -> String {
    format!("E175 bytecode ကို ဖတ်၍မရပါ - {}.", why)
}

pub(crate) fn put_u16(out: &mut Vec<u8>, n: u16) {
    out.extend_from_slice(&n.to_le_bytes());
}

pub(crate) fn put_u32(out: &mut Vec<u8>, n: u32) {
    out.extend_from_slice(&n.to_le_bytes());
}

pub(crate) fn put_str(out: &mut Vec<u8>, text: &str) -> Result<(), String> {
    let len = u32::try_from(text.len()).map_err(|_| "E175 စာသား ရှည်လွန်းပါသည်။".to_string())?;
    put_u32(out, len);
    out.extend_from_slice(text.as_bytes());
    Ok(())
}

/// Reads little-endian fields out of a byte slice, refusing to run off its end.
pub(crate) struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Reader { bytes, at: 0 }
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8], String> {
        let end = self
            .at
            .checked_add(n)
            .filter(|end| *end <= self.bytes.len())
            .ok_or_else(|| malformed("အချက်အလက် မလုံလောက်ပါ"))?;
        let slice = &self.bytes[self.at..end];
        self.at = end;
        Ok(slice)
    }

    fn u8(&mut self) -> Result<u8, String> {
        Ok(self.take(1)?[0])
    }

    fn u16(&mut self) -> Result<u16, String> {
        let b = self.take(2)?;
        Ok(u16::from_le_bytes([b[0], b[1]]))
    }

    fn u32(&mut self) -> Result<u32, String> {
        let b = self.take(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    fn u64(&mut self) -> Result<u64, String> {
        let b = self.take(8)?;
        Ok(u64::from_le_bytes([
            b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7],
        ]))
    }

    fn text(&mut self) -> Result<String, String> {
        let len = self.u32()? as usize;
        let raw = self.take(len)?;
        String::from_utf8(raw.to_vec()).map_err(|_| malformed("စာသားသည် UTF-8 မဟုတ်ပါ"))
    }

    fn done(&self) -> bool {
        self.at == self.bytes.len()
    }
}

pub(crate) fn encode_value(value: &Value, out: &mut Vec<u8>) -> Result<(), String> {
    match value {
        Value::Null => out.push(0),
        Value::Bool(b) => {
            out.push(1);
            out.push(u8::from(*b));
        }
        Value::Int(n) => {
            out.push(2);
            out.extend_from_slice(&n.to_le_bytes());
        }
        Value::Float(f) => {
            out.push(3);
            out.extend_from_slice(&f.to_le_bytes());
        }
        Value::Str(s) => {
            out.push(4);
            put_str(out, s)?;
        }
        Value::List(items) => {
            out.push(5);
            put_u32(out, items.len() as u32);
            for item in items {
                encode_value(item, out)?;
            }
        }
        Value::Tuple(items) => {
            out.push(6);
            put_u32(out, items.len() as u32);
            for item in items {
                encode_value(item, out)?;
            }
        }
        Value::Dict(pairs) => {
            out.push(7);
            put_u32(out, pairs.len() as u32);
            for (key, value) in pairs.iter() {
                encode_value(key.value(), out)?;
                encode_value(value, out)?;
            }
        }
        Value::Set(items) => {
            out.push(8);
            put_u32(out, items.len() as u32);
            for item in items.iter() {
                encode_value(item.value(), out)?;
            }
        }
        other => return Err(format!("E175 {} ကို bytecode တွင် သိမ်း၍မရပါ။", render(other))),
    }
    Ok(())
}

pub(crate) fn decode_value(reader: &mut Reader<'_>) -> Result<Value, String> {
    let tag = reader.u8()?;
    match tag {
        0 => Ok(Value::Null),
        1 => Ok(Value::Bool(reader.u8()? != 0)),
        2 => Ok(Value::Int(reader.u64()? as i64)),
        3 => Ok(Value::Float(f64::from_bits(reader.u64()?))),
        4 => Ok(Value::Str(reader.text()?)),
        5 | 6 => {
            let count = reader.u32()? as usize;
            let mut items = Vec::with_capacity(count.min(1024));
            for _ in 0..count {
                items.push(decode_value(reader)?);
            }
            if tag == 6 {
                Ok(Value::Tuple(items))
            } else {
                Ok(Value::List(items))
            }
        }
        7 => {
            let count = reader.u32()? as usize;
            let mut pairs: IndexMap<Key, Value> = IndexMap::with_capacity(count.min(1024));
            for _ in 0..count {
                let key = decode_value(reader)?;
                let value = decode_value(reader)?;
                pairs.insert(Key::new(key), value);
            }
            Ok(Value::Dict(Rc::new(pairs)))
        }
        8 => {
            let count = reader.u32()? as usize;
            let mut items: IndexSet<Key> = IndexSet::with_capacity(count.min(1024));
            for _ in 0..count {
                items.insert(Key::new(decode_value(reader)?));
            }
            Ok(Value::Set(Rc::new(items)))
        }
        tag => Err(malformed(&format!("မသိသော တန်ဖိုး အမျိုးအစား {}", tag))),
    }
}

pub(crate) fn encode_code(code: &[Instr], out: &mut Vec<u8>) -> Result<(), String> {
    put_u32(out, code.len() as u32);
    for instr in code {
        match instr {
            Instr::Const(v) => {
                out.push(0);
                encode_value(v, out)?;
            }
            Instr::Load(name, line) => {
                out.push(1);
                put_str(out, name)?;
                put_u32(out, *line);
            }
            Instr::Store(name) => {
                out.push(2);
                put_str(out, name)?;
            }
            Instr::Pop => out.push(3),
            Instr::Destructure(names, line) => {
                out.push(73);
                put_u32(out, names.len() as u32);
                for name in names {
                    put_str(out, name)?;
                }
                put_u32(out, *line);
            }
            Instr::Add => out.push(4),
            Instr::Sub => out.push(5),
            Instr::Mul => out.push(6),
            Instr::Div(line) => {
                out.push(7);
                put_u32(out, *line);
            }
            Instr::Mod(line) => {
                out.push(8);
                put_u32(out, *line);
            }
            Instr::Neg => out.push(9),
            Instr::Not(line) => {
                out.push(10);
                put_u32(out, *line);
            }
            Instr::And => out.push(11),
            Instr::Or => out.push(12),
            Instr::Eq => out.push(13),
            Instr::Ne => out.push(14),
            Instr::Lt => out.push(15),
            Instr::Le => out.push(16),
            Instr::Gt => out.push(17),
            Instr::Ge => out.push(18),
            Instr::JumpIfTrue(at) => {
                out.push(19);
                put_u32(out, *at as u32);
            }
            Instr::JumpIfFalse(at) => {
                out.push(20);
                put_u32(out, *at as u32);
            }
            Instr::Jump(at) => {
                out.push(21);
                put_u32(out, *at as u32);
            }
            Instr::Print => out.push(22),
            Instr::Len => out.push(23),
            Instr::Call(index, argc) => {
                out.push(24);
                put_u32(out, *index as u32);
                out.push(*argc);
            }
            Instr::Return(line) => {
                out.push(25);
                put_u32(out, *line);
            }
            Instr::Index(line, count) => {
                out.push(26);
                put_u32(out, *line);
                out.push(*count);
            }
            Instr::Halt => out.push(27),
            Instr::Dict(count) => {
                out.push(28);
                put_u32(out, *count as u32);
            }
            Instr::Contains(line) => {
                out.push(30);
                put_u32(out, *line);
            }
            Instr::IterKeys => out.push(29),
            Instr::IndexStore(name, line, count) => {
                out.push(31);
                put_str(out, name)?;
                put_u32(out, *line);
                out.push(*count);
            }
            Instr::Set(count) => {
                out.push(32);
                put_u32(out, *count as u32);
            }
            Instr::NewObj(index, argc, line) => {
                out.push(33);
                put_u32(out, *index as u32);
                out.push(*argc);
                put_u32(out, *line);
            }
            Instr::SelfSet(field, line) => {
                out.push(34);
                put_str(out, field)?;
                put_u32(out, *line);
            }
            Instr::Field(field, line) => {
                out.push(35);
                put_str(out, field)?;
                put_u32(out, *line);
            }
            Instr::FieldRef(name, field, line) => {
                out.push(36);
                put_str(out, name)?;
                put_str(out, field)?;
                put_u32(out, *line);
            }
            Instr::Prompt => out.push(37),
            Instr::Read => out.push(38),
            Instr::ReadTyped(type_name, line) => {
                out.push(39);
                put_str(out, type_name)?;
                put_u32(out, *line);
            }
            Instr::Declare(name, type_name, is_const, line) => {
                out.push(40);
                put_str(out, name)?;
                put_str(out, type_name)?;
                out.push(if *is_const { 1 } else { 0 });
                put_u32(out, *line);
            }
            Instr::PinGuard(name, line) => {
                out.push(41);
                put_str(out, name)?;
                put_u32(out, *line);
            }
            Instr::Interpolate(line) => {
                out.push(42);
                put_u32(out, *line);
            }
            Instr::Abort(message) => {
                out.push(43);
                put_str(out, message)?;
            }
            Instr::Try(handler) => {
                out.push(44);
                put_u32(out, *handler as u32);
            }
            Instr::TryEnd => out.push(45),
            Instr::ErrorCode => out.push(46),
            Instr::ReThrow => out.push(47),
            Instr::PopHandlers(count) => {
                out.push(48);
                out.push(*count);
            }
            Instr::MarkConst(name) => {
                out.push(49);
                put_str(out, name)?;
            }
            Instr::PinDeclareCheck(name, line) => {
                out.push(50);
                put_str(out, name)?;
                put_u32(out, *line);
            }
            Instr::Builtin(name, argc, line) => {
                out.push(51);
                put_str(out, name)?;
                out.push(*argc);
                put_u32(out, *line);
            }
            Instr::UseLib(name) => {
                out.push(52);
                put_str(out, name)?;
            }
            Instr::RequireLib(name, line) => {
                out.push(53);
                put_str(out, name)?;
                put_u32(out, *line);
            }
            Instr::LibCall(lib, fn_name, argc, line, err) => {
                out.push(54);
                put_str(out, lib)?;
                put_str(out, fn_name)?;
                out.push(*argc);
                put_u32(out, *line);
                put_str(out, err)?;
            }
            Instr::ReturnValue(line) => {
                out.push(55);
                put_u32(out, *line);
            }
            Instr::ReturnVoid(line) => {
                out.push(56);
                put_u32(out, *line);
            }
            Instr::CallValue(index, argc, line, err) => {
                out.push(57);
                put_u32(out, *index as u32);
                out.push(*argc);
                put_u32(out, *line);
                put_str(out, err)?;
            }
            Instr::IterItems(kind, line) => {
                out.push(58);
                out.push(*kind);
                put_u32(out, *line);
            }
            Instr::HigherOrder(kind, line) => {
                out.push(59);
                out.push(*kind);
                put_u32(out, *line);
            }
            Instr::ListArg(name, line) => {
                out.push(60);
                put_str(out, name)?;
                put_u32(out, *line);
            }
            Instr::Mutate(name, var, argc, line) => {
                out.push(61);
                put_str(out, name)?;
                put_str(out, var)?;
                out.push(*argc);
                put_u32(out, *line);
            }
            Instr::ParamMissing(name, target) => {
                out.push(62);
                put_str(out, name)?;
                put_u32(out, *target as u32);
            }
            Instr::List(count) => {
                out.push(63);
                put_u32(out, *count as u32);
            }
            Instr::Tuple(count) => {
                out.push(64);
                put_u32(out, *count as u32);
            }
            Instr::MakeFunc(index) => {
                out.push(65);
                put_u32(out, *index as u32);
            }
            Instr::LoadFn(name, index, line) => {
                out.push(66);
                put_str(out, name)?;
                put_u32(out, *index as u32);
                put_u32(out, *line);
            }
            Instr::CallableGuard(name, line, flags) => {
                out.push(67);
                put_str(out, name)?;
                put_u32(out, *line);
                out.push(*flags);
            }
            Instr::CallDyn(name, argc, line, flags) => {
                out.push(68);
                put_str(out, name)?;
                out.push(*argc);
                put_u32(out, *line);
                out.push(*flags);
            }
            Instr::Reverse(var, line) => {
                out.push(69);
                match var {
                    Some(name) => {
                        out.push(1);
                        put_str(out, name)?;
                    }
                    None => out.push(0),
                }
                put_u32(out, *line);
            }
            Instr::ToBool(line) => {
                out.push(70);
                put_u32(out, *line);
            }
            Instr::AndJump(target, line) => {
                out.push(71);
                put_u32(out, *target as u32);
                put_u32(out, *line);
            }
            Instr::OrJump(target, line) => {
                out.push(72);
                put_u32(out, *target as u32);
                put_u32(out, *line);
            }
        }
    }
    Ok(())
}

pub(crate) fn decode_code(reader: &mut Reader<'_>) -> Result<Vec<Instr>, String> {
    let count = reader.u32()? as usize;
    let mut code = Vec::with_capacity(count.min(4096));
    for _ in 0..count {
        let instr = match reader.u8()? {
            0 => Instr::Const(decode_value(reader)?),
            1 => {
                let name = reader.text()?;
                let line = reader.u32()?;
                Instr::Load(name, line)
            }
            2 => Instr::Store(reader.text()?),
            3 => Instr::Pop,
            4 => Instr::Add,
            5 => Instr::Sub,
            6 => Instr::Mul,
            7 => Instr::Div(reader.u32()?),
            8 => Instr::Mod(reader.u32()?),
            9 => Instr::Neg,
            10 => Instr::Not(reader.u32()?),
            11 => Instr::And,
            12 => Instr::Or,
            13 => Instr::Eq,
            14 => Instr::Ne,
            15 => Instr::Lt,
            16 => Instr::Le,
            17 => Instr::Gt,
            18 => Instr::Ge,
            19 => Instr::JumpIfTrue(reader.u32()? as usize),
            20 => Instr::JumpIfFalse(reader.u32()? as usize),
            21 => Instr::Jump(reader.u32()? as usize),
            22 => Instr::Print,
            23 => Instr::Len,
            24 => {
                let index = reader.u32()? as usize;
                let argc = reader.u8()?;
                Instr::Call(index, argc)
            }
            25 => Instr::Return(reader.u32()?),
            26 => {
                let line = reader.u32()?;
                let count = reader.u8()?;
                Instr::Index(line, count)
            }
            27 => Instr::Halt,
            28 => Instr::Dict(reader.u32()? as usize),
            29 => Instr::IterKeys,
            30 => Instr::Contains(reader.u32()?),
            31 => {
                let name = reader.text()?;
                let line = reader.u32()?;
                let count = reader.u8()?;
                Instr::IndexStore(name, line, count)
            }
            32 => Instr::Set(reader.u32()? as usize),
            33 => {
                let index = reader.u32()? as usize;
                let argc = reader.u8()?;
                let line = reader.u32()?;
                Instr::NewObj(index, argc, line)
            }
            34 => {
                let field = reader.text()?;
                let line = reader.u32()?;
                Instr::SelfSet(field, line)
            }
            35 => {
                let field = reader.text()?;
                let line = reader.u32()?;
                Instr::Field(field, line)
            }
            36 => {
                let name = reader.text()?;
                let field = reader.text()?;
                let line = reader.u32()?;
                Instr::FieldRef(name, field, line)
            }
            37 => Instr::Prompt,
            38 => Instr::Read,
            39 => {
                let type_name = reader.text()?;
                let line = reader.u32()?;
                Instr::ReadTyped(type_name, line)
            }
            40 => {
                let name = reader.text()?;
                let type_name = reader.text()?;
                let is_const = reader.u8()? != 0;
                let line = reader.u32()?;
                Instr::Declare(name, type_name, is_const, line)
            }
            41 => {
                let name = reader.text()?;
                let line = reader.u32()?;
                Instr::PinGuard(name, line)
            }
            42 => Instr::Interpolate(reader.u32()?),
            43 => Instr::Abort(reader.text()?),
            44 => Instr::Try(reader.u32()? as usize),
            45 => Instr::TryEnd,
            46 => Instr::ErrorCode,
            47 => Instr::ReThrow,
            48 => Instr::PopHandlers(reader.u8()?),
            49 => Instr::MarkConst(reader.text()?),
            50 => {
                let name = reader.text()?;
                let line = reader.u32()?;
                Instr::PinDeclareCheck(name, line)
            }
            51 => {
                let name = reader.text()?;
                let argc = reader.u8()?;
                let line = reader.u32()?;
                Instr::Builtin(name, argc, line)
            }
            52 => Instr::UseLib(reader.text()?),
            53 => {
                let name = reader.text()?;
                let line = reader.u32()?;
                Instr::RequireLib(name, line)
            }
            54 => {
                let lib = reader.text()?;
                let fn_name = reader.text()?;
                let argc = reader.u8()?;
                let line = reader.u32()?;
                let err = reader.text()?;
                Instr::LibCall(lib, fn_name, argc, line, err)
            }
            55 => Instr::ReturnValue(reader.u32()?),
            56 => Instr::ReturnVoid(reader.u32()?),
            57 => {
                let index = reader.u32()? as usize;
                let argc = reader.u8()?;
                let line = reader.u32()?;
                let err = reader.text()?;
                Instr::CallValue(index, argc, line, err)
            }
            58 => {
                let kind = reader.u8()?;
                let line = reader.u32()?;
                Instr::IterItems(kind, line)
            }
            59 => {
                let kind = reader.u8()?;
                let line = reader.u32()?;
                Instr::HigherOrder(kind, line)
            }
            60 => {
                let name = reader.text()?;
                let line = reader.u32()?;
                Instr::ListArg(name, line)
            }
            61 => {
                let name = reader.text()?;
                let var = reader.text()?;
                let argc = reader.u8()?;
                let line = reader.u32()?;
                Instr::Mutate(name, var, argc, line)
            }
            62 => {
                let name = reader.text()?;
                let target = reader.u32()? as usize;
                Instr::ParamMissing(name, target)
            }
            63 => Instr::List(reader.u32()? as usize),
            64 => Instr::Tuple(reader.u32()? as usize),
            65 => Instr::MakeFunc(reader.u32()? as usize),
            66 => {
                let name = reader.text()?;
                let index = reader.u32()? as usize;
                let line = reader.u32()?;
                Instr::LoadFn(name, index, line)
            }
            67 => {
                let name = reader.text()?;
                let line = reader.u32()?;
                let flags = reader.u8()?;
                Instr::CallableGuard(name, line, flags)
            }
            68 => {
                let name = reader.text()?;
                let argc = reader.u8()?;
                let line = reader.u32()?;
                let flags = reader.u8()?;
                Instr::CallDyn(name, argc, line, flags)
            }
            69 => {
                let var = if reader.u8()? != 0 {
                    Some(reader.text()?)
                } else {
                    None
                };
                let line = reader.u32()?;
                Instr::Reverse(var, line)
            }
            70 => Instr::ToBool(reader.u32()?),
            71 => {
                let target = reader.u32()? as usize;
                let line = reader.u32()?;
                Instr::AndJump(target, line)
            }
            72 => {
                let target = reader.u32()? as usize;
                let line = reader.u32()?;
                Instr::OrJump(target, line)
            }
            73 => {
                let count = reader.u32()? as usize;
                let mut names = Vec::with_capacity(count.min(1024));
                for _ in 0..count {
                    names.push(reader.text()?);
                }
                let line = reader.u32()?;
                Instr::Destructure(names, line)
            }
            op => return Err(malformed(&format!("မသိသော instruction {}", op))),
        };
        code.push(instr);
    }
    Ok(code)
}

impl Program {
    /// Write this program to the byte container: magic, [`FORMAT_VERSION`],
    /// every function body, then the top-level chunk.
    pub fn encode(&self) -> Result<Vec<u8>, String> {
        let mut out = Vec::new();
        out.extend_from_slice(&MAGIC);
        put_u16(&mut out, FORMAT_VERSION);
        put_u32(&mut out, self.funcs.len() as u32);
        for func in &self.funcs {
            put_str(&mut out, &func.name)?;
            put_u32(&mut out, func.params.len() as u32);
            for param in &func.params {
                put_str(&mut out, param)?;
            }
            put_u32(&mut out, func.required as u32);
            encode_code(&func.code, &mut out)?;
        }
        put_u32(&mut out, self.classes.len() as u32);
        for class in &self.classes {
            put_str(&mut out, &class.name)?;
            put_u32(&mut out, class.params.len() as u32);
            for param in &class.params {
                put_str(&mut out, param)?;
            }
            encode_code(&class.code, &mut out)?;
        }
        encode_code(&self.code, &mut out)?;
        Ok(out)
    }

    /// Read a container written by [`Program::encode`]. The format is fixed by
    /// [`FORMAT_VERSION`]; anything else is `E175`.
    pub fn decode(bytes: &[u8]) -> Result<Program, String> {
        let mut reader = Reader::new(bytes);
        let magic = reader.take(MAGIC.len())?;
        if magic != MAGIC {
            return Err(malformed("AKKBC ခေါင်းစီး မတွေ့ပါ"));
        }
        let version = reader.u16()?;
        if version != FORMAT_VERSION {
            return Err(format!(
                "E175 bytecode ဗားရှင်း {} ကို မသိပါ (ဤ akk သည် {} ကို ဖတ်သည်)။",
                version, FORMAT_VERSION
            ));
        }
        let func_count = reader.u32()? as usize;
        let mut funcs = Vec::with_capacity(func_count.min(4096));
        for _ in 0..func_count {
            let name = reader.text()?;
            let param_count = reader.u32()? as usize;
            let mut params = Vec::with_capacity(param_count.min(256));
            for _ in 0..param_count {
                params.push(reader.text()?);
            }
            let required = reader.u32()? as usize;
            let code = decode_code(&mut reader)?;
            funcs.push(FuncDef {
                name,
                params,
                required,
                code,
            });
        }
        let class_count = reader.u32()? as usize;
        let mut classes = Vec::with_capacity(class_count.min(4096));
        for _ in 0..class_count {
            let name = reader.text()?;
            let param_count = reader.u32()? as usize;
            let mut params = Vec::with_capacity(param_count.min(256));
            for _ in 0..param_count {
                params.push(reader.text()?);
            }
            let code = decode_code(&mut reader)?;
            classes.push(ClassDef { name, params, code });
        }
        let code = decode_code(&mut reader)?;
        if !reader.done() {
            return Err(malformed("အပိုဘိုက်များ ပါနေပါသည်"));
        }
        Ok(Program {
            code,
            funcs,
            classes,
        })
    }
}
