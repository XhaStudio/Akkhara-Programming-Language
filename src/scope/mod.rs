//! Name resolution -- which declaration a name refers to, and which bindings a
//! nested function captures.
//!
//! The interpreter resolves a name at run time against one flat slot table
//! with a per-call undo log (`interp::Env`), and a lambda captures by value
//! when it is created. `ir::lower` needs the static version of that: the same
//! name in the same function means the same `VarId`, so an instruction can
//! address a binding instead of a string.
//!
//! The resolver models what the interpreter does, not what a stricter language
//! would: a name redeclared inside one function is the *same* binding (the
//! flat table has one slot for it), and a name it cannot tie to a declaration
//! is not an error -- it may be a builtin, a library function, a class or a
//! value that only exists at run time. Those are collected as unresolved so
//! the dump can show exactly what stayed dynamic.

pub(crate) mod capture;
pub(crate) mod error;
pub(crate) mod resolver;
pub(crate) mod symbol;
