//! Shared **`old(expr)` pre-state access** policy (encode convergence step 7).
//!
//! `old(e)` means the value of `e` in the **pre-state** (before the step /
//! mutation), not “deprecated code.” Owns [`OldAccessPlan`] / [`plan_old_access`]
//! so Z3 and CVC5 agree on *which* shape applies (`old(x)` vs `old(obj.f)` vs
//! `old(r.m())`) before backend term construction.
//!
//! Complements [`crate::encode_field_policy`] (field flatten vs shallow UF),
//! [`crate::encode_atom_policy`] (`old_ident_name` / `old_snapshot_name` /
//! `__old` snapshot naming), and [`crate::encode_call_policy`] (method calls).
//!
//! **Naming note:** CVC5 live idents often go through [`encode_ident_name`]
//! (`result` → `__result`), so snapshots use [`old_ident_name`]. Z3 may keep
//! source `result` as `result`, so snapshots use [`old_snapshot_name`]. Planning
//! returns the source ident; backends pick the snapshot function.
//!
//! **Raw tokens:** [`classify_raw_old_inner`] handles `old(x)` / `old(x.f)` in
//! Z3/CVC5 Pratt/raw parsers (inner tokens only, without the `old` keyword).
//!
//! **Compound inners:** [`plan_old_access`] classifies `old(x + 1)` as
//! [`OldAccessPlan::Other`]. Backends must encode [`rewrite_expr_under_old`]
//! (or [`rewrite_raw_tokens_under_old`]) instead of the live inner term, or
//! `old(x + 0)` collapses to post-state `x`.

use std::collections::HashSet;

use assura_ast::{Expr, MatchArm, Pattern, SpExpr, Spanned};

use crate::encode_atom_policy::{field_uif_name, old_snapshot_name};
use crate::encode_field_policy::{FieldAccessPlan, plan_field_access};
use crate::encode_raw_ops_policy::{find_matching_delim, is_raw_spec_skip_keyword, raw_op_info};

/// How `old(inner)` should be encoded (pre-state snapshot strategy).
///
/// Not `PartialEq`: variants hold `SpExpr` boxes (spans are not meaningful to compare).
#[derive(Debug, Clone)]
pub(crate) enum OldAccessPlan {
    /// `old(x)` — snapshot of a simple identifier.
    Ident(String),
    /// `old(a.b.c)` when field policy flattens the chain (`a__b__c` + `__old`).
    FlatField(String),
    /// `old(obj.f)` as shallow field UF on `old(obj)`.
    ShallowField { obj: Box<SpExpr>, field: String },
    /// `old(recv.method(...))` as method UF on `old(recv)`.
    MethodCall {
        receiver: Box<SpExpr>,
        method: String,
    },
    /// Unsupported / complex inner: backends encode [`rewrite_expr_under_old`]
    /// (idents become `old(ident)`), not the live post-state term.
    Other,
}

/// Classify `old(inner)` into an [`OldAccessPlan`] (shared Z3 / CVC5 order).
pub(crate) fn plan_old_access(inner: &SpExpr) -> OldAccessPlan {
    match &inner.node {
        Expr::Ident(name) => OldAccessPlan::Ident(name.clone()),
        Expr::Field(obj, field) => match plan_field_access(obj, field) {
            // old(ident.len) is not a special snapshot; treat as shallow UF on the length field.
            FieldAccessPlan::CanonicalLength { obj_name } => OldAccessPlan::ShallowField {
                obj: Box::new(assura_ast::Spanned::no_span(Expr::Ident(obj_name))),
                field: field.to_string(),
            },
            FieldAccessPlan::Flatten(flat) => OldAccessPlan::FlatField(flat),
            FieldAccessPlan::ShallowUf { field: f } => OldAccessPlan::ShallowField {
                obj: obj.clone(),
                field: f,
            },
        },
        Expr::MethodCall {
            receiver, method, ..
        } => OldAccessPlan::MethodCall {
            receiver: receiver.clone(),
            method: method.clone(),
        },
        _ => OldAccessPlan::Other,
    }
}

