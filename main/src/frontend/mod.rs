//! The front end: text becomes tokens (`lexer`) and tokens become
//! syntax-tree nodes (`parser`, `ast`).

pub(crate) mod ast;
pub(crate) mod lexer;
pub(crate) mod parser;
