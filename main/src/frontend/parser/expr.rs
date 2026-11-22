use super::*;

/// The callee of a call written with its arguments first: `(<args>) <name>`,
/// the same call as `<name>(<args>)`. `close` is the index of the `)`; the
/// answer is the name and the index just past it.
///
/// A name followed by `.` is left alone: that is the library of
/// `<lib>.<fn>(...)`, which keeps its own spellings -- the same reason
/// `(1, 2) web.get` stays a syntax error instead of calling something called
/// `web`.
pub(crate) fn reversed_callee(tokens: &[Token], close: usize) -> Option<(String, usize)> {
    let name_pos = close + 1;
    let name = match tokens.get(name_pos).map(|t| &t.tok) {
        Some(Tok::Ident(name)) if !is_reserved_word(name) => name.clone(),
        _ => return None,
    };
    if matches!(tokens.get(name_pos + 1).map(|t| &t.tok), Some(Tok::Dot)) {
        return None;
    }
    Some((name, name_pos + 1))
}

/// Does this token run start a call, in either order? `name(args)` (the callee
/// first) or `(args) name` (the arguments first).
pub(crate) fn call_shaped(tokens: &[Token]) -> bool {
    match tokens.first().map(|t| &t.tok) {
        Some(Tok::Ident(_)) => matches!(tokens.get(1).map(|t| &t.tok), Some(Tok::LParen)),
        Some(Tok::LParen) => match find_close(tokens, 0, tokens[0].line) {
            Ok(close) => reversed_callee(tokens, close).is_some(),
            Err(_) => false,
        },
        _ => false,
    }
}

/// A statement that is nothing but a reversed call: `(1, 2) add` / `() now`.
/// Answers the callee and its arguments, or `None` when the parentheses are
/// something else -- a tuple in the middle of a declaration, a grouping, a
/// tuple literal on its own.
pub(crate) fn reversed_call_stmt(
    body: &[Token],
    line: usize,
) -> Result<Option<(String, Vec<Expr>)>, String> {
    if !matches!(body.first().map(|t| &t.tok), Some(Tok::LParen)) {
        return Ok(None);
    }
    let close = match find_close(body, 0, body[0].line) {
        Ok(close) => close,
        Err(_) => return Ok(None),
    };
    // Only when the name is the last token: `(1, 2) add` is a call, while
    // `(1, 2) add ကို လုပ်ပါ` is the Myanmar sentence handled further down.
    let name = match reversed_callee(body, close) {
        Some((name, after)) if after == body.len() => name,
        _ => return Ok(None),
    };
    let mut args = Vec::new();
    let inner = &body[1..close];
    if !inner.is_empty() {
        for part in split_top_level(inner, |t| matches!(t, Tok::Comma)) {
            args.push(parse_expr(part, line)?);
        }
    }
    Ok(Some((name, args)))
}

/// Parse the argument list for a function call. Accepts either a single
/// inline expression (`<arg>`), a comma-separated inline list
/// (`<arg1>, <arg2>`), a parenthesized list (`(<arg1>, <arg2>)`, the same
/// spelling library calls accept), or a multi-line brace form spanning
/// multiple lines: `{ <arg1>, <arg2>, ... }`.
pub(crate) fn parse_call_args(tokens: &[Token], line: usize) -> Result<Vec<Expr>, String> {
    if tokens.is_empty() {
        return Err(func_call_missing_arg_err(line));
    }
    let inner: &[Token] = if is_single_braced_group(tokens) {
        &tokens[1..tokens.len() - 1]
    } else if is_single_paren_group(tokens) {
        &tokens[1..tokens.len() - 1]
    } else {
        tokens
    };
    if inner.is_empty() {
        return Err(func_call_missing_arg_err(line));
    }
    let parts = split_top_level(inner, |t| matches!(t, Tok::Comma));
    let mut args = Vec::with_capacity(parts.len());
    for p in parts {
        if p.is_empty() {
            return Err(func_call_missing_arg_err(line));
        }
        args.push(parse_expr(p, line)?);
    }
    Ok(args)
}

/// Parse the argument list of a library function call. Accepts a bare
/// comma-separated list (`<arg1>, <arg2>`), a single plain argument
/// (`<arg>`), or the same wrapped in one pair of parens
/// (`(<arg>)`, `(<arg1>, <arg2>)`) -- the two spellings are documented
/// interchangeably. A single paren group is unwrapped first, so
/// `(a, b)` means two arguments rather than one tuple argument. An empty
/// list (`()`) means the call takes no arguments.
pub(crate) fn parse_lib_call_args(tokens: &[Token], line: usize) -> Result<Vec<Expr>, String> {
    if tokens.is_empty() {
        return Ok(Vec::new());
    }
    let inner: &[Token] = if is_single_paren_group(tokens) {
        &tokens[1..tokens.len() - 1]
    } else {
        tokens
    };
    if inner.is_empty() {
        return Ok(Vec::new());
    }
    let parts = split_top_level(inner, |t| matches!(t, Tok::Comma));
    let mut args = Vec::with_capacity(parts.len());
    for p in parts {
        if p.is_empty() {
            return Err(func_call_missing_arg_err(line));
        }
        args.push(parse_expr(p, line)?);
    }
    Ok(args)
}

