//! Native CVC5 encoding for havoc-assume IR bodies.

#[cfg(feature = "cvc5-verify")]
use crate::cvc5_common::sanitize_smtlib_name;
#[cfg(feature = "cvc5-verify")]
use crate::cvc5_encoder_state::{Cvc5EncoderState, canonical_length_cvc5};
#[cfg(feature = "cvc5-verify")]
use crate::cvc5_native_builtins::encode_known_builtin_cvc5;
#[cfg(feature = "cvc5-verify")]
use crate::ir_encode::{IrEncodeContext, is_collection_ir_type, is_length_ir_call, slot_type_map};
use crate::ir_type_ctx::base_type_name;

#[cfg(feature = "cvc5-verify")]
fn mk_ir_arith_cvc5<'a>(
    tm: &'a cvc5::TermManager,
    op: crate::ir::IrArithOp,
    l: cvc5::Term<'a>,
    r: cvc5::Term<'a>,
) -> cvc5::Term<'a> {
    use crate::ir::IrArithOp;

    match op {
        IrArithOp::Add => tm.mk_term(cvc5::Kind::Add, &[l, r]),
        IrArithOp::Sub => tm.mk_term(cvc5::Kind::Sub, &[l, r]),
        IrArithOp::Mul => tm.mk_term(cvc5::Kind::Mult, &[l, r]),
        IrArithOp::Div => tm.mk_term(cvc5::Kind::IntsDivision, &[l, r]),
        IrArithOp::Mod => tm.mk_term(cvc5::Kind::IntsModulus, &[l, r]),
    }
}

#[cfg(feature = "cvc5-verify")]
fn mk_ir_cmp_bool_cvc5<'a>(
    tm: &'a cvc5::TermManager,
    op: crate::ir::IrCmpOp,
    l: cvc5::Term<'a>,
    r: cvc5::Term<'a>,
) -> cvc5::Term<'a> {
    use crate::ir::IrCmpOp;

    match op {
        IrCmpOp::Eq => tm.mk_term(cvc5::Kind::Equal, &[l, r]),
        IrCmpOp::Ne => tm.mk_term(cvc5::Kind::Not, &[tm.mk_term(cvc5::Kind::Equal, &[l, r])]),
        IrCmpOp::Lt => tm.mk_term(cvc5::Kind::Lt, &[l, r]),
        IrCmpOp::Le => tm.mk_term(cvc5::Kind::Leq, &[l, r]),
        IrCmpOp::Gt => tm.mk_term(cvc5::Kind::Gt, &[l, r]),
        IrCmpOp::Ge => tm.mk_term(cvc5::Kind::Geq, &[l, r]),
    }
}

#[cfg(feature = "cvc5-verify")]
fn mk_ir_cmp_as_int_cvc5<'a>(
    tm: &'a cvc5::TermManager,
    op: crate::ir::IrCmpOp,
    l: cvc5::Term<'a>,
    r: cvc5::Term<'a>,
) -> cvc5::Term<'a> {
    let b = mk_ir_cmp_bool_cvc5(tm, op, l, r);
    tm.mk_term(cvc5::Kind::Ite, &[b, tm.mk_integer(1), tm.mk_integer(0)])
}

