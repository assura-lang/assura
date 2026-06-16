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
