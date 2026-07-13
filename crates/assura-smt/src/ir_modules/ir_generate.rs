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
    /// Param name → declared type string (e.g. `Point`) for field index lookup.
    pub name_to_ty: HashMap<&'a str, &'a str>,
    pub return_ty: &'a str,
    /// In-file callees keyed by declaration name (exact `call` target).
    pub callees: &'a HashMap<String, CalleeSpec>,
    /// Struct type name → ordered `(field_name, field_type_name)` from TypeEnv.
    /// Field type names enable nested paths like `o.inner.v` (#896).
    pub field_layouts: &'a HashMap<String, Vec<(String, String)>>,
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
    plan_let_ensures,
    plan_field_access_ensures,
    plan_abs_call_ensures,
    plan_min_max_call_ensures,
    plan_clamp_call_ensures,
    plan_signum_call_ensures,
    plan_bool_comparison_ensures,
    plan_bool_logic_ensures,
    plan_multi_fn_call_chain,
    plan_identity_equality,
    plan_result_bound_ensures,
    plan_length_value_ensures,
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
/// Equivalent to [`generate_ir_sidecar_text_with_callees`] with empty callee /
/// field-layout maps.
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
        &HashMap::new(),
    )
}

/// Like [`generate_ir_sidecar_text`], but synthesizes non-identity sibling
/// bodies for unary pure callees present in `callees` (#863) and field loads
/// when `field_layouts` maps struct types to ordered field names (#892).
pub fn generate_ir_sidecar_text_with_callees(
    name: &str,
    params: &[(usize, String)],
    param_names: &[String],
    return_ty: &str,
    clauses: &[Clause],
    callees: &HashMap<String, CalleeSpec>,
    field_layouts: &HashMap<String, Vec<(String, String)>>,
) -> String {
    let requires_count = clauses
        .iter()
        .filter(|c| c.kind == ClauseKind::Requires)
        .count();
    let ensures_count = clauses
        .iter()
        .filter(|c| c.kind == ClauseKind::Ensures)
        .count();

    let name_to_ty: HashMap<&str, &str> = param_names
        .iter()
        .zip(params.iter())
        .map(|(n, (_, ty))| (n.as_str(), ty.as_str()))
        .collect();

    let ctx = PlanCtx {
        name_to_slot: param_names
            .iter()
            .enumerate()
            .map(|(i, n)| (n.as_str(), i))
            .collect(),
        name_to_ty,
        return_ty,
        callees,
        field_layouts,
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

/// `ensures { result == xs.length() }` (or `.len()` / `.size()`) as IR `call length`.
///
/// Distinct from [`plan_length_copy_ensures`] which handles length *relations*
/// like `result.length() == raw.length()` (identity load of the collection).
/// SMT backends expand `call length ($slot)` via canonical length (#891 pattern).
fn plan_length_value_ensures(expr: &SpExpr, ctx: &PlanCtx<'_>) -> Option<IrGenPlan> {
    let (lhs, rhs) = equality_operands(expr)?;
    let other = if is_result_ident(lhs) {
        rhs
    } else if is_result_ident(rhs) {
        lhs
    } else {
        return None;
    };
    let Expr::MethodCall {
        receiver,
        method,
        args,
    } = &other.node
    else {
        return None;
    };
    if !matches!(method.as_str(), "length" | "len" | "size") || !args.is_empty() {
        return None;
    }
    let Expr::Ident(name) = &receiver.as_ref().node else {
        return None;
    };
    let base = *ctx.name_to_slot.get(name.as_str())?;
    let mut used: Vec<usize> = ctx.name_to_slot.values().copied().collect();
    used.sort_unstable();
    let out = next_temp_slot(&used);
    let lines = vec![
        format!("    ${out} = call length (${base}) : {}", ctx.return_ty),
        format!("    $result = load ${out} : {}", ctx.return_ty),
    ];
    Some(single_fn_plan(IrGenBody { lines }))
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

/// `ensures { result == let y = x + 1 in y * 2 }` → temps for bindings, then body.
///
/// Nested lets are flattened left-to-right into `const`/`arith` temps; the
/// binding name is temporarily mapped into the slot table for the body.
fn plan_let_ensures(expr: &SpExpr, ctx: &PlanCtx<'_>) -> Option<IrGenPlan> {
    let (lhs, rhs) = equality_operands(expr)?;
    let mut other = if is_result_ident(lhs) {
        rhs
    } else if is_result_ident(rhs) {
        lhs
    } else {
        return None;
    };
    if !matches!(&other.node, Expr::Let { .. }) {
        return None;
    }

    let mut lines: Vec<String> = Vec::new();
    let mut used: Vec<usize> = ctx.name_to_slot.values().copied().collect();
    used.sort_unstable();
    // Owned names so we can extend the map past PlanCtx's borrowed keys.
    let mut owned: HashMap<String, usize> = ctx
        .name_to_slot
        .iter()
        .map(|(k, v)| ((*k).to_string(), *v))
        .collect();

    // Flatten nested lets into temps.
    while let Expr::Let { name, value, body } = &other.node {
        let name_map: HashMap<&str, usize> = owned.iter().map(|(k, v)| (k.as_str(), *v)).collect();
        let bind_ctx = PlanCtx {
            name_to_slot: name_map,
            name_to_ty: ctx.name_to_ty.clone(),
            return_ty: ctx.return_ty,
            callees: ctx.callees,
            field_layouts: ctx.field_layouts,
        };
        let slot = operand_to_slot(value.as_ref(), &bind_ctx, &mut lines, &mut used)?;
        owned.insert(name.clone(), slot);
        other = body.as_ref();
    }

    let name_map: HashMap<&str, usize> = owned.iter().map(|(k, v)| (k.as_str(), *v)).collect();
    let body_ctx = PlanCtx {
        name_to_slot: name_map,
        name_to_ty: ctx.name_to_ty.clone(),
        return_ty: ctx.return_ty,
        callees: ctx.callees,
        field_layouts: ctx.field_layouts,
    };
    let body_plan = plan_result_equals(other, &body_ctx)?;
    lines.extend(body_plan.lines);
    Some(single_fn_plan(IrGenBody { lines }))
}

/// `ensures { result == abs(...) }` including nested `abs(min(x,y))` (#891).
///
/// Emits IR `call abs` / nested calls; SMT backends expand via
/// `try_known_builtin` (Z3/CVC5 ite).
fn plan_abs_call_ensures(expr: &SpExpr, ctx: &PlanCtx<'_>) -> Option<IrGenPlan> {
    plan_builtin_call_ensures(expr, ctx, "abs", 1)
}

/// `ensures { result == min(...) }` / `max(...)` including nested args (#891).
fn plan_min_max_call_ensures(expr: &SpExpr, ctx: &PlanCtx<'_>) -> Option<IrGenPlan> {
    let (lhs, rhs) = equality_operands(expr)?;
    let call = if is_result_ident(lhs) {
        rhs
    } else if is_result_ident(rhs) {
        lhs
    } else {
        return None;
    };
    let Expr::Call { func, .. } = &call.node else {
        return None;
    };
    let Expr::Ident(name) = &func.as_ref().node else {
        return None;
    };
    if name != "min" && name != "max" {
        return None;
    }
    plan_builtin_call_ensures(expr, ctx, name, 2)
}

/// `ensures { result == clamp(x, lo, hi) }` as nested min/max IR calls.
fn plan_clamp_call_ensures(expr: &SpExpr, ctx: &PlanCtx<'_>) -> Option<IrGenPlan> {
    plan_builtin_call_ensures(expr, ctx, "clamp", 3)
}

/// `ensures { result == signum(x) }` as nested min/max IR (clamp to [-1, 1]).
fn plan_signum_call_ensures(expr: &SpExpr, ctx: &PlanCtx<'_>) -> Option<IrGenPlan> {
    plan_builtin_call_ensures(expr, ctx, "signum", 1)
}

/// Inequality ensures on `result` (witness body, not full specification).
///
/// | Ensures shape | Synthesized body |
/// |---------------|------------------|
/// | `result >= e` / `result <= e` | `result = e` |
/// | `result > e` | `result = e + 1` |
/// | `result < e` | `result = e - 1` |
///
/// `e` must be a param, literal, or nested arith/abs/min/max/clamp/signum
/// tree (same as equality synthesis). Pure inequalities like `result > 0`
/// get a constant witness (`1` / `-1` / `0`).
fn plan_result_bound_ensures(expr: &SpExpr, ctx: &PlanCtx<'_>) -> Option<IrGenPlan> {
    let Expr::BinOp { op, lhs, rhs } = &expr.node else {
        return None;
    };
    if !op.is_ordering_comparison() {
        return None;
    }
    let (result_on_left, other) = if is_result_ident(lhs.as_ref()) {
        (true, rhs.as_ref())
    } else if is_result_ident(rhs.as_ref()) {
        (false, lhs.as_ref())
    } else {
        return None;
    };
    // Normalize to "result OP other" by flipping comparison when result is RHS.
    let op = if result_on_left {
        op.clone()
    } else {
        flip_ordering_op(op)?
    };
    let mut lines: Vec<String> = Vec::new();
    let mut used: Vec<usize> = ctx.name_to_slot.values().copied().collect();
    used.sort_unstable();
    let body = match op {
        BinOp::Gte | BinOp::Lte => {
            // Weakest equality witness: result = other.
            let slot = operand_to_slot(other, ctx, &mut lines, &mut used)?;
            lines.push(format!("    $result = load ${slot} : {}", ctx.return_ty));
            IrGenBody { lines }
        }
        BinOp::Gt => {
            // result = other + 1
            let o = operand_to_slot(other, ctx, &mut lines, &mut used)?;
            let one = next_temp_slot(&used);
            used.push(one);
            lines.push(format!("    ${one} = const 1 : {}", ctx.return_ty));
            let sum = next_temp_slot(&used);
            used.push(sum);
            lines.push(format!(
                "    ${sum} = arith add ${o} ${one} : {}",
                ctx.return_ty
            ));
            lines.push(format!("    $result = load ${sum} : {}", ctx.return_ty));
            IrGenBody { lines }
        }
        BinOp::Lt => {
            // result = other - 1
            let o = operand_to_slot(other, ctx, &mut lines, &mut used)?;
            let one = next_temp_slot(&used);
            used.push(one);
            lines.push(format!("    ${one} = const 1 : {}", ctx.return_ty));
            let diff = next_temp_slot(&used);
            used.push(diff);
            lines.push(format!(
                "    ${diff} = arith sub ${o} ${one} : {}",
                ctx.return_ty
            ));
            lines.push(format!("    $result = load ${diff} : {}", ctx.return_ty));
            IrGenBody { lines }
        }
        _ => return None,
    };
    Some(single_fn_plan(body))
}

fn flip_ordering_op(op: &BinOp) -> Option<BinOp> {
    Some(match op {
        BinOp::Lt => BinOp::Gt,
        BinOp::Lte => BinOp::Gte,
        BinOp::Gt => BinOp::Lt,
        BinOp::Gte => BinOp::Lte,
        _ => return None,
    })
}

fn plan_builtin_call_ensures(
    expr: &SpExpr,
    ctx: &PlanCtx<'_>,
    expected: &str,
    arity: usize,
) -> Option<IrGenPlan> {
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
    if name != expected || args.len() != arity {
        return None;
    }
    let mut lines: Vec<String> = Vec::new();
    let mut used: Vec<usize> = ctx.name_to_slot.values().copied().collect();
    used.sort_unstable();
    let out = operand_to_slot(call, ctx, &mut lines, &mut used)?;
    lines.push(format!("    $result = load ${out} : {}", ctx.return_ty));
    Some(single_fn_plan(IrGenBody { lines }))
}

/// `ensures { result == p.x }` via IR `field $slot .index` (#892).
fn plan_field_access_ensures(expr: &SpExpr, ctx: &PlanCtx<'_>) -> Option<IrGenPlan> {
    let (lhs, rhs) = equality_operands(expr)?;
    let other = if is_result_ident(lhs) {
        rhs
    } else if is_result_ident(rhs) {
        lhs
    } else {
        return None;
    };
    if !matches!(&other.node, Expr::Field(..)) {
        return None;
    }
    let mut lines: Vec<String> = Vec::new();
    let mut used: Vec<usize> = ctx.name_to_slot.values().copied().collect();
    used.sort_unstable();
    let out = operand_to_slot(other, ctx, &mut lines, &mut used)?;
    lines.push(format!("    $result = load ${out} : {}", ctx.return_ty));
    Some(single_fn_plan(IrGenBody { lines }))
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
    let empty_layouts = HashMap::new();
    let empty_tys = HashMap::new();
    let ctx = PlanCtx {
        name_to_slot,
        name_to_ty: empty_tys,
        return_ty: callee.return_ty.as_str(),
        // Nested call chains are out of scope for v1 sibling synthesis.
        callees: &empty,
        field_layouts: &empty_layouts,
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
/// arithmetic binops (e.g. `(x + 1) * 2`), unary negation (`-x` as `0 - x`),
/// nested `abs`/`min`/`max` calls (#891), and struct field loads (#892).
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
        Expr::Call { func, args } => {
            let Expr::Ident(name) = &func.as_ref().node else {
                return None;
            };
            match (name.as_str(), args.len()) {
                ("abs", 1) => {
                    let a = operand_to_slot(&args[0], ctx, lines, used)?;
                    let slot = next_temp_slot(used);
                    used.push(slot);
                    lines.push(format!("    ${slot} = call abs (${a}) : {}", ctx.return_ty));
                    Some(slot)
                }
                ("min" | "max", 2) => {
                    let a = operand_to_slot(&args[0], ctx, lines, used)?;
                    let b = operand_to_slot(&args[1], ctx, lines, used)?;
                    let slot = next_temp_slot(used);
                    used.push(slot);
                    lines.push(format!(
                        "    ${slot} = call {name} (${a}, ${b}) : {}",
                        ctx.return_ty
                    ));
                    Some(slot)
                }
                // clamp(x, lo, hi) ≡ min(max(x, lo), hi) — same as check-rust encode.
                ("clamp", 3) => {
                    let x = operand_to_slot(&args[0], ctx, lines, used)?;
                    let lo = operand_to_slot(&args[1], ctx, lines, used)?;
                    let hi = operand_to_slot(&args[2], ctx, lines, used)?;
                    let t = next_temp_slot(used);
                    used.push(t);
                    lines.push(format!(
                        "    ${t} = call max (${x}, ${lo}) : {}",
                        ctx.return_ty
                    ));
                    let slot = next_temp_slot(used);
                    used.push(slot);
                    lines.push(format!(
                        "    ${slot} = call min (${t}, ${hi}) : {}",
                        ctx.return_ty
                    ));
                    Some(slot)
                }
                // signum(x) ≡ max(min(x, 1), -1).
                ("signum", 1) => {
                    let x = operand_to_slot(&args[0], ctx, lines, used)?;
                    let one = next_temp_slot(used);
                    used.push(one);
                    lines.push(format!("    ${one} = const 1 : {}", ctx.return_ty));
                    let neg1 = next_temp_slot(used);
                    used.push(neg1);
                    lines.push(format!("    ${neg1} = const -1 : {}", ctx.return_ty));
                    let t = next_temp_slot(used);
                    used.push(t);
                    lines.push(format!(
                        "    ${t} = call min (${x}, ${one}) : {}",
                        ctx.return_ty
                    ));
                    let slot = next_temp_slot(used);
                    used.push(slot);
                    lines.push(format!(
                        "    ${slot} = call max (${t}, ${neg1}) : {}",
                        ctx.return_ty
                    ));
                    Some(slot)
                }
                _ => None,
            }
        }
        Expr::Field(recv, field) => {
            let base = operand_to_slot(recv.as_ref(), ctx, lines, used)?;
            // Resolve field on the receiver's type (supports nested `o.inner.v`).
            let field_ty = field_type_for_receiver(recv.as_ref(), field, ctx)?;
            let slot = next_temp_slot(used);
            used.push(slot);
            // Intermediate loads use the field's type; the outer ensures load
            // will copy into `$result` with the contract return type.
            lines.push(format!(
                "    ${slot} = field ${base} .{} : {}",
                field, field_ty
            ));
            Some(slot)
        }
        _ => None,
    }
}

/// Type name of `recv` for field layout lookup (params or nested field loads).
fn type_name_of_receiver(recv: &SpExpr, ctx: &PlanCtx<'_>) -> Option<String> {
    match &recv.node {
        Expr::Ident(recv_name) => ctx
            .name_to_ty
            .get(recv_name.as_str())
            .map(|s| (*s).to_string()),
        Expr::Field(inner, field) => {
            // Type of `inner.field` is the field's declared type.
            field_type_for_receiver(inner.as_ref(), field, ctx)
        }
        _ => None,
    }
}

/// Field type name for `recv.field` using struct layouts (#892 / #896)
/// or simple tuple types `(Int, Bool)` with numeric fields (#899).
fn field_type_for_receiver(recv: &SpExpr, field: &str, ctx: &PlanCtx<'_>) -> Option<String> {
    let recv_ty = type_name_of_receiver(recv, ctx)?;
    if let Some(fields) = ctx.field_layouts.get(&recv_ty) {
        return fields
            .iter()
            .find(|(n, _)| n == field)
            .map(|(_, ty)| ty.clone());
    }
    // Tuple projections: param type string like `(Int, Bool)`.
    if let Some(elems) = parse_simple_tuple_type_elems(&recv_ty) {
        let idx: usize = field.parse().ok()?;
        return elems.get(idx).cloned();
    }
    None
}

/// Parse a display tuple type `(A, B, C)` into element type strings.
///
/// Handles nested parentheses/generics at top-level commas only.
fn parse_simple_tuple_type_elems(s: &str) -> Option<Vec<String>> {
    let s = s.trim();
    if !s.starts_with('(') || !s.ends_with(')') || s == "()" {
        return None;
    }
    let inner = s[1..s.len() - 1].trim();
    if inner.is_empty() {
        return None;
    }
    let mut elems = Vec::new();
    let mut depth: i32 = 0;
    let mut start = 0usize;
    for (i, c) in inner.char_indices() {
        match c {
            '(' | '<' | '{' => depth += 1,
            ')' | '>' | '}' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                let part = inner[start..i].trim();
                if part.is_empty() {
                    return None;
                }
                elems.push(part.to_string());
                start = i + c.len_utf8();
            }
            _ => {}
        }
    }
    let last = inner[start..].trim();
    if last.is_empty() {
        return None;
    }
    elems.push(last.to_string());
    // Single parenthesized type `(Int)` is not a 1-tuple in Assura (that is just Int
    // grouping); require at least two elements for tuple projection synthesis.
    if elems.len() < 2 {
        return None;
    }
    Some(elems)
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
    fn generates_call_length_when_result_eq_param_length() {
        let clauses = vec![Clause {
            kind: ClauseKind::Ensures,
            body: sp(Expr::BinOp {
                op: BinOp::Eq,
                lhs: spb(Expr::Ident("result".into())),
                rhs: spb(Expr::MethodCall {
                    receiver: spb(Expr::Ident("xs".into())),
                    method: "length".into(),
                    args: vec![],
                }),
            }),
            effect_variables: vec![],
        }];
        let text = generate_ir_sidecar_text(
            "LenOf",
            &[(0, "List<Int>".into())],
            &["xs".into()],
            "Nat",
            &clauses,
        );
        assert!(
            text.contains("call length ($0)") && text.contains("$result = load"),
            "expected call length IR, got:\n{text}"
        );
        assert!(
            !text.contains("Stub IR"),
            "must not stub length value:\n{text}"
        );
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
    fn test_ir_generate_nested_abs_min() {
        // result == abs(min(x, y))
        let clauses = vec![Clause {
            kind: ClauseKind::Ensures,
            body: sp(Expr::BinOp {
                op: BinOp::Eq,
                lhs: spb(Expr::Ident("result".into())),
                rhs: spb(Expr::Call {
                    func: spb(Expr::Ident("abs".into())),
                    args: vec![sp(Expr::Call {
                        func: spb(Expr::Ident("min".into())),
                        args: vec![sp(Expr::Ident("x".into())), sp(Expr::Ident("y".into()))],
                    })],
                }),
            }),
            effect_variables: vec![],
        }];
        let text = generate_ir_sidecar_text(
            "AbsMin",
            &[int_param("x", 0), int_param("y", 1)],
            &["x".into(), "y".into()],
            "Int",
            &clauses,
        );
        assert!(
            text.contains("call min") && text.contains("call abs"),
            "expected nested call min then abs, got:\n{text}"
        );
        assert!(!text.contains("Stub IR"), "must not stub abs(min):\n{text}");
    }

    #[test]
    fn test_ir_generate_field_access() {
        let clauses = vec![Clause {
            kind: ClauseKind::Ensures,
            body: sp(Expr::BinOp {
                op: BinOp::Eq,
                lhs: spb(Expr::Ident("result".into())),
                rhs: spb(Expr::Field(spb(Expr::Ident("p".into())), "x".into())),
            }),
            effect_variables: vec![],
        }];
        let mut layouts = HashMap::new();
        layouts.insert(
            "Point".into(),
            vec![("x".into(), "Int".into()), ("y".into(), "Int".into())],
        );
        let text = generate_ir_sidecar_text_with_callees(
            "GetX",
            &[(0, "Point".into())],
            &["p".into()],
            "Int",
            &clauses,
            &HashMap::new(),
            &layouts,
        );
        assert!(
            text.contains("field $0 .x"),
            "expected named field load .x, got:\n{text}"
        );
        assert!(!text.contains("Stub IR"), "must not stub field:\n{text}");
    }

    #[test]
    fn test_ir_generate_tuple_field_access() {
        // result == t.0
        let clauses = vec![Clause {
            kind: ClauseKind::Ensures,
            body: sp(Expr::BinOp {
                op: BinOp::Eq,
                lhs: spb(Expr::Ident("result".into())),
                rhs: spb(Expr::Field(spb(Expr::Ident("t".into())), "0".into())),
            }),
            effect_variables: vec![],
        }];
        let text = generate_ir_sidecar_text_with_callees(
            "Fst",
            &[(0, "(Int, Bool)".into())],
            &["t".into()],
            "Int",
            &clauses,
            &HashMap::new(),
            &HashMap::new(),
        );
        assert!(
            text.contains("field $0 .0") && text.contains(": Int"),
            "expected tuple field IR, got:\n{text}"
        );
        assert!(
            !text.contains("Stub IR"),
            "must not stub tuple field:\n{text}"
        );
    }

    #[test]
    fn test_ir_generate_tuple_field_second_element() {
        // result == t.1 : Bool
        let clauses = vec![Clause {
            kind: ClauseKind::Ensures,
            body: sp(Expr::BinOp {
                op: BinOp::Eq,
                lhs: spb(Expr::Ident("result".into())),
                rhs: spb(Expr::Field(spb(Expr::Ident("t".into())), "1".into())),
            }),
            effect_variables: vec![],
        }];
        let text = generate_ir_sidecar_text_with_callees(
            "Snd",
            &[(0, "(Int, Bool)".into())],
            &["t".into()],
            "Bool",
            &clauses,
            &HashMap::new(),
            &HashMap::new(),
        );
        assert!(
            text.contains("field $0 .1") && text.contains(": Bool"),
            "expected second tuple element IR, got:\n{text}"
        );
        assert!(!text.contains("Stub IR"), "must not stub t.1:\n{text}");
    }

    #[test]
    fn test_ir_generate_nested_field_access() {
        // result == o.inner.v
        let clauses = vec![Clause {
            kind: ClauseKind::Ensures,
            body: sp(Expr::BinOp {
                op: BinOp::Eq,
                lhs: spb(Expr::Ident("result".into())),
                rhs: spb(Expr::Field(
                    spb(Expr::Field(spb(Expr::Ident("o".into())), "inner".into())),
                    "v".into(),
                )),
            }),
            effect_variables: vec![],
        }];
        let mut layouts = HashMap::new();
        layouts.insert("Outer".into(), vec![("inner".into(), "Inner".into())]);
        layouts.insert("Inner".into(), vec![("v".into(), "Int".into())]);
        let text = generate_ir_sidecar_text_with_callees(
            "GetDeep",
            &[(0, "Outer".into())],
            &["o".into()],
            "Int",
            &clauses,
            &HashMap::new(),
            &layouts,
        );
        assert!(
            text.contains("field $0 .inner") && text.contains("field") && text.contains(".v"),
            "expected nested field loads, got:\n{text}"
        );
        assert!(
            !text.contains("Stub IR"),
            "must not stub nested field:\n{text}"
        );
    }

    #[test]
    fn test_ir_generate_let_binding() {
        // result == let y = x + 1 in y * 2
        let clauses = vec![Clause {
            kind: ClauseKind::Ensures,
            body: sp(Expr::BinOp {
                op: BinOp::Eq,
                lhs: spb(Expr::Ident("result".into())),
                rhs: spb(Expr::Let {
                    name: "y".into(),
                    value: spb(Expr::BinOp {
                        op: BinOp::Add,
                        lhs: spb(Expr::Ident("x".into())),
                        rhs: spb(Expr::Literal(Literal::Int("1".into()))),
                    }),
                    body: spb(Expr::BinOp {
                        op: BinOp::Mul,
                        lhs: spb(Expr::Ident("y".into())),
                        rhs: spb(Expr::Literal(Literal::Int("2".into()))),
                    }),
                }),
            }),
            effect_variables: vec![],
        }];
        let text = generate_ir_sidecar_text(
            "LetBind",
            &[int_param("x", 0)],
            &["x".into()],
            "Int",
            &clauses,
        );
        assert!(
            text.contains("arith add") && text.contains("arith mul"),
            "expected let to expand into add then mul, got:\n{text}"
        );
        assert!(!text.contains("Stub IR"), "must not stub let:\n{text}");
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
            &HashMap::new(),
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
            &HashMap::new(),
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
        // Square-root style equality is not synthesizable (result on both sides of *).
        // Inequality witnesses like result > 0 are synthesized deliberately.
        let clauses = vec![Clause {
            kind: ClauseKind::Ensures,
            body: sp(Expr::BinOp {
                op: BinOp::Eq,
                lhs: spb(Expr::BinOp {
                    op: BinOp::Mul,
                    lhs: spb(Expr::Ident("result".into())),
                    rhs: spb(Expr::Ident("result".into())),
                }),
                rhs: spb(Expr::Ident("x".into())),
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
