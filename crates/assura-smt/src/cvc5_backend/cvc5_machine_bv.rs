//! Bitvector scripts for machine-width division.
//!
//! The integer encoding of Rust `/` is a truncating `ite` around a wrapped
//! product. CVC5's nonlinear integer solver does not finish that query inside
//! the clause budget. Declaring the same values as `(_ BitVec N)` and using
//! `bvsdiv` / `bvudiv` decides it. `int2bv` on a variable, and `bv2nat`, do
//! not: even a short mixed query times out. Eager bitblasting requires
//! `QF_BV`. `(set-logic ALL)` rejects that option.

use std::collections::{BTreeSet, HashMap, HashSet};

use assura_ast::{BinOp, ClauseKind, Expr, Literal, Param, SpExpr, UnaryOp};

use crate::clause_policy::{ClauseCheckPolarity, clause_check_polarity};
use crate::encode_atom_policy::{encode_ident_name, old_ident_name, old_snapshot_name};
use crate::prelude_policy::{collect_wrap_var_names, contract_machine_wrap};

/// One verifiable clause to check against the contract requires.
pub(crate) struct MachineDivClause<'a> {
    pub body: &'a SpExpr,
    pub kind: &'a ClauseKind,
    pub frame_vars: &'a [String],
}

/// Build a `QF_BV` script when every clause is machine arithmetic and at
/// least one uses `/` or `%`. `None` keeps the integer shell script.
pub(crate) fn machine_division_script(
    params: &[Param],
    return_ty: &[String],
    requires: &[&SpExpr],
    clauses: &[MachineDivClause<'_>],
    _has_ir: bool,
) -> Option<String> {
    // Synthesized IR constrains `result`. These division clauses talk about
    // inputs. A clause that mentions `result` fails `encode_bool` and stays
    // on the integer script, which still applies the IR.
    if clauses.is_empty() || crate::havoc_assume::is_collection_return(return_ty) {
        return None;
    }
    let (width, signed) = contract_machine_wrap(params, return_ty)?;
    if width == 0 || width > 64 {
        return None;
    }
    let wrap_vars = collect_wrap_var_names(params, return_ty);
    let mentions_div = requires.iter().any(|req| contains_div(req))
        || clauses.iter().any(|clause| contains_div(clause.body));
    if !mentions_div {
        return None;
    }

    let mut decls = BTreeSet::new();
    let mut base_asserts = Vec::new();
    for req in requires {
        let term = encode_bool(req, width, signed, &wrap_vars, &mut decls)?;
        if term != "true" {
            base_asserts.push(term);
        }
    }

    let mut clause_checks = Vec::new();
    for clause in clauses {
        let polarity = clause_check_polarity(clause.kind)?;
        if matches!(polarity, ClauseCheckPolarity::DecreasesNonNeg) {
            return None;
        }
        let mut frames = Vec::new();
        for name in clause.frame_vars {
            if !wrap_vars.contains(name) {
                return None;
            }
            let current = encode_ident_name(name);
            let old = old_snapshot_name(name);
            decls.insert(current.clone());
            decls.insert(old.clone());
            frames.push(format!("(= {current} {old})"));
        }
        let body = encode_bool(clause.body, width, signed, &wrap_vars, &mut decls)?;
        clause_checks.push((polarity, frames, body));
    }

    let mut script = String::from("(set-logic QF_BV)\n");
    for name in &decls {
        script.push_str(&format!("(declare-const {name} (_ BitVec {width}))\n"));
    }
    for assertion in &base_asserts {
        script.push_str(&format!("(assert {assertion})\n"));
    }
    let multi = clause_checks.len() > 1;
    for (polarity, frames, body) in &clause_checks {
        if multi {
            script.push_str("(push 1)\n");
        }
        for frame in frames {
            script.push_str(&format!("(assert {frame})\n"));
        }
        let asserted = match polarity {
            ClauseCheckPolarity::ValidityNegateBody => format!("(not {body})"),
            ClauseCheckPolarity::SatisfiabilityAssertBody
            | ClauseCheckPolarity::ValidityAssertBody => body.clone(),
            ClauseCheckPolarity::DecreasesNonNeg => return None,
        };
        script.push_str(&format!("(assert {asserted})\n"));
        script.push_str("(check-sat)\n");
        script.push_str("(get-model)\n");
        if multi {
            script.push_str("(pop 1)\n");
        }
    }
    Some(script)
}

fn contains_div(expr: &SpExpr) -> bool {
    match &expr.node {
        Expr::BinOp { op, lhs, rhs } => {
            matches!(op, BinOp::Div | BinOp::Mod) || contains_div(lhs) || contains_div(rhs)
        }
        Expr::UnaryOp { expr: inner, .. } | Expr::Old(inner) => contains_div(inner),
        Expr::If {
            cond,
            then_branch,
            else_branch,
        } => {
            contains_div(cond)
                || contains_div(then_branch)
                || else_branch
                    .as_ref()
                    .is_some_and(|branch| contains_div(branch))
        }
        _ => false,
    }
}

enum IntNode {
    /// Bitvector term already in range.
    Term(String),
    /// Mathematical integer. May sit outside the machine range.
    Num(i128),
}

fn encode_bool(
    expr: &SpExpr,
    width: u32,
    signed: bool,
    wrap_vars: &HashSet<String>,
    decls: &mut BTreeSet<String>,
) -> Option<String> {
    match &expr.node {
        Expr::Literal(Literal::Bool(value)) => Some(value.to_string()),
        Expr::UnaryOp {
            op: UnaryOp::Not,
            expr: inner,
        } => {
            let inner = encode_bool(inner, width, signed, wrap_vars, decls)?;
            Some(format!("(not {inner})"))
        }
        Expr::BinOp { op, lhs, rhs } if matches!(op, BinOp::And | BinOp::Or | BinOp::Implies) => {
            let left = encode_bool(lhs, width, signed, wrap_vars, decls)?;
            let right = encode_bool(rhs, width, signed, wrap_vars, decls)?;
            let name = match op {
                BinOp::And => "and",
                BinOp::Or => "or",
                BinOp::Implies => "=>",
                _ => return None,
            };
            Some(format!("({name} {left} {right})"))
        }
        Expr::BinOp { op, lhs, rhs } if is_cmp(op) => {
            let left = encode_int(lhs, width, signed, wrap_vars, decls)?;
            let right = encode_int(rhs, width, signed, wrap_vars, decls)?;
            encode_cmp(op, left, right, width, signed)
        }
        _ => None,
    }
}

fn encode_int(
    expr: &SpExpr,
    width: u32,
    signed: bool,
    wrap_vars: &HashSet<String>,
    decls: &mut BTreeSet<String>,
) -> Option<IntNode> {
    match &expr.node {
        Expr::Literal(Literal::Int(text)) => Some(IntNode::Num(text.parse().ok()?)),
        Expr::Ident(name) if wrap_vars.contains(name) => {
            let smt = encode_ident_name(name);
            decls.insert(smt.clone());
            Some(IntNode::Term(smt))
        }
        Expr::Old(inner) => match &inner.node {
            Expr::Ident(name) if wrap_vars.contains(name) => {
                let smt = old_ident_name(name);
                decls.insert(smt.clone());
                Some(IntNode::Term(smt))
            }
            _ => None,
        },
        Expr::UnaryOp {
            op: UnaryOp::Neg,
            expr: inner,
        } => match encode_int(inner, width, signed, wrap_vars, decls)? {
            IntNode::Num(n) => Some(IntNode::Num(n.checked_neg()?)),
            IntNode::Term(term) => Some(IntNode::Term(format!("(bvneg {term})"))),
        },
        Expr::BinOp { op, lhs, rhs }
            if matches!(
                op,
                BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Mod
            ) =>
        {
            let left = encode_int(lhs, width, signed, wrap_vars, decls)?;
            let right = encode_int(rhs, width, signed, wrap_vars, decls)?;
            match (left, right) {
                (IntNode::Num(a), IntNode::Num(b)) => Some(IntNode::Num(fold_int(op, a, b)?)),
                (left, right) => {
                    let left = as_term(left, width, signed)?;
                    let right = as_term(right, width, signed)?;
                    let name = bv_arith(op, signed)?;
                    Some(IntNode::Term(format!("({name} {left} {right})")))
                }
            }
        }
        _ => None,
    }
}

fn as_term(node: IntNode, width: u32, signed: bool) -> Option<String> {
    match node {
        IntNode::Term(term) => Some(term),
        IntNode::Num(n) if fits(n, width, signed) => Some(bv_lit(n, width)),
        IntNode::Num(_) => None,
    }
}

fn encode_cmp(
    op: &BinOp,
    left: IntNode,
    right: IntNode,
    width: u32,
    signed: bool,
) -> Option<String> {
    match (left, right) {
        (IntNode::Num(a), IntNode::Num(b)) => Some(fold_cmp(op, a, b).to_string()),
        (IntNode::Term(term), IntNode::Num(n)) => cmp_term_num(op, &term, n, width, signed, true),
        (IntNode::Num(n), IntNode::Term(term)) => cmp_term_num(op, &term, n, width, signed, false),
        (IntNode::Term(left), IntNode::Term(right)) => Some(bv_cmp(op, &left, &right, signed)),
    }
}

/// `num_on_right` means the term is the left operand.
fn cmp_term_num(
    op: &BinOp,
    term: &str,
    n: i128,
    width: u32,
    signed: bool,
    num_on_right: bool,
) -> Option<String> {
    if fits(n, width, signed) {
        let lit = bv_lit(n, width);
        return Some(if num_on_right {
            bv_cmp(op, term, &lit, signed)
        } else {
            bv_cmp(op, &lit, term, signed)
        });
    }
    // `n` is outside every machine value. `x <= BIG` is true. `x >= BIG`
    // is false. Equality with that constant is false.
    let (lo, hi) = machine_bounds(width, signed);
    let above = n > hi;
    debug_assert!(above || n < lo);
    let hold = match (op, num_on_right, above) {
        (BinOp::Eq, _, _) => false,
        (BinOp::Neq, _, _) => true,
        (BinOp::Lt | BinOp::Lte, true, true) | (BinOp::Lt | BinOp::Lte, false, false) => true,
        (BinOp::Lt | BinOp::Lte, _, _) => false,
        (BinOp::Gt | BinOp::Gte, true, false) | (BinOp::Gt | BinOp::Gte, false, true) => true,
        (BinOp::Gt | BinOp::Gte, _, _) => false,
        _ => return None,
    };
    Some(if hold { "true" } else { "false" }.to_string())
}

fn machine_bounds(width: u32, signed: bool) -> (i128, i128) {
    if signed {
        (-1i128 << (width - 1), (1i128 << (width - 1)) - 1)
    } else {
        (0, (1i128 << width) - 1)
    }
}

fn fits(n: i128, width: u32, signed: bool) -> bool {
    let (lo, hi) = machine_bounds(width, signed);
    n >= lo && n <= hi
}

fn bv_lit(n: i128, width: u32) -> String {
    let mask = (1u128 << width) - 1;
    let bits = (n as u128) & mask;
    format!("(_ bv{bits} {width})")
}

fn fold_int(op: &BinOp, left: i128, right: i128) -> Option<i128> {
    match op {
        BinOp::Add => left.checked_add(right),
        BinOp::Sub => left.checked_sub(right),
        BinOp::Mul => left.checked_mul(right),
        BinOp::Div => {
            if right == 0 {
                None
            } else {
                left.checked_div(right)
            }
        }
        BinOp::Mod => {
            if right == 0 {
                None
            } else {
                left.checked_rem(right)
            }
        }
        _ => None,
    }
}

fn fold_cmp(op: &BinOp, left: i128, right: i128) -> &'static str {
    let hold = match op {
        BinOp::Lt => left < right,
        BinOp::Lte => left <= right,
        BinOp::Gt => left > right,
        BinOp::Gte => left >= right,
        BinOp::Eq => left == right,
        BinOp::Neq => left != right,
        _ => false,
    };
    if hold { "true" } else { "false" }
}