/// Parse a Myanmar library call as an expression, for contexts such as
/// `<lib> ၏ <fn>(args) ကို ပြပါ။`. Statement calls keep their existing
/// dedicated forms and behavior.
pub(crate) fn parse_myanmar_lib_call_expr(
    tokens: &[Token],
    line: usize,
) -> Result<Option<Expr>, String> {
    if tokens.len() < 5
        || !matches!(tokens[0].tok, Tok::Ident(_))
        || !ident_eq(&tokens[1].tok, KW_OF)
        || !matches!(tokens[2].tok, Tok::Ident(_))
        || !matches!(tokens[3].tok, Tok::LParen)
    {
        return Ok(None);
    }
    let close = find_close(tokens, 3, line)?;
    if close + 1 != tokens.len() {
        return Ok(None);
    }
    let lib = match &tokens[0].tok {
        Tok::Ident(name) => name.clone(),
        _ => unreachable!(),
    };
    let fn_name = match &tokens[2].tok {
        Tok::Ident(name) => name.clone(),
        _ => unreachable!(),
    };
    let inner = &tokens[4..close];
    let args = if inner.is_empty() {
        Vec::new()
    } else {
        split_top_level(inner, |token| matches!(token, Tok::Comma))
            .into_iter()
            .map(|part| parse_expr(part, line))
            .collect::<Result<Vec<_>, _>>()?
    };
    Ok(Some(Expr::LibCall(lib, fn_name, args, tokens[2].line)))
}

/// Build either the plain or the assigning library-call statement,
/// depending on whether the tokens started with `<var> အတွက်`.
pub(crate) fn lib_call_stmt(
    assign_name: Option<String>,
    lib: String,
    fn_name: String,
    args: Vec<Expr>,
    line: usize,
) -> Stmt {
    match assign_name {
        Some(name) => Stmt::LibCallAssign {
            name,
            lib,
            fn_name,
            args,
            line,
        },
        None => Stmt::LibCall {
            lib,
            fn_name,
            args,
            line,
        },
    }
}

/// Try to parse a library function-call statement. Handles all of:
///
///   `<lib> ၏ <fn>(<args>) ကို လုပ်ပါ။`
///   `<var> အတွက် <lib> ၏ <fn>(<args>) ကို လုပ်ပါ။`
///   `<lib> ၏ <fn> ကို လုပ်ရန် <args> ဖြင့်။`
///   `<lib> ၏ <fn> ကို လုပ်ရန် (<args>) ဖြင့်။`
///   `<var> အတွက် <lib> ၏ <fn> ကို လုပ်ရန် <args> ဖြင့်။`
///   `<var> အတွက် <lib> ၏ <fn> ကို လုပ်ရန် (<args>) ဖြင့်။`
///   plus the zero-argument `<lib> ၏ <fn> ကို လုပ်ပါ။` family.
///
/// Returns `Ok(None)` when `body` doesn't match this shape at all, so the
/// caller can keep trying other statement forms.
pub(crate) fn try_parse_lib_call(
    body: &[Token],
    line: usize,
    end_present: bool,
) -> Result<Option<Stmt>, String> {
    // Optional leading "<var> အတွက်" for the assigning form.
    let (assign_name, rest) = if body.len() >= 2
        && matches!(body[0].tok, Tok::Ident(_))
        && ident_eq(&body[1].tok, KW_FOR)
    {
        let name = match &body[0].tok {
            Tok::Ident(s) => s.clone(),
            _ => unreachable!(),
        };
        (Some(name), &body[2..])
    } else {
        (None, body)
    };

    // "<lib> ၏ <fn>"
    if rest.len() < 3 {
        return Ok(None);
    }
    let lib = match &rest[0].tok {
        Tok::Ident(s) => s.clone(),
        _ => return Ok(None),
    };
    if !ident_eq(&rest[1].tok, KW_OF) {
        return Ok(None);
    }
    let fn_name = match &rest[2].tok {
        Tok::Ident(s) => s.clone(),
        _ => return Ok(None),
    };
    let tail = &rest[3..];

    // `<fn>(<args>) ကို လုပ်ပါ။`
    if !tail.is_empty() && matches!(tail[0].tok, Tok::LParen) {
        let close = find_close(tail, 0, line)?;
        let after = &tail[close + 1..];
        if after.len() != 2
            || !ident_eq(&after[0].tok, KW_PARTICLE)
            || !ident_eq(&after[1].tok, KW_CALL)
        {
            return Err(lib_call_syntax_err(line));
        }
        if !end_present {
            return Err(missing_period_err(line));
        }
        let args = parse_lib_call_args(&tail[1..close], line)?;
        return Ok(Some(lib_call_stmt(assign_name, lib, fn_name, args, line)));
    }

    if tail.len() >= 2 && ident_eq(&tail[0].tok, KW_PARTICLE) {
        // `<fn> ကို (<args>) ဖြင့် လုပ်ပါ။` -- the verb-at-the-end spelling
        // of `<fn> ကို လုပ်ရန် (<args>) ဖြင့်။`.
        if matches!(tail[1].tok, Tok::LParen) {
            let close = find_close(tail, 1, line)?;
            let after = &tail[close + 1..];
            if after.len() != 2
                || !ident_eq(&after[0].tok, KW_BY)
                || !ident_eq(&after[1].tok, KW_CALL)
            {
                return Err(lib_call_syntax_err(line));
            }
            if !end_present {
                return Err(missing_period_err(line));
            }
            let args = parse_lib_call_args(&tail[2..close], line)?;
            return Ok(Some(lib_call_stmt(assign_name, lib, fn_name, args, line)));
        }
        // `<fn> ကို လုပ်ပါ။` (no arguments)
        if ident_eq(&tail[1].tok, KW_CALL) {
            if tail.len() != 2 {
                return Err(lib_call_syntax_err(line));
            }
            if !end_present {
                return Err(missing_period_err(line));
            }
            return Ok(Some(lib_call_stmt(
                assign_name,
                lib,
                fn_name,
                Vec::new(),
                line,
            )));
        }
        // `<fn> ကို လုပ်ရန် <args> ဖြင့်။`
        if ident_eq(&tail[1].tok, KW_CALL_WITH) {
            let after_call = &tail[2..];
            let by_idx = find_kw_top_level(after_call, KW_BY)
                .ok_or_else(|| func_call_missing_by_err(line))?;
            if by_idx + 1 != after_call.len() {
                return Err(lib_call_syntax_err(line));
            }
            let arg_tokens = &after_call[..by_idx];
            if arg_tokens.is_empty() {
                return Err(func_call_missing_arg_err(line));
            }
            if !end_present {
                return Err(missing_period_err(line));
            }
            let args = parse_lib_call_args(arg_tokens, line)?;
            return Ok(Some(lib_call_stmt(assign_name, lib, fn_name, args, line)));
        }
    }

    Ok(None)
}

