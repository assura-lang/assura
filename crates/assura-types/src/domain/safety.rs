//! Safety-related domain checkers.
//!
//! UnsafeEscapeChecker.

use crate::TypeError;

// ===========================================================================
// T100: PERF.1 Unsafe escape with proof
// ===========================================================================

#[derive(Debug, Clone)]
pub(crate) struct UnsafeEscapeChecker {
    unsafe_blocks: Vec<UnsafeBlock>,
}

#[derive(Debug, Clone)]
pub(crate) struct UnsafeBlock {
    pub name: String,
    pub has_safety_proof: bool,
    pub proof_obligations: Vec<String>,
    pub obligations_discharged: Vec<String>,
    pub span: std::ops::Range<usize>,
}

impl UnsafeEscapeChecker {
    pub fn new() -> Self {
        Self {
            unsafe_blocks: Vec::new(),
        }
    }

    pub fn declare_unsafe(
        &mut self,
        name: String,
        obligations: Vec<String>,
        span: std::ops::Range<usize>,
    ) {
        self.unsafe_blocks.push(UnsafeBlock {
            name,
            has_safety_proof: false,
            proof_obligations: obligations,
            obligations_discharged: Vec::new(),
            span,
        });
    }

    pub fn attach_proof(&mut self, name: &str) {
        if let Some(b) = self.unsafe_blocks.iter_mut().find(|b| b.name == name) {
            b.has_safety_proof = true;
        }
    }

    pub fn discharge_obligation(&mut self, block_name: &str, obligation: String) {
        if let Some(b) = self.unsafe_blocks.iter_mut().find(|b| b.name == block_name) {
            b.obligations_discharged.push(obligation);
        }
    }

    pub fn check_unproven(&self) -> Vec<TypeError> {
        self.unsafe_blocks
            .iter()
            .filter(|b| !b.has_safety_proof)
            .map(|b| TypeError {
                code: "A47001".into(),
                message: format!("unsafe block `{}` has no safety proof", b.name),
                span: b.span.clone(),
                secondary: None,
                suggestion: None,
            })
            .collect()
    }

    pub fn check_obligations(&self) -> Vec<TypeError> {
        let mut errors = Vec::new();
        for b in &self.unsafe_blocks {
            for obl in &b.proof_obligations {
                if !b.obligations_discharged.contains(obl) {
                    errors.push(TypeError {
                        code: "A47002".into(),
                        message: format!(
                            "obligation `{obl}` in unsafe block `{}` not discharged",
                            b.name
                        ),
                        span: b.span.clone(),
                        secondary: None,
                        suggestion: None,
                    });
                }
            }
        }
        errors
    }

    pub fn check_empty_obligations(&self) -> Vec<TypeError> {
        self.unsafe_blocks
            .iter()
            .filter(|b| b.proof_obligations.is_empty())
            .map(|b| TypeError {
                code: "A47003".into(),
                message: format!("unsafe block `{}` declares no proof obligations", b.name),
                span: b.span.clone(),
                secondary: None,
                suggestion: None,
            })
            .collect()
    }
}

impl Default for UnsafeEscapeChecker {
    fn default() -> Self {
        Self::new()
    }
}

impl UnsafeEscapeChecker {
    /// AST-walking entry point: scan for unsafe/trusted blocks and check proofs.
    pub fn check_source(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
        use assura_parser::ast::{BlockKind, ClauseKind, Decl, Expr};

        let mut checker = UnsafeEscapeChecker::new();
        let mut found = false;
        for decl in &source.decls {
            match &decl.node {
                Decl::FnDef(f) => {
                    let mut obligations = Vec::new();
                    for clause in &f.clauses {
                        if let ClauseKind::Other(ref k) = clause.kind
                            && (k == "obligation" || k == "proof_obligation" || k == "must_prove")
                        {
                            if let Expr::Ident(obl) = &clause.body.node {
                                obligations.push(obl.clone());
                            } else if let Some((_, args)) =
                                crate::checkers::extract_call(&clause.body)
                            {
                                for arg in args {
                                    if let Some(name) = crate::checkers::extract_ident(arg) {
                                        obligations.push(name.to_string());
                                    }
                                }
                            }
                        }
                    }
                    for clause in &f.clauses {
                        if let ClauseKind::Other(ref k) = clause.kind {
                            if k == "unsafe" || k == "unsafe_escape" || k == "trusted" {
                                found = true;
                                checker.declare_unsafe(
                                    f.name.clone(),
                                    obligations.clone(),
                                    decl.span.clone(),
                                );
                            }
                            if k == "safety_proof" || k == "proof" {
                                checker.attach_proof(&f.name);
                            }
                        }
                    }
                }
                Decl::Block {
                    kind, name, body, ..
                } if *kind == BlockKind::UnsafeEscape => {
                    found = true;
                    let mut obligations = Vec::new();
                    for clause in body {
                        if let ClauseKind::Other(ref k) = clause.kind
                            && (k == "obligation" || k == "proof_obligation" || k == "must_prove")
                        {
                            if let Expr::Ident(obl) = &clause.body.node {
                                obligations.push(obl.clone());
                            } else if let Some((_, args)) =
                                crate::checkers::extract_call(&clause.body)
                            {
                                for arg in args {
                                    if let Some(name) = crate::checkers::extract_ident(arg) {
                                        obligations.push(name.to_string());
                                    }
                                }
                            }
                        }
                    }
                    checker.declare_unsafe(name.clone(), obligations, decl.span.clone());
                    for clause in body {
                        if let ClauseKind::Other(ref k) = clause.kind
                            && (k == "safety_proof" || k == "proof")
                        {
                            checker.attach_proof(name);
                        }
                    }
                }
                _ => {}
            }
        }
        if !found {
            return Vec::new();
        }
        // Discharge obligations from proof clauses
        for decl in &source.decls {
            match &decl.node {
                Decl::FnDef(f) => {
                    for clause in &f.clauses {
                        if let ClauseKind::Other(ref k) = clause.kind
                            && (k == "discharges" || k == "proves")
                            && let Expr::Ident(obligation) = &clause.body.node
                        {
                            checker.discharge_obligation(&f.name, obligation.clone());
                        }
                    }
                }
                Decl::Block {
                    kind, name, body, ..
                } if *kind == BlockKind::UnsafeEscape => {
                    for clause in body {
                        if let ClauseKind::Other(ref k) = clause.kind
                            && (k == "discharges" || k == "proves")
                            && let Expr::Ident(obligation) = &clause.body.node
                        {
                            checker.discharge_obligation(name, obligation.clone());
                        }
                    }
                }
                _ => {}
            }
        }
        let mut errors = checker.check_unproven();
        errors.extend(checker.check_obligations());
        errors.extend(checker.check_empty_obligations());
        errors
    }
}