fn is_cmp(op: &BinOp) -> bool {
    matches!(
        op,
        BinOp::Lt | BinOp::Lte | BinOp::Gt | BinOp::Gte | BinOp::Eq | BinOp::Neq
    )
}

fn bv_arith(op: &BinOp, signed: bool) -> Option<&'static str> {
    Some(match (op, signed) {
        (BinOp::Add, _) => "bvadd",
        (BinOp::Sub, _) => "bvsub",
        (BinOp::Mul, _) => "bvmul",
        (BinOp::Div, true) => "bvsdiv",
        (BinOp::Div, false) => "bvudiv",
        (BinOp::Mod, true) => "bvsrem",
        (BinOp::Mod, false) => "bvurem",
        _ => return None,
    })
}

fn bv_cmp(op: &BinOp, left: &str, right: &str, signed: bool) -> String {
    match op {
        BinOp::Eq => format!("(= {left} {right})"),
        BinOp::Neq => format!("(not (= {left} {right}))"),
        BinOp::Lt => rel(if signed { "bvslt" } else { "bvult" }, left, right),
        BinOp::Lte => rel(if signed { "bvsle" } else { "bvule" }, left, right),
        BinOp::Gt => rel(if signed { "bvsgt" } else { "bvugt" }, left, right),
        BinOp::Gte => rel(if signed { "bvsge" } else { "bvuge" }, left, right),
        _ => "false".to_string(),
    }
}

