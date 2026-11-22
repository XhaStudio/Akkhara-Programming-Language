//! Static analysis between parsing and running: the warnings a program
//! gets before it executes.

pub(crate) mod check;
pub(crate) mod error;
pub(crate) mod infer;
pub(crate) mod warnings;
