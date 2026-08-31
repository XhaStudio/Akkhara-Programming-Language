use crate::parser::{CondAtom, CondChain, Expr, ForSource, LogicalOp, Stmt};
use std::collections::HashMap;
use std::io::{self, Write};

#[derive(Debug, Clone)]
pub enum Value {
    Str(String),
    Int(i64),
    Float(f64),
    Bool(bool),
    List(Vec<Value>),
    Tuple(Vec<Value>),
    Set(Vec<Value>),
    Dict(Vec<(Value, Value)>),
    Object(String, Vec<(String, Value)>),
}

const TYPE_INT: &str = "ကိန်းပြည့်";
const TYPE_FLOAT: &str = "ဒဿမကိန်း";
const TYPE_STR: &str = "စာသား";

fn type_name_mm(v: &Value) -> &'static str {
    match v {
        Value::Str(_) => "စာသား",
        Value::Int(_) => "ကိန်းပြည့်",
        Value::Float(_) => "ဒဿမကိန်း",
        Value::Bool(_) => "မှန်/မှား",
        Value::List(_) => "စာရင်း",
        Value::Tuple(_) => "အစု",
        Value::Set(_) => "အုပ်စု",
        Value::Dict(_) => "အဘိဓာန်",
        Value::Object(_, _) => "class object",
    }
}

/// Structural equality used for set de-duplication and dict key matching.
/// Compares by display representation, which is adequate for the primitive
/// element types Akkhara collections are expected to hold.
fn value_eq(a: &Value, b: &Value) -> bool {
    repr(a) == repr(b)
}

fn op_name_mm(op: char) -> &'static str {
    match op {
        '+' => "ပေါင်း",
        '-' => "နှတ်",
        '*' => "မြှောက်",
        '/' => "စား",
        '%' => "ကြွင်းကိန်းရှာ",
        _ => "?",
    }
}

pub fn display(v: &Value) -> String {
    match v {
        Value::Str(s) => s.clone(),
        Value::Int(i) => i.to_string(),
        Value::Float(f) => {
            if f.fract() == 0.0 {
                format!("{:.1}", f)
            } else {
                let s = format!("{}", f);
                s
            }
        }
        Value::Bool(b) => {
            if *b {
                "True".to_string()
            } else {
                "False".to_string()
            }
        }
        Value::List(_) | Value::Tuple(_) | Value::Set(_) | Value::Dict(_) | Value::Object(_, _) => {
            repr(v)
        }
    }
}

