use super::*;

/// The library names in the tokens between "နည်းပညာများ" and
/// "သုံးမည်", e.g. `["ကျပန်း"]` for `ကျပန်း အဖြစ် r`. Used only to fill in
/// the missing-"ကို" error message, so a part that doesn't start with a name
/// is simply skipped.
pub(crate) fn library_names_before_use(tokens: &[Token]) -> Option<String> {
    let mut names: Vec<String> = Vec::new();
    for part in split_top_level_commas(tokens) {
        match part.first().map(|t| &t.tok) {
            Some(Tok::Ident(s)) => names.push(s.clone()),
            _ => {}
        }
    }
    if names.is_empty() {
        None
    } else {
        Some(names.join(", "))
    }
}

/// Parse an eng declaration (the trailing ';' has already been stripped by
/// the caller). Accepted shapes:
///   <name> :<type> = <value>
///   pin <name> :<type> = <value>
/// The declared type is enforced at runtime by the interpreter.
pub(crate) fn parse_eng_decl(body: &[Token], line: usize) -> Result<Stmt, String> {
    // --- eng calls: print(<value>); / input(<prompt>); ---
    if !body.is_empty() && matches!(&body[0].tok, Tok::Ident(s) if s == ENG_PRINT || s == ENG_INPUT)
    {
        let fn_name = match &body[0].tok {
            Tok::Ident(s) => s.clone(),
            _ => unreachable!(),
        };
        let value = parse_eng_call_arg(&body[1..], &fn_name, line)?;
        return Ok(if fn_name == ENG_PRINT {
            Stmt::EngPrint { value, line }
        } else {
            Stmt::EngInput {
                prompt: value,
                line,
            }
        });
    }
    // --- eng break: `break;` / `break <value>;` ---
    if !body.is_empty() && ident_eq(&body[0].tok, ENG_BREAK) {
        let value_tokens = &body[1..];
        if value_tokens.is_empty() {
            return Ok(Stmt::EngBreak { value: None, line });
        }
        let value = parse_expr(value_tokens, line).map_err(|_| eng_block_form_err(line))?;
        return Ok(Stmt::EngBreak {
            value: Some(value),
            line,
        });
    }

    // --- eng continue: `continue;` -- no value form ---
    if !body.is_empty() && ident_eq(&body[0].tok, ENG_CONTINUE) {
        if body.len() != 1 {
            return Err(generic_syntax_err(line));
        }
        return Ok(Stmt::Continue { line });
    }

    // --- eng return: `return;` / `return <value>;` ---
    if !body.is_empty() && ident_eq(&body[0].tok, ENG_RETURN) {
        let value_tokens = &body[1..];
        if value_tokens.is_empty() {
            return Ok(Stmt::EngReturn { value: None, line });
        }
        let value = parse_expr(value_tokens, line).map_err(|_| eng_return_bad_form_err(line))?;
        return Ok(Stmt::EngReturn {
            value: Some(value),
            line,
        });
    }

    let (is_const, rest) = if !body.is_empty() && ident_eq(&body[0].tok, ENG_PIN) {
        (true, &body[1..])
    } else {
        (false, body)
    };

    // --- eng compound assignment: `name += <value>;` (also -= *= /= %= ^=) ---
    if !is_const
        && rest.len() >= 4
        && matches!(rest[0].tok, Tok::Ident(_))
        && matches!(rest[1].tok, Tok::Op(_))
        && matches!(rest[2].tok, Tok::Assign)
    {
        let name = match &rest[0].tok {
            Tok::Ident(s) => s.clone(),
            _ => unreachable!(),
        };
        let op = match rest[1].tok {
            Tok::Op(c) => c,
            _ => unreachable!(),
        };
        let amount = parse_expr(&rest[3..], line).map_err(|_| eng_bad_form_err(line))?;
        return Ok(Stmt::EngMathAssign {
            name: Sym::new(name),
            op,
            amount,
            line,
        });
    }

    // --- eng assignment: `name = <value>;` (no type annotation) ---
    if !is_const
        && rest.len() >= 3
        && matches!(rest[0].tok, Tok::Ident(_))
        && matches!(rest[1].tok, Tok::Assign)
    {
        let name = match &rest[0].tok {
            Tok::Ident(s) => s.clone(),
            _ => unreachable!(),
        };
        let value = parse_expr(&rest[2..], line).map_err(|_| eng_bad_form_err(line))?;
        return Ok(Stmt::EngAssign {
            name: Sym::new(name),
            value,
            line,
        });
    }

    // --- eng index assignment: `xs[i] = <value>;` and `xs[i] += <value>;` ---
    // The left side is an index expression, so the write lands inside the
    // collection the variable holds instead of rebinding the name.
    if !is_const {
        if let Some(eq) = find_top_level_assign(rest) {
            let compound = eq >= 2 && matches!(rest[eq - 1].tok, Tok::Op(_));
            let (target_tokens, value_from, op) = if compound {
                let op = match &rest[eq - 1].tok {
                    Tok::Op(c) => Some(*c),
                    _ => None,
                };
                (&rest[..eq - 1], eq + 1, op)
            } else {
                (&rest[..eq], eq + 1, None)
            };
            if let Ok(target) = parse_expr(target_tokens, line) {
                if matches!(target, Expr::Index(..)) {
                    let value = parse_expr(&rest[value_from..], line)
                        .map_err(|_| eng_bad_form_err(line))?;
                    return Ok(match op {
                        Some(op) => Stmt::MathAssign {
                            target,
                            op,
                            amount: value,
                            line,
                        },
                        None => Stmt::IndexAssign {
                            target,
                            value,
                            line,
                        },
                    });
                }
            }
        }
    }

    // --- eng function-call statement: `name(arg1, arg2);` / `name();` ---
    if !is_const
        && rest.len() >= 2
        && matches!(rest[0].tok, Tok::Ident(_))
        && matches!(rest[1].tok, Tok::LParen)
    {
        let name = match &rest[0].tok {
            Tok::Ident(s) => s.clone(),
            _ => unreachable!(),
        };
        if !matches!(rest.last().map(|t| &t.tok), Some(Tok::RParen)) {
            return Err(eng_call_bad_form_err(line, &name));
        }
        let inner = &rest[2..rest.len() - 1];
        let mut args = Vec::new();
        if !inner.is_empty() {
            for part in split_top_level(inner, |t| matches!(t, Tok::Comma)) {
                let arg = parse_expr(part, line).map_err(|_| eng_call_bad_form_err(line, &name))?;
                args.push(arg);
            }
        }
        return Ok(Stmt::EngExprCall { name, args, line });
    }

    // --- eng expression statement: a bare expression whose value is
    //     discarded. `<lib>.<fn>(args);` maps onto the ordinary library-call
    //     statement, so calling a function that returns nothing is fine. ---
    if let Ok(expr) = parse_expr(body, line) {
        return Ok(match expr {
            Expr::LibCall(lib, fn_name, args, call_line) => Stmt::LibCall {
                lib,
                fn_name,
                args,
                line: call_line,
            },
            other => Stmt::ExprStmt { value: other, line },
        });
    }

    // <name>
    if rest.len() < 2 || !matches!(rest[0].tok, Tok::Ident(_)) {
        return Err(eng_bad_form_err(line));
    }
    let name = match &rest[0].tok {
        Tok::Ident(s) => s.clone(),
        _ => unreachable!(),
    };

    // :<type>
    if !matches!(rest[1].tok, Tok::Colon) {
        return Err(eng_bad_form_err(line));
    }
    if rest.len() < 4 || !matches!(rest[2].tok, Tok::Ident(_)) {
        return Err(eng_bad_form_err(line));
    }
    let type_name = match &rest[2].tok {
        Tok::Ident(s) => s.clone(),
        _ => unreachable!(),
    };
    if !is_eng_type(&type_name) {
        return Err(eng_bad_type_err(line, &type_name));
    }

    // = <value>
    if !matches!(rest[3].tok, Tok::Assign) {
        return Err(eng_bad_form_err(line));
    }
    let value_tokens = &rest[4..];
    if value_tokens.is_empty() {
        return Err(eng_missing_value_err(line));
    }

    // Special case: `= input(<prompt>)` -- reading console input into the
    // declared variable, with the value coerced to the declared type.
    if value_tokens.len() >= 2 && ident_eq(&value_tokens[0].tok, ENG_INPUT) {
        let prompt = parse_eng_call_arg(&value_tokens[1..], ENG_INPUT, line)?;
        return Ok(Stmt::EngInputAssign {
            name,
            type_name,
            prompt,
            line,
        });
    }

    let value = parse_expr(value_tokens, line)?;

    Ok(Stmt::EngDecl {
        name: Sym::new(name),
        type_name,
        value,
        is_const,
        line,
    })
}

/// True for the keywords that open an eng block statement.
pub(crate) fn is_eng_block_keyword(tok: &Tok) -> bool {
    matches!(
        tok,
        Tok::Ident(s)
            if s == ENG_IF
                || s == ENG_WHILE
                || s == ENG_FN
                || s == ENG_LOOP
                || s == ENG_FOR
                || s == ENG_SWITCH
    )
}

/// The `{` that opens an iterator for loop's body. The body is the *last*
/// depth-0 brace whose matching `}` ends the statement, so a set or dict
/// literal used as the iterated value is left in the value:
/// `for k in {"a" သည် 1 ဖြစ်၏။} { ... }`.
pub(crate) fn find_iter_body_lbrace(tokens: &[Token], line: usize) -> Option<usize> {
    let mut depth = 0i32;
    let mut candidates: Vec<usize> = Vec::new();
    for (i, t) in tokens.iter().enumerate() {
        match t.tok {
            Tok::LParen | Tok::LBracket | Tok::LBrace => {
                if depth == 0 && matches!(t.tok, Tok::LBrace) {
                    candidates.push(i);
                }
                depth += 1;
            }
            Tok::RParen | Tok::RBracket | Tok::RBrace => depth -= 1,
            _ => {}
        }
    }
    candidates
        .into_iter()
        .rev()
        .find(|&i| matches!(find_close(tokens, i, line), Ok(close) if close + 1 == tokens.len()))
}

