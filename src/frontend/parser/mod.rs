//! The Akkhara parser: a token stream becomes `frontend::ast` nodes.
//!
//! The parser works on token slices, not one line at a time: a statement
//! ends at `။` (or `;`), and `split_statements` finds those boundaries at
//! bracket depth 0 before a statement is recognised.

mod class;
mod cond;
mod error;
mod expr;
mod func;
mod stmt;

pub(crate) use class::*;
pub(crate) use cond::*;
pub(crate) use error::*;
pub(crate) use expr::*;
pub(crate) use func::*;
pub(crate) use stmt::*;

pub(crate) use crate::frontend::ast::*;
pub(crate) use crate::frontend::lexer::keywords_en::*;
pub(crate) use crate::frontend::lexer::keywords_my::*;

use crate::frontend::lexer::{Tok, Token};
use crate::interp::Sym;

/// Is this token the keyword that starts a function definition (`အလုပ်` or
/// its older spelling `လုပ်ငန်း`)?
pub(crate) fn is_func_def_kw(tok: &Tok) -> bool {
    ident_eq(tok, KW_FUNC_DEF) || ident_eq(tok, KW_FUNC_DEF_ALT)
}

/// The self keyword.
pub(crate) fn is_self_kw(tok: &Tok) -> bool {
    ident_eq(tok, KW_SELF)
}

pub(crate) fn math_fn_op(kw: &str) -> Option<char> {
    match kw {
        MATH_FN_INC => Some('+'),
        MATH_FN_DEC => Some('-'),
        MATH_FN_MUL => Some('*'),
        MATH_FN_DIV => Some('/'),
        _ => None,
    }
}

pub(crate) fn loop_fn_op(kw: &str) -> Option<char> {
    match kw {
        LOOP_FN_INC => Some('+'),
        LOOP_FN_DEC => Some('-'),
        LOOP_FN_MUL => Some('*'),
        LOOP_FN_DIV => Some('/'),
        _ => None,
    }
}

/// Is this word one the parser owns as a keyword? A keyword never names a
/// callee, so the reversed call form (`(<args>) <name>`) must not read one as
/// the name: in `x မှာ (1, 2) ဖြစ်၏။` the word after the closing paren ends the
/// declaration, it does not call anything.
pub(crate) fn is_reserved_word(name: &str) -> bool {
    const RESERVED: &[&str] = &[
        KW_ASSIGN,
        KW_ASSIGN_COLLECTION,
        KW_IS,
        KW_PARTICLE,
        KW_DECL,
        KW_PRINT,
        KW_PRINT_ALT,
        KW_INPUT,
        KW_FOR,
        KW_TO,
        KW_CONVERT,
        KW_LOOP_FROM,
        KW_LOOP_EACH,
        KW_LOOP_END,
        KW_BREAK,
        KW_CONTINUE,
        KW_TRY,
        KW_CATCH,
        KW_FINALLY,
        KW_IF,
        KW_ELIF,
        KW_ELIF_ALT,
        KW_ELSE,
        KW_ELSE_ALT,
        KW_THEN_POS,
        KW_THEN_POS_ALT,
        KW_THEN_NEG,
        KW_THEN_NEG_ALT,
        KW_AND,
        KW_OR,
        KW_AND_ALT,
        KW_OR_ALT,
        KW_WHILE,
        KW_WHILE_POS,
        KW_WHILE_POS_ALT,
        KW_WHILE_POS_ALT2,
        KW_WHILE_NEG,
        KW_WHILE_NEG_ALT,
        KW_WHILE_NEG_ALT2,
        KW_FUNC_DEF,
        KW_FUNC_DEF_ALT,
        KW_BY,
        KW_CALL,
        KW_CALL_WITH,
        KW_CLASS_DEF,
        KW_SELF,
        KW_NEW,
        KW_USE,
        KW_LIBRARIES,
        KW_WAIT,
        KW_RANDOM,
        KW_BETWEEN,
        KW_AS,
        KW_OF,
        KW_SEND,
        MATH_FN_INC,
        MATH_FN_DEC,
        MATH_FN_MUL,
        MATH_FN_DIV,
    ];
    RESERVED.iter().any(|kw| *kw == name)
}