/// Parse a complete expression from a token slice; errors if any tokens
/// are left over after the expression.
pub(crate) fn parse_expr(tokens: &[Token], line: usize) -> Result<Expr, String> {
    if tokens.is_empty() {
        return Err(generic_syntax_err(line));
    }
    let (expr, pos) = parse_expr_at(tokens, 0, line)?;
    if pos != tokens.len() {
        return Err(generic_syntax_err(line));
    }
    Ok(expr)
}

/// The precedence tiers, lowest first. Each tier parses its operands with the
/// next tier down, so an operand has already grouped everything tighter than
/// the operator that consumes it:
///
///   `||`                                         parse_expr_at
///   `&&`                                         parse_and_at
///   comparisons (`<` `>` `==` `!=` `<=` `>=`)    parse_cmp_at
///   `+` `-`                                      parse_add_at
///   `*` `/` `%` `\` (floor division)             parse_mul_at
///   `**` (exponent)                              parse_pow_at
///   unary `-`, `!`                               parse_unary_at
///   postfix (`[...]`, `၏ <field>`, `lib.fn(...)`)  parse_term_at
///
/// Operators on the same tier still group left to right, so `10 - 4 - 3` is
/// `(10 - 4) - 3`, while `1 + 2 * 3` is `1 + (2 * 3)` -- `*` binds tighter
/// than `+`.
pub(crate) fn parse_expr_at(
    tokens: &[Token],
    pos: usize,
    line: usize,
) -> Result<(Expr, usize), String> {
    // Arrow lambda: `x => x * 2` / `(a, b) => a + b`. Its body is the rest of
    // the expression, so it is recognized before every operator tier.
    if let Some((lambda, next)) = try_parse_arrow_lambda(tokens, pos, line)? {
        return Ok((lambda, next));
    }
    let (cond, i) = parse_or_at(tokens, pos, line)?;
    // Conditional expression: `<cond> ? <then> : <else>`. It sits at the
    // loosest level, below `||`, and associates to the right -- `a ? b : c ?
    // d : e` is `a ? b : (c ? d : e)`. The `:` that ends the `<then>` part is
    // looked for by the caller-recursion, since no operator consumes it.
    if i < tokens.len() && matches!(tokens[i].tok, Tok::Question) {
        let q_line = tokens[i].line;
        let (then_expr, after_then) = parse_expr_at(tokens, i + 1, line)?;
        if !matches!(tokens.get(after_then).map(|t| &t.tok), Some(Tok::Colon)) {
            return Err(ternary_bad_form_err(q_line));
        }
        let (else_expr, after_else) = parse_expr_at(tokens, after_then + 1, line)?;
        return Ok((
            Expr::Ternary(
                Box::new(cond),
                Box::new(then_expr),
                Box::new(else_expr),
                q_line,
            ),
            after_else,
        ));
    }
    Ok((cond, i))
}

