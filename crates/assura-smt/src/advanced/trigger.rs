// ===========================================================================
// T078: CORE.5 Quantifier triggers (e-matching hints)
// ===========================================================================

/// E-matching trigger patterns for SMT quantifier instantiation.
///
/// Triggers guide the SMT solver's quantifier instantiation by specifying
/// which ground terms should cause a quantified formula to be instantiated.
#[derive(Debug, Clone)]
pub struct TriggerPattern {
    /// The pattern terms (multi-trigger if > 1)
    pub terms: Vec<String>,
    /// Whether this is a user-provided trigger
    pub is_user_provided: bool,
}

/// Manages trigger inference and validation for quantified formulas.
#[derive(Debug, Clone)]
pub struct TriggerManager {
    /// Known function symbols for trigger inference
    known_functions: Vec<String>,
    /// User-specified triggers per quantified formula
    triggers: std::collections::HashMap<String, Vec<TriggerPattern>>,
    /// Last validate_trigger warnings (surfaced by encoder paths for diagnostics).
    last_warnings: Vec<String>,
}

impl TriggerManager {
    pub fn new() -> Self {
        Self {
            known_functions: Vec::new(),
            triggers: std::collections::HashMap::new(),
            last_warnings: Vec::new(),
        }
    }

    pub fn register_function(&mut self, name: String) {
        if !self.known_functions.contains(&name) {
            self.known_functions.push(name);
        }
    }

    /// Known function names (for backend pattern construction).
    pub fn known_functions(&self) -> &[String] {
        &self.known_functions
    }

    pub fn add_trigger(&mut self, formula_name: String, pattern: TriggerPattern) {
        self.triggers.entry(formula_name).or_default().push(pattern);
    }

    /// Infer a trigger pattern from the quantifier body (Debug/string form).
    /// Returns None if no suitable trigger can be inferred.
    ///
    /// Prefer [`Self::infer_trigger_from_expr`] when an AST is available; this
    /// string path remains for callers that only have serialized bodies.
    pub fn infer_trigger(&self, body: &str) -> Option<TriggerPattern> {
        for func in &self.known_functions {
            if body.contains(func.as_str()) {
                return Some(TriggerPattern {
                    terms: vec![format!("{func}(x)")],
                    is_user_provided: false,
                });
            }
        }
        None
    }

    /// Infer trigger patterns from quantifier body AST: function/method calls
    /// that mention the bound variable (or any bound-var mention if `bound_var`
    /// is empty and we fall back to known functions in the body).
    pub fn infer_trigger_from_expr(
        &self,
        body: &assura_ast::SpExpr,
        bound_var: &str,
    ) -> Option<TriggerPattern> {
        let mut terms = Vec::new();
        collect_trigger_terms_from_expr(body, bound_var, &self.known_functions, &mut terms);
        if terms.is_empty() {
            // Fallback: string inference on Debug form when AST scan finds nothing
            // but known functions appear in the body serialization.
            let body_str = format!("{body:?}");
            return self.infer_trigger(&body_str);
        }
        terms.sort();
        terms.dedup();
        Some(TriggerPattern {
            terms,
            is_user_provided: false,
        })
    }

    /// Validate that a trigger pattern mentions only known functions.
    /// Stores warnings on `self` for later retrieval via [`Self::take_last_warnings`].
    pub fn validate_trigger(&mut self, pattern: &TriggerPattern) -> Vec<String> {
        let mut warnings = Vec::new();
        for term in &pattern.terms {
            let has_known = self
                .known_functions
                .iter()
                .any(|f| term.contains(f.as_str()));
            if !has_known && !self.known_functions.is_empty() {
                warnings.push(format!(
                    "trigger term `{term}` does not reference any known function"
                ));
            }
        }
        self.last_warnings = warnings.clone();
        warnings
    }

    /// Drain the most recent validate_trigger warnings.
    pub fn take_last_warnings(&mut self) -> Vec<String> {
        std::mem::take(&mut self.last_warnings)
    }

    pub fn get_triggers(&self, formula_name: &str) -> Option<&[TriggerPattern]> {
        self.triggers.get(formula_name).map(|v| v.as_slice())
    }
}

