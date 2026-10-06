//! Loop-invariant and callee-ensures assumptions for check-rust body folding.
//!
//! An annotated loop havocs the locals it assigns and records the invariant
//! as a later IR `post:` axiom. A call whose callee has plain `@ensures`
//! havocs the result and records that postcondition the same way.
//!
//! The axiom is an assumption. This module does not prove that a loop
//! invariant holds on entry or that the body preserves it. Predicates that
//! do not mention a havoc'd name are dropped so an annotation cannot
//! constrain an input the loop never assigns. Missing annotations stay
//! fail-closed in the folder.

use std::collections::HashMap;

#[derive(Clone, Debug)]
pub(super) struct CalleeSpec {
    pub(super) params: Vec<String>,
    pub(super) ensures: Vec<String>,
}

/// Plain `@ensures` on free functions in this module only.
///
/// Nested modules and impl methods are different items. A bare call does
/// not resolve to them, so their postconditions must not be assumed.
/// `ensures_ok` / `ensures_err` are not facts about every return value.
pub(super) fn sibling_free_fn_specs(items: &[syn::Item]) -> HashMap<String, CalleeSpec> {
    let mut out = HashMap::new();
    for item in items {
        let syn::Item::Fn(func) = item else {
            continue;
        };
        let Some((name, spec)) = fn_spec(&func.sig, &func.attrs) else {
            continue;
        };
        out.entry(name).or_insert(spec);
    }
    out
}

fn fn_spec(sig: &syn::Signature, attrs: &[syn::Attribute]) -> Option<(String, CalleeSpec)> {
    let ensures = clause_bodies(attrs, &["ensures"]);
    if ensures.is_empty() {
        return None;
    }
    let mut params = Vec::new();
    for input in &sig.inputs {
        match input {
            syn::FnArg::Receiver(_) => return None,
            syn::FnArg::Typed(pat) => params.push(pat_name(&pat.pat)?),
        }
    }
    Some((sig.ident.to_string(), CalleeSpec { params, ensures }))
}

fn pat_name(pat: &syn::Pat) -> Option<String> {
    match pat {
        syn::Pat::Ident(id) if id.by_ref.is_none() && id.subpat.is_none() => {
            Some(id.ident.to_string())
        }
        syn::Pat::Type(inner) => pat_name(&inner.pat),
        syn::Pat::Reference(inner) => pat_name(&inner.pat),
        _ => None,
    }
}

/// Doc `@keyword body` lines and `#[keyword(body)]` lists, in source order.
pub(super) fn clause_bodies(attrs: &[syn::Attribute], keys: &[&str]) -> Vec<String> {
    let mut out = Vec::new();
    for attr in attrs {
        if attr.path().is_ident("doc") {
            if let syn::Meta::NameValue(nv) = &attr.meta
                && let syn::Expr::Lit(syn::ExprLit {
                    lit: syn::Lit::Str(lit_str),
                    ..
                }) = &nv.value
                && let Some(body) = doc_clause(&lit_str.value(), keys)
            {
                out.push(body);
            }
            continue;
        }
        if !keys.iter().any(|key| attr.path().is_ident(key)) {
            continue;
        }
        if let syn::Meta::List(list) = &attr.meta {
            let body = list.tokens.to_string();
            let body = body.trim();
            if !body.is_empty() {
                out.push(body.to_string());
            }
        }
    }
    out
}

fn doc_clause(line: &str, keys: &[&str]) -> Option<String> {
    let rest = line.trim().strip_prefix('@')?;
    let (keyword, body) = match rest.find(char::is_whitespace) {
        Some(index) => (&rest[..index], rest[index + 1..].trim()),
        None => (rest, ""),
    };
    if keys.contains(&keyword) && !body.is_empty() {
        Some(body.to_string())
    } else {
        None
    }
}

pub(super) fn loop_attrs(expr: &syn::Expr) -> &[syn::Attribute] {
    match expr {
        syn::Expr::While(w) => w.attrs.as_slice(),
        syn::Expr::ForLoop(f) => f.attrs.as_slice(),
        syn::Expr::Loop(l) => l.attrs.as_slice(),
        _ => &[],
    }
}

pub(super) fn simple_call(expr: &syn::Expr) -> Option<&syn::ExprCall> {
    match expr {
        syn::Expr::Call(call) => Some(call),
        syn::Expr::Paren(inner) => simple_call(&inner.expr),
        syn::Expr::Group(inner) => simple_call(&inner.expr),
        _ => None,
    }
}

pub(super) fn call_fn_name(func: &syn::Expr) -> Option<String> {
    match func {
        syn::Expr::Path(path) if path.qself.is_none() && path.path.segments.len() == 1 => {
            Some(path.path.segments[0].ident.to_string())
        }
        syn::Expr::Paren(inner) => call_fn_name(&inner.expr),
        syn::Expr::Group(inner) => call_fn_name(&inner.expr),
        _ => None,
    }
}

