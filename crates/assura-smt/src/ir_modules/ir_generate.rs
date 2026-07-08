//! Heuristic contract-to-Implementation-IR generation.
//!
//! Analyzes `ensures` clauses to produce IR bodies richer than identity stubs.
//! Falls back to `stub_ir_sidecar_text` when no pattern matches.
//!
//! # Call-shaped ensures (#863)
//!
//! `ensures { result == double(x) }` uses a cross-function `call`. Sibling
//! `fn #1` bodies are synthesized from the **callee's ensures** when the
//! callee is known in-file (e.g. `result == x + x` → `arith add`). Unknown
//! or unanalyzable callees no longer emit a silent identity sibling; the
//! planner returns `None` so the module falls back to a labeled stub.

use assura_ast::{BinOp, Clause, ClauseKind, Expr, Literal, SpExpr, UnaryOp};
use std::collections::HashMap;

use crate::ir_codegen::stub_ir_sidecar_text;

/// Same-file callee summary for call-chain IR synthesis (#863).
#[derive(Debug, Clone)]
pub struct CalleeSpec {
    /// Parameter names in slot order (`$0`, `$1`, …).
    pub param_names: Vec<String>,
    /// Return type token for IR (`Int`, `Bool`, …).
    pub return_ty: String,
    /// Callee contract/fn clauses (ensures drive the sibling body).
    pub clauses: Vec<Clause>,
}

/// Shared context for IR body planners (contract params → slots).
#[derive(Debug, Clone)]
pub(crate) struct PlanCtx<'a> {
    pub name_to_slot: HashMap<&'a str, usize>,
    pub return_ty: &'a str,
    /// In-file callees keyed by declaration name (exact `call` target).
    pub callees: &'a HashMap<String, CalleeSpec>,
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
/// Sibling entries are `(block_id, body, param_count)` for signature synthesis.
#[derive(Debug, Clone, PartialEq)]
struct IrGenPlan {
    main: IrGenBody,
    siblings: Vec<(usize, IrGenBody, usize)>,
}