/// Index of the first `{` at paren/bracket depth 0 -- the brace that opens a
/// block statement's body, as opposed to a literal nested in its condition.
pub(crate) fn find_block_lbrace(tokens: &[Token]) -> Option<usize> {
    let mut depth = 0i32;
    for (i, t) in tokens.iter().enumerate() {
        match t.tok {
            Tok::LParen | Tok::LBracket => depth += 1,
            Tok::RParen | Tok::RBracket => depth -= 1,
            Tok::LBrace if depth == 0 => return Some(i),
            _ => {}
        }
    }
    None
}

/// One `{ ... }` body starting at `tokens[open_idx]`; returns the tokens
/// between the braces plus the index just past the closing `}`.
pub(crate) fn take_brace_block(
    tokens: &[Token],
    open_idx: usize,
    line: usize,
) -> Result<(&[Token], usize), String> {
    let close = find_close(tokens, open_idx, line).map_err(|_| eng_block_form_err(line))?;
    Ok((&tokens[open_idx + 1..close], close + 1))
}

/// Parse a whole eng block statement chunk (no trailing ';' / '။').
pub(crate) fn parse_eng_block_statement(tokens: &[Token], line: usize) -> Result<Stmt, String> {
    if tokens.is_empty() {
        return Err(eng_block_form_err(line));
    }
    if ident_eq(&tokens[0].tok, ENG_IF) {
        return parse_eng_if(tokens, line);
    }
    if ident_eq(&tokens[0].tok, ENG_WHILE) {
        return parse_eng_while(tokens, line);
    }
    if ident_eq(&tokens[0].tok, ENG_FN) {
        return parse_eng_fn(tokens, line);
    }
    if ident_eq(&tokens[0].tok, ENG_LOOP) {
        return parse_eng_loop(tokens, line);
    }
    if ident_eq(&tokens[0].tok, ENG_FOR) {
        return parse_eng_for(tokens, line);
    }
    if ident_eq(&tokens[0].tok, ENG_SWITCH) {
        return parse_eng_switch(tokens, line);
    }
    Err(eng_block_form_err(line))
}

/// `for` in the eng spelling. Two forms, told apart by what follows the
/// keyword: a '(' opens the three-part counted header, anything else is the
/// iterator spelling.
///   - `for (<init>; <cond>; <step>) { ... }`  -- C-style counted loop
///   - `for <name> in <value> { ... }`         -- Python-style iteration
pub(crate) fn parse_eng_for(tokens: &[Token], line: usize) -> Result<Stmt, String> {
    if matches!(tokens.get(1).map(|t| &t.tok), Some(Tok::LParen)) {
        return parse_eng_for_counted(tokens, line);
    }
    parse_eng_for_in(tokens, line)
}

/// `for <name> in <value> { ... }` -- iteration over a value, with the same
/// rules as the Myanmar `for x သည် xs ထဲမှ တစ်ခုစီ` loop: a number
/// auto-ranges from 0 (exclusive end), a list / tuple / set yields its
/// elements, and a dict yields its keys. Any other value is rejected when the
/// loop runs.
pub(crate) fn parse_eng_for_in(tokens: &[Token], line: usize) -> Result<Stmt, String> {
    let var_name = match tokens.get(1).map(|t| &t.tok) {
        Some(Tok::Ident(s)) => s.clone(),
        _ => return Err(eng_for_in_form_err(line)),
    };
    if !ident_eq_opt(tokens.get(2), ENG_IN) {
        return Err(eng_for_in_form_err(line));
    }
    let rest = &tokens[3..];
    let brace_rel = find_iter_body_lbrace(rest, line).ok_or_else(|| eng_for_in_form_err(line))?;
    if brace_rel == 0 {
        return Err(eng_for_in_form_err(line));
    }
    let source = parse_expr(&rest[..brace_rel], line).map_err(|_| eng_for_in_form_err(line))?;
    let (body_tokens, after) =
        take_brace_block(tokens, 3 + brace_rel, line).map_err(|_| eng_for_in_form_err(line))?;
    if after != tokens.len() {
        return Err(eng_for_in_form_err(line));
    }
    Ok(Stmt::ForLoop {
        var_name,
        source: ForSource::Auto(source),
        body: parse_block(body_tokens)?,
        line,
    })
}

/// `switch (<value>) { case <value>: <statements> ... default: <statements> }`
/// -- the arms are tried in source order and the first matching one runs; the
/// `default:` arm runs when none matched, wherever it is written. There is no
/// fall-through: only one arm ever runs, so arms need no `break`.
pub(crate) fn parse_eng_switch(tokens: &[Token], line: usize) -> Result<Stmt, String> {
    if !matches!(tokens.get(1).map(|t| &t.tok), Some(Tok::LParen)) {
        return Err(eng_switch_form_err(line));
    }
    let header_close = find_close(tokens, 1, line).map_err(|_| eng_switch_form_err(line))?;
    let subject =
        parse_expr(&tokens[2..header_close], line).map_err(|_| eng_switch_form_err(line))?;
    let brace = header_close + 1;
    if !matches!(tokens.get(brace).map(|t| &t.tok), Some(Tok::LBrace)) {
        return Err(eng_switch_form_err(line));
    }
    let (body_tokens, after) =
        take_brace_block(tokens, brace, line).map_err(|_| eng_switch_form_err(line))?;
    if after != tokens.len() {
        return Err(eng_switch_form_err(line));
    }

    let mut arms: Vec<SwitchArm> = Vec::new();
    let mut seen_default = false;
    for (chunk, chunk_line) in split_statements(body_tokens) {
        let is_case = chunk.first().map(|t| ident_eq(&t.tok, ENG_CASE)) == Some(true);
        let is_default = chunk.first().map(|t| ident_eq(&t.tok, ENG_DEFAULT)) == Some(true);
        if !is_case && !is_default {
            // A further statement of the arm above.
            let Some(arm) = arms.last_mut() else {
                return Err(eng_switch_form_err(chunk_line));
            };
            arm.body.push(parse_stmt(&chunk, chunk_line)?);
            continue;
        }
        if is_default && seen_default {
            return Err(eng_switch_form_err(chunk_line));
        }
        seen_default |= is_default;
        let rest = &chunk[1..];
        let Some(colon) = top_level_colon(rest) else {
            return Err(eng_switch_form_err(chunk_line));
        };
        let case = if is_case {
            if colon == 0 {
                return Err(eng_switch_form_err(chunk_line));
            }
            Some(
                parse_expr(&rest[..colon], chunk_line)
                    .map_err(|_| eng_switch_form_err(chunk_line))?,
            )
        } else {
            if colon != 0 {
                return Err(eng_switch_form_err(chunk_line));
            }
            None
        };
        let mut body = Vec::new();
        let tail = &rest[colon + 1..];
        if !tail.is_empty() {
            body.push(parse_stmt(tail, chunk_line)?);
        }
        arms.push(SwitchArm {
            case,
            body,
            line: chunk_line,
        });
    }
    if arms.is_empty() {
        return Err(eng_switch_form_err(line));
    }
    Ok(Stmt::Switch {
        subject,
        arms,
        line,
    })
}

/// Split a `ရွေးရန်` body into its arms. Every `။` at bracket depth 0 ends
/// an arm statement (the arm's own `ဖြစ်ရင်` is a selector, not a block
/// header), and the statement's `။` stays in its chunk so the ordinary
/// statement parser can read it. A header written on a line of its own
/// (`7 ဖြစ်ရင်`) leaves its body to the following chunks.
pub(crate) fn split_switch_arms(tokens: &[Token]) -> Vec<(Vec<Token>, usize)> {
    let mut arms = Vec::new();
    let mut bracket_depth = 0i32;
    let mut start = 0usize;
    for (i, t) in tokens.iter().enumerate() {
        if is_open(&t.tok) {
            bracket_depth += 1;
        } else if is_close(&t.tok) {
            bracket_depth -= 1;
        } else if bracket_depth == 0 && matches!(t.tok, Tok::End) {
            let slice = &tokens[start..=i];
            arms.push((slice.to_vec(), slice[0].line));
            start = i + 1;
        }
    }
    if start < tokens.len() {
        let slice = &tokens[start..];
        arms.push((slice.to_vec(), slice[0].line));
    }
    arms
}

/// `ရွေးရန် (<value>) ... ပြီး။` -- the Myanmar switch. Each arm is
/// `<value> ဖြစ်ရင် <statements>` and the default is `မူလ <statements>`;
/// arms are tried in order and only the first match runs, so there is no
/// fall-through and no `break` is needed.
pub(crate) fn parse_switch_statement(inner: &[Token], line: usize) -> Result<Stmt, String> {
    if inner.is_empty() || !matches!(inner[0].tok, Tok::LParen) {
        return Err(myanmar_switch_form_err(line));
    }
    let close = find_close(inner, 0, line).map_err(|_| myanmar_switch_form_err(line))?;
    let subject = parse_expr(&inner[1..close], line).map_err(|_| myanmar_switch_form_err(line))?;

    let mut arms: Vec<SwitchArm> = Vec::new();
    let mut seen_default = false;
    for (chunk, chunk_line) in split_switch_arms(&inner[close + 1..]) {
        if chunk.is_empty() {
            continue;
        }
        if ident_eq(&chunk[0].tok, KW_SWITCH_DEFAULT) {
            if seen_default {
                return Err(myanmar_switch_form_err(chunk_line));
            }
            seen_default = true;
            let mut tail = &chunk[1..];
            if matches!(tail.first().map(|t| &t.tok), Some(Tok::Colon)) {
                tail = &tail[1..];
            }
            let mut body = Vec::new();
            if !tail.is_empty() {
                body.push(parse_stmt(tail, chunk_line)?);
            }
            arms.push(SwitchArm {
                case: None,
                body,
                line: chunk_line,
            });
            continue;
        }
        if let Some((then_idx, _)) =
            find_first_at_block_level(&chunk, &[KW_THEN_POS_ALT, KW_THEN_POS])
        {
            if then_idx == 0 {
                return Err(myanmar_switch_form_err(chunk_line));
            }
            let case = parse_expr(&chunk[..then_idx], chunk_line)
                .map_err(|_| myanmar_switch_form_err(chunk_line))?;
            let tail = &chunk[then_idx + 1..];
            let mut body = Vec::new();
            if !tail.is_empty() {
                body.push(parse_stmt(tail, chunk_line)?);
            }
            arms.push(SwitchArm {
                case: Some(case),
                body,
                line: chunk_line,
            });
            continue;
        }
        // A further statement of the arm above (its header stood alone).
        let Some(arm) = arms.last_mut() else {
            return Err(myanmar_switch_form_err(chunk_line));
        };
        arm.body.push(parse_stmt(&chunk, chunk_line)?);
    }
    if arms.is_empty() {
        return Err(myanmar_switch_form_err(line));
    }
    Ok(Stmt::Switch {
        subject,
        arms,
        line,
    })
}

