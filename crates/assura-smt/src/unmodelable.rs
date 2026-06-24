//! Shared unmodelable-feature detection and field-chain helpers (one compiler brain).
//!
//! Z3 and CVC5 previously maintained nearly identical walkers (`encoder/unmodelable.rs`
//! vs historical CVC5-named helpers). Both backends delegate here so adding/removing
//! an unmodelable case happens once. This is **not** full expression encode unification;
//! it only unifies the pre-solver "can we model this clause body?" gate walk.
//!
//! Complements [`crate::clause_gate_policy`] (outcomes/cache keys) which consumes these
//! predicates without caring which backend invoked them.

use assura_ast::{Expr, SpExpr};

/// Returns `true` if the expression tree contains features that the SMT
/// encoder cannot faithfully represent.
///
/// Historically gated field/method/raw/taint/typestate paths; most are now modelable
/// (#198, #200, #201, #262). The walk still recurses so future unmodelable leaves can
/// be added in one place without triplicating backends.
pub(crate) fn expr_has_unmodelable_features(expr: &SpExpr) -> bool {
    match &expr.node {
        // #198: Field access is always modelable (flattened / treated as variables).
        Expr::Field(obj, _field) => expr_has_unmodelable_features(obj),
        // #201: Method calls are always modelable (unknown methods → UFs).
        Expr::MethodCall {
            receiver,
            method: _,
            args,
        } => {
            expr_has_unmodelable_features(receiver)
                || args.iter().any(expr_has_unmodelable_features)
        }
        // #200, #262: Raw tokens (taint, ghost, region, validate, typestate @) modelable.
        Expr::Raw(_tokens) => false,
        Expr::BinOp { lhs, rhs, .. } => {
            expr_has_unmodelable_features(lhs) || expr_has_unmodelable_features(rhs)
        }
        Expr::UnaryOp { expr: inner, .. }
        | Expr::Old(inner)
        | Expr::Ghost(inner)
        | Expr::Cast { expr: inner, .. } => expr_has_unmodelable_features(inner),
        Expr::Call { func, args } => {
            expr_has_unmodelable_features(func) || args.iter().any(expr_has_unmodelable_features)
        }
        Expr::Index { expr: e, index } => {
            expr_has_unmodelable_features(e) || expr_has_unmodelable_features(index)
        }
        Expr::Forall { domain, body, .. } | Expr::Exists { domain, body, .. } => {
            expr_has_unmodelable_features(domain) || expr_has_unmodelable_features(body)
        }
        Expr::If {
            cond,
            then_branch,
            else_branch,
        } => {
            expr_has_unmodelable_features(cond)
                || expr_has_unmodelable_features(then_branch)
                || else_branch
                    .as_ref()
                    .is_some_and(|e| expr_has_unmodelable_features(e))
        }
        Expr::Let { value, body, .. } => {
            expr_has_unmodelable_features(value) || expr_has_unmodelable_features(body)
        }
        Expr::Match { scrutinee, arms } => {
            expr_has_unmodelable_features(scrutinee)
                || arms.iter().any(|a| expr_has_unmodelable_features(&a.body))
        }
        Expr::List(items) | Expr::Tuple(items) | Expr::Block(items) => {
            items.iter().any(expr_has_unmodelable_features)
        }
        Expr::Apply { args, .. } => args.iter().any(expr_has_unmodelable_features),
        Expr::Literal(_) | Expr::Ident(_) => false,
    }
}

/// Reasons list for unmodelable features (empty while all expr kinds are modelable).
///
/// Kept for API stability with [`crate::clause_gate_policy::unmodelable_precheck_if`].
pub(crate) fn collect_unmodelable_reasons(_expr: &SpExpr) -> Vec<String> {
    // #198, #200, #201, #262: All expression types are now modelable.
    Vec::new()
}

/// True if `expr` is rooted at the `self` identifier (possibly via field chain).
pub(crate) fn is_self_rooted(expr: &Expr) -> bool {
    match expr {
        Expr::Ident(name) => name == "self",
        Expr::Field(obj, _) => is_self_rooted(&obj.node),
        _ => false,
    }
}

