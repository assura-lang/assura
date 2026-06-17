//! Symbol table data structures and helpers.

use std::collections::HashMap;

use assura_parser::ast::Span;

use crate::errors::ResolutionError;

/// A resolved symbol in the symbol table.
#[derive(Debug, Clone)]
pub struct Symbol {
    pub kind: SymbolKind,
    pub name: String,
    pub span: Span,
    pub scope_id: usize,
}

/// The kind of a symbol registered in the symbol table.
#[derive(Debug, Clone, PartialEq)]
pub enum SymbolKind {
    TypeDef,
    ContractDef,
    ServiceDef,
    FnDef,
    EnumDef,
    ExternFn,
    BindFn,
    Prophecy,
    CodecRegistry,
    BuiltinType,
    Operation,
    Query,
    Parameter,
    TypeParam,
    Field,
    EnumVariant,
}

/// A lexical scope that maps names to symbol indices.
#[derive(Debug, Clone)]
pub struct Scope {
    pub name: String,
    pub parent: Option<usize>,
    /// Maps symbol name -> index in `SymbolTable::symbols`.
    pub symbols: HashMap<String, usize>,
}

/// The central symbol table built by the resolver.
#[derive(Debug, Clone)]
pub struct SymbolTable {
    pub symbols: Vec<Symbol>,
    pub scopes: Vec<Scope>,
}

impl SymbolTable {
    pub(crate) fn new() -> Self {
        Self {
            symbols: Vec::new(),
            scopes: Vec::new(),
        }
    }

    /// Create a new scope, returning its index.
    pub(crate) fn push_scope(&mut self, name: &str, parent: Option<usize>) -> usize {
        let id = self.scopes.len();
        self.scopes.push(Scope {
            name: name.to_string(),
            parent,
            symbols: HashMap::new(),
        });
        id
    }

    /// Insert a symbol into a scope. Returns `Err` with the existing
    /// symbol's span if a duplicate is detected.
    pub(crate) fn insert(
        &mut self,
        scope_id: usize,
        name: &str,
        kind: SymbolKind,
        span: Span,
    ) -> Result<usize, Span> {
        if let Some(&existing_idx) = self.scopes[scope_id].symbols.get(name) {
            return Err(self.symbols[existing_idx].span.clone());
        }
        let idx = self.symbols.len();
        self.symbols.push(Symbol {
            kind,
            name: name.to_string(),
            span,
            scope_id,
        });
        self.scopes[scope_id].symbols.insert(name.to_string(), idx);
        Ok(idx)
    }

    /// Total number of symbols (including built-ins).
    pub fn len(&self) -> usize {
        self.symbols.len()
    }

    /// Returns `true` if the table contains no symbols.
    pub fn is_empty(&self) -> bool {
        self.symbols.is_empty()
    }

    /// Look up a name starting from `scope_id`, walking up the scope
    /// chain until the name is found or the root is reached.
    pub fn lookup(&self, name: &str, scope_id: usize) -> Option<&Symbol> {
        let mut current = Some(scope_id);
        while let Some(id) = current {
            if let Some(&sym_idx) = self.scopes[id].symbols.get(name) {
                return Some(&self.symbols[sym_idx]);
            }
            current = self.scopes[id].parent;
        }
        None
    }
}

