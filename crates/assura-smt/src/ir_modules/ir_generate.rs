//! Heuristic contract-to-Implementation-IR generation.
//!
//! Analyzes `ensures` clauses to produce IR bodies richer than identity stubs.
//! Falls back to `stub_ir_sidecar_text` when no pattern matches.

use assura_ast::{BinOp, Clause, ClauseKind, Expr, Literal, SpExpr};
use std::collections::HashMap;

use crate::ir_codegen::stub_ir_sidecar_text;

/// Shared context for IR body planners (contract params → slots).
#[derive(Debug, Clone)]
pub(crate) struct PlanCtx<'a> {
    pub name_to_slot: HashMap<&'a str, usize>,
    pub return_ty: &'a str,
    pub param_count: usize,
}

/// Shape of the primary `ensures` clause (drives template suggestion).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnsuresShape {
    Identity,
    Arithmetic,
    LengthCopy,
    BoundsCheck,
    FieldAccess,
    CallChain,
    IfBranch,
    MatchArm,
    Unknown,
}

/// A planned IR instruction sequence for the main `fn #0` body.
#[derive(Debug, Clone, PartialEq)]
struct IrGenBody {
    lines: Vec<String>,
}

/// Main `fn #0` body plus optional sibling `fn #N` blocks in one module.
#[derive(Debug, Clone, PartialEq)]
struct IrGenPlan {
    main: IrGenBody,
    siblings: Vec<(usize, IrGenBody)>,
}