#[cfg(feature = "cvc5-verify")]
#[expect(
    clippy::too_many_arguments,
    reason = "IR block eval threads encoding context"
)]
fn eval_ir_block_cvc5<'a>(
    tm: &'a cvc5::TermManager,
    block_id: usize,
    slots: &std::collections::HashMap<usize, cvc5::Term<'a>>,
    vars: &mut std::collections::HashMap<String, cvc5::Term<'a>>,
    state: &mut Cvc5EncoderState<'a>,
    slot_to_name: &std::collections::HashMap<usize, String>,
    slot_types: &std::collections::HashMap<usize, String>,
    ir_blocks: Option<&std::collections::HashMap<usize, Vec<crate::ir::IrInstr>>>,
    enc_ctx: IrEncodeContext<'a>,
) -> Option<cvc5::Term<'a>> {
    use crate::havoc_assume::RESULT_SLOT;

    let body = ir_blocks?.get(&block_id)?;
    let mut local = slots.clone();
    let mut last = None;
    for instr in body {
        if instr.target != RESULT_SLOT && !local.contains_key(&instr.target) {
            let name = format!("__ir_block{block_id}_slot_{}", instr.target);
            let key = sanitize_smtlib_name(&name);
            let v = vars
                .entry(key.clone())
                .or_insert_with(|| tm.mk_const(tm.integer_sort(), &key))
                .clone();
            local.insert(instr.target, v);
        }
        let computed = encode_ir_expr_cvc5(
            tm,
            &instr.expr,
            &local,
            vars,
            state,
            slot_to_name,
            slot_types,
            ir_blocks,
            enc_ctx,
        );
        if let Some(target) = local.get(&instr.target) {
            state
                .axioms
                .push(tm.mk_term(cvc5::Kind::Equal, &[computed, target.clone()]));
        }
        last = local.get(&instr.target).cloned();
    }
    last
}

