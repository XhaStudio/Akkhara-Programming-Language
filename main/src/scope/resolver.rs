//! The pass that ties each name to its declaration.
//!
//! `ir::lower` asks this for a `VarId` as it walks the tree, so no second walk
//! is needed. Frames model functions: frame 0 is the top-level statements, and
//! every function or lambda body gets the next index. Reading a name from an
//! enclosing frame marks it captured, the way the interpreter's lambda copies
//! the value at creation.

use super::capture::{self, Capture};
use super::error::Unresolved;
use super::symbol::{SymbolKind, SymbolTable, VarId};
use crate::types::Type;

/// One function scope.
#[derive(Debug)]
pub(crate) struct Frame {
    /// The parameters, in declaration order.
    pub(crate) params: Vec<VarId>,
    /// The names from enclosing frames this function reads.
    pub(crate) captures: Vec<Capture>,
}

#[derive(Debug)]
pub(crate) struct Resolver {
    table: SymbolTable,
    stack: Vec<Frame>,
    unresolved: Vec<Unresolved>,
}

impl Resolver {
    pub(crate) fn new() -> Resolver {
        Resolver {
            table: SymbolTable::default(),
            stack: vec![Frame {
                params: Vec::new(),
                captures: Vec::new(),
            }],
            unresolved: Vec::new(),
        }
    }

    /// The frame being filled: `0` for the top-level statements.
    pub(crate) fn depth(&self) -> usize {
        self.stack.len() - 1
    }

    /// Declare `name` in the current frame.
    pub(crate) fn declare(
        &mut self,
        name: &str,
        kind: SymbolKind,
        line: usize,
        ty: Option<Type>,
    ) -> VarId {
        let frame = self.depth();
        let id = self.table.declare(name, kind, line, frame, ty);
        if kind == SymbolKind::Param && !self.stack[frame].params.contains(&id) {
            self.stack[frame].params.push(id);
        }
        id
    }

    /// The binding `name` refers to, if a declaration is in scope. A binding
    /// found in an enclosing function is marked captured.
    pub(crate) fn resolve(&mut self, name: &str) -> Option<VarId> {
        let current = self.depth();
        for frame in (0..=current).rev() {
            if let Some(id) = self.table.find_in_frame(name, frame) {
                if frame != current {
                    self.table.get_mut(id).captured = true;
                    capture::record(&mut self.stack[current].captures, id, frame);
                }
                return Some(id);
            }
        }
        None
    }

    /// The binding a name is written to: the existing one when the program
    /// declared it, a new one when it assigns a name it never declared (the
    /// interpreter reports that at run time, so the IR cannot know better).
    pub(crate) fn declare_or_resolve(&mut self, name: &str, line: usize) -> VarId {
        match self.resolve(name) {
            Some(id) => id,
            None => self.declare(name, SymbolKind::Var, line, None),
        }
    }

    /// Note a name that stays dynamic, once.
    pub(crate) fn note_unresolved(&mut self, name: &str, line: usize) {
        if self.unresolved.iter().any(|known| known.name == name) {
            return;
        }
        self.unresolved.push(Unresolved {
            name: name.to_string(),
            line,
        });
    }

    /// Enter a function (or lambda) body; its parameters are declared in the
    /// new frame.
    pub(crate) fn enter(&mut self, params: &[String], line: usize) {
        self.stack.push(Frame {
            params: Vec::new(),
            captures: Vec::new(),
        });
        for param in params {
            self.declare(param, SymbolKind::Param, line, None);
        }
    }

    /// Leave the current function body, handing back its parameters and
    /// captures.
    pub(crate) fn leave(&mut self) -> Frame {
        self.stack.pop().expect("leave() called without enter()")
    }

    pub(crate) fn into_parts(self) -> (SymbolTable, Vec<Unresolved>) {
        (self.table, self.unresolved)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_parameter_is_a_binding_of_its_own_frame() {
        let mut resolver = Resolver::new();
        resolver.enter(&["a".to_string(), "b".to_string()], 1);
        let params = resolver.leave().params;
        assert_eq!(params.len(), 2);
        assert_ne!(params[0], params[1]);
    }

    #[test]
    fn an_inner_name_is_a_capture_not_a_local() {
        let mut resolver = Resolver::new();
        let outer = resolver.declare("x", SymbolKind::Var, 1, None);
        resolver.enter(&[], 2);
        assert_eq!(resolver.resolve("x"), Some(outer));
        let frame = resolver.leave();
        assert_eq!(
            frame.captures,
            vec![Capture {
                var: outer,
                from: 0
            }]
        );
        let symbol = resolver
            .table
            .iter()
            .find(|symbol| symbol.id == outer)
            .expect("x");
        assert!(symbol.captured);
    }

    #[test]
    fn an_unknown_name_resolves_to_nothing() {
        let mut resolver = Resolver::new();
        assert_eq!(resolver.resolve("print"), None);
    }
}
