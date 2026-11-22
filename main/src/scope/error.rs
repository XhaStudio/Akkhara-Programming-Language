//! Names the IR could not tie to a declaration.
//!
//! These are not errors: Akkhara is dynamically typed, and a name may be a
//! builtin, a library function, a class or a value created at run time. They
//! are collected so the `akk ir` dump can print exactly which names stay
//! dynamic, instead of pretending they were resolved.

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Unresolved {
    pub(crate) name: String,
    pub(crate) line: usize,
}

impl std::fmt::Display for Unresolved {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} (line {})", self.name, self.line)
    }
}