/// `for (<init>; <cond>; <step>) { ... }` -- the C-style counted loop. The
/// `<init>` and `<step>` clauses are ordinary eng statements without their
/// ';' (`i = 0`, `i :int = 0`, `i += 1`, `i = i + 1`), and `<cond>` is an eng
/// condition. The step runs at the end of every iteration; a `break` in the
/// body ends the loop before it.
pub(crate) fn parse_eng_for_counted(tokens: &[Token], line: usize) -> Result<Stmt, String> {
    if !matches!(tokens.get(1).map(|t| &t.tok), Some(Tok::LParen)) {
        return Err(eng_for_form_err(line));
    }
    let header_close = find_close(tokens, 1, line).map_err(|_| eng_for_form_err(line))?;
    let parts = split_top_level(&tokens[2..header_close], |t| matches!(t, Tok::Semicolon));
    let (init_tokens, cond_tokens, step_tokens) = match parts.as_slice() {
        [init, cond, step] if !init.is_empty() && !cond.is_empty() && !step.is_empty() => {
            (*init, *cond, *step)
        }
        _ => return Err(eng_for_form_err(line)),
    };
    let cond = parse_eng_cond(cond_tokens, line).map_err(|_| eng_for_form_err(line))?;
    let init = parse_eng_decl(init_tokens, line).map_err(|_| eng_for_form_err(line))?;
    let step = parse_eng_decl(step_tokens, line).map_err(|_| eng_for_form_err(line))?;

    let brace = header_close + 1;
    if !matches!(tokens.get(brace).map(|t| &t.tok), Some(Tok::LBrace)) {
        return Err(eng_for_form_err(line));
    }
    let (body_tokens, after) =
        take_brace_block(tokens, brace, line).map_err(|_| eng_for_form_err(line))?;
    if after != tokens.len() {
        return Err(eng_for_form_err(line));
    }
    Ok(Stmt::EngFor {
        init: Box::new(init),
        cond,
        step: Box::new(step),
        body: parse_block(body_tokens)?,
        line,
    })
}

/// `loop { ... }` used as a statement.
pub(crate) fn parse_eng_loop(tokens: &[Token], line: usize) -> Result<Stmt, String> {
    Ok(Stmt::EngLoop {
        body: parse_eng_loop_block(tokens, line)?,
    })
}

/// The `{ ... }` body of a `loop` starting at `tokens[0]` (the `loop`
/// keyword); anything after the closing `}` is an error.
pub(crate) fn parse_eng_loop_block(tokens: &[Token], line: usize) -> Result<Vec<Stmt>, String> {
    if tokens.len() < 2
        || !ident_eq(&tokens[0].tok, ENG_LOOP)
        || !matches!(tokens[1].tok, Tok::LBrace)
    {
        return Err(eng_block_form_err(line));
    }
    let (body_tokens, after) = take_brace_block(tokens, 1, line)?;
    if after != tokens.len() {
        return Err(eng_block_form_err(line));
    }
    parse_block(body_tokens)
}

/// Parse an eng library import: `use <lib> [as <alias>] [, ...]` (the
/// trailing ';' has already been stripped by the caller).
pub(crate) fn parse_eng_use(body: &[Token], line: usize) -> Result<Stmt, String> {
    let rest = &body[1..];
    if rest.is_empty() {
        return Err(eng_use_bad_form_err(line));
    }
    let mut libs: Vec<(String, Option<String>)> = Vec::new();
    for part in split_top_level(rest, |t| matches!(t, Tok::Comma)) {
        match part {
            // <lib>
            [name] => match &name.tok {
                Tok::Ident(s) => libs.push((s.clone(), None)),
                _ => return Err(eng_use_bad_form_err(line)),
            },
            // <lib> as <alias>
            [name, as_t, alias] if ident_eq(&as_t.tok, ENG_AS) => match (&name.tok, &alias.tok) {
                (Tok::Ident(lib), Tok::Ident(a)) => libs.push((lib.clone(), Some(a.clone()))),
                _ => return Err(eng_use_bad_form_err(line)),
            },
            _ => return Err(eng_use_bad_form_err(line)),
        }
    }
    if libs.is_empty() {
        return Err(eng_use_bad_form_err(line));
    }
    Ok(Stmt::UseLibrary { libs, line })
}

/// Recognize `<name> [:<type>] = loop { ... }` -- the eng loop-value
/// declaration/assignment (the trailing ';', if any, is already stripped by
/// the caller). Returns `Ok(None)` when the statement is shaped like anything
/// else, so ordinary declarations and Myanmar statements fall through
/// untouched.
pub(crate) fn try_parse_eng_loop_assign(
    body: &[Token],
    line: usize,
) -> Result<Option<Stmt>, String> {
    let (is_const, rest) = if !body.is_empty() && ident_eq(&body[0].tok, ENG_PIN) {
        (true, &body[1..])
    } else {
        (false, body)
    };
    if rest.len() < 4 || !matches!(rest[0].tok, Tok::Ident(_)) {
        return Ok(None);
    }
    let name = match &rest[0].tok {
        Tok::Ident(s) => s.clone(),
        _ => unreachable!(),
    };
    let (type_name, eq_idx) = match &rest[1].tok {
        Tok::Colon => match rest.get(2).map(|t| &t.tok) {
            Some(Tok::Ident(t)) => {
                if !is_eng_type(t) {
                    return Err(eng_bad_type_err(line, t));
                }
                (Some(t.clone()), 3usize)
            }
            _ => return Ok(None),
        },
        // `<name> = loop { ... }`: the '=' is at index 1, so the value starts
        // at index 2.
        Tok::Assign => (None, 1usize),
        _ => return Ok(None),
    };
    if !matches!(rest.get(eq_idx).map(|t| &t.tok), Some(Tok::Assign)) {
        return Ok(None);
    }
    let value_tokens = &rest[eq_idx + 1..];
    if value_tokens.first().map(|t| ident_eq(&t.tok, ENG_LOOP)) != Some(true) {
        return Ok(None);
    }
    Ok(Some(Stmt::EngLoopAssign {
        name,
        type_name,
        is_const,
        body: parse_eng_loop_block(value_tokens, line)?,
        line,
    }))
}

/// eng `break` / `break <value>` / `return` / `return <value>` written
/// without a trailing ';', which is allowed when the statement ends a block
/// (e.g. `if (...) { break }`). Returns `Ok(None)` for anything else.
pub(crate) fn parse_eng_bare_keyword_stmt(
    body: &[Token],
    line: usize,
) -> Result<Option<Stmt>, String> {
    let is_break = body.first().map(|t| ident_eq(&t.tok, ENG_BREAK)) == Some(true);
    let is_return = body.first().map(|t| ident_eq(&t.tok, ENG_RETURN)) == Some(true);
    if !is_break && !is_return {
        return Ok(None);
    }
    let value_tokens = &body[1..];
    let value = if value_tokens.is_empty() {
        None
    } else {
        Some(parse_expr(value_tokens, line).map_err(|_| eng_block_form_err(line))?)
    };
    Ok(Some(if is_break {
        Stmt::EngBreak { value, line }
    } else {
        Stmt::EngReturn { value, line }
    }))
}

/// `[a, b] = <value>` / `(a, b) = <value>` -- names on the left, one value on
/// the right. Returns `Ok(None)` for a statement of any other shape (a call
/// with its arguments first, a tuple literal, ...).
pub(crate) fn try_parse_eng_destructure(
    body: &[Token],
    line: usize,
) -> Result<Option<Stmt>, String> {
    if !matches!(
        body.first().map(|t| &t.tok),
        Some(Tok::LBracket) | Some(Tok::LParen)
    ) {
        return Ok(None);
    }
    let close = find_close(body, 0, line)?;
    if !matches!(body.get(close + 1).map(|t| &t.tok), Some(Tok::Assign)) {
        return Ok(None);
    }
    let mut names = Vec::new();
    for part in split_top_level(&body[1..close], |t| matches!(t, Tok::Comma)) {
        match part {
            [p] => match &p.tok {
                Tok::Ident(s) => names.push(s.clone()),
                _ => return Err(destructure_bad_form_err(line)),
            },
            _ => return Err(destructure_bad_form_err(line)),
        }
    }
    if names.is_empty() {
        return Err(destructure_bad_form_err(line));
    }
    let value = parse_expr(&body[close + 2..], line).map_err(|_| destructure_bad_form_err(line))?;
    Ok(Some(Stmt::Destructure { names, value, line }))
}

/// `<name>(<args>)` written as a standalone statement -- the parenthesized,
/// eng spelling of `<name> ကို လုပ်ပါ။`. It is accepted with or without the
/// trailing ';' (`greet();` / `greet()`), so a call balances a definition
/// written as `အလုပ် greet() သည် ... ပြီး။`. `print` / `input` keep their
/// own statements below, so they're left alone here. Returns `Ok(None)` for
/// any statement with a different shape, so other forms fall through.
pub(crate) fn try_parse_eng_call_stmt(body: &[Token], line: usize) -> Result<Option<Stmt>, String> {
    // --- a call with its arguments first: `(1, 2) add;` / `() now;` ---
    //     The same statement *and the same node* as `add(1, 2);`, so both
    //     orders resolve the name identically and report the same errors.
    if let Some((name, args)) = reversed_call_stmt(body, line)? {
        return Ok(Some(Stmt::EngExprCall { name, args, line }));
    }
    if body.len() < 3
        || !matches!(body[0].tok, Tok::Ident(_))
        || !matches!(body[1].tok, Tok::LParen)
    {
        return Ok(None);
    }
    let name = match &body[0].tok {
        Tok::Ident(s) => s.clone(),
        _ => unreachable!(),
    };
    if name == ENG_PRINT || name == ENG_INPUT {
        return Ok(None);
    }
    // The call's ')' must be the last token: `Name(args) ကို ဖော်ပြပါ။` is
    // an object construction inside a print statement, not a call, so
    // anything after the closing paren leaves the statement to the other
    // forms. An unclosed '(' keeps its own bracket error.
    let close = find_close(body, 1, line)?;
    if close + 1 != body.len() {
        return Ok(None);
    }
    let inner = &body[2..close];
    let mut args = Vec::new();
    if !inner.is_empty() {
        for part in split_top_level(inner, |t| matches!(t, Tok::Comma)) {
            let arg = parse_expr(part, line).map_err(|_| eng_call_bad_form_err(line, &name))?;
            args.push(arg);
        }
    }
    Ok(Some(Stmt::EngExprCall { name, args, line }))
}