/// SMT-LIB2 shape for `old(recv).method` as unary UF apply: `(method old_recv)`.
pub(crate) fn old_method_call_smtlib(method: &str, old_recv_smt: &str) -> String {
    format!("({method} {old_recv_smt})")
}

/// How `old(...)` looks when only **inner** raw tokens are known (no full AST).
///
/// Used by Z3 `parse_raw_expr` / CVC5 `cvc5_raw_smtlib` before falling back to
/// recursive parse or `__old_fresh_*`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RawOldPlan {
    /// Single token: `old(x)` → snapshot name via [`old_snapshot_name`] (source-style).
    Ident(String),
    /// Three tokens `x . f`: shallow field UF on `old(x)` (raw parsers rarely flatten).
    ShallowField { base: String, field: String },
    /// Any other token sequence: backend parses inner or allocates a fresh old temp.
    Complex,
}

/// Classify the token slice inside `old( ... )` (excludes the `old` keyword and parens).
pub(crate) fn classify_raw_old_inner(inner: &[String]) -> RawOldPlan {
    match inner {
        [name] => RawOldPlan::Ident(name.clone()),
        [base, dot, field] if dot == "." => RawOldPlan::ShallowField {
            base: base.clone(),
            field: field.clone(),
        },
        _ => RawOldPlan::Complex,
    }
}

/// Source-style snapshot name for a raw ident (`x` → `x__old`, `result` → `result__old`).
///
/// Matches Z3/CVC5 raw-token paths that do not rewrite live `result` to `__result`.
pub(crate) fn raw_old_ident_snapshot_name(name: &str) -> String {
    old_snapshot_name(name)
}

/// Rewrite a compound `old(inner)` so free identifiers become `old(ident)`.
///
/// `old(x + 1)` is planned as [`OldAccessPlan::Other`]. Encoding the live
/// `x + 1` after havoc uses post-state `x`. Wrapping free names as `old(x)`
/// lets Ident / field / method plans snapshot pre-state, then the operators
/// run on those snapshots (`(+ x__old 1)`).
///
/// Nested `old` is flattened (pre-state of pre-state is pre-state). Bound
/// names in `forall` / `exists` / `let` / match patterns are not wrapped.
/// Call names stay live. Whole fields become `old(field)` so flatten /
/// shallow plans apply; method calls still rewrite receiver and args
/// (wrapping the call would drop arguments).
pub(crate) fn rewrite_expr_under_old(expr: &SpExpr) -> SpExpr {
    rewrite_under_old(expr, &[])
}