/// An arrow lambda, the short spelling of `fn(<params>) { <body> }`:
///   - `<name> => <expression>`
///   - `(<p1>, <p2>) => <expression>`
/// The expression after `=>` is the lambda's body, so `map(xs, x => x * 2)`
/// needs no braces. Returns `None` when the tokens are not an arrow lambda.
pub(crate) fn try_parse_arrow_lambda(
    tokens: &[Token],
    pos: usize,
    line: usize,
) -> Result<Option<(Expr, usize)>, String> {
    let mut params: Vec<String> = Vec::new();
    let body_start = match tokens.get(pos).map(|t| &t.tok) {
        Some(Tok::Ident(name))
            if matches!(tokens.get(pos + 1).map(|t| &t.tok), Some(Tok::Arrow)) =>
        {
            params.push(name.clone());
            pos + 2
        }
        Some(Tok::LParen) => {
            let close = find_close(tokens, pos, line)?;
            if !matches!(tokens.get(close + 1).map(|t| &t.tok), Some(Tok::Arrow)) {
                return Ok(None);
            }
            let inner = &tokens[pos + 1..close];
            if inner.is_empty() {
                return Err(lambda_bad_form_err(line));
            }
            for part in split_top_level(inner, |t| matches!(t, Tok::Comma)) {
                match part {
                    [p] => match &p.tok {
                        Tok::Ident(s) => params.push(s.clone()),
                        _ => return Err(lambda_bad_form_err(line)),
                    },
                    _ => return Err(lambda_bad_form_err(line)),
                }
            }
            close + 2
        }
        _ => return Ok(None),
    };
    if body_start >= tokens.len() {
        return Err(lambda_bad_form_err(line));
    }
    let (value, next) = parse_expr_at(tokens, body_start, line)?;
    let defaults = vec![None; params.len()];
    Ok(Some((
        Expr::Lambda {
            params,
            defaults,
            body: vec![Stmt::ExprStmt { value, line }],
            line,
        },
        next,
    )))
}

pub(crate) fn parse_or_at(
    tokens: &[Token],
    pos: usize,
    line: usize,
) -> Result<(Expr, usize), String> {
    let (mut expr, mut i) = parse_and_at(tokens, pos, line)?;
    while i < tokens.len() && matches!(tokens[i].tok, Tok::Or) {
        let op_line = tokens[i].line;
        let (rhs, ni) = parse_and_at(tokens, i + 1, line)?;
        expr = Expr::Logical(Box::new(expr), LogicalOp::Or, Box::new(rhs), op_line);
        i = ni;
    }
    Ok((expr, i))
}

/// The `&&` tier: it binds tighter than `||` but looser than a comparison,
/// so `a > 1 && b < 2` joins the two comparisons and `a || b && c` is
/// `a || (b && c)`.
pub(crate) fn parse_and_at(
    tokens: &[Token],
    pos: usize,
    line: usize,
) -> Result<(Expr, usize), String> {
    let (mut expr, mut i) = parse_cmp_at(tokens, pos, line)?;
    while i < tokens.len() && matches!(tokens[i].tok, Tok::And) {
        let op_line = tokens[i].line;
        let (rhs, ni) = parse_cmp_at(tokens, i + 1, line)?;
        expr = Expr::Logical(Box::new(expr), LogicalOp::And, Box::new(rhs), op_line);
        i = ni;
    }
    Ok((expr, i))
}

/// The comparison tier.
pub(crate) fn parse_cmp_at(
    tokens: &[Token],
    pos: usize,
    line: usize,
) -> Result<(Expr, usize), String> {
    let (mut expr, mut i) = parse_add_at(tokens, pos, line)?;
    while i < tokens.len() {
        if let Tok::Cmp(op) = &tokens[i].tok {
            // A comparison is the lowest-precedence arithmetic operator in an
            // expression, so `x + 1 > y` compares the whole sum. It yields a
            // bool, which `print`, a declaration or an `if` can all consume.
            let op_line = tokens[i].line;
            let (rhs, ni) = parse_add_at(tokens, i + 1, line)?;
            expr = Expr::Compare(Box::new(expr), op.clone(), Box::new(rhs), op_line);
            i = ni;
            continue;
        }
        // Membership. The two spellings put the collection on different
        // sides: eng reads `<needle> in <haystack>`, Myanmar reads
        // `<haystack> ထဲ <needle>`.
        if ident_eq(&tokens[i].tok, ENG_IN) {
            let op_line = tokens[i].line;
            let (haystack, ni) = parse_add_at(tokens, i + 1, line)?;
            expr = Expr::Contains(Box::new(haystack), Box::new(expr), op_line);
            i = ni;
            continue;
        }
        if ident_eq(&tokens[i].tok, KW_INSIDE) {
            let op_line = tokens[i].line;
            let (needle, ni) = parse_add_at(tokens, i + 1, line)?;
            expr = Expr::Contains(Box::new(expr), Box::new(needle), op_line);
            i = ni;
            continue;
        }
        break;
    }
    Ok((expr, i))
}