#[cfg(feature = "cvc5-verify")]
#[expect(
    clippy::too_many_arguments,
    reason = "IR expr encoding threads encoding context"
)]
fn encode_ir_expr_cvc5<'a>(
    tm: &'a cvc5::TermManager,
    expr: &crate::ir::IrExprKind,
    slots: &std::collections::HashMap<usize, cvc5::Term<'a>>,
    vars: &mut std::collections::HashMap<String, cvc5::Term<'a>>,
    state: &mut Cvc5EncoderState<'a>,
    slot_to_name: &std::collections::HashMap<usize, String>,
    slot_types: &std::collections::HashMap<usize, String>,
    ir_blocks: Option<&std::collections::HashMap<usize, Vec<crate::ir::IrInstr>>>,
    enc_ctx: IrEncodeContext<'a>,
) -> cvc5::Term<'a> {
    use crate::ir::{IrExprKind, IrLiteral};

    match expr {
        IrExprKind::Const(IrLiteral::Int(n)) => tm.mk_integer(*n),
        IrExprKind::Const(IrLiteral::Float(f)) => tm.mk_integer(*f as i64),
        IrExprKind::Const(IrLiteral::Bool(b)) => tm.mk_integer(if *b { 1 } else { 0 }),
        IrExprKind::Const(IrLiteral::Str(_)) => {
            let name = format!("__fresh_{}", state.fresh_counter);
            state.fresh_counter += 1;
            tm.mk_const(tm.integer_sort(), &name)
        }
        IrExprKind::Load(slot) => slots.get(slot).cloned().unwrap_or_else(|| {
            let name = format!("__fresh_{}", state.fresh_counter);
            state.fresh_counter += 1;
            tm.mk_const(tm.integer_sort(), &name)
        }),
        IrExprKind::Arith { op, lhs, rhs } => {
            let l = encode_ir_expr_cvc5(
                tm,
                &IrExprKind::Load(*lhs),
                slots,
                vars,
                state,
                slot_to_name,
                slot_types,
                ir_blocks,
                enc_ctx,
            );
            let r = encode_ir_expr_cvc5(
                tm,
                &IrExprKind::Load(*rhs),
                slots,
                vars,
                state,
                slot_to_name,
                slot_types,
                ir_blocks,
                enc_ctx,
            );
            mk_ir_arith_cvc5(tm, *op, l, r)
        }
        IrExprKind::Cmp { op, lhs, rhs } => {
            let l = encode_ir_expr_cvc5(
                tm,
                &IrExprKind::Load(*lhs),
                slots,
                vars,
                state,
                slot_to_name,
                slot_types,
                ir_blocks,
                enc_ctx,
            );
            let r = encode_ir_expr_cvc5(
                tm,
                &IrExprKind::Load(*rhs),
                slots,
                vars,
                state,
                slot_to_name,
                slot_types,
                ir_blocks,
                enc_ctx,
            );
            mk_ir_cmp_as_int_cvc5(tm, *op, l, r)
        }
        IrExprKind::Call { func, args } => mk_ir_call_cvc5(
            tm,
            func,
            args,
            slots,
            vars,
            state,
            slot_to_name,
            slot_types,
            ir_blocks,
            enc_ctx,
        ),
        IrExprKind::Field { slot, index } => {
            if *index == 0
                && let Some(ty) = slot_types.get(slot)
                && is_collection_ir_type(ty)
                && let Some(name) = slot_to_name.get(slot)
            {
                return canonical_length_cvc5(tm, name, vars, state);
            }
            let base = encode_ir_expr_cvc5(
                tm,
                &IrExprKind::Load(*slot),
                slots,
                vars,
                state,
                slot_to_name,
                slot_types,
                ir_blocks,
                enc_ctx,
            );
            if let Some(ir_ty) = slot_types.get(slot)
                && let Some(field_name) = enc_ctx.type_ctx.field_name_at(ir_ty, *index)
            {
                let type_name = base_type_name(ir_ty);
                return mk_ir_unary_uf_cvc5(
                    tm,
                    &format!("__adt_{type_name}_{field_name}"),
                    base,
                    vars,
                    state,
                );
            }
            let ty_suffix = slot_types
                .get(slot)
                .map(|t| t.replace('<', "_").replace('>', ""))
                .unwrap_or_else(|| "val".into());
            mk_ir_unary_uf_cvc5(
                tm,
                &format!("__ir_field_{ty_suffix}_{index}"),
                base,
                vars,
                state,
            )
        }
        IrExprKind::Construct { type_id, fields } => {
            if enc_ctx.type_ctx.has_struct_layout(type_id) {
                return encode_ir_construct_typed_cvc5(
                    tm,
                    type_id,
                    fields,
                    slots,
                    vars,
                    state,
                    slot_to_name,
                    slot_types,
                    ir_blocks,
                    enc_ctx,
                );
            }
            let args: Vec<cvc5::Term<'a>> = fields
                .iter()
                .map(|(_, s)| {
                    encode_ir_expr_cvc5(
                        tm,
                        &IrExprKind::Load(*s),
                        slots,
                        vars,
                        state,
                        slot_to_name,
                        slot_types,
                        ir_blocks,
                        enc_ctx,
                    )
                })
                .collect();
            mk_ir_nary_uf_cvc5(tm, &format!("__ir_construct_{type_id}"), &args, vars, state)
        }
        IrExprKind::Cast { slot, .. } | IrExprKind::Transition { slot, .. } => encode_ir_expr_cvc5(
            tm,
            &IrExprKind::Load(*slot),
            slots,
            vars,
            state,
            slot_to_name,
            slot_types,
            ir_blocks,
            enc_ctx,
        ),
        IrExprKind::If {
            cond,
            then_block,
            else_block,
        } => {
            let cond_val = encode_ir_expr_cvc5(
                tm,
                &IrExprKind::Load(*cond),
                slots,
                vars,
                state,
                slot_to_name,
                slot_types,
                ir_blocks,
                enc_ctx,
            );
            let zero = tm.mk_integer(0);
            let cond_bool = tm.mk_term(cvc5::Kind::Distinct, &[cond_val, zero]);
            let then_v = eval_ir_block_cvc5(
                tm,
                *then_block,
                slots,
                vars,
                state,
                slot_to_name,
                slot_types,
                ir_blocks,
                enc_ctx,
            )
            .unwrap_or_else(|| {
                mk_ir_nullary_uf_cvc5(tm, &format!("__ir_block_{then_block}"), vars, state)
            });
            let else_v = eval_ir_block_cvc5(
                tm,
                *else_block,
                slots,
                vars,
                state,
                slot_to_name,
                slot_types,
                ir_blocks,
                enc_ctx,
            )
            .unwrap_or_else(|| {
                mk_ir_nullary_uf_cvc5(tm, &format!("__ir_block_{else_block}"), vars, state)
            });
            tm.mk_term(cvc5::Kind::Ite, &[cond_bool, then_v, else_v])
        }
    }
}