/// Element-level representation used inside collections (quotes strings),
/// and as the top-level rendering for collection values themselves.
fn repr(v: &Value) -> String {
    match v {
        Value::Str(s) => format!("\"{}\"", s),
        Value::Int(_) | Value::Float(_) | Value::Bool(_) => display(v),
        Value::List(items) => {
            format!(
                "[{}]",
                items.iter().map(repr).collect::<Vec<_>>().join(", ")
            )
        }
        Value::Tuple(items) => {
            format!(
                "({})",
                items.iter().map(repr).collect::<Vec<_>>().join(", ")
            )
        }
        Value::Set(items) => {
            format!(
                "{{{}}}",
                items.iter().map(repr).collect::<Vec<_>>().join(", ")
            )
        }
        Value::Dict(pairs) => {
            format!(
                "{{{}}}",
                pairs
                    .iter()
                    .map(|(k, v)| format!("{}: {}", repr(k), repr(v)))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        }
        Value::Object(class_name, fields) => {
            format!(
                "{} {{{}}}",
                class_name,
                fields
                    .iter()
                    .map(|(k, v)| format!("{}: {}", k, repr(v)))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        }
    }
}

fn quoted_display(v: &Value) -> String {
    repr(v)
}

pub struct Interpreter {
    env: HashMap<String, Value>,
    functions: HashMap<String, (Vec<String>, Vec<Stmt>)>,
    classes: HashMap<String, Vec<(String, Vec<String>, Vec<Stmt>)>>,
    /// Stack of in-progress object constructions. While non-empty, a
    /// "တန်ဖိုး <field> သည် <value> ဖြစ်၏။" statement writes into the field
    /// list on top of this stack instead of the global environment.
    self_stack: Vec<Vec<(String, Value)>>,
}

impl Interpreter {
    pub fn new() -> Self {
        Interpreter {
            env: HashMap::new(),
            functions: HashMap::new(),
            classes: HashMap::new(),
            self_stack: Vec::new(),
        }
    }

    pub fn run(&mut self, stmts: &[Stmt]) -> Result<(), String> {
        for stmt in stmts {
            self.exec(stmt)?;
        }
        Ok(())
    }

    fn exec(&mut self, stmt: &Stmt) -> Result<(), String> {
        match stmt {
            Stmt::VarDecl { name, value, line } => {
                let v = self.eval(value, *line, Some(name))?;
                self.env.insert(name.clone(), v);
                Ok(())
            }
            Stmt::Print { value, line } => {
                let v = self.eval(value, *line, None)?;
                println!("{}", display(&v));
                Ok(())
            }
            Stmt::InputNoAssign { value, line } => {
                let prompt = self.eval(value, *line, None)?;
                print!("{}", display(&prompt));
                io::stdout().flush().ok();
                let mut buf = String::new();
                io::stdin().read_line(&mut buf).ok();
                Ok(())
            }
            Stmt::InputAssign { name, value, line } => {
                let prompt = self.eval(value, *line, None)?;
                print!("{}", display(&prompt));
                io::stdout().flush().ok();
                let mut buf = String::new();
                io::stdin().read_line(&mut buf).ok();
                let trimmed = buf.trim().to_string();
                let inferred = infer_value(&trimmed);
                self.env.insert(name.clone(), inferred);
                Ok(())
            }
            Stmt::ConvertStmt {
                value,
                target_type,
                line,
            } => {
                let v = self.eval(value, *line, None)?;
                convert_value(&v, target_type, *line)?;
                Ok(())
            }
            Stmt::ExprStmt { value, line } => {
                self.eval(value, *line, None)?;
                Ok(())
            }
            Stmt::MathAssign {
                target,
                op,
                amount,
                line,
            } => {
                let amt = self.eval(amount, *line, None)?;
                match target {
                    Expr::Ident(name) => {
                        // Undeclared variables default to 0 in math-assignment context.
                        let current = self.env.get(name).cloned().unwrap_or(Value::Int(0));
                        let result = binary_op(&current, &amt, *op, *line, Some(name))?;
                        self.env.insert(name.clone(), result);
                        Ok(())
                    }
                    other => {
                        let current = self.eval(other, *line, None)?;
                        binary_op(&current, &amt, *op, *line, None)?;
                        Ok(())
                    }
                }
            }
            Stmt::ForLoop {
                var_name,
                source,
                body,
                line,
            } => self.exec_for_loop(var_name, source, body, *line),
            Stmt::If { branches, .. } => {
                for branch in branches {
                    let take = match &branch.cond {
                        None => true, // the final, unconditional "else" branch
                        Some(chain) => {
                            let c = self.eval_cond_chain(chain)?;
                            if branch.negate {
                                !c
                            } else {
                                c
                            }
                        }
                    };
                    if take {
                        for s in &branch.body {
                            self.exec(s)?;
                        }
                        break;
                    }
                }
                Ok(())
            }
            Stmt::While {
                cond,
                negate,
                body,
                line,
            } => {
                const MAX_ITERS: u64 = 5_000_000;
                let mut iterations: u64 = 0;
                loop {
                    let c = self.eval_cond_chain(cond)?;
                    let should_run = if *negate { !c } else { c };
                    if !should_run {
                        break;
                    }
                    iterations += 1;
                    if iterations > MAX_ITERS {
                        return Err(format!(
                            "E046 လိုင်း {} ၏ while loop သည် ကြိမ်ရေ အလွန်များနေပါသည် (loop ထဲက variable ကို update မလုပ်ထားလို့ အဆုံးမရှိ ပတ်နေခြင်း ဖြစ်နိုင်ပါသည်)။",
                            line
                        ));
                    }
                    for s in body {
                        self.exec(s)?;
                    }
                }
                Ok(())
            }
            Stmt::FuncDef {
                name,
                params,
                body,
                ..
            } => {
                self.functions
                    .insert(name.clone(), (params.clone(), body.clone()));
                Ok(())
            }
            Stmt::FuncCall { name, arg, line } => {
                let (params, body) = match self.functions.get(name) {
                    Some(v) => v.clone(),
                    None => {
                        return Err(format!(
                            "E031 လိုင်း {} တွင် \"{}\" ဆိုသော function ကို ရှာမတွေ့ပါ။",
                            line, name
                        ));
                    }
                };
                let provided: Vec<Expr> = match arg {
                    Some(e) => vec![e.clone()],
                    None => vec![],
                };
                if provided.len() != params.len() {
                    return Err(format!(
                        "E033 လိုင်း {} တွင် \"{}\" function သည် argument {} ခု လိုအပ်ပါသည်၊ {} ခု ပေးထားပါသည်။",
                        line,
                        name,
                        params.len(),
                        provided.len()
                    ));
                }
                for (p, e) in params.iter().zip(provided.iter()) {
                    let v = self.eval(e, *line, None)?;
                    self.env.insert(p.clone(), v);
                }
                for s in &body {
                    self.exec(s)?;
                }
                Ok(())
            }
            Stmt::ClassDef { name, body, line } => {
                let mut methods: Vec<(String, Vec<String>, Vec<Stmt>)> = Vec::new();
                for s in body {
                    match s {
                        Stmt::FuncDef {
                            name: mname,
                            params,
                            body: mbody,
                            ..
                        } => {
                            methods.push((mname.clone(), params.clone(), mbody.clone()));
                        }
                        _ => {
                            return Err(format!(
                                "E027 လိုင်း {} တွင် \"{}\" class အတွင်း method (function) များသာ ပါဝင်နိုင်ပါသည်။",
                                line, name
                            ));
                        }
                    }
                }
                if methods.is_empty() {
                    return Err(format!(
                        "E028 လိုင်း {} တွင် \"{}\" class သည် constructor method အနည်းဆုံး တစ်ခု လိုအပ်ပါသည်။",
                        line, name
                    ));
                }
                self.classes.insert(name.clone(), methods);
                Ok(())
            }
            Stmt::SelfFieldSet { field, value, line } => {
                let v = self.eval(value, *line, None)?;
                match self.self_stack.last_mut() {
                    Some(fields) => {
                        if let Some(slot) = fields.iter_mut().find(|(k, _)| k == field) {
                            slot.1 = v;
                        } else {
                            fields.push((field.clone(), v));
                        }
                        Ok(())
                    }
                    None => Err(format!(
                        "E035 လိုင်း {} တွင် \"တန်ဖိုး\" ကို class constructor အတွင်းမှာသာ သုံးနိုင်ပါသည်။",
                        line
                    )),
                }
            }
            Stmt::TryCatch {
                try_body,
                catch_err,
                catch_var,
                catch_body,
                finally_body,
                line: _,
            } => {
                let try_result = self.run(try_body);
                match try_result {
                    Ok(()) => {
                        if let Some(fb) = finally_body {
                            self.run(fb)?;
                        }
                        Ok(())
                    }
                    Err(err_msg) => {
                        let err_code = extract_error_code(&err_msg);
                        let matched = match (catch_err, catch_body) {
                            (Some(name), Some(cb)) => {
                                // "E" matches any error; otherwise must match exactly.
                                if name == "E" || *name == err_code {
                                    if let Some(cv) = catch_var {
                                        self.env.insert(cv.clone(), Value::Str(err_code));
                                    }
                                    self.run(cb)
                                } else {
                                    Err(err_msg.clone())
                                }
                            }
                            _ => Err(err_msg.clone()),
                        };
                        if let Some(fb) = finally_body {
                            self.run(fb)?;
                        }
                        matched
                    }
                }
            }
        }
    }

    /// Substitute "{VarName}" placeholders inside a string literal with the
    /// current value of that variable, e.g. "Hello!, {Name}".
const SELF_PREFIX: &'static str = "တန်ဖိုး ";

    fn interpolate(&self, s: &str, line: usize) -> Result<String, String> {
        if !s.contains('{') {
            return Ok(s.to_string());
        }
        let mut out = String::new();
        let mut chars = s.chars().peekable();
        while let Some(c) = chars.next() {
            if c == '{' {
                let mut name = String::new();
                let mut closed = false;
                for nc in chars.by_ref() {
                    if nc == '}' {
                        closed = true;
                        break;
                    }
                    name.push(nc);
                }
                if !closed {
                    return Err(format!(
                        "E056 လိုင်း {} တွင် string ထဲက \"{{\" ကိုပိတ်ရန် \"}}\" မရှိပါ။",
                        line
                    ));
                }
                let trimmed = name.trim();
                if trimmed.is_empty() {
                    return Err(format!(
                        "E057 လိုင်း {} တွင် string interpolation \"{{}}\" ထဲမှာ variable အမည် လိုအပ်ပါသည်။",
                        line
                    ));
                }
                // "{တန်ဖိုး <field>}" reads the given field off the
                // object currently under construction (inside a class
                // method), rather than a plain global variable.
                if let Some(field_name) = trimmed.strip_prefix(Self::SELF_PREFIX) {
                    let field_name = field_name.trim();
                    if field_name.is_empty() {
                        return Err(format!(
                            "E059 လိုင်း {} တွင် string interpolation \"{{{}}}\" ထဲမှာ field အမည် လိုအပ်ပါသည်။",
                            line, trimmed
                        ));
                    }
                    let fields = self.self_stack.last().ok_or_else(|| {
                        format!(
                            "E060 လိုင်း {} တွင် \"{{{}}}\" ကို class method အတွင်းမှာသာ သုံးနိုင်ပါသည်။",
                            line, trimmed
                        )
                    })?;
                    let val = fields
                        .iter()
                        .find(|(k, _)| k == field_name)
                        .map(|(_, v)| v)
                        .ok_or_else(|| {
                            format!(
                                "E059 လိုင်း {} တွင် string ထဲက \"{{{}}}\" ၌ \"{}\" ဆိုသော field ကို ရှာမတွေ့ပါ။",
                                line, trimmed, field_name
                            )
                        })?;
                    out.push_str(&display(val));
                    continue;
                }
                let val = self.env.get(trimmed).ok_or_else(|| {
                    format!(
                        "E058 လိုင်း {} တွင် string ထဲက \"{{{}}}\" ၌ \"{}\" ဆိုသော variable ကို ရှာမတွေ့ပါ။",
                        line, trimmed, trimmed
                    )
                })?;
                out.push_str(&display(val));
            } else {
                out.push(c);
            }
        }
        Ok(out)
    }

/// Type-check for the "<var/val> သည် <type>" condition form.
/// "ကိန်း" (num) and "ဒဿမကိန်း" (float) also accept numeric-looking strings,
/// per spec: `"10" သည် ကိန်း` must be true.
fn check_type(v: &Value, type_name: &str) -> bool {
    match type_name {
        "စာသား" => matches!(v, Value::Str(_)),
        "ကိန်း" => match v {
            Value::Int(_) | Value::Float(_) => true,
            Value::Str(s) => {
                let t = s.trim();
                t.parse::<i64>().is_ok() || t.parse::<f64>().is_ok()
            }
            _ => false,
        },
        "ဒဿမကိန်း" => match v {
            Value::Float(_) => true,
            Value::Str(s) => {
                let t = s.trim();
                t.contains('.') && t.parse::<f64>().is_ok()
            }
            _ => false,
        },
        "မှန်/မှား" => matches!(v, Value::Bool(_)),
        _ => false,
    }
}

    fn eval_cond_atom(&mut self, atom: &CondAtom) -> Result<bool, String> {
        let lv = self.eval(&atom.lhs, atom.line, None)?;
        if let Some(type_name) = &atom.type_check {
            return Ok(Self::check_type(&lv, type_name));
        }
        match (&atom.op, &atom.rhs) {
            (Some(op), Some(rhs_expr)) => {
                let rv = self.eval(rhs_expr, atom.line, None)?;
                compare_values(&lv, &rv, op, atom.line)
            }
            _ => match lv {
                Value::Bool(b) => Ok(b),
                other => Err(format!(
                    "E043 လိုင်း {} တွင် {} တန်ဖိုးကို condition အဖြစ် (မှန်/မှား စစ်ရန်) သုံး၍မရပါ။",
                    atom.line,
                    type_name_mm(&other)
                )),
            },
        }
    }

    fn eval_cond_chain(&mut self, chain: &CondChain) -> Result<bool, String> {
        let mut result = self.eval_cond_atom(&chain.first)?;
        for (op, atom) in &chain.rest {
            // Every atom is always evaluated (no short-circuiting), so a
            // type error anywhere in the condition chain always surfaces
            // rather than silently being skipped.
            let v = self.eval_cond_atom(atom)?;
            result = match op {
                LogicalOp::And => result && v,
                LogicalOp::Or => result || v,
            };
        }
        Ok(result)
    }

    fn exec_for_loop(
        &mut self,
        var_name: &str,
        source: &ForSource,
        body: &[Stmt],
        line: usize,
    ) -> Result<(), String> {
        match source {
            ForSource::Range {
                start,
                end,
                step,
                op,
            } => {
                let start_v = match start {
                    Some(e) => self.eval(e, line, None)?,
                    None => Value::Int(0),
                };
                let end_v = self.eval(end, line, None)?;
                let step_v = self.eval(step, line, None)?;

                let mut current = start_v;
                loop {
                    let cur_f = as_f64(&current).ok_or_else(|| loop_not_numeric_err(line))?;
                    let end_f = as_f64(&end_v).ok_or_else(|| loop_not_numeric_err(line))?;
                    let keep_going = match op {
                        '+' | '*' => cur_f < end_f,
                        '-' | '/' => cur_f > end_f,
                        _ => false,
                    };
                    if !keep_going {
                        break;
                    }
                    self.env.insert(var_name.to_string(), current.clone());
                    for s in body {
                        self.exec(s)?;
                    }
                    current = binary_op(&current, &step_v, *op, line, Some(var_name))?;
                }
                Ok(())
            }
            ForSource::Auto(src) => {
                let src_v = self.eval(src, line, None)?;
                match src_v {
                    Value::Int(_) | Value::Float(_) => {
                        // Bare numeric source with no step clause: auto-range
                        // from 0, incrementing by 1, matching the source value
                        // as an exclusive upper bound (so "10" loops 10 times).
                        let end_f = as_f64(&src_v).ok_or_else(|| loop_not_numeric_err(line))?;
                        let mut current = Value::Int(0);
                        loop {
                            let cur_f =
                                as_f64(&current).ok_or_else(|| loop_not_numeric_err(line))?;
                            if cur_f >= end_f {
                                break;
                            }
                            self.env.insert(var_name.to_string(), current.clone());
                            for s in body {
                                self.exec(s)?;
                            }
                            current =
                                binary_op(&current, &Value::Int(1), '+', line, Some(var_name))?;
                        }
                        Ok(())
                    }
                    Value::List(items) | Value::Tuple(items) | Value::Set(items) => {
                        for item in items {
                            self.env.insert(var_name.to_string(), item);
                            for s in body {
                                self.exec(s)?;
                            }
                        }
                        Ok(())
                    }
                    Value::Dict(pairs) => {
                        for (k, _) in pairs {
                            self.env.insert(var_name.to_string(), k);
                            for s in body {
                                self.exec(s)?;
                            }
                        }
                        Ok(())
                    }
                    other => Err(loop_not_iterable_err(line, type_name_mm(&other))),
                }
            }
        }
    }

    fn eval(&mut self, expr: &Expr, line: usize, var_ctx: Option<&str>) -> Result<Value, String> {
        match expr {
            Expr::NumLit(s) => {
                if s.contains('.') {
                    s.parse::<f64>()
                        .map(Value::Float)
                        .map_err(|_| format!("E047 လိုင်း {} တွင် ကိန်းဂဏန်း တန်ဖိုးမှားနေသည်။", line))
                } else {
                    s.parse::<i64>()
                        .map(Value::Int)
                        .map_err(|_| format!("E047 လိုင်း {} တွင် ကိန်းဂဏန်း တန်ဖိုးမှားနေသည်။", line))
                }
            }
            Expr::StrLit(s) => Ok(Value::Str(self.interpolate(s, line)?)),
            Expr::BoolLit(b) => Ok(Value::Bool(*b)),
            Expr::Ident(name) => self.env.get(name).cloned().ok_or_else(|| {
                format!(
                    "E030 လိုင်း {} တွင် \"{}\" ဆိုသော variable ကို ရှာမတွေ့ပါ။",
                    line, name
                )
            }),
            Expr::Binary(l, op, r, op_line) => {
                let lv = self.eval(l, line, var_ctx)?;
                let rv = self.eval(r, line, var_ctx)?;
                binary_op(&lv, &rv, *op, *op_line, var_ctx)
            }
            Expr::Neg(inner, neg_line) => {
                let iv = self.eval(inner, line, var_ctx)?;
                match iv {
                    Value::Int(i) => Ok(Value::Int(-i)),
                    Value::Float(f) => Ok(Value::Float(-f)),
                    other => Err(format!(
                        "E041 လိုင်း {} တွင် {} ကို အနုတ် (-) လုပ်၍မရပါ။",
                        neg_line,
                        type_name_mm(&other)
                    )),
                }
            }
            Expr::Convert(inner, target_type, cline) => {
                let iv = self.eval(inner, line, var_ctx)?;
                convert_value(&iv, target_type, *cline)
            }
            Expr::ListLit(items) => {
                let mut vals = Vec::with_capacity(items.len());
                for it in items {
                    vals.push(self.eval(it, line, var_ctx)?);
                }
                Ok(Value::List(vals))
            }
            Expr::TupleLit(items) => {
                let mut vals = Vec::with_capacity(items.len());
                for it in items {
                    vals.push(self.eval(it, line, var_ctx)?);
                }
                Ok(Value::Tuple(vals))
            }
            Expr::SetLit(items) => {
                let mut vals: Vec<Value> = Vec::with_capacity(items.len());
                for it in items {
                    let v = self.eval(it, line, var_ctx)?;
                    if !vals.iter().any(|existing| value_eq(existing, &v)) {
                        vals.push(v);
                    }
                }
                Ok(Value::Set(vals))
            }
            Expr::DictLit(pairs) => {
                let mut out: Vec<(Value, Value)> = Vec::with_capacity(pairs.len());
                for (k, v) in pairs {
                    let kv = self.eval(k, line, var_ctx)?;
                    let vv = self.eval(v, line, var_ctx)?;
                    if let Some(slot) = out.iter_mut().find(|(ek, _)| value_eq(ek, &kv)) {
                        slot.1 = vv;
                    } else {
                        out.push((kv, vv));
                    }
                }
                Ok(Value::Dict(out))
            }
            Expr::Index(base, indices) => {
                let base_v = self.eval(base, line, var_ctx)?;
                let mut keys = Vec::with_capacity(indices.len());
                for idx_e in indices {
                    keys.push(self.eval(idx_e, line, var_ctx)?);
                }
                index_value(&base_v, &keys, line)
            }
            Expr::NewObj(class_name, arg_exprs, new_line) => {
                self.construct_object(class_name, arg_exprs, *new_line)
            }
        }
    }

    /// Instantiate an object: evaluate the constructor arguments, bind them
    /// to the class's (first-defined) method's parameters, run that method's
    /// body with a fresh field accumulator active, and return the resulting
    /// Value::Object.
    fn construct_object(
        &mut self,
        class_name: &str,
        arg_exprs: &[Expr],
        line: usize,
    ) -> Result<Value, String> {
        let methods = match self.classes.get(class_name) {
            Some(m) => m.clone(),
            None => {
                return Err(format!(
                    "E032 လိုင်း {} တွင် \"{}\" ဆိုသော class ကို ရှာမတွေ့ပါ။",
                    line, class_name
                ));
            }
        };
        // The first method defined in the class acts as its constructor.
        let (_, params, body) = &methods[0];

        if arg_exprs.len() != params.len() {
            return Err(format!(
                "E034 လိုင်း {} တွင် \"{}\" class ၏ constructor သည် argument {} ခု လိုအပ်ပါသည်၊ {} ခု ပေးထားပါသည်။",
                line,
                class_name,
                params.len(),
                arg_exprs.len()
            ));
        }

        let mut arg_values = Vec::with_capacity(arg_exprs.len());
        for e in arg_exprs {
            arg_values.push(self.eval(e, line, None)?);
        }
        for (p, v) in params.iter().zip(arg_values.into_iter()) {
            self.env.insert(p.clone(), v);
        }

        self.self_stack.push(Vec::new());
        let body = body.clone();
        let run_result = (|| -> Result<(), String> {
            for s in &body {
                self.exec(s)?;
            }
            Ok(())
        })();
        let fields = self.self_stack.pop().unwrap_or_default();
        run_result?;

        Ok(Value::Object(class_name.to_string(), fields))
    }
}

/// Error-code helpers for try/catch. All runtime errors are formatted as
/// "E### <message>". `<error_name> ကို ဖမ်းပါ` binds the code (e.g. "E030")
/// into a string variable so scripts can match on it.
fn extract_error_code(msg: &str) -> String {
    let code = msg.split_whitespace().next().unwrap_or("E");
    code.to_string()
}

fn index_as_int(v: &Value, line: usize) -> Result<i64, String> {
    match v {
        Value::Int(i) => Ok(*i),
        Value::Float(f) if f.fract() == 0.0 => Ok(*f as i64),
        _ => Err(format!(
            "E048 လိုင်း {} တွင် index သည် ကိန်းပြည့် ဖြစ်ရပါမည်။",
            line
        )),
    }
}

fn get_seq_item(items: &[Value], idx: i64, line: usize) -> Result<Value, String> {
    if idx < 0 {
        return Err(format!(
            "E049 လိုင်း {} တွင် index {} သည် အကွာအဝေးပြင်ပတွင် ရှိနေပါသည်။",
            line, idx
        ));
    }
    items.get(idx as usize).cloned().ok_or_else(|| {
        format!(
            "E049 လိုင်း {} တွင် index {} သည် အကွာအဝေးပြင်ပတွင် ရှိနေပါသည်။",
            line, idx
        )
    })
}

fn index_seq(items: &[Value], keys: &[Value], line: usize) -> Result<Value, String> {
    match keys {
        [i] => {
            let idx = index_as_int(i, line)?;
            get_seq_item(items, idx, line)
        }
        [row_k, col_k] => {
            let row = index_as_int(row_k, line)?;
            let col = index_as_int(col_k, line)?;
            match get_seq_item(items, row, line)? {
                Value::List(cols) | Value::Tuple(cols) => get_seq_item(&cols, col, line),
                other => Err(format!(
                    "E054 လိုင်း {} တွင် {} ကို [row, column] ဖြင့် index ယူ၍မရပါ။",
                    line,
                    type_name_mm(&other)
                )),
            }
        }
        _ => Err(format!(
            "E050 လိုင်း {} တွင် index အရေအတွက်မှားနေပါသည်။",
            line
        )),
    }
}

fn index_value(base: &Value, keys: &[Value], line: usize) -> Result<Value, String> {
    match base {
        Value::List(items) | Value::Tuple(items) | Value::Set(items) => {
            index_seq(items, keys, line)
        }
        Value::Dict(pairs) => {
            if keys.len() != 1 {
                return Err(format!(
                    "E052 လိုင်း {} တွင် အဘိဓာန် (dict) ကို key တစ်ခုဖြင့်သာ index ယူရပါမည်။",
                    line
                ));
            }
            let key = &keys[0];
            pairs
                .iter()
                .find(|(k, _)| value_eq(k, key))
                .map(|(_, v)| v.clone())
                .ok_or_else(|| {
                    format!(
                        "E053 လိုင်း {} တွင် key {} ကို ရှာမတွေ့ပါ။",
                        line,
                        repr(key)
                    )
                })
        }
        other => Err(format!(
            "E051 လိုင်း {} တွင် {} ကို index ယူ၍မရပါ။",
            line,
            type_name_mm(other)
        )),
    }
}

fn as_f64(v: &Value) -> Option<f64> {
    match v {
        Value::Int(i) => Some(*i as f64),
        Value::Float(f) => Some(*f),
        _ => None,
    }
}

fn loop_not_numeric_err(line: usize) -> String {
    format!(
        "E044 လိုင်း {} တွင် for loop ၏ အစ/အဆုံး/step တန်ဖိုးများသည် ကိန်းဂဏန်း ဖြစ်ရပါမည်။",
        line
    )
}

fn loop_not_iterable_err(line: usize, type_name: &str) -> String {
    format!(
        "E045 လိုင်း {} တွင် {} ကို for loop ဖြင့် ထပ်ခါထပ်ခါ လည်ပတ်၍မရပါ။",
        line, type_name
    )
}

fn cmp_type_err(line: usize, t1: &str, t2: &str, op: &str) -> String {
    format!(
        "E042 လိုင်း {} တွင် {} နှင့် {} ကို \"{}\" ဖြင့် နှိုင်းယှဉ်၍မရပါ။",
        line, t1, t2, op
    )
}

/// Evaluate one comparison ("<", ">", "==", "!=", "<=", ">=") between two
/// already-evaluated values. Equality/inequality works for any pair of
/// values (structural comparison); ordering comparisons require both sides
/// to be numeric, or both sides to be strings (lexicographic order).
fn compare_values(lhs: &Value, rhs: &Value, op: &str, line: usize) -> Result<bool, String> {
    match op {
        "==" => Ok(value_eq(lhs, rhs)),
        "!=" => Ok(!value_eq(lhs, rhs)),
        "<" | ">" | "<=" | ">=" => {
            if let (Some(a), Some(b)) = (as_f64(lhs), as_f64(rhs)) {
                return Ok(match op {
                    "<" => a < b,
                    ">" => a > b,
                    "<=" => a <= b,
                    ">=" => a >= b,
                    _ => unreachable!(),
                });
            }
            if let (Value::Str(a), Value::Str(b)) = (lhs, rhs) {
                return Ok(match op {
                    "<" => a < b,
                    ">" => a > b,
                    "<=" => a <= b,
                    ">=" => a >= b,
                    _ => unreachable!(),
                });
            }
            Err(cmp_type_err(line, type_name_mm(lhs), type_name_mm(rhs), op))
        }
        _ => unreachable!(),
    }
}

fn binary_op(
    lv: &Value,
    rv: &Value,
    op: char,
    line: usize,
    var_ctx: Option<&str>,
) -> Result<Value, String> {
    let type_err = || -> String {
        let (t1, t2) = (type_name_mm(lv), type_name_mm(rv));
        let opn = op_name_mm(op);
        match var_ctx {
            Some(name) => format!(
                "E040 လိုင်း {} ၏ \"{}\"၌ {} နှင့် {} ကို {}၍မရပါ။",
                line, name, t1, t2, opn
            ),
            None => format!("E040 လိုင်း {} တွင် {} နှင့် {} ကို {}၍မရပါ။", line, t1, t2, opn),
        }
    };

    match (lv, rv) {
        (Value::Str(a), Value::Str(b)) => {
            if op == '+' {
                Ok(Value::Str(format!("{}{}", a, b)))
            } else {
                Err(type_err())
            }
        }
        (Value::Int(a), Value::Int(b)) => match op {
            '+' => Ok(Value::Int(a + b)),
            '-' => Ok(Value::Int(a - b)),
            '*' => Ok(Value::Int(a * b)),
            '/' => {
                if *b == 0 {
                    Err(format!("E036 လိုင်း {} တွင် သုညဖြင့် စား၍မရပါ။", line))
                } else {
                    Ok(Value::Float(*a as f64 / *b as f64))
                }
            }
            '%' => {
                if *b == 0 {
                    Err(format!(
                        "E038 လိုင်း {} တွင် သုညဖြင့် ကြွင်းကိန်းရှာ၍မရပါ။",
                        line
                    ))
                } else {
                    Ok(Value::Int(a.rem_euclid(*b)))
                }
            }
            _ => Err(type_err()),
        },
        (Value::Int(a), Value::Float(b)) => numeric_op(*a as f64, *b, op, line),
        (Value::Float(a), Value::Int(b)) => numeric_op(*a, *b as f64, op, line),
        (Value::Float(a), Value::Float(b)) => numeric_op(*a, *b, op, line),
        _ => Err(type_err()),
    }
}

fn numeric_op(a: f64, b: f64, op: char, line: usize) -> Result<Value, String> {
    match op {
        '+' => Ok(Value::Float(a + b)),
        '-' => Ok(Value::Float(a - b)),
        '*' => Ok(Value::Float(a * b)),
        '/' => {
            if b == 0.0 {
                Err(format!("E037 လိုင်း {} တွင် သုညဖြင့် စား၍မရပါ။", line))
            } else {
                Ok(Value::Float(a / b))
            }
        }
        '%' => {
            if b == 0.0 {
                Err(format!(
                    "E039 လိုင်း {} တွင် သုညဖြင့် ကြွင်းကိန်းရှာ၍မရပါ။",
                    line
                ))
            } else {
                Ok(Value::Float(a.rem_euclid(b)))
            }
        }
        _ => unreachable!(),
    }
}

fn infer_value(s: &str) -> Value {
    if s == "True" || s == "မှန်" {
        return Value::Bool(true);
    }
    if s == "False" || s == "မှား" {
        return Value::Bool(false);
    }
    if let Ok(i) = s.parse::<i64>() {
        return Value::Int(i);
    }
    if let Ok(f) = s.parse::<f64>() {
        return Value::Float(f);
    }
    Value::Str(s.to_string())
}

fn convert_value(v: &Value, target_type: &str, line: usize) -> Result<Value, String> {
    let fail = || -> String {
        format!(
            "E055 လိုင်း {} တွင် {} ကို {}သို့ ပြောင်းလဲ၍ မရပါ။",
            line,
            quoted_display(v),
            target_type
        )
    };

    match target_type {
        TYPE_STR => Ok(Value::Str(display(v))),
        TYPE_INT => match v {
            Value::Int(i) => Ok(Value::Int(*i)),
            Value::Float(f) => Ok(Value::Int(*f as i64)),
            Value::Bool(b) => Ok(Value::Int(if *b { 1 } else { 0 })),
            Value::Str(s) => s.trim().parse::<i64>().map(Value::Int).map_err(|_| fail()),
            _ => Err(fail()),
        },
        TYPE_FLOAT => match v {
            Value::Int(i) => Ok(Value::Float(*i as f64)),
            Value::Float(f) => Ok(Value::Float(*f)),
            Value::Bool(b) => Ok(Value::Float(if *b { 1.0 } else { 0.0 })),
            Value::Str(s) => s
                .trim()
                .parse::<f64>()
                .map(Value::Float)
                .map_err(|_| fail()),
            _ => Err(fail()),
        },
        _ => {
            // bool-ish target
            match v {
                Value::Bool(b) => Ok(Value::Bool(*b)),
                Value::Str(s) => match s.as_str() {
                    "True" | "မှန်" => Ok(Value::Bool(true)),
                    "False" | "မှား" => Ok(Value::Bool(false)),
                    _ => Err(fail()),
                },
                Value::Int(i) => Ok(Value::Bool(*i != 0)),
                Value::Float(f) => Ok(Value::Bool(*f != 0.0)),
                _ => Err(fail()),
            }
        }
    }
}