/// True when `expr` may pass a mutable place, or when the shape is not
/// walked. Unknown shapes fail closed so a hidden `&mut` is not ignored.
pub(super) fn expr_has_mut_ref(expr: &syn::Expr) -> bool {
    match expr {
        syn::Expr::Reference(reference) => {
            reference.mutability.is_some() || expr_has_mut_ref(&reference.expr)
        }
        syn::Expr::Paren(inner) => expr_has_mut_ref(&inner.expr),
        syn::Expr::Group(inner) => expr_has_mut_ref(&inner.expr),
        syn::Expr::Cast(cast) => expr_has_mut_ref(&cast.expr),
        syn::Expr::Unary(unary) => expr_has_mut_ref(&unary.expr),
        syn::Expr::Field(field) => expr_has_mut_ref(&field.base),
        syn::Expr::Binary(binary) => {
            expr_has_mut_ref(&binary.left) || expr_has_mut_ref(&binary.right)
        }
        syn::Expr::Call(call) => {
            expr_has_mut_ref(&call.func) || call.args.iter().any(expr_has_mut_ref)
        }
        syn::Expr::MethodCall(call) => {
            expr_has_mut_ref(&call.receiver) || call.args.iter().any(expr_has_mut_ref)
        }
        syn::Expr::Tuple(tuple) => tuple.elems.iter().any(expr_has_mut_ref),
        syn::Expr::Array(array) => array.elems.iter().any(expr_has_mut_ref),
        syn::Expr::Repeat(repeat) => {
            expr_has_mut_ref(&repeat.expr) || expr_has_mut_ref(&repeat.len)
        }
        syn::Expr::Index(index) => expr_has_mut_ref(&index.expr) || expr_has_mut_ref(&index.index),
        syn::Expr::Range(range) => {
            range
                .start
                .as_ref()
                .is_some_and(|start| expr_has_mut_ref(start))
                || range.end.as_ref().is_some_and(|end| expr_has_mut_ref(end))
        }
        syn::Expr::Block(block) => block.block.stmts.iter().any(stmt_has_mut_ref),
        syn::Expr::If(if_expr) => {
            expr_has_mut_ref(&if_expr.cond)
                || if_expr.then_branch.stmts.iter().any(stmt_has_mut_ref)
                || if_expr
                    .else_branch
                    .as_ref()
                    .is_some_and(|(_, else_expr)| expr_has_mut_ref(else_expr))
        }
        syn::Expr::Struct(struct_expr) => {
            struct_expr
                .fields
                .iter()
                .any(|field| expr_has_mut_ref(&field.expr))
                || struct_expr
                    .rest
                    .as_ref()
                    .is_some_and(|rest| expr_has_mut_ref(rest))
        }
        syn::Expr::Path(_) | syn::Expr::Lit(_) => false,
        _ => true,
    }
}

fn stmt_has_mut_ref(stmt: &syn::Stmt) -> bool {
    match stmt {
        syn::Stmt::Expr(expr, _) => expr_has_mut_ref(expr),
        syn::Stmt::Local(local) => local.init.as_ref().is_some_and(|init| {
            expr_has_mut_ref(&init.expr)
                || init
                    .diverge
                    .as_ref()
                    .is_some_and(|(_, else_expr)| expr_has_mut_ref(else_expr))
        }),
        _ => true,
    }
}

pub(super) fn ident_expr(name: &str) -> Option<syn::Expr> {
    syn::parse_str(name).ok()
}

pub(super) fn expr_mentions_ident(expr: &syn::Expr, name: &str) -> bool {
    match expr {
        syn::Expr::Path(path) => path.path.is_ident(name),
        syn::Expr::Binary(binary) => {
            expr_mentions_ident(&binary.left, name) || expr_mentions_ident(&binary.right, name)
        }
        syn::Expr::Unary(unary) => expr_mentions_ident(&unary.expr, name),
        syn::Expr::Paren(inner) => expr_mentions_ident(&inner.expr, name),
        syn::Expr::Group(inner) => expr_mentions_ident(&inner.expr, name),
        syn::Expr::Reference(inner) => expr_mentions_ident(&inner.expr, name),
        syn::Expr::Cast(cast) => expr_mentions_ident(&cast.expr, name),
        syn::Expr::Field(field) => expr_mentions_ident(&field.base, name),
        syn::Expr::Index(index) => {
            expr_mentions_ident(&index.expr, name) || expr_mentions_ident(&index.index, name)
        }
        syn::Expr::Call(call) => {
            expr_mentions_ident(&call.func, name)
                || call.args.iter().any(|arg| expr_mentions_ident(arg, name))
        }
        syn::Expr::Block(block) => block
            .block
            .stmts
            .iter()
            .any(|stmt| stmt_mentions_ident(stmt, name)),
        syn::Expr::If(if_expr) => {
            expr_mentions_ident(&if_expr.cond, name)
                || if_expr
                    .then_branch
                    .stmts
                    .iter()
                    .any(|stmt| stmt_mentions_ident(stmt, name))
                || if_expr
                    .else_branch
                    .as_ref()
                    .is_some_and(|(_, else_expr)| expr_mentions_ident(else_expr, name))
        }
        _ => false,
    }
}

