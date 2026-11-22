//! A symbol: a variable name that remembers its slot.
//!
//! This language has one flat variable table: a function's parameters and
//! locals are written into it and undone by the frame's log, not kept in a
//! per-call scope. The same name therefore always means the same slot, so
//! resolving it once per occurrence -- the first time that node runs -- turns
//! every later read and write into a `Vec` index instead of a hash lookup,
//! without changing which binding the name refers to.
//!
//! It lives here, not in the interpreter, because the front end builds and
//! resolves names and the interpreter is only one consumer of them.

use std::cell::Cell;

/// "No slot": a map-mode log entry has no slot to restore, and a `Sym` that
/// has not been resolved yet carries it.
pub(crate) const NO_SLOT: u32 = u32::MAX;

/// A variable name that remembers its slot.
#[derive(Debug, Clone)]
pub(crate) struct Sym {
    name: std::sync::Arc<str>,
    pub(crate) slot: Cell<u32>,
}

impl Sym {
    pub(crate) fn new(name: impl Into<std::sync::Arc<str>>) -> Sym {
        Sym {
            name: name.into(),
            slot: Cell::new(NO_SLOT),
        }
    }

    #[inline]
    pub(crate) fn as_str(&self) -> &str {
        &self.name
    }
}

impl std::ops::Deref for Sym {
    type Target = str;

    fn deref(&self) -> &str {
        &self.name
    }
}

impl std::fmt::Display for Sym {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.name)
    }
}

impl PartialEq for Sym {
    fn eq(&self, other: &Sym) -> bool {
        self.name == other.name
    }
}

impl PartialEq<str> for Sym {
    fn eq(&self, other: &str) -> bool {
        &*self.name == other
    }
}

impl PartialEq<&str> for Sym {
    fn eq(&self, other: &&str) -> bool {
        &*self.name == *other
    }
}

impl From<String> for Sym {
    fn from(name: String) -> Sym {
        Sym::new(name)
    }
}

impl From<&str> for Sym {
    fn from(name: &str) -> Sym {
        Sym::new(name)
    }
}
