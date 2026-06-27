//! Shared **let / block** encode policy (encode convergence step).
//!
//! Owns solver-neutral shapes for `let name = value in body` (SMT-LIB2 `let`) and
//! blocks (last expression wins; empty block is `true` in SMT-LIB). Term construction
//! for CVC5 native / Z3 (var map insert) stays backend-local.
//!
//! Complements [`crate::encode_atom_policy::sanitize_smt_name`] for bound names.

use assura_ast::SpExpr;

use crate::encode_atom_policy::sanitize_smt_name;

/// SMT-LIB2 for `let name = value in body` via recursive `encode` on value/body.
pub(crate) fn encode_let_smtlib<F>(
    name: &str,
    value: &SpExpr,
    body: &SpExpr,
    mut encode: F,
) -> Option<String>
where
    F: FnMut(&SpExpr) -> Option<String>,
{
    let v = sanitize_smt_name(name);
    let val = encode(value)?;
    let b = encode(body)?;
    Some(format!("(let (({v} {val})) {b})"))
}

/// SMT-LIB2 for a block: last expression, or [`empty_block_smtlib`] if empty.
pub(crate) fn encode_block_smtlib<F>(body: &[SpExpr], mut encode: F) -> Option<String>
where
    F: FnMut(&SpExpr) -> Option<String>,
{
    match classify_block(body) {
        BlockReducePlan::Empty => Some(empty_block_smtlib().to_string()),
        BlockReducePlan::LastExpr => encode(body.last()?),
    }
}

/// How a non-empty block should be reduced (last expr is the value; prior are effects).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BlockReducePlan {
    /// No statements: backends use unit/`true`/fresh as appropriate.
    Empty,
    /// Single or multi: evaluate all for effects; result is last.
    LastExpr,
}

/// Classify block shape for Z3/CVC5 parity (both currently evaluate sequentially, keep last).
pub(crate) fn classify_block(body: &[SpExpr]) -> BlockReducePlan {
    if body.is_empty() {
        BlockReducePlan::Empty
    } else {
        BlockReducePlan::LastExpr
    }
}

/// SMT-LIB empty-block atom (matches shell/history).
pub(crate) fn empty_block_smtlib() -> &'static str {
    "true"
}

#[cfg(test)]
mod tests {
    use super::*;
    use assura_ast::{Expr, Spanned};

    fn sp_ident(n: &str) -> SpExpr {
        Spanned::no_span(Expr::Ident(n.into()))
    }

    #[test]
    fn let_smtlib_shape() {
        let out = encode_let_smtlib("x", &sp_ident("a"), &sp_ident("b"), |e| match &e.node {
            Expr::Ident(n) => Some(n.clone()),
            _ => None,
        });
        assert_eq!(out.as_deref(), Some("(let ((x a)) b)"));
    }

    #[test]
    fn let_sanitizes_dotted_name() {
        let out = encode_let_smtlib("a.b", &sp_ident("1"), &sp_ident("2"), |e| match &e.node {
            Expr::Ident(n) => Some(n.clone()),
            _ => None,
        });
        assert_eq!(out.as_deref(), Some("(let ((a_b 1)) 2)"));
    }

    #[test]
    fn block_last_and_empty() {
        assert_eq!(
            encode_block_smtlib(&[], |_| Some("nope".into())).as_deref(),
            Some("true")
        );
        let body = vec![sp_ident("first"), sp_ident("last")];
        assert_eq!(
            encode_block_smtlib(&body, |e| match &e.node {
                Expr::Ident(n) => Some(n.clone()),
                _ => None,
            })
            .as_deref(),
            Some("last")
        );
        assert_eq!(classify_block(&[]), BlockReducePlan::Empty);
        assert_eq!(classify_block(&body), BlockReducePlan::LastExpr);
        assert_eq!(empty_block_smtlib(), "true");
    }
}
