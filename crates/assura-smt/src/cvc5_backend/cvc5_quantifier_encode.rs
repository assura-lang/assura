//! Shared quantifier encoding for CVC5 shell-out and native backends.

use assura_parser::ast::Expr;

use crate::cvc5_common::sanitize_smtlib_name;
use crate::cvc5_raw_ops::{
    domain_as_range, domain_contains_guard_smtlib, range_guard_smtlib, wrap_ast_quantifier_smtlib,
};

#[cfg(feature = "cvc5-verify")]
use assura_types::checkers::expr_references_var;

/// Build the domain guard for an AST quantifier in SMT-LIB2.
pub(crate) fn encode_quantifier_domain_guard_smtlib<F>(
    domain: &Expr,
    var: &str,
    mut encode: F,
) -> Option<String>
where
    F: FnMut(&Expr) -> Option<String>,
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
    domain: &Expr,
    body_smt: &str,
    encode_domain: F,
) -> Option<String>
where
    F: FnMut(&Expr) -> Option<String>,
{
    let v = sanitize_smtlib_name(var);
    let guard = encode_quantifier_domain_guard_smtlib(domain, &v, encode_domain)?;
    Some(wrap_ast_quantifier_smtlib(is_forall, &v, &guard, body_smt))
}

/// Combine a domain guard with a quantifier body (native API).
#[cfg(feature = "cvc5-verify")]
pub(crate) fn guard_quantifier_body_cvc5<'a, E>(
    ctx: &mut crate::cvc5_encoder_state::Cvc5QuantifierEncodeCtx<'a>,
    domain: &Expr,
    bound_var: &cvc5::Term<'a>,
    body: cvc5::Term<'a>,
    is_forall: bool,
    mut encode: E,
) -> cvc5::Term<'a>
where
    E: FnMut(
        &Expr,
        &mut crate::cvc5_encoder_state::Cvc5QuantifierEncodeCtx<'a>,
    ) -> Option<cvc5::Term<'a>>,
{
    let guard = if let Some((lo, hi)) = domain_as_range(domain) {
        let lo_val = encode(lo, ctx).unwrap_or_else(|| ctx.tm.mk_integer(0));
        let hi_val = encode(hi, ctx).unwrap_or_else(|| ctx.tm.mk_integer(0));
        let ge_lo = ctx
            .tm
            .mk_term(cvc5::Kind::Geq, &[bound_var.clone(), lo_val]);
        let lt_hi = ctx.tm.mk_term(cvc5::Kind::Lt, &[bound_var.clone(), hi_val]);
        ctx.tm.mk_term(cvc5::Kind::And, &[ge_lo, lt_hi])
    } else {
        let domain_val = encode(domain, ctx)
            .unwrap_or_else(|| ctx.tm.mk_const(ctx.tm.integer_sort(), "__domain_unknown"));
        let contains_sort = ctx.tm.mk_fun_sort(
            &[ctx.tm.integer_sort(), ctx.tm.integer_sort()],
            ctx.tm.boolean_sort(),
        );
        let contains_fn = ctx.tm.mk_const(contains_sort, "__domain_contains");
        ctx.tm.mk_term(
            cvc5::Kind::ApplyUf,
            &[contains_fn, domain_val, bound_var.clone()],
        )
    };
    crate::cvc5_raw_ops::combine_quantifier_guard_cvc5(ctx.tm, is_forall, guard, body)
}

/// Encode an AST `forall`/`exists` as a native CVC5 quantifier (with optional triggers).
#[cfg(feature = "cvc5-verify")]
pub(crate) fn encode_ast_quantifier_cvc5<'a, E>(
    ctx: &mut crate::cvc5_encoder_state::Cvc5QuantifierEncodeCtx<'a>,
    is_forall: bool,
    var: &str,
    domain: &Expr,
    body: &Expr,
    mut encode: E,
) -> Option<cvc5::Term<'a>>
where
    E: FnMut(
        &Expr,
        &mut crate::cvc5_encoder_state::Cvc5QuantifierEncodeCtx<'a>,
    ) -> Option<cvc5::Term<'a>>,
{
    let v_name = sanitize_smtlib_name(var);
    let bound_var = ctx.tm.mk_var(ctx.tm.integer_sort(), &v_name);
    let mut local_vars = ctx.vars.clone();
    local_vars.insert(v_name.clone(), bound_var.clone());
    let mut local_ctx = crate::cvc5_encoder_state::Cvc5QuantifierEncodeCtx {
        tm: ctx.tm,
        vars: &mut local_vars,
        state: ctx.state,
    };
    let b = encode(body, &mut local_ctx)?;
    let guarded = guard_quantifier_body_cvc5(ctx, domain, &bound_var, b, is_forall, &mut encode);
    let bound_list = ctx
        .tm
        .mk_term(cvc5::Kind::VariableList, std::slice::from_ref(&bound_var));
    let trigger_terms = infer_quantifier_patterns_cvc5(ctx.tm, body, &v_name, &bound_var);
    let kind = if is_forall {
        cvc5::Kind::Forall
    } else {
        cvc5::Kind::Exists
    };
    if trigger_terms.is_empty() {
        Some(ctx.tm.mk_term(kind, &[bound_list, guarded]))
    } else {
        let inst_pattern = ctx.tm.mk_term(cvc5::Kind::InstPattern, &trigger_terms);
        Some(ctx.tm.mk_term(kind, &[bound_list, guarded, inst_pattern]))
    }
}

