use crate::lexer::{Tok, Token};

#[derive(Debug, Clone)]
pub enum Expr {
    NumLit(String),
    StrLit(String),
    BoolLit(bool),
    Ident(String),
    Binary(Box<Expr>, char, Box<Expr>, usize), // line of the operator
    Neg(Box<Expr>, usize),                     // unary minus, line
    Convert(Box<Expr>, String, usize),         // inner expr, target type keyword, line
    ListLit(Vec<Expr>),
    TupleLit(Vec<Expr>),
    SetLit(Vec<Expr>),
    DictLit(Vec<(Expr, Expr)>),
    /// Postfix index: `xs[i]` or table `xs[row, col]` (or dict `xs[key]`).
    Index(Box<Expr>, Vec<Expr>),
    /// Object instantiation: `<ClassName> [အသစ်] (arg1, arg2, ...)`.
    NewObj(String, Vec<Expr>, usize),
}

#[derive(Debug, Clone)]
pub enum Stmt {
    VarDecl {
        name: String,
        value: Expr,
        line: usize,
    },
    Print {
        value: Expr,
        line: usize,
    },
    InputNoAssign {
        value: Expr,
        line: usize,
    },
    InputAssign {
        name: String,
        value: Expr,
        line: usize,
    },
    ConvertStmt {
        value: Expr,
        target_type: String,
        line: usize,
    },
    ExprStmt {
        value: Expr,
        line: usize,
    },
    MathAssign {
        target: Expr,
        op: char,
        amount: Expr,
        line: usize,
    },
    ForLoop {
        var_name: String,
        source: ForSource,
        body: Vec<Stmt>,
        line: usize,
    },
    If {
        branches: Vec<IfBranch>,
        line: usize,
    },
    While {
        cond: CondChain,
        negate: bool,
        body: Vec<Stmt>,
        line: usize,
    },
    FuncDef {
        name: String,
        params: Vec<String>,
        body: Vec<Stmt>,
        line: usize,
    },
    FuncCall {
        name: String,
        arg: Option<Expr>,
        line: usize,
    },
    ClassDef {
        name: String,
        body: Vec<Stmt>,
        line: usize,
    },
    SelfFieldSet {
        field: String,
        value: Expr,
        line: usize,
    },
}

#[derive(Debug, Clone)]
pub struct IfBranch {
    /// None only for the final, unconditional "မဟုတ်လျှင်" (else) branch.
    pub cond: Option<CondChain>,
    /// True if this branch used the negative form ("မဖြစ်လျှင်": run when
    /// the condition chain is FALSE) rather than the positive form
    /// ("ဖြစ်လျှင်": run when TRUE).
    pub negate: bool,
    pub body: Vec<Stmt>,
}

#[derive(Debug, Clone)]
pub enum LogicalOp {
    And,
    Or,
}

#[derive(Debug, Clone)]
pub struct CondAtom {
    pub lhs: Expr,
    /// None for a bare boolean check like "(အလုပ်)" -- just test lhs's
    /// truthiness. Some(op) for a full comparison "lhs op rhs".
    pub op: Option<String>, // "<" ">" "==" "!=" "<=" ">="
    pub rhs: Option<Expr>,
    /// Some(type_name) for a type-check condition "lhs သည် <type>", e.g.
    /// "(x သည် ကိန်း)". Mutually exclusive with `op`/`rhs`.
    pub type_check: Option<String>,
    pub line: usize,
}

#[derive(Debug, Clone)]
pub struct CondChain {
    pub first: CondAtom,
    pub rest: Vec<(LogicalOp, CondAtom)>,
}

#[derive(Debug, Clone)]
pub enum ForSource {
    Range {
        start: Option<Expr>,
        end: Expr,
        step: Expr,
        op: char,
    },
    /// No explicit step-function clause was given. Resolved at runtime:
    /// a number auto-ranges from 0 (step +1); a list/tuple/set/dict iterates
    /// its elements (keys, for dict).
    Auto(Expr),
}

const KW_ASSIGN: &str = "သည်";
const KW_ASSIGN_COLLECTION: &str = "မှာ";
const KW_IS: &str = "ဖြစ်၏";
const KW_PARTICLE: &str = "ကို";
const KW_PRINT: &str = "ဖော်ပြပါ";
const KW_INPUT: &str = "မေးပါ";
const KW_FOR: &str = "အတွက်";
const KW_TO: &str = "သို့";
const KW_CONVERT: &str = "ပြောင်းပါ";
const KW_LOOP_FROM: &str = "ထဲမှ";
const KW_LOOP_EACH: &str = "တစ်ခုစီ";

// Type-check condition keywords: "<var/val> သည် <type>" inside if/while.
// Distinct from the conversion-target type keywords (e.g. "ကိန်းပြည့်" = int):
// here "ကိန်း" (no ပြည့်) means a broad "is this a number" check that also
// accepts numeric-looking strings, per spec.
const TC_STR: &str = "စာသား";
const TC_NUM: &str = "ကိန်း";
const TC_FLOAT: &str = "ဒဿမကိန်း";
const TC_BOOL: &str = "မှန်/မှား";
const KW_LOOP_END: &str = "ပြီး";

const KW_IF: &str = "အကယ်၍";
const KW_ELIF: &str = "သို့မဟုတ်";
const KW_ELSE: &str = "မဟုတ်လျှင်";
const KW_THEN_POS: &str = "ဖြစ်လျှင်";
const KW_THEN_NEG: &str = "မဖြစ်လျှင်";
const KW_AND: &str = "နှင့်";
const KW_OR: &str = "သို့";

const KW_WHILE: &str = "အခြေအနေ";
const KW_WHILE_POS: &str = "ဖြစ်နေစဉ်";
const KW_WHILE_NEG: &str = "မဖြစ်နေစဉ်";

const KW_FUNC_DEF: &str = "လုပ်ငန်း";
const KW_BY: &str = "ဖြင့်";
const KW_CALL: &str = "လုပ်ပါ";
const KW_CALL_WITH: &str = "လုပ်ရန်";

const KW_CLASS_DEF: &str = "နည်းလမ်း";
const KW_SELF: &str = "တန်ဖိုး";
const KW_NEW: &str = "အသစ်";

const TYPE_INT: &str = "ကိန်းပြည့်";
const TYPE_FLOAT: &str = "ဒဿမကိန်း";
const TYPE_STR: &str = "စာသား";
const TYPE_LIST: &str = "စာရင်း";
const TYPE_TUPLE: &str = "အစု";
const TYPE_SET: &str = "အုပ်စု";
const TYPE_DICT: &str = "အဘိဓာန်";
const TYPE_TABLE: &str = "ဇယား";

pub const MATH_FN_INC: &str = "တိုးပါ";
pub const MATH_FN_DEC: &str = "လျော့ပါ";
pub const MATH_FN_MUL: &str = "မြှောက်ပါ";
pub const MATH_FN_DIV: &str = "စားပါ";

fn math_fn_op(kw: &str) -> Option<char> {
    match kw {
        MATH_FN_INC => Some('+'),
        MATH_FN_DEC => Some('-'),
        MATH_FN_MUL => Some('*'),
        MATH_FN_DIV => Some('/'),
        _ => None,
    }
}