fn rewrite_under_old(expr: &SpExpr, bound: &[String]) -> SpExpr {
    let node = match &expr.node {
        Expr::Literal(lit) => Expr::Literal(lit.clone()),
        Expr::Ident(name) => {
            if name == "true" || name == "false" || bound.iter().any(|b| b == name) {
                Expr::Ident(name.clone())
            } else {
                Expr::Old(Box::new(Spanned {
                    node: Expr::Ident(name.clone()),
                    span: expr.span.clone(),
                }))
            }
        }
        Expr::Field(base, field) => {
            if expr_mentions_bound(base, bound) {
                Expr::Field(Box::new(rewrite_under_old(base, bound)), field.clone())
            } else {
                Expr::Old(Box::new(expr.clone()))
            }
        }
        Expr::MethodCall {
            receiver,
            method,
            args,
        } => Expr::MethodCall {
            receiver: Box::new(rewrite_under_old(receiver, bound)),
            method: method.clone(),
            args: args.iter().map(|a| rewrite_under_old(a, bound)).collect(),
        },
        Expr::Call { func, args } => {
            let rewritten_func = match &func.node {
                Expr::Ident(_) => func.as_ref().clone(),
                _ => rewrite_under_old(func, bound),
            };
            Expr::Call {
                func: Box::new(rewritten_func),
                args: args.iter().map(|a| rewrite_under_old(a, bound)).collect(),
            }
        }
        Expr::Index { expr: coll, index } => Expr::Index {
            expr: Box::new(rewrite_under_old(coll, bound)),
            index: Box::new(rewrite_under_old(index, bound)),
        },
        Expr::BinOp { lhs, op, rhs } => Expr::BinOp {
            lhs: Box::new(rewrite_under_old(lhs, bound)),
            op: op.clone(),
            rhs: Box::new(rewrite_under_old(rhs, bound)),
        },
        Expr::UnaryOp { op, expr: inner } => Expr::UnaryOp {
            op: op.clone(),
            expr: Box::new(rewrite_under_old(inner, bound)),
        },
        Expr::Old(inner) => return rewrite_under_old(inner, bound),
        Expr::Forall { var, domain, body } => {
            let mut inner_bound = bound.to_vec();
            inner_bound.push(var.clone());
            Expr::Forall {
                var: var.clone(),
                domain: Box::new(rewrite_under_old(domain, bound)),
                body: Box::new(rewrite_under_old(body, &inner_bound)),
            }
        }
        Expr::Exists { var, domain, body } => {
            let mut inner_bound = bound.to_vec();
            inner_bound.push(var.clone());
            Expr::Exists {
                var: var.clone(),
                domain: Box::new(rewrite_under_old(domain, bound)),
                body: Box::new(rewrite_under_old(body, &inner_bound)),
            }
        }
        Expr::If {
            cond,
            then_branch,
            else_branch,
        } => Expr::If {
            cond: Box::new(rewrite_under_old(cond, bound)),
            then_branch: Box::new(rewrite_under_old(then_branch, bound)),
            else_branch: else_branch
                .as_ref()
                .map(|e| Box::new(rewrite_under_old(e, bound))),
        },
        Expr::List(items) => {
            Expr::List(items.iter().map(|e| rewrite_under_old(e, bound)).collect())
        }
        Expr::Cast { expr: inner, ty } => Expr::Cast {
            expr: Box::new(rewrite_under_old(inner, bound)),
            ty: ty.clone(),
        },
        Expr::Block(exprs) => {
            Expr::Block(exprs.iter().map(|e| rewrite_under_old(e, bound)).collect())
        }
        Expr::Ghost(inner) => Expr::Ghost(Box::new(rewrite_under_old(inner, bound))),
        Expr::Apply { lemma_name, args } => Expr::Apply {
            lemma_name: lemma_name.clone(),
            args: args.iter().map(|a| rewrite_under_old(a, bound)).collect(),
        },
        Expr::Let { name, value, body } => {
            let mut inner_bound = bound.to_vec();
            inner_bound.push(name.clone());
            Expr::Let {
                name: name.clone(),
                value: Box::new(rewrite_under_old(value, bound)),
                body: Box::new(rewrite_under_old(body, &inner_bound)),
            }
        }
        Expr::Match { scrutinee, arms } => Expr::Match {
            scrutinee: Box::new(rewrite_under_old(scrutinee, bound)),
            arms: arms
                .iter()
                .map(|arm| {
                    let mut inner_bound = bound.to_vec();
                    collect_pattern_binds(&arm.pattern, &mut inner_bound);
                    MatchArm {
                        pattern: arm.pattern.clone(),
                        body: rewrite_under_old(&arm.body, &inner_bound),
                    }
                })
                .collect(),
        },
        Expr::Tuple(items) => {
            Expr::Tuple(items.iter().map(|e| rewrite_under_old(e, bound)).collect())
        }
        Expr::Raw(tokens) => Expr::Raw(rewrite_raw_tokens_under_old(tokens)),
    };
    Spanned {
        node,
        span: expr.span.clone(),
    }
}

