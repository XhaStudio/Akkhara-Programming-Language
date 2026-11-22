//! One token, and the kinds the lexer produces.

#[derive(Debug, Clone, PartialEq)]
pub enum Tok {
    Ident(String), // Myanmar keyword or a variable/identifier name
    Str(String),   // string literal contents (without quotes)
    Num(String),   // normalized ascii numeral text, may contain '.'
    Op(char),      // + - * / % ^ (**, exponent) \ (//, floor division)
    Cmp(String),   // < > == != <= >=
    And,           // && (eng library logical and)
    Or,            // || (eng library logical or)
    Not,           // !  (eng library logical not)
    Dot,           // .  (eng library library-call separator: `lib.fn(args);`)
    Question,      // ?  (ternary condition: `<cond> ? <then> : <else>`)
    Bool(bool),    // true / false (eng library bool literals)
    Assign,        // =  (eng library declaration assignment)
    Arrow,         // => (arrow lambda: `x => x * 2`)
    Colon,         // :  (eng library type annotation)
    Semicolon,     // ;  (eng library statement terminator)
    End,           // ။  (end of sentence)
    LBracket,      // [
    RBracket,      // ]
    LParen,        // (
    RParen,        // )
    LBrace,        // {
    RBrace,        // }
    Comma,         // ,
}

#[derive(Debug, Clone)]
pub struct Token {
    pub tok: Tok,
    pub line: usize,
}