pub const LOOP_FN_INC: &str = "တိုးခြင်းဖြင့်";
pub const LOOP_FN_DEC: &str = "လျော့ခြင်းဖြင့်";
pub const LOOP_FN_MUL: &str = "မြှောက်ခြင်းဖြင့်";
pub const LOOP_FN_DIV: &str = "စားခြင်းဖြင့်";

fn loop_fn_op(kw: &str) -> Option<char> {
    match kw {
        LOOP_FN_INC => Some('+'),
        LOOP_FN_DEC => Some('-'),
        LOOP_FN_MUL => Some('*'),
        LOOP_FN_DIV => Some('/'),
        _ => None,
    }
}

pub fn is_type_keyword(s: &str) -> bool {
    matches!(
        s,
        TYPE_INT | TYPE_FLOAT | TYPE_STR | TYPE_LIST | TYPE_TUPLE | TYPE_SET | TYPE_DICT
            | TYPE_TABLE
    ) || s == "မှန်/မှား"
        || s == "ဘူလ်"
}

fn ident_eq(tok: &Tok, s: &str) -> bool {
    matches!(tok, Tok::Ident(v) if v == s)
}

fn is_open(tok: &Tok) -> bool {
    matches!(tok, Tok::LBracket | Tok::LParen | Tok::LBrace)
}

fn is_close(tok: &Tok) -> bool {
    matches!(tok, Tok::RBracket | Tok::RParen | Tok::RBrace)
}

fn missing_period_err(line: usize) -> String {
    format!("လိုင်း {} ၏စာကြောင်းအဆုံးတွင် '။' မရှိပါ။", line)
}

fn generic_syntax_err(line: usize) -> String {
    format!("လိုင်း {} သည် ရေးသားပုံစည်းမျဉ်းမမှန်ကန်ပါ။", line)
}

fn unclosed_bracket_err(line: usize, open: char) -> String {
    let close = match open {
        '[' => ']',
        '(' => ')',
        '{' => '}',
        _ => '?',
    };
    format!(
        "လိုင်း {} တွင် '{}' ကိုပိတ်ရန် '{}' မရှိပါ။",
        line, open, close
    )
}

fn mismatched_bracket_err(line: usize) -> String {
    format!(
        "လိုင်း {} တွင် ကွင်းများ ({{}}/[]/()) မှန်ကန်စွာ မတွဲထားပါ။",
        line
    )
}

fn dict_entry_err(line: usize) -> String {
    format!(
        "လိုင်း {} တွင် အဘိဓာန် (dict) အတွင်း <key> သည် <value> ဖြစ်၏။ ပုံစံဖြင့် ရေးရပါမည်။",
        line
    )
}

// Generic "function call" particle errors, shared by ဖော်ပြပါ / မေးပါ.
fn fn_missing_particle_err(line: usize, fname: &str) -> String {
    let full = format!("{}။", fname);
    format!(
        "လိုင်း {}၌ \"{}\" လုပ်ဆောင်ရန်  \"{}\" ၏အရှေ့၌ \"ကို\" ခံရေးရန်လိုအပ်သည်။",
        line, full, full
    )
}

fn fn_missing_value_err(line: usize, fname: &str) -> String {
    let full = format!("{}။", fname);
    format!(
        "လိုင်း {}၌ \"{}\" လုပ်ဆောင်ရန်  value မရှိသဖြင့် \"{}\" ကို မလုပ်ဆောင်နိုင်ပါ။",
        line, full, full
    )
}

fn convert_missing_value_err(line: usize) -> String {
    format!(
        "လိုင်း {} တွင် ပြောင်းလဲရန် တန်ဖိုး လိုအပ်ပါသည်။ အသုံးပြုပုံ — <value> ကို <type> သို့ ပြောင်းပါ။",
        line
    )
}

fn convert_missing_type_err(line: usize) -> String {
    format!(
        "လိုင်း {} တွင် ပြောင်းလဲရန် အမျိုးအစား လိုအပ်ပါသည်။ အသုံးပြုပုံ — <value> ကို <type> သို့ ပြောင်းပါ။",
        line
    )
}

fn math_missing_value_err(line: usize, fname: &str) -> String {
    format!(
        "လိုင်း {} တွင် \"{}\" ကိုလုပ်ဆောင်ရန် value မရှိသဖြင့် \"{}\" ကိုမလုပ်ဆောင်နိုင်ပါ။",
        line, fname, fname
    )
}

fn if_missing_condition_err(line: usize) -> String {
    format!(
        "လိုင်း {} တွင် \"အကယ်၍\"/\"သို့မဟုတ်\" အတွက် condition (စည်းကမ်းချက်) လိုအပ်ပါသည်။",
        line
    )
}

fn if_condition_syntax_err(line: usize) -> String {
    format!(
        "လိုင်း {} တွင် condition ရေးသားပုံ မှားနေပါသည်။ အသုံးပြုပုံ — (<value> <တန်ဖိုးနှိုင်းယှဉ်ခြင်း> <value>) (နှင့်|သို့) (...) ဖြစ်လျှင်",
        line
    )
}

fn if_missing_then_err(line: usize) -> String {
    format!(
        "လိုင်း {} တွင် condition အပြီးမှာ \"ဖြစ်လျှင်\" သို့မဟုတ် \"မဖြစ်လျှင်\" လိုအပ်ပါသည်။",
        line
    )
}

fn if_else_not_last_err(line: usize) -> String {
    format!(
        "လိုင်း {} တွင် \"မဟုတ်လျှင်\" (else) ကို \"အကယ်၍\"/\"သို့မဟုတ်\" branch အားလုံးအပြီးမှာသာ တစ်ခုတည်း ထားနိုင်ပါသည်။",
        line
    )
}

fn while_missing_condition_err(line: usize) -> String {
    format!(
        "လိုင်း {} တွင် \"အခြေအနေ\" အတွက် condition (စည်းကမ်းချက်) လိုအပ်ပါသည်။",
        line
    )
}

fn while_condition_syntax_err(line: usize) -> String {
    format!(
        "လိုင်း {} တွင် condition ရေးသားပုံ မှားနေပါသည်။ အသုံးပြုပုံ — အခြေအနေ (<value> <တန်ဖိုးနှိုင်းယှဉ်ခြင်း> <value>) (နှင့်|သို့) (...) ဖြစ်နေစဉ်",
        line
    )
}

fn while_missing_then_err(line: usize) -> String {
    format!(
        "လိုင်း {} တွင် condition အပြီးမှာ \"ဖြစ်နေစဉ်\" သို့မဟုတ် \"မဖြစ်နေစဉ်\" လိုအပ်ပါသည်။",
        line
    )
}

fn func_missing_name_err(line: usize) -> String {
    format!(
        "လိုင်း {} တွင် \"လုပ်ငန်း\" (function) အတွက် အမည် လိုအပ်ပါသည်။ အသုံးပြုပုံ — လုပ်ငန်း <fn name> အတွက် <parameter> ဖြင့်",
        line
    )
}

fn func_bad_param_err(line: usize) -> String {
    format!(
        "လိုင်း {} တွင် \"လုပ်ငန်း\" ၏ parameter ရေးသားပုံ မှားနေပါသည်။ အသုံးပြုပုံ — လုပ်ငန်း <fn name> အတွက် <parameter> ဖြင့်",
        line
    )
}

