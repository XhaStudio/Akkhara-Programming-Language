use crate::library::LibraryLoader;
use crate::parser::{CondAtom, CondChain, Expr, ForSource, LogicalOp, Stmt, WaitUnit};
use std::collections::HashMap;
use std::io::{self, BufWriter, Write};

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
    /// A library function bound as a value (a first-class library
    /// function) -- e.g. the `Tools.add` in `TOOL = Tools.add;`. Calling
    /// the binding (`TOOL(1, 100)`) forwards to the library function.
    Callable(Callable),
}

/// What a `Value::Callable` refers to. Kept as a name pair rather than a
/// closure so values stay `Clone` and comparable.
#[derive(Debug, Clone)]
pub enum Callable {
    /// A function of an imported library: `<lib> ၏ <fn>` / `<lib>.<fn>`.
    Library { lib: String, fn_name: String },
}

impl Callable {
    fn label(&self) -> String {
        match self {
            Callable::Library { lib, fn_name } => format!("{}.{}", lib, fn_name),
        }
    }
}

/// A class registered by `ပုံသေ <name>[(<params>)] သည် ... ပြီး။`.
#[derive(Debug, Clone)]
struct ClassInfo {
    /// Constructor parameters from the class header's `(<params>)` list.
    /// `None` when the header has no list, in which case the first method's
    /// own parameter list supplies the constructor's (the original form).
    ctor_params: Option<Vec<String>>,
    /// The class's methods in source order as `(name, parameters, body)`.
    /// The first one acts as the constructor.
    methods: Vec<(String, Vec<String>, Vec<Stmt>)>,
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
        Value::Callable(_) => "function",
    }
}

/// Structural equality used for set de-duplication and dict key matching.
/// Compares by display representation, which is adequate for the primitive
/// element types Akkhara collections are expected to hold.
///
/// The three primitive `same type` cases are answered directly: building the
/// two display strings just to compare them allocated twice per call, which
/// is the bulk of the cost of `a == b` and of every set/dict membership test.
/// Each fast path is exactly equivalent to comparing those reprs:
/// `display` for Int/Bool is `to_string()`, and repr quotes strings at both
/// ends, so equal reprs mean equal contents. Everything else -- including
/// mixed types such as Int(1) vs Float(1.0), which must stay unequal -- still
/// goes through `repr`, so no other result changes.
fn value_eq(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => x == y,
        (Value::Str(x), Value::Str(y)) => x == y,
        (Value::Bool(x), Value::Bool(y)) => x == y,
        _ => repr(a) == repr(b),
    }
}

fn op_name_mm(op: char) -> &'static str {
    match op {
        '+' => "ပေါင်း",
        '-' => "နှတ်",
        '*' => "မြှောက်",
        '/' => "စား",
        '%' => "ကြွင်းကိန်းရှာ",
        '^' => "ထပ်ကိန်းတင်",
        '\\' => "အပြည့်ကိန်းစား",
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
        Value::Callable(kind) => kind.label(),
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
        Value::Callable(kind) => kind.label(),
    }
}

fn quoted_display(v: &Value) -> String {
    repr(v)
}

// --- "request" library response object ---
//
// A GET/POST call through the "request" library returns a plain
// Value::Object tagged "request-response" with three fields, so it can be
// read with the generic "<expr> ၏ <field>" syntax:
//     အခြေအနေကုဒ်  -- HTTP status code (integer)
//     အကြောင်းအရာ  -- response body text
//     အောင်မြင်မှု   -- true when the status code is below 400
const LIB_REQUEST_NAME: &str = "request";
const RESP_CLASS: &str = "request-response";
const RESP_STATUS: &str = "အခြေအနေကုဒ်";
const RESP_BODY: &str = "အကြောင်းအရာ";
const RESP_OK: &str = "အောင်မြင်မှု";

fn http_response_to_value(r: crate::request_library::HttpResponse) -> Value {
    Value::Object(
        RESP_CLASS.to_string(),
        vec![
            (RESP_STATUS.to_string(), Value::Int(r.status_code)),
            (RESP_BODY.to_string(), Value::Str(r.body)),
            (RESP_OK.to_string(), Value::Bool(r.ok)),
        ],
    )
}

// --- "App" GUI library handles ---
//
// Every `App` window/widget function returns a plain Value::Object tagged
// "App-window" or "App-widget", so a handle behaves like any other value:
// it can be stored in a variable, passed back into `App.*`, and read with
// the generic `<expr> ၏ <field>` syntax (`w ၏ id`). Only the numeric id
// crosses into the GUI library, which owns the real window and widget
// objects.
const LIB_APP_NAME: &str = "App";
const APP_WINDOW_CLASS: &str = "App-window";
const APP_WIDGET_CLASS: &str = "App-widget";

fn app_window_handle(id: u64) -> Value {
    Value::Object(
        APP_WINDOW_CLASS.to_string(),
        vec![
            ("id".to_string(), Value::Int(id as i64)),
            ("class".to_string(), Value::Str("window".to_string())),
        ],
    )
}

fn app_widget_handle(id: u64, kind: &str) -> Value {
    Value::Object(
        APP_WIDGET_CLASS.to_string(),
        vec![
            ("id".to_string(), Value::Int(id as i64)),
            ("class".to_string(), Value::Str("widget".to_string())),
            ("kind".to_string(), Value::Str(kind.to_string())),
        ],
    )
}

/// Pulls the id out of an App handle, with `E120` when the value is not one
/// (`App.get(42)`, `App.move("x", 0, 0)`, ...).
fn app_handle_id(v: &Value, fn_name: &str, line: usize) -> Result<u64, String> {
    if let Value::Object(class, fields) = v {
        if class == APP_WINDOW_CLASS || class == APP_WIDGET_CLASS {
            if let Some(Value::Int(id)) = fields.iter().find(|(k, _)| k == "id").map(|(_, v)| v) {
                if *id > 0 {
                    return Ok(*id as u64);
                }
            }
        }
    }
    Err(format!(
        "E120 လိုင်း {} တွင် \"App.{}\" သည် App handle တစ်ခု လိုအပ်ပါသည်၊ {} ရရှိပါသည်။",
        line,
        fn_name,
        type_name_mm(v)
    ))
}

/// `App.<fn>`'s argument-count check. A few widget functions accept an
/// optional trailing argument (`button`'s callback, `color`'s background),
/// so a range is allowed as well as an exact count.
fn app_argc(fn_name: &str, got: usize, min: usize, max: usize, line: usize) -> Result<(), String> {
    if (min..=max).contains(&got) {
        return Ok(());
    }
    if min == max {
        Err(format!(
            "E087 လိုင်း {} တွင် \"App\" ၏ \"{}\" function သည် argument {} ခု လိုအပ်ပါသည်၊ {} ခု ပေးထားပါသည်။",
            line, fn_name, min, got
        ))
    } else {
        Err(format!(
            "E087 လိုင်း {} တွင် \"App\" ၏ \"{}\" function သည် argument {} မှ {} ခု လိုအပ်ပါသည်၊ {} ခု ပေးထားပါသည်။",
            line, fn_name, min, max, got
        ))
    }
}

fn app_want_text_err(fn_name: &str, line: usize, want: &str) -> String {
    format!(
        "E126 လိုင်း {} တွင် \"App.{}\" ၏ argument သည် {} ဖြစ်ရပါသည်။",
        line, fn_name, want
    )
}

/// A coordinate / size / duration argument: any number.
fn app_num(v: &Value, fn_name: &str, line: usize) -> Result<f32, String> {
    as_f64(v).map(|f| f as f32).ok_or_else(|| {
        format!(
            "E090 လိုင်း {} တွင် \"App.{}\" ၏ argument များသည် ကိန်းဂဏန်း ဖြစ်ရပါသည်။",
            line, fn_name
        )
    })
}

/// A callback-name argument: must be text (`"on_save"`), not a bare name.
fn app_func_name(v: &Value, fn_name: &str, line: usize) -> Result<String, String> {
    match v {
        Value::Str(s) if !s.trim().is_empty() => Ok(s.clone()),
        _ => Err(app_want_text_err(
            fn_name,
            line,
            "callback function နာမည် စာသား (ဥပမာ \"on_save\")",
        )),
    }
}

/// A `choice` / `listbox` item list: a list/tuple/set of values (each shown
/// as text), or a single string used as the only item.
fn app_items(v: &Value, fn_name: &str, line: usize) -> Result<Vec<String>, String> {
    match v {
        Value::List(items) | Value::Tuple(items) | Value::Set(items) => {
            Ok(items.iter().map(display).collect())
        }
        Value::Str(s) => Ok(vec![s.clone()]),
        _ => Err(app_want_text_err(
            fn_name,
            line,
            "ရွေးစရာ item များ ပါသော list တစ်ခု",
        )),
    }
}

/// A `table` row list: a list whose elements are rows. Each row is either a
/// list of cells or a single scalar (a one-cell row).
fn app_rows(v: &Value, fn_name: &str, line: usize) -> Result<Vec<Vec<String>>, String> {
    match v {
        Value::List(items) | Value::Tuple(items) | Value::Set(items) => Ok(items
            .iter()
            .map(|row| match row {
                Value::List(cells) | Value::Tuple(cells) | Value::Set(cells) => {
                    cells.iter().map(display).collect()
                }
                other => vec![display(other)],
            })
            .collect()),
        Value::Str(s) => Ok(vec![vec![s.clone()]]),
        _ => Err(app_want_text_err(
            fn_name,
            line,
            "အတန်းများ ပါသော list တစ်ခု (အတန်းတစ်ခုစီသည် cell စာသားများ၏ list)",
        )),
    }
}

/// The value of `App.set_items`: a list of items, or a list of rows (each a
/// list of cells). A single string becomes a one-item list.
fn app_items_value(
    v: &Value,
    fn_name: &str,
    line: usize,
) -> Result<crate::app_library::AppValue, String> {
    match v {
        Value::List(items) | Value::Tuple(items) | Value::Set(items) => {
            if items
                .iter()
                .any(|e| matches!(e, Value::List(_) | Value::Tuple(_) | Value::Set(_)))
            {
                Ok(crate::app_library::AppValue::Rows(app_rows(
                    v, fn_name, line,
                )?))
            } else {
                Ok(crate::app_library::AppValue::Items(
                    items.iter().map(display).collect(),
                ))
            }
        }
        Value::Str(s) => Ok(crate::app_library::AppValue::Items(vec![s.clone()])),
        _ => Err(app_want_text_err(
            fn_name,
            line,
            "item များ (သို့) အတန်းများ ပါသော list တစ်ခု",
        )),
    }
}

/// A row/column index for `App.cell` / `App.set_cell`: a non-negative whole
/// number.
fn app_index(v: &Value, fn_name: &str, line: usize) -> Result<usize, String> {
    let index = match v {
        Value::Int(i) => Some(*i),
        Value::Float(f) if f.fract() == 0.0 => Some(*f as i64),
        _ => None,
    };
    index.filter(|i| *i >= 0).map(|i| i as usize).ok_or_else(|| {
        format!(
            "E090 လိုင်း {} တွင် \"App.{}\" ၏ row/column သည် 0 နှင့်အထက် ကိန်းပြည့် ဖြစ်ရပါသည်။",
            line, fn_name
        )
    })
}

/// A value handed to `App.set`: numbers and booleans keep their type (a
/// checkbox needs a boolean), everything else becomes text.
fn app_value_from(v: &Value) -> crate::app_library::AppValue {
    match v {
        Value::Int(i) => crate::app_library::AppValue::Int(*i),
        Value::Float(f) => crate::app_library::AppValue::Float(*f),
        Value::Bool(b) => crate::app_library::AppValue::Bool(*b),
        other => crate::app_library::AppValue::Text(display(other)),
    }
}