#[cfg(feature = "cvc5-verify")]
fn ensure_struct_adt_cvc5<'a>(
    tm: &'a cvc5::TermManager,
    state: &mut Cvc5EncoderState<'a>,
    type_name: &str,
    field_names: &[&str],
) {
    use crate::cvc5_adt::declare_struct_adt_ufs_cvc5_native;

    if !state.struct_adt_symbols.contains_key(type_name) {
        let (def, symbols) = declare_struct_adt_ufs_cvc5_native(tm, type_name, field_names);
        state.struct_adt_defs.insert(type_name.to_string(), def);
        state
            .struct_adt_symbols
            .insert(type_name.to_string(), symbols);
    }
}

#[cfg(feature = "cvc5-verify")]
#[expect(
    clippy::too_many_arguments,
    reason = "typed construct encoding threads IR context"
)]
fn encode_ir_construct_typed_cvc5<'a>(
    tm: &'a cvc5::TermManager,
    type_id: &str,
    fields: &[(usize, usize)],
    slots: &std::collections::HashMap<usize, cvc5::Term<'a>>,
    vars: &mut std::collections::HashMap<String, cvc5::Term<'a>>,
    state: &mut Cvc5EncoderState<'a>,
    slot_to_name: &std::collections::HashMap<usize, String>,
    slot_types: &std::collections::HashMap<usize, String>,
    ir_blocks: Option<&std::collections::HashMap<usize, Vec<crate::ir::IrInstr>>>,
    enc_ctx: IrEncodeContext<'a>,
) -> cvc5::Term<'a> {
    use crate::cvc5_adt::adt_constructor_cvc5_native;
    use crate::ir::IrExprKind;

    let field_names: Vec<&str> = enc_ctx
        .type_ctx
        .field_names_for(type_id)
        .unwrap_or_default();
    ensure_struct_adt_cvc5(tm, state, type_id, &field_names);

    let mut ordered = fields.to_vec();
    ordered.sort_by_key(|(idx, _)| *idx);
    let arg_terms: Vec<cvc5::Term<'a>> = ordered
        .iter()
        .map(|(_, s)| {
            encode_ir_expr_cvc5(
                tm,
                &IrExprKind::Load(*s),
                slots,
                vars,
                state,
                slot_to_name,
                slot_types,
                ir_blocks,
                enc_ctx,
            )
        })
        .collect();

    let ctor = state
        .struct_adt_defs
        .get(type_id)
        .and_then(|d| d.constructors.first())
        .expect("struct ADT has one constructor")
        .clone();
    let symbols = state
        .struct_adt_symbols
        .get(type_id)
        .expect("struct ADT symbols");

    adt_constructor_cvc5_native(
        tm,
        symbols,
        &ctor,
        &arg_terms,
        &mut state.axioms,
        &mut state.fresh_counter,
    )
}

