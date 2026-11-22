//! The stack machine that executes a compiled `Program`.

use super::*;
use crate::interp::{value_contains, value_eq, Callable, Interpreter, Key, ObjectValue, Value};
use indexmap::{IndexMap, IndexSet};
use std::collections::HashMap;
use std::rc::Rc;

/// One live `try` region: where to resume when an error happens, and the shape
/// of the world at the moment the region started, so unwinding can restore it.
pub(crate) struct Handler {
    frames: usize,
    stack_len: usize,
    pc: usize,
}

pub(crate) struct Frame {
    func: usize,
    pc: usize,
    locals: HashMap<String, Value>,
    /// `Some(slot)` when this frame is a class constructor: `အတု <field> သည်
    /// ... ဖြစ်၏။` writes into `self_fields`, and the frame's `Return`
    /// produces the object.
    class: Option<usize>,
    self_fields: Vec<(String, Value)>,
    /// Set by `CallValue`: the `E071` message to raise if this body returns
    /// without a value.
    value_err: Option<String>,
    /// True for a library's init function: its top-level declarations belong
    /// to the program's globals, exactly as they do when the interpreter runs
    /// the library at depth zero.
    global: bool,
}

/// One mutating builtin on an already-read list: the interpreter's own rules,
/// including its `E110`/`E049` messages and the value each call returns.
pub(crate) fn mutate_list(
    name: &str,
    items: &mut Vec<Value>,
    mut extra: Vec<Value>,
    line: usize,
) -> Result<Value, String> {
    use crate::interp::{eng_builtin_err, index_as_int, seq_slot, sort_items};
    match name {
        "push" | "append" => {
            if extra.len() != 1 {
                return Err(eng_builtin_err(
                    line,
                    name,
                    &format!("စာရင်း နှင့် တန်ဖိုး တစ်ခု လိုအပ်ပါသည်၊ တန်ဖိုး {} ခု ရပါသည်", extra.len()),
                ));
            }
            items.push(extra.pop().expect("one value"));
            Ok(Value::Int(items.len() as i64))
        }
        "pop" => {
            if !extra.is_empty() {
                return Err(eng_builtin_err(
                    line,
                    name,
                    "စာရင်း တစ်ခုသာ လိုအပ်ပြီး အခြားအချက် မလိုပါ",
                ));
            }
            match items.pop() {
                Some(v) => Ok(v),
                None => Err(eng_builtin_err(line, name, "စာရင်းအလွတ်မှ ဖယ်၍မရပါ")),
            }
        }
        "insert" => {
            if extra.len() != 2 {
                return Err(eng_builtin_err(
                    line,
                    name,
                    &format!(
                        "စာရင်း၊ အညွှန်း နှင့် တန်ဖိုး တစ်ခု လိုအပ်ပါသည်၊ စာရင်းနောက်တွင် အချက် {} ခု ရပါသည်",
                        extra.len()
                    ),
                ));
            }
            let value = extra.pop().expect("the value");
            let at = index_as_int(&extra[0], line)?;
            if at < 0 || at as usize > items.len() {
                return Err(format!(
                    "E049 လိုင်း {} တွင် အညွှန်း {} သည် အကွာအဝေးပြင်ပတွင် ရှိနေပါသည်။",
                    line, at
                ));
            }
            items.insert(at as usize, value);
            Ok(Value::Int(items.len() as i64))
        }
        "remove" => {
            if extra.len() != 1 {
                return Err(eng_builtin_err(
                    line,
                    name,
                    &format!("စာရင်း နှင့် အညွှန်း တစ်ခု လိုအပ်ပါသည်၊ အညွှန်း {} ခု ရပါသည်", extra.len()),
                ));
            }
            let at = index_as_int(&extra[0], line)?;
            let slot = seq_slot(items.len(), at, line)?;
            Ok(items.remove(slot))
        }
        "sort" => {
            if !extra.is_empty() {
                return Err(eng_builtin_err(
                    line,
                    name,
                    "စာရင်း တစ်ခုသာ လိုအပ်ပြီး အခြားအချက် မလိုပါ",
                ));
            }
            sort_items(items, line, name)?;
            Ok(Value::List(items.clone()))
        }
        other => Err(crate::interp::lib_fn_unknown_err(other, other, line)),
    }
}

/// What a callback call did: returned a value, or left the call some other
/// way (an error was handed to a handler below it, or the program halted).
pub(crate) enum CallbackOutcome {
    Value(Value),
    Left,
}

/// One line of program output, rendered by the interpreter's own `display`,
/// so a printed dictionary (or list) reads exactly as it does there.
pub(crate) fn render(value: &Value) -> String {
    crate::interp::display(value)
}

pub(crate) fn truthy(value: &Value) -> Result<bool, String> {
    match value {
        Value::Bool(b) => Ok(*b),
        _ => Err(bool_expected(0)),
    }
}

/// `bool_operand` from the interpreter, with the same `E043` text: a value
/// used where a condition is required must be a bool.
pub(crate) fn bool_operand_vm(value: &Value, line: usize) -> Result<bool, String> {
    match value {
        Value::Bool(b) => Ok(*b),
        other => Err(format!(
            "E043 လိုင်း {} တွင် {} တန်ဖိုးကို အခြေအနေအဖြစ် (မှန်/မှား စစ်ရန်) သုံး၍မရပါ။",
            line,
            crate::interp::type_name_mm(other)
        )),
    }
}

/// The error for a call whose name resolves to nothing. Each of the
/// interpreter's call paths has its own code, so the mode picks it.
pub(crate) fn missing_callee(name: &str, line: usize, mode: u8) -> String {
    match mode {
        // `... ကို လုပ်ပါ။` falls through to the missing-function error.
        1 => crate::interp::eng_missing_function_err(line, name),
        // The eng `name(args);` statement has its own error code.
        2 => format!(
            "E113 လိုင်း {} တွင် \"{}\" သည် အလုပ် သို့မဟုတ် builtin မဟုတ်ပါ။",
            line, name
        ),
        // An expression ends up in the class-not-found path.
        _ => format!("E032 လိုင်း {} တွင် \"{}\" ဆိုသော ပုံသေကို ရှာမတွေ့ပါ။", line, name),
    }
}

/// Reverse a value the way the interpreter's `reverse` does: a string by
/// clusters, a list or tuple by element. Anything else is `E111`.
pub(crate) fn reverse_value(value: Value, line: usize) -> Result<Value, String> {
    match value {
        Value::Str(s) => Ok(Value::Str(crate::text::reverse(&s))),
        Value::List(mut items) => {
            items.reverse();
            Ok(Value::List(items))
        }
        Value::Tuple(mut items) => {
            items.reverse();
            Ok(Value::Tuple(items))
        }
        other => Err(crate::interp::eng_builtin_err(
            line,
            "reverse",
            &format!(
                "စာရင်း သို့မဟုတ် စာသား လိုအပ်ပါသည်၊ {} ရပါသည်",
                crate::interp::type_name_mm(&other)
            ),
        )),
    }
}