type IrPlannerFn = fn(&SpExpr, &PlanCtx<'_>) -> Option<IrGenPlan>;

const ENSURES_PLANNERS: &[IrPlannerFn] = &[
    plan_if_branch_ensures,
    plan_match_arm_ensures,
    plan_abs_call_ensures,
    plan_min_max_call_ensures,
    plan_bool_comparison_ensures,
    plan_bool_logic_ensures,
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
///
/// Equivalent to [`generate_ir_sidecar_text_with_callees`] with an empty
/// callee map (no call-chain sibling synthesis from other contracts).
pub fn generate_ir_sidecar_text(
    name: &str,
    params: &[(usize, String)],
    param_names: &[String],
    return_ty: &str,
    clauses: &[Clause],
) -> String {
    generate_ir_sidecar_text_with_callees(
        name,
        params,
        param_names,
        return_ty,
        clauses,
        &HashMap::new(),
    )
}

/// Like [`generate_ir_sidecar_text`], but synthesizes non-identity sibling
/// bodies for unary pure callees present in `callees` (#863).
pub fn generate_ir_sidecar_text_with_callees(
    name: &str,
    params: &[(usize, String)],
    param_names: &[String],
    return_ty: &str,
    clauses: &[Clause],
    callees: &HashMap<String, CalleeSpec>,
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
        callees,
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

/// Max nesting depth for `if` synthesis (pathological nesting falls back to stub).
const MAX_IF_NESTING: usize = 4;

/// `ensures { result == if cond then a else b }` → branch blocks `#1` / `#2`.
///
/// Nested then/else (`if x > 0 then if x > 10 then 2 else 1 else 0`) allocate
/// further sibling block IDs up to [`MAX_IF_NESTING`] (#885).
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

    let mut lines: Vec<String> = Vec::new();
    let mut used: Vec<usize> = ctx.name_to_slot.values().copied().collect();
    used.sort_unstable();
    let mut next_block = 1usize;
    let mut siblings: Vec<(usize, IrGenBody, usize)> = Vec::new();

    let cond_slot = plan_bool_condition_slot(cond, ctx, &mut lines, &mut used)?;
    let then_id = next_block;
    next_block += 1;
    let else_id = next_block;
    next_block += 1;
    plan_branch_into_block(then_branch, then_id, ctx, 1, &mut next_block, &mut siblings)?;
    plan_branch_into_block(else_branch, else_id, ctx, 1, &mut next_block, &mut siblings)?;

    let out_slot = next_temp_slot(&used);
    lines.push(format!(
        "    ${out_slot} = if ${cond_slot} then #{then_id} else #{else_id} : {}",
        ctx.return_ty
    ));
    lines.push(format!(
        "    $result = load ${out_slot} : {}",
        ctx.return_ty
    ));
    Some(IrGenPlan {
        main: IrGenBody { lines },
        siblings,
    })
}

/// Fill sibling `block_id` with either a leaf result body or a nested `if`.
fn plan_branch_into_block(
    expr: &SpExpr,
    block_id: usize,
    ctx: &PlanCtx<'_>,
    depth: usize,
    next_block: &mut usize,
    siblings: &mut Vec<(usize, IrGenBody, usize)>,
) -> Option<()> {
    if depth < MAX_IF_NESTING
        && let Expr::If {
            cond,
            then_branch,
            else_branch,
        } = &expr.node
    {
        let Some(else_branch) = else_branch.as_deref() else {
            // Missing else: fall through to leaf planner if possible.
            let body = plan_branch_result(expr, ctx)?;
            siblings.push((block_id, body, 0));
            return Some(());
        };
        let mut lines: Vec<String> = Vec::new();
        let mut used: Vec<usize> = ctx.name_to_slot.values().copied().collect();
        used.sort_unstable();
        let cond_slot = plan_bool_condition_slot(cond, ctx, &mut lines, &mut used)?;
        let then_id = *next_block;
        *next_block += 1;
        let else_id = *next_block;
        *next_block += 1;
        plan_branch_into_block(then_branch, then_id, ctx, depth + 1, next_block, siblings)?;
        plan_branch_into_block(else_branch, else_id, ctx, depth + 1, next_block, siblings)?;
        let out_slot = next_temp_slot(&used);
        lines.push(format!(
            "    ${out_slot} = if ${cond_slot} then #{then_id} else #{else_id} : {}",
            ctx.return_ty
        ));
        lines.push(format!(
            "    $result = load ${out_slot} : {}",
            ctx.return_ty
        ));
        siblings.push((block_id, IrGenBody { lines }, 0));
        return Some(());
    }

    let body = plan_branch_result(expr, ctx)?;
    siblings.push((block_id, body, 0));
    Some(())
}

/// Encode a boolean condition (param Bool, comparison, or nested) as a slot.
///
/// Bools are 0/1 Int slots. Logical `&&`/`||`/`==>` lower without extra blocks:
/// - `a && b` → `arith mul a b`
/// - `a || b` → `cmp ne (a + b) 0`
/// - `a ==> b` → `(!a) || b`
fn plan_bool_condition_slot(
    cond: &SpExpr,
    ctx: &PlanCtx<'_>,
    lines: &mut Vec<String>,
    used: &mut Vec<usize>,
) -> Option<usize> {
    match &cond.node {
        Expr::Ident(name) => {
            // Assume named Bool params are already Bool-typed slots.
            ctx.name_to_slot.get(name.as_str()).copied()
        }
        Expr::BinOp { op, lhs, rhs } if ir_cmp_op_name(op).is_some() => {
            let ir_op = ir_cmp_op_name(op)?;
            let a = operand_to_slot(lhs.as_ref(), ctx, lines, used)?;
            let b = operand_to_slot(rhs.as_ref(), ctx, lines, used)?;
            let slot = next_temp_slot(used);
            used.push(slot);
            lines.push(format!("    ${slot} = cmp {ir_op} ${a} ${b} : Bool"));
            Some(slot)
        }
        Expr::BinOp {
            op: BinOp::And,
            lhs,
            rhs,
        } => {
            let a = plan_bool_condition_slot(lhs.as_ref(), ctx, lines, used)?;
            let b = plan_bool_condition_slot(rhs.as_ref(), ctx, lines, used)?;
            let slot = next_temp_slot(used);
            used.push(slot);
            lines.push(format!("    ${slot} = arith mul ${a} ${b} : Bool"));
            Some(slot)
        }
        Expr::BinOp {
            op: BinOp::Or,
            lhs,
            rhs,
        } => {
            let a = plan_bool_condition_slot(lhs.as_ref(), ctx, lines, used)?;
            let b = plan_bool_condition_slot(rhs.as_ref(), ctx, lines, used)?;
            let sum = next_temp_slot(used);
            used.push(sum);
            lines.push(format!("    ${sum} = arith add ${a} ${b} : Bool"));
            let zero = next_temp_slot(used);
            used.push(zero);
            lines.push(format!("    ${zero} = const 0 : Bool"));
            let slot = next_temp_slot(used);
            used.push(slot);
            lines.push(format!("    ${slot} = cmp ne ${sum} ${zero} : Bool"));
            Some(slot)
        }
        Expr::BinOp {
            op: BinOp::Implies,
            lhs,
            rhs,
        } => {
            // a ==> b  ≡  (!a) || b on 0/1 Bool slots
            let a = plan_bool_condition_slot(lhs.as_ref(), ctx, lines, used)?;
            let b = plan_bool_condition_slot(rhs.as_ref(), ctx, lines, used)?;
            let zero = next_temp_slot(used);
            used.push(zero);
            lines.push(format!("    ${zero} = const 0 : Bool"));
            let not_a = next_temp_slot(used);
            used.push(not_a);
            lines.push(format!("    ${not_a} = cmp eq ${a} ${zero} : Bool"));
            let sum = next_temp_slot(used);
            used.push(sum);
            lines.push(format!("    ${sum} = arith add ${not_a} ${b} : Bool"));
            let slot = next_temp_slot(used);
            used.push(slot);
            lines.push(format!("    ${slot} = cmp ne ${sum} ${zero} : Bool"));
            Some(slot)
        }
        Expr::UnaryOp {
            op: UnaryOp::Not,
            expr: inner,
        } => {
            let inner_slot = plan_bool_condition_slot(inner.as_ref(), ctx, lines, used)?;
            // Encode !b as `b == false` via cmp ne against true... use if: 0/1 not ideal.
            // Prefer cmp eq inner false: need const false as 0 for Bool?
            let zero = next_temp_slot(used);
            used.push(zero);
            lines.push(format!("    ${zero} = const 0 : Bool"));
            let slot = next_temp_slot(used);
            used.push(slot);
            lines.push(format!("    ${slot} = cmp eq ${inner_slot} ${zero} : Bool"));
            Some(slot)
        }
        _ => None,
    }
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
        siblings.push((block_id, body, 0));
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
        if let Some((_, last_body, nparams)) = siblings.last().cloned() {
            let wid = siblings.len() + 1;
            arm_texts.push(format!("_ => #{wid}"));
            siblings.push((wid, last_body, nparams));
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

/// `ensures { result == abs(x) }` → `if x >= 0 then x else 0 - x`.
fn plan_abs_call_ensures(expr: &SpExpr, ctx: &PlanCtx<'_>) -> Option<IrGenPlan> {
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
    let Expr::Ident(name) = &func.as_ref().node else {
        return None;
    };
    if name != "abs" || args.len() != 1 {
        return None;
    }
    let mut lines: Vec<String> = Vec::new();
    let mut used: Vec<usize> = ctx.name_to_slot.values().copied().collect();
    used.sort_unstable();
    let x_slot = operand_to_slot(&args[0], ctx, &mut lines, &mut used)?;
    let zero = next_temp_slot(&used);
    used.push(zero);
    lines.push(format!("    ${zero} = const 0 : Int"));
    let cond = next_temp_slot(&used);
    used.push(cond);
    lines.push(format!("    ${cond} = cmp ge ${x_slot} ${zero} : Bool"));
    let out = next_temp_slot(&used);
    used.push(out);
    lines.push(format!(
        "    ${out} = if ${cond} then #1 else #2 : {}",
        ctx.return_ty
    ));
    lines.push(format!("    $result = load ${out} : {}", ctx.return_ty));
    let pos = single_load(x_slot, ctx.return_ty);
    // Fresh temps so sibling body does not clobber the outer x slot (often $0).
    let z = next_temp_slot(&[x_slot]);
    let t = next_temp_slot(&[x_slot, z]);
    let neg = IrGenBody {
        lines: vec![
            format!("    ${z} = const 0 : {}", ctx.return_ty),
            format!("    ${t} = arith sub ${z} ${x_slot} : {}", ctx.return_ty),
            format!("    $result = load ${t} : {}", ctx.return_ty),
        ],
    };
    Some(IrGenPlan {
        main: IrGenBody { lines },
        siblings: vec![(1, pos, 0), (2, neg, 0)],
    })
}

/// `ensures { result == min(x, y) }` / `max(x, y)` → if-compare over args.
fn plan_min_max_call_ensures(expr: &SpExpr, ctx: &PlanCtx<'_>) -> Option<IrGenPlan> {
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
    let Expr::Ident(name) = &func.as_ref().node else {
        return None;
    };
    if args.len() != 2 {
        return None;
    }
    // min: if x < y then x else y; max: if x > y then x else y
    let cmp = match name.as_str() {
        "min" => "lt",
        "max" => "gt",
        _ => return None,
    };
    let mut lines: Vec<String> = Vec::new();
    let mut used: Vec<usize> = ctx.name_to_slot.values().copied().collect();
    used.sort_unstable();
    let a_slot = operand_to_slot(&args[0], ctx, &mut lines, &mut used)?;
    let b_slot = operand_to_slot(&args[1], ctx, &mut lines, &mut used)?;
    let cond = next_temp_slot(&used);
    used.push(cond);
    lines.push(format!(
        "    ${cond} = cmp {cmp} ${a_slot} ${b_slot} : Bool"
    ));
    let out = next_temp_slot(&used);
    used.push(out);
    lines.push(format!(
        "    ${out} = if ${cond} then #1 else #2 : {}",
        ctx.return_ty
    ));
    lines.push(format!("    $result = load ${out} : {}", ctx.return_ty));
    Some(IrGenPlan {
        main: IrGenBody { lines },
        siblings: vec![
            (1, single_load(a_slot, ctx.return_ty), 0),
            (2, single_load(b_slot, ctx.return_ty), 0),
        ],
    })
}

/// `ensures { result == (x > 0) }` (Bool return) via IR `cmp`.
fn plan_bool_comparison_ensures(expr: &SpExpr, ctx: &PlanCtx<'_>) -> Option<IrGenPlan> {
    if !ctx.return_ty.eq_ignore_ascii_case("Bool") {
        return None;
    }
    let (lhs, rhs) = equality_operands(expr)?;
    let other = if is_result_ident(lhs) {
        rhs
    } else if is_result_ident(rhs) {
        lhs
    } else {
        return None;
    };
    let Expr::BinOp { op, lhs: a, rhs: b } = &other.node else {
        return None;
    };
    let ir_op = ir_cmp_op_name(op)?;
    let mut lines: Vec<String> = Vec::new();
    let mut used: Vec<usize> = ctx.name_to_slot.values().copied().collect();
    used.sort_unstable();
    let a_slot = operand_to_slot(a.as_ref(), ctx, &mut lines, &mut used)?;
    let b_slot = operand_to_slot(b.as_ref(), ctx, &mut lines, &mut used)?;
    let out = next_temp_slot(&used);
    lines.push(format!(
        "    ${out} = cmp {ir_op} ${a_slot} ${b_slot} : Bool"
    ));
    lines.push(format!("    $result = load ${out} : Bool"));
    Some(single_fn_plan(IrGenBody { lines }))
}

/// `ensures { result == <bool-expr> }` for nested `!` / `&&` / `||` / `=>` / cmp.
///
/// Materializes the entire RHS through [`plan_bool_condition_slot`] so nested
/// forms like `a && (b || c)` and `((x || y) && !(x && y))` work. Earlier
/// And/Or paths used sibling blocks + [`plan_branch_result`], which only
/// accepted identity/arith leaves and dropped nested bool operators.
fn plan_bool_logic_ensures(expr: &SpExpr, ctx: &PlanCtx<'_>) -> Option<IrGenPlan> {
    if !ctx.return_ty.eq_ignore_ascii_case("Bool") {
        return None;
    }
    let (lhs, rhs) = equality_operands(expr)?;
    let other = if is_result_ident(lhs) {
        rhs
    } else if is_result_ident(rhs) {
        lhs
    } else {
        return None;
    };

    // Only claim shapes that look boolean; pure identity `result == x` stays
    // with plan_identity_equality (single load, no temps).
    let looks_bool = match &other.node {
        Expr::UnaryOp {
            op: UnaryOp::Not, ..
        } => true,
        Expr::BinOp {
            op: BinOp::And | BinOp::Or | BinOp::Implies,
            ..
        } => true,
        Expr::BinOp { op, .. } if ir_cmp_op_name(op).is_some() => true,
        _ => false,
    };
    if !looks_bool {
        return None;
    }

    let mut lines: Vec<String> = Vec::new();
    let mut used: Vec<usize> = ctx.name_to_slot.values().copied().collect();
    used.sort_unstable();
    let out = plan_bool_condition_slot(other, ctx, &mut lines, &mut used)?;
    lines.push(format!("    $result = load ${out} : Bool"));
    Some(single_fn_plan(IrGenBody { lines }))
}

fn ir_cmp_op_name(op: &BinOp) -> Option<&'static str> {
    match op {
        BinOp::Eq => Some("eq"),
        BinOp::Neq => Some("ne"),
        BinOp::Lt => Some("lt"),
        BinOp::Lte => Some("le"),
        BinOp::Gt => Some("gt"),
        BinOp::Gte => Some("ge"),
        _ => None,
    }
}

/// Plans IR for `ensures { result == f(x) }` call chains (#863).
///
/// Unary pure helpers only. Sibling `fn #1` is synthesized from the
/// callee's analyzable ensures (e.g. `result == x + x` → `arith add`).
/// Multi-arg, recursive, effectful, or unknown callees return `None`
/// (explicit stub fallback) rather than a silent identity sibling.
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
    // Builtin names are not synthesized as call plans.
    if is_builtin_call(helper) {
        return None;
    }
    let callee = ctx.callees.get(helper.as_str())?;
    // Pure callees only when arity matches and ensures is analyzable.
    if callee.param_names.len() != args.len() {
        return None;
    }
    let sibling_body = plan_callee_body_from_ensures(callee)?;
    let mut arg_slots = Vec::with_capacity(args.len());
    for arg in args {
        arg_slots.push(expr_to_param_slot(arg, &ctx.name_to_slot)?);
    }
    let temp = next_temp_slot(&arg_slots);
    let arg_list = arg_slots
        .iter()
        .map(|s| format!("${s}"))
        .collect::<Vec<_>>()
        .join(", ");
    Some(IrGenPlan {
        main: IrGenBody {
            lines: vec![
                format!(
                    "    ${temp} = call {helper} ({arg_list}) : {}",
                    ctx.return_ty
                ),
                format!("    $result = load ${temp} : {}", ctx.return_ty),
            ],
        },
        siblings: vec![(1, sibling_body, callee.param_names.len())],
    })
}

/// Strict body from a callee's ensures: Ident / Literal / arithmetic only.
/// No single-param identity fallback (that would reintroduce silent stubs).
fn plan_callee_body_from_ensures(callee: &CalleeSpec) -> Option<IrGenBody> {
    let name_to_slot: HashMap<&str, usize> = callee
        .param_names
        .iter()
        .enumerate()
        .map(|(i, n)| (n.as_str(), i))
        .collect();
    let empty = HashMap::new();
    let ctx = PlanCtx {
        name_to_slot,
        return_ty: callee.return_ty.as_str(),
        // Nested call chains are out of scope for v1 sibling synthesis.
        callees: &empty,
    };
    for clause in callee
        .clauses
        .iter()
        .filter(|c| c.kind == ClauseKind::Ensures)
    {
        let (lhs, rhs) = match equality_operands(&clause.body) {
            Some(pair) => pair,
            None => continue,
        };
        let other = if is_result_ident(lhs) {
            rhs
        } else if is_result_ident(rhs) {
            lhs
        } else {
            continue;
        };
        // Nested calls / complex shapes: unanalyzable for v1.
        if matches!(
            &other.node,
            Expr::Call { .. } | Expr::If { .. } | Expr::Match { .. }
        ) {
            return None;
        }
        if let Some(body) = plan_result_equals_strict(other, &ctx) {
            return Some(body);
        }
    }
    None
}

/// Like [`plan_result_equals`] but without the single-param identity fallback.
fn plan_result_equals_strict(other: &SpExpr, ctx: &PlanCtx<'_>) -> Option<IrGenBody> {
    match &other.node {
        Expr::Ident(name) => {
            let slot = *ctx.name_to_slot.get(name.as_str())?;
            Some(single_load(slot, ctx.return_ty))
        }
        Expr::Literal(lit) => Some(single_const(&literal_to_ir_const(lit)?, ctx.return_ty)),
        Expr::BinOp { op, lhs, rhs } => {
            plan_result_arith(op.clone(), lhs.as_ref(), rhs.as_ref(), ctx)
        }
        _ => None,
    }
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
        Expr::BinOp { op, lhs, rhs } if op.is_arithmetic() => {
            plan_result_arith(op.clone(), lhs.as_ref(), rhs.as_ref(), ctx)
        }
        // Nested arith / unary negation: materialize temps then load into result.
        // Do NOT fall back to identity for one-param contracts (that produced
        // wrong IR for unanalyzable RHS and hid the "not synthesizable" path).
        Expr::BinOp { .. } | Expr::UnaryOp { .. } => {
            let mut lines: Vec<String> = Vec::new();
            let mut used: Vec<usize> = ctx.name_to_slot.values().copied().collect();
            used.sort_unstable();
            let slot = operand_to_slot(other, ctx, &mut lines, &mut used)?;
            lines.push(format!("    $result = load ${slot} : {}", ctx.return_ty));
            Some(IrGenBody { lines })
        }
        _ => None,
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

    // Support param/param, param/literal, and literal/param (e.g. `result == x + 1`).
    // Literals become `const` temps so IR arith always takes slot operands.
    let mut lines: Vec<String> = Vec::new();
    let mut used: Vec<usize> = ctx.name_to_slot.values().copied().collect();
    used.sort_unstable();
    let lhs_slot = operand_to_slot(lhs, ctx, &mut lines, &mut used)?;
    let rhs_slot = operand_to_slot(rhs, ctx, &mut lines, &mut used)?;
    let temp_slot = next_temp_slot(&used);
    lines.push(format!(
        "    ${temp_slot} = arith {ir_op} ${lhs_slot} ${rhs_slot} : {}",
        ctx.return_ty
    ));
    lines.push(format!(
        "    $result = load ${temp_slot} : {}",
        ctx.return_ty
    ));

    Some(IrGenBody { lines })
}

/// Resolve an arithmetic operand to a slot.
///
/// Supports parameters, integer/bool literals (`const` temps), nested
/// arithmetic binops (e.g. `(x + 1) * 2`), and unary negation (`-x` as `0 - x`).
fn operand_to_slot(
    expr: &SpExpr,
    ctx: &PlanCtx<'_>,
    lines: &mut Vec<String>,
    used: &mut Vec<usize>,
) -> Option<usize> {
    match &expr.node {
        Expr::Ident(name) => ctx.name_to_slot.get(name.as_str()).copied(),
        Expr::Literal(lit) => {
            let value = literal_to_ir_const(lit)?;
            let slot = next_temp_slot(used);
            used.push(slot);
            lines.push(format!("    ${slot} = const {value} : {}", ctx.return_ty));
            Some(slot)
        }
        Expr::BinOp { op, lhs, rhs } if op.is_arithmetic() => {
            let ir_op = op.as_ident();
            let lhs_slot = operand_to_slot(lhs.as_ref(), ctx, lines, used)?;
            let rhs_slot = operand_to_slot(rhs.as_ref(), ctx, lines, used)?;
            let slot = next_temp_slot(used);
            used.push(slot);
            lines.push(format!(
                "    ${slot} = arith {ir_op} ${lhs_slot} ${rhs_slot} : {}",
                ctx.return_ty
            ));
            Some(slot)
        }
        Expr::UnaryOp {
            op: UnaryOp::Neg,
            expr: inner,
        } => {
            let zero = next_temp_slot(used);
            used.push(zero);
            lines.push(format!("    ${zero} = const 0 : {}", ctx.return_ty));
            let inner_slot = operand_to_slot(inner.as_ref(), ctx, lines, used)?;
            let slot = next_temp_slot(used);
            used.push(slot);
            lines.push(format!(
                "    ${slot} = arith sub ${zero} ${inner_slot} : {}",
                ctx.return_ty
            ));
            Some(slot)
        }
        _ => None,
    }
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
    for (block_id, sibling, nparams) in &plan.siblings {
        let sib_body = sibling.lines.join("\n");
        // Branch/match arms historically use a dummy `$0: Int` signature even
        // when they close over main slots; call callees use real arity.
        let param_list = if *nparams == 0 {
            "$0: Int".to_string()
        } else {
            (0..*nparams)
                .map(|i| format!("${i}: Int"))
                .collect::<Vec<_>>()
                .join(", ")
        };
        out.push_str(&format!(
            "  fn #{block_id} : ({param_list}) -> {return_ty} ! pure\n\
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
        // Condition is materialised as Bool cmp, not raw param $0.
        assert!(
            text.contains("cmp gt") && text.contains("then #1 else #2"),
            "expected cmp + if branches, got:\n{text}"
        );
        assert!(text.contains("fn #1"));
        assert!(text.contains("fn #2"));
    }

    #[test]
    fn test_ir_generate_nested_if() {
        // result == if x > 0 then (if x > 10 then 2 else 1) else 0
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
                    then_branch: spb(Expr::If {
                        cond: spb(Expr::BinOp {
                            op: BinOp::Gt,
                            lhs: spb(Expr::Ident("x".into())),
                            rhs: spb(Expr::Literal(Literal::Int("10".into()))),
                        }),
                        then_branch: spb(Expr::Literal(Literal::Int("2".into()))),
                        else_branch: Some(spb(Expr::Literal(Literal::Int("1".into())))),
                    }),
                    else_branch: Some(spb(Expr::Literal(Literal::Int("0".into())))),
                }),
            }),
            effect_variables: vec![],
        }];
        let text = generate_ir_sidecar_text(
            "Nested",
            &[int_param("x", 0)],
            &["x".into()],
            "Int",
            &clauses,
        );
        assert!(
            text.contains("then #1 else #2") && text.contains("fn #1") && text.contains("fn #3"),
            "expected nested if with multiple sibling blocks, got:\n{text}"
        );
        // Nested then-branch should itself contain an if (not only leaf load).
        assert!(
            text.matches("if $").count() >= 2 || text.matches("then #").count() >= 2,
            "expected at least two ifs in nested plan, got:\n{text}"
        );
        assert!(
            !text.contains("Stub IR"),
            "nested if must not fall back to stub:\n{text}"
        );
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
    fn test_ir_generate_bool_not() {
        let clauses = vec![Clause {
            kind: ClauseKind::Ensures,
            body: sp(Expr::BinOp {
                op: BinOp::Eq,
                lhs: spb(Expr::Ident("result".into())),
                rhs: spb(Expr::UnaryOp {
                    op: UnaryOp::Not,
                    expr: spb(Expr::Ident("x".into())),
                }),
            }),
            effect_variables: vec![],
        }];
        let text = generate_ir_sidecar_text(
            "NotB",
            &[(0, "Bool".into())],
            &["x".into()],
            "Bool",
            &clauses,
        );
        assert!(
            text.contains("cmp eq") && text.contains("const 0 : Bool"),
            "expected !x as cmp eq x 0, got:\n{text}"
        );
        assert!(text.contains("$result = load"));
    }

    #[test]
    fn test_ir_generate_bool_and() {
        let clauses = vec![Clause {
            kind: ClauseKind::Ensures,
            body: sp(Expr::BinOp {
                op: BinOp::Eq,
                lhs: spb(Expr::Ident("result".into())),
                rhs: spb(Expr::BinOp {
                    op: BinOp::And,
                    lhs: spb(Expr::Ident("x".into())),
                    rhs: spb(Expr::Ident("y".into())),
                }),
            }),
            effect_variables: vec![],
        }];
        let text = generate_ir_sidecar_text(
            "AndB",
            &[(0, "Bool".into()), (1, "Bool".into())],
            &["x".into(), "y".into()],
            "Bool",
            &clauses,
        );
        assert!(
            text.contains("arith mul") && text.contains("$result = load"),
            "expected && as mul on 0/1 Bool slots, got:\n{text}"
        );
        assert!(
            !text.contains("Stub IR"),
            "must not fall back to stub:\n{text}"
        );
    }

    #[test]
    fn test_ir_generate_nested_bool_and_or() {
        // result == (a && (b || c))
        let clauses = vec![Clause {
            kind: ClauseKind::Ensures,
            body: sp(Expr::BinOp {
                op: BinOp::Eq,
                lhs: spb(Expr::Ident("result".into())),
                rhs: spb(Expr::BinOp {
                    op: BinOp::And,
                    lhs: spb(Expr::Ident("a".into())),
                    rhs: spb(Expr::BinOp {
                        op: BinOp::Or,
                        lhs: spb(Expr::Ident("b".into())),
                        rhs: spb(Expr::Ident("c".into())),
                    }),
                }),
            }),
            effect_variables: vec![],
        }];
        let text = generate_ir_sidecar_text(
            "Nest",
            &[(0, "Bool".into()), (1, "Bool".into()), (2, "Bool".into())],
            &["a".into(), "b".into(), "c".into()],
            "Bool",
            &clauses,
        );
        assert!(
            text.contains("arith mul") && text.contains("arith add") && text.contains("cmp ne"),
            "expected nested &&/|| via mul + add/ne, got:\n{text}"
        );
        assert!(
            !text.contains("Stub IR"),
            "must not stub nested bool:\n{text}"
        );
    }

    #[test]
    fn test_ir_generate_if_with_and_condition() {
        // result == if x > 0 && y > 0 then 1 else 0
        let clauses = vec![Clause {
            kind: ClauseKind::Ensures,
            body: sp(Expr::BinOp {
                op: BinOp::Eq,
                lhs: spb(Expr::Ident("result".into())),
                rhs: spb(Expr::If {
                    cond: spb(Expr::BinOp {
                        op: BinOp::And,
                        lhs: spb(Expr::BinOp {
                            op: BinOp::Gt,
                            lhs: spb(Expr::Ident("x".into())),
                            rhs: spb(Expr::Literal(Literal::Int("0".into()))),
                        }),
                        rhs: spb(Expr::BinOp {
                            op: BinOp::Gt,
                            lhs: spb(Expr::Ident("y".into())),
                            rhs: spb(Expr::Literal(Literal::Int("0".into()))),
                        }),
                    }),
                    then_branch: spb(Expr::Literal(Literal::Int("1".into()))),
                    else_branch: Some(spb(Expr::Literal(Literal::Int("0".into())))),
                }),
            }),
            effect_variables: vec![],
        }];
        let text = generate_ir_sidecar_text(
            "IfAnd",
            &[int_param("x", 0), int_param("y", 1)],
            &["x".into(), "y".into()],
            "Int",
            &clauses,
        );
        assert!(
            text.contains("arith mul") && text.contains("then #1 else #2"),
            "expected && condition as mul + if, got:\n{text}"
        );
        assert!(
            !text.contains("Stub IR"),
            "must not fall back to stub:\n{text}"
        );
    }

    fn double_callee_x_plus_x() -> HashMap<String, CalleeSpec> {
        let mut m = HashMap::new();
        m.insert(
            "double".into(),
            CalleeSpec {
                param_names: vec!["x".into()],
                return_ty: "Int".into(),
                clauses: vec![Clause {
                    kind: ClauseKind::Ensures,
                    body: sp(Expr::BinOp {
                        op: BinOp::Eq,
                        lhs: spb(Expr::Ident("result".into())),
                        rhs: spb(Expr::BinOp {
                            op: BinOp::Add,
                            lhs: spb(Expr::Ident("x".into())),
                            rhs: spb(Expr::Ident("x".into())),
                        }),
                    }),
                    effect_variables: vec![],
                }],
            },
        );
        m
    }

    fn caller_result_eq_double_x() -> Vec<Clause> {
        vec![Clause {
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
        }]
    }

    #[test]
    fn test_ir_generate_multi_fn_call_chain_with_callee_arith() {
        // #863: known unary pure callee with result == x + x → non-identity sibling.
        let callees = double_callee_x_plus_x();
        let text = generate_ir_sidecar_text_with_callees(
            "UseDouble",
            &[int_param("x", 0)],
            &["x".into()],
            "Int",
            &caller_result_eq_double_x(),
            &callees,
        );
        assert!(
            text.contains("call double ($0)"),
            "expected call to double, got:\n{text}"
        );
        assert!(
            text.contains("fn #1"),
            "expected sibling fn #1, got:\n{text}"
        );
        assert!(
            text.contains("arith add $0 $0"),
            "sibling must implement double (x+x), not identity; got:\n{text}"
        );
        // The sibling body must not be a lone identity load of $0 as its only result.
        let after_fn1 = text.split("fn #1").nth(1).unwrap_or("");
        assert!(
            !after_fn1.contains("$result = load $0 : Int") || after_fn1.contains("arith add"),
            "must not be identity-only sibling; got:\n{text}"
        );
    }

    #[test]
    fn test_ir_generate_call_chain_unknown_callee_no_identity_sibling() {
        // #863: without callee specs, do not emit a silent identity sibling plan.
        let text = generate_ir_sidecar_text(
            "UseDouble",
            &[int_param("x", 0)],
            &["x".into()],
            "Int",
            &caller_result_eq_double_x(),
        );
        assert!(
            text.contains("Stub IR") || !text.contains("call double"),
            "unknown callee must not silently identity-verify via call plan; got:\n{text}"
        );
        if text.contains("fn #1") {
            // If a multi-fn plan appears, sibling must not be the only content as identity
            // for an unanalyzable double — prefer full stub.
            panic!("unexpected call plan without callees:\n{text}");
        }
    }

    #[test]
    fn test_ir_generate_multi_arg_callee_arith() {
        // Multi-arg pure helper with result == a + b → non-identity sibling.
        let clauses = vec![Clause {
            kind: ClauseKind::Ensures,
            body: sp(Expr::BinOp {
                op: BinOp::Eq,
                lhs: spb(Expr::Ident("result".into())),
                rhs: spb(Expr::Call {
                    func: spb(Expr::Ident("helper".into())),
                    args: vec![sp(Expr::Ident("a".into())), sp(Expr::Ident("b".into()))],
                }),
            }),
            effect_variables: vec![],
        }];
        let mut callees = HashMap::new();
        callees.insert(
            "helper".into(),
            CalleeSpec {
                param_names: vec!["a".into(), "b".into()],
                return_ty: "Int".into(),
                clauses: vec![Clause {
                    kind: ClauseKind::Ensures,
                    body: sp(Expr::BinOp {
                        op: BinOp::Eq,
                        lhs: spb(Expr::Ident("result".into())),
                        rhs: spb(Expr::BinOp {
                            op: BinOp::Add,
                            lhs: spb(Expr::Ident("a".into())),
                            rhs: spb(Expr::Ident("b".into())),
                        }),
                    }),
                    effect_variables: vec![],
                }],
            },
        );
        let text = generate_ir_sidecar_text_with_callees(
            "Caller",
            &[int_param("a", 0), (1, "Int".into())],
            &["a".into(), "b".into()],
            "Int",
            &clauses,
            &callees,
        );
        assert!(
            text.contains("call helper ($0, $1)"),
            "expected multi-arg call, got:\n{text}"
        );
        assert!(
            text.contains("arith add $0 $1") || text.contains("arith add $0 $0"),
            "sibling must implement a+b, not identity; got:\n{text}"
        );
        assert!(
            text.contains("$0: Int, $1: Int") || text.contains("$0: Int,$1: Int"),
            "sibling signature should list two params; got:\n{text}"
        );
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