/// True if `expr` uses a name from `bound` (so wrapping the whole node as
/// `old(...)` would snapshot a binder as if it were pre-state).
fn expr_mentions_bound(expr: &SpExpr, bound: &[String]) -> bool {
    if bound.is_empty() {
        return false;
    }
    match &expr.node {
        Expr::Ident(name) => bound.iter().any(|b| b == name),
        Expr::Field(base, _)
        | Expr::UnaryOp { expr: base, .. }
        | Expr::Old(base)
        | Expr::Ghost(base)
        | Expr::Cast { expr: base, .. } => expr_mentions_bound(base, bound),
        Expr::Index { expr: coll, index } => {
            expr_mentions_bound(coll, bound) || expr_mentions_bound(index, bound)
        }
        Expr::BinOp { lhs, rhs, .. } => {
            expr_mentions_bound(lhs, bound) || expr_mentions_bound(rhs, bound)
        }
        Expr::MethodCall { receiver, args, .. } => {
            expr_mentions_bound(receiver, bound)
                || args.iter().any(|a| expr_mentions_bound(a, bound))
        }
        Expr::Call { func, args } => {
            expr_mentions_bound(func, bound) || args.iter().any(|a| expr_mentions_bound(a, bound))
        }
        Expr::List(items) | Expr::Tuple(items) | Expr::Block(items) => {
            items.iter().any(|e| expr_mentions_bound(e, bound))
        }
        Expr::If {
            cond,
            then_branch,
            else_branch,
        } => {
            expr_mentions_bound(cond, bound)
                || expr_mentions_bound(then_branch, bound)
                || else_branch
                    .as_ref()
                    .is_some_and(|e| expr_mentions_bound(e, bound))
        }
        Expr::Apply { args, .. } => args.iter().any(|a| expr_mentions_bound(a, bound)),
        Expr::Forall { domain, body, .. } | Expr::Exists { domain, body, .. } => {
            expr_mentions_bound(domain, bound) || expr_mentions_bound(body, bound)
        }
        Expr::Let { value, body, .. } => {
            expr_mentions_bound(value, bound) || expr_mentions_bound(body, bound)
        }
        Expr::Match { scrutinee, arms } => {
            expr_mentions_bound(scrutinee, bound)
                || arms.iter().any(|a| expr_mentions_bound(&a.body, bound))
        }
        Expr::Literal(_) | Expr::Raw(_) => false,
    }
}

fn collect_pattern_binds(pattern: &Pattern, bound: &mut Vec<String>) {
    match pattern {
        Pattern::Ident(name) => bound.push(name.clone()),
        Pattern::Literal(_) | Pattern::Wildcard => {}
        Pattern::Constructor { name: _, fields } => {
            for field in fields {
                collect_pattern_binds(field, bound);
            }
        }
        Pattern::Tuple(fields) => {
            for field in fields {
                collect_pattern_binds(field, bound);
            }
        }
    }
}

fn is_raw_old_non_state_token(tok: &str) -> bool {
    if tok.is_empty() {
        return true;
    }
    if raw_op_info(tok).is_some() || is_raw_spec_skip_keyword(tok) {
        return true;
    }
    matches!(
        tok,
        "(" | ")"
            | "["
            | "]"
            | "{"
            | "}"
            | ","
            | "."
            | ":"
            | ";"
            | "="
            | "->"
            | "=>"
            | "_"
            | "true"
            | "false"
            | "old"
            | "forall"
            | "exists"
            | "not"
            | "and"
            | "or"
            | "implies"
            | "mod"
            | "div"
            | "in"
            | "if"
            | "then"
            | "else"
            | "let"
            | "match"
            | "as"
            | "ghost"
            | "apply"
    ) || tok.chars().next().is_some_and(|c| c.is_ascii_digit())
        || !tok
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
}

/// Snapshot free identifier tokens inside `old( ... )` raw inners.
///
/// `["x", "+", "1"]` becomes `["x__old", "+", "1"]`. Nested `old ( y )`
/// collapses to the snapshot. Field names after `.` and names followed
/// by `(` stay live. Raw `forall` / `let` binders are heuristic (the
/// rest of the token slice); precise binders need the AST rewrite.
pub(crate) fn rewrite_raw_tokens_under_old(tokens: &[String]) -> Vec<String> {
    rewrite_raw_tokens_under_old_bound(tokens, &HashSet::new())
}