fn rel(op: &str, left: &str, right: &str) -> String {
    format!("({op} {left} {right})")
}

/// True when every `+`/`-`/`*` sits on machine atoms, so 64-bit wrap matches
/// the integer shell encoding. A multiply of a quotient does not.
pub(crate) fn integer_wrap_matches_bitvector(
    params: &[Param],
    return_ty: &[String],
    requires: &[&SpExpr],
    body: &SpExpr,
) -> bool {
    let Some(_) = contract_machine_wrap(params, return_ty) else {
        return false;
    };
    let vars = collect_wrap_var_names(params, return_ty);
    requires
        .iter()
        .all(|req| tree_matches_machine_wrap(req, &vars))
        && tree_matches_machine_wrap(body, &vars)
}

fn tree_matches_machine_wrap(expr: &SpExpr, vars: &HashSet<String>) -> bool {
    match &expr.node {
        Expr::Literal(_) | Expr::Ident(_) => true,
        Expr::Old(inner) | Expr::UnaryOp { expr: inner, .. } => {
            tree_matches_machine_wrap(inner, vars)
        }
        Expr::BinOp { op, lhs, rhs } => {
            if matches!(op, BinOp::Add | BinOp::Sub | BinOp::Mul)
                && (!crate::prelude_policy::is_machine_arith_atom(lhs, vars)
                    || !crate::prelude_policy::is_machine_arith_atom(rhs, vars))
            {
                return false;
            }
            tree_matches_machine_wrap(lhs, vars) && tree_matches_machine_wrap(rhs, vars)
        }
        _ => false,
    }
}

