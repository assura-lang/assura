use assura_parser::ast::{ClauseKind, Expr};

use crate::checkers::{extract_call, extract_ident, extract_int_literal, extract_kv_pairs};
use crate::checks::clauses_contract_fn_block;
use crate::types::DEFAULT_PARAM_ONE;
use crate::TypeError;

// ===========================================================================
// T103: TEST.3 Multi-pass refinement
// ===========================================================================

#[derive(Debug, Clone)]
pub(crate) struct MultiPassRefinementChecker {
    passes: Vec<RefinementPass>,
}

#[derive(Debug, Clone)]
pub(crate) struct RefinementPass {
    pub name: String,
    pub from_level: String,
    pub to_level: String,
    pub obligations_total: usize,
    pub obligations_discharged: usize,
    pub span: std::ops::Range<usize>,
}

impl MultiPassRefinementChecker {
    pub fn new() -> Self {
        Self { passes: Vec::new() }
    }

    pub fn add_pass(
        &mut self,
        name: String,
        from_level: String,
        to_level: String,
        obligations: usize,
        span: std::ops::Range<usize>,
    ) {
        self.passes.push(RefinementPass {
            name,
            from_level,
            to_level,
            obligations_total: obligations,
            obligations_discharged: 0,
            span,
        });
    }

    pub fn discharge(&mut self, pass_name: &str, count: usize) {
        if let Some(p) = self.passes.iter_mut().find(|p| p.name == pass_name) {
            p.obligations_discharged += count;
        }
    }

    pub fn check_complete(&self) -> Vec<TypeError> {
        self.passes
            .iter()
            .filter(|p| p.obligations_discharged < p.obligations_total)
            .map(|p| TypeError {
                code: "A50001".into(),
                message: format!(
                    "refinement `{}` ({} -> {}): {}/{} obligations discharged",
                    p.name, p.from_level, p.to_level, p.obligations_discharged, p.obligations_total
                ),
                span: p.span.clone(),
                secondary: None,
                suggestion: None,
            })
            .collect()
    }

    pub fn check_chain(&self) -> Vec<TypeError> {
        let mut errors = Vec::new();
        for i in 1..self.passes.len() {
            if self.passes[i].from_level != self.passes[i - 1].to_level {
                errors.push(TypeError {
                    code: "A50002".into(),
                    message: format!(
                        "refinement chain gap: `{}` starts at `{}` but `{}` ends at `{}`",
                        self.passes[i].name,
                        self.passes[i].from_level,
                        self.passes[i - 1].name,
                        self.passes[i - 1].to_level
                    ),
                    span: self.passes[i].span.clone(),
                    secondary: None,
                    suggestion: None,
                });
            }
        }
        errors
    }

    pub fn check_non_trivial(&self) -> Vec<TypeError> {
        self.passes
            .iter()
            .filter(|p| p.obligations_total == 0)
            .map(|p| TypeError {
                code: "A50003".into(),
                message: format!("refinement pass `{}` has zero obligations", p.name),
                span: p.span.clone(),
                secondary: None,
                suggestion: None,
            })
            .collect()
    }
}

impl Default for MultiPassRefinementChecker {
    fn default() -> Self {
        Self::new()
    }
}

// ===========================================================================
// Multi-pass refinement source walking
// ===========================================================================

impl MultiPassRefinementChecker {
    pub fn check_source(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
        let mut checker = MultiPassRefinementChecker::new();
        let mut found = false;
        for decl in &source.decls {
            let Some(clauses) = clauses_contract_fn_block(&decl.node) else {
                continue;
            };
            for clause in clauses {
                if let ClauseKind::Other(ref k) = clause.kind
                    && (k == "refinement_pass" || k == "multi_pass" || k == "refine")
                {
                    found = true;
                    match &clause.body.node {
                        Expr::Call { func, args } => {
                            if let Expr::Ident(name) = &func.as_ref().node {
                                let from = args
                                    .first()
                                    .and_then(extract_ident)
                                    .unwrap_or("abstract")
                                    .to_string();
                                let to = args
                                    .get(1)
                                    .and_then(extract_ident)
                                    .unwrap_or("concrete")
                                    .to_string();
                                let order = args
                                    .get(2)
                                    .and_then(extract_int_literal)
                                    .unwrap_or(DEFAULT_PARAM_ONE)
                                    as usize;
                                checker.add_pass(name.clone(), from, to, order, decl.span.clone());
                            }
                        }
                        Expr::Ident(name) => {
                            checker.add_pass(
                                name.clone(),
                                "abstract".into(),
                                "concrete".into(),
                                1,
                                decl.span.clone(),
                            );
                        }
                        _ => {
                            let kvs = extract_kv_pairs(&clause.body);
                            let name = kvs
                                .iter()
                                .find(|(k, _)| *k == "name" || *k == "pass")
                                .and_then(|(_, v)| extract_ident(v))
                                .unwrap_or("unnamed")
                                .to_string();
                            let from = kvs
                                .iter()
                                .find(|(k, _)| *k == "from" || *k == "source")
                                .and_then(|(_, v)| extract_ident(v))
                                .unwrap_or("abstract")
                                .to_string();
                            let to = kvs
                                .iter()
                                .find(|(k, _)| *k == "to" || *k == "target")
                                .and_then(|(_, v)| extract_ident(v))
                                .unwrap_or("concrete")
                                .to_string();
                            let order = kvs
                                .iter()
                                .find(|(k, _)| *k == "order")
                                .and_then(|(_, v)| extract_int_literal(v))
                                .unwrap_or(DEFAULT_PARAM_ONE)
                                as usize;
                            checker.add_pass(name, from, to, order, decl.span.clone());
                        }
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
                    && (k == "discharge_pass" || k == "pass_proved")
                {
                    if let Some((name, args)) = extract_call(&clause.body) {
                        let count =
                            args.first()
                                .and_then(extract_int_literal)
                                .unwrap_or(DEFAULT_PARAM_ONE) as usize;
                        checker.discharge(name, count);
                    } else if let Expr::Ident(name) = &clause.body.node {
                        checker.discharge(name, 1);
                    } else {
                        let kvs = extract_kv_pairs(&clause.body);
                        let name = kvs
                            .iter()
                            .find(|(k, _)| *k == "name" || *k == "pass")
                            .and_then(|(_, v)| extract_ident(v))
                            .unwrap_or("unnamed");
                        let count =
                            kvs.iter()
                                .find(|(k, _)| *k == "count" || *k == "obligations")
                                .and_then(|(_, v)| extract_int_literal(v))
                                .unwrap_or(DEFAULT_PARAM_ONE) as usize;
                        checker.discharge(name, count);
                    }
                }
            }
        }
        let mut errors = checker.check_complete();
        errors.extend(checker.check_chain());
        errors.extend(checker.check_non_trivial());
        errors
    }
}