fn func_missing_by_err(line: usize) -> String {
    format!(
        "လိုင်း {} တွင် \"လုပ်ငန်း\" ၏ header အပြီးမှာ \"ဖြင့်\" (parameter ပါလျှင်) သို့မဟုတ် \"သည်\" (parameter မပါလျှင်) လိုအပ်ပါသည်။",
        line
    )
}

fn func_call_bad_name_err(line: usize) -> String {
    format!(
        "လိုင်း {} တွင် ခေါ်လိုသော function ၏ အမည် မှားနေပါသည်။",
        line
    )
}

fn func_call_missing_particle_err(line: usize) -> String {
    format!(
        "လိုင်း {} တွင် function ခေါ်ရန် \"<fn name> ကို လုပ်ပါ။\" သို့မဟုတ် \"<fn name> ကို လုပ်ရန် <argument> ဖြင့်\" ပုံစံဖြင့် ရေးရပါမည်။",
        line
    )
}

fn func_call_missing_by_err(line: usize) -> String {
    format!(
        "လိုင်း {} တွင် \"လုပ်ရန်\" ၏ argument အပြီးမှာ \"ဖြင့်\" လိုအပ်ပါသည်။",
        line
    )
}

fn func_call_missing_arg_err(line: usize) -> String {
    format!(
        "လိုင်း {} တွင် \"လုပ်ရန်\" ရန် argument (value) လိုအပ်ပါသည်။",
        line
    )
}

fn class_missing_name_err(line: usize) -> String {
    format!(
        "လိုင်း {} တွင် \"နည်းလမ်း\" (class) အတွက် အမည် လိုအပ်ပါသည်။ အသုံးပြုပုံ — နည်းလမ်း <class name>။",
        line
    )
}

fn class_missing_period_err(line: usize) -> String {
    format!(
        "လိုင်း {} တွင် \"နည်းလမ်း <class name>\" ၏ အပြီးမှာ \"သည်\" သို့မဟုတ် '။' လိုအပ်ပါသည်။",
        line
    )
}

fn class_body_not_method_err(line: usize) -> String {
    format!(
        "လိုင်း {} တွင် \"နည်းလမ်း\" (class) အတွင်း \"လုပ်ငန်း\" (method) များသာ ပါဝင်နိုင်ပါသည်။",
        line
    )
}

fn class_missing_constructor_err(line: usize, name: &str) -> String {
    format!(
        "လိုင်း {} တွင် \"{}\" class သည် constructor အနေဖြင့် method (function) အနည်းဆုံး တစ်ခု လိုအပ်ပါသည်။",
        line, name
    )
}

fn new_obj_missing_class_err(line: usize) -> String {
    format!(
        "လိုင်း {} တွင် object အသစ်ဖန်တီးမည့် class ကို ရေးပုံ မှားနေပါသည်။ အသုံးပြုပုံ — <class name> (<argument>, ...)",
        line
    )
}

// ---------------------------------------------------------------------
// Bracket-aware helpers
// ---------------------------------------------------------------------

/// Given the index of an opening bracket token, find the index of its
/// matching closing bracket, validating proper nesting along the way.
fn find_close(tokens: &[Token], open_idx: usize, line: usize) -> Result<usize, String> {
    let open_char = match &tokens[open_idx].tok {
        Tok::LBracket => '[',
        Tok::LParen => '(',
        Tok::LBrace => '{',
        _ => unreachable!(),
    };
    let mut stack: Vec<char> = vec![open_char];
    let mut i = open_idx + 1;
    while i < tokens.len() {
        match &tokens[i].tok {
            Tok::LBracket => stack.push('['),
            Tok::LParen => stack.push('('),
            Tok::LBrace => stack.push('{'),
            Tok::RBracket | Tok::RParen | Tok::RBrace => {
                let expected = match &tokens[i].tok {
                    Tok::RBracket => '[',
                    Tok::RParen => '(',
                    Tok::RBrace => '{',
                    _ => unreachable!(),
                };
                match stack.pop() {
                    Some(top) if top == expected => {
                        if stack.is_empty() {
                            return Ok(i);
                        }
                    }
                    _ => return Err(mismatched_bracket_err(tokens[i].line)),
                }
            }
            _ => {}
        }
        i += 1;
    }
    Err(unclosed_bracket_err(line, open_char))
}

/// Split `tokens` at top-level (bracket-depth 0) positions matching `is_delim`.
/// A trailing empty part (from a trailing delimiter, or from a fully-empty
/// input) is dropped so `[]`/`[1,]` behave sensibly.
fn split_top_level<'a>(tokens: &'a [Token], is_delim: impl Fn(&Tok) -> bool) -> Vec<&'a [Token]> {
    let mut parts = Vec::new();
    let mut depth = 0i32;
    let mut start = 0usize;
    for (i, t) in tokens.iter().enumerate() {
        if is_open(&t.tok) {
            depth += 1;
        } else if is_close(&t.tok) {
            depth -= 1;
        } else if depth == 0 && is_delim(&t.tok) {
            parts.push(&tokens[start..i]);
            start = i + 1;
        }
    }
    parts.push(&tokens[start..]);
    if let Some(last) = parts.last() {
        if last.is_empty() {
            parts.pop();
        }
    }
    parts
}

/// Find the first top-level (bracket-depth 0) index matching `kw`.
fn find_kw_top_level(tokens: &[Token], kw: &str) -> Option<usize> {
    let mut depth = 0i32;
    for (i, t) in tokens.iter().enumerate() {
        if is_open(&t.tok) {
            depth += 1;
        } else if is_close(&t.tok) {
            depth -= 1;
        } else if depth == 0 && ident_eq(&t.tok, kw) {
            return Some(i);
        }
    }
    None
}

fn find_kw(tokens: &[Token], kw: &str) -> Option<usize> {
    find_kw_top_level(tokens, kw)
}

fn has_end(tokens: &[Token]) -> bool {
    matches!(tokens.last().map(|t| &t.tok), Some(Tok::End))
}

// ---------------------------------------------------------------------
// Expression parsing (cursor based, supports collection literals)
// ---------------------------------------------------------------------

/// Parse a complete expression from a token slice; errors if any tokens
/// are left over after the expression.
fn parse_expr(tokens: &[Token], line: usize) -> Result<Expr, String> {
    if tokens.is_empty() {
        return Err(generic_syntax_err(line));
    }
    let (expr, pos) = parse_expr_at(tokens, 0, line)?;
    if pos != tokens.len() {
        return Err(generic_syntax_err(line));
    }
    Ok(expr)
}

fn parse_expr_at(tokens: &[Token], pos: usize, line: usize) -> Result<(Expr, usize), String> {
    let (mut expr, mut i) = parse_term_at(tokens, pos, line)?;
    while i < tokens.len() {
        if let Tok::Op(c) = &tokens[i].tok {
            let op_line = tokens[i].line;
            let (rhs, ni) = parse_term_at(tokens, i + 1, line)?;
            expr = Expr::Binary(Box::new(expr), *c, Box::new(rhs), op_line);
            i = ni;
        } else {
            break;
        }
    }
    Ok((expr, i))
}

