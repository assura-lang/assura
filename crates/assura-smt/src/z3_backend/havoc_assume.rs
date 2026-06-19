//! Z3 havoc+assume encoding: structural result axioms, cross-clause
//! length inference, and IR body constraints (#267).

use super::encoder::Encoder;
use crate::cvc5_builtins::{KnownBuiltin, classify_known_builtin, pattern_hash_name};
use crate::havoc_assume::{
    RESULT_SLOT, infer_length_identity_links, ir_param_names, is_collection_return,
};
use crate::ir::{
    IrArithOp, IrCmpOp, IrExprKind, IrFunction, IrInstr, IrLiteral, IrPred, IrPredArg,
};
use crate::ir_encode::{is_collection_ir_type, is_length_ir_call, slot_type_map};
use assura_parser::ast::Clause;
use std::collections::HashMap;
use z3::ast;

struct IrEncodeCtx<'a> {
    slot_to_name: HashMap<usize, String>,
    slot_types: HashMap<usize, String>,
    blocks: Option<&'a HashMap<usize, Vec<IrInstr>>>,
}

/// Apply havoc+assume axioms before verifying ensures clauses.
pub(crate) fn apply_havoc_assume_z3(
    encoder: &mut Encoder,
    requires: &[&Clause],
    ensures: &[&Clause],
    return_ty: &[String],
    param_names: &[String],
    ir: Option<&IrFunction>,
    ir_blocks: Option<&HashMap<usize, Vec<IrInstr>>>,
) {
    apply_structural_result_axioms(encoder, return_ty);
    apply_length_identity_axioms(encoder, requires, ensures);
    if let Some(func) = ir {
        apply_ir_body_constraints(encoder, func, param_names, ir_blocks);
    }
}

fn apply_structural_result_axioms(encoder: &mut Encoder, return_ty: &[String]) {
    if !is_collection_return(return_ty) {
        return;
    }
    let len = encoder.canonical_length("result");
    let zero = ast::Int::from_i64(0);
    encoder.background_axioms.push(len.ge(&zero));
}

fn apply_length_identity_axioms(encoder: &mut Encoder, requires: &[&Clause], ensures: &[&Clause]) {
    for (result, input) in infer_length_identity_links(requires, ensures) {
        let len_result = encoder.canonical_length(&result);
        let len_input = encoder.canonical_length(&input);
        encoder.background_axioms.push(len_result.le(&len_input));
    }
}

fn apply_ir_body_constraints(
    encoder: &mut Encoder,
    func: &IrFunction,
    contract_param_names: &[String],
    ir_blocks: Option<&HashMap<usize, Vec<IrInstr>>>,
) {
    let mut slots: HashMap<usize, ast::Int> = HashMap::new();

    for (slot, name) in ir_param_names(func, contract_param_names) {
        let v = encoder.get_or_create_int(&name);
        slots.insert(slot, v);
    }

    let result = encoder.get_or_create_int("result");
    slots.insert(RESULT_SLOT, result);

    let slot_to_name: HashMap<usize, String> = ir_param_names(func, contract_param_names)
        .into_iter()
        .collect();
    let ctx = IrEncodeCtx {
        slot_to_name,
        slot_types: slot_type_map(func),
        blocks: ir_blocks,
    };

    for instr in &func.body {
        if instr.target != RESULT_SLOT && !slots.contains_key(&instr.target) {
            let name = format!("__ir_slot_{}", instr.target);
            let v = encoder.get_or_create_int(&name);
            slots.insert(instr.target, v);
        }
        let computed = encode_ir_expr_z3(encoder, &instr.expr, &slots, &ctx);
        if let Some(target) = slots.get(&instr.target) {
            encoder.background_axioms.push(computed.eq(target));
        }
        if instr.target == RESULT_SLOT
            && let IrExprKind::Load(src) = &instr.expr
            && let Some(param) = ctx.slot_to_name.get(src)
        {
            let len_result = encoder.canonical_length("result");
            let len_param = encoder.canonical_length(param);
            encoder.background_axioms.push(len_result.eq(&len_param));
        }
        if instr.target == RESULT_SLOT
            && let IrExprKind::Construct { type_id, .. } = &instr.expr
        {
            let tag = pattern_hash_name(type_id);
            let tag_val = encoder.get_or_create_int(&format!("__ir_tag_{type_id}"));
            encoder
                .background_axioms
                .push(tag_val.eq(ast::Int::from_i64(tag)));
        }
    }

    if let Some(post) = &func.post
        && let Some(pred) = encode_ir_pred_z3(encoder, post, &slots)
    {
        encoder.background_axioms.push(pred);
    }
}