fn rewrite_raw_tokens_under_old_bound(tokens: &[String], bound: &HashSet<String>) -> Vec<String> {
    let mut out = Vec::with_capacity(tokens.len());
    let mut i = 0;
    while i < tokens.len() {
        let tok = &tokens[i];

        if tok == "old"
            && i + 1 < tokens.len()
            && tokens[i + 1] == "("
            && let Some(close) = find_matching_delim(tokens, i + 1, "(", ")")
        {
            let inner = &tokens[i + 2..close];
            out.extend(rewrite_raw_tokens_under_old_bound(inner, bound));
            i = close + 1;
            continue;
        }

        if (tok == "forall" || tok == "exists")
            && i + 1 < tokens.len()
            && !is_raw_old_non_state_token(&tokens[i + 1])
        {
            out.push(tok.clone());
            let var = tokens[i + 1].clone();
            out.push(var.clone());
            i += 2;
            let mut body_bound = bound.clone();
            body_bound.insert(var);
            out.extend(rewrite_raw_tokens_under_old_bound(
                &tokens[i..],
                &body_bound,
            ));
            break;
        }

        if tok == "let" && i + 1 < tokens.len() && !is_raw_old_non_state_token(&tokens[i + 1]) {
            out.push(tok.clone());
            let name = tokens[i + 1].clone();
            out.push(name.clone());
            i += 2;
            let mut body_bound = bound.clone();
            body_bound.insert(name);
            out.extend(rewrite_raw_tokens_under_old_bound(
                &tokens[i..],
                &body_bound,
            ));
            break;
        }

        if tok == "." {
            out.push(tok.clone());
            i += 1;
            if i < tokens.len() {
                out.push(tokens[i].clone());
                i += 1;
            }
            continue;
        }

        if !is_raw_old_non_state_token(tok)
            && i + 1 < tokens.len()
            && tokens[i + 1] == "("
            && !bound.contains(tok.as_str())
        {
            out.push(tok.clone());
            i += 1;
            continue;
        }

        if is_raw_old_non_state_token(tok) || bound.contains(tok.as_str()) {
            out.push(tok.clone());
        } else {
            out.push(raw_old_ident_snapshot_name(tok));
        }
        i += 1;
    }
    out
}

/// Allocate a fresh temporary for complex `old()` inner expressions that
/// the raw token parser could not resolve.
///
/// Wraps [`old_fresh_temp_name`](crate::encode_atom_policy::old_fresh_temp_name)
/// with counter increment so both CVC5 shell and native raw parsers use the
/// same `__old_fresh_N` naming and avoid collisions.
pub(crate) fn allocate_old_complex_fresh(counter: &mut usize) -> String {
    let name = crate::encode_atom_policy::old_fresh_temp_name(*counter);
    *counter += 1;
    name
}

/// SMT-LIB2 for raw `old(base.field)` as `(__field_f base__old)`.
pub(crate) fn raw_old_shallow_field_smtlib(base: &str, field: &str) -> String {
    let old_base = old_snapshot_name(base);
    format!("({} {old_base})", field_uif_name(field))
}

#[cfg(test)]
mod tests {
    use super::*;
    use assura_ast::Spanned;

    #[test]
    fn old_ident_plan() {
        assert!(matches!(
            plan_old_access(&Spanned::no_span(Expr::Ident("x".into()))),
            OldAccessPlan::Ident(name) if name == "x"
        ));
    }

    #[test]
    fn old_shallow_field_plan() {
        let obj = Box::new(Spanned::no_span(Expr::Ident("buf".into())));
        let inner = Spanned::no_span(Expr::Field(obj, "len".into()));
        // ident.len is FieldAccessPlan::CanonicalLength; old() maps it to shallow on the ident.
        assert!(matches!(
            plan_old_access(&inner),
            OldAccessPlan::ShallowField { field, .. } if field == "len"
        ));
    }

    #[test]
    fn old_non_length_shallow_field() {
        let obj = Box::new(Spanned::no_span(Expr::Ident("buf".into())));
        let inner = Spanned::no_span(Expr::Field(obj, "head".into()));
        assert!(matches!(
            plan_old_access(&inner),
            OldAccessPlan::ShallowField { field, .. } if field == "head"
        ));
    }

    #[test]
    fn old_self_rooted_field_flattens() {
        let obj = Box::new(Spanned::no_span(Expr::Ident("self".into())));
        let inner = Spanned::no_span(Expr::Field(obj, "head".into()));
        assert!(matches!(
            plan_old_access(&inner),
            OldAccessPlan::FlatField(name) if name == "self__head"
        ));
    }

