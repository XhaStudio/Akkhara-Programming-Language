use super::*;

pub(crate) fn is_then_pos(tok: &Tok) -> bool {
    ident_eq(tok, KW_THEN_POS) || ident_eq(tok, KW_THEN_POS_ALT)
}

pub(crate) fn is_then_neg(tok: &Tok) -> bool {
    ident_eq(tok, KW_THEN_NEG) || ident_eq(tok, KW_THEN_NEG_ALT)
}

pub(crate) fn is_while_pos(tok: &Tok) -> bool {
    ident_eq(tok, KW_WHILE_POS)
        || ident_eq(tok, KW_WHILE_POS_ALT)
        || ident_eq(tok, KW_WHILE_POS_ALT2)
}

pub(crate) fn is_while_neg(tok: &Tok) -> bool {
    ident_eq(tok, KW_WHILE_NEG)
        || ident_eq(tok, KW_WHILE_NEG_ALT)
        || ident_eq(tok, KW_WHILE_NEG_ALT2)
}

/// A keyword that closes a header and starts its body: `ဖြစ်လျှင်` /
/// `ဖြစ်ရင်` and their negative forms, plus `ဖြစ်နေစဉ်` / `ဖြစ်နေစဥ်` for a
/// while loop.
pub(crate) fn is_cond_terminator(tok: &Tok) -> bool {
    is_then_pos(tok) || is_then_neg(tok) || is_while_pos(tok) || is_while_neg(tok)
}

/// `ဖြစ်နေစဉ်` / `မဖြစ်နေစဉ်` (either spelling): the header of a while loop,
/// as opposed to the `ဖြစ်ရင်` of an if.
pub(crate) fn is_while_terminator(tok: &Tok) -> bool {
    is_while_pos(tok) || is_while_neg(tok)
}

/// The leading keyword of an if or while header. It may be left out.
pub(crate) fn is_cond_opener(tok: &Tok) -> bool {
    ident_eq(tok, KW_IF) || ident_eq(tok, KW_WHILE)
}

pub(crate) fn is_elif(tok: &Tok) -> bool {
    ident_eq(tok, KW_ELIF) || ident_eq(tok, KW_ELIF_ALT)
}

pub(crate) fn is_else(tok: &Tok) -> bool {
    ident_eq(tok, KW_ELSE) || ident_eq(tok, KW_ELSE_ALT)
}

/// `သို့မဟုတ်` / `အခြား` (else-if) and `မဟုတ်လျှင်` / `မဟုတ်ရင်` (else).
pub(crate) fn is_branch_separator(tok: &Tok) -> bool {
    is_elif(tok) || is_else(tok)
}

/// Parse one eng condition atom: an optional `!`, an optional wrapping pair
/// of parentheses, then a comparison / type-check / bare boolean.
pub(crate) fn parse_eng_cond_atom(tokens: &[Token], line: usize) -> Result<CondAtom, String> {
    let mut toks = tokens;
    let negate = !toks.is_empty() && matches!(toks[0].tok, Tok::Not);
    if negate {
        toks = &toks[1..];
    }
    if toks.len() >= 2 && matches!(toks[0].tok, Tok::LParen) {
        if let Ok(close) = find_close(toks, 0, line) {
            if close == toks.len() - 1 {
                toks = &toks[1..toks.len() - 1];
            }
        }
    }
    let mut atom = parse_cond_atom(toks, line).map_err(|_| eng_block_form_err(line))?;
    atom.negate ^= negate;
    Ok(atom)
}