fn eval_ir_block_z3(
    encoder: &mut Encoder,
    block_id: usize,
    slots: &HashMap<usize, ast::Int>,
    ctx: &IrEncodeCtx<'_>,
) -> Option<ast::Int> {
    let body = ctx.blocks?.get(&block_id)?;
    let mut local = slots.clone();
    let mut last: Option<ast::Int> = None;
    for instr in body {
        if instr.target != RESULT_SLOT && !local.contains_key(&instr.target) {
            let name = format!("__ir_block{block_id}_slot_{}", instr.target);
            local.insert(instr.target, encoder.get_or_create_int(&name));
        }
        let computed = encode_ir_expr_z3(encoder, &instr.expr, &local, ctx);
        if let Some(target) = local.get(&instr.target) {
            encoder.background_axioms.push(computed.eq(target));
        }
        last = local.get(&instr.target).cloned();
    }
    last
}

fn encode_ir_expr_z3(
    encoder: &mut Encoder,
    expr: &IrExprKind,
    slots: &HashMap<usize, ast::Int>,
    ctx: &IrEncodeCtx<'_>,
) -> ast::Int {
    match expr {
        IrExprKind::Const(IrLiteral::Int(n)) => ast::Int::from_i64(*n),
        IrExprKind::Const(IrLiteral::Float(f)) => ast::Int::from_i64(*f as i64),
        IrExprKind::Const(IrLiteral::Bool(b)) => ast::Int::from_i64(if *b { 1 } else { 0 }),
        IrExprKind::Const(IrLiteral::Str(_)) => encoder.fresh_int(),
        IrExprKind::Load(slot) => slots
            .get(slot)
            .cloned()
            .unwrap_or_else(|| encoder.fresh_int()),
        IrExprKind::Arith { op, lhs, rhs } => {
            let l = encode_ir_expr_z3(encoder, &IrExprKind::Load(*lhs), slots, ctx);
            let r = encode_ir_expr_z3(encoder, &IrExprKind::Load(*rhs), slots, ctx);
            match op {
                IrArithOp::Add => ast::Int::add(&[&l, &r]),
                IrArithOp::Sub => ast::Int::sub(&[&l, &r]),
                IrArithOp::Mul => ast::Int::mul(&[&l, &r]),
                IrArithOp::Div => l.div(&r),
                IrArithOp::Mod => l.modulo(&r),
            }
        }
        IrExprKind::Cmp { op, lhs, rhs } => {
            let l = encode_ir_expr_z3(encoder, &IrExprKind::Load(*lhs), slots, ctx);
            let r = encode_ir_expr_z3(encoder, &IrExprKind::Load(*rhs), slots, ctx);
            let b = match op {
                IrCmpOp::Eq => l.eq(&r),
                IrCmpOp::Ne => l.eq(&r).not(),
                IrCmpOp::Lt => l.lt(&r),
                IrCmpOp::Le => l.le(&r),
                IrCmpOp::Gt => l.gt(&r),
                IrCmpOp::Ge => l.ge(&r),
            };
            b.ite(&ast::Int::from_i64(1), &ast::Int::from_i64(0))
        }
        IrExprKind::Call { func, args } => {
            if is_length_ir_call(func, args.len())
                && let Some(slot) = args.first()
                && let Some(name) = ctx.slot_to_name.get(slot)
            {
                return encoder.canonical_length(name);
            }
            let arg_ints: Vec<ast::Int> = args
                .iter()
                .map(|a| encode_ir_expr_z3(encoder, &IrExprKind::Load(*a), slots, ctx))
                .collect();
            if let Some(kind) = classify_known_builtin(func, args.len()) {
                let zero = ast::Int::from_i64(0);
                return match kind {
                    KnownBuiltin::Abs => {
                        let x = &arg_ints[0];
                        let neg = ast::Int::sub(&[zero.clone(), x.clone()]);
                        x.ge(&zero).ite(x, &neg)
                    }
                    KnownBuiltin::Min => {
                        let (a, b) = (&arg_ints[0], &arg_ints[1]);
                        a.le(b).ite(a, b)
                    }
                    KnownBuiltin::Max => {
                        let (a, b) = (&arg_ints[0], &arg_ints[1]);
                        a.ge(b).ite(a, b)
                    }
                    _ => encode_ir_call_uf(encoder, func, &arg_ints),
                };
            }
            encode_ir_call_uf(encoder, func, &arg_ints)
        }
        IrExprKind::Field { slot, index } => {
            if *index == 0
                && let Some(ty) = ctx.slot_types.get(slot)
                && is_collection_ir_type(ty)
                && let Some(name) = ctx.slot_to_name.get(slot)
            {
                return encoder.canonical_length(name);
            }
            let base = encode_ir_expr_z3(encoder, &IrExprKind::Load(*slot), slots, ctx);
            let ty_suffix = ctx
                .slot_types
                .get(slot)
                .map(|t| t.replace('<', "_").replace('>', ""))
                .unwrap_or_else(|| "val".into());
            let decl = encoder.make_func(&format!("__ir_field_{ty_suffix}_{index}"), 1);
            decl.apply(&[&base as &dyn z3::ast::Ast])
                .as_int()
                .unwrap_or_else(|| encoder.fresh_int())
        }
        IrExprKind::Construct { type_id, fields } => {
            let arg_ints: Vec<ast::Int> = fields
                .iter()
                .map(|(_, s)| encode_ir_expr_z3(encoder, &IrExprKind::Load(*s), slots, ctx))
                .collect();
            let decl = encoder.make_func(&format!("__ir_construct_{type_id}"), arg_ints.len());
            let ast_args: Vec<&dyn z3::ast::Ast> =
                arg_ints.iter().map(|i| i as &dyn z3::ast::Ast).collect();
            decl.apply(&ast_args)
                .as_int()
                .unwrap_or_else(|| encoder.fresh_int())
        }
        IrExprKind::Cast { slot, .. } => {
            encode_ir_expr_z3(encoder, &IrExprKind::Load(*slot), slots, ctx)
        }
        IrExprKind::Transition { slot, state } => {
            let val = encode_ir_expr_z3(encoder, &IrExprKind::Load(*slot), slots, ctx);
            let decl = encoder.make_func(&format!("__ir_state_{state}"), 1);
            decl.apply(&[&val as &dyn z3::ast::Ast])
                .as_int()
                .unwrap_or(val)
        }
        IrExprKind::If {
            cond,
            then_block,
            else_block,
        } => {
            let cond_val = encode_ir_expr_z3(encoder, &IrExprKind::Load(*cond), slots, ctx);
            let cond_bool = cond_val.eq(ast::Int::from_i64(0)).not();
            let then_val =
                eval_ir_block_z3(encoder, *then_block, slots, ctx).unwrap_or_else(|| {
                    let then_decl = encoder.make_func(&format!("__ir_block_{then_block}"), 0);
                    then_decl
                        .apply(&[])
                        .as_int()
                        .unwrap_or_else(|| encoder.fresh_int())
                });
            let else_val =
                eval_ir_block_z3(encoder, *else_block, slots, ctx).unwrap_or_else(|| {
                    let else_decl = encoder.make_func(&format!("__ir_block_{else_block}"), 0);
                    else_decl
                        .apply(&[])
                        .as_int()
                        .unwrap_or_else(|| encoder.fresh_int())
                });
            cond_bool.ite(&then_val, &else_val)
        }
    }
}