/// Try to insert a symbol; on duplicate, push an A02003 error.
/// Returns `true` if the symbol was inserted successfully.
pub(crate) fn try_insert(
    table: &mut SymbolTable,
    errors: &mut Vec<ResolutionError>,
    scope_id: usize,
    name: &str,
    kind: SymbolKind,
    span: Span,
) -> bool {
    match table.insert(scope_id, name, kind, span.clone()) {
        Ok(_) => true,
        Err(prev_span) => {
            errors.push(ResolutionError {
                code: "A02003".into(),
                message: format!("duplicate definition of `{name}`"),
                span,
                secondary: Some((prev_span, format!("`{name}` previously defined here"))),
                suggestion: None,
            });
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_table_is_empty() {
        let table = SymbolTable::new();
        assert!(table.is_empty());
        assert_eq!(table.len(), 0);
    }

    #[test]
    fn push_scope_returns_sequential_ids() {
        let mut table = SymbolTable::new();
        let s0 = table.push_scope("root", None);
        let s1 = table.push_scope("child", Some(s0));
        assert_eq!(s0, 0);
        assert_eq!(s1, 1);
        assert_eq!(table.scopes.len(), 2);
        assert_eq!(table.scopes[1].parent, Some(0));
    }

    #[test]
    fn insert_and_lookup() {
        let mut table = SymbolTable::new();
        let scope = table.push_scope("root", None);
        let idx = table
            .insert(scope, "x", SymbolKind::Parameter, 0..5)
            .unwrap();
        assert_eq!(idx, 0);
        assert_eq!(table.len(), 1);
        let sym = table.lookup("x", scope).unwrap();
        assert_eq!(sym.name, "x");
        assert_eq!(sym.kind, SymbolKind::Parameter);
        assert_eq!(sym.span, 0..5);
    }

    #[test]
    fn insert_duplicate_returns_err() {
        let mut table = SymbolTable::new();
        let scope = table.push_scope("root", None);
        table.insert(scope, "x", SymbolKind::FnDef, 0..3).unwrap();
        let err = table.insert(scope, "x", SymbolKind::FnDef, 10..13);
        assert!(err.is_err());
        // The error contains the span of the first definition
        assert_eq!(err.unwrap_err(), 0..3);
    }

    #[test]
    fn lookup_walks_scope_chain() {
        let mut table = SymbolTable::new();
        let root = table.push_scope("root", None);
        table
            .insert(root, "global", SymbolKind::TypeDef, 0..6)
            .unwrap();
        let child = table.push_scope("child", Some(root));
        table
            .insert(child, "local", SymbolKind::Parameter, 10..15)
            .unwrap();

        // Child can see both local and parent symbols
        assert!(table.lookup("local", child).is_some());
        assert!(table.lookup("global", child).is_some());

        // Root cannot see child symbols
        assert!(table.lookup("local", root).is_none());
        assert!(table.lookup("global", root).is_some());
    }

    #[test]
    fn lookup_child_shadows_parent() {
        let mut table = SymbolTable::new();
        let root = table.push_scope("root", None);
        table.insert(root, "x", SymbolKind::TypeDef, 0..1).unwrap();
        let child = table.push_scope("child", Some(root));
        table
            .insert(child, "x", SymbolKind::Parameter, 10..11)
            .unwrap();

        let sym = table.lookup("x", child).unwrap();
        assert_eq!(sym.kind, SymbolKind::Parameter);
        assert_eq!(sym.span, 10..11);
    }

    #[test]
    fn lookup_nonexistent_returns_none() {
        let mut table = SymbolTable::new();
        let scope = table.push_scope("root", None);
        assert!(table.lookup("missing", scope).is_none());
    }

    #[test]
    fn try_insert_success() {
        let mut table = SymbolTable::new();
        let mut errors = vec![];
        let scope = table.push_scope("root", None);
        assert!(try_insert(
            &mut table,
            &mut errors,
            scope,
            "f",
            SymbolKind::FnDef,
            0..1
        ));
        assert!(errors.is_empty());
        assert_eq!(table.len(), 1);
    }

    #[test]
    fn try_insert_duplicate_pushes_error() {
        let mut table = SymbolTable::new();
        let mut errors = vec![];
        let scope = table.push_scope("root", None);
        try_insert(&mut table, &mut errors, scope, "f", SymbolKind::FnDef, 0..1);
        let ok = try_insert(&mut table, &mut errors, scope, "f", SymbolKind::FnDef, 5..6);
        assert!(!ok);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, "A02003");
        assert!(errors[0].message.contains("duplicate"));
    }

    #[test]
    fn symbol_kind_variants_eq() {
        assert_eq!(SymbolKind::ContractDef, SymbolKind::ContractDef);
        assert_ne!(SymbolKind::FnDef, SymbolKind::ExternFn);
    }

    #[test]
    fn deep_scope_chain_lookup() {
        let mut table = SymbolTable::new();
        let s0 = table.push_scope("root", None);
        table
            .insert(s0, "root_sym", SymbolKind::TypeDef, 0..1)
            .unwrap();
        let s1 = table.push_scope("level1", Some(s0));
        let s2 = table.push_scope("level2", Some(s1));
        let s3 = table.push_scope("level3", Some(s2));

        // Deepest scope can see root symbol
        let sym = table.lookup("root_sym", s3).unwrap();
        assert_eq!(sym.name, "root_sym");
    }
}
