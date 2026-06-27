use assura_parser::ast::{ClauseKind, Expr};

use crate::checks::clauses_contract_fn_block;
use crate::TypeError;

// ===========================================================================
// T102: TEST.2 Behavioral equivalence
// ===========================================================================

#[derive(Debug, Clone)]
pub(crate) struct BehavioralEquivalenceChecker {
    equivalences: Vec<EquivalenceDecl>,
}

#[derive(Debug, Clone)]
pub(crate) struct EquivalenceDecl {
    pub name: String,
    pub impl_a: String,
    pub impl_b: String,
    pub contract: String,
    pub verified: bool,
    pub span: std::ops::Range<usize>,
}

impl BehavioralEquivalenceChecker {
    pub fn new() -> Self {
        Self {
            equivalences: Vec::new(),
        }
    }

    pub fn declare(
        &mut self,
        name: String,
        impl_a: String,
        impl_b: String,
        contract: String,
        span: std::ops::Range<usize>,
    ) {
        self.equivalences.push(EquivalenceDecl {
            name,
            impl_a,
            impl_b,
            contract,
            verified: false,
            span,
        });
    }

    pub fn mark_verified(&mut self, name: &str) {
        if let Some(e) = self.equivalences.iter_mut().find(|e| e.name == name) {
            e.verified = true;
        }
    }

    pub fn check_unverified(&self) -> Vec<TypeError> {
        self.equivalences
            .iter()
            .filter(|e| !e.verified)
            .map(|e| TypeError {
                code: "A49001".into(),
                message: format!(
                    "behavioral equivalence `{}` between `{}` and `{}` not verified",
                    e.name, e.impl_a, e.impl_b
                ),
                span: e.span.clone(),
                secondary: None,
                suggestion: None,
            })
            .collect()
    }

    pub fn check_self_equivalence(&self) -> Vec<TypeError> {
        self.equivalences
            .iter()
            .filter(|e| e.impl_a == e.impl_b)
            .map(|e| TypeError {
                code: "A49002".into(),
                message: format!(
                    "trivial self-equivalence in `{}`: both sides are `{}`",
                    e.name, e.impl_a
                ),
                span: e.span.clone(),
                secondary: None,
                suggestion: None,
            })
            .collect()
    }

    pub fn check_contract_ref(&self) -> Vec<TypeError> {
        self.equivalences
            .iter()
            .filter(|e| e.contract.is_empty())
            .map(|e| TypeError {
                code: "A49003".into(),
                message: format!("equivalence `{}` has no contract reference", e.name),
                span: e.span.clone(),
                secondary: None,
                suggestion: None,
            })
            .collect()
    }
}

impl Default for BehavioralEquivalenceChecker {
    fn default() -> Self {
        Self::new()
    }
}

// ===========================================================================
// Behavioral equivalence source walking
// ===========================================================================

impl BehavioralEquivalenceChecker {
    pub fn check_source(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
        let mut checker = BehavioralEquivalenceChecker::new();
        let mut found = false;
        for decl in &source.decls {
            let Some(clauses) = clauses_contract_fn_block(&decl.node) else {
                continue;
            };
            let parent_name = decl.node.name().unwrap_or("");
            for clause in clauses {
                if let ClauseKind::Other(ref k) = clause.kind
                    && (k == "equivalent" || k == "behavioral_equiv" || k == "equiv")
                {
                    found = true;
                    if let Expr::BinOp { lhs, rhs, .. } = &clause.body.node
                        && let (Expr::Ident(a), Expr::Ident(b)) =
                            (&lhs.as_ref().node, &rhs.as_ref().node)
                    {
                        checker.declare(
                            format!("{a}_equiv_{b}"),
                            a.clone(),
                            b.clone(),
                            parent_name.to_string(),
                            decl.span.clone(),
                        );
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
                if let ClauseKind::Other(ref k) = clause.kind
                    && (k == "verified_equiv" || k == "equiv_proved")
                    && let Expr::Ident(name) = &clause.body.node
                {
                    checker.mark_verified(name);
                }
            }
        }
        let mut errors = checker.check_unverified();
        errors.extend(checker.check_self_equivalence());
        errors.extend(checker.check_contract_ref());
        errors
    }
}
