use super::*;

/// `fn <name>([<param> [:<type>], ...]) [-> <type> | : <type>] { ... }`
pub(crate) fn parse_eng_fn(tokens: &[Token], line: usize) -> Result<Stmt, String> {
    if tokens.len() < 3 || !matches!(tokens[1].tok, Tok::Ident(_)) {
        return Err(eng_block_form_err(line));
    }
    let name = match &tokens[1].tok {
        Tok::Ident(s) => s.clone(),
        _ => unreachable!(),
    };
    if !matches!(tokens[2].tok, Tok::LParen) {
        return Err(eng_block_form_err(line));
    }
    let params_close = find_close(tokens, 2, line).map_err(|_| eng_block_form_err(line))?;
    let (params, defaults) = parse_eng_fn_sig(&tokens[3..params_close], line)?;

    let mut cursor = params_close + 1;
    // Optional return-type annotation, spelled `-> int` or `: int`. It is
    // validated but not enforced (Akkhara functions are dynamically typed).
    let has_ret_annotation = if cursor < tokens.len() {
        let arrow = matches!(tokens[cursor].tok, Tok::Op('-'))
            && matches!(tokens.get(cursor + 1).map(|t| &t.tok), Some(Tok::Cmp(s)) if s == ">");
        if arrow {
            cursor += 2;
            true
        } else if matches!(tokens[cursor].tok, Tok::Colon) {
            cursor += 1;
            true
        } else {
            false
        }
    } else {
        false
    };
    if has_ret_annotation {
        match tokens.get(cursor).map(|t| &t.tok) {
            Some(Tok::Ident(t)) if is_eng_type(t) => cursor += 1,
            Some(Tok::Ident(t)) => return Err(eng_bad_type_err(line, t)),
            _ => return Err(eng_block_form_err(line)),
        }
    }

    if !matches!(tokens.get(cursor).map(|t| &t.tok), Some(Tok::LBrace)) {
        return Err(eng_block_form_err(line));
    }
    let (body_tokens, after) = take_brace_block(tokens, cursor, line)?;
    if after != tokens.len() {
        return Err(eng_block_form_err(line));
    }
    Ok(Stmt::FuncDef {
        name,
        params,
        defaults,
        body: parse_block(body_tokens)?,
        line,
    })
}

/// Parse an eng `fn` parameter list with its defaults: `p1, p2 = 1, p :int`.
/// A parameter is a name, optionally annotated `: type`, optionally given a
/// default value with `=`. Defaults have to be the trailing parameters, so a
/// call may leave them off from the right -- `fn greet(name, punct = "!")`
/// takes one argument or two.
pub(crate) fn parse_eng_fn_sig(
    tokens: &[Token],
    line: usize,
) -> Result<(Vec<String>, Vec<Option<Expr>>), String> {
    if tokens.is_empty() {
        return Ok((Vec::new(), Vec::new()));
    }
    let mut names = Vec::new();
    let mut defaults: Vec<Option<Expr>> = Vec::new();
    for part in split_top_level(tokens, |t| matches!(t, Tok::Comma)) {
        // Split off a default (`= <expr>`), if this parameter has one.
        let eq = part.iter().position(|t| matches!(t.tok, Tok::Assign));
        let (head, default) = match eq {
            Some(i) => {
                let expr =
                    parse_expr(&part[i + 1..], line).map_err(|_| eng_block_form_err(line))?;
                (&part[..i], Some(expr))
            }
            None => (part, None),
        };
        let name = match head {
            [p] => match &p.tok {
                Tok::Ident(s) => s.clone(),
                _ => return Err(eng_block_form_err(line)),
            },
            [p, colon, ty] => match (&p.tok, &colon.tok, &ty.tok) {
                (Tok::Ident(s), Tok::Colon, Tok::Ident(t)) => {
                    if !is_eng_type(t) {
                        return Err(eng_bad_type_err(line, t));
                    }
                    s.clone()
                }
                _ => return Err(eng_block_form_err(line)),
            },
            _ => return Err(eng_block_form_err(line)),
        };
        // A default before a parameter that has none could never be filled
        // by a positional argument, so it is a syntax error.
        if default.is_none() && defaults.iter().any(|d| d.is_some()) {
            return Err(eng_block_form_err(line));
        }
        names.push(name);
        defaults.push(default);
    }
    Ok((names, defaults))
}

/// Parse a function definition. `inner` is everything between the leading
/// "အလုပ်" (already stripped by the caller) and the trailing "ပြီး" (also
/// already stripped): "<fn name> [အတွက် <param1>, <param2>, ...] ဖြင့် <body...>".
/// Parse a function definition. `inner` is everything between the leading
/// "အလုပ်" (already stripped by the caller) and the trailing "ပြီး" (also
/// already stripped). Two header forms:
///   - no-argument short form: "<fn name> သည် <body...>"
///   - with parameters:        "<fn name> [အတွက် <param1>, ...] ဖြင့် <body...>"
pub(crate) fn parse_func_def(inner: &[Token], line: usize) -> Result<Stmt, String> {
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
            defaults: Vec::new(),
            body,
            line,
        });
    }

    // Parenthesized parameter list, the eng spelling of the header:
    //   "<fn name>() သည် <body...>"      -- no parameters
    //   "<fn name>(<p1>, <p2>) ဖြင့် <body...>"
    // The `()`-style list accepts either header terminator ("သည်" or
    // "ဖြင့်"), so `greet() သည်` and `greet(who) ဖြင့်` both read naturally.
    if !rest.is_empty() && matches!(rest[0].tok, Tok::LParen) {
        let close = find_close(rest, 0, line).map_err(|_| func_bad_param_err(line))?;
        // Parameters may carry a trailing default, the same `name = <value>`
        // spelling an eng `fn` header takes:
        //   လုပ်ငန်း greet(name, punct = "!") သည် ...
        let (params, defaults) =
            parse_eng_fn_sig(&rest[1..close], line).map_err(|_| func_bad_param_err(line))?;
        let term_idx = close + 1;
        let terminated = matches!(
            rest.get(term_idx).map(|t| &t.tok),
            Some(t) if ident_eq(t, KW_ASSIGN) || ident_eq(t, KW_BY)
        );
        if !terminated {
            return Err(func_missing_by_err(line));
        }
        let body = parse_block(&rest[term_idx + 1..])?;
        return Ok(Stmt::FuncDef {
            name,
            params,
            defaults,
            body,
            line,
        });
    }

    let by_idx = find_kw_top_level(rest, KW_BY).ok_or_else(|| func_missing_by_err(line))?;
    let header_tokens = &rest[..by_idx];

    let (params, defaults): (Vec<String>, Vec<Option<Expr>>) = if header_tokens.is_empty() {
        (Vec::new(), Vec::new())
    } else if ident_eq(&header_tokens[0].tok, KW_FOR) {
        let name_tokens = &header_tokens[1..];
        if name_tokens.is_empty() {
            return Err(func_bad_param_err(line));
        }
        parse_eng_fn_sig(name_tokens, line).map_err(|_| func_bad_param_err(line))?
    } else {
        return Err(func_bad_param_err(line));
    };

    let body_tokens = &rest[by_idx + 1..];
    let body = parse_block(body_tokens)?;

    Ok(Stmt::FuncDef {
        name,
        params,
        defaults,
        body,
        line,
    })
}
