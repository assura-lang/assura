//! Shared unmodelable-feature detection and field-chain helpers (one compiler brain).
//!
//! Z3 and CVC5 previously maintained nearly identical walkers (`encoder/unmodelable.rs`
//! vs historical CVC5-named helpers). Both backends delegate here so adding/removing
//! an unmodelable case happens once. This is **not** full expression encode unification;
//! it only unifies the pre-solver "can we model this clause body?" gate walk.
//!
//! Complements [`crate::clause_gate_policy`] (outcomes/cache keys) which consumes these
//! predicates without caring which backend invoked them.

use assura_ast::{Expr, ExprVisitor, SpExpr};

/// Returns `true` if the expression tree contains features that the SMT
/// encoder cannot faithfully represent.
///
/// Uses `ExprVisitor` to walk the tree. Override `visit_*` methods in
/// `UnmodelableCheck` to gate specific expression kinds. Currently all
/// expression types are modelable (#198, #200, #201, #262), so the walk
/// finds nothing. Future unmodelable leaves can be added by overriding
/// a single visitor method instead of maintaining a 55-line match block.
pub(crate) fn expr_has_unmodelable_features(expr: &SpExpr) -> bool {
    struct UnmodelableCheck {
        found: bool,
    }

    impl ExprVisitor for UnmodelableCheck {
        fn visit_expr(&mut self, expr: &SpExpr) {
            if self.found {
                return; // short-circuit once any unmodelable feature is found
            }
            assura_ast::walk_expr(self, expr);
        }
        // All expression kinds are currently modelable.
        // To gate a future unmodelable kind, override its visit_* method:
        //
        //   fn visit_some_kind(&mut self, ...) {
        //       self.found = true;
        //   }
    }

    let mut check = UnmodelableCheck { found: false };
    check.visit_expr(expr);
    check.found
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
