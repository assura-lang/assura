//! T090: STOR.5 Monotonic state.

use assura_parser::ast::{ClauseKind, Expr};

use crate::TypeError;
use crate::checkers::*;
use crate::types::*;

#[derive(Debug, Clone)]
pub(crate) struct MonotonicStateChecker {
    monotonic_vars: std::collections::HashMap<String, MonotonicInfo>,
}

#[derive(Debug, Clone)]
pub(crate) struct MonotonicInfo {
    pub current_value: i64,
    pub direction: MonotonicDirection,
    pub span: std::ops::Range<usize>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum MonotonicDirection {
    Increasing,
    StrictlyIncreasing,
    Decreasing,
}

impl MonotonicStateChecker {
    pub fn new() -> Self {
        Self {
            monotonic_vars: std::collections::HashMap::new(),
        }
    }

    pub fn declare(
        &mut self,
        name: String,
        direction: MonotonicDirection,
        initial: i64,
        span: std::ops::Range<usize>,
    ) {
        self.monotonic_vars.insert(
            name,
            MonotonicInfo {
                current_value: initial,
                direction,
                span,
            },
        );
    }

    pub fn update(&mut self, name: &str, new_value: i64) -> Option<TypeError> {
        if let Some(info) = self.monotonic_vars.get_mut(name) {
            let violation = match info.direction {
                MonotonicDirection::Increasing => new_value < info.current_value,
                MonotonicDirection::StrictlyIncreasing => new_value <= info.current_value,
                MonotonicDirection::Decreasing => new_value > info.current_value,
            };
            if violation {
                return Some(TypeError {
                    code: "A37001".into(),
                    message: format!(
                        "monotonicity violation: `{name}` changed from {} to {new_value}",
                        info.current_value
                    ),
                    span: info.span.clone(),
                    secondary: None,
                    suggestion: None,
                });
            }
            info.current_value = new_value;
        }
        None
    }

    pub fn check_reset(&self, name: &str) -> Option<TypeError> {
        if self.monotonic_vars.contains_key(name) {
            Some(TypeError {
                code: "A37002".into(),
                message: format!("illegal reset of monotonic variable `{name}`"),
                span: 0..1,
                secondary: None,
                suggestion: None,
            })
        } else {
            None
        }
    }

    pub fn check_access(&self, name: &str) -> Option<TypeError> {
        if !self.monotonic_vars.contains_key(name) {
            Some(TypeError {
                code: "A37003".into(),
                message: format!("access to undeclared monotonic variable `{name}`"),
                span: 0..1,
                secondary: None,
                suggestion: None,
            })
        } else {
            None
        }
    }

    pub fn current_value(&self, name: &str) -> Option<i64> {
        self.monotonic_vars.get(name).map(|i| i.current_value)
    }
}

impl MonotonicStateChecker {
    pub fn check_source(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
        let mut checker = Self::new();
        let mut found = false;
        for decl in &source.decls {
            let Some(clauses) = crate::checks::clauses_contract_fn(&decl.node) else {
                continue;
            };
            for clause in clauses {
                if let ClauseKind::Other(ref k) = clause.kind {
                    if k == "monotonic" || k == "monotone" || k == "increasing" {
                        found = true;
                        match &clause.body.node {
                            Expr::Call { func, args } => {
                                if let Expr::Ident(name) = &func.as_ref().node {
                                    let direction = args
                                        .first()
                                        .and_then(extract_ident)
                                        .map(|d| match d {
                                            "strictly_increasing" => {
                                                MonotonicDirection::StrictlyIncreasing
                                            }
                                            "decreasing" => MonotonicDirection::Decreasing,
                                            _ => MonotonicDirection::Increasing,
                                        })
                                        .unwrap_or(MonotonicDirection::Increasing);
                                    let initial = args
                                        .get(1)
                                        .and_then(extract_int_literal)
                                        .unwrap_or(DEFAULT_PARAM_ZERO);
                                    checker.declare(
                                        name.clone(),
                                        direction,
                                        initial,
                                        decl.span.clone(),
                                    );
                                }
                            }
                            Expr::Ident(name) => {
                                checker.declare(
                                    name.clone(),
                                    MonotonicDirection::Increasing,
                                    0,
                                    decl.span.clone(),
                                );
                            }
                            _ => {}
                        }
                    }
                    if (k == "update" || k == "assign" || k == "set")
                        && let Some((name, args)) = extract_call(&clause.body)
                        && let Some(val) = args.first().and_then(extract_int_literal)
                        && let Some(err) = checker.update(name, val)
                    {
                        return vec![err];
                    }
                    if k == "reset"
                        && let Some(name) = extract_ident(&clause.body)
                        && let Some(err) = checker.check_reset(name)
                    {
                        return vec![err];
                    }
                }
            }
        }
        if !found {
            return Vec::new();
        }
        let mut errors = Vec::new();
        for decl in &source.decls {
            let Some(clauses) = crate::checks::clauses_contract_fn(&decl.node) else {
                continue;
            };
            for clause in clauses {
                if clause.kind == ClauseKind::Ensures {
                    let refs = collect_ident_references(&clause.body);
                    for name in &refs {
                        if let Some(mut err) = checker.check_access(name) {
                            if let Some(val) = checker.current_value(name) {
                                err.message.push_str(&format!(" (current value: {val})"));
                            }
                            errors.push(err);
                        }
                    }
                }
            }
        }
        errors
    }
}

impl Default for MonotonicStateChecker {
    fn default() -> Self {
        Self::new()
    }
}