/// The `+` / `-` tier.
pub(crate) fn parse_add_at(
    tokens: &[Token],
    pos: usize,
    line: usize,
) -> Result<(Expr, usize), String> {
    let (mut expr, mut i) = parse_mul_at(tokens, pos, line)?;
    while i < tokens.len() {
        let op = match &tokens[i].tok {
            Tok::Op(c) if *c == '+' || *c == '-' => *c,
            _ => break,
        };
        let op_line = tokens[i].line;
        let (rhs, ni) = parse_mul_at(tokens, i + 1, line)?;
        expr = Expr::Binary(Box::new(expr), op, Box::new(rhs), op_line);
        i = ni;
    }
    Ok((expr, i))
}

/// The `*` / `/` / `%` / `\` (floor division) tier.
pub(crate) fn parse_mul_at(
    tokens: &[Token],
    pos: usize,
    line: usize,
) -> Result<(Expr, usize), String> {
    let (mut expr, mut i) = parse_unary_at(tokens, pos, line)?;
    while i < tokens.len() {
        let op = match &tokens[i].tok {
            Tok::Op(c) if *c == '*' || *c == '/' || *c == '%' || *c == '\\' => *c,
            _ => break,
        };
        let op_line = tokens[i].line;
        let (rhs, ni) = parse_unary_at(tokens, i + 1, line)?;
        expr = Expr::Binary(Box::new(expr), op, Box::new(rhs), op_line);
        i = ni;
    }
    Ok((expr, i))
}

/// The exponent tier (`**`, tokenized as `Op('^')`), which sits just above
/// multiplication so `2 * 3 ** 2` is `2 * (3 ** 2)`. The exponent itself is
/// parsed as a unary expression, which makes `**` right-associative
/// (`2 ** 2 ** 3` is `2 ** (2 ** 3)`), allows `2 ** -1`, and makes
/// `-2 ** 2` mean `-(2 ** 2)`.
pub(crate) fn parse_pow_at(
    tokens: &[Token],
    pos: usize,
    line: usize,
) -> Result<(Expr, usize), String> {
    let (mut expr, mut i) = parse_term_at(tokens, pos, line)?;
    while i < tokens.len() && matches!(tokens[i].tok, Tok::Op('^')) {
        let op_line = tokens[i].line;
        let (rhs, ni) = parse_unary_at(tokens, i + 1, line)?;
        expr = Expr::Binary(Box::new(expr), '^', Box::new(rhs), op_line);
        i = ni;
    }
    Ok((expr, i))
}

/// The unary tier. Unary minus: `-2 * 3` negates `2` first, and `-x[0]`
/// negates the indexed element. Logical not: `!done`, `!(a > 1 && b < 2)`.
pub(crate) fn parse_unary_at(
    tokens: &[Token],
    pos: usize,
    line: usize,
) -> Result<(Expr, usize), String> {
    if let Some(t) = tokens.get(pos) {
        if matches!(t.tok, Tok::Op('-')) {
            let op_line = t.line;
            let (inner, next) = parse_unary_at(tokens, pos + 1, line)?;
            return Ok((Expr::Neg(Box::new(inner), op_line), next));
        }
        if matches!(t.tok, Tok::Not) {
            let op_line = t.line;
            let (inner, next) = parse_unary_at(tokens, pos + 1, line)?;
            return Ok((Expr::Not(Box::new(inner), op_line), next));
        }
        // Unary plus, so a range step can be written with its sign:
        // `(1, 2, +1)` is the same step as `(1, 2, 1)`.
        if matches!(t.tok, Tok::Op('+')) {
            return parse_unary_at(tokens, pos + 1, line);
        }
    }
    parse_pow_at(tokens, pos, line)
}

/// A lambda body: a block whose final bare value is the lambda's result
/// (`fn(x) { x }`, `fn(x) { y = x * 2; y }`). The statement parser's bare
/// expression fallback needs an arithmetic operator in the chunk, so a
/// statement that is only a value (`x`, `1`, `(v)`) is retried here as an
/// expression statement.
pub(crate) fn parse_lambda_block(tokens: &[Token]) -> Result<Vec<Stmt>, String> {
    let mut stmts = Vec::new();
    let chunks: Vec<(Vec<Token>, usize)> = split_statements(tokens)
        .into_iter()
        .map(|(chunk, line)| (chunk.to_vec(), line))
        .collect();
    let last = chunks.len().saturating_sub(1);
    for (i, (chunk, line)) in chunks.iter().enumerate() {
        match parse_stmt(chunk, *line) {
            // A lambda's body is its value, so a call in tail position
            // (`fn(x) { f(x) }`) has to hand its result back instead of
            // running as a bare statement the way it does in a named
            // function's body. Everything before the last statement keeps
            // statement semantics.
            Ok(Stmt::EngExprCall {
                name,
                args,
                line: call_line,
            }) if i == last => stmts.push(Stmt::ExprStmt {
                value: Expr::NewObj(name, args, call_line),
                line: call_line,
            }),
            Ok(stmt) => stmts.push(stmt),
            Err(err) => match parse_expr(chunk, *line) {
                Ok(value) => stmts.push(Stmt::ExprStmt { value, line: *line }),
                Err(_) => return Err(err),
            },
        }
    }
    Ok(stmts)
}