/// The conversion functions that replaced the `<value> ကို <type> သို့
/// ပြောင်းပါ။` sentence, keyed by the type that sentence named. Returns both
/// spellings so the removal error can show the one to use instead.
pub(crate) fn convert_hint(type_keyword: &str) -> Option<(&'static str, &'static str)> {
    match type_keyword {
        "ကိန်းပြည့်" | "int" => Some(("ကိန်းပြည့်", "int")),
        "ဒဿမကိန်း" | "ဒဿမ" | "float" => Some(("ဒဿမ", "float")),
        "စာသား" | "စာ" | "str" => Some(("စာ", "str")),
        "ဘူလ်" | "ဘူးလ်" | "bool" => Some(("ဘူးလ်", "bool")),
        "ကိန်း" | "digits" => Some(("ကိန်း", "digits")),
        _ => None,
    }
}

/// Splits a token slice on `,` tokens, but only at paren-nesting depth 0, so
/// a parenthesized group like `(r, random)` stays intact as one part instead
/// of being torn apart by its own inner comma.
pub(crate) fn split_top_level_commas(tokens: &[Token]) -> Vec<&[Token]> {
    let mut parts = Vec::new();
    let mut start = 0;
    let mut depth = 0i32;
    for (i, t) in tokens.iter().enumerate() {
        match t.tok {
            Tok::LParen => depth += 1,
            Tok::RParen => depth -= 1,
            Tok::Comma if depth == 0 => {
                parts.push(&tokens[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    parts.push(&tokens[start..]);
    parts
}

pub(crate) fn ident_eq(tok: &Tok, s: &str) -> bool {
    matches!(tok, Tok::Ident(v) if v == s)
}

/// `ident_eq` for an optional token, for lookahead checks.
pub(crate) fn ident_eq_opt(tok: Option<&Token>, s: &str) -> bool {
    matches!(tok, Some(t) if ident_eq(&t.tok, s))
}

pub(crate) fn is_open(tok: &Tok) -> bool {
    matches!(tok, Tok::LBracket | Tok::LParen | Tok::LBrace)
}

pub(crate) fn is_close(tok: &Tok) -> bool {
    matches!(tok, Tok::RBracket | Tok::RParen | Tok::RBrace)
}

/// Given the index of an opening bracket token, find the index of its
/// matching closing bracket, validating proper nesting along the way.
pub(crate) fn find_close(tokens: &[Token], open_idx: usize, line: usize) -> Result<usize, String> {
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
pub(crate) fn split_top_level<'a>(
    tokens: &'a [Token],
    is_delim: impl Fn(&Tok) -> bool,
) -> Vec<&'a [Token]> {
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
/// The index of the first `=` at the statement's own nesting level (not
/// inside `[...]`, `(...)` or `{...}`), or `None`. Recognizes an eng
/// assignment whose left side is an index, e.g. `xs[0] = 1;`.
pub(crate) fn find_top_level_assign(tokens: &[Token]) -> Option<usize> {
    let mut depth = 0i32;
    for (i, t) in tokens.iter().enumerate() {
        if is_open(&t.tok) {
            depth += 1;
        } else if is_close(&t.tok) {
            depth -= 1;
        } else if depth == 0 && matches!(t.tok, Tok::Assign) {
            return Some(i);
        }
    }
    None
}

pub(crate) fn find_kw_top_level(tokens: &[Token], kw: &str) -> Option<usize> {
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

pub(crate) fn find_kw(tokens: &[Token], kw: &str) -> Option<usize> {
    find_kw_top_level(tokens, kw)
}

/// True if `tokens` is a single balanced `{ ... }` group spanning the whole
/// slice (i.e. the opening brace's matching close is the very last token).
pub(crate) fn is_single_braced_group(tokens: &[Token]) -> bool {
    if tokens.len() < 2 || !matches!(tokens[0].tok, Tok::LBrace) {
        return false;
    }
    if !matches!(tokens.last().unwrap().tok, Tok::RBrace) {
        return false;
    }
    let mut depth = 0i32;
    for (i, t) in tokens.iter().enumerate() {
        if is_open(&t.tok) {
            depth += 1;
        } else if is_close(&t.tok) {
            depth -= 1;
            if depth == 0 {
                return i == tokens.len() - 1;
            }
        }
    }
    false
}

/// True if `tokens` is a single balanced `( ... )` group spanning the whole
/// slice (i.e. the opening paren's matching close is the very last token).
pub(crate) fn is_single_paren_group(tokens: &[Token]) -> bool {
    if tokens.len() < 2 || !matches!(tokens[0].tok, Tok::LParen) {
        return false;
    }
    if !matches!(tokens.last().unwrap().tok, Tok::RParen) {
        return false;
    }
    let mut depth = 0i32;
    for (i, t) in tokens.iter().enumerate() {
        if is_open(&t.tok) {
            depth += 1;
        } else if is_close(&t.tok) {
            depth -= 1;
            if depth == 0 {
                return i == tokens.len() - 1;
            }
        }
    }
    false
}

pub(crate) fn has_end(tokens: &[Token]) -> bool {
    matches!(tokens.last().map(|t| &t.tok), Some(Tok::End))
}

/// Bookkeeping shared by `split_statements` and `find_first_at_block_level`,
/// so both agree on where a block starts and ends. `depth` counts the nesting
/// levels that a `ပြီး` closes; `pending_header` counts headers whose leading
/// keyword has already opened its level.
///
/// A level is opened by the leading keyword (`အကယ်၍`, `အခြေအနေ`) or, when it
/// is left out, by the header's terminator (`(<cond>) ဖြစ်ရင်`), so exactly
/// one of the two counts it. An else-if / else separator marks the header as
/// pending again, which keeps the next branch's terminator neutral -- every
/// branch belongs to the same level.
pub(crate) fn advance_block_depth(
    tok: &Tok,
    depth: &mut i32,
    pending_header: &mut i32,
    neutral_terminators: bool,
) {
    if is_open(tok) {
        *depth += 1;
    } else if is_close(tok) {
        *depth -= 1;
    } else if ident_eq(tok, KW_SWITCH) {
        // A switch opens its own level, closed by its trailing `ပြီး`;
        // its arms' `ဖြစ်ရင်` terminators are neutral (see below).
        *depth += 1;
    } else if neutral_terminators && is_cond_terminator(tok) {
        // The `ဖြစ်ရင်` of a parenthesized conditional expression
        // (`(cond ဖြစ်ရင် <then> မဟုတ်ရင် <else>)`) is part of a value, and
        // the `ဖြစ်ရင်` of a switch arm selects a branch -- neither opens a
        // block of its own.
    } else if ident_eq(tok, KW_LOOP_EACH)
        || is_func_def_kw(tok)
        || ident_eq(tok, KW_CLASS_DEF)
        || ident_eq(tok, KW_TRY)
    {
        // for / function / class / try: their headers have no terminator, so
        // the keyword itself opens the level.
        *depth += 1;
    } else if is_cond_opener(tok) {
        *depth += 1;
        *pending_header += 1;
    } else if is_cond_terminator(tok) {
        if *pending_header > 0 {
            *pending_header -= 1;
        } else {
            *depth += 1;
        }
    } else if is_branch_separator(tok) {
        *pending_header += 1;
    } else if ident_eq(tok, KW_LOOP_END) {
        *depth -= 1;
        *pending_header = 0;
    }
}

pub(crate) fn split_statements(tokens: &[Token]) -> Vec<(Vec<Token>, usize)> {
    let mut stmts = Vec::new();
    let mut depth = 0i32;
    let mut pending_header = 0i32;
    let mut start = 0usize;
    // A one-line guard (`(<cond>) ဖြစ်ရင်။ <statement>။`) has no `ပြီး။` of
    // its own: its header opens a level that the guarded statement's own `။`
    // closes. `guard_levels` holds the depth of every open guard, and
    // `guard_headers` says whether that guard's header `။` was consumed.
    let mut guard_levels: Vec<i32> = Vec::new();
    let mut guard_headers: Vec<bool> = Vec::new();
    // Bracket nesting is tracked separately from block nesting: a `ဖြစ်ရင်`
    // inside `( ... )` belongs to a conditional expression, not a block.
    let mut bracket_depth = 0i32;
    // The block depth of every open `ရွေးရန်` switch. Its arms' terminators
    // are neutral, so the level stays open until the switch's own `ပြီး`.
    let mut switch_levels: Vec<i32> = Vec::new();
    for (i, t) in tokens.iter().enumerate() {
        let was_close = is_close(&t.tok);
        let neutral_terminators = bracket_depth > 0 || !switch_levels.is_empty();
        // A condition terminator written with its own `။` is a guard header,
        // not a block header: no `ပြီး။` will ever close it.
        if pending_header == 0
            && (is_then_pos(&t.tok) || is_then_neg(&t.tok))
            && matches!(tokens.get(i + 1).map(|t| &t.tok), Some(Tok::End))
        {
            guard_levels.push(depth + 1);
            guard_headers.push(false);
        }
        advance_block_depth(&t.tok, &mut depth, &mut pending_header, neutral_terminators);
        if ident_eq(&t.tok, KW_SWITCH) && bracket_depth == 0 {
            switch_levels.push(depth);
        }
        if ident_eq(&t.tok, KW_LOOP_END) {
            while switch_levels.last().is_some_and(|level| *level > depth) {
                switch_levels.pop();
            }
        }
        if is_open(&t.tok) {
            bracket_depth += 1;
        } else if was_close {
            bracket_depth -= 1;
        }
        if was_close {
            // An eng block statement (`if (...) { ... }`, `while (...) {...}`,
            // `fn ... {...}`, `loop {...}`) or a `= loop { ... }` value has no
            // ';' / '။' of its own, so its closing '}' ends the chunk. `else`
            // keeps the same chunk (an if/else chain), and a following ';' is
            // handled by the semicolon rule below.
            if depth == 0
                && matches!(t.tok, Tok::RBrace)
                && (tokens.get(start).map(|t| is_eng_block_keyword(&t.tok)) == Some(true)
                    || chunk_has_loop_block(tokens, start, i))
                && !ident_eq_opt(tokens.get(i + 1), ENG_ELSE)
                && !matches!(tokens.get(i + 1).map(|t| &t.tok), Some(Tok::Semicolon))
                // A '}' immediately followed by '{' cannot end the statement:
                // it closes a set/dict literal in an iterator for loop's
                // header (`for x in {1, 2} { ... }`), and no statement begins
                // with a bare '{'.
                && !matches!(tokens.get(i + 1).map(|t| &t.tok), Some(Tok::LBrace))
            {
                let slice = &tokens[start..=i];
                let line = slice[0].line;
                stmts.push((slice.to_vec(), line));
                start = i + 1;
            }
        } else if matches!(t.tok, Tok::End) {
            if let Some(level) = guard_levels.last().copied() {
                if depth == level {
                    if !guard_headers.last().copied().unwrap_or(true) {
                        // The `။` closing the guard's own header.
                        if let Some(header) = guard_headers.last_mut() {
                            *header = true;
                        }
                    } else {
                        // The guarded statement ends here, and with it the
                        // guard's level -- so its `။` also ends the chunk
                        // when the guard sits at the top level.
                        guard_levels.pop();
                        guard_headers.pop();
                        depth -= 1;
                        if depth == 0 {
                            let slice = &tokens[start..=i];
                            let line = slice[0].line;
                            stmts.push((slice.to_vec(), line));
                            start = i + 1;
                        }
                    }
                    continue;
                }
            }
            if depth == 0 {
                let slice = &tokens[start..=i];
                let line = slice[0].line;
                stmts.push((slice.to_vec(), line));
                start = i + 1;
            }
        } else if depth == 0 && matches!(t.tok, Tok::Semicolon) {
            // The eng library's ';' terminator: ends a statement just like
            // '။' does, so several eng declarations can sit on consecutive
            // lines without merging into one statement chunk.
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

/// True when `tokens[start..=end]` contains an eng `loop {` at bracket depth
/// 0 -- the signature of a `<var> [:<type>] = loop { ... }` declaration, whose
/// closing '}' ends the statement even without a trailing ';'.
pub(crate) fn chunk_has_loop_block(tokens: &[Token], start: usize, end: usize) -> bool {
    let mut depth = 0i32;
    for i in start..=end {
        if is_open(&tokens[i].tok) {
            depth += 1;
        } else if is_close(&tokens[i].tok) {
            depth -= 1;
        } else if depth == 0
            && ident_eq(&tokens[i].tok, ENG_LOOP)
            && matches!(tokens.get(i + 1).map(|t| &t.tok), Some(Tok::LBrace))
        {
            return true;
        }
    }
    false
}

/// Same nesting rules as `split_statements` (brackets + for/if block
/// keywords), but used to locate the first top-level occurrence of one of
/// several target keywords within an if-statement's tokens (branch
/// separators / condition terminators). Returns (index, which kws entry).
pub(crate) fn find_first_at_block_level(tokens: &[Token], kws: &[&str]) -> Option<(usize, usize)> {
    let mut depth = 0i32;
    let mut pending_header = 0i32;
    for (i, t) in tokens.iter().enumerate() {
        if depth == 0 {
            if let Tok::Ident(s) = &t.tok {
                for (ki, kw) in kws.iter().enumerate() {
                    if s == kw {
                        return Some((i, ki));
                    }
                }
            }
        }
        advance_block_depth(&t.tok, &mut depth, &mut pending_header, false);
    }
    None
}

/// `မဖြစ်ရင်` / `မဖြစ်လျှင်` written alone on its own line closes an
/// if-chain as the else branch, the same job `မဟုတ်ရင်` does:
///
/// ```text
/// (x > 3) ဖြစ်ရင်
///     "big" ကို ဖော်ပြပါ။
/// မဖြစ်ရင်
///     "small" ကို ဖော်ပြပါ။
/// ပြီး။
/// ```
///
/// The same two words are also the *negative condition terminator* when they
/// close a header (`အခြား (x > 10) မဖြစ်ရင်`), so the rewrite is only made when
/// the word stands first on its line with nothing but the line's `။` after
/// it. Rewriting it to the plain else keyword keeps one branch rule for the
/// statement splitter, the block-level scans and `parse_if_statement`.
pub(crate) fn normalize_bare_else(tokens: &mut [Token]) {
    for i in 0..tokens.len() {
        let (bare, line) = {
            let Token { tok, line } = &tokens[i];
            let Tok::Ident(word) = tok else { continue };
            if word != KW_THEN_NEG && word != KW_THEN_NEG_ALT {
                continue;
            }
            // The bare spelling has no `။` of its own, so "ends the line"
            // means: end of input, the line's own `။`, or the next token is
            // already on a later line.
            let ends_line = match tokens.get(i + 1) {
                None => true,
                Some(next) => matches!(next.tok, Tok::End) || next.line > *line,
            };
            let starts_line = i == 0 || tokens[i - 1].line < *line;
            (ends_line && starts_line, *line)
        };
        if bare {
            tokens[i] = Token {
                tok: Tok::Ident(KW_ELSE_ALT.to_string()),
                line,
            };
        }
    }
}

pub fn parse(tokens: &[Token]) -> Result<Vec<Stmt>, String> {
    let mut stmts = Vec::new();
    let mut tokens = tokens.to_vec();
    normalize_bare_else(&mut tokens);
    let tokens = &tokens[..];
    for (stmt_tokens, line) in split_statements(tokens) {
        stmts.push(parse_stmt(&stmt_tokens, line)?);
    }
    Ok(stmts)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `မဖြစ်ရင်` on a line of its own is the chain's else branch, not the
    /// negative terminator of a header.
    #[test]
    fn bare_neg_else_line_closes_an_if_chain() {
        let src = "x သည် 50 ဖြစ်၏။\n\
                   (x > 3) ဖြစ်ရင်\n\
                       \"big\" ကို ဖော်ပြပါ။\n\
                   အခြား (x > 10) မဖြစ်ရင်\n\
                       \"huge\" ကို ဖော်ပြပါ။\n\
                   မဖြစ်ရင်\n\
                       \"small\" ကို ဖော်ပြပါ။\n\
                   ပြီး။\n";
        let stmts =
            parse(&crate::frontend::lexer::lex(src).expect("lexes")).expect("the chain parses");
        let Some(Stmt::If { branches }) = stmts.last() else {
            panic!("last statement should be the if chain: {:?}", stmts.last());
        };
        assert_eq!(branches.len(), 3, "positive, else-if and else");
        assert!(branches[0].cond.is_some() && !branches[0].negate);
        assert!(
            branches[1].cond.is_some() && branches[1].negate,
            "the မဖြစ်ရင် header branch"
        );
        assert!(
            branches[2].cond.is_none(),
            "the bare မဖြစ်ရင် line is the else"
        );
        assert_eq!(branches[2].body.len(), 1);
    }

    /// The same two words still close a header: `အခြား (c) မဖြစ်ရင်` keeps its
    /// condition and stays a negated branch, and an if-chain without a bare
    /// `မဖြစ်ရင်` line is untouched.
    #[test]
    fn neg_term_inside_a_header_is_not_an_else() {
        let src = "(x > 3) ဖြစ်ရင်\n\
                       \"big\" ကို ဖော်ပြပါ။\n\
                   အခြား (x > 10) မဖြစ်ရင်\n\
                       \"huge\" ကို ဖော်ပြပါ။\n\
                   မဟုတ်ရင်\n\
                       \"small\" ကို ဖော်ပြပါ။\n\
                   ပြီး။\n";
        let stmts =
            parse(&crate::frontend::lexer::lex(src).expect("lexes")).expect("the chain parses");
        let Some(Stmt::If { branches }) = stmts.last() else {
            panic!("last statement should be the if chain");
        };
        assert_eq!(branches.len(), 3);
        assert!(branches[1].cond.is_some() && branches[1].negate);
        assert!(branches[2].cond.is_none());
    }
}