/// Parse an eng function alias: `<fn>() as <alias>;` or `<fn> as <alias>;`,
/// with a comma-separated alias list allowed -- the English spelling of
/// `<fn> အဖြစ် <alias>။`. The empty `()` is optional, so both a
/// definition-looking name (`calculate as c;`) and a call-looking one
/// (`calculate() as c;`) work, and each alias is a callable name for the same
/// function. `terminated` says whether the statement's ';' was present:
/// without it, an alias-shaped statement is reported as the missing ';'
/// rather than silently accepted. Returns `Ok(None)` for any other shape, so
/// every other statement form falls through untouched.
pub(crate) fn try_parse_eng_func_alias(
    body: &[Token],
    line: usize,
    terminated: bool,
) -> Result<Option<Stmt>, String> {
    let original = match body.first().map(|t| &t.tok) {
        Some(Tok::Ident(s)) => s.clone(),
        _ => return Ok(None),
    };
    let mut rest = &body[1..];
    // An optional empty `()` between the name and `as`.
    if matches!(rest.first().map(|t| &t.tok), Some(Tok::LParen)) {
        if !matches!(rest.get(1).map(|t| &t.tok), Some(Tok::RParen)) {
            return Ok(None);
        }
        rest = &rest[2..];
    }
    if !rest.first().is_some_and(|t| ident_eq(&t.tok, ENG_AS)) {
        return Ok(None);
    }
    if !terminated {
        return Err(missing_semicolon_err(line));
    }
    let alias_tokens = &rest[1..];
    if alias_tokens.is_empty() {
        return Err(eng_func_alias_bad_form_err(line));
    }
    let mut aliases = Vec::new();
    for part in split_top_level(alias_tokens, |t| matches!(t, Tok::Comma)) {
        match part {
            [t] => match &t.tok {
                Tok::Ident(alias) => aliases.push(alias.clone()),
                _ => return Err(eng_func_alias_bad_form_err(line)),
            },
            _ => return Err(eng_func_alias_bad_form_err(line)),
        }
    }
    if aliases.is_empty() {
        return Err(eng_func_alias_bad_form_err(line));
    }
    Ok(Some(Stmt::FuncAlias {
        original,
        aliases,
        line,
    }))
}

/// True when a statement is shaped like an eng (English-spelled) statement,
/// which ends with ';' rather than '။': `use ...`, `print(...)`,
/// `input(...)`, `break`, `return`, `pin ...`, `name :<type> = <value>`,
/// `name = <value>`, `name += <value>`. Used to report a missing ';' as E002
/// instead of a generic parse failure.
pub(crate) fn looks_like_eng_stmt(body: &[Token]) -> bool {
    let first = match body.first() {
        Some(t) => t,
        None => return false,
    };
    match &first.tok {
        Tok::Ident(s) => {
            if s == ENG_USE
                || s == ENG_PRINT
                || s == ENG_INPUT
                || s == ENG_BREAK
                || s == ENG_RETURN
                || s == ENG_PIN
            {
                return true;
            }
        }
        _ => return false,
    }
    // A statement starting with `name(` is deliberately *not* listed here:
    // `name(args)` is either an eng call (claimed before this check) or,
    // with anything after the `)`, an ordinary statement like
    // `အကယ်၍ (n == 1) ဖြစ်လျှင်` or `Point(1, 2) ကို ဖော်ပြပါ။`.
    match body.get(1).map(|t| &t.tok) {
        Some(Tok::Colon) | Some(Tok::Assign) => true,
        Some(Tok::Op(_)) => matches!(body.get(2).map(|t| &t.tok), Some(Tok::Assign)),
        _ => false,
    }
}

