//! The Akkhara abstract syntax tree.
//!
//! One node set serves both spellings; the parser decides which node a
//! statement is, and nothing downstream asks how it was written.

mod expr;
mod stmt;

pub(crate) use expr::*;
pub(crate) use stmt::*;

use crate::shared::sym::Sym;
