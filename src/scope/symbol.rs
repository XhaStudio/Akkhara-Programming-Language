//! A binding: its name, kind, declaration site and type.

use crate::types::Type;

/// Which declaration introduced a name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SymbolKind {
    /// `<name> သည် <value> ဖြစ်၏။` / `name = value;` / `name :int = 5;`
    Var,
    /// A function or lambda parameter.
    Param,
    /// The variable a `for` loop binds.
    LoopVar,
}

impl SymbolKind {
    /// The word the `akk ir` dump prints.
    pub(crate) fn label(self) -> &'static str {
        match self {
            SymbolKind::Var => "var",
            SymbolKind::Param => "param",
            SymbolKind::LoopVar => "loop",
        }
    }
}

/// A binding's identity.
///
/// Two occurrences of one name inside one function share a `VarId`; the same
/// name inside another function is a different binding with a different id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) struct VarId(pub(crate) u32);

impl std::fmt::Display for VarId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "#{}", self.0)
    }
}

/// One declared name.
#[derive(Debug, Clone)]
pub(crate) struct Symbol {
    pub(crate) id: VarId,
    pub(crate) name: String,
    pub(crate) kind: SymbolKind,
    /// The line of the first declaration the resolver saw.
    pub(crate) line: usize,
    /// `0` for the top-level statements, `1` for the first nested function.
    pub(crate) frame: usize,
    /// The type an eng declaration wrote (`name :int = 5;`), if any.
    pub(crate) ty: Option<Type>,
    /// True once a nested function reads it: the binding is a capture.
    pub(crate) captured: bool,
}

/// Every binding the resolver created, in creation order.
#[derive(Debug, Default)]
pub(crate) struct SymbolTable {
    symbols: Vec<Symbol>,
}

impl SymbolTable {
    /// Declare `name` in `frame`. A name already declared in that frame is the
    /// same binding, so the existing id is returned -- redeclaration rebinds
    /// one slot, which is what the interpreter's flat table does too.
    pub(crate) fn declare(
        &mut self,
        name: &str,
        kind: SymbolKind,
        line: usize,
        frame: usize,
        ty: Option<Type>,
    ) -> VarId {
        if let Some(existing) = self.find_in_frame(name, frame) {
            let symbol = &mut self.symbols[existing.0 as usize];
            symbol.kind = kind;
            if ty.is_some() {
                symbol.ty = ty;
            }
            return existing;
        }
        let id = VarId(self.symbols.len() as u32);
        self.symbols.push(Symbol {
            id,
            name: name.to_string(),
            kind,
            line,
            frame,
            ty,
            captured: false,
        });
        id
    }

    /// The binding `name` names in `frame`, if that frame declared it.
    pub(crate) fn find_in_frame(&self, name: &str, frame: usize) -> Option<VarId> {
        self.symbols
            .iter()
            .rev()
            .find(|symbol| symbol.frame == frame && symbol.name == name)
            .map(|symbol| symbol.id)
    }

    pub(crate) fn get_mut(&mut self, id: VarId) -> &mut Symbol {
        &mut self.symbols[id.0 as usize]
    }

    pub(crate) fn len(&self) -> usize {
        self.symbols.len()
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = &Symbol> {
        self.symbols.iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_name_in_one_frame_is_one_binding() {
        let mut table = SymbolTable::default();
        let first = table.declare("x", SymbolKind::Var, 1, 0, None);
        let again = table.declare("x", SymbolKind::Var, 4, 0, None);
        assert_eq!(first, again);
        assert_eq!(table.len(), 1);
    }

    #[test]
    fn the_same_name_in_another_frame_is_another_binding() {
        let mut table = SymbolTable::default();
        let outer = table.declare("x", SymbolKind::Var, 1, 0, None);
        let inner = table.declare("x", SymbolKind::Param, 1, 1, None);
        assert_ne!(outer, inner);
        assert_eq!(table.len(), 2);
        assert_eq!(table.find_in_frame("x", 1), Some(inner));
    }

    #[test]
    fn a_declared_type_is_remembered() {
        let mut table = SymbolTable::default();
        let id = table.declare("n", SymbolKind::Var, 7, 0, Some(Type::Int));
        let symbol = table.iter().find(|symbol| symbol.id == id).expect("n");
        assert_eq!(symbol.ty, Some(Type::Int));
    }
}