/// Parse an eng condition: `<atom> [&&|\|\| <atom> ...]`, where one optional
/// outer pair of parentheses wraps the whole thing.
pub(crate) fn parse_eng_cond(tokens: &[Token], line: usize) -> Result<CondChain, String> {
    if tokens.is_empty() {
        return Err(eng_block_form_err(line));
    }
    let mut inner: &[Token] = tokens;
    if matches!(tokens[0].tok, Tok::LParen) {
        if let Ok(close) = find_close(tokens, 0, line) {
            if close == tokens.len() - 1 {
                inner = &tokens[1..tokens.len() - 1];
            }
        }
    }
    if inner.is_empty() {
        return Err(eng_block_form_err(line));
    }

    let mut parts: Vec<(Option<LogicalOp>, &[Token])> = Vec::new();
    let mut pending: Option<LogicalOp> = None;
    let mut depth = 0i32;
    let mut start = 0usize;
    for (i, t) in inner.iter().enumerate() {
        if is_open(&t.tok) {
            depth += 1;
            continue;
        }
        if is_close(&t.tok) {
            depth -= 1;
            continue;
        }
        if depth == 0 && matches!(t.tok, Tok::And | Tok::Or) {
            if i == start {
                return Err(eng_block_form_err(line));
            }
            parts.push((pending.take(), &inner[start..i]));
            pending = Some(if matches!(t.tok, Tok::And) {
                LogicalOp::And
            } else {
                LogicalOp::Or
            });
            start = i + 1;
        }
    }
    if start == inner.len() {
        return Err(eng_block_form_err(line));
    }
    parts.push((pending.take(), &inner[start..]));

    let mut atoms: Vec<(Option<LogicalOp>, CondAtom)> = Vec::with_capacity(parts.len());
    for (op, toks) in parts {
        atoms.push((op, parse_eng_cond_atom(toks, line)?));
    }
    let first = atoms[0].1.clone();
    let rest = atoms[1..]
        .iter()
        .map(|(op, atom)| (op.clone().unwrap(), atom.clone()))
        .collect();
    Ok(CondChain { first, rest })
}

/// `if (<cond>) { ... } [else if (<cond>) { ... }] [else { ... }]`
pub(crate) fn parse_eng_if(tokens: &[Token], line: usize) -> Result<Stmt, String> {
    let mut branches: Vec<IfBranch> = Vec::new();
    let mut cursor = 0usize; // points at the "if" that starts the branch
    loop {
        let rest = &tokens[cursor + 1..];
        let brace_rel = find_block_lbrace(rest).ok_or_else(|| eng_block_form_err(line))?;
        let brace_abs = cursor + 1 + brace_rel;
        let cond = parse_eng_cond(&tokens[cursor + 1..brace_abs], line)?;
        let (body_tokens, after) = take_brace_block(tokens, brace_abs, line)?;
        branches.push(IfBranch {
            cond: Some(cond),
            negate: false,
            body: parse_block(body_tokens)?,
        });
        cursor = after;
        if cursor >= tokens.len() {
            break;
        }
        if !ident_eq(&tokens[cursor].tok, ENG_ELSE) {
            return Err(eng_block_form_err(line));
        }
        cursor += 1;
        if cursor >= tokens.len() {
            return Err(eng_block_form_err(line));
        }
        if ident_eq(&tokens[cursor].tok, ENG_IF) {
            continue; // `else if (...) { ... }` -- another conditional branch
        }
        if !matches!(tokens[cursor].tok, Tok::LBrace) {
            return Err(eng_block_form_err(line));
        }
        let (body_tokens, after) = take_brace_block(tokens, cursor, line)?;
        branches.push(IfBranch {
            cond: None,
            negate: false,
            body: parse_block(body_tokens)?,
        });
        cursor = after;
        break;
    }
    if cursor != tokens.len() {
        return Err(eng_block_form_err(line));
    }
    Ok(Stmt::If { branches })
}

/// `while (<cond>) { ... }`
pub(crate) fn parse_eng_while(tokens: &[Token], line: usize) -> Result<Stmt, String> {
    let rest = &tokens[1..];
    let brace_rel = find_block_lbrace(rest).ok_or_else(|| eng_block_form_err(line))?;
    let cond = parse_eng_cond(&rest[..brace_rel], line)?;
    let (body_tokens, after) = take_brace_block(tokens, 1 + brace_rel, line)?;
    if after != tokens.len() {
        return Err(eng_block_form_err(line));
    }
    Ok(Stmt::While {
        cond,
        negate: false,
        body: parse_block(body_tokens)?,
        line,
    })
}