#[cfg(feature = "cvc5-verify")]
#[expect(
    clippy::too_many_arguments,
    reason = "call inlining threads callee slot maps"
)]
fn eval_ir_call_cvc5<'a>(
    tm: &'a cvc5::TermManager,
    func: &str,
    args: &[usize],
    slots: &std::collections::HashMap<usize, cvc5::Term<'a>>,
    vars: &mut std::collections::HashMap<String, cvc5::Term<'a>>,
    state: &mut Cvc5EncoderState<'a>,
    slot_to_name: &std::collections::HashMap<usize, String>,
    slot_types: &std::collections::HashMap<usize, String>,
    ir_blocks: Option<&std::collections::HashMap<usize, Vec<crate::ir::IrInstr>>>,
    enc_ctx: IrEncodeContext<'a>,
) -> Option<cvc5::Term<'a>> {
    use crate::havoc_assume::RESULT_SLOT;
    use crate::ir::IrExprKind;

    let callee = enc_ctx.callee_ir(func)?;
    if callee.params.len() != args.len() {
        return None;
    }

    let prefix = format!("__ir_call_{func}_");
    let mut local: std::collections::HashMap<usize, cvc5::Term<'a>> =
        std::collections::HashMap::new();

    for (i, param) in callee.params.iter().enumerate() {
        let arg_val = encode_ir_expr_cvc5(
            tm,
            &IrExprKind::Load(args[i]),
            slots,
            vars,
            state,
            slot_to_name,
            slot_types,
            ir_blocks,
            enc_ctx,
        );
        let key = sanitize_smtlib_name(&format!("{prefix}param_{}", param.slot));
        let slot_var = vars
            .entry(key.clone())
            .or_insert_with(|| tm.mk_const(tm.integer_sort(), &key))
            .clone();
        state
            .axioms
            .push(tm.mk_term(cvc5::Kind::Equal, &[arg_val, slot_var.clone()]));
        local.insert(param.slot, slot_var);
    }

    let result_key = sanitize_smtlib_name(&format!("{prefix}result"));
    let result_var = vars
        .entry(result_key.clone())
        .or_insert_with(|| tm.mk_const(tm.integer_sort(), &result_key))
        .clone();
    local.insert(RESULT_SLOT, result_var);

    let callee_slot_types = slot_type_map(callee);
    let callee_names: std::collections::HashMap<usize, String> = callee
        .params
        .iter()
        .map(|p| (p.slot, format!("{prefix}param_{}", p.slot)))
        .collect();

    for instr in &callee.body {
        if instr.target != RESULT_SLOT && !local.contains_key(&instr.target) {
            let key = sanitize_smtlib_name(&format!("{prefix}slot_{}", instr.target));
            let v = vars
                .entry(key.clone())
                .or_insert_with(|| tm.mk_const(tm.integer_sort(), &key))
                .clone();
            local.insert(instr.target, v);
        }
        let computed = encode_ir_expr_cvc5(
            tm,
            &instr.expr,
            &local,
            vars,
            state,
            &callee_names,
            &callee_slot_types,
            ir_blocks,
            enc_ctx,
        );
        if let Some(target) = local.get(&instr.target) {
            state
                .axioms
                .push(tm.mk_term(cvc5::Kind::Equal, &[computed, target.clone()]));
        }
    }

    local.get(&RESULT_SLOT).cloned()
}

#[cfg(feature = "cvc5-verify")]
#[expect(
    clippy::too_many_arguments,
    reason = "call dispatch threads encoding context"
)]
fn mk_ir_call_cvc5<'a>(
    tm: &'a cvc5::TermManager,
    func: &str,
    args: &[usize],
    slots: &std::collections::HashMap<usize, cvc5::Term<'a>>,
    vars: &mut std::collections::HashMap<String, cvc5::Term<'a>>,
    state: &mut Cvc5EncoderState<'a>,
    slot_to_name: &std::collections::HashMap<usize, String>,
    slot_types: &std::collections::HashMap<usize, String>,
    ir_blocks: Option<&std::collections::HashMap<usize, Vec<crate::ir::IrInstr>>>,
    enc_ctx: IrEncodeContext<'a>,
) -> cvc5::Term<'a> {
    use crate::ir::IrExprKind;

    if is_length_ir_call(func, args.len())
        && let Some(slot) = args.first()
        && let Some(name) = slot_to_name.get(slot)
    {
        return canonical_length_cvc5(tm, name, vars, state);
    }

    if let Some(inlined) = eval_ir_call_cvc5(
        tm,
        func,
        args,
        slots,
        vars,
        state,
        slot_to_name,
        slot_types,
        ir_blocks,
        enc_ctx,
    ) {
        return inlined;
    }

    let arg_terms: Vec<cvc5::Term<'a>> = args
        .iter()
        .map(|a| {
            encode_ir_expr_cvc5(
                tm,
                &IrExprKind::Load(*a),
                slots,
                vars,
                state,
                slot_to_name,
                slot_types,
                ir_blocks,
                enc_ctx,
            )
        })
        .collect();
    if let Some(term) = encode_known_builtin_cvc5(tm, func, &arg_terms, state) {
        return term;
    }
    mk_ir_nary_uf_cvc5(tm, &format!("__ir_call_{func}"), &arg_terms, vars, state)
}