fn app_value_to(v: crate::app_library::AppValue) -> Value {
    match v {
        crate::app_library::AppValue::Text(s) => Value::Str(s),
        crate::app_library::AppValue::Int(i) => Value::Int(i),
        crate::app_library::AppValue::Float(f) => Value::Float(f),
        crate::app_library::AppValue::Bool(b) => Value::Bool(b),
        crate::app_library::AppValue::Items(items) => {
            Value::List(items.into_iter().map(Value::Str).collect())
        }
        crate::app_library::AppValue::Rows(rows) => Value::List(
            rows.into_iter()
                .map(|r| Value::List(r.into_iter().map(Value::Str).collect()))
                .collect(),
        ),
    }
}

/// Appends the source line to a library error, which reports only its own
/// `E1xx` code (the same treatment the `request` library gets).
fn app_err(e: String, line: usize) -> String {
    format!("{} (လိုင်း {})", e, line)
}

/// Text -> number for `App.number`, in the language's own number model:
/// an integer-looking text becomes `ကိန်းပြည့်`, anything else with a
/// decimal point / exponent becomes `ဒဿမကိန်း`. Myanmar digits count (a box
/// holding `၅` is five), and an empty box counts as 0. `None` means the
/// text isn't a number at all (`E128`).
fn app_parse_number(text: &str) -> Option<Value> {
    let ascii: String = text
        .trim()
        .chars()
        .map(|c| match c {
            '၀' => '0',
            '၁' => '1',
            '၂' => '2',
            '၃' => '3',
            '၄' => '4',
            '၅' => '5',
            '၆' => '6',
            '၇' => '7',
            '၈' => '8',
            '၉' => '9',
            other => other,
        })
        .collect();

    if ascii.is_empty() {
        return Some(Value::Int(0));
    }
    if let Ok(i) = ascii.parse::<i64>() {
        return Some(Value::Int(i));
    }
    if let Ok(f) = ascii.parse::<f64>() {
        return Some(Value::Float(f));
    }
    None
}

// --- Errors for the `<lib> ၏ <fn>(...) ကို လုပ်ပါ။` library-call syntax. ---

fn lib_unknown_err(lib: &str, line: usize) -> String {
    format!(
        "E085 လိုင်း {} တွင် \"{}\" ဆိုသော နည်းပညာများ (library) ကို ရှာမတွေ့ပါ။",
        line, lib
    )
}

fn lib_fn_unknown_err(lib: &str, fn_name: &str, line: usize) -> String {
    format!(
        "E086 လိုင်း {} တွင် \"{}\" နည်းပညာများတွင် \"{}\" ဆိုသော function ကို ရှာမတွေ့ပါ။",
        line, lib, fn_name
    )
}

fn lib_fn_argc(lib: &str, fn_name: &str, args: &[Expr], want: usize, line: usize) -> Result<(), String> {
    if args.len() == want {
        Ok(())
    } else {
        Err(format!(
            "E087 လိုင်း {} တွင် \"{}\" ၏ \"{}\" function သည် argument {} ခု လိုအပ်ပါသည်၊ {} ခု ပေးထားပါသည်။",
            line,
            lib,
            fn_name,
            want,
            args.len()
        ))
    }
}

fn lib_fn_argtype_err(lib: &str, fn_name: &str, line: usize) -> String {
    format!(
        "E090 လိုင်း {} တွင် \"{}\" ၏ \"{}\" function သည် ကိန်းဂဏန်း argument များ လိုအပ်ပါသည်။",
        line, lib, fn_name
    )
}

/// "this function wants a collection, not that scalar" -- raised when a
/// value-taking library function (e.g. `ကျပန်း ၏ တန်ဖိုး`) is handed
/// something like an int.
fn lib_fn_collection_err(lib: &str, fn_name: &str, line: usize) -> String {
    format!(
        "E092 လိုင်း {} တွင် \"{}\" ၏ \"{}\" function သည် စာရင်း (list), အစု (tuple), အုပ်စု (set) သို့ စာသား (str) argument တစ်ခု လိုအပ်ပါသည်။",
        line, lib, fn_name
    )
}

// --- variable-table hasher ---------------------------------------------------
//
// `env` is hashed on every variable read and every variable write, and its
// keys are very short identifiers -- exactly the case where the default
// SipHash-1-3 costs more than the lookup it is protecting. This is the public
// FxHash mixing step (rotate-xor-multiply per byte), the usual choice for
// compiler/interpreter symbol tables. It is not collision-resistant against
// adversarial keys, which is fine here: variable names come from the program's
// own source. `env` is never iterated, so the changed bucket order is not
// observable; `functions`, `classes` and the library tables keep the default
// hasher because they are looked up once, not per statement.
const FX_SEED: u64 = 0x51_7c_c1_b7_27_22_0a_95;

#[derive(Default)]
struct FxHasher {
    hash: u64,
}

impl std::hash::Hasher for FxHasher {
    #[inline]
    fn finish(&self) -> u64 {
        self.hash
    }

    #[inline]
    fn write(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.write_u8(byte);
        }
    }

    #[inline]
    fn write_u8(&mut self, byte: u8) {
        self.hash = (self.hash.rotate_left(5) ^ u64::from(byte)).wrapping_mul(FX_SEED);
    }
}

/// The variable table: `FxHasher` instead of the default SipHash.
type VarMap = HashMap<String, Value, std::hash::BuildHasherDefault<FxHasher>>;

pub struct Interpreter {
    env: VarMap,
    /// Names declared with the eng library's `pin` form (`pin name :bool =
    /// true;`). Assigning to a name here -- through either syntax family --
    /// is an error; `pin` values are immutable for the rest of the program.
    consts: std::collections::HashSet<String>,
    functions: HashMap<String, (Vec<String>, Vec<Stmt>)>,
    classes: HashMap<String, ClassInfo>,
    /// Stack of in-progress object constructions. While non-empty, a
    /// "တန်ဖိုး <field> သည် <value> ဖြစ်၏။" statement writes into the field
    /// list on top of this stack instead of the global environment.
    self_stack: Vec<Vec<(String, Value)>>,
    /// Loader that resolves and activates libraries imported with
    /// `နည်းပညာများ <name> ကို အသုံးပြုပါ။`. Only loaded libraries may use
    /// their builtins (e.g. စောင့်ပါ).
    libraries: LibraryLoader,
    /// Aliases registered by `<lib> အဖြစ် <alias>။` (and by the
    /// `နည်းပညာများ <lib> အဖြစ် <alias> ကို အသုံးပြုပါ။` import form):
    /// alias -> canonical library name. Every `<lib> ၏ ...` reference is
    /// resolved through this before dispatch.
    lib_aliases: HashMap<String, String>,
    /// Function names each imported script/package library defined, keyed
    /// by canonical library name. Set when the library's source is run, so
    /// `<lib> ၏ <fn>` / `<lib>.<fn>` only reaches the functions that came
    /// from that library -- a plain function name registered elsewhere
    /// isn't reachable through an unrelated library.
    lib_functions: HashMap<String, Vec<String>>,
    /// Pending `return` from inside a function body: `Some(None)` is a bare
    /// `return;`, `Some(Some(v))` returns `v`, and `None` means no return is
    /// in flight. Set by `Stmt::EngReturn`, consumed by `call_function`.
    return_signal: Option<Option<Value>>,
    /// How many function bodies are currently executing. A `return` while
    /// this is 0 escaped to the top level (E108).
    call_depth: usize,
    /// Pending `break` from inside a loop body, with the same shape as
    /// `return_signal`: `Some(None)` is a bare `break`, `Some(Some(v))` is
    /// `break <value>`. Consumed by the innermost enclosing loop.
    break_signal: Option<Option<Value>>,
    /// How many loops are currently running (eng `loop`, `while`, `for`). A
    /// `break` while this is 0 has no loop to stop (E116).
    loop_depth: usize,
    /// When `Some`, everything the program prints is collected here as well
    /// as shown on stdout, so `akk` can save the run's output next to the
    /// program as a `<file_name>.akop` file. `None` disables the capture.
    output_log: Option<String>,
    /// Buffered writer for program output. `print!` goes through the global
    /// `io::Stdout` line writer, which flushes to the OS on every newline, so
    /// a program printing in a loop paid one write per line. Holding the
    /// lock and buffering keeps the same bytes in the same order (the buffer
    /// is flushed before anything is read, and once the run ends) while
    /// turning those writes into a handful of large ones.
    out: BufWriter<io::StdoutLock<'static>>,
}

impl Interpreter {
    /// `libraries_dir` is where `akk install <name>` places downloaded
    /// packages (normally the `libraries/` folder next to the akk binary).
    /// It's used to resolve `နည်းပညာများ <name> ကို အသုံးပြုပါ။` for any
    /// name that isn't one of the built-in libraries compiled into akk.
    pub fn new(libraries_dir: std::path::PathBuf) -> Self {
        Interpreter {
            env: VarMap::default(),
            consts: std::collections::HashSet::new(),
            functions: HashMap::new(),
            classes: HashMap::new(),
            self_stack: Vec::new(),
            libraries: LibraryLoader::new(libraries_dir),
            lib_aliases: HashMap::new(),
            lib_functions: HashMap::new(),
            return_signal: None,
            call_depth: 0,
            break_signal: None,
            loop_depth: 0,
            output_log: None,
            out: BufWriter::new(io::stdout().lock()),
        }
    }

    /// Starts collecting the program's output (see `output_log`).
    pub fn capture_output(&mut self) {
        self.output_log = Some(String::new());
    }

    /// Returns the output collected so far, and stops capturing.
    pub fn take_output(&mut self) -> Option<String> {
        self.output_log.take()
    }

    /// Bind `value` to `name`, reusing an existing binding's key.
    ///
    /// `HashMap::insert` takes an owned key, so assigning to a variable that
    /// already exists -- by far the common case for `x = ...;`, `x += ...;`
    /// and the Myanmar assignment forms -- allocated a fresh copy of the
    /// name on every single assignment just to look the same slot up again.
    /// `get_mut` updates that slot in place; a name that isn't bound yet is
    /// inserted exactly as before.
    fn store(&mut self, name: &str, value: Value) {
        if let Some(slot) = self.env.get_mut(name) {
            *slot = value;
        } else {
            self.env.insert(name.to_string(), value);
        }
    }

    /// Whether `name` is pinned (`pin` / a constant): assigning to one is an
    /// error. `consts` is empty for programs that never use `pin`, and an
    /// empty `HashSet` still hashes the key on every `contains`, so the
    /// empty check short-circuits the hash on every assignment.
    fn is_pinned(&self, name: &str) -> bool {
        !self.consts.is_empty() && self.consts.contains(name)
    }

    /// Program output: shown on stdout, and appended to the capture buffer
    /// when one is active. A write error panics with the same message `print!`
    /// produces (that is what std's own print macro does), so a closed stdout
    /// still fails loudly.
    fn emit(&mut self, text: &str) {
        if let Err(e) = self.out.write_all(text.as_bytes()) {
            panic!("failed printing to stdout: {}", e);
        }
        if let Some(log) = self.output_log.as_mut() {
            log.push_str(text);
        }
    }

    fn emit_line(&mut self, text: &str) {
        self.emit(&format!("{}\n", text));
    }

    /// Prompts are written without a trailing newline, so flush the output
    /// buffer explicitly before the program blocks on input.
    fn flush_stdout(&mut self) {
        let _ = self.out.flush();
    }

    /// Flushes everything the program has printed but that is still buffered.
    /// `akk` calls this once the run is over -- before printing the closing
    /// `Interpreted in ...` line or an error, so the terminal shows the
    /// program's output ahead of them exactly as it did when each line was
    /// written out as it was produced.
    pub fn flush_output(&mut self) {
        let _ = self.out.flush();
    }

    /// Adds a folder to search for plain `<name>.akk` libraries, on top of
    /// the program's own folder. `akk` calls this with the directory of the
    /// file being run, so `နည်းပညာများ Tools ကို အသုံးပြုပါ။` can pull in a
    /// neighboring `Tools.akk`.
    pub fn add_library_search_dir(&mut self, dir: std::path::PathBuf) {
        self.libraries.add_script_dir(dir);
    }