/// The postfix tier: a primary value followed by any number of `[...]`
/// indexes, `၏ <field>` accesses and `lib.fn(...)` calls.
pub(crate) fn parse_term_at(
    tokens: &[Token],
    pos: usize,
    line: usize,
) -> Result<(Expr, usize), String> {
    let (mut expr, mut i) = parse_primary_at(tokens, pos, line)?;
    loop {
        if i < tokens.len() && matches!(tokens[i].tok, Tok::LBracket) {
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
            continue;
        }

        // eng library call `<lib>.<fn>(args)` -- the English spelling of
        // `<lib> ၏ <fn>(args) ကို လုပ်ပါ။` -- and, without a call, either the
        // first-class function reference `<lib>.<fn>` (whose value a later
        // `TOOL(1, 100)` can invoke) or a dot field access `<value>.<field>`
        // (`p.name`, `response.အခြေအနေကုဒ်`), the same read as
        // `<value> ၏ <field>`. The parser cannot tell a library from a
        // variable, so a bare name stays a `LibFnRef` and the interpreter
        // decides: a variable of that name is read as an object field.
        if i + 1 < tokens.len()
            && matches!(tokens[i].tok, Tok::Dot)
            && matches!(tokens[i + 1].tok, Tok::Ident(_))
        {
            let base = match &expr {
                Expr::Ident(name) => Some(name.clone()),
                _ => None,
            };
            let fn_name = match &tokens[i + 1].tok {
                Tok::Ident(s) => s.clone(),
                _ => unreachable!(),
            };
            let dot_line = tokens[i].line;
            if i + 2 < tokens.len() && matches!(tokens[i + 2].tok, Tok::LParen) {
                // `<lib>.<fn>(args)`: a library call needs a library name,
                // so an expression on the left (a field read) is not one.
                let lib = base.ok_or_else(|| generic_syntax_err(line))?;
                let close = find_close(tokens, i + 2, tokens[i + 2].line)?;
                let inner = &tokens[i + 3..close];
                let parts = split_top_level(inner, |t| matches!(t, Tok::Comma));
                let mut args = Vec::with_capacity(parts.len());
                for p in parts {
                    let arg = parse_expr(p, line).map_err(|_| {
                        eng_call_bad_form_err(line, &format!("{}.{}", lib, fn_name))
                    })?;
                    args.push(arg);
                }
                expr = Expr::LibCall(lib.to_string(), fn_name, args, dot_line);
                i = close + 1;
            } else if let Some(lib) = base {
                expr = Expr::LibFnRef(lib.to_string(), fn_name, dot_line);
                i += 2;
            } else {
                expr = Expr::FieldAccess(Box::new(expr), fn_name, dot_line);
                i += 2;
            }
            continue;
        }

        // "၏" ("of"/possessive marker) doubles as a postfix operator here:
        //   <lib name> ၏ <url expr>   -- a call into a network library
        //                                (currently just "request"), e.g.
        //                                `request ၏ "https://..."`.
        //   <expr> ၏ <field ident>    -- generic field access on an
        //                                object value, e.g.
        //                                `response ၏ အခြေအနေကုဒ်`.
        if i < tokens.len() && ident_eq(&tokens[i].tok, KW_OF) {
            let of_line = tokens[i].line;
            if let Expr::Ident(name) = &expr {
                if name == LIB_REQUEST {
                    let (arg, ni) = parse_primary_at(tokens, i + 1, line)?;
                    expr = Expr::HttpGet(name.to_string(), Box::new(arg), of_line);
                    i = ni;
                    continue;
                }
                // Any other library name reaches here too when what
                // follows is not a field name -- that keeps
                // `<lib> ၏ <url>` working under an alias registered with
                // `<lib> အဖြစ် <alias>။` (e.g. `req ၏ "https://..."`),
                // while `<var> ၏ <field>` stays a field access. The
                // interpreter resolves the name and rejects non-libraries.
                if !(i + 1 < tokens.len() && matches!(tokens[i + 1].tok, Tok::Ident(_))) {
                    let (arg, ni) = parse_primary_at(tokens, i + 1, line)?;
                    expr = Expr::HttpGet(name.to_string(), Box::new(arg), of_line);
                    i = ni;
                    continue;
                }
            }
            if i + 1 < tokens.len() {
                if let Tok::Ident(field_name) = &tokens[i + 1].tok {
                    expr = Expr::FieldAccess(Box::new(expr), field_name.clone(), of_line);
                    i += 2;
                    continue;
                }
            }
            return Err(generic_syntax_err(line));
        }

        break;
    }
    Ok((expr, i))
}