fn parse_term_at(tokens: &[Token], pos: usize, line: usize) -> Result<(Expr, usize), String> {
    let (mut expr, mut i) = parse_primary_at(tokens, pos, line)?;
    while i < tokens.len() && matches!(tokens[i].tok, Tok::LBracket) {
        let close = find_close(tokens, i, tokens[i].line)?;
        let inner = &tokens[i + 1..close];
        if inner.is_empty() {
            return Err(generic_syntax_err(line));
        }
        let parts = split_top_level(inner, |t| matches!(t, Tok::Comma));
        let mut indices = Vec::with_capacity(parts.len());
        for p in parts {
            if p.is_empty() {
                return Err(generic_syntax_err(line));
            }
            indices.push(parse_expr(p, line)?);
        }
        expr = Expr::Index(Box::new(expr), indices);
        i = close + 1;
    }
    Ok((expr, i))
}

fn parse_primary_at(tokens: &[Token], pos: usize, line: usize) -> Result<(Expr, usize), String> {
    if pos >= tokens.len() {
        return Err(generic_syntax_err(line));
    }
    match &tokens[pos].tok {
        Tok::Op('-') => {
            let op_line = tokens[pos].line;
            let (inner, next) = parse_term_at(tokens, pos + 1, line)?;
            Ok((Expr::Neg(Box::new(inner), op_line), next))
        }
        Tok::Num(s) => Ok((Expr::NumLit(s.clone()), pos + 1)),
        Tok::Str(s) => Ok((Expr::StrLit(s.clone()), pos + 1)),
        Tok::Ident(s) => match s.as_str() {
            "True" => Ok((Expr::BoolLit(true), pos + 1)),
            "False" => Ok((Expr::BoolLit(false), pos + 1)),
            "မှန်" => Ok((Expr::BoolLit(true), pos + 1)),
            "မှား" => Ok((Expr::BoolLit(false), pos + 1)),
            _ => {
                // Object instantiation. Two accepted orderings for the
                // optional "အသစ်" (new) marker, since both are seen in the
                // wild: "<ClassName> အသစ် (args)" and "<ClassName>(args) အသစ်".
                let mut next = pos + 1;
                if next < tokens.len() && ident_eq(&tokens[next].tok, KW_NEW) {
                    next += 1;
                }
                if next < tokens.len() && matches!(tokens[next].tok, Tok::LParen) {
                    let close = find_close(tokens, next, tokens[next].line)?;
                    let inner = &tokens[next + 1..close];
                    let parts = split_top_level(inner, |t| matches!(t, Tok::Comma));
                    let mut args = Vec::with_capacity(parts.len());
                    for p in parts {
                        args.push(parse_expr(p, line)?);
                    }
                    let mut after = close + 1;
                    if after < tokens.len() && ident_eq(&tokens[after].tok, KW_NEW) {
                        after += 1;
                    }
                    return Ok((Expr::NewObj(s.clone(), args, tokens[pos].line), after));
                }
                Ok((Expr::Ident(s.clone()), pos + 1))
            }
        },
        Tok::LBracket => {
            let close = find_close(tokens, pos, tokens[pos].line)?;
            let inner = &tokens[pos + 1..close];
            let parts = split_top_level(inner, |t| matches!(t, Tok::Comma));
            let mut elems = Vec::with_capacity(parts.len());
            for p in parts {
                elems.push(parse_expr(p, line)?);
            }
            Ok((Expr::ListLit(elems), close + 1))
        }
        Tok::LParen => {
            let close = find_close(tokens, pos, tokens[pos].line)?;
            let inner = &tokens[pos + 1..close];
            let parts = split_top_level(inner, |t| matches!(t, Tok::Comma));
            let mut elems = Vec::with_capacity(parts.len());
            for p in parts {
                elems.push(parse_expr(p, line)?);
            }
            Ok((Expr::TupleLit(elems), close + 1))
        }
        Tok::LBrace => {
            let close = find_close(tokens, pos, tokens[pos].line)?;
            let inner = &tokens[pos + 1..close];
            if inner.iter().any(|t| matches!(t.tok, Tok::End)) {
                // Dict: entries separated by "။", each "<key> သည် <value> ဖြစ်၏"
                let entries = split_top_level(inner, |t| matches!(t, Tok::End));
                let mut pairs = Vec::with_capacity(entries.len());
                for e in entries {
                    let is_idx =
                        find_kw_top_level(e, KW_IS).ok_or_else(|| dict_entry_err(line))?;
                    if is_idx + 1 != e.len() {
                        return Err(dict_entry_err(line));
                    }
                    let body_e = &e[..is_idx];
                    let assign_idx = find_kw_top_level(body_e, KW_ASSIGN)
                        .ok_or_else(|| dict_entry_err(line))?;
                    let key = parse_expr(&body_e[..assign_idx], line)?;
                    let value = parse_expr(&body_e[assign_idx + 1..], line)?;
                    pairs.push((key, value));
                }
                Ok((Expr::DictLit(pairs), close + 1))
            } else {
                // Set: comma-separated elements
                let parts = split_top_level(inner, |t| matches!(t, Tok::Comma));
                let mut elems = Vec::with_capacity(parts.len());
                for p in parts {
                    elems.push(parse_expr(p, line)?);
                }
                Ok((Expr::SetLit(elems), close + 1))
            }
        }
        _ => Err(generic_syntax_err(line)),
    }
}

/// Parse "value ကို <FUNC>" shaped statements (print / input-no-assign),
/// where `body` excludes the trailing End token and the function keyword itself.
fn parse_particle_call(
    body: &[Token],
    fn_idx: usize,
    fname: &str,
    line: usize,
) -> Result<Expr, String> {
    let before = &body[..fn_idx];
    if let Some(last) = before.last() {
        if ident_eq(&last.tok, KW_PARTICLE) {
            let value_tokens = &before[..before.len() - 1];
            if value_tokens.is_empty() {
                return Err(fn_missing_value_err(line, fname));
            }
            return parse_expr(value_tokens, line);
        }
    }
    if before.is_empty() {
        return Err(fn_missing_value_err(line, fname));
    }
    Err(fn_missing_particle_err(line, fname))
}

// ---------------------------------------------------------------------
// Statement splitting: '။' at bracket-depth 0 ends a statement.
// ---------------------------------------------------------------------

fn split_statements(tokens: &[Token]) -> Vec<(Vec<Token>, usize)> {
    let mut stmts = Vec::new();
    let mut depth = 0i32;
    let mut start = 0usize;
    for (i, t) in tokens.iter().enumerate() {
        if is_open(&t.tok) {
            depth += 1;
        } else if is_close(&t.tok) {
            depth -= 1;
        } else if ident_eq(&t.tok, KW_LOOP_EACH) || ident_eq(&t.tok, KW_IF) || ident_eq(&t.tok, KW_WHILE) || ident_eq(&t.tok, KW_FUNC_DEF) || ident_eq(&t.tok, KW_CLASS_DEF) {
            // Opens a for-loop, if/else, while, or function-definition block:
            // its header has no terminating '။' of its own, so treat it as a
            // nesting level too, just like brackets, so interior statement-
            // terminating '။' tokens don't prematurely close the outer
            // chunk. All block kinds share the same closing keyword "ပြီး".
            depth += 1;
        } else if ident_eq(&t.tok, KW_LOOP_END) {
            depth -= 1;
        } else if depth == 0 && matches!(t.tok, Tok::End) {
            let slice = &tokens[start..=i];
            let line = slice[0].line;
            stmts.push((slice.to_vec(), line));
            start = i + 1;
        }
    }
    if start < tokens.len() {
        let slice = &tokens[start..];
        let line = slice[0].line;
        stmts.push((slice.to_vec(), line));
    }
    stmts
}

