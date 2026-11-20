use super::*;

/// Parse a class definition. `inner` is everything between the leading
/// "ပုံသေ" (already stripped by the caller) and the trailing "ပြီး" (also
/// already stripped). The header is the class name, an optional parenthesized
/// constructor parameter list, and a terminator bit ("သည်" or '။'):
///   - "<class name> သည် <method definitions...>"              (no arguments)
///   - "<class name>။ <method definitions...>"                  (older form)
///   - "<class name>(<p1>, <p2>) သည် <method definitions...>"  (constructor params)
///   - "<class name>() သည် <method definitions...>"             (explicitly none)
/// The parenthesized form declares the constructor's parameters up front. The
/// first method in the body is still the constructor and runs with those
/// parameters bound, so it must not declare parameters of its own.
pub(crate) fn parse_class_def(inner: &[Token], line: usize) -> Result<Stmt, String> {
    if inner.is_empty() {
        return Err(class_missing_name_err(line));
    }
    let name = match &inner[0].tok {
        Tok::Ident(s) => s.clone(),
        _ => return Err(class_missing_name_err(line)),
    };

    let rest = &inner[1..];

    // New/preferred header: "<class name>(<p1>, <p2>) သည်". The list accepts
    // either terminator ("သည်" after `()`, or '။' for symmetry with the
    // argument-less form).
    let (params, body_tokens) = if !rest.is_empty() && matches!(rest[0].tok, Tok::LParen) {
        let close = find_close(rest, 0, line).map_err(|_| class_bad_param_err(line))?;
        let param_tokens = &rest[1..close];
        let mut params: Vec<String> = Vec::new();
        if !param_tokens.is_empty() {
            for part in split_top_level(param_tokens, |t| matches!(t, Tok::Comma)) {
                match part {
                    [p] => match &p.tok {
                        Tok::Ident(s) => params.push(s.clone()),
                        _ => return Err(class_bad_param_err(line)),
                    },
                    _ => return Err(class_bad_param_err(line)),
                }
            }
        }
        let term_idx = close + 1;
        let terminated = rest
            .get(term_idx)
            .is_some_and(|t| ident_eq(&t.tok, KW_ASSIGN) || matches!(t.tok, Tok::End));
        if !terminated {
            return Err(class_missing_period_err(line));
        }
        (Some(params), &rest[term_idx + 1..])
    } else {
        // Argument-less header: "<name> သည် ..." / "<name>။ ...".
        if rest.is_empty() {
            return Err(class_missing_period_err(line));
        }
        if !(matches!(rest[0].tok, Tok::End) || ident_eq(&rest[0].tok, KW_ASSIGN)) {
            return Err(class_missing_period_err(line));
        }
        (None, &rest[1..])
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

    Ok(Stmt::ClassDef {
        name,
        params,
        body,
        line,
    })
}