/// Recognize a type-check target name from the tokens after "သည်" inside a
/// condition group, e.g. "ကိန်း". The bool keyword "မှန်/မှား" contains a
/// literal '/', which the lexer tokenizes as a division operator, so it
/// actually arrives as three tokens: Ident("မှန်"), Op('/'), Ident("မှား").
pub(crate) fn parse_typecheck_name(tokens: &[Token]) -> Option<String> {
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

/// Parse a single parenthesized condition group's inner tokens. Four forms,
/// checked in this order:
///   1. Type-check: "<expr> သည် <type>", e.g. "x သည် ကိန်း".
///   2. Myanmar equality: "<expr> သည် <value>" / "<expr> က <value>", both
///      meaning `==` -- e.g. "(found က မှန်)" and "(tries သည် ၂၀)".
///   3. Comparison: "<expr> <cmp-op> <expr>", e.g. "x == 10".
///   4. Bare boolean-valued expression, e.g. "flag" (test its truthiness).
pub(crate) fn parse_cond_atom(tokens: &[Token], line: usize) -> Result<CondAtom, String> {
    // A group that is one whole logical expression -- `(a < b && b > 1)`,
    // `(!done)`, `(!(n == 2))` -- is a bare boolean atom. Splitting it at its
    // first comparison would tear the `&&` / `||` apart, so it is parsed as
    // an expression first; plain comparisons (`x == 10`) fall through to the
    // forms below, which keep their own conditions and error messages.
    if let Ok(expr) = parse_expr(tokens, line) {
        if matches!(expr, Expr::Logical(..) | Expr::Not(..)) {
            return Ok(CondAtom {
                lhs: expr,
                op: None,
                rhs: None,
                negate: false,
                type_check: None,
                line,
            });
        }
    }
    // eng library logical-not: an atom may be introduced by `!`, negating
    // its truth value. The rest of the atom parses normally.
    let negate = !tokens.is_empty() && matches!(tokens[0].tok, Tok::Not);
    let tokens = if negate { &tokens[1..] } else { tokens };
    if tokens.is_empty() {
        return Err(if_condition_syntax_err(line));
    }
    // "<lhs> က <rhs>" -- the declaration particle read as "is", compared
    // with `==`. A condition has no declaration to make, so this spelling is
    // unambiguous here.
    if let Some(eq_idx) = find_kw_top_level(tokens, KW_DECL) {
        if eq_idx == 0 || eq_idx + 1 >= tokens.len() {
            return Err(if_condition_syntax_err(line));
        }
        let lhs = parse_expr(&tokens[..eq_idx], line)?;
        let rhs = parse_expr(&tokens[eq_idx + 1..], line)?;
        return Ok(CondAtom {
            lhs,
            op: Some("==".to_string()),
            rhs: Some(rhs),
            negate,
            type_check: None,
            line,
        });
    }
    if let Some(is_idx) = find_kw_top_level(tokens, KW_ASSIGN) {
        if is_idx == 0 || is_idx + 1 >= tokens.len() {
            return Err(if_condition_syntax_err(line));
        }
        let lhs = parse_expr(&tokens[..is_idx], line)?;
        let type_tokens = &tokens[is_idx + 1..];
        if let Some(type_name) = parse_typecheck_name(type_tokens) {
            return Ok(CondAtom {
                lhs,
                op: None,
                rhs: None,
                negate,
                type_check: Some(type_name),
                line,
            });
        }
        // `x သည် <type keyword>` names a type, but not one this parser
        // knows how to check -- keep reporting the condition-syntax error
        // instead of reading the type word as a variable.
        if type_tokens.len() == 1 {
            if let Tok::Ident(name) = &type_tokens[0].tok {
                if TYPE_WORDS.contains(&name.as_str()) {
                    return Err(if_condition_syntax_err(line));
                }
            }
        }
        // Otherwise the right side is a value, so this is the
        // natural-language "<lhs> သည် <value>" equality, the same
        // comparison as `==`.
        let rhs = parse_expr(type_tokens, line)?;
        return Ok(CondAtom {
            lhs,
            op: Some("==".to_string()),
            rhs: Some(rhs),
            negate,
            type_check: None,
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
                negate,
                type_check: None,
                line,
            })
        }
        None => {
            // No comparison operator at all: treat the whole group as a
            // single boolean-valued expression, e.g. "(flag)".
            let lhs = parse_expr(tokens, line)?;
            Ok(CondAtom {
                lhs,
                op: None,
                rhs: None,
                negate,
                type_check: None,
                line,
            })
        }
    }
}