/// A `<digits>[.<digits>]` token as an expression: parsed once here instead
/// of on every evaluation. A literal that parses as neither (the lexer happily
/// glues two dots into one token, e.g. `1.2.3`) stays a `NumLit`, so the
/// interpreter reports exactly the same `E047` as before -- and only when the
/// literal is actually evaluated.
pub(crate) fn num_lit(s: &str) -> Expr {
    if s.contains('.') {
        match s.parse::<f64>() {
            Ok(v) => Expr::Float(v),
            Err(_) => Expr::NumLit(s.to_string()),
        }
    } else {
        match s.parse::<i64>() {
            Ok(v) => Expr::Int(v),
            Err(_) => Expr::NumLit(s.to_string()),
        }
    }
}

pub(crate) fn parse_primary_at(
    tokens: &[Token],
    pos: usize,
    line: usize,
) -> Result<(Expr, usize), String> {
    if pos >= tokens.len() {
        return Err(generic_syntax_err(line));
    }
    match &tokens[pos].tok {
        Tok::Op('-') => {
            let op_line = tokens[pos].line;
            let (inner, next) = parse_term_at(tokens, pos + 1, line)?;
            Ok((Expr::Neg(Box::new(inner), op_line), next))
        }
        Tok::Num(s) => Ok((num_lit(s), pos + 1)),
        Tok::Str(s) => Ok((Expr::StrLit(s.clone()), pos + 1)),
        Tok::Bool(b) => Ok((Expr::BoolLit(*b), pos + 1)),
        Tok::Ident(s) => match s.as_str() {
            "True" => Ok((Expr::BoolLit(true), pos + 1)),
            "False" => Ok((Expr::BoolLit(false), pos + 1)),
            "မှန်" => Ok((Expr::BoolLit(true), pos + 1)),
            "မှား" => Ok((Expr::BoolLit(false), pos + 1)),
            "null" | "ဗလာ" => Ok((Expr::NullLit, pos + 1)),
            _ => {
                let next = pos + 1;
                // Anonymous function: `fn(x) { x * 2 }`. A named `fn
                // <name>(...)` is a statement, so `fn` followed straight by
                // the parameter list can only be a lambda value here.
                if s == ENG_FN && next < tokens.len() && matches!(tokens[next].tok, Tok::LParen) {
                    let close = find_close(tokens, next, tokens[next].line)?;
                    let (params, defaults) = parse_eng_fn_sig(&tokens[next + 1..close], line)
                        .map_err(|_| lambda_bad_form_err(line))?;
                    let brace = close + 1;
                    if !matches!(tokens.get(brace).map(|t| &t.tok), Some(Tok::LBrace)) {
                        return Err(lambda_bad_form_err(line));
                    }
                    let (body_tokens, after) = take_brace_block(tokens, brace, line)
                        .map_err(|_| lambda_bad_form_err(line))?;
                    let body = parse_lambda_block(body_tokens)?;
                    return Ok((
                        Expr::Lambda {
                            params,
                            defaults,
                            body,
                            line: tokens[pos].line,
                        },
                        after,
                    ));
                }
                // Object instantiation: "<ClassName>(args)". There is only
                // this one spelling. The old optional "အသစ်" (new) marker --
                // written before the argument list or after it -- is refused
                // with a pointer to the accepted form rather than being
                // silently skipped.
                if next < tokens.len() && ident_eq(&tokens[next].tok, KW_NEW) {
                    return Err(new_obj_missing_class_err(tokens[pos].line));
                }
                if next < tokens.len() && matches!(tokens[next].tok, Tok::LParen) {
                    let close = find_close(tokens, next, tokens[next].line)?;
                    let inner = &tokens[next + 1..close];
                    let parts = split_top_level(inner, |t| matches!(t, Tok::Comma));
                    let mut args = Vec::with_capacity(parts.len());
                    for p in parts {
                        args.push(parse_expr(p, line)?);
                    }
                    let after = close + 1;
                    if after < tokens.len() && ident_eq(&tokens[after].tok, KW_NEW) {
                        return Err(new_obj_missing_class_err(tokens[pos].line));
                    }
                    return Ok((Expr::NewObj(s.clone(), args, tokens[pos].line), after));
                }
                Ok((Expr::Ident(Sym::new(s.clone())), pos + 1))
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
            // Myanmar conditional expression, the value form of an if/else:
            //   (n % 2 == 0 ဖြစ်ရင် "even" မဟုတ်ရင် "odd")
            // The same node and evaluation rules as the eng
            // `<cond> ? <then> : <else>`.
            if let Some((then_idx, _)) =
                find_first_at_block_level(inner, &[KW_THEN_POS_ALT, KW_THEN_POS])
            {
                let (else_rel, _) =
                    find_first_at_block_level(&inner[then_idx + 1..], &[KW_ELSE_ALT, KW_ELSE])
                        .ok_or_else(|| myanmar_ternary_bad_form_err(line))?;
                let else_idx = then_idx + 1 + else_rel;
                let cond = parse_expr(&inner[..then_idx], line)
                    .map_err(|_| myanmar_ternary_bad_form_err(line))?;
                let then_expr = parse_expr(&inner[then_idx + 1..else_idx], line)
                    .map_err(|_| myanmar_ternary_bad_form_err(line))?;
                let else_expr = parse_expr(&inner[else_idx + 1..], line)
                    .map_err(|_| myanmar_ternary_bad_form_err(line))?;
                return Ok((
                    Expr::Ternary(
                        Box::new(cond),
                        Box::new(then_expr),
                        Box::new(else_expr),
                        tokens[pos].line,
                    ),
                    close + 1,
                ));
            }
            // A call written with its arguments first -- `(1, 2) add`, `() now`
            // -- is the same call as `add(1, 2)` / `now()`, and `(<args>)`
            // before a class name creates an object the same way
            // `Point(<args>)` does (the interpreter decides class, function or
            // builtin for both orders). Only a name that is not a keyword can
            // be a callee, so a plain tuple followed by the rest of a sentence
            // is still a tuple.
            if let Some((name, after)) = reversed_callee(tokens, close) {
                let mut args = Vec::new();
                if !inner.is_empty() {
                    for part in split_top_level(inner, |t| matches!(t, Tok::Comma)) {
                        args.push(parse_expr(part, line)?);
                    }
                }
                let caret = tokens[pos].line;
                return Ok((Expr::NewObj(name, args, caret), after));
            }
            let parts = split_top_level(inner, |t| matches!(t, Tok::Comma));
            if parts.len() == 1 {
                // A single, comma-less parenthesized group is just a
                // grouping that overrides operator precedence (e.g.
                // "(temp - 32) * 5"), not a one-element tuple.
                let inner_expr = parse_expr(parts[0], line)?;
                Ok((inner_expr, close + 1))
            } else {
                let mut elems = Vec::with_capacity(parts.len());
                for p in parts {
                    elems.push(parse_expr(p, line)?);
                }
                Ok((Expr::TupleLit(elems), close + 1))
            }
        }
        Tok::LBrace => {
            let close = find_close(tokens, pos, tokens[pos].line)?;
            let inner = &tokens[pos + 1..close];
            if inner.iter().any(|t| matches!(t.tok, Tok::End)) {
                // Dict: entries separated by "။", each "<key> သည် <value> ဖြစ်၏"
                let entries = split_top_level(inner, |t| matches!(t, Tok::End));
                let mut pairs = Vec::with_capacity(entries.len());
                for e in entries {
                    // Long entry: `"<key> သည် <value> ဖြစ်၏။`.
                    if let Some(is_idx) = find_kw_top_level(e, KW_IS) {
                        if is_idx + 1 != e.len() {
                            return Err(dict_entry_err(line));
                        }
                        let body_e = &e[..is_idx];
                        let assign_idx = find_kw_top_level(body_e, KW_ASSIGN)
                            .ok_or_else(|| dict_entry_err(line))?;
                        let key = parse_expr(&body_e[..assign_idx], line)?;
                        let value = parse_expr(&body_e[assign_idx + 1..], line)?;
                        pairs.push((key, value));
                        continue;
                    }
                    // Short entry: `"<key> က <value>။` -- the same pair with
                    // the `ဖြစ်၏` left out.
                    let decl_idx =
                        find_kw_top_level(e, KW_DECL).ok_or_else(|| dict_entry_err(line))?;
                    if decl_idx == 0 || decl_idx + 1 >= e.len() {
                        return Err(dict_entry_err(line));
                    }
                    let key = parse_expr(&e[..decl_idx], line)?;
                    let value = parse_expr(&e[decl_idx + 1..], line)?;
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

/// Parse "တန်ဖိုးကို <FUNC>" shaped statements (print / input-no-assign),
/// where `body` excludes the trailing End token and the function keyword itself.
pub(crate) fn parse_particle_call(
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

/// Parse the single argument of `print(...)` / `input(...)`: exactly one
/// parenthesized expression, nothing after the closing paren.
pub(crate) fn parse_eng_call_arg(
    tokens: &[Token],
    fn_name: &str,
    line: usize,
) -> Result<Expr, String> {
    if tokens.len() < 2
        || !matches!(tokens[0].tok, Tok::LParen)
        || !matches!(tokens[tokens.len() - 1].tok, Tok::RParen)
    {
        return Err(eng_call_bad_form_err(line, fn_name));
    }
    parse_expr(&tokens[1..tokens.len() - 1], line).map_err(|_| eng_call_bad_form_err(line, fn_name))
}

/// The first `:` written at the top level of `tokens` (not inside brackets,
/// parentheses or braces). A `case <value>:` label splits there.
pub(crate) fn top_level_colon(tokens: &[Token]) -> Option<usize> {
    let mut depth = 0usize;
    for (i, t) in tokens.iter().enumerate() {
        match t.tok {
            Tok::LBracket | Tok::LParen | Tok::LBrace => depth += 1,
            Tok::RBracket | Tok::RParen | Tok::RBrace => depth = depth.saturating_sub(1),
            Tok::Colon if depth == 0 => return Some(i),
            _ => {}
        }
    }
    None
}