#[cfg(feature = "cvc5-verify")]
fn mk_ir_unary_uf_cvc5<'a>(
    tm: &'a cvc5::TermManager,
    name: &str,
    arg: cvc5::Term<'a>,
    vars: &mut std::collections::HashMap<String, cvc5::Term<'a>>,
    state: &mut Cvc5EncoderState<'a>,
) -> cvc5::Term<'a> {
    mk_ir_nary_uf_cvc5(tm, name, &[arg], vars, state)
}

#[cfg(feature = "cvc5-verify")]
fn mk_ir_nullary_uf_cvc5<'a>(
    tm: &'a cvc5::TermManager,
    name: &str,
    vars: &mut std::collections::HashMap<String, cvc5::Term<'a>>,
    state: &mut Cvc5EncoderState<'a>,
) -> cvc5::Term<'a> {
    mk_ir_nary_uf_cvc5(tm, name, &[], vars, state)
}

#[cfg(feature = "cvc5-verify")]
fn mk_ir_nary_uf_cvc5<'a>(
    tm: &'a cvc5::TermManager,
    name: &str,
    args: &[cvc5::Term<'a>],
    _vars: &mut std::collections::HashMap<String, cvc5::Term<'a>>,
    _state: &mut Cvc5EncoderState<'a>,
) -> cvc5::Term<'a> {
    let key = sanitize_smtlib_name(name);
    let domain: Vec<cvc5::Sort<'_>> = (0..args.len()).map(|_| tm.integer_sort()).collect();
    let fun_sort = if domain.is_empty() {
        tm.integer_sort()
    } else {
        tm.mk_fun_sort(&domain, tm.integer_sort())
    };
    let decl = tm.mk_const(fun_sort, &key);
    if args.is_empty() {
        decl
    } else {
        let mut apply_args = Vec::with_capacity(1 + args.len());
        apply_args.push(decl);
        apply_args.extend_from_slice(args);
        tm.mk_term(cvc5::Kind::ApplyUf, &apply_args)
    }
}

#[cfg(feature = "cvc5-verify")]
fn encode_ir_pred_arg_cvc5<'a>(
    tm: &'a cvc5::TermManager,
    arg: &crate::ir::IrPredArg,
    slots: &std::collections::HashMap<usize, cvc5::Term<'a>>,
    vars: &mut std::collections::HashMap<String, cvc5::Term<'a>>,
    state: &mut Cvc5EncoderState<'a>,
) -> cvc5::Term<'a> {
    use crate::havoc_assume::RESULT_SLOT;
    use crate::ir::{IrLiteral, IrPredArg};

    match arg {
        IrPredArg::Slot(n) => slots.get(n).cloned().unwrap_or_else(|| {
            let name = format!("__fresh_{}", state.fresh_counter);
            state.fresh_counter += 1;
            tm.mk_const(tm.integer_sort(), &name)
        }),
        IrPredArg::SlotResult => slots.get(&RESULT_SLOT).cloned().unwrap_or_else(|| {
            let key = sanitize_smtlib_name("result");
            vars.entry(key.clone())
                .or_insert_with(|| tm.mk_const(tm.integer_sort(), &key))
                .clone()
        }),
        IrPredArg::Lit(IrLiteral::Int(n)) => tm.mk_integer(*n),
        IrPredArg::Lit(IrLiteral::Float(f)) => tm.mk_integer(*f as i64),
        IrPredArg::Lit(IrLiteral::Bool(b)) => tm.mk_integer(if *b { 1 } else { 0 }),
        IrPredArg::Lit(IrLiteral::Str(_)) => {
            let name = format!("__fresh_{}", state.fresh_counter);
            state.fresh_counter += 1;
            tm.mk_const(tm.integer_sort(), &name)
        }
        IrPredArg::Arith { op, lhs, rhs } => {
            let l = encode_ir_pred_arg_cvc5(tm, lhs, slots, vars, state);
            let r = encode_ir_pred_arg_cvc5(tm, rhs, slots, vars, state);
            mk_ir_arith_cvc5(tm, *op, l, r)
        }
    }
}