/// The bitvector model breaks the clause under integer wrap (machine `+`/`-`/`*`
/// only), not merely under wrapping every multiply.
pub(crate) fn model_refutes_negated_clause(
    params: &[Param],
    return_ty: &[String],
    requires: &[&SpExpr],
    body: &SpExpr,
    kind: &ClauseKind,
    model_text: &str,
) -> bool {
    if clause_check_polarity(kind) != Some(ClauseCheckPolarity::ValidityNegateBody) {
        return false;
    }
    let Some((width, signed)) = contract_machine_wrap(params, return_ty) else {
        return false;
    };
    let vars = collect_wrap_var_names(params, return_ty);
    let model = parse_bv_model(model_text, width, signed);
    let ctx = EvalCtx {
        width,
        signed,
        vars: &vars,
        model: &model,
    };
    requires
        .iter()
        .all(|req| eval_bool(req, &ctx) == Some(true))
        && eval_bool(body, &ctx) == Some(false)
}

struct EvalCtx<'a> {
    width: u32,
    signed: bool,
    vars: &'a HashSet<String>,
    model: &'a HashMap<String, i128>,
}

fn parse_bv_model(model_text: &str, width: u32, signed: bool) -> HashMap<String, i128> {
    let mut model = HashMap::new();
    for line in model_text.lines() {
        let line = line.trim();
        let Some(rest) = line.strip_prefix("(define-fun ") else {
            continue;
        };
        let Some((name, after)) = rest.split_once(' ') else {
            continue;
        };
        let Some(bits) = bv_bits(after) else {
            continue;
        };
        model.insert(name.to_string(), bits_to_int(bits, width, signed));
    }
    model
}