/// Infer CVC5 trigger patterns from function calls referencing the bound variable.
#[cfg(feature = "cvc5-verify")]
pub(crate) fn infer_quantifier_patterns_cvc5<'a>(
    tm: &'a cvc5::TermManager,
    body: &Expr,
    bound_var_name: &str,
    bound_cvc5: &cvc5::Term<'a>,
) -> Vec<cvc5::Term<'a>> {
    let mut patterns = Vec::new();

    let trigger_mgr = crate::advanced::TriggerManager::new();
    let body_str = format!("{body:?}");
    if let Some(trigger) = trigger_mgr.infer_trigger(&body_str) {
        for term in &trigger.terms {
            if let Some(fname) = term.split('(').next() {
                let fname = fname.trim();
                let fun_sort = tm.mk_fun_sort(&[tm.integer_sort()], tm.integer_sort());
                let func = tm.mk_const(fun_sort, fname);
                let app = tm.mk_term(cvc5::Kind::ApplyUf, &[func, bound_cvc5.clone()]);
                patterns.push(app);
            }
        }
    }

    if patterns.is_empty() {
        collect_trigger_calls_cvc5(tm, body, bound_var_name, bound_cvc5, &mut patterns);
    }

    patterns
}

#[cfg(feature = "cvc5-verify")]
fn collect_trigger_calls_cvc5<'a>(
    tm: &'a cvc5::TermManager,
    expr: &Expr,
    bound_var: &str,
    bound_cvc5: &cvc5::Term<'a>,
    patterns: &mut Vec<cvc5::Term<'a>>,
) {
    match expr {
        Expr::Call { func, args } => {
            let refs_bound = args.iter().any(|a| expr_references_var(a, bound_var));
            if refs_bound && let Expr::Ident(fname) = func.as_ref() {
                let arity = args.len();
                let param_sorts: Vec<cvc5::Sort> = (0..arity).map(|_| tm.integer_sort()).collect();
                let fun_sort = tm.mk_fun_sort(&param_sorts, tm.integer_sort());
                let func_decl = tm.mk_const(fun_sort, fname.as_str());
                let mut uf_args = vec![func_decl];
                for a in args {
                    if expr_references_var(a, bound_var) {
                        uf_args.push(bound_cvc5.clone());
                    } else {
                        uf_args.push(tm.mk_const(tm.integer_sort(), "__trigger_other"));
                    }
                }
                let app = tm.mk_term(cvc5::Kind::ApplyUf, &uf_args);
                patterns.push(app);
            }
            for a in args {
                collect_trigger_calls_cvc5(tm, a, bound_var, bound_cvc5, patterns);
            }
        }
        Expr::MethodCall { receiver, args, .. } => {
            collect_trigger_calls_cvc5(tm, receiver, bound_var, bound_cvc5, patterns);
            for a in args {
                collect_trigger_calls_cvc5(tm, a, bound_var, bound_cvc5, patterns);
            }
        }
        Expr::BinOp { lhs, rhs, .. } => {
            collect_trigger_calls_cvc5(tm, lhs, bound_var, bound_cvc5, patterns);
            collect_trigger_calls_cvc5(tm, rhs, bound_var, bound_cvc5, patterns);
        }
        Expr::UnaryOp { expr: e, .. } | Expr::Old(e) | Expr::Ghost(e) => {
            collect_trigger_calls_cvc5(tm, e, bound_var, bound_cvc5, patterns);
        }
        Expr::If {
            cond,
            then_branch,
            else_branch,
        } => {
            collect_trigger_calls_cvc5(tm, cond, bound_var, bound_cvc5, patterns);
            collect_trigger_calls_cvc5(tm, then_branch, bound_var, bound_cvc5, patterns);
            if let Some(eb) = else_branch {
                collect_trigger_calls_cvc5(tm, eb, bound_var, bound_cvc5, patterns);
            }
        }
        Expr::Index { expr: e, index } => {
            collect_trigger_calls_cvc5(tm, e, bound_var, bound_cvc5, patterns);
            collect_trigger_calls_cvc5(tm, index, bound_var, bound_cvc5, patterns);
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use assura_parser::ast::{BinOp, Literal};

    #[test]
    fn range_domain_guard_smtlib() {
        let domain = Expr::BinOp {
            op: BinOp::Range,
            lhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
            rhs: Box::new(Expr::Literal(Literal::Int("10".into()))),
        };
        let guard = encode_quantifier_domain_guard_smtlib(&domain, "x", |e| match e {
            Expr::Literal(Literal::Int(n)) => Some(n.clone()),
            _ => None,
        })
        .unwrap();
        assert_eq!(guard, "(and (>= x 0) (< x 10))");
    }
}
