//! CVC5 shell SMT-LIB2 expression encoder.
//!
//! Prefer [`crate::encode_*_policy`] for solver-neutral shapes; `cvc5_*_encode`
//! modules remain for term/orchestration that is CVC5-specific.

use assura_ast::{Expr, SpExpr};
use std::sync::OnceLock;

use crate::cvc5_adt::{Cvc5AdtDef, adt_is_constructor_smt, define_adt_cvc5};
use crate::cvc5_atom_encode::{encode_apply_smtlib, encode_ident_smtlib, encode_literal_smtlib};
use crate::cvc5_call_encode::{encode_call_smtlib, encode_method_call_smtlib};
use crate::cvc5_list_encode::encode_list_smtlib;
use crate::cvc5_match_encode::encode_match_smtlib;
use crate::cvc5_old_access::encode_old_smtlib;
use crate::cvc5_raw_encode::encode_raw_expr_smtlib;
use crate::cvc5_tuple_encode::encode_tuple_smtlib;
use crate::cvc5_wrapper_encode::encode_wrapper_smtlib;
use crate::encode_atom_policy::index_access_smtlib;
use crate::encode_binop_policy::{encode_ast_binop_smtlib, encode_ast_unary_smtlib};
use crate::encode_field_policy::{FieldAccessPlan, plan_field_access, shallow_field_smtlib};
use crate::encode_if_policy::encode_if_smtlib;
use crate::encode_let_policy::{encode_block_smtlib, encode_let_smtlib};
use crate::encode_quantifier_policy::encode_ast_quantifier_smtlib;

/// Baseline Option ADT for shell-out match encoding (#263).
static SHELL_MATCH_ADT: OnceLock<Cvc5AdtDef> = OnceLock::new();

fn shell_match_adt_def() -> &'static Cvc5AdtDef {
    SHELL_MATCH_ADT.get_or_init(|| {
        let (def, _) = define_adt_cvc5("Option", &[("Some", &["value"]), ("None", &[])]);
        assert_eq!(def.name, "Option");
        def
    })
}

/// Convert an AST expression to an SMT-LIB2 string representation.
pub fn expr_to_smtlib(expr: &SpExpr) -> Option<String> {
    match &expr.node {
        Expr::Literal(lit) => encode_literal_smtlib(lit),
        Expr::Ident(name) => Some(encode_ident_smtlib(name)),
        Expr::BinOp { op, lhs, rhs } => {
            let l = expr_to_smtlib(lhs)?;
            let r = expr_to_smtlib(rhs)?;
            encode_ast_binop_smtlib(op, &l, &r)
        }
        Expr::UnaryOp { op, expr: inner } => {
            let e = expr_to_smtlib(inner)?;
            Some(encode_ast_unary_smtlib(op, &e))
        }
        Expr::If {
            cond,
            then_branch,
            else_branch,
        } => {
            let c = expr_to_smtlib(cond)?;
            let t = expr_to_smtlib(then_branch)?;
            let e = else_branch.as_ref().and_then(|eb| expr_to_smtlib(eb));
            Some(encode_if_smtlib(&c, &t, e.as_deref()))
        }
        Expr::Forall { var, domain, body } => {
            let b = expr_to_smtlib(body)?;
            encode_ast_quantifier_smtlib(true, var, domain, &b, expr_to_smtlib)
        }
        Expr::Exists { var, domain, body } => {
            let b = expr_to_smtlib(body)?;
            encode_ast_quantifier_smtlib(false, var, domain, &b, expr_to_smtlib)
        }
        Expr::Call { func, args } => encode_call_smtlib(func, args, expr_to_smtlib),
        Expr::Old(inner) => encode_old_smtlib(inner.as_ref(), expr_to_smtlib),
        Expr::Ghost(inner) => encode_wrapper_smtlib(inner, expr_to_smtlib),
        Expr::Cast { expr: inner, .. } => encode_wrapper_smtlib(inner, expr_to_smtlib),
        Expr::Let {
            name, value, body, ..
        } => encode_let_smtlib(name, value, body, expr_to_smtlib),
        Expr::Match {
            scrutinee, arms, ..
        } => encode_match_smtlib(scrutinee, arms, expr_to_smtlib, |name, s| {
            adt_is_constructor_smt("Option", name, s, shell_match_adt_def())
        }),
        Expr::Field(obj, field) => match plan_field_access(obj, field) {
            FieldAccessPlan::CanonicalLength { obj_name } => Some(
                crate::encode_field_policy::canonical_length_field_smtlib(&obj_name),
            ),
            FieldAccessPlan::Flatten(name) => Some(name),
            FieldAccessPlan::ShallowUf { field: f } => {
                let o = expr_to_smtlib(obj)?;
                Some(shallow_field_smtlib(&f, &o))
            }
        },
        Expr::Index { expr: coll, index } => {
            let c = expr_to_smtlib(coll)?;
            let i = expr_to_smtlib(index)?;
            Some(index_access_smtlib(&c, &i))
        }
        Expr::Block(body) => encode_block_smtlib(body, expr_to_smtlib),
        Expr::Raw(tokens) => encode_raw_expr_smtlib(tokens),
        Expr::Tuple(_) => Some(encode_tuple_smtlib()),
        Expr::MethodCall {
            receiver,
            method,
            args,
        } => encode_method_call_smtlib(receiver, method, args, expr_to_smtlib),
        Expr::List(_) => Some(encode_list_smtlib()),
        Expr::Apply { lemma_name, .. } => Some(encode_apply_smtlib(lemma_name)),
    }
}