/// The result of `+ - *`: an integer when both operands were integers, a float
/// otherwise -- the type the interpreter keeps for those operations.
pub(crate) fn number(value: f64, both_int: bool) -> Value {
    if both_int {
        Value::Int(value as i64)
    } else {
        Value::Float(value)
    }
}

pub(crate) fn numbers(a: &Value, b: &Value) -> Option<(f64, f64)> {
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => Some((*x as f64, *y as f64)),
        (Value::Int(x), Value::Float(y)) => Some((*x as f64, *y)),
        (Value::Float(x), Value::Int(y)) => Some((*x, *y as f64)),
        (Value::Float(x), Value::Float(y)) => Some((*x, *y)),
        _ => None,
    }
}

pub(crate) fn values_equal(a: &Value, b: &Value) -> bool {
    value_eq(a, b)
}

/// Indexed writes and reads go through the interpreter's own `set_index` and
/// `index_value` (see [`Instr::Index`] / [`Instr::IndexStore`]), so a dict
/// write, a missing key and an out-of-range list index all fail with the very
/// same `E0xx` text in both backends.
/// An indexed write runs the interpreter's own `set_index`, so a dict write, a
/// wrong-arity key path and an out-of-range list slot fail with the very same
/// `E0xx` text in both backends.
pub(crate) fn set_index_vm(
    base: &mut Value,
    keys: &[Value],
    value: Value,
    line: usize,
) -> Result<(), String> {
    crate::interp::set_index(base, keys, value, line)
}

/// The stack machine. [`Vm::run`] executes until `Halt`.
pub struct Vm<'p> {
    prog: &'p Program,
    stack: Vec<Value>,
    globals: HashMap<String, Value>,
    frames: Vec<Frame>,
    main_pc: usize,
    /// Everything the program printed, one entry per line.
    pub out: Vec<String>,
    /// Write output to stdout as it is produced, instead of collecting it in
    /// `out` for after the run. A real run streams, so a prompt is on screen
    /// before the read blocks.
    stream: bool,
    /// The names declared with `pin`, so a later write is refused with the
    /// interpreter's `E104`. The interpreter keeps one set for the whole run,
    /// so this does too.
    decls: HashMap<String, ()>,
    /// The live `try` regions, innermost last.
    handlers: Vec<Handler>,
    /// The libraries imported so far, by every name they answer to.
    libs: std::collections::HashSet<String>,
    /// The script/package libraries whose init function is running right now.
    /// Importing one of these again is the interpreter's `E119`; a library
    /// leaves this set (and joins `libs`) only when its init finishes.
    libs_loading: std::collections::HashSet<String>,
    /// The message of the error a handler is currently running for.
    pending_error: Option<String>,
    /// Set when the program has stopped (its `Halt`, or a `Return` out of the
    /// top-level chunk), so the run loop can tell that from an error.
    halted: bool,
    /// Everything the program wrote, byte for byte -- the `.akop` transcript.
    pub log: String,
    /// A scripted stdin (tests); `None` reads the process's own stdin.
    input: Option<Box<dyn std::io::BufRead>>,
    /// Set while a nested run is going: it stops as soon as the frame stack
    /// drops back to this depth, so the callback returns to its caller.
    stop_depth: Option<usize>,
}

impl<'p> Vm<'p> {
    pub fn new(prog: &'p Program) -> Self {
        Vm {
            prog,
            stack: Vec::new(),
            globals: HashMap::new(),
            frames: Vec::new(),
            main_pc: 0,
            out: Vec::new(),
            stream: false,
            decls: HashMap::new(),
            handlers: Vec::new(),
            libs: std::collections::HashSet::new(),
            libs_loading: std::collections::HashSet::new(),
            pending_error: None,
            halted: false,
            log: String::new(),
            input: None,
            stop_depth: None,
        }
    }

    /// A VM that writes to stdout as the program runs, for `akk --vm` and for
    /// a built bytecode artifact.
    pub fn streaming(prog: &'p Program) -> Self {
        let mut vm = Vm::new(prog);
        vm.stream = true;
        vm
    }

    /// Feed this VM scripted stdin instead of the process's own.
    #[cfg(test)]
    pub fn feed(&mut self, text: &str) {
        self.input = Some(Box::new(std::io::Cursor::new(text.as_bytes().to_vec())));
    }

    /// Show program text: always recorded in `log`, and written to stdout now
    /// when the VM is streaming (a prompt must precede the read it belongs to).
    fn write_out(&mut self, text: &str) {
        self.log.push_str(text);
        if self.stream {
            use std::io::Write;
            let stdout = std::io::stdout();
            let mut lock = stdout.lock();
            let _ = lock.write_all(text.as_bytes());
            let _ = lock.flush();
        }
    }

    /// One line of stdin, without its line ending. EOF reads as an empty line,
    /// which is what the interpreter's `read_line` leaves behind.
    fn read_input(&mut self) -> String {
        let mut buf = String::new();
        match self.input.as_mut() {
            Some(reader) => {
                let _ = reader.read_line(&mut buf);
            }
            None => {
                let _ = std::io::stdin().read_line(&mut buf);
            }
        }
        buf
    }

    fn pop(&mut self) -> Value {
        self.stack.pop().expect("stack underflow")
    }

    fn load(&self, name: &str, line: usize) -> Result<Value, String> {
        // The interpreter's scope chain is dynamic: a callee sees the frames
        // of its callers, so a function may read a caller's local. Walk the
        // frames from innermost outward, then the globals.
        for frame in self.frames.iter().rev() {
            if let Some(v) = frame.locals.get(name) {
                return Ok(v.clone());
            }
        }
        self.globals
            .get(name)
            .cloned()
            .ok_or_else(|| unknown_var(name, line))
    }

    /// Pop the frame a `return` is leaving and hand `value` back to the
    /// caller. `produced` says whether the body actually yielded a value; when
    /// the call demanded one (`CallValue`) and it did not, that is `E071`,
    /// raised exactly where the interpreter raises it.
    fn leave_frame(&mut self, value: Value, line: u32, produced: bool) -> Result<(), String> {
        if self.frames.is_empty() {
            // `return` outside any function is `E108`, exactly as the
            // interpreter reports it.
            return Err(crate::interp::eng_return_outside_fn_err(line as usize));
        }
        let frame = self.frames.pop().expect("a frame was just checked");
        if !produced {
            if let Some(err) = frame.value_err {
                return Err(err);
            }
        }
        // A constructor's result is the object it filled in, not whatever its
        // last expression evaluated to.
        let value = match frame.class {
            Some(index) => Value::Object(Rc::new(ObjectValue {
                class: self.prog.classes[index].name.clone(),
                fields: frame.self_fields,
            })),
            None => value,
        };
        // Back in the caller, right after its call.
        self.stack.push(value);
        Ok(())
    }