/// `SpExpr` convenience (CVC5 helpers historically took `SpExpr` only).
pub(crate) fn is_self_rooted_sp(expr: &SpExpr) -> bool {
    is_self_rooted(&expr.node)
}

/// Depth of consecutive field accesses (`a.b.c` → 2).
pub(crate) fn field_chain_depth(expr: &Expr) -> usize {
    match expr {
        Expr::Field(obj, _) => 1 + field_chain_depth(&obj.node),
        _ => 0,
    }
}

/// `SpExpr` convenience (used by CVC5 aliases / tests).
#[cfg_attr(
    not(test),
    allow(dead_code, reason = "CVC5 alias + tests; lib callers use Expr path")
)]
pub(crate) fn field_chain_depth_sp(expr: &SpExpr) -> usize {
    field_chain_depth(&expr.node)
}

/// True if field access chain has depth >= 2 (e.g. `state.head.extra`).
pub(crate) fn has_deep_field_chain(expr: &Expr) -> bool {
    field_chain_depth(expr) >= 2
}

pub(crate) fn has_deep_field_chain_sp(expr: &SpExpr) -> bool {
    has_deep_field_chain(&expr.node)
}

/// Flatten `state.head.extra` into `state__head__extra` (Z3/CVC5 variable naming).
pub(crate) fn flatten_field_chain(expr: &Expr) -> String {
    match expr {
        Expr::Field(obj, field) => {
            let prefix = flatten_field_chain(&obj.node);
            format!("{prefix}__{field}")
        }
        Expr::Ident(name) => name.clone(),
        _ => crate::encode_atom_policy::obj_ptr_name(format!("{:p}", expr as *const _)),
    }
}

pub(crate) fn flatten_field_chain_sp(expr: &SpExpr) -> String {
    flatten_field_chain(&expr.node)
}

#[cfg(test)]
mod tests {
    use super::*;
    use assura_ast::{Literal, Spanned};

    fn sp(e: Expr) -> SpExpr {
        Spanned::no_span(e)
    }

    #[test]
    fn literal_and_ident_are_modelable() {
        assert!(!expr_has_unmodelable_features(&sp(Expr::Literal(
            Literal::Bool(true)
        ))));
        assert!(!expr_has_unmodelable_features(&sp(Expr::Ident("x".into()))));
        assert!(collect_unmodelable_reasons(&sp(Expr::Ident("x".into()))).is_empty());
    }

    #[test]
    fn field_and_method_walk_does_not_flag_unmodelable() {
        let field = sp(Expr::Field(
            Box::new(sp(Expr::Ident("buf".into()))),
            "len".into(),
        ));
        assert!(!expr_has_unmodelable_features(&field));
        let method = sp(Expr::MethodCall {
            receiver: Box::new(sp(Expr::Ident("s".into()))),
            method: "length".into(),
            args: vec![],
        });
        assert!(!expr_has_unmodelable_features(&method));
    }

    #[test]
    fn self_rooted_and_field_chain_helpers() {
        let self_id = Expr::Ident("self".into());
        assert!(is_self_rooted(&self_id));
        assert!(!is_self_rooted(&Expr::Ident("other".into())));

        let chain = Expr::Field(
            Box::new(sp(Expr::Field(
                Box::new(sp(Expr::Ident("state".into()))),
                "head".into(),
            ))),
            "extra".into(),
        );
        assert!(has_deep_field_chain(&chain));
        assert_eq!(field_chain_depth(&chain), 2);
        assert_eq!(flatten_field_chain(&chain), "state__head__extra");

        let shallow = Expr::Field(Box::new(sp(Expr::Ident("buf".into()))), "len".into());
        assert!(!has_deep_field_chain(&shallow));
        assert_eq!(flatten_field_chain(&shallow), "buf__len");
    }

    #[test]
    fn sp_wrappers_match_expr_helpers() {
        let se = sp(Expr::Field(
            Box::new(sp(Expr::Ident("self".into()))),
            "x".into(),
        ));
        assert!(is_self_rooted_sp(&se));
        assert_eq!(field_chain_depth_sp(&se), 1);
        assert!(!has_deep_field_chain_sp(&se));
        assert_eq!(flatten_field_chain_sp(&se), "self__x");
    }
}