/// Same nesting rules as `split_statements` (brackets + for/if block
/// keywords), but used to locate the first top-level occurrence of one of
/// several target keywords within an if-statement's tokens (branch
/// separators / condition terminators). Returns (index, which kws entry).
fn find_first_at_block_level(tokens: &[Token], kws: &[&str]) -> Option<(usize, usize)> {
    let mut depth = 0i32;
    for (i, t) in tokens.iter().enumerate() {
        if is_open(&t.tok) {
            depth += 1;
            continue;
        }
        if is_close(&t.tok) {
            depth -= 1;
            continue;
        }
        if ident_eq(&t.tok, KW_LOOP_EACH) || ident_eq(&t.tok, KW_IF) || ident_eq(&t.tok, KW_WHILE) || ident_eq(&t.tok, KW_FUNC_DEF) || ident_eq(&t.tok, KW_CLASS_DEF) {
            depth += 1;
            continue;
        }
        if ident_eq(&t.tok, KW_LOOP_END) {
            depth -= 1;
            continue;
        }
        if depth == 0 {
            if let Tok::Ident(s) = &t.tok {
                for (ki, kw) in kws.iter().enumerate() {
                    if s == kw {
                        return Some((i, ki));
                    }
                }
            }
        }
    }
    None
}

fn parse_stmt(tokens: &[Token], line: usize) -> Result<Stmt, String> {
    let end_present = has_end(tokens);
    let mut body: Vec<Token> = if end_present {
        tokens[..tokens.len() - 1].to_vec()
    } else {
        tokens.to_vec()
    };

    // --- For-loop / if-else / while block: header (no '။' of its own) ... body ... ပြီး။ ---
    if end_present {
        if let Some(last) = body.last() {
            if ident_eq(&last.tok, KW_LOOP_END) {
                body.pop();
                if !body.is_empty() && ident_eq(&body[0].tok, KW_IF) {
                    let inner = body[1..].to_vec();
                    return parse_if_statement(&inner, line);
                }
                if !body.is_empty() && ident_eq(&body[0].tok, KW_WHILE) {
                    let inner = body[1..].to_vec();
                    return parse_while_statement(&inner, line);
                }
                if !body.is_empty() && ident_eq(&body[0].tok, KW_FUNC_DEF) {
                    let inner = body[1..].to_vec();
                    return parse_func_def(&inner, line);
                }
                if !body.is_empty() && ident_eq(&body[0].tok, KW_CLASS_DEF) {
                    let inner = body[1..].to_vec();
                    return parse_class_def(&inner, line);
                }
                return parse_for_loop(&body, line);
            }
        }
    }

    // --- Function call with argument: <fn name> ကို လုပ်ရန် <argument> ဖြင့်။ ---
    if let Some(call_idx) = find_kw(&body, KW_CALL_WITH) {
        if body.is_empty() || !matches!(body[0].tok, Tok::Ident(_)) {
            return Err(func_call_bad_name_err(line));
        }
        let fn_name = match &body[0].tok {
            Tok::Ident(s) => s.clone(),
            _ => unreachable!(),
        };
        if call_idx != 2 || !ident_eq(&body[1].tok, KW_PARTICLE) {
            return Err(func_call_missing_particle_err(line));
        }
        let after_call = &body[call_idx + 1..];
        let by_idx = find_kw_top_level(after_call, KW_BY)
            .ok_or_else(|| func_call_missing_by_err(line))?;
        let arg_tokens = &after_call[..by_idx];
        if arg_tokens.is_empty() {
            return Err(func_call_missing_arg_err(line));
        }
        let arg = parse_expr(arg_tokens, line)?;
        if !end_present {
            return Err(missing_period_err(line));
        }
        if by_idx + 1 != after_call.len() {
            return Err(generic_syntax_err(line));
        }
        return Ok(Stmt::FuncCall {
            name: fn_name,
            arg: Some(arg),
            line,
        });
    }

    // --- Function call (no argument): <fn name> ကို လုပ်ပါ။ ---
    if let Some(call_idx) = find_kw(&body, KW_CALL) {
        if body.is_empty() || !matches!(body[0].tok, Tok::Ident(_)) {
            return Err(func_call_bad_name_err(line));
        }
        let fn_name = match &body[0].tok {
            Tok::Ident(s) => s.clone(),
            _ => unreachable!(),
        };
        if call_idx != 2 || !ident_eq(&body[1].tok, KW_PARTICLE) {
            return Err(func_call_missing_particle_err(line));
        }
        if !end_present {
            return Err(missing_period_err(line));
        }
        if call_idx + 1 != body.len() {
            return Err(generic_syntax_err(line));
        }
        return Ok(Stmt::FuncCall {
            name: fn_name,
            arg: None,
            line,
        });
    }

    // --- Print statement: ... ကို ဖော်ပြပါ။ ---
    if let Some(idx) = find_kw(&body, KW_PRINT) {
        let value = parse_particle_call(&body, idx, KW_PRINT, line)?;
        if !end_present {
            return Err(missing_period_err(line));
        }
        if idx + 1 != body.len() {
            return Err(generic_syntax_err(line));
        }
        return Ok(Stmt::Print { value, line });
    }

    // --- Input statements: ... ကို မေးပါ။  OR  name အတွက် ... ကို မေးပါ။ ---
    if let Some(idx) = find_kw(&body, KW_INPUT) {
        if let Some(for_idx) = find_kw(&body, KW_FOR) {
            if for_idx == 0 {
                return Err(generic_syntax_err(line));
            }
            let name = match &body[0].tok {
                Tok::Ident(s) => s.clone(),
                _ => return Err(generic_syntax_err(line)),
            };
            let rest = &body[for_idx + 1..];
            let inp_idx_in_rest = find_kw(rest, KW_INPUT).ok_or_else(|| generic_syntax_err(line))?;
            let value = parse_particle_call(rest, inp_idx_in_rest, KW_INPUT, line)?;
            if !end_present {
                return Err(missing_period_err(line));
            }
            if inp_idx_in_rest + 1 != rest.len() {
                return Err(generic_syntax_err(line));
            }
            return Ok(Stmt::InputAssign { name, value, line });
        } else {
            let value = parse_particle_call(&body, idx, KW_INPUT, line)?;
            if !end_present {
                return Err(missing_period_err(line));
            }
            if idx + 1 != body.len() {
                return Err(generic_syntax_err(line));
            }
            return Ok(Stmt::InputNoAssign { value, line });
        }
    }

    // --- Convert statement (standalone): value ကို <type> သို့ ပြောင်းပါ။ ---
    if let Some(conv_idx) = find_kw(&body, KW_CONVERT) {
        if conv_idx == 0 || !ident_eq(&body[conv_idx - 1].tok, KW_TO) {
            return Err(generic_syntax_err(line));
        }
        let to_idx = conv_idx - 1;
        if to_idx == 0 {
            return Err(convert_missing_type_err(line));
        }
        let type_idx = to_idx - 1;

        if ident_eq(&body[type_idx].tok, KW_PARTICLE) {
            return Err(convert_missing_type_err(line));
        }

        let type_name = match &body[type_idx].tok {
            Tok::Ident(s) if is_type_keyword(s) => s.clone(),
            _ => return Err(convert_missing_type_err(line)),
        };

        if type_idx == 0 || !ident_eq(&body[type_idx - 1].tok, KW_PARTICLE) {
            return Err(convert_missing_value_err(line));
        }
        let ko_idx = type_idx - 1;
        let value_tokens = &body[..ko_idx];
        if value_tokens.is_empty() {
            return Err(convert_missing_value_err(line));
        }
        let value = parse_expr(value_tokens, line)?;
        if !end_present {
            return Err(missing_period_err(line));
        }
        if conv_idx + 1 != body.len() {
            return Err(generic_syntax_err(line));
        }
        return Ok(Stmt::ConvertStmt {
            value,
            target_type: type_name,
            line,
        });
    }

    // --- Math assignment: <var/value> ကို <amount> <fn>ပါ။  OR  <var> အတွက် <amount> <fn>ပါ။ ---
    let math_fn_idx = body
        .iter()
        .position(|t| matches!(&t.tok, Tok::Ident(s) if math_fn_op(s).is_some()));
    if let Some(fn_idx) = math_fn_idx {
        let fname = match &body[fn_idx].tok {
            Tok::Ident(s) => s.clone(),
            _ => unreachable!(),
        };
        let op = math_fn_op(&fname).unwrap();

        let amount_idx = if fn_idx == 0 { None } else { Some(fn_idx - 1) };
        let amount_is_num = amount_idx
            .map(|i| matches!(&body[i].tok, Tok::Num(_)))
            .unwrap_or(false);
        let amount_is_neg_num = fn_idx >= 2
            && matches!(&body[fn_idx - 1].tok, Tok::Num(_))
            && matches!(&body[fn_idx - 2].tok, Tok::Op('-'));

        if !amount_is_num && !amount_is_neg_num {
            return Err(math_missing_value_err(line, &fname));
        }
        let amount_start = if amount_is_neg_num { fn_idx - 2 } else { fn_idx - 1 };
        let (amount, _) = parse_term_at(&body, amount_start, line)?;

        if amount_start == 0 {
            return Err(generic_syntax_err(line));
        }
        let particle_idx = amount_start - 1;

        let target = if ident_eq(&body[particle_idx].tok, KW_FOR) {
            if particle_idx != 1 {
                return Err(generic_syntax_err(line));
            }
            match &body[0].tok {
                Tok::Ident(s) => Expr::Ident(s.clone()),
                _ => return Err(generic_syntax_err(line)),
            }
        } else if ident_eq(&body[particle_idx].tok, KW_PARTICLE) {
            let target_tokens = &body[..particle_idx];
            if target_tokens.is_empty() {
                return Err(generic_syntax_err(line));
            }
            parse_expr(target_tokens, line)?
        } else {
            return Err(generic_syntax_err(line));
        };

        if !end_present {
            return Err(missing_period_err(line));
        }
        if fn_idx + 1 != body.len() {
            return Err(generic_syntax_err(line));
        }

        return Ok(Stmt::MathAssign {
            target,
            op,
            amount,
            line,
        });
    }

    // --- Self field assignment: တန်ဖိုး <field> သည် <value> ဖြစ်၏။ (inside a class method) ---
    if !body.is_empty() && ident_eq(&body[0].tok, KW_SELF) {
        if body.len() < 2 {
            return Err(generic_syntax_err(line));
        }
        let field_name = match &body[1].tok {
            Tok::Ident(s) => s.clone(),
            _ => return Err(generic_syntax_err(line)),
        };
        if body.len() < 3 || !ident_eq(&body[2].tok, KW_ASSIGN) {
            return Err(format!(
                "လိုင်း {} တွင် {} {} ကို တန်ဖိုးသတ်မှတ်ရာမှာ 'သည်' လိုအပ်ပါသည်။",
                line, KW_SELF, field_name
            ));
        }
        let is_idx = find_kw(&body, KW_IS).ok_or_else(|| missing_period_err(line))?;
        let value_tokens = &body[3..is_idx];
        if value_tokens.is_empty() {
            return Err(format!(
                "လိုင်း {} တွင် {} {} သည် တန်ဖိုးသတ်မှတ်ထားခြင်းမရှိပါ။",
                line, KW_SELF, field_name
            ));
        }
        if !end_present {
            return Err(missing_period_err(line));
        }
        if is_idx + 1 != body.len() {
            return Err(generic_syntax_err(line));
        }
        let value = parse_expr(value_tokens, line)?;
        return Ok(Stmt::SelfFieldSet {
            field: field_name,
            value,
            line,
        });
    }

    // --- Variable / collection declaration: name (သည်|မှာ) value ဖြစ်၏။ ---
    if let Some(is_idx) = find_kw(&body, KW_IS) {
        if body.is_empty() {
            return Err(generic_syntax_err(line));
        }
        let name = match &body[0].tok {
            Tok::Ident(s) => s.clone(),
            _ => return Err(generic_syntax_err(line)),
        };
        let assign_ok = body.len() >= 2
            && (ident_eq(&body[1].tok, KW_ASSIGN) || ident_eq(&body[1].tok, KW_ASSIGN_COLLECTION));
        if !assign_ok {
            return Err(format!(
                "လိုင်း {} တွင် တန်ဖိုးသတ်မှတ်ရာမှာ 'သည်' လိုအပ်ပါသည်။",
                line
            ));
        }
        let value_tokens = &body[2..is_idx];
        if value_tokens.is_empty() {
            return Err(format!(
                "လိုင်း {} တွင် {} သည် တန်ဖိုးသတ်မှတ်ထားခြင်းမရှိပါ။",
                line, name
            ));
        }
        if !end_present {
            return Err(missing_period_err(line));
        }
        if is_idx + 1 != body.len() {
            return Err(generic_syntax_err(line));
        }
        let value = parse_expr(value_tokens, line)?;
        return Ok(Stmt::VarDecl { name, value, line });
    }

    // --- Bare expression statement (e.g. "10 + \"10\"") ---
    if body.iter().any(|t| matches!(t.tok, Tok::Op(_))) {
        let value = parse_expr(&body, line)?;
        return Ok(Stmt::ExprStmt { value, line });
    }

    Err(generic_syntax_err(line))
}