/// Walk expression tree collecting `func(bound)` / `method(bound)` style trigger terms.
fn collect_trigger_terms_from_expr(
    expr: &assura_ast::SpExpr,
    bound_var: &str,
    known: &[String],
    out: &mut Vec<String>,
) {
    use assura_ast::Expr;
    use assura_types::checkers::expr_references_var;

    let mentions_bound = |e: &assura_ast::SpExpr| -> bool {
        if bound_var.is_empty() {
            true
        } else {
            expr_references_var(e, bound_var)
        }
    };

    match &expr.node {
        Expr::Call { func, args } => {
            if let Expr::Ident(fname) = &func.as_ref().node {
                let refs_bound = args.iter().any(mentions_bound);
                let is_known = known.is_empty() || known.iter().any(|k| k == fname);
                if refs_bound && is_known {
                    out.push(format!("{fname}({bound_var})"));
                }
            }
            for a in args {
                collect_trigger_terms_from_expr(a, bound_var, known, out);
            }
        }
        Expr::MethodCall {
            receiver,
            method,
            args,
        } => {
            let refs_bound = mentions_bound(receiver) || args.iter().any(mentions_bound);
            let is_known = known.is_empty() || known.iter().any(|k| k == method);
            if refs_bound && is_known {
                out.push(format!("{method}({bound_var})"));
            }
            collect_trigger_terms_from_expr(receiver, bound_var, known, out);
            for a in args {
                collect_trigger_terms_from_expr(a, bound_var, known, out);
            }
        }
        Expr::BinOp { lhs, rhs, .. } => {
            collect_trigger_terms_from_expr(lhs, bound_var, known, out);
            collect_trigger_terms_from_expr(rhs, bound_var, known, out);
        }
        Expr::UnaryOp { expr: inner, .. } | Expr::Old(inner) | Expr::Ghost(inner) => {
            collect_trigger_terms_from_expr(inner, bound_var, known, out);
        }
        Expr::If {
            cond,
            then_branch,
            else_branch,
        } => {
            collect_trigger_terms_from_expr(cond, bound_var, known, out);
            collect_trigger_terms_from_expr(then_branch, bound_var, known, out);
            if let Some(eb) = else_branch {
                collect_trigger_terms_from_expr(eb, bound_var, known, out);
            }
        }
        Expr::Forall { domain, body, .. } | Expr::Exists { domain, body, .. } => {
            collect_trigger_terms_from_expr(domain, bound_var, known, out);
            collect_trigger_terms_from_expr(body, bound_var, known, out);
        }
        Expr::Index { expr: e, index } => {
            collect_trigger_terms_from_expr(e, bound_var, known, out);
            collect_trigger_terms_from_expr(index, bound_var, known, out);
        }
        Expr::Field(obj, _) => collect_trigger_terms_from_expr(obj, bound_var, known, out),
        Expr::Block(items) | Expr::Tuple(items) | Expr::List(items) => {
            for e in items {
                collect_trigger_terms_from_expr(e, bound_var, known, out);
            }
        }
        Expr::Apply { args, .. } => {
            for a in args {
                collect_trigger_terms_from_expr(a, bound_var, known, out);
            }
        }
        _ => {}
    }
}

impl Default for TriggerManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trigger_new_is_empty() {
        let tm = TriggerManager::new();
        assert!(tm.get_triggers("any").is_none());
    }

    #[test]
    fn trigger_default_is_empty() {
        let tm = TriggerManager::default();
        assert!(tm.get_triggers("any").is_none());
    }

    #[test]
    fn trigger_register_function_deduplicates() {
        let mut tm = TriggerManager::new();
        tm.register_function("f".into());
        tm.register_function("f".into());
        // infer should still produce a single-term trigger
        let t = tm.infer_trigger("f(x) > 0").unwrap();
        assert_eq!(t.terms.len(), 1);
    }

    #[test]
    fn trigger_infer_finds_known_function() {
        let mut tm = TriggerManager::new();
        tm.register_function("hash".into());
        let t = tm.infer_trigger("hash(x) == hash(y)").unwrap();
        assert_eq!(t.terms, vec!["hash(x)"]);
        assert!(!t.is_user_provided);
    }

    #[test]
    fn trigger_infer_returns_none_for_unknown() {
        let tm = TriggerManager::new();
        assert!(tm.infer_trigger("x + y > 0").is_none());
    }

    #[test]
    fn trigger_add_and_get() {
        let mut tm = TriggerManager::new();
        tm.add_trigger(
            "q1".into(),
            TriggerPattern {
                terms: vec!["f(x)".into()],
                is_user_provided: true,
            },
        );
        let triggers = tm.get_triggers("q1").unwrap();
        assert_eq!(triggers.len(), 1);
        assert!(triggers[0].is_user_provided);
    }

    #[test]
    fn trigger_validate_warns_on_unknown_function() {
        let mut tm = TriggerManager::new();
        tm.register_function("known_only".into());
        let pat = TriggerPattern {
            terms: vec!["unknown_func(x)".into()],
            is_user_provided: true,
        };
        let warnings = tm.validate_trigger(&pat);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("unknown_func(x)"));
    }

    #[test]
    fn trigger_validate_no_warning_for_known() {
        let mut tm = TriggerManager::new();
        tm.register_function("f".into());
        let pat = TriggerPattern {
            terms: vec!["f(x)".into()],
            is_user_provided: true,
        };
        assert!(tm.validate_trigger(&pat).is_empty());
    }

    #[test]
    fn trigger_infer_from_expr_call_with_bound_var() {
        use assura_ast::{Expr, Spanned};
        let mut tm = TriggerManager::new();
        tm.register_function("lookup".into());
        let body = Spanned::no_span(Expr::BinOp {
            lhs: Box::new(Spanned::no_span(Expr::Call {
                func: Box::new(Spanned::no_span(Expr::Ident("lookup".into()))),
                args: vec![Spanned::no_span(Expr::Ident("i".into()))],
            })),
            op: assura_ast::BinOp::Gt,
            rhs: Box::new(Spanned::no_span(Expr::Literal(assura_ast::Literal::Int(
                "0".into(),
            )))),
        });
        let t = tm
            .infer_trigger_from_expr(&body, "i")
            .expect("should infer from Call");
        assert!(
            t.terms.iter().any(|term| term.contains("lookup")),
            "expected lookup trigger, got {:?}",
            t.terms
        );
    }
}