    #[test]
    fn old_method_call_smtlib_shape() {
        assert_eq!(
            old_method_call_smtlib("length", "buf__old"),
            "(length buf__old)"
        );
    }

    #[test]
    fn classify_raw_old_inner_shapes() {
        assert_eq!(
            classify_raw_old_inner(&[String::from("x")]),
            RawOldPlan::Ident("x".into())
        );
        assert_eq!(
            classify_raw_old_inner(&["buf".into(), ".".into(), "len".into()]),
            RawOldPlan::ShallowField {
                base: "buf".into(),
                field: "len".into(),
            }
        );
        assert_eq!(
            classify_raw_old_inner(&["a".into(), "+".into(), "b".into()]),
            RawOldPlan::Complex
        );
        assert_eq!(raw_old_ident_snapshot_name("result"), "result__old");
        assert_eq!(
            raw_old_shallow_field_smtlib("buf", "len"),
            "(__field_len buf__old)"
        );
    }

    #[test]
    fn allocate_old_complex_fresh_increments() {
        let mut counter = 0;
        assert_eq!(allocate_old_complex_fresh(&mut counter), "__old_fresh_0");
        assert_eq!(counter, 1);
        assert_eq!(allocate_old_complex_fresh(&mut counter), "__old_fresh_1");
        assert_eq!(counter, 2);
    }

    #[test]
    fn rewrite_sum_wraps_free_ident() {
        let inner = Spanned::no_span(Expr::BinOp {
            lhs: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
            op: assura_ast::BinOp::Add,
            rhs: Box::new(Spanned::no_span(Expr::Literal(assura_ast::Literal::Int(
                "1".into(),
            )))),
        });
        let rewritten = rewrite_expr_under_old(&inner);
        match &rewritten.node {
            Expr::BinOp { lhs, op, rhs } => {
                assert_eq!(*op, assura_ast::BinOp::Add);
                assert!(matches!(
                    &lhs.node,
                    Expr::Old(inner) if matches!(&inner.node, Expr::Ident(n) if n == "x")
                ));
                assert!(matches!(
                    &rhs.node,
                    Expr::Literal(assura_ast::Literal::Int(n)) if n == "1"
                ));
            }
            other => panic!("expected binop, got {other:?}"),
        }
    }

    #[test]
    fn rewrite_leaves_true_and_bound_forall_var() {
        let body = Spanned::no_span(Expr::BinOp {
            lhs: Box::new(Spanned::no_span(Expr::Ident("i".into()))),
            op: assura_ast::BinOp::Lt,
            rhs: Box::new(Spanned::no_span(Expr::Ident("n".into()))),
        });
        let inner = Spanned::no_span(Expr::Forall {
            var: "i".into(),
            domain: Box::new(Spanned::no_span(Expr::Ident("xs".into()))),
            body: Box::new(body),
        });
        let rewritten = rewrite_expr_under_old(&inner);
        let Expr::Forall { var, domain, body } = &rewritten.node else {
            panic!("expected forall");
        };
        assert_eq!(var, "i");
        assert!(matches!(
            &domain.node,
            Expr::Old(inner) if matches!(&inner.node, Expr::Ident(n) if n == "xs")
        ));
        let Expr::BinOp { lhs, rhs, .. } = &body.node else {
            panic!("expected body binop");
        };
        assert!(matches!(&lhs.node, Expr::Ident(n) if n == "i"));
        assert!(matches!(
            &rhs.node,
            Expr::Old(inner) if matches!(&inner.node, Expr::Ident(n) if n == "n")
        ));
    }

    #[test]
    fn rewrite_raw_tokens_snapshots_idents_not_calls_or_fields() {
        assert_eq!(
            rewrite_raw_tokens_under_old(&["x".into(), "+".into(), "1".into()]),
            vec!["x__old", "+", "1"]
        );
        assert_eq!(
            rewrite_raw_tokens_under_old(&["buf".into(), ".".into(), "len".into()]),
            vec!["buf__old", ".", "len"]
        );
        assert_eq!(
            rewrite_raw_tokens_under_old(&["abs".into(), "(".into(), "x".into(), ")".into()]),
            vec!["abs", "(", "x__old", ")"]
        );
    }