/// Parse a for-loop's inner tokens: the header (variable, source, optional
/// step+fn) followed by the loop body's own statement tokens. The trailing
/// "ပြီး" marker has already been stripped by the caller.
fn parse_for_loop(inner: &[Token], line: usize) -> Result<Stmt, String> {
    if inner.len() < 2 {
        return Err(generic_syntax_err(line));
    }
    let var_name = match &inner[0].tok {
        Tok::Ident(s) => s.clone(),
        _ => return Err(generic_syntax_err(line)),
    };
    if !ident_eq(&inner[1].tok, KW_ASSIGN) {
        return Err(generic_syntax_err(line));
    }

    let htm_idx =
        find_kw_top_level(inner, KW_LOOP_FROM).ok_or_else(|| generic_syntax_err(line))?;
    let source_tokens = &inner[2..htm_idx];
    if source_tokens.is_empty() {
        return Err(generic_syntax_err(line));
    }

    let tcs_idx = htm_idx + 1;
    if tcs_idx >= inner.len() || !ident_eq(&inner[tcs_idx].tok, KW_LOOP_EACH) {
        return Err(generic_syntax_err(line));
    }

    // Optionally, a range step + math-fn immediately follows "တစ်ခုစီ"
    // (e.g. "1 တိုးခြင်းဖြင့်"). If it's not there, this is a plain
    // collection-iteration loop and the body starts right away.
    let mut cursor = tcs_idx + 1;
    let mut range_step: Option<(Expr, char)> = None;
    if cursor < inner.len()
        && (matches!(inner[cursor].tok, Tok::Num(_)) || matches!(inner[cursor].tok, Tok::Op('-')))
    {
        if let Ok((step_expr, next)) = parse_term_at(inner, cursor, line) {
            if next < inner.len() {
                if let Tok::Ident(s) = &inner[next].tok {
                    if let Some(op) = loop_fn_op(s) {
                        range_step = Some((step_expr, op));
                        cursor = next + 1;
                    }
                }
            }
        }
    }

    let body_tokens = &inner[cursor..];
    let mut body_stmts = Vec::new();
    for (chunk, cline) in split_statements(body_tokens) {
        body_stmts.push(parse_stmt(&chunk, cline)?);
    }

    let source = if let Some((step, op)) = range_step {
        let parts = split_top_level(source_tokens, |t| matches!(t, Tok::Comma));
        let (start, end) = match parts.len() {
            1 => (None, parse_expr(parts[0], line)?),
            2 => (
                Some(parse_expr(parts[0], line)?),
                parse_expr(parts[1], line)?,
            ),
            _ => return Err(generic_syntax_err(line)),
        };
        ForSource::Range {
            start,
            end,
            step,
            op,
        }
    } else {
        ForSource::Auto(parse_expr(source_tokens, line)?)
    };

    Ok(Stmt::ForLoop {
        var_name,
        source,
        body: body_stmts,
        line,
    })
}

