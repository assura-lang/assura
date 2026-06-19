//! Shared CVC5 utilities used by shell-out and native backends.

use assura_parser::ast::Expr;

/// Rational denominator for Float literal encoding (matches Z3/CVC5 native).
pub(crate) const FLOAT_RATIONAL_DENOM: i64 = 1_000_000;

/// Sanitize an Assura identifier for SMT-LIB2/CVC5 names.
pub(crate) fn sanitize_smtlib_name(name: &str) -> String {
    name.replace('.', "_")
}

/// Map `result` to the encoder's return-value name.
pub(crate) fn smtlib_result_name() -> &'static str {
    "__result"
}

/// SMT-LIB name for an `old()` snapshot of an identifier.
pub(crate) fn old_ident_smtlib_name(name: &str) -> String {
    if name == "result" {
        "__result__old".to_string()
    } else {
        format!("{}__old", sanitize_smtlib_name(name))
    }
}

/// Render a float literal as SMT-LIB rational `(/ numer denom)`.
pub(crate) fn float_literal_to_smtlib(f: &str) -> String {
    let (numer, denom) = float_to_rational_parts(f);
    format!("(/ {numer} {denom})")
}

/// Convert a float string to `(numerator, denominator)` rational parts.
pub(crate) fn float_to_rational_parts(f: &str) -> (i64, i64) {
    let fv: f64 = f.parse().unwrap_or(0.0);
    let numer = (fv * FLOAT_RATIONAL_DENOM as f64) as i64;
    (numer, FLOAT_RATIONAL_DENOM)
}

// -------------------------------------------------------------------------
// Deep field-chain flattening (#250)
// -------------------------------------------------------------------------

pub(crate) fn is_self_rooted_cvc5(expr: &Expr) -> bool {
    match expr {
        Expr::Ident(name) => name == "self",
        Expr::Field(obj, _) => is_self_rooted_cvc5(obj),
        Expr::Paren(inner) => is_self_rooted_cvc5(inner),
        _ => false,
    }
}

pub(crate) fn field_chain_depth_cvc5(expr: &Expr) -> usize {
    match expr {
        Expr::Field(obj, _) => 1 + field_chain_depth_cvc5(obj),
        Expr::Paren(inner) => field_chain_depth_cvc5(inner),
        _ => 0,
    }
}

pub(crate) fn has_deep_field_chain_cvc5(expr: &Expr) -> bool {
    field_chain_depth_cvc5(expr) >= 2
}

/// Flatten a field chain like `a.b.c` into `"a__b__c"`.
pub(crate) fn flatten_field_chain_cvc5(expr: &Expr) -> String {
    match expr {
        Expr::Field(obj, field) => {
            let prefix = flatten_field_chain_cvc5(obj);
            format!("{prefix}__{field}")
        }
        Expr::Ident(name) => name.clone(),
        Expr::Paren(inner) => flatten_field_chain_cvc5(inner),
        _ => format!("__obj_{:p}", expr as *const _),
    }
}

// -------------------------------------------------------------------------
// Unmodelable-feature detection (mirrors Z3 encoder, no z3-verify dep)
// -------------------------------------------------------------------------

pub(crate) fn expr_has_unmodelable_features_cvc5(expr: &Expr) -> bool {
    match expr {
        Expr::Field(obj, _) => expr_has_unmodelable_features_cvc5(obj),
        Expr::MethodCall {
            receiver,
            method: _,
            args,
        } => {
            expr_has_unmodelable_features_cvc5(receiver)
                || args.iter().any(expr_has_unmodelable_features_cvc5)
        }
        Expr::Raw(_tokens) => false,
        Expr::BinOp { lhs, rhs, .. } => {
            expr_has_unmodelable_features_cvc5(lhs) || expr_has_unmodelable_features_cvc5(rhs)
        }
        Expr::UnaryOp { expr: inner, .. }
        | Expr::Paren(inner)
        | Expr::Old(inner)
        | Expr::Ghost(inner)
        | Expr::Cast { expr: inner, .. } => expr_has_unmodelable_features_cvc5(inner),
        Expr::Call { func, args } => {
            expr_has_unmodelable_features_cvc5(func)
                || args.iter().any(expr_has_unmodelable_features_cvc5)
        }
        Expr::Index { expr: e, index } => {
            expr_has_unmodelable_features_cvc5(e) || expr_has_unmodelable_features_cvc5(index)
        }
        Expr::Forall { domain, body, .. } | Expr::Exists { domain, body, .. } => {
            expr_has_unmodelable_features_cvc5(domain) || expr_has_unmodelable_features_cvc5(body)
        }
        Expr::If {
            cond,
            then_branch,
            else_branch,
        } => {
            expr_has_unmodelable_features_cvc5(cond)
                || expr_has_unmodelable_features_cvc5(then_branch)
                || else_branch
                    .as_ref()
                    .is_some_and(|e| expr_has_unmodelable_features_cvc5(e))
        }
        Expr::Let { value, body, .. } => {
            expr_has_unmodelable_features_cvc5(value) || expr_has_unmodelable_features_cvc5(body)
        }
        Expr::Match { scrutinee, arms } => {
            expr_has_unmodelable_features_cvc5(scrutinee)
                || arms
                    .iter()
                    .any(|a| expr_has_unmodelable_features_cvc5(&a.body))
        }
        Expr::List(items) | Expr::Tuple(items) | Expr::Block(items) => {
            items.iter().any(expr_has_unmodelable_features_cvc5)
        }
        Expr::Apply { args, .. } => args.iter().any(expr_has_unmodelable_features_cvc5),
        Expr::Literal(_) | Expr::Ident(_) => false,
    }
}

