//! Shared **AST quantifier** encode policy (encode convergence step 3).
//!
//! Owns domain extraction, SMT-LIB quantifier orchestration, and domain-guard
//! UF names used by CVC5 shell/native (and eventually Z3 AST quantifiers).
//! Complements [`crate::encode_raw_ops_policy`] (raw-token quantifier shapes)
//! and [`crate::encode_atom_policy`] (identifier sanitization).
//!
//! Still **not** full `Expr` → solver term encode: Z3 `Encoder` and CVC5 term
//! builders remain separate; only quantifier **policy and SMT-LIB orchestration**
//! live here.

use assura_ast::{BinOp, Expr, SpExpr};

use crate::encode_atom_policy::sanitize_smt_name;
use crate::encode_raw_ops_policy::{
    domain_contains_guard_smtlib, range_guard_smtlib, wrap_ast_quantifier_smtlib,
};

/// Uninterpreted `__domain_contains(domain, var)` UF name (collection quantifiers).
///
/// Used by Z3 `guard_quantifier_body` and CVC5 `guard_quantifier_body_cvc5`.
pub(crate) const DOMAIN_CONTAINS_UF_NAME: &str = "__domain_contains";

/// Placeholder domain constant when encoding fails to produce a domain term.
///
/// Referenced from CVC5 native quantifier encode (`cvc5-verify` only in default builds).
#[cfg_attr(not(feature = "cvc5-verify"), allow(dead_code))]
pub(crate) const DOMAIN_UNKNOWN_NAME: &str = "__domain_unknown";

/// Extract `(lo, hi)` when a quantifier domain is a range expression (`lo..hi`).
pub(crate) fn domain_as_range(domain: &SpExpr) -> Option<(&SpExpr, &SpExpr)> {
    match &domain.node {
        Expr::BinOp {
            op: BinOp::Range,
            lhs,
            rhs,
        } => Some((lhs, rhs)),
        _ => None,
    }
}

/// Build the domain guard for an AST quantifier in SMT-LIB2.
pub(crate) fn encode_quantifier_domain_guard_smtlib<F>(
    domain: &SpExpr,
    var: &str,
    mut encode: F,
) -> Option<String>
where
    F: FnMut(&SpExpr) -> Option<String>,
{
    if let Some((lo, hi)) = domain_as_range(domain) {
        let lo_s = encode(lo)?;
        let hi_s = encode(hi)?;
        Some(range_guard_smtlib(var, &lo_s, &hi_s))
    } else {
        let d = encode(domain).unwrap_or_else(|| var.to_string());
        Some(domain_contains_guard_smtlib(&d, var))
    }
}

/// Encode `forall`/`exists` with domain guard in SMT-LIB2.
pub(crate) fn encode_ast_quantifier_smtlib<F>(
    is_forall: bool,
    var: &str,
    domain: &SpExpr,
    body_smt: &str,
    encode_domain: F,
) -> Option<String>
where
    F: FnMut(&SpExpr) -> Option<String>,
{
    let v = sanitize_smt_name(var);
    let guard = encode_quantifier_domain_guard_smtlib(domain, &v, encode_domain)?;
    Some(wrap_ast_quantifier_smtlib(is_forall, &v, &guard, body_smt))
}

#[cfg(test)]
mod tests {
    use super::*;
    use assura_ast::Spanned;

    fn sp(e: Expr) -> SpExpr {
        Spanned::no_span(e)
    }

    #[test]
    fn domain_range_and_names() {
        assert_eq!(DOMAIN_CONTAINS_UF_NAME, "__domain_contains");
        assert_eq!(DOMAIN_UNKNOWN_NAME, "__domain_unknown");
        let lo = sp(Expr::Ident("0".into()));
        let hi = sp(Expr::Ident("n".into()));
        let domain = sp(Expr::BinOp {
            op: BinOp::Range,
            lhs: Box::new(lo),
            rhs: Box::new(hi),
        });
        let (a, b) = domain_as_range(&domain).expect("range");
        assert!(matches!(&a.node, Expr::Ident(s) if s == "0"));
        assert!(matches!(&b.node, Expr::Ident(s) if s == "n"));
        assert!(domain_as_range(&sp(Expr::Ident("xs".into()))).is_none());
    }

    #[test]
    fn ast_quantifier_smtlib_orchestration() {
        let domain = sp(Expr::BinOp {
            op: BinOp::Range,
            lhs: Box::new(sp(Expr::Ident("0".into()))),
            rhs: Box::new(sp(Expr::Ident("10".into()))),
        });
        let smt = encode_ast_quantifier_smtlib(true, "x", &domain, "(>= x 0)", |e| match &e.node {
            Expr::Ident(n) => Some(n.clone()),
            _ => None,
        })
        .expect("smt");
        assert!(smt.contains("forall"));
        assert!(smt.contains("=>"));
        assert!(smt.contains(">= x 0"));
        assert!(smt.contains("< x 10") || smt.contains("(< x 10)"));
    }
}