fn bv_bits(after_name: &str) -> Option<u128> {
    if let Some(idx) = after_name.find("#b") {
        let digits: String = after_name[idx + 2..]
            .chars()
            .take_while(|ch| *ch == '0' || *ch == '1')
            .collect();
        return u128::from_str_radix(&digits, 2).ok();
    }
    if let Some(idx) = after_name.find("#x") {
        let digits: String = after_name[idx + 2..]
            .chars()
            .take_while(|ch| ch.is_ascii_hexdigit())
            .collect();
        return u128::from_str_radix(&digits, 16).ok();
    }
    let marker = "(_ bv";
    let idx = after_name.find(marker)?;
    let number: String = after_name[idx + marker.len()..]
        .chars()
        .skip_while(|ch| ch.is_whitespace())
        .take_while(|ch| ch.is_ascii_digit())
        .collect();
    number.parse().ok()
}

fn bits_to_int(bits: u128, width: u32, signed: bool) -> i128 {
    let mask = (1u128 << width) - 1;
    let bits = bits & mask;
    if signed && bits & (1u128 << (width - 1)) != 0 {
        (bits as i128) - (1i128 << width)
    } else {
        bits as i128
    }
}

fn eval_bool(expr: &SpExpr, ctx: &EvalCtx<'_>) -> Option<bool> {
    match &expr.node {
        Expr::Literal(Literal::Bool(value)) => Some(*value),
        Expr::UnaryOp {
            op: UnaryOp::Not,
            expr: inner,
        } => Some(!eval_bool(inner, ctx)?),
        Expr::BinOp { op, lhs, rhs } if matches!(op, BinOp::And | BinOp::Or | BinOp::Implies) => {
            let left = eval_bool(lhs, ctx)?;
            let right = eval_bool(rhs, ctx)?;
            Some(match op {
                BinOp::And => left && right,
                BinOp::Or => left || right,
                BinOp::Implies => !left || right,
                _ => return None,
            })
        }
        Expr::BinOp { op, lhs, rhs } if is_cmp(op) => {
            let left = eval_int(lhs, ctx)?;
            let right = eval_int(rhs, ctx)?;
            Some(match op {
                BinOp::Lt => left < right,
                BinOp::Lte => left <= right,
                BinOp::Gt => left > right,
                BinOp::Gte => left >= right,
                BinOp::Eq => left == right,
                BinOp::Neq => left != right,
                _ => return None,
            })
        }
        _ => None,
    }
}

fn eval_int(expr: &SpExpr, ctx: &EvalCtx<'_>) -> Option<i128> {
    match &expr.node {
        Expr::Literal(Literal::Int(text)) => text.parse().ok(),
        Expr::Ident(name) if ctx.vars.contains(name) => {
            ctx.model.get(&encode_ident_name(name)).copied()
        }
        Expr::Old(inner) => match &inner.node {
            Expr::Ident(name) if ctx.vars.contains(name) => {
                ctx.model.get(&old_ident_name(name)).copied()
            }
            _ => None,
        },
        Expr::UnaryOp {
            op: UnaryOp::Neg,
            expr: inner,
        } => {
            let value = eval_int(inner, ctx)?.checked_neg()?;
            if machine_value(inner, ctx) {
                Some(wrap_machine(value, ctx)?)
            } else {
                Some(value)
            }
        }
        Expr::BinOp { op, lhs, rhs }
            if matches!(
                op,
                BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Mod
            ) =>
        {
            let left = eval_int(lhs, ctx)?;
            let right = eval_int(rhs, ctx)?;
            let value = match op {
                BinOp::Add => left.checked_add(right)?,
                BinOp::Sub => left.checked_sub(right)?,
                BinOp::Mul => left.checked_mul(right)?,
                BinOp::Div => left.checked_div(right)?,
                BinOp::Mod => left.checked_rem(right)?,
                _ => return None,
            };
            if matches!(op, BinOp::Add | BinOp::Sub | BinOp::Mul)
                && machine_value(lhs, ctx)
                && machine_value(rhs, ctx)
            {
                Some(wrap_machine(value, ctx)?)
            } else {
                Some(value)
            }
        }
        _ => None,
    }
}

fn machine_value(expr: &SpExpr, ctx: &EvalCtx<'_>) -> bool {
    crate::prelude_policy::is_machine_arith_atom(expr, ctx.vars)
}

