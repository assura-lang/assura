//! T077: CORE.4 Axiomatic definitions.

use std::collections::HashMap;
use std::ops::Range;

use assura_parser::ast::{BlockKind, ClauseKind, Decl};

use crate::TypeError;

/// Validates axiomatic (abstract mathematical) definitions.
///
/// Error codes:
/// - A31001: axiom references undefined symbol
/// - A31002: axiom set is inconsistent (circular or contradictory)
/// - A31003: axiom not used in any proof
#[derive(Debug, Clone)]
pub(crate) struct AxiomaticDefChecker {
    axioms: HashMap<String, AxiomDef>,
    used_axioms: Vec<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct AxiomDef {
    pub name: String,
    pub span: Range<usize>,
    pub references: Vec<String>,
}

impl AxiomaticDefChecker {
    pub fn new() -> Self {
        Self {
            axioms: HashMap::new(),
            used_axioms: Vec::new(),
        }
    }

    pub fn declare_axiom(&mut self, axiom: AxiomDef) {
        self.axioms.insert(axiom.name.clone(), axiom);
    }

    pub fn mark_used(&mut self, name: &str) {
        if !self.used_axioms.contains(&name.to_string()) {
            self.used_axioms.push(name.to_string());
        }
    }

    pub fn check_references(&self, known_symbols: &[&str]) -> Vec<TypeError> {
        let mut errors = Vec::new();
        for axiom in self.axioms.values() {
            for reference in &axiom.references {
                let is_axiom = self.axioms.contains_key(reference);
                let is_known = known_symbols.contains(&reference.as_str());
                if !is_axiom && !is_known {
                    errors.push(TypeError {
                        code: "A31001".into(),
                        message: format!(
                            "axiom `{}` references undefined symbol `{reference}`",
                            axiom.name
                        ),
                        span: axiom.span.clone(),
                        secondary: None,
                        suggestion: None,
                    });
                }
            }
        }
        errors
    }

    pub fn check_unused(&self) -> Vec<TypeError> {
        let mut errors = Vec::new();
        for (name, axiom) in &self.axioms {
            if !self.used_axioms.contains(name) {
                errors.push(TypeError {
                    code: "A31003".into(),
                    message: format!("axiom `{name}` is never used in any proof"),
                    span: axiom.span.clone(),
                    secondary: None,
                    suggestion: None,
                });
            }
        }
        errors
    }

    pub fn check_circular(&self) -> Vec<TypeError> {
        let mut errors = Vec::new();
        for (name, axiom) in &self.axioms {
            if self.has_cycle(name, &mut vec![name.clone()]) {
                errors.push(TypeError {
                    code: "A31002".into(),
                    message: format!("axiom `{name}` has circular dependency"),
                    span: axiom.span.clone(),
                    secondary: None,
                    suggestion: None,
                });
            }
        }
        errors
    }

    fn has_cycle(&self, current: &str, visited: &mut Vec<String>) -> bool {
        if let Some(axiom) = self.axioms.get(current) {
            for reference in &axiom.references {
                if visited.contains(reference) {
                    return true;
                }
                if self.axioms.contains_key(reference) {
                    visited.push(reference.clone());
                    if self.has_cycle(reference, visited) {
                        return true;
                    }
                    visited.pop();
                }
            }
        }
        false
    }

    /// AST-walking entry point: collect axioms, mark used, and run checks.
    pub fn check_source(
        source: &assura_parser::ast::SourceFile,
        symbols: &assura_resolve::SymbolTable,
    ) -> Vec<TypeError> {
        let mut checker = AxiomaticDefChecker::new();
        let axiom_names: Vec<String> = source
            .decls
            .iter()
            .filter_map(|d| {
                if let Decl::Block { kind, name, .. } = &d.node
                    && *kind == BlockKind::Axiomatic
                {
                    Some(name.clone())
                } else {
                    None
                }
            })
            .collect();
        for decl in &source.decls {
            if let Decl::Block {
                kind, name, body, ..
            } = &decl.node
                && *kind == BlockKind::Axiomatic
            {
                let mut refs = Vec::new();
                for clause in body {
                    let idents = crate::checkers::collect_ident_references(&clause.body);
                    for ident in &idents {
                        if axiom_names.contains(ident) && ident != name {
                            refs.push(ident.clone());
                        }
                    }
                }
                refs.sort();
                refs.dedup();
                checker.declare_axiom(AxiomDef {
                    name: name.clone(),
                    span: decl.span.clone(),
                    references: refs,
                });
            }
        }
        for decl in &source.decls {
            let Some(clauses) = crate::checks::clauses_contract_fn(&decl.node) else {
                continue;
            };
            for clause in clauses {
                if clause.kind == ClauseKind::Requires || clause.kind == ClauseKind::Ensures {
                    let refs = crate::checkers::collect_ident_references(&clause.body);
                    for name in &refs {
                        checker.mark_used(name);
                    }
                }
            }
        }
        let known: Vec<&str> = symbols.symbols.iter().map(|s| s.name.as_str()).collect();
        let mut errors = checker.check_references(&known);
        errors.extend(checker.check_unused());
        errors.extend(checker.check_circular());
        errors
    }
}

impl Default for AxiomaticDefChecker {
    fn default() -> Self {
        Self::new()
    }
}
