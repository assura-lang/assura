//! Havoc+assume encoding for result-field verification (#267).
//!
//! Pure contracts without an implementation body treat `result` and
//! `result.length()` as independent symbols unless we add structural
//! axioms and cross-clause length inference. When IR is available,
//! instruction constraints tie `$result` to the encoded body.

use std::collections::HashSet;

use crate::ir::{IrExprKind, IrFunction};
use crate::ir_encode::IrEncodeContext;
use assura_ast::{BinOp, Clause, Expr, SpExpr};

/// Shared havoc+assume clause and IR context for Z3 and CVC5 backends.
pub(crate) struct HavocAssumeInput<'a> {
    pub requires: &'a [&'a Clause],
    pub ensures: &'a [&'a Clause],
    pub return_ty: &'a [String],
    pub param_names: &'a [String],
    pub ir: Option<&'a IrFunction>,
    pub enc_ctx: IrEncodeContext<'a>,
}

/// SMT-LIB2 output target for havoc+assume encoding (shell-out path).
pub(crate) struct HavocAssumeSmtlibTarget<'a> {
    pub script: &'a mut String,
    pub vars: &'a mut HashSet<String>,
}

/// Sentinel slot for `$result` in the IR (matches `parse_slot("$result")`).
pub const RESULT_SLOT: usize = usize::MAX;

/// A length upper-bound pattern: `obj.length() <= bound`.
#[derive(Debug, Clone)]
pub struct LengthBound {
    pub object: String,
    pub bound: SpExpr,
    pub strict: bool,
}

/// Extract `obj.length() <= bound` (or `<`) from an expression tree.
pub fn extract_length_bounds(expr: &SpExpr) -> Vec<LengthBound> {
    let mut out = Vec::new();
    collect_length_bounds(expr, &mut out);
    out
}

fn collect_length_bounds(expr: &SpExpr, out: &mut Vec<LengthBound>) {
    match &expr.node {
        Expr::BinOp { lhs, op, rhs } => match op {
            BinOp::Lte | BinOp::Lt => {
                if let Some(obj) = length_object(lhs) {
                    out.push(LengthBound {
                        object: obj.to_string(),
                        bound: rhs.as_ref().clone(),
                        strict: matches!(op, BinOp::Lt),
                    });
                }
            }
            BinOp::And | BinOp::Or => {
                collect_length_bounds(lhs, out);
                collect_length_bounds(rhs, out);
            }
            _ => {}
        },
        Expr::Ghost(inner) => collect_length_bounds(inner, out),
        Expr::If {
            cond,
            then_branch,
            else_branch,
        } => {
            collect_length_bounds(cond, out);
            collect_length_bounds(then_branch, out);
            if let Some(else_e) = else_branch {
                collect_length_bounds(else_e, out);
            }
        }
        Expr::Block(items) | Expr::Tuple(items) | Expr::List(items) => {
            for e in items {
                collect_length_bounds(e, out);
            }
        }
        _ => {}
    }
}

/// Return the identifier whose `.length()` is called, if any.
pub fn length_object(expr: &SpExpr) -> Option<&str> {
    match &expr.node {
        Expr::MethodCall {
            receiver,
            method,
            args,
        } if (method == "length" || method == "len") && args.is_empty() => {
            match &receiver.as_ref().node {
                Expr::Ident(name) => Some(name.as_str()),
                _ => None,
            }
        }
        Expr::Field(obj, field) if field == "length" || field == "len" => {
            match &obj.as_ref().node {
                Expr::Ident(name) => Some(name.as_str()),
                _ => None,
            }
        }
        _ => None,
    }
}

/// Structural equality for bound expressions (for cross-clause matching).
pub fn expr_structurally_eq(a: &SpExpr, b: &SpExpr) -> bool {
    match (&a.node, &b.node) {
        (Expr::Ident(x), Expr::Ident(y)) => x == y,
        (Expr::Literal(la), Expr::Literal(lb)) => la == lb,
        (
            Expr::BinOp {
                lhs: la,
                op: oa,
                rhs: ra,
            },
            Expr::BinOp {
                lhs: lb,
                op: ob,
                rhs: rb,
            },
        ) => oa == ob && expr_structurally_eq(la, lb) && expr_structurally_eq(ra, rb),

        (
            Expr::MethodCall {
                receiver: ra,
                method: ma,
                args: aa,
            },
            Expr::MethodCall {
                receiver: rb,
                method: mb,
                args: ab,
            },
        ) if ma == mb && aa.len() == ab.len() => {
            expr_structurally_eq(ra, rb)
                && aa
                    .iter()
                    .zip(ab.iter())
                    .all(|(x, y)| expr_structurally_eq(x, y))
        }
        _ => false,
    }
}