    #[test]
    fn rewrite_leaves_bound_let_var() {
        let inner = Spanned::no_span(Expr::Let {
            name: "t".into(),
            value: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
            body: Box::new(Spanned::no_span(Expr::BinOp {
                lhs: Box::new(Spanned::no_span(Expr::Ident("t".into()))),
                op: assura_ast::BinOp::Add,
                rhs: Box::new(Spanned::no_span(Expr::Ident("y".into()))),
            })),
        });
        let rewritten = rewrite_expr_under_old(&inner);
        let Expr::Let { name, value, body } = &rewritten.node else {
            panic!("expected let");
        };
        assert_eq!(name, "t");
        assert!(matches!(
            &value.node,
            Expr::Old(inner) if matches!(&inner.node, Expr::Ident(n) if n == "x")
        ));
        let Expr::BinOp { lhs, rhs, .. } = &body.node else {
            panic!("expected body binop");
        };
        assert!(matches!(&lhs.node, Expr::Ident(n) if n == "t"));
        assert!(matches!(
            &rhs.node,
            Expr::Old(inner) if matches!(&inner.node, Expr::Ident(n) if n == "y")
        ));
    }

    #[test]
    fn rewrite_nested_old_flattens_to_prestate() {
        let inner = Spanned::no_span(Expr::Old(Box::new(Spanned::no_span(Expr::Ident(
            "x".into(),
        )))));
        let rewritten = rewrite_expr_under_old(&inner);
        assert!(matches!(
            &rewritten.node,
            Expr::Old(inner) if matches!(&inner.node, Expr::Ident(n) if n == "x")
        ));
    }

    #[test]
    fn rewrite_bound_field_does_not_snapshot_binder() {
        let field = Spanned::no_span(Expr::Field(
            Box::new(Spanned::no_span(Expr::Ident("t".into()))),
            "n".into(),
        ));
        let inner = Spanned::no_span(Expr::Let {
            name: "t".into(),
            value: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
            body: Box::new(field),
        });
        let rewritten = rewrite_expr_under_old(&inner);
        let Expr::Let { body, .. } = &rewritten.node else {
            panic!("expected let");
        };
        assert!(
            matches!(&body.node, Expr::Field(_, f) if f == "n"),
            "bound t.n must stay a field of t, not old(t.n): {body:?}"
        );
        let Expr::Field(base, _) = &body.node else {
            panic!("expected field");
        };
        assert!(
            matches!(&base.node, Expr::Ident(n) if n == "t"),
            "binder t must stay live: {base:?}"
        );
    }

    #[test]
    fn rewrite_field_wraps_as_old_for_flatten_plan() {
        let obj = Box::new(Spanned::no_span(Expr::Ident("self".into())));
        let field = Spanned::no_span(Expr::Field(obj, "head".into()));
        let inner = Spanned::no_span(Expr::BinOp {
            lhs: Box::new(field),
            op: assura_ast::BinOp::Add,
            rhs: Box::new(Spanned::no_span(Expr::Literal(assura_ast::Literal::Int(
                "1".into(),
            )))),
        });
        let rewritten = rewrite_expr_under_old(&inner);
        let Expr::BinOp { lhs, .. } = &rewritten.node else {
            panic!("expected binop");
        };
        assert!(matches!(
            &lhs.node,
            Expr::Old(inner) if matches!(&inner.node, Expr::Field(_, f) if f == "head")
        ));
    }

    #[test]
    fn rewrite_raw_tokens_nested_old_and_field() {
        assert_eq!(
            rewrite_raw_tokens_under_old(&[
                "old".into(),
                "(".into(),
                "y".into(),
                ")".into(),
                "+".into(),
                "x".into(),
            ]),
            vec!["y__old", "+", "x__old"]
        );
        assert_eq!(
            rewrite_raw_tokens_under_old(&["forall".into(), "i".into(), "in".into(), "xs".into()]),
            vec!["forall", "i", "in", "xs__old"]
        );
    }
}