    /// The items `map`/`filter`/`reduce` walk, with the interpreter's own
    /// `E110` for a value that cannot be walked.
    fn iter_items(kind: u8, collection: &Value, line: usize) -> Result<Vec<Value>, String> {
        match collection {
            Value::List(items) | Value::Tuple(items) => Ok(items.clone()),
            Value::Set(items) => Ok(items.iter().map(|k| k.value().clone()).collect()),
            Value::Dict(pairs) => Ok(pairs.keys().map(|k| k.value().clone()).collect()),
            Value::Str(text) => Ok(crate::text::clusters(text)
                .into_iter()
                .map(|c| Value::Str(c.to_string()))
                .collect()),
            other => Err(crate::interp::eng_builtin_err(
                line,
                higher_order_name(kind),
                &format!(
                    "စုစည်းမှု သို့မဟုတ် စာသား လိုအပ်ပါသည်၊ {} ရပါသည်",
                    crate::interp::type_name_mm(other)
                ),
            )),
        }
    }

    /// Run the callback function `index` with `args`, the way the interpreter's
    /// `call_callback` does: a fresh frame, the interpreter's `E033` for the
    /// wrong argument count, and `err` (its `E114`) when the body returns
    /// nothing. `Left` says control did not come back from the call -- an
    /// error was handed to a `try` handler below it, or the program halted --
    /// so the caller must not touch the stack.
    /// Start a call: check the argument count, push a fresh frame and bind the
    /// parameters. `next_pc` is where the caller resumes once the callee
    /// returns; `value_err` (when present) turns a body that yields nothing
    /// into the interpreter's `E114`.
    fn enter_call(
        &mut self,
        index: usize,
        args: Vec<Value>,
        value_err: Option<String>,
        line: u32,
        next_pc: usize,
    ) -> Result<(), String> {
        let func = &self.prog.funcs[index];
        let name = func.name.clone();
        let params = func.params.clone();
        let required = func.required;
        if args.len() < required || args.len() > params.len() {
            return Err(crate::interp::eng_arity_err(
                &name,
                args.len(),
                required,
                params.len(),
                line as usize,
            ));
        }
        self.set_pc(next_pc);
        self.frames.push(Frame {
            func: index,
            pc: 0,
            locals: HashMap::new(),
            class: None,
            self_fields: Vec::new(),
            value_err,
            global: false,
        });
        self.bind_params(&params, args);
        Ok(())
    }

    /// Bind a call's parameters with the interpreter's `bind_param` rule: a
    /// name that has no binding yet is written into the shared environment
    /// without a frame log entry, so it outlives the call (the interpreter's
    /// own quirk, which makes a returned lambda that reads a parameter work);
    /// a name that already has a binding is restored when the frame pops.
    fn bind_params(&mut self, params: &[String], args: Vec<Value>) {
        for (name, value) in params.iter().zip(args) {
            let existed = self
                .frames
                .iter()
                .rev()
                .any(|frame| frame.locals.contains_key(name))
                || self.globals.contains_key(name);
            if !existed {
                self.globals.insert(name.clone(), value.clone());
            }
            self.frames
                .last_mut()
                .expect("the frame was just pushed")
                .locals
                .insert(name.clone(), value);
        }
    }

    /// Call the callable `name` holds, after [`Instr::CallableGuard`] checked
    /// that it is one. The arguments are already evaluated, as the
    /// interpreter's `invoke_callable` has them by this point.
    fn call_dynamic(
        &mut self,
        name: &str,
        args: Vec<Value>,
        line: u32,
        mode: u8,
    ) -> Result<(), String> {
        let value = match self.load(name, line as usize) {
            Ok(value) => value,
            Err(_) => return Err(missing_callee(name, line as usize, mode)),
        };
        let callable = match value {
            Value::Callable(kind) => kind,
            _ => return Err(missing_callee(name, line as usize, mode)),
        };
        let (index, label) = match &*callable {
            Callable::Compiled { index, label } => (*index, label.clone()),
            other => {
                return Err(format!(
                    "E170 လိုင်း {} တွင် \"{}\" ကို bytecode စက် ဖြင့် ခေါ်၍မရပါ။",
                    line,
                    other.label()
                ))
            }
        };
        // Every callable call wants a value: the interpreter's
        // `invoke_callable` raises `E114` when the body yields none, even
        // for a statement whose result is discarded.
        let value_err = Some(crate::interp::eng_no_return_err(&label, line as usize));
        // The arm already moved past the call instruction, so the caller
        // resumes exactly where the pc points now.
        let next_pc = self.current_pc();
        self.enter_call(index, args, value_err, line, next_pc)
    }

    fn call_callback(
        &mut self,
        kind: &Callable,
        args: Vec<Value>,
        line: u32,
    ) -> Result<CallbackOutcome, String> {
        let (index, label) = match kind {
            Callable::Compiled { index, label } => (*index, label.clone()),
            other => {
                return Err(format!(
                    "E170 လိုင်း {} တွင် \"{}\" ကို bytecode စက် ဖြင့် ခေါ်၍မရပါ။",
                    line,
                    other.label()
                ))
            }
        };
        let func = &self.prog.funcs[index];
        let params = func.params.clone();
        let required = func.required;
        if args.len() < required || args.len() > params.len() {
            return Err(crate::interp::eng_arity_err(
                &label,
                args.len(),
                required,
                params.len(),
                line as usize,
            ));
        }
        let base = self.frames.len();
        // The stack before the frame: a normal return leaves exactly the one
        // value it produced on top of it, which is how a callback's own
        // return is told from control leaving the call another way (an error
        // handed to a `try` handler below it, or a halt).
        let stack_base = self.stack.len();
        self.frames.push(Frame {
            func: index,
            pc: 0,
            locals: HashMap::new(),
            class: None,
            self_fields: Vec::new(),
            value_err: Some(crate::interp::eng_no_return_err(&label, line as usize)),
            global: false,
        });
        self.bind_params(&params, args);
        let saved_stop = self.stop_depth;
        self.stop_depth = Some(base);
        let result = self.run();
        self.stop_depth = saved_stop;
        result?;
        if self.frames.len() != base || self.stack.len() != stack_base + 1 {
            return Ok(CallbackOutcome::Left);
        }
        Ok(CallbackOutcome::Value(self.pop()))
    }

    fn store(&mut self, name: &str, value: Value) {
        match self.frames.last_mut() {
            Some(frame) if !frame.global => {
                frame.locals.insert(name.to_string(), value);
            }
            _ => {
                self.globals.insert(name.to_string(), value);
            }
        }
    }