/// Infer identity-style length links from requires/ensures clauses.
///
/// Returns pairs `(result_side, input_side)` meaning `len(result) <= len(input)`.
pub fn infer_length_identity_links(
    requires: &[&Clause],
    ensures: &[&Clause],
) -> Vec<(String, String)> {
    let mut links = Vec::new();

    let req_bounds: Vec<LengthBound> = requires
        .iter()
        .flat_map(|c| extract_length_bounds(&c.body))
        .collect();

    for ens in ensures {
        let ens_bounds = extract_length_bounds(&ens.body);
        for eb in &ens_bounds {
            if eb.object != "result" {
                continue;
            }
            for rb in &req_bounds {
                if rb.object == "result" {
                    continue;
                }
                if expr_structurally_eq(&eb.bound, &rb.bound) {
                    push_link(&mut links, "result", &rb.object);
                }
            }
            if let Some(param) = length_object(&eb.bound)
                && param != "result"
            {
                push_link(&mut links, "result", param);
            }
        }
    }

    links
}

fn push_link(links: &mut Vec<(String, String)>, result: &str, input: &str) {
    let pair = (result.to_string(), input.to_string());
    if !links.contains(&pair) {
        links.push(pair);
    }
}

/// Returns true when the return type is a collection-like type.
pub fn is_collection_return(return_ty: &[String]) -> bool {
    matches!(
        return_ty.first().map(String::as_str),
        Some("Bytes") | Some("String") | Some("List") | Some("Vec")
    )
}

/// Map IR parameter slots to contract parameter names by position.
pub fn ir_param_names(func: &IrFunction, contract_param_names: &[String]) -> Vec<(usize, String)> {
    func.params
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let name = contract_param_names
                .get(i)
                .cloned()
                .unwrap_or_else(|| format!("slot_{}", p.slot));
            (p.slot, name)
        })
        .collect()
}

/// Describe an IR expression for diagnostics/tests.
pub fn describe_ir_expr(expr: &IrExprKind) -> String {
    format!("{expr:?}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use assura_ast::{ClauseKind, Literal, Spanned};

    fn sp(e: Expr) -> SpExpr {
        Spanned::no_span(e)
    }

    fn spb(e: Expr) -> Box<SpExpr> {
        Box::new(sp(e))
    }

    fn len_le(obj: &str, bound: SpExpr) -> SpExpr {
        sp(Expr::BinOp {
            lhs: spb(Expr::MethodCall {
                receiver: spb(Expr::Ident(obj.into())),
                method: "length".into(),
                args: vec![],
            }),
            op: BinOp::Lte,
            rhs: Box::new(bound),
        })
    }

    #[test]
    fn extract_length_bound_simple() {
        let e = len_le("raw", sp(Expr::Literal(Literal::Int("100".into()))));
        let bounds = extract_length_bounds(&e);
        assert_eq!(bounds.len(), 1);
        assert_eq!(bounds[0].object, "raw");
    }

    #[test]
    fn infer_cross_clause_same_bound() {
        let n = sp(Expr::Literal(Literal::Int("100".into())));
        let requires = vec![Clause {
            kind: ClauseKind::Requires,
            body: len_le("raw", n.clone()),
            effect_variables: vec![],
        }];
        let ensures = vec![Clause {
            kind: ClauseKind::Ensures,
            body: len_le("result", n),
            effect_variables: vec![],
        }];
        let links = infer_length_identity_links(
            &requires.iter().collect::<Vec<_>>(),
            &ensures.iter().collect::<Vec<_>>(),
        );
        assert!(links.contains(&("result".to_string(), "raw".to_string())));
    }

    #[test]
    fn infer_direct_result_raw_length() {
        let requires = vec![Clause {
            kind: ClauseKind::Requires,
            body: sp(Expr::BinOp {
                lhs: spb(Expr::MethodCall {
                    receiver: spb(Expr::Ident("raw".into())),
                    method: "length".into(),
                    args: vec![],
                }),
                op: BinOp::Gt,
                rhs: spb(Expr::Literal(Literal::Int("0".into()))),
            }),
            effect_variables: vec![],
        }];
        let ensures = vec![Clause {
            kind: ClauseKind::Ensures,
            body: len_le(
                "result",
                sp(Expr::MethodCall {
                    receiver: spb(Expr::Ident("raw".into())),
                    method: "length".into(),
                    args: vec![],
                }),
            ),
            effect_variables: vec![],
        }];
        let links = infer_length_identity_links(
            &requires.iter().collect::<Vec<_>>(),
            &ensures.iter().collect::<Vec<_>>(),
        );
        assert!(links.contains(&("result".to_string(), "raw".to_string())));
    }
}
