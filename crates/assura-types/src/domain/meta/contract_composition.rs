use assura_parser::ast::{ClauseKind, Decl, Expr};

use crate::TypeError;

// ===========================================================================
// T110: Contract composition with extends
// ===========================================================================

/// Tracks contract inheritance/composition via extends.
#[derive(Debug, Clone)]
pub(crate) struct ContractCompositionChecker {
    contracts: std::collections::HashMap<String, ComposableContract>,
}

#[derive(Debug, Clone)]
pub(crate) struct ComposableContract {
    pub name: String,
    pub extends: Vec<String>,
    pub own_clauses: usize,
}

impl ContractCompositionChecker {
    pub fn new() -> Self {
        Self {
            contracts: std::collections::HashMap::new(),
        }
    }

    pub fn declare(&mut self, name: String, extends: Vec<String>, own_clauses: usize) {
        self.contracts.insert(
            name.clone(),
            ComposableContract {
                name,
                extends,
                own_clauses,
            },
        );
    }

    /// Check that all extended contracts exist.
    pub fn check_extends(&self) -> Vec<TypeError> {
        let mut errors = Vec::new();
        for (name, contract) in &self.contracts {
            for parent in &contract.extends {
                if !self.contracts.contains_key(parent) {
                    errors.push(TypeError {
                        code: "A54001".into(),
                        message: format!("contract `{name}` extends unknown contract `{parent}`"),
                        span: 0..1,
                        secondary: None,
                        suggestion: None,
                    });
                }
            }
        }
        errors
    }

    /// Check for circular extends.
    pub fn check_circular(&self) -> Vec<TypeError> {
        let mut errors = Vec::new();
        for name in self.contracts.keys() {
            let mut visited = vec![name.clone()];
            if self.has_extends_cycle(name, &mut visited) {
                errors.push(TypeError {
                    code: "A54002".into(),
                    message: format!("circular extends chain involving `{name}`"),
                    span: 0..1,
                    secondary: None,
                    suggestion: None,
                });
            }
        }
        errors
    }

    fn has_extends_cycle(&self, current: &str, visited: &mut Vec<String>) -> bool {
        if let Some(contract) = self.contracts.get(current) {
            for parent in &contract.extends {
                if visited.contains(parent) {
                    return true;
                }
                visited.push(parent.clone());
                if self.has_extends_cycle(parent, visited) {
                    return true;
                }
                visited.pop();
            }
        }
        false
    }

    /// Check for diamond inheritance (same contract extended via two paths).
    pub fn check_diamond(&self) -> Vec<TypeError> {
        let mut errors = Vec::new();
        for (name, contract) in &self.contracts {
            let mut all_ancestors = Vec::new();
            for parent in &contract.extends {
                let ancestors = self.collect_ancestors(parent);
                for a in &ancestors {
                    if all_ancestors.contains(a) {
                        errors.push(TypeError {
                            code: "A54003".into(),
                            message: format!(
                                "diamond inheritance in `{name}`: `{a}` reached via multiple paths"
                            ),
                            span: 0..1,
                            secondary: None,
                            suggestion: None,
                        });
                    }
                }
                all_ancestors.extend(ancestors);
            }
        }
        errors
    }

    fn collect_ancestors(&self, name: &str) -> Vec<String> {
        let mut result = vec![name.to_string()];
        if let Some(c) = self.contracts.get(name) {
            for parent in &c.extends {
                result.extend(self.collect_ancestors(parent));
            }
        }
        result
    }

    /// Check for contracts with zero own clauses (pure composition).
    pub fn check_empty_contracts(&self) -> Vec<TypeError> {
        self.contracts
            .values()
            .filter(|c| c.own_clauses == 0 && c.extends.is_empty())
            .map(|c| TypeError {
                code: "A54003".into(),
                message: format!("contract `{}` has no clauses and extends nothing", c.name),
                span: 0..1,
                secondary: None,
                suggestion: None,
            })
            .collect()
    }
}

impl Default for ContractCompositionChecker {
    fn default() -> Self {
        Self::new()
    }
}

// ===========================================================================
// Contract composition source walking
// ===========================================================================

impl ContractCompositionChecker {
    pub fn check_source(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
        let mut checker = ContractCompositionChecker::new();
        let mut found = false;
        for decl in &source.decls {
            if let Decl::Contract(c) = &decl.node {
                let extends: Vec<String> = c
                    .clauses
                    .iter()
                    .filter(|cl| {
                        matches!(&cl.kind, ClauseKind::Other(k) if k == "extends" || k == "inherits")
                    })
                    .filter_map(|cl| {
                        if let Expr::Ident(name) = &cl.body.node {
                            Some(name.clone())
                        } else {
                            None
                        }
                    })
                    .collect();
                if !extends.is_empty() {
                    found = true;
                }
                checker.declare(c.name.clone(), extends, c.clauses.len());
            }
        }
        if !found {
            return Vec::new();
        }
        let mut errors = checker.check_extends();
        errors.extend(checker.check_circular());
        errors.extend(checker.check_diamond());
        errors.extend(checker.check_empty_contracts());
        errors
    }
}