fn stmt_mentions_ident(stmt: &syn::Stmt, name: &str) -> bool {
    match stmt {
        syn::Stmt::Expr(expr, _) => expr_mentions_ident(expr, name),
        syn::Stmt::Local(local) => local
            .init
            .as_ref()
            .is_some_and(|init| expr_mentions_ident(&init.expr, name)),
        _ => false,
    }
}

/// Conjunction of assumption expressions as one IR `post:` predicate.
pub(super) fn lower_assumes(assumes: &[String], names: &[&str]) -> Option<String> {
    let mut preds = Vec::new();
    for assume in assumes {
        let expr: syn::Expr = syn::parse_str(assume).ok()?;
        preds.push(expr_to_ir_pred(&expr, names)?);
    }
    let mut preds = preds.into_iter();
    let mut combined = preds.next()?;
    for pred in preds {
        combined = format!("and ({combined}) ({pred})");
    }
    Some(combined)
}

fn expr_to_ir_pred(expr: &syn::Expr, names: &[&str]) -> Option<String> {
    match peel_expr(expr) {
        syn::Expr::Binary(binary) => {
            if let Some(op) = cmp_op_name(&binary.op) {
                let lhs = expr_to_pred_arg(&binary.left, names)?;
                let rhs = expr_to_pred_arg(&binary.right, names)?;
                return Some(format!("cmp {op} {lhs} {rhs}"));
            }
            if matches!(binary.op, syn::BinOp::And(_)) {
                let lhs = expr_to_ir_pred(&binary.left, names)?;
                let rhs = expr_to_ir_pred(&binary.right, names)?;
                return Some(format!("and ({lhs}) ({rhs})"));
            }
            if matches!(binary.op, syn::BinOp::Or(_)) {
                let lhs = expr_to_ir_pred(&binary.left, names)?;
                let rhs = expr_to_ir_pred(&binary.right, names)?;
                return Some(format!("or ({lhs}) ({rhs})"));
            }
            None
        }
        syn::Expr::Unary(unary) if matches!(unary.op, syn::UnOp::Not(_)) => {
            let inner = expr_to_ir_pred(&unary.expr, names)?;
            Some(format!("not {inner}"))
        }
        syn::Expr::Lit(syn::ExprLit {
            lit: syn::Lit::Bool(value),
            ..
        }) => Some(if value.value { "true" } else { "false" }.to_string()),
        _ => None,
    }
}

fn expr_to_pred_arg(expr: &syn::Expr, names: &[&str]) -> Option<String> {
    match peel_expr(expr) {
        syn::Expr::Path(path) if path.path.segments.len() == 1 => {
            let name = path.path.segments[0].ident.to_string();
            let index = names.iter().position(|candidate| *candidate == name)?;
            Some(format!("${index}"))
        }
        syn::Expr::Lit(syn::ExprLit {
            lit: syn::Lit::Int(number),
            ..
        }) => {
            let text = number.base10_digits();
            text.parse::<i64>().ok()?;
            Some(format!("(const {text})"))
        }
        syn::Expr::Unary(unary) if matches!(unary.op, syn::UnOp::Neg(_)) => {
            if let syn::Expr::Lit(syn::ExprLit {
                lit: syn::Lit::Int(number),
                ..
            }) = peel_expr(&unary.expr)
            {
                let value = number.base10_parse::<i64>().ok()?.checked_neg()?;
                return Some(format!("(const {value})"));
            }
            let inner = expr_to_pred_arg(&unary.expr, names)?;
            Some(format!("(arith sub (const 0) {inner})"))
        }
        syn::Expr::Binary(binary) => {
            let op = arith_op_name(&binary.op)?;
            let lhs = expr_to_pred_arg(&binary.left, names)?;
            let rhs = expr_to_pred_arg(&binary.right, names)?;
            Some(format!("(arith {op} {lhs} {rhs})"))
        }
        _ => None,
    }
}

fn peel_expr(expr: &syn::Expr) -> &syn::Expr {
    match expr {
        syn::Expr::Paren(inner) => peel_expr(&inner.expr),
        syn::Expr::Group(inner) => peel_expr(&inner.expr),
        syn::Expr::Reference(inner) => peel_expr(&inner.expr),
        other => other,
    }
}

fn cmp_op_name(op: &syn::BinOp) -> Option<&'static str> {
    Some(match op {
        syn::BinOp::Eq(_) => "eq",
        syn::BinOp::Ne(_) => "ne",
        syn::BinOp::Lt(_) => "lt",
        syn::BinOp::Le(_) => "le",
        syn::BinOp::Gt(_) => "gt",
        syn::BinOp::Ge(_) => "ge",
        _ => return None,
    })
}

fn arith_op_name(op: &syn::BinOp) -> Option<&'static str> {
    Some(match op {
        syn::BinOp::Add(_) => "add",
        syn::BinOp::Sub(_) => "sub",
        syn::BinOp::Mul(_) => "mul",
        syn::BinOp::Div(_) => "div",
        syn::BinOp::Rem(_) => "mod",
        _ => return None,
    })
}