fn encode_ir_call_uf(encoder: &mut Encoder, func: &str, arg_ints: &[ast::Int]) -> ast::Int {
    let decl = encoder.make_func(&format!("__ir_call_{func}"), arg_ints.len());
    let ast_args: Vec<&dyn z3::ast::Ast> =
        arg_ints.iter().map(|i| i as &dyn z3::ast::Ast).collect();
    decl.apply(&ast_args)
        .as_int()
        .unwrap_or_else(|| encoder.fresh_int())
}

fn encode_ir_pred_z3(
    encoder: &mut Encoder,
    pred: &IrPred,
    slots: &HashMap<usize, ast::Int>,
) -> Option<ast::Bool> {
    match pred {
        IrPred::True => Some(ast::Bool::from_bool(true)),
        IrPred::False => Some(ast::Bool::from_bool(false)),
        IrPred::Cmp { op, lhs, rhs } => {
            let l = encode_ir_pred_arg(encoder, lhs, slots);
            let r = encode_ir_pred_arg(encoder, rhs, slots);
            Some(match op {
                IrCmpOp::Eq => l.eq(&r),
                IrCmpOp::Ne => l.eq(&r).not(),
                IrCmpOp::Lt => l.lt(&r),
                IrCmpOp::Le => l.le(&r),
                IrCmpOp::Gt => l.gt(&r),
                IrCmpOp::Ge => l.ge(&r),
            })
        }
        IrPred::And(a, b) => {
            let la = encode_ir_pred_z3(encoder, a, slots)?;
            let lb = encode_ir_pred_z3(encoder, b, slots)?;
            Some(la & lb)
        }
        IrPred::Or(a, b) => {
            let la = encode_ir_pred_z3(encoder, a, slots)?;
            let lb = encode_ir_pred_z3(encoder, b, slots)?;
            Some(la | lb)
        }
        IrPred::Not(inner) => encode_ir_pred_z3(encoder, inner, slots).map(|p| p.not()),
    }
}