/// Parse a sequence of parenthesized conditions and parenthesized logical
/// operators, e.g. "(x == 10) (နှင့်) (y == 20)". `is_while` selects which
/// set of error messages to use (if/elif vs while wording).
pub(crate) fn parse_cond_chain(
    tokens: &[Token],
    line: usize,
    is_while: bool,
) -> Result<CondChain, String> {
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
        // Logical operators between condition groups: the eng `&&` / `||`
        // tokens, or the words `and` / `or`.
        let bare_op = match &tokens[idx].tok {
            Tok::And => Some(LogicalOp::And),
            Tok::Or => Some(LogicalOp::Or),
            Tok::Ident(s) if s == KW_AND_ALT => Some(LogicalOp::And),
            Tok::Ident(s) if s == KW_OR_ALT => Some(LogicalOp::Or),
            _ => None,
        };
        if let Some(op) = bare_op {
            if pending_op.is_some() || atoms.is_empty() {
                return Err(syntax_err(line));
            }
            pending_op = Some(op);
            idx += 1;
            continue;
        }
        if !matches!(tokens[idx].tok, Tok::LParen) {
            return Err(syntax_err(line));
        }
        let close = find_close(tokens, idx, line)?;
        let group = &tokens[idx + 1..close];

        // An operator on its own in parentheses: `(နှင့်)` / `(သို့)`, or the
        // English `(and)` / `(or)`.
        let is_and = group.len() == 1
            && (ident_eq(&group[0].tok, KW_AND) || ident_eq(&group[0].tok, KW_AND_ALT));
        let is_or = group.len() == 1
            && (ident_eq(&group[0].tok, KW_OR) || ident_eq(&group[0].tok, KW_OR_ALT));

        if is_and || is_or {
            if pending_op.is_some() || atoms.is_empty() {
                return Err(syntax_err(line));
            }
            pending_op = Some(if is_and {
                LogicalOp::And
            } else {
                LogicalOp::Or
            });
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
pub(crate) fn parse_if_statement(inner: &[Token], line: usize) -> Result<Stmt, String> {
    let mut branches: Vec<IfBranch> = Vec::new();
    let mut cursor = 0usize;

    loop {
        if !branches.is_empty() {
            if cursor >= inner.len() {
                break;
            }
            if is_else(&inner[cursor].tok) {
                cursor += 1;
                let body_stmts = parse_block(&inner[cursor..])?;
                branches.push(IfBranch {
                    cond: None,
                    negate: false,
                    body: body_stmts,
                });
                break;
            } else if is_elif(&inner[cursor].tok) {
                cursor += 1;
            } else {
                return Err(generic_syntax_err(line));
            }
        } else if cursor >= inner.len() {
            return Err(if_missing_condition_err(line));
        }

        // Parse condition-chain, terminated by "ဖြစ်လျှင်"/"ဖြစ်ရင်"
        // (positive) or "မဖြစ်လျှင်"/"မဖြစ်ရင်" (negative).
        let (term_rel, which) = find_first_at_block_level(
            &inner[cursor..],
            &[KW_THEN_POS, KW_THEN_POS_ALT, KW_THEN_NEG, KW_THEN_NEG_ALT],
        )
        .ok_or_else(|| if_missing_then_err(line))?;
        let term_abs = cursor + term_rel;
        let negate = which >= 2;
        let cond_tokens = &inner[cursor..term_abs];
        let cond = parse_cond_chain(cond_tokens, line, false)?;
        cursor = term_abs + 1;

        // Body runs until the next branch keyword at this same level, or to
        // the end of the whole if-statement.
        let body_end = match find_first_at_block_level(
            &inner[cursor..],
            &[KW_ELIF, KW_ELIF_ALT, KW_ELSE, KW_ELSE_ALT],
        ) {
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

    Ok(Stmt::If { branches })
}

/// Parse a while-loop. `inner` is everything between the leading "အခြေအနေ"
/// (already stripped by the caller) and the trailing "ပြီး" (also already
/// stripped): "(<condition...>) ဖြစ်နေစဉ်/ဖြစ်နေရင်/မဖြစ်နေစဉ်/မဖြစ်နေရင်
/// <body...>".
pub(crate) fn parse_while_statement(inner: &[Token], line: usize) -> Result<Stmt, String> {
    if inner.is_empty() {
        return Err(while_missing_condition_err(line));
    }

    let (term_idx, _) = find_first_at_block_level(
        inner,
        &[
            KW_WHILE_POS,
            KW_WHILE_POS_ALT,
            KW_WHILE_POS_ALT2,
            KW_WHILE_NEG,
            KW_WHILE_NEG_ALT,
            KW_WHILE_NEG_ALT2,
        ],
    )
    .ok_or_else(|| while_missing_then_err(line))?;
    // The `ဖြစ်` spellings loop while the condition holds; the `မဖြစ်`
    // spellings loop while it does not.
    let negate = is_while_neg(&inner[term_idx].tok);
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
