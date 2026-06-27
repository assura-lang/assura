use assura_parser::ast::{ClauseKind, Expr};

use crate::checks::clauses_contract_fn;
use crate::TypeError;

// ===========================================================================
// T101: PERF.2 Complexity bounds (AARA)
// ===========================================================================

#[derive(Debug, Clone)]
pub(crate) struct ComplexityBoundChecker {
    bounds: std::collections::HashMap<String, ComplexityBound>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ComplexityClass {
    Constant,
    Logarithmic,
    Linear,
    NLogN,
    Quadratic,
    Cubic,
    Exponential,
}

#[derive(Debug, Clone)]
pub(crate) struct ComplexityBound {
    pub declared: ComplexityClass,
    pub measured: Option<ComplexityClass>,
    pub span: std::ops::Range<usize>,
}

impl ComplexityBoundChecker {
    pub fn new() -> Self {
        Self {
            bounds: std::collections::HashMap::new(),
        }
    }

    pub fn declare_bound(
        &mut self,
        fn_name: String,
        declared: ComplexityClass,
        span: std::ops::Range<usize>,
    ) {
        self.bounds.insert(
            fn_name,
            ComplexityBound {
                declared,
                measured: None,
                span,
            },
        );
    }

    pub fn record_measured(&mut self, fn_name: &str, measured: ComplexityClass) {
        if let Some(b) = self.bounds.get_mut(fn_name) {
            b.measured = Some(measured);
        }
    }

    fn class_rank(c: &ComplexityClass) -> u8 {
        match c {
            ComplexityClass::Constant => 0,
            ComplexityClass::Logarithmic => 1,
            ComplexityClass::Linear => 2,
            ComplexityClass::NLogN => 3,
            ComplexityClass::Quadratic => 4,
            ComplexityClass::Cubic => 5,
            ComplexityClass::Exponential => 6,
        }
    }

    pub fn check_bounds(&self) -> Vec<TypeError> {
        let mut errors = Vec::new();
        for (name, bound) in &self.bounds {
            if let Some(ref measured) = bound.measured
                && Self::class_rank(measured) > Self::class_rank(&bound.declared)
            {
                errors.push(TypeError {
                    code: "A48001".into(),
                    message: format!(
                        "function `{name}` declared as {:?} but measured as {measured:?}",
                        bound.declared
                    ),
                    span: bound.span.clone(),
                    secondary: None,
                    suggestion: None,
                });
            }
        }
        errors
    }

    pub fn check_unverified(&self) -> Vec<TypeError> {
        self.bounds
            .iter()
            .filter(|(_, b)| b.measured.is_none())
            .map(|(n, b)| TypeError {
                code: "A48002".into(),
                message: format!("complexity bound for `{n}` is not verified"),
                span: b.span.clone(),
                secondary: None,
                suggestion: None,
            })
            .collect()
    }

    pub fn check_expensive(&self) -> Vec<TypeError> {
        self.bounds
            .iter()
            .filter(|(_, b)| b.declared == ComplexityClass::Exponential)
            .map(|(n, b)| TypeError {
                code: "A48003".into(),
                message: format!("function `{n}` has exponential complexity bound"),
                span: b.span.clone(),
                secondary: None,
                suggestion: None,
            })
            .collect()
    }
}

impl Default for ComplexityBoundChecker {
    fn default() -> Self {
        Self::new()
    }
}

// ===========================================================================
// Complexity bounds source walking
// ===========================================================================

impl ComplexityBoundChecker {
    pub fn check_source(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
        let mut checker = ComplexityBoundChecker::new();
        let mut found = false;
        for decl in &source.decls {
            let Some(clauses) = clauses_contract_fn(&decl.node) else {
                continue;
            };
            let Some(name) = decl.node.name().map(|s| s.to_string()) else {
                continue;
            };
            for clause in clauses {
                if let ClauseKind::Other(ref k) = clause.kind
                    && (k == "complexity" || k == "time_complexity" || k == "big_o")
                {
                    found = true;
                    if let Expr::Ident(class_name) = &clause.body.node {
                        let class = parse_complexity_class(class_name);
                        checker.declare_bound(name.clone(), class, decl.span.clone());
                    }
                }
            }
        }
        if !found {
            return Vec::new();
        }
        for decl in &source.decls {
            let Some(clauses) = clauses_contract_fn(&decl.node) else {
                continue;
            };
            let Some(name) = decl.node.name() else {
                continue;
            };
            for clause in clauses {
                if let ClauseKind::Other(ref k) = clause.kind
                    && (k == "measured_complexity" || k == "actual_complexity")
                    && let Expr::Ident(class_name) = &clause.body.node
                {
                    checker.record_measured(name, parse_complexity_class(class_name));
                }
            }
        }
        let mut errors = checker.check_bounds();
        errors.extend(checker.check_unverified());
        errors.extend(checker.check_expensive());
        errors
    }
}

fn parse_complexity_class(name: &str) -> ComplexityClass {
    match name {
        "constant" | "O1" => ComplexityClass::Constant,
        "logarithmic" | "O_log_n" => ComplexityClass::Logarithmic,
        "linear" | "On" => ComplexityClass::Linear,
        "nlogn" | "O_n_log_n" => ComplexityClass::NLogN,
        "quadratic" | "On2" => ComplexityClass::Quadratic,
        "cubic" | "On3" => ComplexityClass::Cubic,
        "exponential" | "O2n" => ComplexityClass::Exponential,
        _ => ComplexityClass::Linear,
    }
}
