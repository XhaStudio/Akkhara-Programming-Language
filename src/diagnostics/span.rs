//! Planned: a source span (line, later column and length).
//!
//! Tokens carry a line today (`lexer::Token::line`) and every message prints
//! it; a span will add the column and the length a caret needs.