    /// Run one library's compiled top-level statements in its own source, at
    /// the moment its import executes. The frame's store goes to the globals
    /// (the interpreter runs a library at call depth zero), and its source
    /// travels with the frame, so an error there names the library file and
    /// line. Returns after the init function does, or with its annotated
    /// error -- the caller's frame is untouched either way.
    fn run_library_init(&mut self, index: usize) -> Result<(), String> {
        let base = self.frames.len();
        let stack_base = self.stack.len();
        self.frames.push(Frame {
            func: index,
            pc: 0,
            locals: HashMap::new(),
            class: None,
            self_fields: Vec::new(),
            value_err: None,
            global: true,
        });
        let saved_stop = self.stop_depth;
        self.stop_depth = Some(base);
        let result = self.run();
        self.stop_depth = saved_stop;
        result?;
        // The init function returns a value like every chunk does; the import
        // statement has nothing to push, so drop it.
        self.stack.truncate(stack_base);
        Ok(())
    }

    /// Hand `message` to the innermost live `try` handler.
    ///
    /// Returns `true` when a handler took over: the frames opened inside the
    /// region are dropped, the value stack is cut back to its depth at the
    /// `try`, the message is kept for `catch`/`ReThrow`, and execution resumes
    /// at the handler. Returns `false` when no handler is left, and the caller
    /// reports the error as uncaught.
    fn unwind(&mut self, message: &str) -> bool {
        let Some(handler) = self.handlers.pop() else {
            return false;
        };
        while self.frames.len() > handler.frames {
            self.frames.pop();
        }
        self.stack.truncate(handler.stack_len);
        self.pending_error = Some(message.to_string());
        self.set_pc(handler.pc);
        true
    }

    /// The file whose instruction is running: the class constructor or
    /// function of the top frame, or the top-level chunk. An error is rendered
    /// against it, so a function body points at the file it was declared in.
    fn current_source(&self) -> &str {
        match self.frames.last() {
            Some(frame) => match frame.class {
                Some(slot) => &self.prog.classes[slot].source,
                None => &self.prog.funcs[frame.func].source,
            },
            None => &self.prog.source,
        }
    }

    fn current_pc(&self) -> usize {
        match self.frames.last() {
            Some(frame) => frame.pc,
            None => self.main_pc,
        }
    }

    fn set_pc(&mut self, pc: usize) {
        match self.frames.last_mut() {
            Some(frame) => frame.pc = pc,
            None => self.main_pc = pc,
        }
    }

    fn arith(&mut self, op: char, line: u32) -> Result<(), String> {
        let rhs = self.pop();
        let lhs = self.pop();
        if op == '+' {
            if let (Value::Str(a), Value::Str(b)) = (&lhs, &rhs) {
                self.stack.push(Value::Str(format!("{}{}", a, b)));
                return Ok(());
            }
        }
        let Some((a, b)) = numbers(&lhs, &rhs) else {
            return Err(math_type_err(op));
        };
        let both_int = matches!(lhs, Value::Int(_)) && matches!(rhs, Value::Int(_));
        let line = line as usize;
        // The interpreter's own rules: integer `+ - *` stay integers, `/`
        // always yields a float, and `%` is the Euclidean remainder. A zero
        // divisor is `E036`/`E038` for integers and `E037`/`E039` for floats,
        // each with the line the operation was written on.
        let value = match op {
            '+' => number(a + b, both_int),
            '-' => number(a - b, both_int),
            '*' => number(a * b, both_int),
            '/' => {
                if b == 0.0 {
                    return Err(if both_int {
                        format!("E036 လိုင်း {} တွင် သုညဖြင့် စား၍မရပါ။", line)
                    } else {
                        format!("E037 လိုင်း {} တွင် သုညဖြင့် စား၍မရပါ။", line)
                    });
                }
                Value::Float(a / b)
            }
            '%' => {
                if b == 0.0 {
                    return Err(if both_int {
                        format!("E038 လိုင်း {} တွင် သုညဖြင့် ကြွင်းကိန်းရှာ၍မရပါ။", line)
                    } else {
                        format!("E039 လိုင်း {} တွင် သုညဖြင့် ကြွင်းကိန်းရှာ၍မရပါ။", line)
                    });
                }
                if both_int {
                    Value::Int((a as i64).rem_euclid(b as i64))
                } else {
                    Value::Float(a.rem_euclid(b))
                }
            }
            // `//`: division then `floor`, the interpreter's `\` operator.
            // Integers stay integers; a float operand gives a float floor.
            '\\' => {
                if b == 0.0 {
                    return Err(if both_int {
                        format!("E073 လိုင်း {} တွင် သုညဖြင့် အပြည့်ကိန်းစား၍မရပါ။", line)
                    } else {
                        format!("E074 လိုင်း {} တွင် သုညဖြင့် အပြည့်ကိန်းစား၍မရပါ။", line)
                    });
                }
                if both_int {
                    Value::Int((a / b).floor() as i64)
                } else {
                    Value::Float((a / b).floor())
                }
            }
            other => return Err(math_type_err(other)),
        };
        self.stack.push(value);
        Ok(())
    }

    fn compare(&mut self, want: fn(std::cmp::Ordering) -> bool) -> Result<(), String> {
        let rhs = self.pop();
        let lhs = self.pop();
        let ordering = match (&lhs, &rhs) {
            (Value::Str(a), Value::Str(b)) => a.cmp(b),
            _ => {
                let Some((a, b)) = numbers(&lhs, &rhs) else {
                    return Err("E172 တွင် နှိုင်းယှဉ်၍မရပါ။".to_string());
                };
                a.partial_cmp(&b).unwrap_or(std::cmp::Ordering::Equal)
            }
        };
        self.stack.push(Value::Bool(want(ordering)));
        Ok(())
    }

