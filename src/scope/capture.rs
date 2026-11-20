//! Closure captures: a nested function reading a name from an enclosing one.
//!
//! `interp` copies the value when the lambda is created, so a capture is a
//! variable read, not a reference -- `Capture::from` records which frame the
//! value comes from. Recording them during resolution means the IR can tell a
//! local read from a capture without walking the tree a second time.

use super::symbol::VarId;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Capture {
    pub(crate) var: VarId,
    /// The frame index the binding lives in; always below the capturing one.
    pub(crate) from: usize,
}

impl std::fmt::Display for Capture {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}<-{}", self.var, self.from)
    }
}

/// Record `var` as a capture of the frame that owns `captures`, once.
pub(crate) fn record(captures: &mut Vec<Capture>, var: VarId, from: usize) {
    if !captures.iter().any(|capture| capture.var == var) {
        captures.push(Capture { var, from });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_capture_is_recorded_once() {
        let mut captures = Vec::new();
        record(&mut captures, VarId(3), 0);
        record(&mut captures, VarId(3), 0);
        record(&mut captures, VarId(4), 0);
        assert_eq!(captures.len(), 2);
        assert_eq!(
            captures[0],
            Capture {
                var: VarId(3),
                from: 0
            }
        );
    }
}