type IrPlannerFn = fn(&SpExpr, &PlanCtx<'_>) -> Option<IrGenPlan>;

const ENSURES_PLANNERS: &[IrPlannerFn] = &[
    plan_if_branch_ensures,
    plan_match_arm_ensures,
    plan_multi_fn_call_chain,
    plan_identity_equality,
    plan_length_copy_ensures,
];

/// Classify the best-matching ensures shape for template selection.
pub fn classify_ensures_shape(clauses: &[Clause], param_names: &[String]) -> EnsuresShape {
    let name_to_slot: HashMap<&str, usize> = param_names
        .iter()
        .enumerate()
        .map(|(i, n)| (n.as_str(), i))
        .collect();

    let has_bounds_requires = clauses
        .iter()
        .any(|c| c.kind == ClauseKind::Requires && clause_mentions_index_bounds(&c.body));

    for clause in clauses.iter().filter(|c| c.kind == ClauseKind::Ensures) {
        if length_relation_ensures(&clause.body, &name_to_slot).is_some() {
            return if has_bounds_requires {
                EnsuresShape::BoundsCheck
            } else {
                EnsuresShape::LengthCopy
            };
        }
        if let Some((lhs, rhs)) = equality_operands(&clause.body) {
            let other = if is_result_ident(lhs) {
                rhs
            } else if is_result_ident(rhs) {
                lhs
            } else {
                continue;
            };
            if expr_suggests_call_chain(other) {
                return EnsuresShape::CallChain;
            }
            if matches!(&other.node, Expr::If { .. }) {
                return EnsuresShape::IfBranch;
            }
            if matches!(&other.node, Expr::Match { .. }) {
                return EnsuresShape::MatchArm;
            }
            if matches!(&other.node, Expr::Ident(_)) {
                return EnsuresShape::Identity;
            }
            if matches!(&other.node, Expr::BinOp { op, .. } if op.is_arithmetic()) {
                return EnsuresShape::Arithmetic;
            }
        }
        if clause_mentions_result_field(&clause.body) {
            return EnsuresShape::FieldAccess;
        }
    }

    EnsuresShape::Unknown
}

/// Generate `.ir` sidecar text from contract structure and ensures clauses.
pub fn generate_ir_sidecar_text(
    name: &str,
    params: &[(usize, String)],
    param_names: &[String],
    return_ty: &str,
    clauses: &[Clause],
) -> String {
    let requires_count = clauses
        .iter()
        .filter(|c| c.kind == ClauseKind::Requires)
        .count();
    let ensures_count = clauses
        .iter()
        .filter(|c| c.kind == ClauseKind::Ensures)
        .count();

    let ctx = PlanCtx {
        name_to_slot: param_names
            .iter()
            .enumerate()
            .map(|(i, n)| (n.as_str(), i))
            .collect(),
        return_ty,
        param_count: params.len(),
    };

    for clause in clauses.iter().filter(|c| c.kind == ClauseKind::Ensures) {
        if let Some(plan) = plan_from_ensures(&clause.body, &ctx) {
            return format_ir_module_plan(
                name,
                params,
                return_ty,
                requires_count,
                ensures_count,
                &plan,
            );
        }
    }

    stub_ir_sidecar_text(name, params, return_ty, requires_count, ensures_count)
}

fn plan_from_ensures(expr: &SpExpr, ctx: &PlanCtx<'_>) -> Option<IrGenPlan> {
    for planner in ENSURES_PLANNERS {
        if let Some(plan) = planner(expr, ctx) {
            return Some(plan);
        }
    }
    None
}

fn single_fn_plan(body: IrGenBody) -> IrGenPlan {
    IrGenPlan {
        main: body,
        siblings: Vec::new(),
    }
}

fn plan_identity_equality(expr: &SpExpr, ctx: &PlanCtx<'_>) -> Option<IrGenPlan> {
    let (lhs, rhs) = equality_operands(expr)?;
    if is_result_ident(lhs) {
        return plan_result_equals(rhs, ctx).map(single_fn_plan);
    }
    if is_result_ident(rhs) {
        return plan_result_equals(lhs, ctx).map(single_fn_plan);
    }
    None
}

fn plan_length_copy_ensures(expr: &SpExpr, ctx: &PlanCtx<'_>) -> Option<IrGenPlan> {
    let slot = length_relation_ensures(expr, &ctx.name_to_slot)?;
    Some(single_fn_plan(single_load(slot, ctx.return_ty)))
}

/// `ensures { result == if cond then a else b }` → branch blocks `#1` / `#2`.
fn plan_if_branch_ensures(expr: &SpExpr, ctx: &PlanCtx<'_>) -> Option<IrGenPlan> {
    let (lhs, rhs) = equality_operands(expr)?;
    let if_expr = if is_result_ident(lhs) {
        rhs
    } else if is_result_ident(rhs) {
        lhs
    } else {
        return None;
    };
    let Expr::If {
        cond,
        then_branch,
        else_branch,
    } = &if_expr.node
    else {
        return None;
    };
    let else_branch = else_branch.as_deref()?;
    let then_body = plan_branch_result(then_branch, ctx)?;
    let else_body = plan_branch_result(else_branch, ctx)?;
    let cond_slot = expr_to_param_slot(cond, &ctx.name_to_slot).unwrap_or(0);
    let out_slot = next_temp_slot(&[cond_slot]);
    Some(IrGenPlan {
        main: IrGenBody {
            lines: vec![
                format!(
                    "    ${out_slot} = if ${cond_slot} then #1 else #2 : {}",
                    ctx.return_ty
                ),
                format!("    $result = load ${out_slot} : {}", ctx.return_ty),
            ],
        },
        siblings: vec![(1, then_body), (2, else_body)],
    })
}

/// Plans IR for `ensures { result == match x { p1 => e1, p2 => e2 } }`.
///
/// Emits a real IR `match` instruction with arm patterns (int / bool / string /
/// wildcard) so discrimination is not a pure boolean `if $scrut` (#854 / #307).
fn plan_match_arm_ensures(expr: &SpExpr, ctx: &PlanCtx<'_>) -> Option<IrGenPlan> {
    use assura_ast::Pattern;

    let (lhs, rhs) = equality_operands(expr)?;
    let match_expr = if is_result_ident(lhs) {
        rhs
    } else if is_result_ident(rhs) {
        lhs
    } else {
        return None;
    };
    let Expr::Match { scrutinee, arms } = &match_expr.node else {
        return None;
    };
    if arms.len() < 2 {
        return None;
    }
    let scrut_slot = expr_to_param_slot(scrutinee, &ctx.name_to_slot)?;

    let mut arm_texts = Vec::with_capacity(arms.len());
    let mut siblings = Vec::with_capacity(arms.len());
    for (i, arm) in arms.iter().enumerate() {
        let block_id = i + 1;
        let pat_text = ir_match_pattern_text(&arm.pattern)?;
        let body = plan_branch_result(&arm.body, ctx)?;
        arm_texts.push(format!("{pat_text} => #{block_id}"));
        siblings.push((block_id, body));
    }

    // Ensure a wildcard arm exists so the IR match is total; if none, add `_ => #last`.
    let has_wildcard = arms.iter().any(|a| match &a.pattern {
        Pattern::Wildcard => true,
        Pattern::Ident(n) if n == "_" => true,
        _ => false,
    });
    if !has_wildcard {
        // Fall back: last arm already planned; no extra sibling (IR may still be partial).
        // Prefer explicit wildcard when the last arm is not one, by reusing last body.
        if let Some((_, last_body)) = siblings.last().cloned() {
            let wid = siblings.len() + 1;
            arm_texts.push(format!("_ => #{wid}"));
            siblings.push((wid, last_body));
        }
    }

    let arms_joined = arm_texts.join(", ");
    Some(IrGenPlan {
        main: IrGenBody {
            lines: vec![
                format!(
                    "    $1 = match ${scrut_slot} {{ {arms_joined} }} : {}",
                    ctx.return_ty
                ),
                format!("    $result = load $1 : {}", ctx.return_ty),
            ],
        },
        siblings,
    })
}

/// Format an AST pattern for IR `match $N { … }` arms.
fn ir_match_pattern_text(pat: &assura_ast::Pattern) -> Option<String> {
    use assura_ast::Pattern;
    match pat {
        Pattern::Wildcard => Some("_".into()),
        Pattern::Ident(n) if n == "_" => Some("_".into()),
        Pattern::Ident(n) => Some(format!("\"{n}\"")),
        Pattern::Constructor { name, .. } => Some(format!("\"{name}\"")),
        Pattern::Literal(Literal::Int(s)) => Some(s.clone()),
        Pattern::Literal(Literal::Bool(b)) => Some(if *b { "true".into() } else { "false".into() }),
        Pattern::Literal(Literal::Str(s)) => Some(format!("\"{s}\"")),
        Pattern::Literal(_) | Pattern::Tuple(_) => None,
    }
}

/// Plans IR for `ensures { result == f(x) }` call chains.
///
/// `ensures { result == helper(x) }` with callee body as sibling `fn #1`.
///
/// # Limitation (#306)
///
/// Sibling `fn #N` blocks are always generated as identity stubs
/// (`$result = load $0`), regardless of the callee's actual semantics.
/// For example, `ensures { result == double(x) }` produces `fn #1`
/// with `$result = load $0` instead of `$result = arith add $0 $0`.
/// The verifier inlines this stub, so verification only confirms that
/// the call plumbing works, not that the callee computes the right value.
fn plan_multi_fn_call_chain(expr: &SpExpr, ctx: &PlanCtx<'_>) -> Option<IrGenPlan> {
    let (lhs, rhs) = equality_operands(expr)?;
    let call = if is_result_ident(lhs) {
        rhs
    } else if is_result_ident(rhs) {
        lhs
    } else {
        return None;
    };
    let Expr::Call { func, args } = &call.node else {
        return None;
    };
    let Expr::Ident(helper) = &func.as_ref().node else {
        return None;
    };
    if is_builtin_call(helper) || args.len() != 1 {
        return None;
    }
    let arg_slot = expr_to_param_slot(&args[0], &ctx.name_to_slot)?;
    let temp = next_temp_slot(&[arg_slot]);
    Some(IrGenPlan {
        main: IrGenBody {
            lines: vec![
                format!(
                    "    ${temp} = call {helper} (${arg_slot}) : {}",
                    ctx.return_ty
                ),
                format!("    $result = load ${temp} : {}", ctx.return_ty),
            ],
        },
        siblings: vec![(1, single_load(0, ctx.return_ty))],
    })
}

fn plan_branch_result(expr: &SpExpr, ctx: &PlanCtx<'_>) -> Option<IrGenBody> {
    plan_result_equals(expr, ctx)
}

fn plan_result_equals(other: &SpExpr, ctx: &PlanCtx<'_>) -> Option<IrGenBody> {
    match &other.node {
        Expr::Ident(name) => {
            let slot = *ctx.name_to_slot.get(name.as_str())?;
            Some(single_load(slot, ctx.return_ty))
        }
        Expr::Literal(lit) => Some(single_const(&literal_to_ir_const(lit)?, ctx.return_ty)),
        Expr::BinOp { op, lhs, rhs } => {
            plan_result_arith(op.clone(), lhs.as_ref(), rhs.as_ref(), ctx)
        }
        _ => {
            if ctx.param_count == 1 {
                Some(single_load(0, ctx.return_ty))
            } else {
                None
            }
        }
    }
}

fn plan_result_arith(
    op: BinOp,
    lhs: &SpExpr,
    rhs: &SpExpr,
    ctx: &PlanCtx<'_>,
) -> Option<IrGenBody> {
    let ir_op = match op {
        _ if op.is_arithmetic() => op.as_ident(),
        _ => return None,
    };

    let lhs_slot = expr_to_param_slot(lhs, &ctx.name_to_slot)?;
    let rhs_slot = expr_to_param_slot(rhs, &ctx.name_to_slot)?;
    let temp_slot = next_temp_slot(&[lhs_slot, rhs_slot]);

    Some(IrGenBody {
        lines: vec![
            format!(
                "    ${temp_slot} = arith {ir_op} ${lhs_slot} ${rhs_slot} : {}",
                ctx.return_ty
            ),
            format!("    $result = load ${temp_slot} : {}", ctx.return_ty),
        ],
    })
}

fn length_relation_ensures(expr: &SpExpr, name_to_slot: &HashMap<&str, usize>) -> Option<usize> {
    match &expr.node {
        Expr::BinOp {
            op: BinOp::Lte | BinOp::Lt | BinOp::Eq,
            lhs,
            rhs,
        } => length_pair_to_param_slot(lhs.as_ref(), rhs.as_ref(), name_to_slot)
            .or_else(|| length_pair_to_param_slot(rhs.as_ref(), lhs.as_ref(), name_to_slot)),
        _ => None,
    }
}

fn length_pair_to_param_slot(
    result_side: &SpExpr,
    other_side: &SpExpr,
    name_to_slot: &HashMap<&str, usize>,
) -> Option<usize> {
    if !is_result_length_call(result_side) {
        return None;
    }
    match &other_side.node {
        Expr::MethodCall {
            receiver, method, ..
        } if method == "length" => {
            // Prefer structured match over expect-after-guard (Developer/MPI).
            match &receiver.as_ref().node {
                Expr::Ident(name) => name_to_slot.get(name.as_str()).copied(),
                _ => None,
            }
        }
        Expr::Ident(name) => name_to_slot.get(name.as_str()).copied(),
        _ => None,
    }
}

fn is_result_length_call(expr: &SpExpr) -> bool {
    matches!(
        &expr.node,
        Expr::MethodCall {
            receiver,
            method,
            ..
        } if method == "length"
            && matches!(&receiver.as_ref().node, Expr::Ident(name) if name == "result")
    )
}

/// Requires clauses that mention index/buffer access (not mere `length() > 0`).
fn clause_mentions_index_bounds(expr: &SpExpr) -> bool {
    match &expr.node {
        Expr::Index { .. } => true,
        Expr::BinOp { lhs, rhs, .. } => {
            clause_mentions_index_bounds(lhs) || clause_mentions_index_bounds(rhs)
        }
        Expr::Field(inner, field) => {
            clause_mentions_index_bounds(inner)
                || matches!(
                    field.as_str(),
                    "offset" | "index" | "start" | "end" | "capacity"
                )
        }
        Expr::Ident(name) => matches!(
            name.as_str(),
            "offset" | "index" | "start" | "end" | "capacity" | "buf_size"
        ),
        Expr::MethodCall { receiver, .. } => clause_mentions_index_bounds(receiver),
        Expr::UnaryOp { expr: inner, .. } | Expr::Old(inner) => clause_mentions_index_bounds(inner),
        _ => false,
    }
}

fn clause_mentions_result_field(expr: &SpExpr) -> bool {
    match &expr.node {
        Expr::Field(receiver, _) => {
            matches!(&receiver.as_ref().node, Expr::Ident(name) if name == "result")
        }
        Expr::BinOp { lhs, rhs, .. } => {
            clause_mentions_result_field(lhs) || clause_mentions_result_field(rhs)
        }
        _ => false,
    }
}

fn expr_to_param_slot(expr: &SpExpr, name_to_slot: &HashMap<&str, usize>) -> Option<usize> {
    match &expr.node {
        Expr::Ident(name) => name_to_slot.get(name.as_str()).copied(),
        Expr::Literal(_) => None,
        _ => None,
    }
}

fn next_temp_slot(used: &[usize]) -> usize {
    used.iter().max().copied().unwrap_or(0) + 1
}

fn single_load(slot: usize, return_ty: &str) -> IrGenBody {
    IrGenBody {
        lines: vec![format!("    $result = load ${slot} : {return_ty}")],
    }
}

fn single_const(value: &str, return_ty: &str) -> IrGenBody {
    IrGenBody {
        lines: vec![format!("    $result = const {value} : {return_ty}")],
    }
}

fn literal_to_ir_const(lit: &Literal) -> Option<String> {
    match lit {
        Literal::Int(s) => Some(s.clone()),
        Literal::Float(s) => Some(s.clone()),
        Literal::Bool(b) => Some(if *b { "1".into() } else { "0".into() }),
        Literal::Str(_) => None,
    }
}

fn equality_operands(expr: &SpExpr) -> Option<(&SpExpr, &SpExpr)> {
    match &expr.node {
        Expr::BinOp {
            op: BinOp::Eq,
            lhs,
            rhs,
        } => Some((lhs.as_ref(), rhs.as_ref())),
        _ => None,
    }
}

fn is_result_ident(expr: &SpExpr) -> bool {
    matches!(&expr.node, Expr::Ident(name) if name == "result")
}

/// `ensures { result == helper(x) }` — delegate via a cross-function `call`.
fn expr_suggests_call_chain(expr: &SpExpr) -> bool {
    match &expr.node {
        Expr::Call { func, .. } => {
            matches!(&func.as_ref().node, Expr::Ident(name) if !is_builtin_call(name))
        }
        _ => false,
    }
}

fn is_builtin_call(name: &str) -> bool {
    matches!(name, "length" | "old" | "abs" | "min" | "max")
}

fn format_ir_module_plan(
    name: &str,
    params: &[(usize, String)],
    return_ty: &str,
    requires_count: usize,
    ensures_count: usize,
    plan: &IrGenPlan,
) -> String {
    let module = sanitize_module_name(name);
    let param_list = params
        .iter()
        .map(|(slot, ty)| format!("${slot}: {ty}"))
        .collect::<Vec<_>>()
        .join(", ");
    let main_body = plan.main.lines.join("\n");
    let mut out = format!(
        "// Generated IR for {name} from ensures heuristics\n\
         // Contract: {requires_count} requires, {ensures_count} ensures\n\
         module {module} {{\n\
           fn #0 : ({param_list}) -> {return_ty} ! pure\n\
           pre: true\n\
           {{\n\
         {main_body}\n\
           }}\n"
    );
    for (block_id, sibling) in &plan.siblings {
        let sib_body = sibling.lines.join("\n");
        out.push_str(&format!(
            "  fn #{block_id} : ($0: Int) -> {return_ty} ! pure\n\
             pre: true\n\
             {{\n\
         {sib_body}\n\
           }}\n"
        ));
    }
    out.push_str("}\n");
    out
}

fn sanitize_module_name(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use assura_ast::{BinOp, ClauseKind, Expr, SpExpr, Spanned};

    fn sp(e: Expr) -> SpExpr {
        Spanned::no_span(e)
    }

    fn spb(e: Expr) -> Box<SpExpr> {
        Box::new(sp(e))
    }

    fn int_param(_name: &str, slot: usize) -> (usize, String) {
        (slot, "Int".into())
    }

    fn bytes_len_le_result_raw() -> SpExpr {
        sp(Expr::BinOp {
            op: BinOp::Lte,
            lhs: spb(Expr::MethodCall {
                receiver: spb(Expr::Ident("result".into())),
                method: "length".into(),
                args: vec![],
            }),
            rhs: spb(Expr::MethodCall {
                receiver: spb(Expr::Ident("raw".into())),
                method: "length".into(),
                args: vec![],
            }),
        })
    }

    #[test]
    fn generates_load_when_ensures_result_eq_param() {
        let clauses = vec![Clause {
            kind: ClauseKind::Ensures,
            body: sp(Expr::BinOp {
                op: BinOp::Eq,
                lhs: spb(Expr::Ident("result".into())),
                rhs: spb(Expr::Ident("x".into())),
            }),
            effect_variables: vec![],
        }];
        let text =
            generate_ir_sidecar_text("Echo", &[int_param("x", 0)], &["x".into()], "Int", &clauses);
        assert!(text.contains("$result = load $0 : Int"));
        assert!(text.contains("Generated IR"));
    }

    #[test]
    fn generates_const_when_ensures_result_eq_literal() {
        let clauses = vec![Clause {
            kind: ClauseKind::Ensures,
            body: sp(Expr::BinOp {
                op: BinOp::Eq,
                lhs: spb(Expr::Ident("result".into())),
                rhs: spb(Expr::Literal(Literal::Int("42".into()))),
            }),
            effect_variables: vec![],
        }];
        let text = generate_ir_sidecar_text("Const42", &[], &[], "Int", &clauses);
        assert!(text.contains("$result = const 42 : Int"));
    }

    #[test]
    fn generates_arith_when_ensures_result_eq_sum() {
        let clauses = vec![Clause {
            kind: ClauseKind::Ensures,
            body: sp(Expr::BinOp {
                op: BinOp::Eq,
                lhs: spb(Expr::Ident("result".into())),
                rhs: spb(Expr::BinOp {
                    op: BinOp::Add,
                    lhs: spb(Expr::Ident("x".into())),
                    rhs: spb(Expr::Ident("y".into())),
                }),
            }),
            effect_variables: vec![],
        }];
        let text = generate_ir_sidecar_text(
            "Add",
            &[int_param("x", 0), (1, "Int".into())],
            &["x".into(), "y".into()],
            "Int",
            &clauses,
        );
        assert!(text.contains("arith add $0 $1"));
        assert!(text.contains("$result = load $2"));
    }

    #[test]
    fn generates_load_when_ensures_length_copy() {
        let clauses = vec![Clause {
            kind: ClauseKind::Ensures,
            body: bytes_len_le_result_raw(),
            effect_variables: vec![],
        }];
        let text = generate_ir_sidecar_text(
            "CopyBytes",
            &[(0, "Bytes".into())],
            &["raw".into()],
            "Bytes",
            &clauses,
        );
        assert!(text.contains("$result = load $0 : Bytes"));
        assert!(text.contains("Generated IR"));
    }

    #[test]
    fn classifies_call_chain_when_result_eq_helper_call() {
        let clauses = vec![Clause {
            kind: ClauseKind::Ensures,
            body: sp(Expr::BinOp {
                op: BinOp::Eq,
                lhs: spb(Expr::Ident("result".into())),
                rhs: spb(Expr::Call {
                    func: spb(Expr::Ident("double".into())),
                    args: vec![sp(Expr::Ident("x".into()))],
                }),
            }),
            effect_variables: vec![],
        }];
        assert_eq!(
            classify_ensures_shape(&clauses, &["x".into()]),
            EnsuresShape::CallChain
        );
    }

    #[test]
    fn classifies_length_copy_shape() {
        let clauses = vec![Clause {
            kind: ClauseKind::Ensures,
            body: bytes_len_le_result_raw(),
            effect_variables: vec![],
        }];
        assert_eq!(
            classify_ensures_shape(&clauses, &["raw".into()]),
            EnsuresShape::LengthCopy
        );
    }

    #[test]
    fn test_ir_generate_if_branch() {
        let clauses = vec![Clause {
            kind: ClauseKind::Ensures,
            body: sp(Expr::BinOp {
                op: BinOp::Eq,
                lhs: spb(Expr::Ident("result".into())),
                rhs: spb(Expr::If {
                    cond: spb(Expr::BinOp {
                        op: BinOp::Gt,
                        lhs: spb(Expr::Ident("x".into())),
                        rhs: spb(Expr::Literal(Literal::Int("0".into()))),
                    }),
                    then_branch: spb(Expr::Ident("x".into())),
                    else_branch: Some(spb(Expr::Literal(Literal::Int("0".into())))),
                }),
            }),
            effect_variables: vec![],
        }];
        let text = generate_ir_sidecar_text(
            "IfBranch",
            &[int_param("x", 0)],
            &["x".into()],
            "Int",
            &clauses,
        );
        assert!(text.contains("if $0 then #1 else #2"));
        assert!(text.contains("fn #1"));
        assert!(text.contains("fn #2"));
    }

    #[test]
    fn test_ir_generate_match_arm() {
        use assura_ast::{MatchArm, Pattern};

        let clauses = vec![Clause {
            kind: ClauseKind::Ensures,
            body: sp(Expr::BinOp {
                op: BinOp::Eq,
                lhs: spb(Expr::Ident("result".into())),
                rhs: spb(Expr::Match {
                    scrutinee: spb(Expr::Ident("x".into())),
                    arms: vec![
                        MatchArm {
                            pattern: Pattern::Literal(Literal::Int("0".into())),
                            body: sp(Expr::Literal(Literal::Int("0".into()))),
                        },
                        MatchArm {
                            pattern: Pattern::Ident("_".into()),
                            body: sp(Expr::Ident("x".into())),
                        },
                    ],
                }),
            }),
            effect_variables: vec![],
        }];
        let text = generate_ir_sidecar_text(
            "MatchArm",
            &[int_param("x", 0)],
            &["x".into()],
            "Int",
            &clauses,
        );
        // #854: must emit IR match with patterns, not boolean if on scrutinee alone.
        assert!(
            text.contains("match $0"),
            "expected IR match instruction, got:\n{text}"
        );
        assert!(
            text.contains("0 => #1") || text.contains("0 =>#1"),
            "expected pattern arm for literal 0, got:\n{text}"
        );
        assert!(
            text.contains("_ => #2") || text.contains("_ =>#2"),
            "expected wildcard arm, got:\n{text}"
        );
        assert!(
            !text.contains("if $0 then #1 else #2"),
            "must not use pattern-blind boolean if, got:\n{text}"
        );
        assert!(text.contains("fn #1"));
        assert!(text.contains("fn #2"));
    }

    #[test]
    fn test_ir_generate_match_constructor_patterns() {
        use assura_ast::{MatchArm, Pattern};

        // match status { "Ok" => 1, "Err" => 0 } style via Ident patterns
        let clauses = vec![Clause {
            kind: ClauseKind::Ensures,
            body: sp(Expr::BinOp {
                op: BinOp::Eq,
                lhs: spb(Expr::Ident("result".into())),
                rhs: spb(Expr::Match {
                    scrutinee: spb(Expr::Ident("status".into())),
                    arms: vec![
                        MatchArm {
                            pattern: Pattern::Ident("Ok".into()),
                            body: sp(Expr::Literal(Literal::Int("1".into()))),
                        },
                        MatchArm {
                            pattern: Pattern::Ident("Err".into()),
                            body: sp(Expr::Literal(Literal::Int("0".into()))),
                        },
                    ],
                }),
            }),
            effect_variables: vec![],
        }];
        let text = generate_ir_sidecar_text(
            "StatusMatch",
            &[int_param("status", 0)],
            &["status".into()],
            "Int",
            &clauses,
        );
        assert!(
            text.contains("match $0"),
            "constructor-style match should use IR match:\n{text}"
        );
        assert!(
            text.contains("\"Ok\"") && text.contains("\"Err\""),
            "arms should record pattern discriminators:\n{text}"
        );
        assert!(
            !text.contains("if $0 then"),
            "must not ignore patterns via boolean if:\n{text}"
        );
    }

    #[test]
    fn test_ir_generate_multi_fn_call_chain() {
        let clauses = vec![Clause {
            kind: ClauseKind::Ensures,
            body: sp(Expr::BinOp {
                op: BinOp::Eq,
                lhs: spb(Expr::Ident("result".into())),
                rhs: spb(Expr::Call {
                    func: spb(Expr::Ident("double".into())),
                    args: vec![sp(Expr::Ident("x".into()))],
                }),
            }),
            effect_variables: vec![],
        }];
        let text = generate_ir_sidecar_text(
            "Double",
            &[int_param("x", 0)],
            &["x".into()],
            "Int",
            &clauses,
        );
        assert!(text.contains("call double ($0)"));
        assert!(text.contains("fn #1"));
    }

    #[test]
    fn falls_back_to_stub_when_ensures_unrecognized() {
        let clauses = vec![Clause {
            kind: ClauseKind::Ensures,
            body: sp(Expr::BinOp {
                op: BinOp::Gt,
                lhs: spb(Expr::Ident("result".into())),
                rhs: spb(Expr::Literal(Literal::Int("0".into()))),
            }),
            effect_variables: vec![],
        }];
        let text = generate_ir_sidecar_text(
            "Unknown",
            &[int_param("x", 0)],
            &["x".into()],
            "Int",
            &clauses,
        );
        assert!(text.contains("Stub IR"));
    }
}