    pub fn run(&mut self, stmts: &[Stmt]) -> Result<(), String> {
        for stmt in stmts {
            // A `return` is unwinding out of the enclosing function (and a
            // `break` is heading for the innermost loop): stop running
            // statements here -- the try/catch and library bodies use this
            // same entry point.
            if self.return_signal.is_some() || self.break_signal.is_some() {
                break;
            }
            self.exec(stmt)?;
        }
        Ok(())
    }

    /// Run one loop body. Returns `Some(v)` when a `break` ended the
    /// iteration -- `v` being the break's value (`None` for a bare `break`) --
    /// and `None` when the body ran to completion. A pending `return` is left
    /// in place so the enclosing function call can unwind.
    fn run_loop_body(&mut self, body: &[Stmt]) -> Result<Option<Option<Value>>, String> {
        for s in body {
            self.exec(s)?;
            if self.return_signal.is_some() || self.break_signal.is_some() {
                break;
            }
        }
        Ok(self.break_signal.take())
    }

    fn exec(&mut self, stmt: &Stmt) -> Result<(), String> {
        // Once a `return` or `break` is in flight nothing else runs until the
        // enclosing function/loop consumes it -- this keeps `return` inside
        // if/while/for and try bodies from executing the statements that
        // follow it.
        if self.return_signal.is_some() || self.break_signal.is_some() {
            return Ok(());
        }
        match stmt {
            Stmt::VarDecl { name, value, line } => {
                if self.is_pinned(name) {
                    return Err(eng_const_reassign_err(name, *line));
                }
                let v = self.eval(value, *line, Some(name))?;
                self.store(name, v);
                Ok(())
            }
            Stmt::EngDecl {
                name,
                type_name,
                value,
                is_const,
                line,
            } => {
                let v = self.eval(value, *line, Some(name))?;
                let v = check_eng_value(name, type_name, v, *line)?;
                if *is_const {
                    if self.env.contains_key(name) {
                        return Err(eng_pin_redeclare_err(name, *line));
                    }
                    self.consts.insert(name.clone());
                }
                self.env.insert(name.clone(), v);
                Ok(())
            }
            Stmt::EngPrint { value, line } => {
                let v = self.eval(value, *line, None)?;
                self.emit_line(&display(&v));
                Ok(())
            }
            Stmt::EngInput { prompt, line } => {
                let p = self.eval(prompt, *line, None)?;
                self.emit(&display(&p));
                self.flush_stdout();
                let mut buf = String::new();
                io::stdin().read_line(&mut buf).ok();
                Ok(())
            }
            Stmt::EngInputAssign {
                name,
                type_name,
                prompt,
                line,
            } => {
                let p = self.eval(prompt, *line, None)?;
                self.emit(&display(&p));
                self.flush_stdout();
                let mut buf = String::new();
                io::stdin().read_line(&mut buf).ok();
                let trimmed = buf.trim().to_string();
                let v = coerce_eng_input(&trimmed, type_name, *line)?;
                self.env.insert(name.clone(), v);
                Ok(())
            }
            Stmt::EngReturn { value, line } => {
                if self.call_depth == 0 {
                    return Err(eng_return_outside_fn_err(*line));
                }
                let v = match value {
                    Some(e) => Some(self.eval(e, *line, None)?),
                    None => None,
                };
                self.return_signal = Some(v);
                Ok(())
            }
            Stmt::EngExprCall { name, args, line } => {
                self.call_named(name, args, *line)?;
                Ok(())
            }
            Stmt::EngAssign { name, value, line } => {
                if self.is_pinned(name) {
                    return Err(eng_const_reassign_err(name, *line));
                }
                // A first `name = <value>;` declares the variable, so an eng
                // assignment needs no separate typed declaration.
                let v = self.eval(value, *line, Some(name))?;
                self.store(name, v);
                Ok(())
            }
            Stmt::EngMathAssign {
                name,
                op,
                amount,
                line,
            } => {
                if self.is_pinned(name) {
                    return Err(eng_const_reassign_err(name, *line));
                }
                // An undeclared name starts from 0, the way the Myanmar
                // `... ကို <n> တိုးပါ။` form does. Reading `current` before
                // the amount keeps the old `x += <expr>` order: <expr> sees
                // the value `x` had before this statement.
                let current = match self.env.get(name) {
                    Some(v) => v.clone(),
                    None => Value::Int(0),
                };
                let amt = self.eval(amount, *line, Some(name))?;
                let result = binary_op(&current, &amt, *op, *line, Some(name))?;
                self.store(name, result);
                Ok(())
            }
            Stmt::EngBreak { value, line } => {
                if self.loop_depth == 0 {
                    return Err(eng_break_outside_loop_err(*line));
                }
                let v = match value {
                    Some(e) => Some(self.eval(e, *line, None)?),
                    None => None,
                };
                self.break_signal = Some(v);
                Ok(())
            }
            Stmt::EngLoop { body } => {
                self.loop_depth += 1;
                let result = (|| -> Result<(), String> {
                    loop {
                        let broke = self.run_loop_body(body)?;
                        if broke.is_some() || self.return_signal.is_some() {
                            return Ok(());
                        }
                    }
                })();
                self.loop_depth -= 1;
                result
            }
            Stmt::EngLoopAssign {
                name,
                type_name,
                is_const,
                body,
                line,
            } => {
                // `<name> :<type> = loop { ... }` / `<name> = loop { ... }`.
                if self.consts.contains(name) {
                    return Err(eng_const_reassign_err(name, *line));
                }
                if *is_const && self.env.contains_key(name) {
                    return Err(eng_pin_redeclare_err(name, *line));
                }
                // The declared name is seeded with its type's default before
                // the loop runs, so the body can accumulate into it
                // (`x :int = loop { x += 1; ... }` starts at 0).
                if let Some(t) = type_name {
                    self.env.insert(name.clone(), eng_type_default(t));
                }
                self.loop_depth += 1;
                let outcome = (|| -> Result<Option<Option<Value>>, String> {
                    loop {
                        let broke = self.run_loop_body(body)?;
                        if broke.is_some() {
                            return Ok(broke);
                        }
                        if self.return_signal.is_some() {
                            // A `return` inside the loop abandons the
                            // declaration and unwinds to the caller.
                            return Ok(None);
                        }
                    }
                })();
                self.loop_depth -= 1;
                if self.return_signal.is_some() {
                    return Ok(());
                }
                let value = match outcome? {
                    Some(Some(v)) => v,
                    Some(None) => return Err(eng_loop_no_value_err(*line)),
                    None => return Ok(()),
                };
                let value = match type_name {
                    Some(t) => check_eng_value(name, t, value, *line)?,
                    None => value,
                };
                if *is_const {
                    self.consts.insert(name.clone());
                }
                self.env.insert(name.clone(), value);
                Ok(())
            }
            Stmt::Print { value, line } => {
                let v = self.eval(value, *line, None)?;
                self.emit_line(&display(&v));
                Ok(())
            }
            Stmt::InputNoAssign { value, line } => {
                let prompt = self.eval(value, *line, None)?;
                self.emit(&display(&prompt));
                self.flush_stdout();
                let mut buf = String::new();
                io::stdin().read_line(&mut buf).ok();
                Ok(())
            }
            Stmt::InputAssign { name, value, line } => {
                let prompt = self.eval(value, *line, None)?;
                self.emit(&display(&prompt));
                self.flush_stdout();
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
                        if self.is_pinned(name) {
                            return Err(eng_const_reassign_err(name, *line));
                        }
                        // Undeclared variables default to 0 in math-assignment context.
                        let current = self.env.get(name).cloned().unwrap_or(Value::Int(0));
                        let result = binary_op(&current, &amt, *op, *line, Some(name))?;
                        self.store(name, result);
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
                            if self.return_signal.is_some() || self.break_signal.is_some() {
                                break;
                            }
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
            } => self.exec_condition_loop(cond, *negate, body, *line, "while loop"),
            Stmt::EngFor {
                init,
                cond,
                step,
                body,
                line,
            } => {
                // `init` runs once, before the loop; the `step` becomes the
                // loop body's last statement, so `break` and `return` inside
                // the body leave the loop without running it.
                self.exec(init)?;
                let mut loop_body = body.clone();
                loop_body.push((**step).clone());
                self.exec_condition_loop(cond, false, &loop_body, *line, "for loop")
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
            Stmt::FuncCall { name, args, line } => {
                // A user function wins; otherwise a variable bound to a
                // first-class library function (`TOOL`) is callable, so the
                // Myanmar form `<name> ကို လုပ်ရန် <args> ဖြင့်။` works for
                // either.
                if !self.functions.contains_key(name) {
                    if let Some(Value::Callable(kind)) = self.env.get(name).cloned() {
                        self.invoke_callable(&kind, args, *line)?;
                        return Ok(());
                    }
                }
                self.call_function(name, args, *line)?;
                Ok(())
            }
            Stmt::FuncCallAssign {
                name,
                fn_name,
                args,
                line,
            } => {
                let result = if !self.functions.contains_key(fn_name) {
                    match self.env.get(fn_name).cloned() {
                        Some(Value::Callable(kind)) => {
                            Some(self.invoke_callable(&kind, args, *line)?)
                        }
                        _ => self.call_function(fn_name, args, *line)?,
                    }
                } else {
                    self.call_function(fn_name, args, *line)?
                };
                let value = result.ok_or_else(|| {
                    format!(
                        "E071 လိုင်း {} တွင် \"{}\" function သည် value ပြန်မပေးသဖြင့် \"{}\" ကို သိမ်းဆည်း၍မရပါ။ function ၏ နောက်ဆုံး statement သည် value တစ်ခု ဖြစ်ရပါမည်။",
                        line, fn_name, name
                    )
                })?;
                self.env.insert(name.clone(), value);
                Ok(())
            }
            Stmt::LibCall {
                lib,
                fn_name,
                args,
                line,
            } => {
                self.eval_lib_call(lib, fn_name, args, *line)?;
                Ok(())
            }
            Stmt::LibCallAssign {
                name,
                lib,
                fn_name,
                args,
                line,
            } => {
                let result = self.eval_lib_call(lib, fn_name, args, *line)?;
                let value = result.ok_or_else(|| {
                    format!(
                        "E088 လိုင်း {} တွင် \"{}\" ၏ \"{}\" function သည် value ပြန်မပေးသဖြင့် \"{}\" ကို သိမ်းဆည်း၍မရပါ။",
                        line, lib, fn_name, name
                    )
                })?;
                self.env.insert(name.clone(), value);
                Ok(())
            }
            Stmt::FuncAlias {
                original,
                aliases,
                line,
            } => {
                // `<lib> အဖြစ် <alias>။` -- aliasing a library rather than a
                // function. It also imports the library, so the alias is
                // usable on the very next line.
                if !self.functions.contains_key(original) && self.is_known_library(original) {
                    for alias in aliases {
                        self.lib_aliases.insert(alias.clone(), original.clone());
                    }
                    if !self.libraries.is_loaded(original) {
                        self.import_library(original, *line)?;
                    }
                    return Ok(());
                }
                let def = self.functions.get(original).cloned().ok_or_else(|| {
                    format!(
                        "E031 လိုင်း {} တွင် \"{}\" ဆိုသော function ကို ရှာမတွေ့ပါ။",
                        line, original
                    )
                })?;
                for alias in aliases {
                    self.functions.insert(alias.clone(), def.clone());
                }
                Ok(())
            }
            Stmt::ClassDef {
                name,
                params,
                body,
                line,
            } => {
                let mut methods: Vec<(String, Vec<String>, Vec<Stmt>)> = Vec::new();
                for s in body {
                    match s {
                        Stmt::FuncDef {
                            name: mname,
                            params: mparams,
                            body: mbody,
                            ..
                        } => {
                            methods.push((mname.clone(), mparams.clone(), mbody.clone()));
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
                // A header parameter list already names the constructor's
                // parameters, so the first (constructor) method must be
                // written without a parameter list of its own.
                if params.is_some() && !methods[0].1.is_empty() {
                    return Err(format!(
                        "E096 လိုင်း {} တွင် \"{}\" class ၏ header တွင် constructor parameter များ ရေးထားပြီးဖြစ်၍ ပထမ method သည် parameter များ ထပ်မံမရေးရပါ။",
                        line, name
                    ));
                }
                self.classes.insert(
                    name.clone(),
                    ClassInfo {
                        ctor_params: params.clone(),
                        methods,
                    },
                );
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
                        "E035 လိုင်း {} တွင် \"တန်ဖိုး\" (သို့မဟုတ် \"အတု\") ကို class constructor အတွင်းမှာသာ သုံးနိုင်ပါသည်။",
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
            Stmt::UseLibrary { libs, line } => {
                for (name, alias) in libs {
                    self.import_library(name, *line)?;
                    if let Some(alias) = alias {
                        self.lib_aliases.insert(alias.clone(), name.clone());
                    }
                }
                Ok(())
            }
            Stmt::RandomDecl { name, min, max, line } => {
                if !self.libraries.is_loaded("ကျပန်း") {
                    return Err(format!(
                        "E064 လိုင်း {} တွင် \"ကျပန်းကိန်း\" ကို သုံးရန် \"နည်းပညာများ ကျပန်း ကို အသုံးပြုပါ။\" ဖြင့် ကျပန်းနည်းပညာများကို အရင်ထည့်သွင်းရပါမည်။",
                        line
                    ));
                }
                let min_v = self.eval(min, *line, None)?;
                let max_v = self.eval(max, *line, None)?;

                let min_i = as_i64(&min_v).ok_or_else(|| {
                    format!("E065 လိုင်း {} တွင် ကျပန်းကိန်း အပိုင်းအခြား၏ အစသည် ကိန်းဂဏန်း ဖြစ်ရပါမည်။", line)
                })?;
                let max_i = as_i64(&max_v).ok_or_else(|| {
                    format!("E065 လိုင်း {} တွင် ကျပန်းကိန်း အပိုင်းအခြား၏ အဆုံးသည် ကိန်းဂဏန်း ဖြစ်ရပါမည်။", line)
                })?;

                let val = crate::random_library::random_int(min_i, max_i)?;
                self.env.insert(name.clone(), Value::Int(val));
                Ok(())
            }
            Stmt::Wait { amount, unit, line } => {
                if !self.libraries.is_loaded("အချိန်") {
                    return Err(format!(
                        "E063 လိုင်း {} တွင် \"စောင့်ပါ\" ကို သုံးရန် \"နည်းပညာများ အချိန် ကို အသုံးပြုပါ။\" ဖြင့် အချိန်နည်းပညာများကို အရင်ထည့်သွင်းရပါမည်။",
                        line
                    ));
                }
                let v = self.eval(amount, *line, None)?;
                let secs = as_f64(&v).ok_or_else(|| {
                    format!(
                        "E061 လိုင်း {} တွင် စောင့်ရန် အချိန်တန်ဖိုးသည် ကိန်းဂဏန်း ဖြစ်ရပါမည်။",
                        line
                    )
                })?;
                let unit = match unit {
                    WaitUnit::Seconds => crate::time_library::WaitUnit::Seconds,
                    WaitUnit::Minutes => crate::time_library::WaitUnit::Minutes,
                    WaitUnit::Hours => crate::time_library::WaitUnit::Hours,
                };
                crate::time_library::wait(secs, unit)
            }
            Stmt::HttpSend {
                name,
                lib,
                url,
                data,
                line,
            } => {
                let resolved = self.resolve_lib(lib);
                if resolved != "request" {
                    return Err(format!(
                        "E062 လိုင်း {} တွင် \"{}\" ဆိုသော နည်းပညာများ (library) ကို ရှာမတွေ့ပါ။",
                        line, resolved
                    ));
                }
                if !self.libraries.is_loaded("request") {
                    return Err(format!(
                        "E078 လိုင်း {} တွင် \"request\" ကို သုံးရန် \"နည်းပညာများ request ကို အသုံးပြုပါ။\" ဖြင့် request နည်းပညာများကို အရင်ထည့်သွင်းရပါမည်။",
                        line
                    ));
                }
                let url_v = self.eval(url, *line, None)?;
                let data_v = self.eval(data, *line, None)?;
                let url_s = display(&url_v);
                let data_s = display(&data_v);
                let resp = crate::request_library::post(&url_s, &data_s)
                    .map_err(|e| format!("{} (လိုင်း {})", e, line))?;
                self.env.insert(name.clone(), http_response_to_value(resp));
                Ok(())
            }
        }
    }

    /// Substitute "{VarName}" placeholders inside a string literal with the
    /// current value of that variable, e.g. "Hello!, {Name}".
/// The two accepted spellings of the self keyword inside a `{...}`
/// placeholder: "{တန်ဖိုး <field>}" and its synonym "{အတု <field>}".
const SELF_PREFIX: &'static str = "တန်ဖိုး ";
const SELF_PREFIX_ALT: &'static str = "အတု ";

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
                // "{တန်ဖိုး <field>}" (or its synonym "{အတု <field>}") reads
                // the given field off the object currently under construction
                // (inside a class method), rather than a plain global
                // variable.
                let self_field = trimmed
                    .strip_prefix(Self::SELF_PREFIX)
                    .or_else(|| trimmed.strip_prefix(Self::SELF_PREFIX_ALT));
                if let Some(field_name) = self_field {
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
        let result = if let Some(type_name) = &atom.type_check {
            Self::check_type(&lv, type_name)
        } else {
            match (&atom.op, &atom.rhs) {
                (Some(op), Some(rhs_expr)) => {
                    let rv = self.eval(rhs_expr, atom.line, None)?;
                    compare_values(&lv, &rv, op, atom.line)?
                }
                _ => match lv {
                    Value::Bool(b) => b,
                    other => {
                        return Err(format!(
                            "E043 လိုင်း {} တွင် {} တန်ဖိုးကို condition အဖြစ် (မှန်/မှား စစ်ရန်) သုံး၍မရပါ။",
                            atom.line,
                            type_name_mm(&other)
                        ))
                    }
                },
            }
        };
        // The eng library's `!` (and any negative-form condition) flips the
        // atom's truth value.
        Ok(if atom.negate { !result } else { result })
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

    /// Run a `for` loop. The loop wrapper exists so `loop_depth` (which gates
    /// `break`) is restored even when the body errors out and the error is
    /// caught by an enclosing try/catch.
    fn exec_for_loop(
        &mut self,
        var_name: &str,
        source: &ForSource,
        body: &[Stmt],
        line: usize,
    ) -> Result<(), String> {
        self.loop_depth += 1;
        let result = self.exec_for_loop_inner(var_name, source, body, line);
        self.loop_depth -= 1;
        result
    }

    fn exec_for_loop_inner(
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
                    let broke = self.run_loop_body(body)?;
                    if broke.is_some() || self.return_signal.is_some() {
                        break;
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
                            let broke = self.run_loop_body(body)?;
                            if broke.is_some() || self.return_signal.is_some() {
                                break;
                            }
                            current =
                                binary_op(&current, &Value::Int(1), '+', line, Some(var_name))?;
                        }
                        Ok(())
                    }
                    Value::List(items) | Value::Tuple(items) | Value::Set(items) => {
                        for item in items {
                            self.env.insert(var_name.to_string(), item);
                            let broke = self.run_loop_body(body)?;
                            if broke.is_some() || self.return_signal.is_some() {
                                break;
                            }
                        }
                        Ok(())
                    }
                    Value::Dict(pairs) => {
                        for (k, _) in pairs {
                            self.env.insert(var_name.to_string(), k);
                            let broke = self.run_loop_body(body)?;
                            if broke.is_some() || self.return_signal.is_some() {
                                break;
                            }
                        }
                        Ok(())
                    }
                    other => Err(loop_not_iterable_err(line, type_name_mm(&other))),
                }
            }
        }
    }

    /// Call a built-in library function reached through the
    /// `<lib> ၏ <fn>(<args>) ကို လုပ်ပါ။` family of forms
    /// (`Stmt::LibCall` / `Stmt::LibCallAssign`).
    ///
    /// The library must already have been imported with
    /// `နည်းပညာများ <lib> ကို အသုံးပြုပါ။`, and `<fn>` must be one of the
    /// native functions that library exposes:
    ///
    ///   request   get(<url>) -> response, post(<url>, <data>) -> response
    ///   ကျပန်း     ကိန်း/ကိန်ပြည့်(<min>, <max>) -> int, ဒဿမ(<min>, <max>) -> float,
    ///             တန်ဖိုး(<collection>) -> a random element of it
    ///   အချိန်     စောင့်(<seconds>) -> no value
    ///
    /// `lib` may be an alias registered with `<lib> အဖြစ် <alias>။`.
    ///
    /// Returns the function's value, or `None` for the ones (like စောင့်)
    /// that only carry out an action.
    fn eval_lib_call(
        &mut self,
        lib: &str,
        fn_name: &str,
        args: &[Expr],
        line: usize,
    ) -> Result<Option<Value>, String> {
        let resolved = self.resolve_lib(lib);
        let lib = resolved.as_str();
        match lib {
            "request" => {
                self.require_library_loaded(LIB_REQUEST_NAME, line)?;
                match fn_name {
                    "get" => {
                        lib_fn_argc(lib, fn_name, args, 1, line)?;
                        let url_v = self.eval(&args[0], line, None)?;
                        let resp = crate::request_library::get(&display(&url_v))
                            .map_err(|e| format!("{} (လိုင်း {})", e, line))?;
                        Ok(Some(http_response_to_value(resp)))
                    }
                    "post" => {
                        lib_fn_argc(lib, fn_name, args, 2, line)?;
                        let url_v = self.eval(&args[0], line, None)?;
                        let data_v = self.eval(&args[1], line, None)?;
                        let resp = crate::request_library::post(&display(&url_v), &display(&data_v))
                            .map_err(|e| format!("{} (လိုင်း {})", e, line))?;
                        Ok(Some(http_response_to_value(resp)))
                    }
                    _ => Err(lib_fn_unknown_err(lib, fn_name, line)),
                }
            }
            "ကျပန်း" => {
                self.require_library_loaded("ကျပန်း", line)?;
                match fn_name {
                    "ကိန်း" | "ကိန်ပြည့်" | "random_int" => {
                        lib_fn_argc(lib, fn_name, args, 2, line)?;
                        let min_v = self.eval(&args[0], line, None)?;
                        let max_v = self.eval(&args[1], line, None)?;
                        let min_i = as_i64(&min_v)
                            .ok_or_else(|| lib_fn_argtype_err(lib, fn_name, line))?;
                        let max_i = as_i64(&max_v)
                            .ok_or_else(|| lib_fn_argtype_err(lib, fn_name, line))?;
                        Ok(Some(Value::Int(crate::random_library::random_int(
                            min_i, max_i,
                        )?)))
                    }
                    "ဒဿမ" | "random_float" => {
                        lib_fn_argc(lib, fn_name, args, 2, line)?;
                        let min_v = self.eval(&args[0], line, None)?;
                        let max_v = self.eval(&args[1], line, None)?;
                        let min_f = as_f64(&min_v)
                            .ok_or_else(|| lib_fn_argtype_err(lib, fn_name, line))?;
                        let max_f = as_f64(&max_v)
                            .ok_or_else(|| lib_fn_argtype_err(lib, fn_name, line))?;
                        Ok(Some(Value::Float(crate::random_library::random_float(
                            min_f, max_f,
                        )?)))
                    }
                    // တန်ဖိုး(<collection>) -- pick one element at random from a
                    // list / tuple / set / string.
                    "တန်ဖိုး" | "value" => {
                        lib_fn_argc(lib, fn_name, args, 1, line)?;
                        let v = self.eval(&args[0], line, None)?;
                        let items: Vec<Value> = match &v {
                            Value::List(items) => items.clone(),
                            Value::Tuple(items) => items.clone(),
                            Value::Set(items) => items.clone(),
                            Value::Str(s) => s.chars().map(|c| Value::Str(c.to_string())).collect(),
                            _ => return Err(lib_fn_collection_err(lib, fn_name, line)),
                        };
                        let picked = crate::random_library::random_value(&items)
                            .map_err(|e| format!("{} (လိုင်း {})", e, line))?;
                        Ok(Some(picked))
                    }
                    _ => Err(lib_fn_unknown_err(lib, fn_name, line)),
                }
            }
            "အချိန်" => {
                self.require_library_loaded("အချိန်", line)?;
                match fn_name {
                    "စောင့်" | "wait" => {
                        lib_fn_argc(lib, fn_name, args, 1, line)?;
                        let amount_v = self.eval(&args[0], line, None)?;
                        let secs = as_f64(&amount_v)
                            .ok_or_else(|| lib_fn_argtype_err(lib, fn_name, line))?;
                        crate::time_library::wait(secs, crate::time_library::WaitUnit::Seconds)?;
                        Ok(None)
                    }
                    _ => Err(lib_fn_unknown_err(lib, fn_name, line)),
                }
            }
            "App" => {
                self.require_library_loaded(LIB_APP_NAME, line)?;
                self.eval_app_call(fn_name, args, line)
            }
            // Not a built-in: a script/package imported with
            // `နည်းပညာများ <name> ကို အသုံးပြုပါ။` registers its functions
            // globally, so `<lib> ၏ <fn>` / `<lib>.<fn>` reaches the one
            // that library defined. Functions defined outside that library
            // aren't reachable through it.
            _ => {
                let owns_fn = self
                    .lib_functions
                    .get(lib)
                    .map(|fns| fns.iter().any(|f| f == fn_name))
                    .unwrap_or(false);
                if owns_fn {
                    return self.call_function(fn_name, args, line);
                }
                if self.lib_functions.contains_key(lib) {
                    return Err(lib_fn_unknown_err(lib, fn_name, line));
                }
                Err(lib_unknown_err(lib, line))
            }
        }
    }

    /// Call one of the `App` GUI library's functions
    /// (`Stmt::LibCall` / `Stmt::LibCallAssign` / `Expr::LibCall`).
    ///
    /// Everything the GUI needs is a plain value here: handles are objects
    /// carrying an id, callbacks are function *names* as text, and the
    /// windows/widgets themselves live in the library's own model. `run`
    /// blocks until its window closes, and every callback the GUI sends is
    /// dispatched back into `call_function` by name.
    fn eval_app_call(
        &mut self,
        fn_name: &str,
        args: &[Expr],
        line: usize,
    ) -> Result<Option<Value>, String> {
        let mut vals: Vec<Value> = Vec::with_capacity(args.len());
        for arg in args {
            vals.push(self.eval(arg, line, None)?);
        }
        let count = vals.len();
        let num = |i: usize| app_num(&vals[i], fn_name, line);
        let handle = |i: usize| app_handle_id(&vals[i], fn_name, line);
        let text = |i: usize| display(&vals[i]);

        match fn_name {
            // ----- window -----
            "screen" => {
                app_argc(fn_name, count, 2, 2, line)?;
                Ok(Some(app_window_handle(crate::app_library::screen(
                    num(0)?,
                    num(1)?,
                ))))
            }
            "title" => {
                app_argc(fn_name, count, 2, 2, line)?;
                let win = handle(0)?;
                crate::app_library::title(win, &text(1)).map_err(|e| app_err(e, line))?;
                Ok(None)
            }
            "run" => {
                app_argc(fn_name, count, 1, 1, line)?;
                let win = handle(0)?;
                // The callback the GUI hands back is a function name; the
                // interpreter is the only side that can call it.
                let mut on_callback = |name: &str| -> Result<(), String> {
                    if !self.functions.contains_key(name) {
                        return Err(app_err(
                            crate::app_library::err_missing_callback(name),
                            line,
                        ));
                    }
                    self.call_function(name, &[], line).map(|_| ())
                };
                crate::app_library::run(win, &mut on_callback).map_err(|e| {
                    // Errors out of a callback already carry their own line.
                    if e.contains("လိုင်း ") {
                        e
                    } else {
                        app_err(e, line)
                    }
                })?;
                Ok(None)
            }
            "close" => {
                app_argc(fn_name, count, 1, 1, line)?;
                let win = handle(0)?;
                crate::app_library::close(win).map_err(|e| app_err(e, line))?;
                Ok(None)
            }

            // ----- widgets -----
            "label" => {
                app_argc(fn_name, count, 4, 4, line)?;
                let win = handle(0)?;
                let id = crate::app_library::label(win, &text(1), num(2)?, num(3)?)
                    .map_err(|e| app_err(e, line))?;
                Ok(Some(app_widget_handle(id, "label")))
            }
            "button" => {
                app_argc(fn_name, count, 4, 5, line)?;
                let win = handle(0)?;
                let callback = if count >= 5 {
                    Some(app_func_name(&vals[4], fn_name, line)?)
                } else {
                    None
                };
                let id = crate::app_library::button(
                    win,
                    &text(1),
                    num(2)?,
                    num(3)?,
                    callback.as_deref(),
                )
                .map_err(|e| app_err(e, line))?;
                Ok(Some(app_widget_handle(id, "button")))
            }
            "input" => {
                app_argc(fn_name, count, 4, 4, line)?;
                let win = handle(0)?;
                let id = crate::app_library::input(win, num(1)?, num(2)?, num(3)?)
                    .map_err(|e| app_err(e, line))?;
                Ok(Some(app_widget_handle(id, "input")))
            }
            "textarea" => {
                app_argc(fn_name, count, 5, 5, line)?;
                let win = handle(0)?;
                let id =
                    crate::app_library::textarea(win, num(1)?, num(2)?, num(3)?, num(4)?)
                        .map_err(|e| app_err(e, line))?;
                Ok(Some(app_widget_handle(id, "textarea")))
            }
            "checkbox" => {
                app_argc(fn_name, count, 4, 4, line)?;
                let win = handle(0)?;
                let id = crate::app_library::checkbox(win, &text(1), num(2)?, num(3)?)
                    .map_err(|e| app_err(e, line))?;
                Ok(Some(app_widget_handle(id, "checkbox")))
            }
            "choice" => {
                app_argc(fn_name, count, 4, 4, line)?;
                let win = handle(0)?;
                let items = app_items(&vals[1], fn_name, line)?;
                let id = crate::app_library::choice(win, items, num(2)?, num(3)?)
                    .map_err(|e| app_err(e, line))?;
                Ok(Some(app_widget_handle(id, "choice")))
            }
            "listbox" => {
                app_argc(fn_name, count, 6, 6, line)?;
                let win = handle(0)?;
                let items = app_items(&vals[1], fn_name, line)?;
                let id = crate::app_library::listbox(
                    win,
                    items,
                    num(2)?,
                    num(3)?,
                    num(4)?,
                    num(5)?,
                )
                .map_err(|e| app_err(e, line))?;
                Ok(Some(app_widget_handle(id, "listbox")))
            }
            "table" => {
                app_argc(fn_name, count, 7, 7, line)?;
                let win = handle(0)?;
                let headers = app_items(&vals[1], fn_name, line)?;
                let rows = app_rows(&vals[2], fn_name, line)?;
                let id = crate::app_library::table(
                    win,
                    headers,
                    rows,
                    num(3)?,
                    num(4)?,
                    num(5)?,
                    num(6)?,
                )
                .map_err(|e| app_err(e, line))?;
                Ok(Some(app_widget_handle(id, "table")))
            }
            "image" => {
                app_argc(fn_name, count, 4, 4, line)?;
                let win = handle(0)?;
                let path = app_func_name(&vals[1], fn_name, line)
                    .map_err(|_| app_want_text_err(fn_name, line, "ပုံဖိုင် လမ်းကြောင်း စာသား"))?;
                let id = crate::app_library::image(win, &path, num(2)?, num(3)?)
                    .map_err(|e| app_err(e, line))?;
                Ok(Some(app_widget_handle(id, "image")))
            }
            "canvas" => {
                app_argc(fn_name, count, 5, 5, line)?;
                let win = handle(0)?;
                let id =
                    crate::app_library::canvas(win, num(1)?, num(2)?, num(3)?, num(4)?)
                        .map_err(|e| app_err(e, line))?;
                Ok(Some(app_widget_handle(id, "canvas")))
            }

            // ----- values, geometry, style -----
            "get" => {
                app_argc(fn_name, count, 1, 1, line)?;
                let widget = handle(0)?;
                let value = crate::app_library::get(widget).map_err(|e| app_err(e, line))?;
                Ok(Some(app_value_to(value)))
            }
            "set" => {
                app_argc(fn_name, count, 2, 2, line)?;
                let widget = handle(0)?;
                let value = app_value_from(&vals[1]);
                crate::app_library::set(widget, value).map_err(|e| app_err(e, line))?;
                Ok(None)
            }
            // `App.items(w)` / `App.set_items(w, list)`: the item list of a
            // choice/listbox, or the rows of a table.
            "items" => {
                app_argc(fn_name, count, 1, 1, line)?;
                let widget = handle(0)?;
                let value =
                    crate::app_library::items(widget).map_err(|e| app_err(e, line))?;
                Ok(Some(app_value_to(value)))
            }
            "set_items" => {
                app_argc(fn_name, count, 2, 2, line)?;
                let widget = handle(0)?;
                let value = app_items_value(&vals[1], fn_name, line)?;
                crate::app_library::set_items(widget, value).map_err(|e| app_err(e, line))?;
                Ok(None)
            }
            // `App.cell(t, row, col)` / `App.set_cell(t, row, col, v)`:
            // one table cell read or written by position.
            "cell" => {
                app_argc(fn_name, count, 3, 3, line)?;
                let widget = handle(0)?;
                let row = app_index(&vals[1], fn_name, line)?;
                let column = app_index(&vals[2], fn_name, line)?;
                let value = crate::app_library::cell(widget, row, column)
                    .map_err(|e| app_err(e, line))?;
                Ok(Some(app_value_to(value)))
            }
            "set_cell" => {
                app_argc(fn_name, count, 4, 4, line)?;
                let widget = handle(0)?;
                let row = app_index(&vals[1], fn_name, line)?;
                let column = app_index(&vals[2], fn_name, line)?;
                let value = app_value_from(&vals[3]);
                crate::app_library::set_cell(widget, row, column, &value)
                    .map_err(|e| app_err(e, line))?;
                Ok(None)
            }
            // `App.number(widget)` / `App.number("12.5")`: the value of a
            // widget -- or a piece of text -- read as a number, so a text
            // box can be used in arithmetic (`x :float = App.number(box);`).
            "number" => {
                app_argc(fn_name, count, 1, 1, line)?;
                let text = match &vals[0] {
                    Value::Object(..) => {
                        let widget = handle(0)?;
                        let value =
                            crate::app_library::get(widget).map_err(|e| app_err(e, line))?;
                        match value {
                            crate::app_library::AppValue::Text(s) => s,
                            crate::app_library::AppValue::Int(i) => return Ok(Some(Value::Int(i))),
                            crate::app_library::AppValue::Float(f) => {
                                return Ok(Some(Value::Float(f)))
                            }
                            crate::app_library::AppValue::Bool(_)
                            | crate::app_library::AppValue::Items(_)
                            | crate::app_library::AppValue::Rows(_) => {
                                return Err(app_want_text_err(
                                    fn_name,
                                    line,
                                    "စာသား (text) သို့မဟုတ် ကိန်းဂဏန်း",
                                ))
                            }
                        }
                    }
                    other => display(other),
                };
                match app_parse_number(&text) {
                    Some(v) => Ok(Some(v)),
                    None => Err(app_err(crate::app_library::err_not_a_number(&text), line)),
                }
            }
            "move" => {
                app_argc(fn_name, count, 3, 3, line)?;
                let widget = handle(0)?;
                crate::app_library::move_to(widget, num(1)?, num(2)?)
                    .map_err(|e| app_err(e, line))?;
                Ok(None)
            }
            "size" => {
                app_argc(fn_name, count, 3, 3, line)?;
                let widget = handle(0)?;
                crate::app_library::resize(widget, num(1)?, num(2)?)
                    .map_err(|e| app_err(e, line))?;
                Ok(None)
            }
            "color" => {
                app_argc(fn_name, count, 2, 3, line)?;
                let widget = handle(0)?;
                let fg = if !matches!(vals[1], Value::Str(_)) {
                    return Err(app_want_text_err(fn_name, line, "အရောင်စာသား"));
                } else {
                    display(&vals[1])
                };
                let bg = if count >= 3 {
                    Some(display(&vals[2]))
                } else {
                    None
                };
                crate::app_library::color(widget, Some(&fg), bg.as_deref())
                    .map_err(|e| app_err(e, line))?;
                Ok(None)
            }
            "font" => {
                app_argc(fn_name, count, 3, 3, line)?;
                let widget = handle(0)?;
                let name = app_func_name(&vals[1], fn_name, line)?;
                crate::app_library::font(widget, &name, num(2)?)
                    .map_err(|e| app_err(e, line))?;
                Ok(None)
            }
            "show" | "hide" | "enable" | "disable" => {
                app_argc(fn_name, count, 1, 1, line)?;
                let widget = handle(0)?;
                let result = match fn_name {
                    "show" => crate::app_library::show(widget),
                    "hide" => crate::app_library::hide(widget),
                    "enable" => crate::app_library::enable(widget),
                    _ => crate::app_library::disable(widget),
                };
                result.map_err(|e| app_err(e, line))?;
                Ok(None)
            }

            // ----- events -----
            "on_key" => {
                app_argc(fn_name, count, 3, 3, line)?;
                let win = handle(0)?;
                let key = app_func_name(&vals[1], fn_name, line)?;
                let func = app_func_name(&vals[2], fn_name, line)?;
                crate::app_library::on_key(win, &key, &func).map_err(|e| app_err(e, line))?;
                Ok(None)
            }
            "every" => {
                app_argc(fn_name, count, 3, 3, line)?;
                let win = handle(0)?;
                let ms = as_i64(&vals[1])
                    .filter(|ms| *ms > 0)
                    .ok_or_else(|| app_want_text_err(fn_name, line, "1 နှင့်အထက် မီလီစက္ကန့် ကိန်းပြည့်"))?;
                let func = app_func_name(&vals[2], fn_name, line)?;
                crate::app_library::every(win, ms as u64, &func).map_err(|e| app_err(e, line))?;
                Ok(None)
            }

            // ----- dialogs -----
            "message" => {
                app_argc(fn_name, count, 1, 1, line)?;
                crate::app_library::message(&text(0));
                Ok(None)
            }
            "ask" => {
                app_argc(fn_name, count, 1, 1, line)?;
                Ok(Some(Value::Bool(crate::app_library::ask(&text(0)))))
            }
            "pick_file" => {
                app_argc(fn_name, count, 0, 0, line)?;
                Ok(Some(Value::Str(crate::app_library::pick_file())))
            }

            // ----- canvas drawing -----
            "rect" => {
                app_argc(fn_name, count, 6, 6, line)?;
                let cv = handle(0)?;
                crate::app_library::rect(cv, num(1)?, num(2)?, num(3)?, num(4)?, &display(&vals[5]))
                    .map_err(|e| app_err(e, line))?;
                Ok(None)
            }
            "circle" => {
                app_argc(fn_name, count, 5, 5, line)?;
                let cv = handle(0)?;
                crate::app_library::circle(cv, num(1)?, num(2)?, num(3)?, &display(&vals[4]))
                    .map_err(|e| app_err(e, line))?;
                Ok(None)
            }
            "line" => {
                app_argc(fn_name, count, 6, 6, line)?;
                let cv = handle(0)?;
                crate::app_library::line(
                    cv,
                    num(1)?,
                    num(2)?,
                    num(3)?,
                    num(4)?,
                    &display(&vals[5]),
                )
                .map_err(|e| app_err(e, line))?;
                Ok(None)
            }
            "text" => {
                app_argc(fn_name, count, 5, 5, line)?;
                let cv = handle(0)?;
                crate::app_library::text(cv, num(1)?, num(2)?, &text(3), &display(&vals[4]))
                    .map_err(|e| app_err(e, line))?;
                Ok(None)
            }
            "clear" => {
                app_argc(fn_name, count, 1, 1, line)?;
                let cv = handle(0)?;
                crate::app_library::clear(cv).map_err(|e| app_err(e, line))?;
                Ok(None)
            }

            _ => Err(lib_fn_unknown_err(LIB_APP_NAME, fn_name, line)),
        }
    }

    /// Guard for library functions reached through the `<lib> ၏ <fn>`
    /// syntax: the library has to have been imported first, exactly like
    /// the keyword forms (`စောင့်ပါ`, `ကျပန်းကိန်း`, ...) require.
    fn require_library_loaded(&self, lib: &str, line: usize) -> Result<(), String> {
        if self.libraries.is_loaded(lib) {
            Ok(())
        } else {
            Err(format!(
                "E089 လိုင်း {} တွင် \"{}\" ကို သုံးရန် \"နည်းပညာများ {} ကို အသုံးပြုပါ။\" ဖြင့် နည်းပညာများကို အရင်ထည့်သွင်းရပါမည်။",
                line, lib, lib
            ))
        }
    }

    /// Imports one library by name, in this order:
    ///   1. built-ins (`ကျပန်း`, `အချိန်`, `request`) compiled into akk,
    ///   2. a package downloaded with `akk install <name>`, and
    ///   3. a plain `<name>.akk` script next to the running program.
    /// Anything else is the "library not found" error (E062). A script or
    /// package is plain Akkhara, so loading it just means running it --
    /// that registers its function/class definitions the same way any
    /// top-level definition does -- and the functions it defines are
    /// remembered so `<lib> ၏ <fn>` reaches exactly those.
    fn import_library(&mut self, name: &str, line: usize) -> Result<(), String> {
        if self.libraries.is_loaded(name) {
            // Already brought in: running it again would re-run its
            // top-level statements and re-register everything it defines.
            return Ok(());
        }
        if name == "ကျပန်း" || name == "အချိန်" || name == "request" || name == LIB_APP_NAME {
            self.libraries.mark_loaded(name);
            return Ok(());
        }
        let src = self
            .libraries
            .find_dynamic_source(name)
            .or_else(|| self.libraries.find_script_source(name));
        match src {
            Some(src) => self.run_library_source(name, &src, line),
            None => Err(format!(
                "E062 လိုင်း {} တွင် \"{}\" ဆိုသော နည်းပညာများ (library) ကို ရှာမတွေ့ပါ။ \"akk install {}\" ဖြင့် ထည့်သွင်းကြည့်ပါ၊ သို့မဟုတ် program နှင့် တစ်ခုတည်းသော folder တွင် \"{}.akk\" ဖိုင်ကို ထားပါ။",
                line, name, name, name
            )),
        }
    }

    /// Lexes, parses and runs one script/package library's source, then
    /// records the functions it defined. `begin_loading`/`end_loading`
    /// turn an import cycle (a library that imports itself, directly or
    /// through other scripts) into the E119 error instead of infinite
    /// recursion; the library is only marked loaded once it succeeds.
    fn run_library_source(&mut self, name: &str, src: &str, line: usize) -> Result<(), String> {
        if !self.libraries.begin_loading(name) {
            return Err(format!(
                "E119 လိုင်း {} တွင် \"{}\" နည်းပညာများကို အပြန်အလှန် import လုပ်နေပါသည် (import cycle)။",
                line, name
            ));
        }
        let before: std::collections::HashSet<String> =
            self.functions.keys().cloned().collect();
        let result = (|| -> Result<(), String> {
            let tokens = crate::lexer::lex(src).map_err(|e| {
                format!(
                    "E066 လိုင်း {} တွင် \"{}\" library ကို ဖတ်ရာတွင် error ဖြစ်ပေါ်ခဲ့ပါသည် - {}",
                    line, name, e
                )
            })?;
            let stmts = crate::parser::parse(&tokens).map_err(|e| {
                format!(
                    "E067 လိုင်း {} တွင် \"{}\" library ကို parse လုပ်ရာတွင် error ဖြစ်ပေါ်ခဲ့ပါသည် - {}",
                    line, name, e
                )
            })?;
            self.run(&stmts)
        })();
        self.libraries.end_loading(name);
        result?;
        let defined: Vec<String> = self
            .functions
            .keys()
            .filter(|k| !before.contains(*k))
            .cloned()
            .collect();
        self.lib_functions.insert(name.to_string(), defined);
        self.libraries.mark_loaded(name);
        Ok(())
    }

    /// Is `name` a library this binary knows about? Used to tell
    /// `<lib> အဖြစ် <alias>။` (a library alias) apart from the older
    /// `<fn> အဖြစ် <alias>` function alias. A name counts when it's a
    /// built-in, a downloaded package, or a plain `<name>.akk` script.
    fn is_known_library(&self, name: &str) -> bool {
        name == "request" || name == "ကျပန်း" || name == "အချိန်" || name == LIB_APP_NAME
            || self.libraries.find_dynamic_source(name).is_some()
            || self.libraries.find_script_source(name).is_some()
    }

    /// Rewrites a `<lib>` written in source through any registered alias,
    /// so `r ကိန်း(1, 10) ကို လုပ်ပါ။` dispatches to `ကျပန်း`. Names with
    /// no alias pass through unchanged.
    fn resolve_lib(&self, name: &str) -> String {
        self.lib_aliases
            .get(name)
            .cloned()
            .unwrap_or_else(|| name.to_string())
    }

    fn eval(&mut self, expr: &Expr, line: usize, var_ctx: Option<&str>) -> Result<Value, String> {
        match expr {
            // Literals already parsed by the parser -- no per-evaluation
            // string parsing.
            Expr::Int(i) => Ok(Value::Int(*i)),
            Expr::Float(f) => Ok(Value::Float(*f)),
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
            Expr::LibCall(lib, fn_name, arg_exprs, call_line) => {
                // `<lib>.<fn>(args)` -- the English spelling of
                // `<lib> ၏ <fn>(args) ကို လုပ်ပါ။`. The library still has to
                // be imported, and `lib` may be a registered alias.
                self.eval_lib_call(lib, fn_name, arg_exprs, *call_line)?
                    .ok_or_else(|| eng_lib_no_value_err(lib, fn_name, *call_line))
            }
            Expr::LibFnRef(lib, fn_name, refline) => {
                // `<lib>.<fn>` without a call is a first-class reference:
                // its value binds the library function, so `TOOL = Tools.add;`
                // then `TOOL(1, 100)` calls it. The library must be imported,
                // and for a script/package library the function must be one
                // it defines.
                let resolved = self.resolve_lib(lib);
                let lib = resolved.as_str();
                if lib == LIB_REQUEST_NAME || lib == "ကျပန်း" || lib == "အချိန်" {
                    self.require_library_loaded(lib, *refline)?;
                } else {
                    let owns_fn = self
                        .lib_functions
                        .get(lib)
                        .map(|fns| fns.iter().any(|f| f == fn_name))
                        .unwrap_or(false);
                    if !owns_fn {
                        if self.lib_functions.contains_key(lib) {
                            return Err(lib_fn_unknown_err(lib, fn_name, *refline));
                        }
                        return Err(lib_unknown_err(lib, *refline));
                    }
                }
                Ok(Value::Callable(Callable::Library {
                    lib: lib.to_string(),
                    fn_name: fn_name.clone(),
                }))
            }
            Expr::NewObj(class_name, arg_exprs, new_line) => {
                // `name(args)` in an expression reaches here (the parser uses
                // this node for any call-shaped `<name>(...)`): a user class
                // wins, then a user function, then an eng builtin. Anything
                // else falls through to the class-not-found error.
                if !self.classes.contains_key(class_name) {
                    if self.functions.contains_key(class_name) {
                        let result = self.call_function(class_name, arg_exprs, *new_line)?;
                        return result
                            .ok_or_else(|| eng_no_return_err(class_name, *new_line));
                    }
                    if is_eng_builtin(class_name) {
                        return self.call_builtin(class_name, arg_exprs, *new_line);
                    }
                    // A variable holding a callable: `TOOL(1, 100)` invokes
                    // the library function `TOOL` was bound to.
                    if let Some(Value::Callable(kind)) = self.env.get(class_name).cloned() {
                        return self.invoke_callable(&kind, arg_exprs, *new_line);
                    }
                }
                self.construct_object(class_name, arg_exprs, *new_line)
            }
            Expr::HttpGet(lib, url_expr, gline) => {
                let resolved = self.resolve_lib(lib);
                if resolved != LIB_REQUEST_NAME {
                    return Err(format!(
                        "E062 လိုင်း {} တွင် \"{}\" ဆိုသော နည်းပညာများ (library) ကို ရှာမတွေ့ပါ။",
                        gline, resolved
                    ));
                }
                if !self.libraries.is_loaded(LIB_REQUEST_NAME) {
                    return Err(format!(
                        "E078 လိုင်း {} တွင် \"request\" ကို သုံးရန် \"နည်းပညာများ request ကို အသုံးပြုပါ။\" ဖြင့် request နည်းပညာများကို အရင်ထည့်သွင်းရပါမည်။",
                        gline
                    ));
                }
                let url_v = self.eval(url_expr, *gline, var_ctx)?;
                let url_s = display(&url_v);
                let resp = crate::request_library::get(&url_s)
                    .map_err(|e| format!("{} (လိုင်း {})", e, gline))?;
                Ok(http_response_to_value(resp))
            }
            Expr::FieldAccess(obj_expr, field, fline) => {
                let ov = self.eval(obj_expr, line, var_ctx)?;
                match &ov {
                    Value::Object(_, fields) => fields
                        .iter()
                        .find(|(k, _)| k == field)
                        .map(|(_, v)| v.clone())
                        .ok_or_else(|| {
                            format!(
                                "E082 လိုင်း {} တွင် object ၌ \"{}\" ဆိုသော field မရှိပါ။",
                                fline, field
                            )
                        }),
                    other => Err(format!(
                        "E083 လိုင်း {} တွင် {} အပေါ် \"၏\" ကို သုံး၍မရပါ (Object တစ်ခု ဖြစ်ရပါမည်)။",
                        fline,
                        type_name_mm(other)
                    )),
                }
            }
        }
    }

    /// Look up a user-defined function, bind its argument(s) to its
    /// parameter(s) in the shared environment, and run its body. If the
    /// body's final statement is a bare expression statement (`ExprStmt`),
    /// that expression's value is returned as the function's "return value";
    /// otherwise `None` is returned (the function produced no value).
    fn call_function(
        &mut self,
        name: &str,
        args: &[Expr],
        line: usize,
    ) -> Result<Option<Value>, String> {
        let (params, body) = match self.functions.get(name) {
            Some(v) => v.clone(),
            None => {
                return Err(format!(
                    "E031 လိုင်း {} တွင် \"{}\" ဆိုသော function ကို ရှာမတွေ့ပါ။",
                    line, name
                ));
            }
        };
        if args.len() != params.len() {
            return Err(format!(
                "E033 လိုင်း {} တွင် \"{}\" function သည် argument {} ခု လိုအပ်ပါသည်၊ {} ခု ပေးထားပါသည်။",
                line,
                name,
                params.len(),
                args.len()
            ));
        }
        // Evaluate every argument in the caller's environment first, then
        // bind the parameters -- remembering any globals they shadow so the
        // bindings can be pushed back when the call returns. Without that,
        // a call would clobber same-named globals, and recursion would see a
        // sibling frame's parameters.
        let mut arg_values = Vec::with_capacity(args.len());
        for e in args {
            arg_values.push(self.eval(e, line, None)?);
        }
        // A function body is its own loop context: a `break` inside it belongs
        // to a loop written inside it, never to the caller's enclosing loop.
        let outer_loop_depth = self.loop_depth;
        self.loop_depth = 0;
        let mut saved: Vec<(String, Option<Value>)> = Vec::with_capacity(params.len());
        for (p, v) in params.iter().zip(arg_values.into_iter()) {
            if !saved.iter().any(|(saved_name, _)| saved_name == p) {
                saved.push((p.clone(), self.env.get(p).cloned()));
            }
            self.env.insert(p.clone(), v);
        }
        let mut return_value: Option<Value> = None;
        let mut failure: Option<String> = None;
        self.call_depth += 1;
        for (i, s) in body.iter().enumerate() {
            if self.return_signal.is_some() {
                break;
            }
            if i + 1 == body.len() {
                match s {
                    Stmt::ExprStmt { value, line: sline } => {
                        match self.eval(value, *sline, None) {
                            Ok(v) => {
                                return_value = Some(v);
                                continue;
                            }
                            Err(e) => {
                                failure = Some(e);
                                break;
                            }
                        }
                    }
                    // A function ending in "<var> သည် <expr> ဖြစ်၏။" -- the
                    // idiomatic "ရလဒ် သည် ... ဖြစ်၏။" pattern -- also counts
                    // as an implicit return: the variable still gets
                    // assigned as normal, and its value is also returned.
                    Stmt::VarDecl {
                        name: var_name,
                        value,
                        line: sline,
                    } => match self.eval(value, *sline, None) {
                        Ok(v) => {
                            self.env.insert(var_name.clone(), v.clone());
                            return_value = Some(v);
                            continue;
                        }
                        Err(e) => {
                            failure = Some(e);
                            break;
                        }
                    },
                    _ => {}
                }
            }
            if let Err(e) = self.exec(s) {
                failure = Some(e);
                break;
            }
        }
        self.call_depth -= 1;
        self.loop_depth = outer_loop_depth;
        // An explicit `return` overrides whatever the last statement yielded,
        // and a `break` that couldn't find a loop in here must not leak out to
        // the caller's.
        self.break_signal = None;
        if let Some(signal) = self.return_signal.take() {
            return_value = signal;
        }
        for (p, old) in saved {
            match old {
                Some(v) => {
                    self.env.insert(p, v);
                }
                None => {
                    self.env.remove(&p);
                }
            }
        }
        if let Some(e) = failure {
            return Err(e);
        }
        Ok(return_value)
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
        let info = match self.classes.get(class_name) {
            Some(c) => c.clone(),
            None => {
                return Err(format!(
                    "E032 လိုင်း {} တွင် \"{}\" ဆိုသော class ကို ရှာမတွေ့ပါ။",
                    line, class_name
                ));
            }
        };
        // The constructor's parameters come from the class header's
        // `(params)` list when it has one; otherwise the first method (the
        // constructor) supplies them from its own parameter list. Either
        // way the first method defined in the class is the constructor.
        let params: Vec<String> = match &info.ctor_params {
            Some(header_params) => header_params.clone(),
            None => info.methods[0].1.clone(),
        };
        let body = info.methods[0].2.clone();

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
        // Constructor parameters shadow globals only for the duration of the
        // construction, same as function parameters.
        let mut saved: Vec<(String, Option<Value>)> = Vec::with_capacity(params.len());
        for (p, v) in params.iter().zip(arg_values.into_iter()) {
            if !saved.iter().any(|(saved_name, _)| saved_name == p) {
                saved.push((p.clone(), self.env.get(p).cloned()));
            }
            self.env.insert(p.clone(), v);
        }

        self.self_stack.push(Vec::new());
        // The constructor body counts as a function body, so a `return`
        // inside a method doesn't escape to the top level (E108), and a
        // `break` in it can't reach a caller's loop.
        self.call_depth += 1;
        let outer_signal = self.return_signal.take();
        let outer_loop_depth = self.loop_depth;
        self.loop_depth = 0;
        let run_result = (|| -> Result<(), String> {
            for s in &body {
                if self.return_signal.is_some() {
                    break;
                }
                self.exec(s)?;
            }
            Ok(())
        })();
        // A method-level `return` only ends construction: the object itself
        // is still the result, and any outer pending return is preserved.
        self.return_signal = outer_signal;
        self.loop_depth = outer_loop_depth;
        self.break_signal = None;
        self.call_depth -= 1;
        for (p, old) in saved {
            match old {
                Some(v) => {
                    self.env.insert(p, v);
                }
                None => {
                    self.env.remove(&p);
                }
            }
        }
        let fields = self.self_stack.pop().unwrap_or_default();
        run_result?;

        Ok(Value::Object(class_name.to_string(), fields))
    }

    /// Run a condition-tested loop body -- the `while (<cond>) { ... }`
    /// spelling, and (through `Stmt::EngFor`) the C-style eng `for` loop,
    /// which only differs in the label its iteration-cap error names.
    fn exec_condition_loop(
        &mut self,
        cond: &CondChain,
        negate: bool,
        body: &[Stmt],
        line: usize,
        label: &str,
    ) -> Result<(), String> {
        const MAX_ITERS: u64 = 5_000_000;
        // `loop_depth` gates `break`: a `break` in here must find this loop,
        // and must be consumed on the way out (so an outer loop keeps
        // running).
        self.loop_depth += 1;
        let result = (|| -> Result<(), String> {
            let mut iterations: u64 = 0;
            loop {
                let c = self.eval_cond_chain(cond)?;
                let should_run = if negate { !c } else { c };
                if !should_run {
                    break;
                }
                iterations += 1;
                if iterations > MAX_ITERS {
                    return Err(format!(
                        "E046 လိုင်း {} ၏ {} သည် ကြိမ်ရေ အလွန်များနေပါသည် (loop ထဲက variable ကို update မလုပ်ထားလို့ အဆုံးမရှိ ပတ်နေခြင်း ဖြစ်နိုင်ပါသည်)။",
                        line, label
                    ));
                }
                let broke = self.run_loop_body(body)?;
                if broke.is_some() || self.return_signal.is_some() {
                    break;
                }
            }
            Ok(())
        })();
        self.loop_depth -= 1;
        result
    }

    /// Call a named callee as a statement (`name(args);`): a user-defined
    /// function wins, then an eng builtin. Returns the callee's value when it
    /// produced one (statement callers may discard it).
    fn call_named(
        &mut self,
        name: &str,
        args: &[Expr],
        line: usize,
    ) -> Result<Option<Value>, String> {
        if self.functions.contains_key(name) {
            return self.call_function(name, args, line);
        }
        if is_eng_builtin(name) {
            return Ok(Some(self.call_builtin(name, args, line)?));
        }
        if let Some(Value::Callable(kind)) = self.env.get(name).cloned() {
            return self.invoke_callable(&kind, args, line).map(Some);
        }
        Err(eng_unknown_fn_err(line, name))
    }

    /// Invoke a first-class library function bound as a value (see
    /// `Value::Callable`). Produced no value like any other library call --
    /// e.g. `အချိန်`'s `စောင့်` -- is the E117 error.
    fn invoke_callable(
        &mut self,
        kind: &Callable,
        args: &[Expr],
        line: usize,
    ) -> Result<Value, String> {
        match kind {
            Callable::Library { lib, fn_name } => self
                .eval_lib_call(lib, fn_name, args, line)?
                .ok_or_else(|| eng_lib_no_value_err(lib, fn_name, line)),
        }
    }

    /// Run one eng builtin: `len`, `abs`, `min`, `max`, `sqrt`, `floor`,
    /// `ceil`, `round`, `upper`, `lower`, `trim`, `contains`. Builtins are
    /// core eng syntax -- no `နည်းပညာများ ... ကို အသုံးပြုပါ။` needed -- and
    /// work both as statements (`len(xs);`) and inside expressions
    /// (`n :int = len(xs);`).
    fn call_builtin(
        &mut self,
        name: &str,
        arg_exprs: &[Expr],
        line: usize,
    ) -> Result<Value, String> {
        let mut args: Vec<Value> = Vec::with_capacity(arg_exprs.len());
        for e in arg_exprs {
            args.push(self.eval(e, line, None)?);
        }

        let one_arg = || -> Result<(), String> {
            if args.len() == 1 {
                Ok(())
            } else {
                Err(eng_builtin_err(
                    line,
                    name,
                    &format!("takes exactly one argument, got {}", args.len()),
                ))
            }
        };
        let bad_arg = |detail: String| eng_builtin_err(line, name, &detail);

        match name {
            "len" => {
                one_arg()?;
                match &args[0] {
                    Value::Str(s) => Ok(Value::Int(s.chars().count() as i64)),
                    Value::List(items) | Value::Tuple(items) | Value::Set(items) => {
                        Ok(Value::Int(items.len() as i64))
                    }
                    Value::Dict(pairs) => Ok(Value::Int(pairs.len() as i64)),
                    other => Err(bad_arg(format!(
                        "needs a string or collection, got {}",
                        type_name_mm(other)
                    ))),
                }
            }
            "abs" => {
                one_arg()?;
                match &args[0] {
                    Value::Int(i) => Ok(Value::Int(i.abs())),
                    Value::Float(f) => Ok(Value::Float(f.abs())),
                    other => Err(bad_arg(format!(
                        "needs a number, got {}",
                        type_name_mm(other)
                    ))),
                }
            }
            "min" | "max" => {
                // Either a spread of values (`min(3, 1, 2)`) or one
                // collection to pick the smallest/largest element of.
                let candidates: Vec<Value> = if args.len() == 1 {
                    match &args[0] {
                        Value::List(items) | Value::Tuple(items) | Value::Set(items) => {
                            items.clone()
                        }
                        other => vec![other.clone()],
                    }
                } else {
                    args.clone()
                };
                if candidates.is_empty() {
                    return Err(bad_arg("needs at least one value".to_string()));
                }
                let mut best = candidates[0].clone();
                for c in &candidates[1..] {
                    let ord = value_ordering(&best, c, line, name)?;
                    let replace = if name == "min" {
                        ord == std::cmp::Ordering::Greater
                    } else {
                        ord == std::cmp::Ordering::Less
                    };
                    if replace {
                        best = c.clone();
                    }
                }
                Ok(best)
            }
            "sqrt" => {
                one_arg()?;
                let f = as_f64(&args[0]).ok_or_else(|| {
                    bad_arg(format!("needs a number, got {}", type_name_mm(&args[0])))
                })?;
                if f < 0.0 {
                    return Err(eng_sqrt_negative_err(line, &args[0]));
                }
                Ok(Value::Float(f.sqrt()))
            }
            "floor" | "ceil" | "round" => {
                one_arg()?;
                let f = as_f64(&args[0]).ok_or_else(|| {
                    bad_arg(format!("needs a number, got {}", type_name_mm(&args[0])))
                })?;
                let rounded = match name {
                    "floor" => f.floor(),
                    "ceil" => f.ceil(),
                    _ => f.round(),
                };
                Ok(Value::Int(rounded as i64))
            }
            "upper" | "lower" | "trim" => {
                one_arg()?;
                match &args[0] {
                    Value::Str(s) => Ok(Value::Str(match name {
                        "upper" => s.to_uppercase(),
                        "lower" => s.to_lowercase(),
                        _ => s.trim().to_string(),
                    })),
                    other => Err(bad_arg(format!(
                        "needs a string, got {}",
                        type_name_mm(other)
                    ))),
                }
            }
            "contains" => {
                if args.len() != 2 {
                    return Err(bad_arg(format!(
                        "takes exactly two arguments, got {}",
                        args.len()
                    )));
                }
                let needle = &args[1];
                let found = match &args[0] {
                    Value::Str(s) => match needle {
                        Value::Str(n) => s.contains(n.as_str()),
                        other => {
                            return Err(bad_arg(format!(
                                "needs a string to search for, got {}",
                                type_name_mm(other)
                            )))
                        }
                    },
                    Value::List(items) | Value::Tuple(items) | Value::Set(items) => {
                        items.iter().any(|v| value_eq(v, needle))
                    }
                    Value::Dict(pairs) => pairs.iter().any(|(k, _)| value_eq(k, needle)),
                    other => {
                        return Err(bad_arg(format!(
                            "needs a string or collection first, got {}",
                            type_name_mm(other)
                        )))
                    }
                };
                Ok(Value::Bool(found))
            }
            _ => Err(eng_unknown_fn_err(line, name)),
        }
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

fn as_i64(v: &Value) -> Option<i64> {
    match v {
        Value::Int(i) => Some(*i),
        Value::Float(f) => Some(*f as i64),
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
            '^' => {
                if *b >= 0 {
                    match (*a).checked_pow(*b as u32) {
                        Some(v) => Ok(Value::Int(v)),
                        None => Ok(Value::Float((*a as f64).powf(*b as f64))),
                    }
                } else {
                    Ok(Value::Float((*a as f64).powf(*b as f64)))
                }
            }
            '\\' => {
                if *b == 0 {
                    Err(format!(
                        "E073 လိုင်း {} တွင် သုညဖြင့် အပြည့်ကိန်းစား၍မရပါ။",
                        line
                    ))
                } else {
                    Ok(Value::Int((*a as f64 / *b as f64).floor() as i64))
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
        '^' => Ok(Value::Float(a.powf(b))),
        '\\' => {
            if b == 0.0 {
                Err(format!(
                    "E074 လိုင်း {} တွင် သုညဖြင့် အပြည့်ကိန်းစား၍မရပါ။",
                    line
                ))
            } else {
                Ok(Value::Float((a / b).floor()))
            }
        }
        _ => unreachable!(),
    }
}

// --- eng library (English-spelled declarations) helpers ---

/// Human-readable type family name for eng error messages: `int` and
/// `float` both accept any number, so they share one phrasing.
fn eng_type_expectation(type_name: &str) -> &'static str {
    match type_name {
        "int" | "float" => "a number",
        "str" => "a string",
        "bool" => "true or false",
        _ => "a value of the declared type",
    }
}

/// Error for assigning (or redeclaring) a name that was declared with the
/// eng `pin` form. Raised from any assignment path -- Myanmar sentence
/// forms included -- so `pin` really is immutable for the whole program.
fn eng_const_reassign_err(name: &str, line: usize) -> String {
    format!(
        "E104 line {}: \"{}\" was declared with 'pin' and cannot be reassigned",
        line, name
    )
}

/// Error for console input that doesn't parse as the declared type.
fn eng_input_convert_err(line: usize, text: &str, type_name: &str) -> String {
    format!(
        "E107 line {}: cannot convert input \"{}\" to {}",
        line,
        text,
        eng_type_expectation(type_name)
    )
}

/// Coerce one line of console input to an eng-declared type. `str` keeps
/// the raw text; `int`/`float` parse numbers; `bool` accepts true/false
/// (any casing) plus the 1/0 spellings.
fn coerce_eng_input(
    text: &str,
    type_name: &str,
    line: usize,
) -> Result<Value, String> {
    match type_name {
        "str" => Ok(Value::Str(text.to_string())),
        "int" => text
            .parse::<i64>()
            .map(Value::Int)
            .map_err(|_| eng_input_convert_err(line, text, type_name)),
        "float" => text
            .parse::<f64>()
            .map(Value::Float)
            .map_err(|_| eng_input_convert_err(line, text, type_name)),
        "bool" => match text {
            "true" | "True" | "1" => Ok(Value::Bool(true)),
            "false" | "False" | "0" => Ok(Value::Bool(false)),
            _ => Err(eng_input_convert_err(line, text, type_name)),
        },
        _ => Err(eng_input_convert_err(line, text, type_name)),
    }
}

/// The eng library's built-in helpers. They're core syntax, so they work
/// without importing anything.
fn is_eng_builtin(name: &str) -> bool {
    matches!(
        name,
        "len"
            | "abs"
            | "min"
            | "max"
            | "sqrt"
            | "floor"
            | "ceil"
            | "round"
            | "upper"
            | "lower"
            | "trim"
            | "contains"
    )
}

/// Enforce an eng declaration's type rule on a value and coerce an int to
/// float for a `:float` annotation (`pi :float = 3;` holds 3.0). Shared by
/// every eng declaration form, loop-value declarations included.
fn check_eng_value(name: &str, type_name: &str, v: Value, line: usize) -> Result<Value, String> {
    // A callable (a library function bound to a name) may be declared under
    // any type: the annotation describes the value the function returns,
    // which is enforced when it is actually called.
    if matches!(v, Value::Callable(_)) {
        return Ok(v);
    }
    // int accepts floats the way the rest of Akkhara does: int/float are one
    // numeric family.
    let type_ok = match type_name {
        "int" | "float" => matches!(v, Value::Int(_) | Value::Float(_)),
        "str" => matches!(v, Value::Str(_)),
        "bool" => matches!(v, Value::Bool(_)),
        _ => false,
    };
    if !type_ok {
        return Err(format!(
            "E103 line {}: cannot assign {} to \"{}\" (declared :{}; value must be {})",
            line,
            type_name_mm(&v),
            name,
            type_name,
            eng_type_expectation(type_name)
        ));
    }
    Ok(if type_name == "float" {
        match v {
            Value::Int(i) => Value::Float(i as f64),
            other => other,
        }
    } else {
        v
    })
}

/// The value an eng declaration seeds before its `loop { ... }` runs, so the
/// body has something to accumulate into.
fn eng_type_default(type_name: &str) -> Value {
    match type_name {
        "int" => Value::Int(0),
        "float" => Value::Float(0.0),
        "str" => Value::Str(String::new()),
        _ => Value::Bool(false),
    }
}

/// Error for a `pin` name that would redeclare an existing variable.
fn eng_pin_redeclare_err(name: &str, line: usize) -> String {
    format!(
        "E105 line {}: pin \"{}\" cannot redeclare an existing variable",
        line, name
    )
}

/// `break` (eng or `ရပ်ပါ`) used where there is no loop to stop.
fn eng_break_outside_loop_err(line: usize) -> String {
    format!("E116 line {}: 'break' outside a loop", line)
}

/// A `loop { ... }` used as a value ended with a bare `break`, so it has
/// nothing to assign.
fn eng_loop_no_value_err(line: usize) -> String {
    format!(
        "E115 line {}: loop used as a value ended with a bare 'break' -- use `break <value>;`",
        line
    )
}

/// Error for a builtin called with the wrong number or kind of arguments.
fn eng_builtin_err(line: usize, name: &str, detail: &str) -> String {
    format!("E110 line {}: {}() {}", line, name, detail)
}

fn eng_sqrt_negative_err(line: usize, v: &Value) -> String {
    format!(
        "E111 line {}: sqrt() needs a non-negative number, got {}",
        line,
        display(v)
    )
}

/// Order two `min`/`max` candidates: numbers compare numerically (ints and
/// floats mix freely), strings lexicographically.
fn value_ordering(
    a: &Value,
    b: &Value,
    line: usize,
    name: &str,
) -> Result<std::cmp::Ordering, String> {
    if let (Some(x), Some(y)) = (as_f64(a), as_f64(b)) {
        return Ok(x.partial_cmp(&y).unwrap_or(std::cmp::Ordering::Equal));
    }
    if let (Value::Str(x), Value::Str(y)) = (a, b) {
        return Ok(x.cmp(y));
    }
    Err(eng_builtin_err(
        line,
        name,
        &format!(
            "can only compare numbers or strings, got {} and {}",
            type_name_mm(a),
            type_name_mm(b)
        ),
    ))
}

/// `return` used where there is no enclosing function to return from.
fn eng_return_outside_fn_err(line: usize) -> String {
    format!("E108 line {}: 'return' outside a function", line)
}

/// A library function used as a value didn't produce one (e.g. the `အချိန်`
/// library's `စောင့်`/wait, which only carries out an action).
fn eng_lib_no_value_err(lib: &str, fn_name: &str, line: usize) -> String {
    format!(
        "E117 line {}: {}.{}() did not return a value",
        line, lib, fn_name
    )
}

/// A `name(...)` call naming something that is neither a function nor a builtin.
fn eng_unknown_fn_err(line: usize, name: &str) -> String {
    format!(
        "E113 line {}: \"{}\" is not a function or builtin",
        line, name
    )
}

/// A user function used as a value didn't return anything.
fn eng_no_return_err(name: &str, line: usize) -> String {
    format!(
        "E114 line {}: function \"{}\" did not return a value",
        line, name
    )
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