    /// Run to `Halt` (or off the end of the main chunk).
    pub fn run(&mut self) -> Result<(), String> {
        loop {
            if let Some(depth) = self.stop_depth {
                if self.frames.len() <= depth {
                    return Ok(());
                }
            }
            let pc = self.current_pc();
            let instr = {
                let code = match self.frames.last() {
                    Some(frame) => match frame.class {
                        Some(slot) => &self.prog.classes[slot].code,
                        None => &self.prog.funcs[frame.func].code,
                    },
                    None => &self.prog.code,
                };
                if pc >= code.len() {
                    return Ok(());
                }
                code[pc].clone()
            };
            // Each instruction runs in its own closure, so an error it
            // returns can be handed to a `try` handler instead of ending
            // the run. The arms keep using `?` and `return Err`, which is
            // what makes them report the interpreter's own text.
            let outcome: Result<(), String> = (|| {
                match instr {
                    Instr::Const(v) => {
                        self.stack.push(v);
                        self.set_pc(pc + 1);
                    }
                    Instr::Load(name, line) => {
                        let v = self.load(&name, line as usize)?;
                        self.stack.push(v);
                        self.set_pc(pc + 1);
                    }
                    Instr::Store(name) => {
                        let v = self.pop();
                        self.store(&name, v);
                        self.set_pc(pc + 1);
                    }
                    Instr::Destructure(names, line) => {
                        let value = self.pop();
                        let items = match &value {
                            Value::List(items) | Value::Tuple(items) => items.clone(),
                            other => {
                                return Err(crate::interp::destructure_not_collection_err(
                                    line as usize,
                                    crate::interp::type_name_mm(other),
                                ))
                            }
                        };
                        if items.len() != names.len() {
                            return Err(crate::interp::destructure_count_err(
                                line as usize,
                                names.len(),
                                items.len(),
                            ));
                        }
                        for (name, item) in names.iter().zip(items.into_iter()) {
                            if self.decls.contains_key(name) {
                                return Err(crate::interp::eng_const_reassign_err(
                                    name,
                                    line as usize,
                                ));
                            }
                            self.store(name, item);
                        }
                        self.set_pc(pc + 1);
                    }
                    Instr::Pop => {
                        self.pop();
                        self.set_pc(pc + 1);
                    }
                    Instr::Add => {
                        self.arith('+', 0)?;
                        self.set_pc(pc + 1);
                    }
                    Instr::Sub => {
                        self.arith('-', 0)?;
                        self.set_pc(pc + 1);
                    }
                    Instr::Mul => {
                        self.arith('*', 0)?;
                        self.set_pc(pc + 1);
                    }
                    Instr::Div(line) => {
                        self.arith('/', line)?;
                        self.set_pc(pc + 1);
                    }
                    Instr::FloorDiv(line) => {
                        self.arith('\\', line)?;
                        self.set_pc(pc + 1);
                    }
                    Instr::Mod(line) => {
                        self.arith('%', line)?;
                        self.set_pc(pc + 1);
                    }
                    Instr::Neg => {
                        let v = self.pop();
                        let value = match v {
                            Value::Int(n) => Value::Int(-n),
                            Value::Float(f) => Value::Float(-f),
                            _ => return Err("E172 တွင် အနုတ်လက္ခဏာ မထည့်နိုင်ပါ။".to_string()),
                        };
                        self.stack.push(value);
                        self.set_pc(pc + 1);
                    }
                    Instr::Not(line) => {
                        let v = self.pop();
                        let b = bool_operand_vm(&v, line as usize)?;
                        self.stack.push(Value::Bool(!b));
                        self.set_pc(pc + 1);
                    }
                    Instr::ToBool(line) => {
                        let v = self.pop();
                        let b = bool_operand_vm(&v, line as usize)?;
                        self.stack.push(Value::Bool(b));
                        self.set_pc(pc + 1);
                    }
                    Instr::AndJump(target, line) => {
                        let v = self.pop();
                        if bool_operand_vm(&v, line as usize)? {
                            self.set_pc(pc + 1);
                        } else {
                            self.stack.push(Value::Bool(false));
                            self.set_pc(target);
                        }
                    }
                    Instr::OrJump(target, line) => {
                        let v = self.pop();
                        if bool_operand_vm(&v, line as usize)? {
                            self.stack.push(Value::Bool(true));
                            self.set_pc(target);
                        } else {
                            self.set_pc(pc + 1);
                        }
                    }
                    Instr::MakeFunc(index) => {
                        let label = self.prog.funcs[index].name.clone();
                        self.stack.push(Value::Callable(Rc::new(Callable::Compiled {
                            index,
                            label,
                        })));
                        self.set_pc(pc + 1);
                    }
                    Instr::LoadFn(name, index, line) => {
                        // A live variable wins over the function of the same name,
                        // exactly as the interpreter's `Expr::Ident` says.
                        let value = match self.load(&name, line as usize) {
                            Ok(value) => value,
                            Err(_) => {
                                Value::Callable(Rc::new(Callable::Compiled { index, label: name }))
                            }
                        };
                        self.stack.push(value);
                        self.set_pc(pc + 1);
                    }
                    Instr::CallableGuard(name, line, mode) => {
                        let value = match self.load(&name, line as usize) {
                            Ok(value) => value,
                            Err(_) => return Err(missing_callee(&name, line as usize, mode)),
                        };
                        if !matches!(value, Value::Callable(_)) {
                            return Err(missing_callee(&name, line as usize, mode));
                        }
                        self.set_pc(pc + 1);
                    }
                    Instr::CallDyn(name, argc, line, mode) => {
                        let mut args = Vec::with_capacity(argc as usize);
                        for _ in 0..argc {
                            args.push(self.pop());
                        }
                        args.reverse();
                        self.set_pc(pc + 1);
                        self.call_dynamic(&name, args, line, mode)?;
                    }
                    Instr::Reverse(var, line) => {
                        let value = self.pop();
                        let in_place = matches!(value, Value::List(_));
                        let reversed = reverse_value(value, line as usize)?;
                        if in_place {
                            if let Some(name) = var {
                                if self.decls.contains_key(&name) {
                                    return Err(crate::interp::eng_const_reassign_err(
                                        &name,
                                        line as usize,
                                    ));
                                }
                                self.store(&name, reversed.clone());
                            }
                        }
                        self.stack.push(reversed);
                        self.set_pc(pc + 1);
                    }
                    Instr::And => {
                        let b = truthy(&self.pop())?;
                        let a = truthy(&self.pop())?;
                        self.stack.push(Value::Bool(a && b));
                        self.set_pc(pc + 1);
                    }
                    Instr::Or => {
                        let b = truthy(&self.pop())?;
                        let a = truthy(&self.pop())?;
                        self.stack.push(Value::Bool(a || b));
                        self.set_pc(pc + 1);
                    }
                    Instr::Eq => {
                        let b = self.pop();
                        let a = self.pop();
                        self.stack.push(Value::Bool(values_equal(&a, &b)));
                        self.set_pc(pc + 1);
                    }
                    Instr::Ne => {
                        let b = self.pop();
                        let a = self.pop();
                        self.stack.push(Value::Bool(!values_equal(&a, &b)));
                        self.set_pc(pc + 1);
                    }
                    Instr::Lt => {
                        self.compare(|o| o == std::cmp::Ordering::Less)?;
                        self.set_pc(pc + 1);
                    }
                    Instr::Le => {
                        self.compare(|o| o != std::cmp::Ordering::Greater)?;
                        self.set_pc(pc + 1);
                    }
                    Instr::Gt => {
                        self.compare(|o| o == std::cmp::Ordering::Greater)?;
                        self.set_pc(pc + 1);
                    }
                    Instr::Ge => {
                        self.compare(|o| o != std::cmp::Ordering::Less)?;
                        self.set_pc(pc + 1);
                    }
                    Instr::JumpIfTrue(target) => {
                        let v = self.pop();
                        self.set_pc(if truthy(&v)? { target } else { pc + 1 });
                    }
                    Instr::JumpIfFalse(target) => {
                        let v = self.pop();
                        self.set_pc(if truthy(&v)? { pc + 1 } else { target });
                    }
                    Instr::Jump(target) => self.set_pc(target),
                    Instr::Print => {
                        let v = self.pop();
                        let line = render(&v);
                        self.out.push(line.clone());
                        self.write_out(&format!("{}\n", line));
                        self.set_pc(pc + 1);
                    }
                    Instr::Prompt => {
                        let v = self.pop();
                        let text = render(&v);
                        self.write_out(&text);
                        self.set_pc(pc + 1);
                    }
                    Instr::Read => {
                        let line = self.read_input();
                        let value = crate::interp::infer_value(line.trim());
                        self.stack.push(value);
                        self.set_pc(pc + 1);
                    }
                    Instr::ReadTyped(type_name, line) => {
                        let text = self.read_input();
                        let value = crate::interp::coerce_eng_input(
                            text.trim(),
                            &type_name,
                            line as usize,
                        )?;
                        self.stack.push(value);
                        self.set_pc(pc + 1);
                    }
                    Instr::Declare(name, type_name, is_const, line) => {
                        let value = self.pop();
                        let value = crate::interp::check_eng_value(
                            &name,
                            &type_name,
                            value,
                            line as usize,
                        )?;
                        if is_const {
                            let exists = self.decls.contains_key(&name)
                                || self.globals.contains_key(&name)
                                || self
                                    .frames
                                    .last()
                                    .map(|frame| frame.locals.contains_key(&name))
                                    .unwrap_or(false);
                            if exists {
                                return Err(crate::interp::eng_pin_redeclare_err(
                                    &name,
                                    line as usize,
                                ));
                            }
                            self.decls.insert(name.clone(), ());
                        }
                        self.store(&name, value);
                        self.set_pc(pc + 1);
                    }
                    Instr::Builtin(name, argc, line) => {
                        let mut args = Vec::with_capacity(argc as usize);
                        for _ in 0..argc {
                            args.push(self.pop());
                        }
                        args.reverse();
                        let value = Interpreter::builtin_value_result(&name, &args, line as usize)?;
                        self.stack.push(value);
                        self.set_pc(pc + 1);
                    }
                    Instr::UseLib(name) => {
                        let builtin = crate::project::library::builtin_names(&name);
                        let already = match builtin {
                            Some(names) => {
                                names.iter().all(|spelling| self.libs.contains(*spelling))
                            }
                            None => self.libs.contains(&name),
                        };
                        if already {
                            self.set_pc(pc + 1);
                        } else if let Some(names) = builtin {
                            for spelling in names {
                                self.libs.insert((*spelling).to_string());
                            }
                            self.set_pc(pc + 1);
                        } else if let Some(&index) = self.prog.script_libs.get(&name) {
                            if self.libs_loading.contains(&name) {
                                // The interpreter's cycle rule: the library is
                                // mid load, so this import is `E119` at this
                                // line, in the importing library's own source.
                                let line = self
                                    .frames
                                    .last()
                                    .and_then(|frame| {
                                        self.prog.lib_imports.get(&(frame.func, pc)).copied()
                                    })
                                    .unwrap_or(0);
                                return Err(crate::interp::lib_cycle_err(&name, line));
                            }
                            self.libs_loading.insert(name.clone());
                            self.set_pc(pc + 1);
                            let outcome = self.run_library_init(index);
                            self.libs_loading.remove(&name);
                            match outcome {
                                // Only a run that finished normally marks the
                                // library loaded, exactly as the interpreter.
                                Ok(()) if self.pending_error.is_none() => {
                                    self.libs.insert(name);
                                }
                                // The error went to an enclosing `try`; the
                                // import stays failed, like the interpreter's.
                                Ok(()) => {}
                                Err(e) => return Err(e),
                            }
                        } else {
                            // No compiled chunk (a name only the interpreter
                            // could resolve): mark it imported, as before.
                            self.libs.insert(name.clone());
                            self.set_pc(pc + 1);
                        }
                    }
                    Instr::RequireLib(name, line) => {
                        if !self.libs.contains(&name) {
                            return Err(crate::interp::lib_not_imported_err(&name, line as usize));
                        }
                        self.set_pc(pc + 1);
                    }
                    Instr::LibCall(lib, fn_name, argc, line, err) => {
                        let mut args = Vec::with_capacity(argc as usize);
                        for _ in 0..argc {
                            args.push(self.pop());
                        }
                        args.reverse();
                        match crate::interp::lib_value_call(&lib, &fn_name, &args, line as usize)? {
                            Some(value) => self.stack.push(value),
                            None if err.is_empty() => self.stack.push(Value::Null),
                            None => return Err(err.clone()),
                        }
                        self.set_pc(pc + 1);
                    }
                    Instr::MarkConst(name) => {
                        self.decls.insert(name, ());
                        self.set_pc(pc + 1);
                    }
                    Instr::PinDeclareCheck(name, line) => {
                        let exists = self.decls.contains_key(&name)
                            || self.globals.contains_key(&name)
                            || self
                                .frames
                                .last()
                                .map(|frame| frame.locals.contains_key(&name))
                                .unwrap_or(false);
                        if exists {
                            return Err(crate::interp::eng_pin_redeclare_err(&name, line as usize));
                        }
                        self.set_pc(pc + 1);
                    }
                    Instr::PinGuard(name, line) => {
                        if self.decls.contains_key(&name) {
                            return Err(crate::interp::eng_const_reassign_err(
                                &name,
                                line as usize,
                            ));
                        }
                        self.set_pc(pc + 1);
                    }
                    Instr::Abort(message) => return Err(message),
                    Instr::Try(handler) => {
                        self.handlers.push(Handler {
                            frames: self.frames.len(),
                            stack_len: self.stack.len(),
                            pc: handler,
                        });
                        self.set_pc(pc + 1);
                    }
                    Instr::TryEnd => {
                        self.handlers.pop();
                        self.set_pc(pc + 1);
                    }
                    Instr::ErrorCode => {
                        let code = crate::interp::extract_error_code(
                            self.pending_error.as_deref().unwrap_or("E"),
                        );
                        self.stack.push(Value::Str(code));
                        self.set_pc(pc + 1);
                    }
                    Instr::Throw(identity, line) => {
                        let payload = self.pop();
                        return Err(crate::interp::raised_error(
                            line as usize,
                            &identity,
                            &render(&payload),
                        ));
                    }
                    Instr::ErrorMatches(name) => {
                        let message = self.pending_error.as_deref().unwrap_or("E").to_string();
                        let matched = crate::interp::error_matches(&message, &name);
                        self.stack.push(Value::Bool(matched));
                        self.set_pc(pc + 1);
                    }
                    Instr::ErrorBind => {
                        let message = self.pending_error.as_deref().unwrap_or("E").to_string();
                        self.stack
                            .push(Value::Str(crate::interp::error_binding(&message)));
                        self.set_pc(pc + 1);
                    }
                    Instr::ReThrow => {
                        let message = self.pending_error.take().unwrap_or_else(|| {
                            "E170 လိုင်း 0 တွင် မိမိဘာသာ ပြန်မတင်ႇနိူင်သော အမှားတစ်ခု ရှိနေပါသည်။".to_string()
                        });
                        return Err(message);
                    }
                    Instr::PopHandlers(count) => {
                        let keep = self.handlers.len().saturating_sub(count as usize);
                        self.handlers.truncate(keep);
                        self.set_pc(pc + 1);
                    }
                    Instr::Interpolate(line) => {
                        let value = self.pop();
                        let text = match value {
                            Value::Str(text) => text,
                            other => render(&other),
                        };
                        // The lookups only read the VM, so the borrow ends before
                        // the expanded string is pushed back on the stack.
                        let vm: &Vm = self;
                        let expanded = crate::interp::interpolate_text(
                            &text,
                            line as usize,
                            |name| {
                                vm.load(name, line as usize).map_err(|_| {
                                    crate::interp::interpolate_missing_var_err(name, line as usize)
                                })
                            },
                            |field_name, whole| {
                                crate::interp::self_field_lookup(
                                    vm.frames
                                        .last()
                                        .filter(|frame| frame.class.is_some())
                                        .map(|frame| frame.self_fields.as_slice()),
                                    field_name,
                                    whole,
                                    line as usize,
                                )
                            },
                        )?;
                        self.stack.push(Value::Str(expanded));
                        self.set_pc(pc + 1);
                    }
                    Instr::Len => {
                        let v = self.pop();
                        let n = match &v {
                            Value::Str(s) => crate::text::len(s),
                            Value::List(items) | Value::Tuple(items) => items.len() as i64,
                            Value::Dict(pairs) => pairs.len() as i64,
                            Value::Set(items) => items.len() as i64,
                            _ => return Err("E172 တွင် အရှည် မတွက်နိုင်ပါ။".to_string()),
                        };
                        self.stack.push(Value::Int(n));
                        self.set_pc(pc + 1);
                    }
                    Instr::Index(line, count) => {
                        let mut keys = Vec::with_capacity(count as usize);
                        for _ in 0..count {
                            keys.push(self.pop());
                        }
                        keys.reverse();
                        let base = self.pop();
                        let item = crate::interp::index_value(&base, &keys, line as usize)?;
                        self.stack.push(item);
                        self.set_pc(pc + 1);
                    }
                    Instr::Dict(count) => {
                        let mut pairs: IndexMap<Key, Value> = IndexMap::with_capacity(count);
                        let mut items = Vec::with_capacity(count * 2);
                        for _ in 0..count * 2 {
                            items.push(self.pop());
                        }
                        items.reverse();
                        let mut it = items.into_iter();
                        while let (Some(key), Some(value)) = (it.next(), it.next()) {
                            pairs.insert(Key::new(key), value);
                        }
                        self.stack.push(Value::Dict(Rc::new(pairs)));
                        self.set_pc(pc + 1);
                    }
                    Instr::Contains(line) => {
                        let needle = self.pop();
                        let haystack = self.pop();
                        let found = value_contains(&haystack, &needle, line as usize)?;
                        self.stack.push(Value::Bool(found));
                        self.set_pc(pc + 1);
                    }
                    Instr::IterKeys => {
                        let v = self.pop();
                        let walked = match &v {
                            Value::Dict(pairs) => {
                                Value::List(pairs.keys().map(|k| k.value().clone()).collect())
                            }
                            Value::Set(items) => {
                                Value::List(items.iter().map(|k| k.value().clone()).collect())
                            }
                            _ => v,
                        };
                        self.stack.push(walked);
                        self.set_pc(pc + 1);
                    }
                    Instr::Set(count) => {
                        let mut items: IndexSet<Key> = IndexSet::with_capacity(count);
                        let mut values = Vec::with_capacity(count);
                        for _ in 0..count {
                            values.push(self.pop());
                        }
                        values.reverse();
                        for value in values {
                            items.insert(Key::new(value));
                        }
                        self.stack.push(Value::Set(Rc::new(items)));
                        self.set_pc(pc + 1);
                    }
                    Instr::NewObj(index, argc, line) => {
                        let mut args = Vec::with_capacity(argc as usize);
                        for _ in 0..argc {
                            args.push(self.pop());
                        }
                        args.reverse();
                        let params = self.prog.classes[index].params.clone();
                        if args.len() != params.len() {
                            return Err(format!(
                            "E034 လိုင်း {} တွင် \"{}\" ပုံသေ၏ တည်ဆောက်သူသည် အချက် {} ခု လိုအပ်ပါသည်၊ {} ခု ပေးထားပါသည်။",
                            line,
                            self.prog.classes[index].name,
                            params.len(),
                            args.len()
                        ));
                        }
                        let mut locals = HashMap::new();
                        for (name, value) in params.iter().zip(args) {
                            locals.insert(name.clone(), value);
                        }
                        self.set_pc(pc + 1);
                        self.frames.push(Frame {
                            func: 0,
                            pc: 0,
                            locals,
                            class: Some(index),
                            self_fields: Vec::new(),
                            value_err: None,
                            global: false,
                        });
                    }
                    Instr::SelfSet(field, line) => {
                        let value = self.pop();
                        let Some(frame) = self.frames.last_mut() else {
                            return Err(format!(
                                "E035 လိုင်း {} တွင် \"အတု\" ကို ပုံသေ တည်ဆောက်သူ အတွင်းမှာသာ သုံးနိုင်ပါသည်။",
                                line
                            ));
                        };
                        if frame.class.is_none() {
                            return Err(format!(
                                "E035 လိုင်း {} တွင် \"အတု\" ကို ပုံသေ တည်ဆောက်သူ အတွင်းမှာသာ သုံးနိုင်ပါသည်။",
                                line
                            ));
                        }
                        match frame.self_fields.iter_mut().find(|(k, _)| *k == field) {
                            Some(slot) => slot.1 = value,
                            None => frame.self_fields.push((field, value)),
                        }
                        self.set_pc(pc + 1);
                    }
                    Instr::Field(field, line) => {
                        let target = self.pop();
                        let value = crate::interp::field_of(&target, &field, line as usize)?;
                        self.stack.push(value);
                        self.set_pc(pc + 1);
                    }
                    Instr::FieldRef(name, field, line) => {
                        // A variable wins over a library name, as in the
                        // interpreter: `<var>.<field>` is a field read.
                        let bound = match self.frames.last() {
                            Some(frame) => frame.locals.get(&name).cloned(),
                            None => None,
                        }
                        .or_else(|| self.globals.get(&name).cloned());
                        let value = match bound {
                            Some(target) => {
                                crate::interp::field_of(&target, &field, line as usize)?
                            }
                            None => {
                                return Err(format!(
                                    "E085 လိုင်း {} တွင် \"{}\" ဆိုသော နည်းပညာများကို ရှာမတွေ့ပါ။",
                                    line, name
                                ))
                            }
                        };
                        self.stack.push(value);
                        self.set_pc(pc + 1);
                    }
                    Instr::IndexStore(name, line, count) => {
                        let value = self.pop();
                        let mut keys = Vec::with_capacity(count as usize);
                        for _ in 0..count {
                            keys.push(self.pop());
                        }
                        keys.reverse();
                        let mut base = self.pop();
                        set_index_vm(&mut base, &keys, value, line as usize)?;
                        self.store(&name, base);
                        self.set_pc(pc + 1);
                    }
                    Instr::Call(index, argc) => {
                        let mut args = Vec::with_capacity(argc as usize);
                        for _ in 0..argc {
                            args.push(self.pop());
                        }
                        args.reverse();
                        // The caller resumes at the instruction after the call;
                        // `Return` finds it there when the frame pops.
                        self.enter_call(index, args, None, 0, pc + 1)?;
                    }
                    Instr::CallValue(index, argc, line, err) => {
                        let mut args = Vec::with_capacity(argc as usize);
                        for _ in 0..argc {
                            args.push(self.pop());
                        }
                        args.reverse();
                        // The caller wants a value: a body that ends without one
                        // is the interpreter's `E071`.
                        self.enter_call(index, args, Some(err), line, pc + 1)?;
                    }
                    Instr::IterItems(kind, line) => {
                        let collection = self.pop();
                        let items = Vm::iter_items(kind, &collection, line as usize)?;
                        self.stack.push(Value::List(items));
                        self.set_pc(pc + 1);
                    }
                    Instr::HigherOrder(kind, line) => {
                        // Stack: items, the callback value, and for `reduce` its
                        // starting value -- the order the interpreter evaluates
                        // them in.
                        let initial = if kind == 2 { Some(self.pop()) } else { None };
                        let callback = self.pop();
                        let items = match self.pop() {
                            Value::List(items) => items,
                            other => vec![other],
                        };
                        let callable = match callback {
                            Value::Callable(kind) => kind,
                            other => {
                                return Err(crate::interp::eng_builtin_err(
                                    line as usize,
                                    higher_order_name(kind),
                                    &format!(
                                        "ပြန်ခေါ် အလုပ်အဖြစ် အလုပ် တစ်ခု လိုအပ်ပါသည်၊ {} ရပါသည်",
                                        crate::interp::type_name_mm(&other)
                                    ),
                                ))
                            }
                        };
                        // The caller resumes at the next instruction; a callback
                        // that leaves through a handler must find it that way.
                        self.set_pc(pc + 1);
                        let mut out: Vec<Value> = Vec::with_capacity(items.len());
                        let mut acc = initial;
                        for item in items {
                            let args = match kind {
                                2 => vec![
                                    acc.take().expect("reduce keeps an accumulator"),
                                    item.clone(),
                                ],
                                _ => vec![item.clone()],
                            };
                            let value = match self.call_callback(&callable, args, line)? {
                                CallbackOutcome::Value(value) => value,
                                CallbackOutcome::Left => return Ok(()),
                            };
                            match kind {
                            0 => out.push(value),
                            1 => match value {
                                Value::Bool(true) => out.push(item),
                                Value::Bool(false) => {}
                                other => {
                                    return Err(crate::interp::eng_builtin_err(
                                        line as usize,
                                        higher_order_name(kind),
                                        &format!(
                                            "True သို့မဟုတ် False ပြန်ပေးသော ပြန်ခေါ် အလုပ် လိုအပ်ပါသည်၊ {} ရပါသည်",
                                            crate::interp::type_name_mm(&other)
                                        ),
                                    ))
                                }
                            },
                            _ => acc = Some(value),
                        }
                        }
                        match kind {
                            2 => self.stack.push(acc.expect("reduce has its result")),
                            _ => self.stack.push(Value::List(out)),
                        }
                    }
                    Instr::ListArg(name, line) => {
                        if let Some(other) = self.stack.last() {
                            if !matches!(other, Value::List(_)) {
                                let got = crate::interp::type_name_mm(other);
                                return Err(crate::interp::eng_builtin_err(
                                    line as usize,
                                    &name,
                                    &format!("စာရင်း လိုအပ်ပါသည်၊ {} ရပါသည်", got),
                                ));
                            }
                        }
                        self.set_pc(pc + 1);
                    }
                    Instr::Mutate(name, var, argc, line) => {
                        let mut extra = Vec::with_capacity(argc.saturating_sub(1) as usize);
                        for _ in 0..argc.saturating_sub(1) {
                            extra.push(self.pop());
                        }
                        extra.reverse();
                        let current = self.pop();
                        let mut items = match current {
                            Value::List(items) => items,
                            other => {
                                return Err(crate::interp::eng_builtin_err(
                                    line as usize,
                                    &name,
                                    &format!(
                                        "စာရင်း လိုအပ်ပါသည်၊ {} ရပါသည်",
                                        crate::interp::type_name_mm(&other)
                                    ),
                                ))
                            }
                        };
                        let result = mutate_list(&name, &mut items, extra, line as usize)?;
                        self.store(&var, Value::List(items));
                        self.stack.push(result);
                        self.set_pc(pc + 1);
                    }
                    Instr::List(count) => {
                        let mut items = Vec::with_capacity(count.min(1024));
                        for _ in 0..count {
                            items.push(self.pop());
                        }
                        items.reverse();
                        self.stack.push(Value::List(items));
                        self.set_pc(pc + 1);
                    }
                    Instr::Tuple(count) => {
                        let mut items = Vec::with_capacity(count.min(1024));
                        for _ in 0..count {
                            items.push(self.pop());
                        }
                        items.reverse();
                        self.stack.push(Value::Tuple(items));
                        self.set_pc(pc + 1);
                    }
                    Instr::ParamMissing(name, target) => {
                        let bound = self
                            .frames
                            .last()
                            .map(|f| f.locals.contains_key(&name))
                            .unwrap_or(false);
                        self.set_pc(if bound { target } else { pc + 1 });
                    }
                    Instr::Return(line) | Instr::ReturnValue(line) => {
                        let value = self.stack.pop().unwrap_or(Value::Null);
                        self.leave_frame(value, line, true)?;
                    }
                    Instr::ReturnVoid(line) => {
                        let value = self.stack.pop().unwrap_or(Value::Null);
                        self.leave_frame(value, line, false)?;
                    }
                    Instr::Halt => self.halted = true,
                }
                Ok(())
            })();
            if self.halted {
                return Ok(());
            }
            if let Err(message) = outcome {
                // A function or class body draws its error against its own
                // file before a handler can catch it, exactly as the
                // interpreter's `invoke_function` does. Top-level code is
                // drawn only when no handler takes the error, matching the
                // interpreter's `run_with_source` boundary. The renderer
                // draws a message once, so an error passing through both
                // boundaries is not redrawn.
                if self.frames.is_empty() {
                    if !self.unwind(&message) {
                        return Err(crate::diagnostics::render::with_snippet(
                            &message,
                            &self.prog.source,
                        ));
                    }
                } else {
                    let message =
                        crate::diagnostics::render::with_snippet(&message, self.current_source());
                    if !self.unwind(&message) {
                        return Err(message);
                    }
                }
            }
        }
    }
}