#[cfg(feature = "cvc5-verify")]
fn encode_ir_pred_cvc5<'a>(
    tm: &'a cvc5::TermManager,
    pred: &crate::ir::IrPred,
    slots: &std::collections::HashMap<usize, cvc5::Term<'a>>,
    vars: &mut std::collections::HashMap<String, cvc5::Term<'a>>,
    state: &mut Cvc5EncoderState<'a>,
) -> Option<cvc5::Term<'a>> {
    use crate::ir::IrPred;

    match pred {
        IrPred::True => Some(tm.mk_boolean(true)),
        IrPred::False => Some(tm.mk_boolean(false)),
        IrPred::Cmp { op, lhs, rhs } => {
            let l = encode_ir_pred_arg_cvc5(tm, lhs, slots, vars, state);
            let r = encode_ir_pred_arg_cvc5(tm, rhs, slots, vars, state);
            Some(mk_ir_cmp_bool_cvc5(tm, *op, l, r))
        }
        IrPred::And(a, b) => {
            let la = encode_ir_pred_cvc5(tm, a, slots, vars, state)?;
            let lb = encode_ir_pred_cvc5(tm, b, slots, vars, state)?;
            Some(tm.mk_term(cvc5::Kind::And, &[la, lb]))
        }
        IrPred::Or(a, b) => {
            let la = encode_ir_pred_cvc5(tm, a, slots, vars, state)?;
            let lb = encode_ir_pred_cvc5(tm, b, slots, vars, state)?;
            Some(tm.mk_term(cvc5::Kind::Or, &[la, lb]))
        }
        IrPred::Not(inner) => encode_ir_pred_cvc5(tm, inner, slots, vars, state)
            .map(|p| tm.mk_term(cvc5::Kind::Not, &[p])),
    }
}

/// Apply havoc-assume IR body constraints as background axioms.
#[cfg(feature = "cvc5-verify")]
pub(crate) fn apply_ir_body_constraints_cvc5<'a>(
    tm: &'a cvc5::TermManager,
    func: &crate::ir::IrFunction,
    contract_param_names: &[String],
    vars: &mut std::collections::HashMap<String, cvc5::Term<'a>>,
    state: &mut Cvc5EncoderState<'a>,
    ir_blocks: Option<&std::collections::HashMap<usize, Vec<crate::ir::IrInstr>>>,
    enc_ctx: IrEncodeContext<'a>,
) {
    use crate::havoc_assume::{RESULT_SLOT, ir_param_names};
    use crate::ir::IrExprKind;

    let mut slots: std::collections::HashMap<usize, cvc5::Term<'a>> =
        std::collections::HashMap::new();

    for (slot, name) in ir_param_names(func, contract_param_names) {
        let key = sanitize_smtlib_name(&name);
        let v = vars
            .entry(key.clone())
            .or_insert_with(|| tm.mk_const(tm.integer_sort(), &key))
            .clone();
        slots.insert(slot, v);
    }

    let result_key = sanitize_smtlib_name("result");
    let result = vars
        .entry(result_key.clone())
        .or_insert_with(|| tm.mk_const(tm.integer_sort(), &result_key))
        .clone();
    slots.insert(RESULT_SLOT, result);

    let slot_to_name: std::collections::HashMap<usize, String> =
        ir_param_names(func, contract_param_names)
            .into_iter()
            .collect();
    let slot_types = slot_type_map(func);

    for instr in &func.body {
        if instr.target != RESULT_SLOT && !slots.contains_key(&instr.target) {
            let key = sanitize_smtlib_name(&format!("__ir_slot_{}", instr.target));
            let v = vars
                .entry(key.clone())
                .or_insert_with(|| tm.mk_const(tm.integer_sort(), &key))
                .clone();
            slots.insert(instr.target, v);
        }
        let computed = encode_ir_expr_cvc5(
            tm,
            &instr.expr,
            &slots,
            vars,
            state,
            &slot_to_name,
            &slot_types,
            ir_blocks,
            enc_ctx,
        );
        if let Some(target) = slots.get(&instr.target) {
            state
                .axioms
                .push(tm.mk_term(cvc5::Kind::Equal, &[computed, target.clone()]));
        }
        if instr.target == RESULT_SLOT
            && let IrExprKind::Load(src) = &instr.expr
            && let Some(param) = slot_to_name.get(src)
        {
            let len_result = canonical_length_cvc5(tm, "result", vars, state);
            let len_param = canonical_length_cvc5(tm, param, vars, state);
            state
                .axioms
                .push(tm.mk_term(cvc5::Kind::Equal, &[len_result, len_param]));
        }
    }

    if let Some(post) = &func.post
        && let Some(pred) = encode_ir_pred_cvc5(tm, post, &slots, vars, state)
    {
        state.axioms.push(pred);
    }
}