fn encode_ir_pred_arg(
    encoder: &mut Encoder,
    arg: &IrPredArg,
    slots: &HashMap<usize, ast::Int>,
) -> ast::Int {
    match arg {
        IrPredArg::Slot(n) => slots.get(n).cloned().unwrap_or_else(|| encoder.fresh_int()),
        IrPredArg::SlotResult => slots
            .get(&RESULT_SLOT)
            .cloned()
            .unwrap_or_else(|| encoder.get_or_create_int("result")),
        IrPredArg::Lit(IrLiteral::Int(n)) => ast::Int::from_i64(*n),
        IrPredArg::Lit(IrLiteral::Float(f)) => ast::Int::from_i64(*f as i64),
        IrPredArg::Lit(IrLiteral::Bool(b)) => ast::Int::from_i64(if *b { 1 } else { 0 }),
        IrPredArg::Lit(IrLiteral::Str(_)) => encoder.fresh_int(),
        IrPredArg::Arith { op, lhs, rhs } => {
            let l = encode_ir_pred_arg(encoder, lhs, slots);
            let r = encode_ir_pred_arg(encoder, rhs, slots);
            match op {
                IrArithOp::Add => ast::Int::add(&[&l, &r]),
                IrArithOp::Sub => ast::Int::sub(&[&l, &r]),
                IrArithOp::Mul => ast::Int::mul(&[&l, &r]),
                IrArithOp::Div => l.div(&r),
                IrArithOp::Mod => l.modulo(&r),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use assura_parser::ast::{BinOp, ClauseKind, Expr, Literal};

    #[test]
    fn test_z3_havoc_assume_encoding() {
        z3::with_z3_config(&z3::Config::new(), || {
            let mut encoder = Encoder::new();
            let requires = vec![Clause {
                kind: ClauseKind::Requires,
                body: Expr::BinOp {
                    lhs: Box::new(Expr::MethodCall {
                        receiver: Box::new(Expr::Ident("raw".into())),
                        method: "length".into(),
                        args: vec![],
                    }),
                    op: BinOp::Lte,
                    rhs: Box::new(Expr::Literal(Literal::Int("100".into()))),
                },
                effect_variables: vec![],
            }];
            let ensures = vec![Clause {
                kind: ClauseKind::Ensures,
                body: Expr::BinOp {
                    lhs: Box::new(Expr::MethodCall {
                        receiver: Box::new(Expr::Ident("result".into())),
                        method: "length".into(),
                        args: vec![],
                    }),
                    op: BinOp::Lte,
                    rhs: Box::new(Expr::Literal(Literal::Int("100".into()))),
                },
                effect_variables: vec![],
            }];
            apply_havoc_assume_z3(
                &mut encoder,
                &requires.iter().collect::<Vec<_>>(),
                &ensures.iter().collect::<Vec<_>>(),
                &["Bytes".into()],
                &["raw".into()],
                None,
                None,
            );
            assert!(
                !encoder.background_axioms.is_empty(),
                "havoc+assume should emit background axioms"
            );
        });
    }

    #[test]
    fn test_z3_ir_call_uses_uninterpreted_function() {
        use crate::ir::{IrFunction, parse_ir_module};

        z3::with_z3_config(&z3::Config::new(), || {
            let ir: IrFunction = parse_ir_module(
                r#"
module test {
  fn #0 : ($0: Int) -> Bool ! pure
  {
    $1 = const 42 : Int
    $2 = call is_valid ($0, $1) : Bool
    $result = load $2 : Bool
  }
}
"#,
            )
            .unwrap()
            .functions[0]
                .clone();

            let mut encoder = Encoder::new();
            apply_havoc_assume_z3(
                &mut encoder,
                &[],
                &[],
                &["Bool".into()],
                &["x".into()],
                Some(&ir),
                None,
            );
            assert!(
                !encoder.background_axioms.is_empty(),
                "IR call body should emit axioms"
            );
        });
    }

    #[test]
    fn test_z3_ir_length_call_uses_canonical_length() {
        use crate::ir::{IrFunction, parse_ir_module};

        z3::with_z3_config(&z3::Config::new(), || {
            let ir: IrFunction = parse_ir_module(
                r#"
module test {
  fn #0 : ($0: Bytes) -> Nat ! pure
  {
    $1 = call length ($0) : Nat
    $result = load $1 : Nat
  }
}
"#,
            )
            .unwrap()
            .functions[0]
                .clone();

            let mut encoder = Encoder::new();
            apply_havoc_assume_z3(
                &mut encoder,
                &[],
                &[],
                &["Nat".into()],
                &["raw".into()],
                Some(&ir),
                None,
            );
            assert!(
                !encoder.background_axioms.is_empty(),
                "length call should tie result to raw.length()"
            );
        });
    }
}
