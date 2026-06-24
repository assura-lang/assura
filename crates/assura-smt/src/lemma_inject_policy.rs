//! Shared **lemma injection** selection (one compiler brain).
//!
//! Owns which lemma ensures bodies must be asserted when contracts reference
//! lemmas via `apply`. Z3 and CVC5 still encode/assert those bodies with their
//! own term APIs; they must not diverge on *which* lemmas or bodies are chosen.
//!
//! Complements [`crate::verify_labels::collect_lemma_defs`] (lemma map build)
//! and [`crate::prelude_policy`] (when lemma injection runs in the verify order).

use std::collections::HashMap;

use assura_ast::{Clause, Expr, SpExpr};

/// Collect lemma names referenced via `Expr::Apply` in an expression (depth-first, duplicates allowed).
pub(crate) fn collect_apply_refs_from_expr(expr: &SpExpr) -> Vec<String> {
    let mut refs = Vec::new();
    collect_apply_refs_inner(expr, &mut refs);
    refs
}

fn collect_apply_refs_inner(expr: &SpExpr, refs: &mut Vec<String>) {
    match &expr.node {
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

/// Collect lemma names from all clause bodies; sort+dedup for stable injection order.
pub(crate) fn collect_apply_refs_from_clauses(clauses: &[Clause]) -> Vec<String> {
    let mut refs = Vec::new();
    for clause in clauses {
        refs.extend(collect_apply_refs_from_expr(&clause.body));
    }
    refs.sort();
    refs.dedup();
    refs
}

/// Collect lemma names from an iterator of expressions (no sort/dedup; preserves encounter order + duplicates).
///
/// Used by CVC5 per-clause injection (`cvc5-verify` only at call sites).
#[cfg_attr(
    not(feature = "cvc5-verify"),
    allow(dead_code, reason = "CVC5 native injection only")
)]
pub(crate) fn collect_apply_refs_from_exprs<'a, I>(exprs: I) -> Vec<String>
where
    I: IntoIterator<Item = &'a SpExpr>,
{
    let mut refs = Vec::new();
    for e in exprs {
        refs.extend(collect_apply_refs_from_expr(e));
    }
    refs
}

/// Lemma ensures bodies to assert for the given `apply` lemma names.
///
/// Order: lemma names as in `apply_refs`, then each ensures body in definition order.
/// Unknown lemma names are skipped (missing lemmas must not inject nothing silently in
/// backends that treat apply as unconstrained — that is still an encode concern).
#[cfg_attr(
    not(any(feature = "z3-verify", feature = "cvc5-verify")),
    allow(dead_code, reason = "solver backends only")
)]
pub(crate) fn lemma_ensures_bodies_for_refs<'a>(
    apply_refs: &[String],
    lemma_defs: &HashMap<String, Vec<&'a SpExpr>>,
) -> Vec<&'a SpExpr> {
    let mut out = Vec::new();
    for lemma_name in apply_refs {
        if let Some(ensures_bodies) = lemma_defs.get(lemma_name) {
            out.extend(ensures_bodies.iter().copied());
        }
    }
    out
}

/// All ensures bodies to inject for `apply` refs found in the given expressions.
#[cfg_attr(
    not(feature = "cvc5-verify"),
    allow(dead_code, reason = "CVC5 native injection only")
)]
pub(crate) fn lemma_ensures_bodies_for_exprs<'a, I>(
    exprs: I,
    lemma_defs: &HashMap<String, Vec<&'a SpExpr>>,
) -> Vec<&'a SpExpr>
where
    I: IntoIterator<Item = &'a SpExpr>,
{
    let refs = collect_apply_refs_from_exprs(exprs);
    lemma_ensures_bodies_for_refs(&refs, lemma_defs)
}

/// All ensures bodies to inject for `apply` refs in contract clauses (sorted/deduped refs).
#[cfg_attr(
    not(feature = "z3-verify"),
    allow(dead_code, reason = "Z3 contract verify only")
)]
pub(crate) fn lemma_ensures_bodies_for_clauses<'a>(
    clauses: &[Clause],
    lemma_defs: &HashMap<String, Vec<&'a SpExpr>>,
) -> Vec<&'a SpExpr> {
    let refs = collect_apply_refs_from_clauses(clauses);
    lemma_ensures_bodies_for_refs(&refs, lemma_defs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use assura_ast::{ClauseKind, Spanned};

    fn apply_expr(name: &str) -> SpExpr {
        Spanned::no_span(Expr::Apply {
            lemma_name: name.into(),
            args: vec![],
        })
    }

    fn ident(s: &str) -> SpExpr {
        Spanned::no_span(Expr::Ident(s.into()))
    }

    #[test]
    fn collect_apply_refs_nested() {
        let inner = apply_expr("inner");
        let outer = Spanned::no_span(Expr::BinOp {
            op: assura_ast::BinOp::And,
            lhs: Box::new(apply_expr("outer")),
            rhs: Box::new(inner),
        });
        let refs = collect_apply_refs_from_expr(&outer);
        assert_eq!(refs, vec!["outer".to_string(), "inner".to_string()]);
    }

    #[test]
    fn lemma_ensures_selection_skips_missing_and_preserves_order() {
        let e1 = ident("e1");
        let e2 = ident("e2");
        let e3 = ident("e3");
        let mut defs: HashMap<String, Vec<&SpExpr>> = HashMap::new();
        defs.insert("a".into(), vec![&e1, &e2]);
        defs.insert("b".into(), vec![&e3]);
        let refs = vec!["missing".into(), "a".into(), "b".into(), "a".into()];
        let bodies = lemma_ensures_bodies_for_refs(&refs, &defs);
        assert_eq!(bodies.len(), 5);
        // a,a,b,a,a from two a refs (2+2) + one b (1)
        assert!(std::ptr::eq(bodies[0], &e1));
        assert!(std::ptr::eq(bodies[1], &e2));
        assert!(std::ptr::eq(bodies[2], &e3));
        assert!(std::ptr::eq(bodies[3], &e1));
        assert!(std::ptr::eq(bodies[4], &e2));
    }

    #[test]
    fn clauses_path_dedups_refs() {
        let clauses = vec![
            Clause {
                kind: ClauseKind::Ensures,
                body: apply_expr("L"),
                effect_variables: vec![],
            },
            Clause {
                kind: ClauseKind::Requires,
                body: apply_expr("L"),
                effect_variables: vec![],
            },
        ];
        let e = ident("ens");
        let mut defs: HashMap<String, Vec<&SpExpr>> = HashMap::new();
        defs.insert("L".into(), vec![&e]);
        let bodies = lemma_ensures_bodies_for_clauses(&clauses, &defs);
        // deduped refs => inject once
        assert_eq!(bodies.len(), 1);
    }
}