fn parse_block(tokens: &[Token]) -> Result<Vec<Stmt>, String> {
    let mut stmts = Vec::new();
    for (chunk, cline) in split_statements(tokens) {
        stmts.push(parse_stmt(&chunk, cline)?);
    }
    Ok(stmts)
}

/// Recognize a type-check target name from the tokens after "သည်" inside a
/// condition group, e.g. "ကိန်း". The bool keyword "မှန်/မှား" contains a
/// literal '/', which the lexer tokenizes as a division operator, so it
/// actually arrives as three tokens: Ident("မှန်"), Op('/'), Ident("မှား").
fn parse_typecheck_name(tokens: &[Token]) -> Option<String> {
    if tokens.len() == 3 {
        if let (Tok::Ident(a), Tok::Op('/'), Tok::Ident(b)) =
            (&tokens[0].tok, &tokens[1].tok, &tokens[2].tok)
        {
            if a == "မှန်" && b == "မှား" {
                return Some(TC_BOOL.to_string());
            }
        }
    }
    if tokens.len() == 1 {
        if let Tok::Ident(s) = &tokens[0].tok {
            if matches!(s.as_str(), TC_STR | TC_NUM | TC_FLOAT) {
                return Some(s.clone());
            }
        }
    }
    None
}

/// Parse a single parenthesized condition group's inner tokens. Three forms,
/// checked in this order:
///   1. Type-check: "<expr> သည် <type>", e.g. "x သည် ကိန်း".
///   2. Comparison: "<expr> <cmp-op> <expr>", e.g. "x == 10".
///   3. Bare boolean-valued expression, e.g. "အလုပ်" (test its truthiness).
fn parse_cond_atom(tokens: &[Token], line: usize) -> Result<CondAtom, String> {
    if let Some(is_idx) = find_kw_top_level(tokens, KW_ASSIGN) {
        if is_idx == 0 {
            return Err(if_condition_syntax_err(line));
        }
        let lhs = parse_expr(&tokens[..is_idx], line)?;
        let type_tokens = &tokens[is_idx + 1..];
        let type_name = parse_typecheck_name(type_tokens)
            .ok_or_else(|| if_condition_syntax_err(line))?;
        return Ok(CondAtom {
            lhs,
            op: None,
            rhs: None,
            type_check: Some(type_name),
            line,
        });
    }

    let cmp_idx = tokens.iter().position(|t| matches!(t.tok, Tok::Cmp(_)));
    match cmp_idx {
        Some(idx) => {
            if idx == 0 || idx + 1 >= tokens.len() {
                return Err(if_condition_syntax_err(line));
            }
            let lhs = parse_expr(&tokens[..idx], line)?;
            let op = match &tokens[idx].tok {
                Tok::Cmp(s) => s.clone(),
                _ => unreachable!(),
            };
            let rhs = parse_expr(&tokens[idx + 1..], line)?;
            Ok(CondAtom {
                lhs,
                op: Some(op),
                rhs: Some(rhs),
                type_check: None,
                line,
            })
        }
        None => {
            // No comparison operator at all: treat the whole group as a
            // single boolean-valued expression, e.g. "(အလုပ်)".
            let lhs = parse_expr(tokens, line)?;
            Ok(CondAtom {
                lhs,
                op: None,
                rhs: None,
                type_check: None,
                line,
            })
        }
    }
}

/// Parse a sequence of parenthesized conditions and parenthesized logical
/// operators, e.g. "(x == 10) (နှင့်) (y == 20)". `is_while` selects which
/// set of error messages to use (if/elif vs while wording).
fn parse_cond_chain(tokens: &[Token], line: usize, is_while: bool) -> Result<CondChain, String> {
    let missing_err = |line| {
        if is_while {
            while_missing_condition_err(line)
        } else {
            if_missing_condition_err(line)
        }
    };
    let syntax_err = |line| {
        if is_while {
            while_condition_syntax_err(line)
        } else {
            if_condition_syntax_err(line)
        }
    };

    if tokens.is_empty() {
        return Err(missing_err(line));
    }
    let mut idx = 0usize;
    let mut atoms: Vec<(Option<LogicalOp>, CondAtom)> = Vec::new();
    let mut pending_op: Option<LogicalOp> = None;

    while idx < tokens.len() {
        if !matches!(tokens[idx].tok, Tok::LParen) {
            return Err(syntax_err(line));
        }
        let close = find_close(tokens, idx, line)?;
        let group = &tokens[idx + 1..close];

        let is_and = group.len() == 1 && ident_eq(&group[0].tok, KW_AND);
        let is_or = group.len() == 1 && ident_eq(&group[0].tok, KW_OR);

        if is_and || is_or {
            if pending_op.is_some() || atoms.is_empty() {
                return Err(syntax_err(line));
            }
            pending_op = Some(if is_and { LogicalOp::And } else { LogicalOp::Or });
        } else {
            if !atoms.is_empty() && pending_op.is_none() {
                return Err(syntax_err(line));
            }
            let atom = parse_cond_atom(group, line)?;
            atoms.push((pending_op.take(), atom));
        }
        idx = close + 1;
    }

    if pending_op.is_some() || atoms.is_empty() {
        return Err(syntax_err(line));
    }

    let first = atoms[0].1.clone();
    let rest = atoms[1..]
        .iter()
        .map(|(op, atom)| (op.clone().unwrap(), atom.clone()))
        .collect();
    Ok(CondChain { first, rest })
}