pub(crate) fn collect_unmodelable_reasons_cvc5(_expr: &Expr) -> Vec<String> {
    Vec::new()
}

// -------------------------------------------------------------------------
// Counterexample model filtering
// -------------------------------------------------------------------------

pub(crate) fn is_internal_cvc5_var(name: &str) -> bool {
    name.starts_with("__str_")
        || name.starts_with("__tuple_")
        || name.starts_with("__list_")
        || name.starts_with("__fresh_")
        || name.starts_with("__field_")
        || name.starts_with("__index")
        || name.starts_with("__len")
        || name.starts_with("__arr_")
        || name.starts_with("__domain_contains")
        || name.starts_with("__apply_")
        || name.starts_with("__coerce")
        || name.starts_with("__trigger_")
        || name.starts_with("__list_get")
        || name.starts_with("__result")
        || name.starts_with("__contains")
        || name.starts_with("__obj_")
}

// -------------------------------------------------------------------------
// Lemma apply-ref collection
// -------------------------------------------------------------------------

pub(crate) fn collect_apply_refs_from_expr(expr: &Expr) -> Vec<String> {
    let mut refs = Vec::new();
    collect_apply_refs_inner(expr, &mut refs);
    refs
}

fn collect_apply_refs_inner(expr: &Expr, refs: &mut Vec<String>) {
    match expr {
        Expr::Apply { lemma_name, args } => {
            refs.push(lemma_name.clone());
            for arg in args {
                collect_apply_refs_inner(arg, refs);
            }
        }
        Expr::BinOp { lhs, rhs, .. } => {
            collect_apply_refs_inner(lhs, refs);
            collect_apply_refs_inner(rhs, refs);
        }
        Expr::UnaryOp { expr: inner, .. }
        | Expr::Paren(inner)
        | Expr::Old(inner)
        | Expr::Ghost(inner)
        | Expr::Field(inner, _)
        | Expr::Cast { expr: inner, .. } => {
            collect_apply_refs_inner(inner, refs);
        }
        Expr::Call { func, args } => {
            collect_apply_refs_inner(func, refs);
            for a in args {
                collect_apply_refs_inner(a, refs);
            }
        }
        Expr::MethodCall { receiver, args, .. } => {
            collect_apply_refs_inner(receiver, refs);
            for a in args {
                collect_apply_refs_inner(a, refs);
            }
        }
        Expr::Index { expr: e, index } => {
            collect_apply_refs_inner(e, refs);
            collect_apply_refs_inner(index, refs);
        }
        Expr::Forall { domain, body, .. } | Expr::Exists { domain, body, .. } => {
            collect_apply_refs_inner(domain, refs);
            collect_apply_refs_inner(body, refs);
        }
        Expr::If {
            cond,
            then_branch,
            else_branch,
        } => {
            collect_apply_refs_inner(cond, refs);
            collect_apply_refs_inner(then_branch, refs);
            if let Some(eb) = else_branch {
                collect_apply_refs_inner(eb, refs);
            }
        }
        Expr::Let { value, body, .. } => {
            collect_apply_refs_inner(value, refs);
            collect_apply_refs_inner(body, refs);
        }
        Expr::Match { scrutinee, arms } => {
            collect_apply_refs_inner(scrutinee, refs);
            for a in arms {
                collect_apply_refs_inner(&a.body, refs);
            }
        }
        Expr::List(items) | Expr::Block(items) | Expr::Tuple(items) => {
            for item in items {
                collect_apply_refs_inner(item, refs);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use assura_parser::ast::Expr;

    #[test]
    fn sanitize_dots() {
        assert_eq!(sanitize_smtlib_name("a.b"), "a_b");
    }

    #[test]
    fn flatten_deep_chain() {
        let expr = Expr::Field(
            Box::new(Expr::Field(
                Box::new(Expr::Ident("state".into())),
                "head".into(),
            )),
            "extra".into(),
        );
        assert_eq!(flatten_field_chain_cvc5(&expr), "state__head__extra");
    }

    #[test]
    fn float_rational_encoding() {
        assert_eq!(float_literal_to_smtlib("1.5"), "(/ 1500000 1000000)");
    }
}
