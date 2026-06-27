use assura_parser::ast::{ClauseKind, Expr};

use crate::checkers::collect_ident_references;
use crate::checks::clauses_contract_fn_block;
use crate::TypeError;

// ===========================================================================
// T105: MISC.2 Scoped invariant suspension
// ===========================================================================

#[derive(Debug, Clone)]
pub(crate) struct ScopedInvariantChecker {
    invariants: std::collections::HashMap<String, InvariantState>,
    suspension_depth: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum InvariantState {
    Active,
    Suspended,
    Restored,
}

impl ScopedInvariantChecker {
    pub fn new() -> Self {
        Self {
            invariants: std::collections::HashMap::new(),
            suspension_depth: 0,
        }
    }

    pub fn declare_invariant(&mut self, name: String) {
        self.invariants.insert(name, InvariantState::Active);
    }

    pub fn suspend(&mut self, name: &str) -> Option<TypeError> {
        if let Some(state) = self.invariants.get_mut(name) {
            if *state == InvariantState::Suspended {
                return Some(TypeError {
                    code: "A52001".into(),
                    message: format!("invariant `{name}` is already suspended"),
                    span: 0..1,
                    secondary: None,
                    suggestion: None,
                });
            }
            *state = InvariantState::Suspended;
            self.suspension_depth += 1;
            None
        } else {
            Some(TypeError {
                code: "A52002".into(),
                message: format!("cannot suspend undeclared invariant `{name}`"),
                span: 0..1,
                secondary: None,
                suggestion: None,
            })
        }
    }

    pub fn restore(&mut self, name: &str) -> Option<TypeError> {
        if let Some(state) = self.invariants.get_mut(name) {
            if *state != InvariantState::Suspended {
                return Some(TypeError {
                    code: "A52003".into(),
                    message: format!("invariant `{name}` is not currently suspended"),
                    span: 0..1,
                    secondary: None,
                    suggestion: None,
                });
            }
            *state = InvariantState::Restored;
            if self.suspension_depth > 0 {
                self.suspension_depth -= 1;
            }
            None
        } else {
            None
        }
    }

    pub fn check_all_restored(&self) -> Vec<TypeError> {
        self.invariants
            .iter()
            .filter(|(_, s)| **s == InvariantState::Suspended)
            .map(|(n, _)| TypeError {
                code: "A52001".into(),
                message: format!("invariant `{n}` still suspended at scope exit"),
                span: 0..1,
                secondary: None,
                suggestion: None,
            })
            .collect()
    }

    pub fn is_suspended(&self, name: &str) -> bool {
        self.invariants.get(name) == Some(&InvariantState::Suspended)
    }
}

impl Default for ScopedInvariantChecker {
    fn default() -> Self {
        Self::new()
    }
}

// ===========================================================================
// Scoped invariants source walking
// ===========================================================================

impl ScopedInvariantChecker {
    pub fn check_source(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
        let mut checker = ScopedInvariantChecker::new();
        let mut errors = Vec::new();
        let mut found = false;
        for decl in &source.decls {
            let Some(clauses) = clauses_contract_fn_block(&decl.node) else {
                continue;
            };
            for clause in clauses {
                if let ClauseKind::Other(ref k) = clause.kind {
                    if k == "suspend_invariant" || k == "scoped_invariant" {
                        found = true;
                        if let Expr::Ident(name) = &clause.body.node {
                            checker.declare_invariant(name.clone());
                            if let Some(err) = checker.suspend(name) {
                                errors.push(err);
                            }
                        }
                    }
                    if (k == "restore_invariant" || k == "restore")
                        && let Expr::Ident(name) = &clause.body.node
                        && let Some(err) = checker.restore(name)
                    {
                        errors.push(err);
                    }
                }
            }
        }
        if !found {
            return Vec::new();
        }
        for decl in &source.decls {
            let Some(clauses) = clauses_contract_fn_block(&decl.node) else {
                continue;
            };
            for clause in clauses {
                if clause.kind == ClauseKind::Requires || clause.kind == ClauseKind::Ensures {
                    let refs = collect_ident_references(&clause.body);
                    for name in &refs {
                        if checker.is_suspended(name) {
                            errors.push(TypeError {
                                code: "A52001".into(),
                                message: format!(
                                    "invariant `{name}` is suspended in active clause context"
                                ),
                                span: decl.span.clone(),
                                secondary: None,
                                suggestion: None,
                            });
                        }
                    }
                }
            }
        }
        errors.extend(checker.check_all_restored());
        errors
    }
}