fn wrap_machine(value: i128, ctx: &EvalCtx<'_>) -> Option<i128> {
    let modulus = 1i128 << ctx.width;
    let wrapped = value.rem_euclid(modulus);
    if ctx.signed {
        let half = 1i128 << (ctx.width - 1);
        if wrapped >= half {
            Some(wrapped - modulus)
        } else {
            Some(wrapped)
        }
    } else {
        Some(wrapped)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use assura_ast::{BinOp, Spanned, TypeExpr};

    fn int_param(name: &str) -> Param {
        Param {
            name: name.into(),
            ty: Some(TypeExpr::named("Int")),
        }
    }

    fn ident(name: &str) -> SpExpr {
        Spanned::no_span(Expr::Ident(name.into()))
    }

    fn lit(n: &str) -> SpExpr {
        Spanned::no_span(Expr::Literal(Literal::Int(n.into())))
    }

    fn bin(op: BinOp, lhs: SpExpr, rhs: SpExpr) -> SpExpr {
        Spanned::no_span(Expr::BinOp {
            op,
            lhs: Box::new(lhs),
            rhs: Box::new(rhs),
        })
    }

    fn script_for(requires: &[&SpExpr], body: &SpExpr) -> Option<String> {
        let params = vec![int_param("a"), int_param("b")];
        machine_division_script(
            &params,
            &[],
            requires,
            &[MachineDivClause {
                body,
                kind: &ClauseKind::Ensures,
                frame_vars: &[],
            }],
            false,
        )
    }

    #[test]
    fn division_comparison_is_bitvector_without_int_bridge() {
        let req = bin(BinOp::Gte, ident("b"), lit("1"));
        let body = bin(
            BinOp::Lte,
            bin(BinOp::Div, ident("a"), ident("b")),
            ident("a"),
        );
        let script = script_for(&[&req], &body).expect("div clause");
        assert!(script.starts_with("(set-logic QF_BV)\n"), "{script}");
        assert!(script.contains("bvsdiv"), "{script}");
        assert!(script.contains("bvsle"), "{script}");
        assert!(!script.contains("int2bv"), "{script}");
        assert!(!script.contains("bv2nat"), "{script}");
        assert!(!script.contains("(div "), "{script}");
        assert!(!script.contains("ite"), "{script}");
    }

    #[test]
    fn bound_above_the_machine_range_is_not_truncated() {
        let req = bin(BinOp::Lte, ident("a"), lit("79228162514264337593543950335"));
        let body = bin(BinOp::Div, ident("a"), ident("b"));
        // Body must be Bool. Compare the quotient.
        let body = bin(BinOp::Gte, body, lit("0"));
        let params = vec![int_param("a"), int_param("b")];
        let script = machine_division_script(
            &params,
            &[],
            &[&req],
            &[MachineDivClause {
                body: &body,
                kind: &ClauseKind::Ensures,
                frame_vars: &[],
            }],
            false,
        )
        .expect("script");
        assert!(
            !script.contains("79228162514264337593543950335"),
            "out-of-range bound must not be emitted as a truncated literal: {script}"
        );
        assert!(
            !script.contains("(assert true)"),
            "tautological bound is omitted: {script}"
        );
    }

    #[test]
    fn addition_without_division_stays_on_the_integer_path() {
        let body = bin(BinOp::Gt, bin(BinOp::Add, ident("a"), lit("1")), ident("a"));
        assert!(script_for(&[], &body).is_none());
    }

    #[test]
    fn method_call_is_not_rewritten_as_bitvector() {
        let body = Spanned::no_span(Expr::MethodCall {
            receiver: Box::new(ident("a")),
            method: "length".into(),
            args: vec![],
        });
        let body = bin(BinOp::Gte, body, lit("0"));
        let div_req = bin(BinOp::Gt, bin(BinOp::Div, ident("a"), ident("b")), lit("0"));
        assert!(script_for(&[&div_req], &body).is_none());
    }

    #[test]
    fn quotient_multiply_does_not_match_full_bitvector_wrap() {
        let params = vec![int_param("a"), int_param("b"), int_param("c")];
        let body = bin(
            BinOp::Lte,
            bin(
                BinOp::Mul,
                bin(BinOp::Div, ident("a"), ident("b")),
                ident("c"),
            ),
            ident("a"),
        );
        assert!(!integer_wrap_matches_bitvector(&params, &[], &[], &body));
        let wrapped_only = bin(
            BinOp::Lte,
            bin(
                BinOp::Div,
                bin(BinOp::Mul, ident("a"), ident("b")),
                ident("c"),
            ),
            ident("a"),
        );
        assert!(integer_wrap_matches_bitvector(
            &params,
            &[],
            &[],
            &wrapped_only
        ));
    }

    #[test]
    fn real_rounding_model_is_kept_and_wrapped_fee_model_is_not() {
        let params = vec![
            int_param("rounds"),
            int_param("deposit_amount"),
            int_param("total_shares"),
            int_param("total_assets"),
        ];
        let minted = bin(
            BinOp::Div,
            bin(
                BinOp::Mul,
                bin(BinOp::Mul, ident("rounds"), ident("deposit_amount")),
                ident("total_shares"),
            ),
            ident("total_assets"),
        );
        let per = bin(
            BinOp::Div,
            bin(BinOp::Mul, ident("deposit_amount"), ident("total_shares")),
            ident("total_assets"),
        );
        let body = bin(
            BinOp::Lte,
            bin(
                BinOp::Div,
                bin(
                    BinOp::Mul,
                    minted,
                    bin(BinOp::Add, ident("total_assets"), ident("deposit_amount")),
                ),
                bin(BinOp::Add, ident("total_shares"), per),
            ),
            bin(BinOp::Mul, ident("rounds"), ident("deposit_amount")),
        );
        let reqs = [
            bin(BinOp::Gte, ident("rounds"), lit("1")),
            bin(BinOp::Lte, ident("rounds"), lit("1000")),
            bin(BinOp::Gte, ident("deposit_amount"), lit("1")),
            bin(BinOp::Lte, ident("deposit_amount"), lit("1000000000")),
            bin(BinOp::Gte, ident("total_shares"), lit("1")),
            bin(BinOp::Lte, ident("total_shares"), lit("1000000000")),
            bin(BinOp::Gte, ident("total_assets"), lit("1")),
            bin(BinOp::Lte, ident("total_assets"), lit("1000000000")),
        ];
        let req_refs: Vec<&SpExpr> = reqs.iter().collect();
        let model = "\
(define-fun deposit_amount () (_ BitVec 64) (_ bv5592149 64))
(define-fun rounds () (_ BitVec 64) (_ bv128 64))
(define-fun total_assets () (_ BitVec 64) (_ bv543286791 64))
(define-fun total_shares () (_ BitVec 64) (_ bv536944338 64))
";
        assert!(model_refutes_negated_clause(
            &params,
            &[],
            &req_refs,
            &body,
            &ClauseKind::Ensures,
            model,
        ));

        let fee_params = vec![
            int_param("minimum_assets"),
            int_param("platform_fee"),
            int_param("time_split"),
            int_param("time_total"),
        ];
        let fee = |t: SpExpr| {
            bin(
                BinOp::Div,
                bin(
                    BinOp::Mul,
                    bin(
                        BinOp::Div,
                        bin(BinOp::Mul, ident("minimum_assets"), ident("platform_fee")),
                        lit("10000"),
                    ),
                    t,
                ),
                lit("31536000"),
            )
        };
        let fee_body = bin(
            BinOp::Lte,
            bin(
                BinOp::Add,
                fee(ident("time_split")),
                fee(bin(BinOp::Sub, ident("time_total"), ident("time_split"))),
            ),
            bin(BinOp::Add, fee(ident("time_total")), lit("1")),
        );
        let fee_model = "\
(define-fun minimum_assets () (_ BitVec 64) (_ bv9209972996557815808 64))
(define-fun platform_fee () (_ BitVec 64) (_ bv512 64))
(define-fun time_split () (_ BitVec 64) (_ bv229376 64))
(define-fun time_total () (_ BitVec 64) (_ bv1188681 64))
";
        assert!(!model_refutes_negated_clause(
            &fee_params,
            &[],
            &[],
            &fee_body,
            &ClauseKind::Ensures,
            fee_model,
        ));
    }
}