pub(crate) fn parse_stmt(tokens: &[Token], line: usize) -> Result<Stmt, String> {
    let end_present = has_end(tokens);
    let mut body: Vec<Token> = if end_present {
        tokens[..tokens.len() - 1].to_vec()
    } else {
        tokens.to_vec()
    };

    let terminated = matches!(body.last().map(|t| &t.tok), Some(Tok::Semicolon));
    let decl_body: &[Token] = if terminated {
        &body[..body.len() - 1]
    } else {
        &body[..]
    };

    // --- eng library import: `use <lib> [as <alias>] [, ...];` -- the
    //     English spelling of `နည်းပညာများ <lib> ကို သုံးမည်။`. Like
    //     every eng statement it needs its ';' terminator. ---
    if body.first().map(|t| ident_eq(&t.tok, ENG_USE)) == Some(true) {
        if !terminated {
            return Err(missing_semicolon_err(line));
        }
        return parse_eng_use(decl_body, line);
    }

    // --- eng library block statements (brace-terminated, C-style spelling):
    //     if (<cond>) { ... } [else if ...] [else { ... }]
    //     while (<cond>) { ... }
    //     loop { ... }
    //     fn <name>(<params>) { ... }
    // These carry no ';' / '။' of their own, so they're recognized by their
    // leading keyword. A stray trailing ';' after the closing '}' is fine.
    if body.first().map(|t| is_eng_block_keyword(&t.tok)) == Some(true) {
        return parse_eng_block_statement(decl_body, line);
    }

    // --- eng postfix guard: `<statement> if (<condition>);` -- a one-line
    //     conditional, e.g. `print("x") if (age > 18);` ---
    if terminated {
        if let Some(stmt) = try_parse_postfix_guard(decl_body, line)? {
            return Ok(stmt);
        }
    }

    // --- eng `break` / `return` without their ';', as the last statement of a
    //     block: `if (...) { break }` ---
    if !end_present && !terminated {
        if let Some(stmt) = parse_eng_bare_keyword_stmt(decl_body, line)? {
            return Ok(stmt);
        }
    }

    // --- eng loop as a value: `<name> [:<type>] = loop { ... }`, with or
    //     without the trailing ';'. ---
    if let Some(stmt) = try_parse_eng_loop_assign(decl_body, line)? {
        return Ok(stmt);
    }

    // --- eng function alias: `calculate() as c;` / `calculate as c, d;` --
    //     the English spelling of `calculate အဖြစ် c။`. Checked before the
    //     call form below, which would otherwise read `calculate()` as a call
    //     and stop at the `as`. ---
    if let Some(stmt) = try_parse_eng_func_alias(decl_body, line, terminated)? {
        return Ok(stmt);
    }

    // --- destructuring assignment: `[a, b] = <value>;` (also `(a, b) =`) ---
    if terminated {
        if let Some(stmt) = try_parse_eng_destructure(decl_body, line)? {
            return Ok(stmt);
        }
    }

    // --- bare parenthesized call: `name(args)` / `name(args);` -- the eng
    //     spelling of `<name> ကို လုပ်ပါ။`, e.g. `greet()`. ---
    if let Some(stmt) = try_parse_eng_call_stmt(decl_body, line)? {
        return Ok(stmt);
    }

    // --- eng statement with its ';' left off: `x :int = 5`, `print("hi")`,
    //     `x = 5`. Report the missing statement terminator rather than a
    //     generic syntax error -- whether the line ends with nothing at all
    //     or with a Myanmar '။'. ---
    if !terminated && looks_like_eng_stmt(decl_body) {
        return Err(missing_semicolon_err(line));
    }

    // --- eng library declarations (semicolon-terminated, C-style spelling):
    //     <name> :<type> = <value>;
    //     pin <name> :<type> = <value>;
    // Detected by a ';' terminator, which no Myanmar sentence form uses.
    if terminated {
        return parse_eng_decl(decl_body, line);
    }

    // --- break, spelled with the Myanmar keyword: `ရပ်ပါ။` stops the
    //     innermost loop; `<value> ကို ရပ်ပါ။` stops it with that value. ---
    // --- continue, spelled with the Myanmar keyword: `ဆက်ပါ။` skips the
    //     rest of the innermost loop body. It takes no value. ---
    if !body.is_empty() && ident_eq(&body.last().unwrap().tok, KW_CONTINUE) {
        if body.len() != 1 {
            return Err(generic_syntax_err(line));
        }
        return Ok(Stmt::Continue { line });
    }

    if !body.is_empty() && ident_eq(&body.last().unwrap().tok, KW_BREAK) {
        let fn_idx = body.len() - 1;
        if fn_idx == 0 {
            return Ok(Stmt::EngBreak { value: None, line });
        }
        let value = parse_particle_call(&body, fn_idx, KW_BREAK, line)?;
        return Ok(Stmt::EngBreak {
            value: Some(value),
            line,
        });
    }

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
                // An if or while with its leading keyword left out:
                // `(<cond>) ဖြစ်ရင် ... ပြီး။` -- the header's terminator says
                // which of the two it is.
                if let Some((idx, _)) = find_first_at_block_level(
                    &body,
                    &[
                        KW_THEN_POS,
                        KW_THEN_POS_ALT,
                        KW_THEN_NEG,
                        KW_THEN_NEG_ALT,
                        KW_WHILE_POS,
                        KW_WHILE_POS_ALT,
                        KW_WHILE_POS_ALT2,
                        KW_WHILE_NEG,
                        KW_WHILE_NEG_ALT,
                        KW_WHILE_NEG_ALT2,
                    ],
                ) {
                    if is_while_terminator(&body[idx].tok) {
                        return parse_while_statement(&body, line);
                    }
                    return parse_if_statement(&body, line);
                }
                if !body.is_empty() && is_func_def_kw(&body[0].tok) {
                    let inner = body[1..].to_vec();
                    return parse_func_def(&inner, line);
                }
                if !body.is_empty() && ident_eq(&body[0].tok, KW_CLASS_DEF) {
                    let inner = body[1..].to_vec();
                    return parse_class_def(&inner, line);
                }
                if !body.is_empty() && ident_eq(&body[0].tok, KW_TRY) {
                    let inner = body[1..].to_vec();
                    return parse_try_catch(&inner);
                }
                if !body.is_empty() && ident_eq(&body[0].tok, KW_SWITCH) {
                    let inner = body[1..].to_vec();
                    return parse_switch_statement(&inner, line);
                }
                if is_for_each_header(&body) {
                    return parse_for_each_loop(&body, line);
                }
                return parse_for_loop(&body, line);
            }
        }
    }

    // --- Myanmar one-line guard: `(<condition>) ဖြစ်ရင်။ <statement>။` --
    //     the single-statement if that needs no closing `ပြီး။` ---
    if end_present {
        if let Some(stmt) = try_parse_one_line_guard(&body, line)? {
            return Ok(stmt);
        }
    }

    // --- Library import: နည်းပညာများ <lib name>[, <lib name>, ...] ကို သုံးမည်။ ---
    if !body.is_empty() && ident_eq(&body[0].tok, KW_LIBRARIES) {
        if !end_present {
            return Err(missing_period_err(line));
        }
        let ends_with_use = ident_eq(&body.last().unwrap().tok, KW_USE)
            || ident_eq(&body.last().unwrap().tok, KW_USE_ALT);
        if ends_with_use && body.len() >= 4 && ident_eq(&body[body.len() - 2].tok, KW_PARTICLE) {
            let names_tokens = &body[1..body.len() - 2];
            let mut libs: Vec<(String, Option<String>)> = Vec::new();
            // Split on top-level commas only, so a parenthesized alias list
            // like "(r, random)" isn't torn apart by its own inner comma.
            for part in split_top_level_commas(names_tokens) {
                match part {
                    // <lib name>
                    [t] => match &t.tok {
                        Tok::Ident(s) => libs.push((s.clone(), None)),
                        _ => return Err(library_use_bad_list_err(line)),
                    },
                    // <lib name> အဖြစ် <alias>
                    [name_t, as_t, alias_t] if ident_eq(&as_t.tok, KW_AS) => {
                        match (&name_t.tok, &alias_t.tok) {
                            (Tok::Ident(lib), Tok::Ident(alias)) => {
                                libs.push((lib.clone(), Some(alias.clone())));
                            }
                            _ => return Err(library_use_alias_err(line)),
                        }
                    }
                    // <lib name> အဖြစ် (<alias1>, <alias2>, ...) -- one
                    // library registered under several aliases at once.
                    [name_t, as_t, paren_open, rest @ ..]
                        if ident_eq(&as_t.tok, KW_AS)
                            && matches!(paren_open.tok, Tok::LParen)
                            && matches!(rest.last().map(|t| &t.tok), Some(Tok::RParen)) =>
                    {
                        let lib = match &name_t.tok {
                            Tok::Ident(lib) => lib.clone(),
                            _ => return Err(library_use_alias_err(line)),
                        };
                        let inner = &rest[..rest.len() - 1];
                        if inner.is_empty() {
                            return Err(library_use_alias_err(line));
                        }
                        for alias_part in inner.split(|t| matches!(t.tok, Tok::Comma)) {
                            match alias_part {
                                [alias_t] => match &alias_t.tok {
                                    Tok::Ident(alias) => {
                                        libs.push((lib.clone(), Some(alias.clone())));
                                    }
                                    _ => return Err(library_use_alias_err(line)),
                                },
                                _ => return Err(library_use_alias_err(line)),
                            }
                        }
                    }
                    // "... အဖြစ်" with the alias swallowed by the statement's
                    // own "ကို": the alias name is missing.
                    [_, as_t] if ident_eq(&as_t.tok, KW_AS) => {
                        return Err(library_use_alias_err(line));
                    }
                    _ => return Err(library_use_bad_list_err(line)),
                }
            }
            if libs.is_empty() {
                return Err(library_use_bad_list_err(line));
            }
            return Ok(Stmt::UseLibrary { libs, line });
        }
        // `နည်းပညာများ <lib> သုံးမည်။` -- "သုံးမည်" is there but the
        // "ကို" in front of it is missing, so the sentence was never closed
        // as `<lib> ကို ...`. Reporting it by name makes the fix obvious.
        if ends_with_use {
            if let Some(names) = library_names_before_use(&body[1..body.len() - 1]) {
                return Err(library_use_missing_particle_err(line, &names));
            }
        }
        return Err(generic_syntax_err(line));
    }

    // --- Wait (sleep): <amount><unit?> ကို စောင့်ပါ။ ---
    if let Some(wait_idx) = find_kw(&body, KW_WAIT) {
        if !end_present {
            return Err(missing_period_err(line));
        }
        if wait_idx + 1 != body.len() {
            return Err(generic_syntax_err(line));
        }
        if wait_idx == 0 || !ident_eq(&body[wait_idx - 1].tok, KW_PARTICLE) {
            return Err(generic_syntax_err(line));
        }
        let value_tokens = &body[..wait_idx - 1];
        if value_tokens.is_empty() {
            return Err(generic_syntax_err(line));
        }
        // Optional unit suffix (s/m/h) fused on the amount, defaults to seconds.
        let (amount_tokens, unit) =
            if let Some(Tok::Ident(suf)) = value_tokens.last().map(|t| &t.tok) {
                match suf.as_str() {
                    "s" if value_tokens.len() >= 2 => {
                        (&value_tokens[..value_tokens.len() - 1], WaitUnit::Seconds)
                    }
                    "m" if value_tokens.len() >= 2 => {
                        (&value_tokens[..value_tokens.len() - 1], WaitUnit::Minutes)
                    }
                    "h" if value_tokens.len() >= 2 => {
                        (&value_tokens[..value_tokens.len() - 1], WaitUnit::Hours)
                    }
                    _ => (value_tokens, WaitUnit::Seconds),
                }
            } else {
                (value_tokens, WaitUnit::Seconds)
            };
        if amount_tokens.is_empty() {
            return Err(generic_syntax_err(line));
        }
        let amount = parse_expr(amount_tokens, line)?;
        return Ok(Stmt::Wait { amount, unit, line });
    }

    // --- Function alias: <fn name> အဖြစ် <alias1>, <alias2>, ... ---
    if let Some(as_idx) = find_kw(&body, KW_AS) {
        if as_idx == 0 || !matches!(body[0].tok, Tok::Ident(_)) {
            return Err(func_alias_missing_name_err(line));
        }
        if as_idx != 1 {
            // Only a single identifier is allowed before "အဖြစ်".
            return Err(func_alias_missing_name_err(line));
        }
        let original = match &body[0].tok {
            Tok::Ident(s) => s.clone(),
            _ => unreachable!(),
        };
        if !end_present {
            return Err(missing_period_err(line));
        }
        let alias_tokens = &body[as_idx + 1..];
        if alias_tokens.is_empty() {
            return Err(func_alias_missing_alias_err(line));
        }
        let parts = split_top_level(alias_tokens, |t| matches!(t, Tok::Comma));
        if parts.is_empty() {
            return Err(func_alias_missing_alias_err(line));
        }
        let mut aliases = Vec::with_capacity(parts.len());
        for p in &parts {
            match p {
                [t] => match &t.tok {
                    Tok::Ident(s) => aliases.push(s.clone()),
                    _ => return Err(func_alias_bad_alias_err(line)),
                },
                _ => return Err(func_alias_bad_alias_err(line)),
            }
        }
        return Ok(Stmt::FuncAlias {
            original,
            aliases,
            line,
        });
    }

    // --- HTTP send (POST-style):
    //     <var> အတွက် <lib> ၏ <url> သို့ <data> ကို ပို့ပါ။
    // e.g. y အတွက် request ၏ "https://api.example.com/submit" သို့ "a=1&b=2" ကို ပို့ပါ။
    if let Some(send_idx) = find_kw(&body, KW_SEND) {
        if send_idx == 0 || !ident_eq(&body[send_idx - 1].tok, KW_PARTICLE) {
            return Err(generic_syntax_err(line));
        }
        let head = &body[..send_idx - 1];
        if head.is_empty() || !matches!(head[0].tok, Tok::Ident(_)) {
            return Err(generic_syntax_err(line));
        }
        let var_name = match &head[0].tok {
            Tok::Ident(s) => s.clone(),
            _ => unreachable!(),
        };
        let for_idx = find_kw(head, KW_FOR).ok_or_else(|| generic_syntax_err(line))?;
        if for_idx != 1 {
            return Err(generic_syntax_err(line));
        }
        let rest = &head[for_idx + 1..];
        if rest.is_empty() || !matches!(rest[0].tok, Tok::Ident(_)) {
            return Err(generic_syntax_err(line));
        }
        let lib_name = match &rest[0].tok {
            Tok::Ident(s) => s.clone(),
            _ => unreachable!(),
        };
        if rest.len() < 2 || !ident_eq(&rest[1].tok, KW_OF) {
            return Err(generic_syntax_err(line));
        }
        let after_of = &rest[2..];
        let to_idx = find_kw_top_level(after_of, KW_TO).ok_or_else(|| generic_syntax_err(line))?;
        let url_tokens = &after_of[..to_idx];
        let data_tokens = &after_of[to_idx + 1..];
        if url_tokens.is_empty() || data_tokens.is_empty() {
            return Err(generic_syntax_err(line));
        }
        if !end_present {
            return Err(missing_period_err(line));
        }
        let url = parse_expr(url_tokens, line)?;
        let data = parse_expr(data_tokens, line)?;
        return Ok(Stmt::HttpSend {
            name: var_name,
            lib: lib_name,
            url,
            data,
            line,
        });
    }

    // A library call can also be the value printed directly:
    // `<lib> ၏ <fn>(args) ကို ပြပါ။`. Handle it before the statement-call
    // parser below, which otherwise expects `လုပ်ပါ` after the particle.
    if let Some(print_idx) = find_kw(&body, KW_PRINT).or_else(|| find_kw(&body, KW_PRINT_ALT)) {
        if print_idx + 1 == body.len()
            && print_idx > 0
            && ident_eq(&body[print_idx - 1].tok, KW_PARTICLE)
            && end_present
        {
            if let Some(value) = parse_myanmar_lib_call_expr(&body[..print_idx - 1], line)? {
                return Ok(Stmt::Print { value, line });
            }
        }
    }

    // --- Library function call: <lib> ၏ <fn>(<arg>) ကို လုပ်ပါ။ and its
    //     `လုပ်ရန် ... ဖြင့်` / `<var> အတွက် ...` variants. Detected by the
    //     "၏" between the library and function names, so it must be tried
    //     before the generic function-call branches below. ---
    if let Some(stmt) = try_parse_lib_call(&body, line, end_present)? {
        return Ok(stmt);
    }

    // --- Call expression in a Myanmar sentence: `<name>(<args>) ကို လုပ်ပါ။`,
    //     and its assigning form `<var> အတွက် <name>(<args>) ကို လုပ်ပါ။`.
    //     This is the `(...)` spelling of `<name> ကို လုပ်ရန် <args> ဖြင့်။`,
    //     so `<name>` may be a function, a library, or a variable bound to a
    //     first-class library function (`TOOL ...`). ---
    if end_present
        && body.len() >= 3
        && ident_eq(&body[body.len() - 1].tok, KW_CALL)
        && ident_eq(&body[body.len() - 2].tok, KW_PARTICLE)
    {
        let head = &body[..body.len() - 2];
        let (assign_name, call_tokens) = if head.len() >= 2
            && matches!(head[0].tok, Tok::Ident(_))
            && ident_eq(&head[1].tok, KW_FOR)
        {
            let name = match &head[0].tok {
                Tok::Ident(s) => s.clone(),
                _ => unreachable!(),
            };
            (Some(name), &head[2..])
        } else {
            (None, head)
        };
        // Only when the callee is written as a call -- `name(...)` or the
        // reversed `(...) name` -- so the plain `<fn> ကို လုပ်ပါ။` forms
        // handled below are left untouched.
        if call_shaped(call_tokens) {
            if let Ok(expr) = parse_expr(call_tokens, line) {
                match expr {
                    Expr::LibCall(lib, fn_name, args, cline) => {
                        return Ok(match assign_name {
                            Some(name) => Stmt::LibCallAssign {
                                name,
                                lib,
                                fn_name,
                                args,
                                line: cline,
                            },
                            None => Stmt::LibCall {
                                lib,
                                fn_name,
                                args,
                                line: cline,
                            },
                        });
                    }
                    Expr::NewObj(fn_name, args, cline) => {
                        return Ok(match assign_name {
                            Some(name) => Stmt::FuncCallAssign {
                                name,
                                fn_name,
                                args,
                                line: cline,
                            },
                            None => Stmt::ExprStmt {
                                value: Expr::NewObj(fn_name, args, cline),
                                line: cline,
                            },
                        });
                    }
                    _ => {}
                }
            }
        }
    }

    // --- Function call with the verb at the end:
    //       <fn name> ကို (<arg1>, <arg2>) ဖြင့် လုပ်ပါ။
    //     the same call as `<fn name> ကို လုပ်ရန် (<args>) ဖြင့်။`. The
    //     assigning form `<var> အတွက် <fn name> ကို (<args>) ဖြင့် လုပ်ပါ။`
    //     works too. `<fn name>` may be a function, or a name bound to a
    //     first-class library function. ---
    if end_present && body.len() >= 4 && ident_eq(&body[body.len() - 1].tok, KW_CALL) {
        let (assign_name, rest) =
            if matches!(body[0].tok, Tok::Ident(_)) && ident_eq(&body[1].tok, KW_FOR) {
                let name = match &body[0].tok {
                    Tok::Ident(s) => s.clone(),
                    _ => unreachable!(),
                };
                (Some(name), &body[2..])
            } else {
                (None, &body[..])
            };
        // `<fn name> ကို ( ... ) ဖြင့် လုပ်ပါ`
        if rest.len() >= 4
            && matches!(rest[0].tok, Tok::Ident(_))
            && ident_eq(&rest[1].tok, KW_PARTICLE)
            && matches!(rest[2].tok, Tok::LParen)
        {
            let fn_name = match &rest[0].tok {
                Tok::Ident(s) => s.clone(),
                _ => unreachable!(),
            };
            let close = find_close(rest, 2, rest[2].line)?;
            let after = &rest[close + 1..];
            if after.len() != 2
                || !ident_eq(&after[0].tok, KW_BY)
                || !ident_eq(&after[1].tok, KW_CALL)
            {
                return Err(generic_syntax_err(line));
            }
            let inner = &rest[3..close];
            let args = if inner.is_empty() {
                Vec::new()
            } else {
                parse_call_args(inner, line)?
            };
            return Ok(match assign_name {
                Some(name) => Stmt::FuncCallAssign {
                    name,
                    fn_name,
                    args,
                    line,
                },
                None => Stmt::FuncCall {
                    name: fn_name,
                    args,
                    line,
                },
            });
        }
    }

    // --- Function call with argument(s): <fn name> ကို လုပ်ရန် <argument> ဖြင့်။
    //     Multi-line, multi-argument brace form is also accepted:
    //       <fn name> ကို လုပ်ရန် { <arg1>, <arg2>, ... } ဖြင့်
    //     OR, assigned to a variable: <var> အတွက် <fn name> ကို လုပ်ရန် <argument> ဖြင့်။ ---
    if let Some(call_idx) = find_kw(&body, KW_CALL_WITH) {
        if let Some(for_idx) = find_kw(&body[..call_idx], KW_FOR) {
            // Assigned form: "<var> အတွက် <fn name> ကို လုပ်ရန် <arg(s)> ဖြင့်"
            if for_idx == 0 || !matches!(body[0].tok, Tok::Ident(_)) {
                return Err(func_call_assign_missing_var_err(line));
            }
            let var_name = match &body[0].tok {
                Tok::Ident(s) => s.clone(),
                _ => unreachable!(),
            };
            let rest = &body[for_idx + 1..];
            if rest.is_empty() || !matches!(rest[0].tok, Tok::Ident(_)) {
                return Err(func_call_assign_bad_name_err(line));
            }
            let fn_name = match &rest[0].tok {
                Tok::Ident(s) => s.clone(),
                _ => unreachable!(),
            };
            let call_idx_in_rest = find_kw(rest, KW_CALL_WITH)
                .ok_or_else(|| func_call_assign_missing_particle_err(line))?;
            if call_idx_in_rest != 2 || !ident_eq(&rest[1].tok, KW_PARTICLE) {
                return Err(func_call_assign_missing_particle_err(line));
            }
            let after_call = &rest[call_idx_in_rest + 1..];
            let by_idx = find_kw_top_level(after_call, KW_BY)
                .ok_or_else(|| func_call_missing_by_err(line))?;
            let arg_tokens = &after_call[..by_idx];
            let args = parse_call_args(arg_tokens, line)?;
            if !end_present {
                return Err(missing_period_err(line));
            }
            if by_idx + 1 != after_call.len() {
                return Err(generic_syntax_err(line));
            }
            return Ok(Stmt::FuncCallAssign {
                name: var_name,
                fn_name,
                args,
                line,
            });
        }

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
        let by_idx =
            find_kw_top_level(after_call, KW_BY).ok_or_else(|| func_call_missing_by_err(line))?;
        let arg_tokens = &after_call[..by_idx];
        let args = parse_call_args(arg_tokens, line)?;
        if !end_present {
            return Err(missing_period_err(line));
        }
        if by_idx + 1 != after_call.len() {
            return Err(generic_syntax_err(line));
        }
        return Ok(Stmt::FuncCall {
            name: fn_name,
            args,
            line,
        });
    }

    // --- Function call (no argument): <fn name> ကို လုပ်ပါ။
    //     OR, assigned to a variable: <var> အတွက် <fn name> ကို လုပ်ပါ။ ---
    if let Some(call_idx) = find_kw(&body, KW_CALL) {
        if let Some(for_idx) = find_kw(&body[..call_idx], KW_FOR) {
            // Assigned form: "<var> အတွက် <fn name> ကို လုပ်ပါ။"
            if for_idx == 0 || !matches!(body[0].tok, Tok::Ident(_)) {
                return Err(func_call_assign_missing_var_err(line));
            }
            let var_name = match &body[0].tok {
                Tok::Ident(s) => s.clone(),
                _ => unreachable!(),
            };
            let rest = &body[for_idx + 1..];
            if rest.is_empty() || !matches!(rest[0].tok, Tok::Ident(_)) {
                return Err(func_call_assign_bad_name_err(line));
            }
            let fn_name = match &rest[0].tok {
                Tok::Ident(s) => s.clone(),
                _ => unreachable!(),
            };
            let call_idx_in_rest = find_kw(rest, KW_CALL)
                .ok_or_else(|| func_call_assign_missing_particle_err(line))?;
            if call_idx_in_rest != 2 || !ident_eq(&rest[1].tok, KW_PARTICLE) {
                return Err(func_call_assign_missing_particle_err(line));
            }
            if !end_present {
                return Err(missing_period_err(line));
            }
            if call_idx_in_rest + 1 != rest.len() {
                return Err(generic_syntax_err(line));
            }
            return Ok(Stmt::FuncCallAssign {
                name: var_name,
                fn_name,
                args: Vec::new(),
                line,
            });
        }

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
            args: Vec::new(),
            line,
        });
    }

    // --- Print statement: ... ကို ဖော်ပြပါ။ ---
    if let Some(idx) = find_kw(&body, KW_PRINT).or_else(|| find_kw(&body, KW_PRINT_ALT)) {
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
            let inp_idx_in_rest =
                find_kw(rest, KW_INPUT).ok_or_else(|| generic_syntax_err(line))?;
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

    // --- The conversion sentence that used to live here ----
    //     value ကို <type> သို့ ပြောင်းပါ။
    // Conversion is now a plain function in both spellings -- `စာ(x)` /
    // `str(x)`, and the rest of `convert_hint` below -- so a line still
    // written with the old verb is diagnosed with its replacement instead of
    // falling through to the generic syntax error. `သို့` itself stays: the
    // condition operator `(သို့)` and the request library's send sentence
    // still use it.
    if find_kw(&body, KW_CONVERT).is_some() {
        let target = body
            .iter()
            .position(|t| ident_eq(&t.tok, KW_TO))
            .and_then(|to_idx| to_idx.checked_sub(1))
            .and_then(|type_idx| match &body[type_idx].tok {
                Tok::Ident(s) => convert_hint(s),
                _ => None,
            });
        return Err(convert_removed_err(line, target));
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
        let amount_start = if amount_is_neg_num {
            fn_idx - 2
        } else {
            fn_idx - 1
        };
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
                Tok::Ident(s) => Expr::Ident(Sym::new(s.clone())),
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

    // --- Self field assignment: အတု <field> သည် <value> ဖြစ်၏။ (inside a
    //     class method). ---
    if !body.is_empty() && is_self_kw(&body[0].tok) {
        // Quote back whichever spelling the program actually used.
        let self_kw = match &body[0].tok {
            Tok::Ident(s) => s.clone(),
            _ => unreachable!("is_self_kw matches identifiers only"),
        };
        if body.len() < 2 {
            return Err(generic_syntax_err(line));
        }
        let field_name = match &body[1].tok {
            Tok::Ident(s) => s.clone(),
            _ => return Err(generic_syntax_err(line)),
        };
        if body.len() < 3 || !ident_eq(&body[2].tok, KW_ASSIGN) {
            return Err(format!(
                "E001 လိုင်း {} တွင် {} {} ကို တန်ဖိုးသတ်မှတ်ရာမှာ 'သည်' လိုအပ်ပါသည်။",
                line, self_kw, field_name
            ));
        }
        let is_idx = find_kw(&body, KW_IS).ok_or_else(|| missing_period_err(line))?;
        let value_tokens = &body[3..is_idx];
        if value_tokens.is_empty() {
            return Err(format!(
                "E001 လိုင်း {} တွင် {} {} သည် တန်ဖိုးသတ်မှတ်ထားခြင်းမရှိပါ။",
                line, self_kw, field_name
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

    // --- Random number assignment: ကျပန်းကိန်း <name> သည် <min> နှင့် <max> အကြား ဖြစ်၏။ ---
    if !body.is_empty() && ident_eq(&body[0].tok, KW_RANDOM) {
        if !end_present {
            return Err(missing_period_err(line));
        }
        if body.len() < 8 {
            return Err(generic_syntax_err(line));
        }
        if !ident_eq(&body[2].tok, KW_ASSIGN) {
            return Err(generic_syntax_err(line));
        }
        let and_idx = find_kw(&body[3..], KW_AND).map(|i| i + 3);
        let between_idx = find_kw(&body[3..], KW_BETWEEN).map(|i| i + 3);

        match (and_idx, between_idx) {
            (Some(a_idx), Some(b_idx)) if a_idx < b_idx => {
                if !ident_eq(&body[b_idx + 1].tok, KW_IS) {
                    return Err(generic_syntax_err(line));
                }
                let name = match &body[1].tok {
                    Tok::Ident(s) => s.clone(),
                    _ => return Err(generic_syntax_err(line)),
                };
                let min = parse_expr(&body[3..a_idx], line)?;
                let max = parse_expr(&body[a_idx + 1..b_idx], line)?;
                return Ok(Stmt::RandomDecl {
                    name,
                    min,
                    max,
                    line,
                });
            }
            _ => return Err(generic_syntax_err(line)),
        }
    }

    // --- Variable / collection declaration: <target> (သည်|မှာ) value ဖြစ်၏။ ---
    // `<target>` is a name or an index, so `xs[0] သည် 5 ဖြစ်၏။` writes into
    // the collection exactly like the eng `xs[0] = 5;` does.
    if let Some(is_idx) = find_kw_top_level(&body, KW_IS) {
        if body.is_empty() {
            return Err(generic_syntax_err(line));
        }
        let assign_idx = find_kw_top_level(&body[..is_idx], KW_ASSIGN)
            .or_else(|| find_kw_top_level(&body[..is_idx], KW_ASSIGN_COLLECTION));
        let assign_idx = match assign_idx {
            Some(i) if i > 0 => i,
            _ => return Err(format!("E001 လိုင်း {} တွင် တန်ဖိုးသတ်မှတ်ရာမှာ 'သည်' လိုအပ်ပါသည်။", line)),
        };
        let target = parse_expr(&body[..assign_idx], line)?;
        let label = match &target {
            Expr::Ident(name) => name.to_string(),
            Expr::Index(..) => "index".to_string(),
            Expr::ListLit(_) | Expr::TupleLit(_) => "destructure".to_string(),
            _ => return Err(generic_syntax_err(line)),
        };
        let value_tokens = &body[assign_idx + 1..is_idx];
        if value_tokens.is_empty() {
            return Err(format!(
                "E001 လိုင်း {} တွင် {} သည် တန်ဖိုးသတ်မှတ်ထားခြင်းမရှိပါ။",
                line, label
            ));
        }
        if !end_present {
            return Err(missing_period_err(line));
        }
        if is_idx + 1 != body.len() {
            return Err(generic_syntax_err(line));
        }
        let value = parse_expr(value_tokens, line)?;
        return Ok(match target {
            Expr::Ident(name) => Stmt::VarDecl { name, value, line },
            // Myanmar destructuring: `[a, b] သည် [1, 2] ဖြစ်၏။` -- one name
            // per item, in order, the same statement as `[a, b] = [1, 2];`.
            Expr::ListLit(items) | Expr::TupleLit(items) => {
                if items.is_empty() {
                    return Err(myanmar_destructure_bad_form_err(line));
                }
                let mut names = Vec::with_capacity(items.len());
                for item in items {
                    match item {
                        Expr::Ident(name) => names.push(name.to_string()),
                        _ => return Err(myanmar_destructure_bad_form_err(line)),
                    }
                }
                Stmt::Destructure { names, value, line }
            }
            other => Stmt::IndexAssign {
                target: other,
                value,
                line,
            },
        });
    }

    // --- Short index assignment: `xs[i] က <value>။` ---
    // The short declaration's `က` spelling, for a collection element.
    if matches!(body.first().map(|t| &t.tok), Some(Tok::Ident(_)))
        && matches!(body.get(1).map(|t| &t.tok), Some(Tok::LBracket))
    {
        if let Ok(close) = find_close(&body, 1, line) {
            if close + 2 < body.len() && ident_eq(&body[close + 1].tok, KW_DECL) {
                if !end_present {
                    return Err(missing_period_err(line));
                }
                let target = parse_expr(&body[..close + 1], line)?;
                let value = parse_expr(&body[close + 2..], line)?;
                return Ok(Stmt::IndexAssign {
                    target,
                    value,
                    line,
                });
            }
        }
    }

    // --- Short declaration: `name က value ။` -- the same statement as
    //     `name သည် value ဖြစ်၏။`, with the `ဖြစ်၏` left out. ---
    if body.len() >= 3 && matches!(body[0].tok, Tok::Ident(_)) && ident_eq(&body[1].tok, KW_DECL) {
        if !end_present {
            return Err(missing_period_err(line));
        }
        let name = match &body[0].tok {
            Tok::Ident(s) => s.clone(),
            _ => unreachable!(),
        };
        let value = parse_expr(&body[2..], line)?;
        return Ok(Stmt::VarDecl {
            name: Sym::new(name),
            value,
            line,
        });
    }

    // --- Bare expression statement (e.g. "10 + \"10\"") ---
    if body.iter().any(|t| matches!(t.tok, Tok::Op(_))) {
        let value = parse_expr(&body, line)?;
        return Ok(Stmt::ExprStmt { value, line });
    }

    Err(generic_syntax_err(line))
}

/// Where `တစ်ခုစီ` sits in a parenthesized loop header -- `<var> အတွက်
/// တစ်ခုစီ(...)` or `<var> သည် တစ်ခုစီ(...)` -- or `None` when the statement
/// is not one. `တစ်ခုစီ` is reserved, so no other statement can hold that
/// slot.
pub(crate) fn for_each_head(tokens: &[Token]) -> Option<usize> {
    if !matches!(tokens.first().map(|t| &t.tok), Some(Tok::Ident(_))) {
        return None;
    }
    let is_header = (ident_eq_opt(tokens.get(1), KW_FOR) || ident_eq_opt(tokens.get(1), KW_ASSIGN))
        && ident_eq_opt(tokens.get(2), KW_LOOP_EACH);
    if !is_header {
        return None;
    }
    if matches!(tokens.get(3).map(|t| &t.tok), Some(Tok::LParen)) {
        Some(2)
    } else {
        None
    }
}

/// Does a block's tokens open a parenthesized for-loop header?
pub(crate) fn is_for_each_header(tokens: &[Token]) -> bool {
    for_each_head(tokens).is_some()
}

/// The argument list of a parenthesized for-loop source. The argument count
/// picks the source:
///
///   - one: a collection, or a number (which ranges from 0, exclusive)
///   - two: `<from>, <end>`, stepping by 1
///   - three: `<from>, <end>, <step>` (the step may carry its sign: `+1`)
///
/// `<end>` is exclusive and the sign of `<step>` picks the direction.
pub(crate) fn parse_for_source_args(tokens: &[Token], line: usize) -> Result<ForSource, String> {
    match split_top_level(tokens, |t| matches!(t, Tok::Comma)).as_slice() {
        [src] => Ok(ForSource::Auto(parse_expr(src, line)?)),
        [start, end] => Ok(ForSource::RangeSigned {
            start: parse_expr(start, line)?,
            end: parse_expr(end, line)?,
            step: Expr::Int(1),
        }),
        [start, end, step] => Ok(ForSource::RangeSigned {
            start: parse_expr(start, line)?,
            end: parse_expr(end, line)?,
            step: parse_expr(step, line)?,
        }),
        _ => Err(generic_syntax_err(line)),
    }
}

/// Parse a `<var> အတွက် တစ်ခုစီ(<args>)` loop, the parenthesized spelling of
/// the for loop. The argument count picks the source:
///
///   - one: a collection, or a number (which ranges from 0, exclusive)
///   - two: `<from>, <end>`, stepping by 1
///   - three: `<from>, <end>, <step>`
///
/// `<end>` is exclusive and the sign of `<step>` picks the direction, so
/// `i အတွက် တစ်ခုစီ(10, 0, -1)` counts down. The trailing "ပြီး" marker has
/// already been stripped by the caller.
pub(crate) fn parse_for_each_loop(inner: &[Token], line: usize) -> Result<Stmt, String> {
    let var_name = match &inner[0].tok {
        Tok::Ident(s) => s.clone(),
        _ => unreachable!(),
    };
    let each = for_each_head(inner).ok_or_else(|| generic_syntax_err(line))?;
    let close = find_close(inner, each + 1, line)?;
    let source = parse_for_source_args(&inner[each + 2..close], line)?;
    Ok(Stmt::ForLoop {
        var_name,
        source,
        body: parse_block(&inner[close + 1..])?,
        line,
    })
}

/// Parse a for-loop's inner tokens: the header (variable, source, optional
/// step+fn) followed by the loop body's own statement tokens. The trailing
/// "ပြီး" marker has already been stripped by the caller.
pub(crate) fn parse_for_loop(inner: &[Token], line: usize) -> Result<Stmt, String> {
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

    // `ထဲ` is the short particle; `ထဲမှ` is the longer spelling of the same
    // loop header.
    let htm_idx = find_kw_top_level(inner, KW_LOOP_FROM)
        .or_else(|| find_kw_top_level(inner, KW_INSIDE))
        .ok_or_else(|| generic_syntax_err(line))?;
    let source_tokens = &inner[2..htm_idx];

    let tcs_idx = htm_idx + 1;
    if tcs_idx >= inner.len() || !ident_eq(&inner[tcs_idx].tok, KW_LOOP_EACH) {
        return Err(generic_syntax_err(line));
    }

    // `x သည် တစ်ခုစီ(1, 2, +1)` -- the source is the parenthesized argument
    // list, the same one the `အတွက် တစ်ခုစီ(...)` header takes. It replaces
    // the value before `ထဲ`, so the two are not written together.
    if matches!(inner.get(tcs_idx + 1).map(|t| &t.tok), Some(Tok::LParen)) {
        if !source_tokens.is_empty() {
            return Err(generic_syntax_err(line));
        }
        let close = find_close(inner, tcs_idx + 1, line)?;
        let source = parse_for_source_args(&inner[tcs_idx + 2..close], line)?;
        return Ok(Stmt::ForLoop {
            var_name,
            source,
            body: parse_block(&inner[close + 1..])?,
            line,
        });
    }
    if source_tokens.is_empty() {
        return Err(generic_syntax_err(line));
    }

    // Optionally, a range step + math-fn immediately follows "တစ်ခုစီ"
    // (e.g. "1 တိုးခြင်းဖြင့်"; the step may carry its sign, "+1 ...").
    // If it's not there, this is a plain collection-iteration loop and the
    // body starts right away.
    let mut cursor = tcs_idx + 1;
    let mut range_step: Option<(Expr, char)> = None;
    if cursor < inner.len()
        && (matches!(inner[cursor].tok, Tok::Num(_))
            || matches!(inner[cursor].tok, Tok::Op('-'))
            || matches!(inner[cursor].tok, Tok::Op('+')))
    {
        // A step may carry its sign (`+2`, `-1`), so it is parsed with the
        // unary tier rather than a bare term.
        if let Ok((step_expr, next)) = parse_unary_at(inner, cursor, line) {
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

/// Parse a try/catch/finally block. `inner` has the leading `စမ်းရန်` and
/// trailing `ပြီး` already stripped by the caller.
pub(crate) fn parse_try_catch(inner: &[Token]) -> Result<Stmt, String> {
    // Find `နောက်ဆုံးတွင်` (finally) at block level.
    let finally_rel = find_first_at_block_level(inner, &[KW_FINALLY]);
    let (try_catch_tokens, finally_tokens) = match finally_rel {
        Some((idx, _)) => (&inner[..idx], Some(&inner[idx + 1..])),
        None => (inner, None),
    };

    // Find `ဖမ်းပါ` (catch) at block level within the try+catch section.
    // The catch header is "<error_name> ကို ဖမ်းပါ။", spanning the tokens
    // [<error_name>, ကို, ဖမ်းပါ, End]. `ဖမ်းပါ` sits at `idx`, so the header
    // runs from `idx-2` through `idx+1`; the try body ends at `idx-2` and the
    // catch body begins right after the header's trailing '။'.
    let catch_rel = find_first_at_block_level(try_catch_tokens, &[KW_CATCH]);
    let (catch_err, try_tokens, catch_body_tokens) = match catch_rel {
        Some((idx, _))
            if idx >= 2
                && ident_eq(&try_catch_tokens[idx - 1].tok, KW_PARTICLE)
                && try_catch_tokens.get(idx + 1).map(|t| &t.tok) == Some(&Tok::End) =>
        {
            let err_name = match &try_catch_tokens[idx - 2].tok {
                Tok::Ident(s) => s.clone(),
                _ => {
                    return Err(format!(
                        "E001 လိုင်း {} တွင် ဖမ်းသည့်အပိုင်း ထဲတွင် <error_name> မှားနေပါသည်။",
                        try_catch_tokens[idx - 2].line
                    ))
                }
            };
            // Validate the error-name itself: "E" or "E###".
            if err_name != "E" && !(err_name.len() == 4 && err_name.starts_with('E')) {
                return Err(format!(
                    "E001 လိုင်း {} တွင် ဖမ်းသည့်အပိုင်း၏ အမှား name \"{}\" မှားနေပါသည်။ (E သို့မဟုတ် E001 ကဲ့သို့ ရေးရပါမည်)",
                    try_catch_tokens[idx - 2].line,
                    err_name
                ));
            }
            let try_end = idx - 2;
            let body_start = idx + 2;
            (
                Some(err_name),
                &try_catch_tokens[..try_end],
                Some(&try_catch_tokens[body_start..]),
            )
        }
        Some((idx, _)) => (
            None,
            &try_catch_tokens[..idx],
            Some(&try_catch_tokens[idx + 1..]),
        ),
        None => (None, try_catch_tokens, None),
    };

    let try_body = parse_block(try_tokens)?;

    let (catch_var, catch_body) = match catch_body_tokens {
        Some(ct) => (catch_err.clone(), Some(parse_block(ct)?)),
        None => (None, None),
    };

    let finally_body = match finally_tokens {
        Some(ft) => Some(parse_block(ft)?),
        None => None,
    };

    Ok(Stmt::TryCatch {
        try_body,
        catch_err,
        catch_var,
        catch_body,
        finally_body,
    })
}

/// Parse the eng postfix guard, a one-line conditional with its condition
/// written after the statement it guards:
///
///   `print("x") if (age > 18);`
///
/// `body` is the statement without its `;`. Returns `None` when the chunk has
/// no trailing `if (<condition>)`.
pub(crate) fn try_parse_postfix_guard(body: &[Token], line: usize) -> Result<Option<Stmt>, String> {
    let Some(if_idx) = find_kw_top_level(body, ENG_IF) else {
        return Ok(None);
    };
    if if_idx == 0 || !matches!(body.get(if_idx + 1).map(|t| &t.tok), Some(Tok::LParen)) {
        return Ok(None);
    }
    let close = find_close(body, if_idx + 1, line)?;
    if close + 1 != body.len() {
        return Ok(None);
    }
    let cond = parse_eng_cond(&body[if_idx + 1..=close], line)?;
    // The guarded statement ends at the guard's own ';', so it gets a
    // terminator of the spelling it was written in.
    let mut guarded = body[..if_idx].to_vec();
    guarded.push(Token {
        tok: if looks_like_eng_stmt(&body[..if_idx]) {
            Tok::Semicolon
        } else {
            Tok::End
        },
        line,
    });
    let guarded = parse_stmt(&guarded, line)?;
    Ok(Some(Stmt::If {
        branches: vec![IfBranch {
            cond: Some(cond),
            negate: false,
            body: vec![guarded],
        }],
    }))
}

/// Parse the Myanmar one-line guard, the short spelling of an if whose body
/// follows its header's own `။`, so no closing `ပြီး။` is needed:
///
///   `(age > 18) ဖြစ်ရင်။ "x" ကို ပြပါ။`
///
/// The negative forms (`မဖြစ်ရင်`) guard the other way, exactly like their
/// block spelling. `body` is a statement chunk with its trailing `။` already
/// stripped; `None` means the chunk is not a guard, so the ordinary statement
/// forms still report their own errors.
pub(crate) fn try_parse_one_line_guard(
    body: &[Token],
    line: usize,
) -> Result<Option<Stmt>, String> {
    let Some((term, which)) = find_first_at_block_level(
        body,
        &[KW_THEN_POS, KW_THEN_POS_ALT, KW_THEN_NEG, KW_THEN_NEG_ALT],
    ) else {
        return Ok(None);
    };
    // A guard has a condition in front of its terminator, and only the short
    // spelling (no leading `အကယ်၍` / `အခြေအနေ`) takes this form.
    if term == 0 || ident_eq(&body[0].tok, KW_IF) || ident_eq(&body[0].tok, KW_WHILE) {
        return Ok(None);
    }
    let mut start = term + 1;
    // The header's own `။`.
    if matches!(body.get(start).map(|t| &t.tok), Some(Tok::End)) {
        start += 1;
    }
    if start >= body.len() {
        return Ok(None);
    }
    let cond = parse_cond_chain(&body[..term], line, false)?;
    let mut guarded = body[start..].to_vec();
    guarded.push(Token {
        tok: Tok::End,
        line,
    });
    Ok(Some(Stmt::If {
        branches: vec![IfBranch {
            cond: Some(cond),
            negate: which >= 2,
            body: parse_block(&guarded)?,
        }],
    }))
}

pub(crate) fn parse_block(tokens: &[Token]) -> Result<Vec<Stmt>, String> {
    let mut stmts = Vec::new();
    for (chunk, cline) in split_statements(tokens) {
        stmts.push(parse_stmt(&chunk, cline)?);
    }
    Ok(stmts)
}