#[cfg(all(test, feature = "cvc5-verify"))]
mod tests {
    use super::*;
    use crate::cvc5_encoder_state::default_cvc5_encoder_state;
    use crate::ir::{IrArithOp, IrExprKind};

    #[test]
    fn ir_arith_add_encodes() {
        let tm = cvc5::TermManager::new();
        let mut state = default_cvc5_encoder_state();
        let slots = std::collections::HashMap::new();
        let mut vars = std::collections::HashMap::new();
        let expr = IrExprKind::Arith {
            op: IrArithOp::Add,
            lhs: 0,
            rhs: 1,
        };
        let names = std::collections::HashMap::new();
        let types = std::collections::HashMap::new();
        let _ = encode_ir_expr_cvc5(
            &tm,
            &expr,
            &slots,
            &mut vars,
            &mut state,
            &names,
            &types,
            None,
            IrEncodeContext::default(),
        );
    }

    #[test]
    fn cvc5_ir_call_inlines_callee_sidecar() {
        use crate::ir::parse_ir_module;
        use std::collections::HashMap;

        let main_ir = parse_ir_module(
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

        let helper_ir = parse_ir_module(
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

        let tm = cvc5::TermManager::new();
        let mut state = default_cvc5_encoder_state();
        let mut vars = std::collections::HashMap::new();
        apply_ir_body_constraints_cvc5(
            &tm,
            &main_ir,
            &["x".into()],
            &mut vars,
            &mut state,
            None,
            IrEncodeContext::new(None, Some(&bodies), None),
        );

        assert!(
            state.axioms.len() >= 3,
            "inlined call should emit callee binding axioms, got {}",
            state.axioms.len()
        );
    }

    #[test]
    fn cvc5_ir_blocks_inlines_sibling_functions() {
        use crate::ir::parse_ir_module;

        let ir_source = r#"
module branch {
  fn #0 : ($0: Int) -> Int ! pure
  {
    $1 = if $0 then #1 else #2 : Int
    $result = load $1 : Int
  }
  fn #1 : ($0: Int) -> Int ! pure
  {
    $result = load $0 : Int
  }
  fn #2 : ($0: Int) -> Int ! pure
  {
    $result = const 0 : Int
  }
}
"#;
        let module = parse_ir_module(ir_source).unwrap();
        let func = module.functions[0].clone();
        let blocks = crate::ir_encode::block_map_from_module(&module);

        let tm = cvc5::TermManager::new();
        let mut state = default_cvc5_encoder_state();
        let mut vars = std::collections::HashMap::new();
        apply_ir_body_constraints_cvc5(
            &tm,
            &func,
            &["x".into()],
            &mut vars,
            &mut state,
            Some(&blocks),
            IrEncodeContext::default(),
        );

        assert!(
            state.axioms.len() >= 4,
            "expected inlined block axioms, got {}",
            state.axioms.len()
        );
    }
}