/// Parse an if/elif/else chain. `inner` is everything between the leading
/// "အကယ်၍" (already stripped by the caller) and the trailing "ပြီး" (also
/// already stripped).
fn parse_if_statement(inner: &[Token], line: usize) -> Result<Stmt, String> {
    let mut branches: Vec<IfBranch> = Vec::new();
    let mut cursor = 0usize;
    let mut seen_else = false;

    loop {
        if !branches.is_empty() {
            if cursor >= inner.len() {
                break;
            }
            if seen_else {
                return Err(if_else_not_last_err(line));
            }
            if ident_eq(&inner[cursor].tok, KW_ELSE) {
                seen_else = true;
                cursor += 1;
                let body_stmts = parse_block(&inner[cursor..])?;
                branches.push(IfBranch {
                    cond: None,
                    negate: false,
                    body: body_stmts,
                });
                cursor = inner.len();
                break;
            } else if ident_eq(&inner[cursor].tok, KW_ELIF) {
                cursor += 1;
            } else {
                return Err(generic_syntax_err(line));
            }
        } else if cursor >= inner.len() {
            return Err(if_missing_condition_err(line));
        }

        // Parse condition-chain, terminated by "ဖြစ်လျှင်" (positive) or
        // "မဖြစ်လျှင်" (negative).
        let (term_rel, which) =
            find_first_at_block_level(&inner[cursor..], &[KW_THEN_POS, KW_THEN_NEG])
                .ok_or_else(|| if_missing_then_err(line))?;
        let term_abs = cursor + term_rel;
        let negate = which == 1;
        let cond_tokens = &inner[cursor..term_abs];
        let cond = parse_cond_chain(cond_tokens, line, false)?;
        cursor = term_abs + 1;

        // Body runs until the next branch keyword at this same level, or to
        // the end of the whole if-statement.
        let body_end = match find_first_at_block_level(&inner[cursor..], &[KW_ELIF, KW_ELSE]) {
            Some((rel, _)) => cursor + rel,
            None => inner.len(),
        };
        let body_stmts = parse_block(&inner[cursor..body_end])?;
        branches.push(IfBranch {
            cond: Some(cond),
            negate,
            body: body_stmts,
        });
        cursor = body_end;
    }

    if branches.is_empty() {
        return Err(if_missing_condition_err(line));
    }

    Ok(Stmt::If { branches, line })
}

/// Parse a while-loop. `inner` is everything between the leading "အခြေအနေ"
/// (already stripped by the caller) and the trailing "ပြီး" (also already
/// stripped): "(<condition...>) ဖြစ်နေစဉ်/မဖြစ်နေစဉ် <body...>".
fn parse_while_statement(inner: &[Token], line: usize) -> Result<Stmt, String> {
    if inner.is_empty() {
        return Err(while_missing_condition_err(line));
    }

    let (term_idx, which) = find_first_at_block_level(inner, &[KW_WHILE_POS, KW_WHILE_NEG])
        .ok_or_else(|| while_missing_then_err(line))?;
    let negate = which == 1;
    let cond_tokens = &inner[..term_idx];
    let cond = parse_cond_chain(cond_tokens, line, true)?;

    let body_tokens = &inner[term_idx + 1..];
    let body = parse_block(body_tokens)?;

    Ok(Stmt::While {
        cond,
        negate,
        body,
        line,
    })
}

/// Parse a function definition. `inner` is everything between the leading
/// "လုပ်ငန်း" (already stripped by the caller) and the trailing "ပြီး" (also
/// already stripped): "<fn name> [အတွက် <param1>, <param2>, ...] ဖြင့် <body...>".
/// Parse a function definition. `inner` is everything between the leading
/// "လုပ်ငန်း" (already stripped by the caller) and the trailing "ပြီး" (also
/// already stripped). Two header forms:
///   - no-argument short form: "<fn name> သည် <body...>"
///   - with parameters:        "<fn name> [အတွက် <param1>, ...] ဖြင့် <body...>"
fn parse_func_def(inner: &[Token], line: usize) -> Result<Stmt, String> {
    if inner.is_empty() {
        return Err(func_missing_name_err(line));
    }
    let name = match &inner[0].tok {
        Tok::Ident(s) => s.clone(),
        _ => return Err(func_missing_name_err(line)),
    };

    let rest = &inner[1..];

    // No-argument short form: "<fn name> သည် <body...>"
    if !rest.is_empty() && ident_eq(&rest[0].tok, KW_ASSIGN) {
        let body_tokens = &rest[1..];
        let body = parse_block(body_tokens)?;
        return Ok(Stmt::FuncDef {
            name,
            params: Vec::new(),
            body,
            line,
        });
    }

    let by_idx = find_kw_top_level(rest, KW_BY).ok_or_else(|| func_missing_by_err(line))?;
    let header_tokens = &rest[..by_idx];

    let params: Vec<String> = if header_tokens.is_empty() {
        Vec::new()
    } else if ident_eq(&header_tokens[0].tok, KW_FOR) {
        let name_tokens = &header_tokens[1..];
        if name_tokens.is_empty() {
            return Err(func_bad_param_err(line));
        }
        let parts = split_top_level(name_tokens, |t| matches!(t, Tok::Comma));
        let mut names = Vec::with_capacity(parts.len());
        for p in parts {
            if p.len() != 1 {
                return Err(func_bad_param_err(line));
            }
            match &p[0].tok {
                Tok::Ident(s) => names.push(s.clone()),
                _ => return Err(func_bad_param_err(line)),
            }
        }
        names
    } else {
        return Err(func_bad_param_err(line));
    };

    let body_tokens = &rest[by_idx + 1..];
    let body = parse_block(body_tokens)?;

    Ok(Stmt::FuncDef {
        name,
        params,
        body,
        line,
    })
}

/// Parse a class definition. `inner` is everything between the leading
/// "နည်းလမ်း" (already stripped by the caller) and the trailing "ပြီး" (also
/// already stripped). Two header forms are accepted:
///   - "<class name> သည် <method definitions...>"   (current/preferred)
///   - "<class name>။ <method definitions...>"       (older form)
fn parse_class_def(inner: &[Token], line: usize) -> Result<Stmt, String> {
    if inner.is_empty() {
        return Err(class_missing_name_err(line));
    }
    let name = match &inner[0].tok {
        Tok::Ident(s) => s.clone(),
        _ => return Err(class_missing_name_err(line)),
    };
    if inner.len() < 2 {
        return Err(class_missing_period_err(line));
    }
    let body_tokens = if matches!(inner[1].tok, Tok::End) || ident_eq(&inner[1].tok, KW_ASSIGN) {
        &inner[2..]
    } else {
        return Err(class_missing_period_err(line));
    };
    let body = parse_block(body_tokens)?;

    for s in &body {
        if !matches!(s, Stmt::FuncDef { .. }) {
            return Err(class_body_not_method_err(line));
        }
    }
    if body.is_empty() {
        return Err(class_missing_constructor_err(line, &name));
    }

    Ok(Stmt::ClassDef { name, body, line })
}

pub fn parse(tokens: &[Token]) -> Result<Vec<Stmt>, String> {
    let mut stmts = Vec::new();
    for (stmt_tokens, line) in split_statements(tokens) {
        stmts.push(parse_stmt(&stmt_tokens, line)?);
    }
    Ok(stmts)
}
