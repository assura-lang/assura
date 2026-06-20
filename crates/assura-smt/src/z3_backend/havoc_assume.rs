//! Z3 havoc+assume encoding: structural result axioms, cross-clause
//! length inference, and IR body constraints (#267).

use super::encoder::Encoder;
use crate::cvc5_builtins::{KnownBuiltin, classify_known_builtin, pattern_hash_name};
use crate::havoc_assume::{
    HavocAssumeInput, RESULT_SLOT, infer_length_identity_links, ir_param_names,
    is_collection_return,
};
use crate::ir::{IrArithOp, IrCmpOp, IrExprKind, IrFunction, IrLiteral, IrPred, IrPredArg};
use crate::ir_encode::{IrEncodeContext, is_collection_ir_type, is_length_ir_call, slot_type_map};
use crate::ir_type_ctx::base_type_name;
use assura_parser::ast::Clause;
use std::collections::HashMap;
use z3::ast;

struct IrEncodeCtx<'a> {
    slot_to_name: HashMap<usize, String>,
    slot_types: HashMap<usize, String>,
    enc: IrEncodeContext<'a>,
}

/// Apply havoc+assume axioms before verifying ensures clauses.
pub(crate) fn apply_havoc_assume_z3(encoder: &mut Encoder, input: &HavocAssumeInput<'_>) {
    apply_structural_result_axioms(encoder, input.return_ty);
    apply_length_identity_axioms(encoder, input.requires, input.ensures);
    if let Some(func) = input.ir {
        apply_ir_body_constraints(encoder, func, input.param_names, input.enc_ctx);
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
    enc_ctx: IrEncodeContext<'_>,
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
        enc: enc_ctx,
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

fn eval_ir_call_z3(
    encoder: &mut Encoder,
    func: &str,
    args: &[usize],
    slots: &HashMap<usize, ast::Int>,
    ctx: &IrEncodeCtx<'_>,
) -> Option<ast::Int> {
    let callee = ctx.enc.callee_ir(func)?;
    if callee.params.len() != args.len() {
        return None;
    }

    let prefix = format!("__ir_call_{func}_");
    let mut local: HashMap<usize, ast::Int> = HashMap::new();

    for (i, param) in callee.params.iter().enumerate() {
        let arg_val = encode_ir_expr_z3(encoder, &IrExprKind::Load(args[i]), slots, ctx);
        let name = format!("{prefix}param_{}", param.slot);
        let slot_var = encoder.get_or_create_int(&name);
        encoder.background_axioms.push(arg_val.eq(&slot_var));
        local.insert(param.slot, slot_var);
    }

    let result_name = format!("{prefix}result");
    let result_var = encoder.get_or_create_int(&result_name);
    local.insert(RESULT_SLOT, result_var);

    let callee_slot_types = slot_type_map(callee);
    let callee_names: HashMap<usize, String> = callee
        .params
        .iter()
        .map(|p| (p.slot, format!("{prefix}param_{}", p.slot)))
        .collect();
    let callee_ctx = IrEncodeCtx {
        slot_to_name: callee_names,
        slot_types: callee_slot_types,
        enc: ctx.enc,
    };

    for instr in &callee.body {
        if instr.target != RESULT_SLOT && !local.contains_key(&instr.target) {
            let name = format!("{prefix}slot_{}", instr.target);
            let v = encoder.get_or_create_int(&name);
            local.insert(instr.target, v);
        }
        let computed = encode_ir_expr_z3(encoder, &instr.expr, &local, &callee_ctx);
        if let Some(target) = local.get(&instr.target) {
            encoder.background_axioms.push(computed.eq(target));
        }
    }

    local.get(&RESULT_SLOT).cloned()
}

fn eval_ir_block_z3(
    encoder: &mut Encoder,
    block_id: usize,
    slots: &HashMap<usize, ast::Int>,
    ctx: &IrEncodeCtx<'_>,
) -> Option<ast::Int> {
    let body = ctx.enc.ir_blocks?.get(&block_id)?;
    let mut local = slots.clone();
    // Block-local result: do not inherit parent RESULT_SLOT or sibling branches
    // would push unconditional (= x result) and (= 0 result) into global axioms.
    let block_result = encoder.get_or_create_int(&format!("__ir_block{block_id}_result"));
    local.insert(RESULT_SLOT, block_result);
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
            if let Some(inlined) = eval_ir_call_z3(encoder, func, args, slots, ctx) {
                return inlined;
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
                    KnownBuiltin::Concat => ast::Int::add(&[&arg_ints[0], &arg_ints[1]]),
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
            if let Some(ir_ty) = ctx.slot_types.get(slot)
                && let Some(field_name) = ctx.enc.type_ctx.field_name_at(ir_ty, *index)
            {
                let type_name = base_type_name(ir_ty);
                if let Some(names) = ctx.enc.type_ctx.field_names_for(type_name) {
                    encoder.ensure_struct_adt(
                        type_name,
                        &names.into_iter().map(str::to_string).collect::<Vec<_>>(),
                    );
                    return encoder.adt_accessor(type_name, field_name, &base);
                }
            }
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
            if ctx.enc.type_ctx.has_struct_layout(type_id)
                && let Some(field_names) = ctx.enc.type_ctx.field_names_for(type_id)
            {
                encoder.ensure_struct_adt(
                    type_id,
                    &field_names
                        .into_iter()
                        .map(str::to_string)
                        .collect::<Vec<_>>(),
                );
                let mut ordered = fields.clone();
                ordered.sort_by_key(|(idx, _)| *idx);
                let arg_ints: Vec<ast::Int> = ordered
                    .iter()
                    .map(|(_, s)| encode_ir_expr_z3(encoder, &IrExprKind::Load(*s), slots, ctx))
                    .collect();
                return encoder.adt_constructor(type_id, type_id, &arg_ints);
            }
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
    use crate::ir::IrInstr;
    use assura_parser::ast::{BinOp, ClauseKind, Expr, Literal};
    use assura_types::TypeEnv;
    use std::collections::HashMap;

    fn havoc_input<'a>(
        requires: &'a [&'a Clause],
        ensures: &'a [&'a Clause],
        return_ty: &'a [String],
        param_names: &'a [String],
        ir: Option<&'a IrFunction>,
        ir_blocks: Option<&'a HashMap<usize, Vec<IrInstr>>>,
        ir_bodies: Option<&'a HashMap<String, IrFunction>>,
        type_env: Option<&'a TypeEnv>,
    ) -> HavocAssumeInput<'a> {
        HavocAssumeInput {
            requires,
            ensures,
            return_ty,
            param_names,
            ir,
            enc_ctx: IrEncodeContext::new(type_env, ir_bodies, ir_blocks),
        }
    }

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
            let req_refs: Vec<_> = requires.iter().collect();
            let ens_refs: Vec<_> = ensures.iter().collect();
            apply_havoc_assume_z3(
                &mut encoder,
                &havoc_input(
                    &req_refs,
                    &ens_refs,
                    &["Bytes".into()],
                    &["raw".into()],
                    None,
                    None,
                    None,
                    None,
                ),
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
                &havoc_input(
                    &[],
                    &[],
                    &["Bool".into()],
                    &["x".into()],
                    Some(&ir),
                    None,
                    None,
                    None,
                ),
            );
            assert!(
                !encoder.background_axioms.is_empty(),
                "IR call body should emit axioms"
            );
        });
    }

    #[test]
    fn test_z3_ir_field_construct_uses_struct_adt_accessors() {
        use crate::ir::{IrFunction, parse_ir_module};
        use assura_types::{Type, TypeEnv};

        z3::with_z3_config(&z3::Config::new(), || {
            let ir: IrFunction = parse_ir_module(
                r#"
module test {
  fn #0 : ($0: Int, $1: Int) -> Point ! pure
  {
    $2 = construct Point { .0 = $0, .1 = $1 } : Point
    $result = load $2 : Point
  }
}
"#,
            )
            .unwrap()
            .functions[0]
                .clone();

            let mut env = TypeEnv::new();
            env.struct_fields.insert(
                "Point".into(),
                vec![("x".into(), Type::Int), ("y".into(), Type::Int)],
            );

            let mut encoder = Encoder::new();
            apply_havoc_assume_z3(
                &mut encoder,
                &havoc_input(
                    &[],
                    &[],
                    &["Point".into()],
                    &["a".into(), "b".into()],
                    Some(&ir),
                    None,
                    None,
                    Some(&env),
                ),
            );
            assert!(
                encoder.adt_defs.contains_key("Point"),
                "typed construct should register Point ADT from TypeEnv"
            );
            assert!(
                !encoder.background_axioms.is_empty(),
                "typed construct should emit accessor/tag axioms"
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
                &havoc_input(
                    &[],
                    &[],
                    &["Nat".into()],
                    &["raw".into()],
                    Some(&ir),
                    None,
                    None,
                    None,
                ),
            );
            assert!(
                !encoder.background_axioms.is_empty(),
                "length call should tie result to raw.length()"
            );
        });
    }

    #[test]
    fn test_z3_ir_call_inlines_callee_sidecar() {
        use crate::ir::{IrFunction, parse_ir_module};

        z3::with_z3_config(&z3::Config::new(), || {
            let main_ir: IrFunction = parse_ir_module(
                r#"
module main {
  fn #0 : ($0: Int) -> Int ! pure
  {
    $1 = call double ($0) : Int
    $result = load $1 : Int
  }
}
"#,
            )
            .unwrap()
            .functions[0]
                .clone();

            let helper_ir: IrFunction = parse_ir_module(
                r#"
module double {
  fn #0 : ($0: Int) -> Int ! pure
  {
    $1 = arith add $0 $0 : Int
    $result = load $1 : Int
  }
}
"#,
            )
            .unwrap()
            .functions[0]
                .clone();

            let mut bodies = HashMap::new();
            bodies.insert("double".into(), helper_ir);

            let mut encoder = Encoder::new();
            apply_havoc_assume_z3(
                &mut encoder,
                &havoc_input(
                    &[],
                    &[],
                    &["Int".into()],
                    &["x".into()],
                    Some(&main_ir),
                    None,
                    Some(&bodies),
                    None,
                ),
            );
            assert!(
                encoder
                    .background_axioms
                    .iter()
                    .any(|a| a.to_string().contains("__ir_call_double_")),
                "call double should inline callee IR with prefixed slots"
            );
        });
    }

    #[test]
    fn test_z3_ir_blocks_inlines_sibling_functions() {
        z3::with_z3_config(&z3::Config::new(), || {
            let (func, blocks) = crate::ir_encode::branch_if_else_ir_fixture();

            let mut encoder = Encoder::new();
            apply_havoc_assume_z3(
                &mut encoder,
                &havoc_input(
                    &[],
                    &[],
                    &["Int".into()],
                    &["x".into()],
                    Some(&func),
                    Some(&blocks),
                    None,
                    None,
                ),
            );

            let axiom_text: String = encoder
                .background_axioms
                .iter()
                .map(|a| a.to_string())
                .collect::<Vec<_>>()
                .join("\n");
            crate::ir_encode::assert_ir_blocks_inlined(
                &axiom_text,
                encoder.background_axioms.len(),
            );
        });
    }
}
