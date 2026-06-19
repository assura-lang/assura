//! Z3 havoc+assume encoding: structural result axioms, cross-clause
//! length inference, and IR body constraints (#267).

use super::encoder::Encoder;
use crate::havoc_assume::{
    RESULT_SLOT, infer_length_identity_links, ir_param_names, is_collection_return,
};
use crate::ir::{IrArithOp, IrCmpOp, IrExprKind, IrFunction, IrLiteral, IrPred, IrPredArg};
use assura_parser::ast::Clause;
use std::collections::HashMap;
use z3::ast;

/// Apply havoc+assume axioms before verifying ensures clauses.
pub(crate) fn apply_havoc_assume_z3(
    encoder: &mut Encoder,
    requires: &[&Clause],
    ensures: &[&Clause],
    return_ty: &[String],
    param_names: &[String],
    ir: Option<&IrFunction>,
) {
    apply_structural_result_axioms(encoder, return_ty);
    apply_length_identity_axioms(encoder, requires, ensures);
    if let Some(func) = ir {
        apply_ir_body_constraints(encoder, func, param_names);
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
) {
    let mut slots: HashMap<usize, ast::Int> = HashMap::new();

    for (slot, name) in ir_param_names(func, contract_param_names) {
        let v = encoder.get_or_create_int(&name);
        slots.insert(slot, v);
    }

    // Havoc $result, then constrain via IR instructions.
    let result = encoder.get_or_create_int("result");
    slots.insert(RESULT_SLOT, result);

    let slot_to_name: HashMap<usize, String> = ir_param_names(func, contract_param_names)
        .into_iter()
        .collect();

    for instr in &func.body {
        if instr.target != RESULT_SLOT && !slots.contains_key(&instr.target) {
            let name = format!("__ir_slot_{}", instr.target);
            let v = encoder.get_or_create_int(&name);
            slots.insert(instr.target, v);
        }
        let computed = encode_ir_expr_z3(encoder, &instr.expr, &slots);
        if let Some(target) = slots.get(&instr.target) {
            encoder.background_axioms.push(computed.eq(target));
        }
        if instr.target == RESULT_SLOT
            && let IrExprKind::Load(src) = &instr.expr
            && let Some(param) = slot_to_name.get(src)
        {
            let len_result = encoder.canonical_length("result");
            let len_param = encoder.canonical_length(param);
            encoder.background_axioms.push(len_result.eq(&len_param));
        }
    }

    if let Some(post) = &func.post
        && let Some(pred) = encode_ir_pred_z3(encoder, post, &slots)
    {
        encoder.background_axioms.push(pred);
    }
}

fn encode_ir_expr_z3(
    encoder: &mut Encoder,
    expr: &IrExprKind,
    slots: &HashMap<usize, ast::Int>,
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
            let l = encode_ir_expr_z3(encoder, &IrExprKind::Load(*lhs), slots);
            let r = encode_ir_expr_z3(encoder, &IrExprKind::Load(*rhs), slots);
            match op {
                IrArithOp::Add => ast::Int::add(&[&l, &r]),
                IrArithOp::Sub => ast::Int::sub(&[&l, &r]),
                IrArithOp::Mul => ast::Int::mul(&[&l, &r]),
                IrArithOp::Div => l.div(&r),
                IrArithOp::Mod => l.modulo(&r),
            }
        }
        IrExprKind::Cmp { op, lhs, rhs } => {
            let l = encode_ir_expr_z3(encoder, &IrExprKind::Load(*lhs), slots);
            let r = encode_ir_expr_z3(encoder, &IrExprKind::Load(*rhs), slots);
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
            let arg_ints: Vec<ast::Int> = args
                .iter()
                .map(|a| encode_ir_expr_z3(encoder, &IrExprKind::Load(*a), slots))
                .collect();
            let decl = encoder.make_func(&format!("__ir_call_{func}"), arg_ints.len());
            let ast_args: Vec<&dyn z3::ast::Ast> =
                arg_ints.iter().map(|i| i as &dyn z3::ast::Ast).collect();
            decl.apply(&ast_args)
                .as_int()
                .unwrap_or_else(|| encoder.fresh_int())
        }
        IrExprKind::Field { slot, index } => {
            let base = encode_ir_expr_z3(encoder, &IrExprKind::Load(*slot), slots);
            let decl = encoder.make_func(&format!("__ir_field_{index}"), 1);
            decl.apply(&[&base as &dyn z3::ast::Ast])
                .as_int()
                .unwrap_or_else(|| encoder.fresh_int())
        }
        IrExprKind::Construct { type_id, fields } => {
            let arg_ints: Vec<ast::Int> = fields
                .iter()
                .map(|(_, s)| encode_ir_expr_z3(encoder, &IrExprKind::Load(*s), slots))
                .collect();
            let decl = encoder.make_func(&format!("__ir_construct_{type_id}"), arg_ints.len());
            let ast_args: Vec<&dyn z3::ast::Ast> =
                arg_ints.iter().map(|i| i as &dyn z3::ast::Ast).collect();
            decl.apply(&ast_args)
                .as_int()
                .unwrap_or_else(|| encoder.fresh_int())
        }
        IrExprKind::Cast { slot, .. } | IrExprKind::Transition { slot, .. } => {
            encode_ir_expr_z3(encoder, &IrExprKind::Load(*slot), slots)
        }
        IrExprKind::If {
            cond,
            then_block,
            else_block,
        } => {
            let cond_val = encode_ir_expr_z3(encoder, &IrExprKind::Load(*cond), slots);
            let cond_bool = cond_val.eq(ast::Int::from_i64(0)).not();
            let then_decl = encoder.make_func(&format!("__ir_block_{then_block}"), 0);
            let else_decl = encoder.make_func(&format!("__ir_block_{else_block}"), 0);
            let then_val = then_decl
                .apply(&[])
                .as_int()
                .unwrap_or_else(|| encoder.fresh_int());
            let else_val = else_decl
                .apply(&[])
                .as_int()
                .unwrap_or_else(|| encoder.fresh_int());
            cond_bool.ite(&then_val, &else_val)
        }
    }
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
        IrPredArg::Lit(IrLiteral::Float(f)) => {
            let i = *f as i64;
            ast::Int::from_i64(i)
        }
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
            );
            assert!(
                !encoder.background_axioms.is_empty(),
                "IR call body should emit axioms"
            );
        });
    }
}
