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
struct Cvc5IrEncodeFrame<'a> {
    slots: &'a std::collections::HashMap<usize, cvc5::Term<'a>>,
    vars: &'a mut std::collections::HashMap<String, cvc5::Term<'a>>,
    state: &'a mut Cvc5EncoderState<'a>,
    slot_to_name: &'a std::collections::HashMap<usize, String>,
    slot_types: &'a std::collections::HashMap<usize, String>,
}

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
fn eval_ir_block_cvc5<'a>(
    tm: &'a cvc5::TermManager,
    block_id: usize,
    frame: &mut Cvc5IrEncodeFrame<'a>,
    enc_ctx: IrEncodeContext<'a>,
) -> Option<cvc5::Term<'a>> {
    use crate::havoc_assume::RESULT_SLOT;

    let body = enc_ctx.ir_blocks?.get(&block_id)?;
    let mut local = frame.slots.clone();
    let mut last = None;
    for instr in body {
        if instr.target != RESULT_SLOT && !local.contains_key(&instr.target) {
            let name = format!("__ir_block{block_id}_slot_{}", instr.target);
            let key = sanitize_smtlib_name(&name);
            let v = frame
                .vars
                .entry(key.clone())
                .or_insert_with(|| tm.mk_const(tm.integer_sort(), &key))
                .clone();
            local.insert(instr.target, v);
        }
        let mut local_frame = Cvc5IrEncodeFrame {
            slots: &local,
            vars: frame.vars,
            state: frame.state,
            slot_to_name: frame.slot_to_name,
            slot_types: frame.slot_types,
        };
        let computed = encode_ir_expr_cvc5(tm, &instr.expr, &mut local_frame, enc_ctx);
        if let Some(target) = local.get(&instr.target) {
            frame
                .state
                .axioms
                .push(tm.mk_term(cvc5::Kind::Equal, &[computed, target.clone()]));
        }
        last = local.get(&instr.target).cloned();
    }
    last
}

#[cfg(feature = "cvc5-verify")]
fn encode_ir_expr_cvc5<'a>(
    tm: &'a cvc5::TermManager,
    expr: &crate::ir::IrExprKind,
    frame: &mut Cvc5IrEncodeFrame<'a>,
    enc_ctx: IrEncodeContext<'a>,
) -> cvc5::Term<'a> {
    use crate::ir::{IrExprKind, IrLiteral};

    match expr {
        IrExprKind::Const(IrLiteral::Int(n)) => tm.mk_integer(*n),
        IrExprKind::Const(IrLiteral::Float(f)) => tm.mk_integer(*f as i64),
        IrExprKind::Const(IrLiteral::Bool(b)) => tm.mk_integer(if *b { 1 } else { 0 }),
        IrExprKind::Const(IrLiteral::Str(_)) => {
            let name = format!("__fresh_{}", frame.state.fresh_counter);
            frame.state.fresh_counter += 1;
            tm.mk_const(tm.integer_sort(), &name)
        }
        IrExprKind::Load(slot) => frame.slots.get(slot).cloned().unwrap_or_else(|| {
            let name = format!("__fresh_{}", frame.state.fresh_counter);
            frame.state.fresh_counter += 1;
            tm.mk_const(tm.integer_sort(), &name)
        }),
        IrExprKind::Arith { op, lhs, rhs } => {
            let l = encode_ir_expr_cvc5(tm, &IrExprKind::Load(*lhs), frame, enc_ctx);
            let r = encode_ir_expr_cvc5(tm, &IrExprKind::Load(*rhs), frame, enc_ctx);
            mk_ir_arith_cvc5(tm, *op, l, r)
        }
        IrExprKind::Cmp { op, lhs, rhs } => {
            let l = encode_ir_expr_cvc5(tm, &IrExprKind::Load(*lhs), frame, enc_ctx);
            let r = encode_ir_expr_cvc5(tm, &IrExprKind::Load(*rhs), frame, enc_ctx);
            mk_ir_cmp_as_int_cvc5(tm, *op, l, r)
        }
        IrExprKind::Call { func, args } => {
            mk_ir_call_cvc5(tm, func, args, frame, enc_ctx)
        }
        IrExprKind::Field { slot, index } => {
            if *index == 0
                && let Some(ty) = frame.slot_types.get(slot)
                && is_collection_ir_type(ty)
                && let Some(name) = frame.slot_to_name.get(slot)
            {
                return canonical_length_cvc5(tm, name, frame.vars, frame.state);
            }
            let base = encode_ir_expr_cvc5(tm, &IrExprKind::Load(*slot), frame, enc_ctx);
            if let Some(ir_ty) = frame.slot_types.get(slot)
                && let Some(field_name) = enc_ctx.type_ctx.field_name_at(ir_ty, *index)
            {
                let type_name = base_type_name(ir_ty);
                return mk_ir_unary_uf_cvc5(
                    tm,
                    &format!("__adt_{type_name}_{field_name}"),
                    base,
                    frame.vars,
                    frame.state,
                );
            }
            let ty_suffix = frame
                .slot_types
                .get(slot)
                .map(|t| t.replace('<', "_").replace('>', ""))
                .unwrap_or_else(|| "val".into());
            mk_ir_unary_uf_cvc5(
                tm,
                &format!("__ir_field_{ty_suffix}_{index}"),
                base,
                frame.vars,
                frame.state,
            )
        }
        IrExprKind::Construct { type_id, fields } => {
            if enc_ctx.type_ctx.has_struct_layout(type_id) {
                return encode_ir_construct_typed_cvc5(tm, type_id, fields, frame, enc_ctx);
            }
            let args: Vec<cvc5::Term<'a>> = fields
                .iter()
                .map(|(_, s)| {
                    encode_ir_expr_cvc5(tm, &IrExprKind::Load(*s), frame, enc_ctx)
                })
                .collect();
            mk_ir_nary_uf_cvc5(
                tm,
                &format!("__ir_construct_{type_id}"),
                &args,
                frame.vars,
                frame.state,
            )
        }
        IrExprKind::Cast { slot, .. } | IrExprKind::Transition { slot, .. } => {
            encode_ir_expr_cvc5(tm, &IrExprKind::Load(*slot), frame, enc_ctx)
        }
        IrExprKind::If {
            cond,
            then_block,
            else_block,
        } => {
            let cond_val = encode_ir_expr_cvc5(tm, &IrExprKind::Load(*cond), frame, enc_ctx);
            let zero = tm.mk_integer(0);
            let cond_bool = tm.mk_term(cvc5::Kind::Distinct, &[cond_val, zero]);
            let then_v = eval_ir_block_cvc5(tm, *then_block, frame, enc_ctx)
                .unwrap_or_else(|| {
                    mk_ir_nullary_uf_cvc5(
                        tm,
                        &format!("__ir_block_{then_block}"),
                        frame.vars,
                        frame.state,
                    )
                });
            let else_v = eval_ir_block_cvc5(tm, *else_block, frame, enc_ctx)
                .unwrap_or_else(|| {
                    mk_ir_nullary_uf_cvc5(
                        tm,
                        &format!("__ir_block_{else_block}"),
                        frame.vars,
                        frame.state,
                    )
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
fn encode_ir_construct_typed_cvc5<'a>(
    tm: &'a cvc5::TermManager,
    type_id: &str,
    fields: &[(usize, usize)],
    frame: &mut Cvc5IrEncodeFrame<'a>,
    enc_ctx: IrEncodeContext<'a>,
) -> cvc5::Term<'a> {
    use crate::cvc5_adt::adt_constructor_cvc5_native;
    use crate::ir::IrExprKind;

    let field_names: Vec<&str> = enc_ctx
        .type_ctx
        .field_names_for(type_id)
        .unwrap_or_default();
    ensure_struct_adt_cvc5(tm, frame.state, type_id, &field_names);

    let mut ordered = fields.to_vec();
    ordered.sort_by_key(|(idx, _)| *idx);
    let arg_terms: Vec<cvc5::Term<'a>> = ordered
        .iter()
        .map(|(_, s)| encode_ir_expr_cvc5(tm, &IrExprKind::Load(*s), frame, enc_ctx))
        .collect();

    let ctor = frame
        .state
        .struct_adt_defs
        .get(type_id)
        .and_then(|d| d.constructors.first())
        .expect("struct ADT has one constructor")
        .clone();
    let symbols = frame
        .state
        .struct_adt_symbols
        .get(type_id)
        .expect("struct ADT symbols");

    adt_constructor_cvc5_native(
        tm,
        symbols,
        &ctor,
        &arg_terms,
        &mut frame.state.axioms,
        &mut frame.state.fresh_counter,
    )
}

#[cfg(feature = "cvc5-verify")]
fn eval_ir_call_cvc5<'a>(
    tm: &'a cvc5::TermManager,
    func: &str,
    args: &[usize],
    frame: &mut Cvc5IrEncodeFrame<'a>,
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
        let arg_val = encode_ir_expr_cvc5(tm, &IrExprKind::Load(args[i]), frame, enc_ctx);
        let key = sanitize_smtlib_name(&format!("{prefix}param_{}", param.slot));
        let slot_var = frame
            .vars
            .entry(key.clone())
            .or_insert_with(|| tm.mk_const(tm.integer_sort(), &key))
            .clone();
        frame
            .state
            .axioms
            .push(tm.mk_term(cvc5::Kind::Equal, &[arg_val, slot_var.clone()]));
        local.insert(param.slot, slot_var);
    }

    let result_key = sanitize_smtlib_name(&format!("{prefix}result"));
    let result_var = frame
        .vars
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
            let v = frame
                .vars
                .entry(key.clone())
                .or_insert_with(|| tm.mk_const(tm.integer_sort(), &key))
                .clone();
            local.insert(instr.target, v);
        }
        let mut local_frame = Cvc5IrEncodeFrame {
            slots: &local,
            vars: frame.vars,
            state: frame.state,
            slot_to_name: &callee_names,
            slot_types: &callee_slot_types,
        };
        let computed = encode_ir_expr_cvc5(tm, &instr.expr, &mut local_frame, enc_ctx);
        if let Some(target) = local.get(&instr.target) {
            frame
                .state
                .axioms
                .push(tm.mk_term(cvc5::Kind::Equal, &[computed, target.clone()]));
        }
    }

    local.get(&RESULT_SLOT).cloned()
}

#[cfg(feature = "cvc5-verify")]
fn mk_ir_call_cvc5<'a>(
    tm: &'a cvc5::TermManager,
    func: &str,
    args: &[usize],
    frame: &mut Cvc5IrEncodeFrame<'a>,
    enc_ctx: IrEncodeContext<'a>,
) -> cvc5::Term<'a> {
    use crate::ir::IrExprKind;

    if is_length_ir_call(func, args.len())
        && let Some(slot) = args.first()
        && let Some(name) = frame.slot_to_name.get(slot)
    {
        return canonical_length_cvc5(tm, name, frame.vars, frame.state);
    }

    if let Some(inlined) = eval_ir_call_cvc5(tm, func, args, frame, enc_ctx) {
        return inlined;
    }

    let arg_terms: Vec<cvc5::Term<'a>> = args
        .iter()
        .map(|a| encode_ir_expr_cvc5(tm, &IrExprKind::Load(*a), frame, enc_ctx))
        .collect();
    if let Some(term) = encode_known_builtin_cvc5(tm, func, &arg_terms, frame.state) {
        return term;
    }
    mk_ir_nary_uf_cvc5(
        tm,
        &format!("__ir_call_{func}"),
        &arg_terms,
        frame.vars,
        frame.state,
    )
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

    let mut frame = Cvc5IrEncodeFrame {
        slots: &slots,
        vars,
        state,
        slot_to_name: &slot_to_name,
        slot_types: &slot_types,
    };

    for instr in &func.body {
        if instr.target != RESULT_SLOT && !slots.contains_key(&instr.target) {
            let key = sanitize_smtlib_name(&format!("__ir_slot_{}", instr.target));
            let v = vars
                .entry(key.clone())
                .or_insert_with(|| tm.mk_const(tm.integer_sort(), &key))
                .clone();
            slots.insert(instr.target, v);
        }
        let computed = encode_ir_expr_cvc5(tm, &instr.expr, &mut frame, enc_ctx);
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
        let mut frame = Cvc5IrEncodeFrame {
            slots: &slots,
            vars: &mut vars,
            state: &mut state,
            slot_to_name: &names,
            slot_types: &types,
        };
        let _ = encode_ir_expr_cvc5(&tm, &expr, &mut frame, IrEncodeContext::default());
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
        let (func, blocks) = crate::ir_encode::branch_if_else_ir_fixture();
        let enc_ctx = IrEncodeContext::new(None, None, Some(&blocks));

        let tm = cvc5::TermManager::new();
        let mut state = default_cvc5_encoder_state();
        let mut vars = std::collections::HashMap::new();
        apply_ir_body_constraints_cvc5(
            &tm,
            &func,
            &["x".into()],
            &mut vars,
            &mut state,
            enc_ctx,
        );

        let text: String = state
            .axioms
            .iter()
            .map(|t| t.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        crate::ir_encode::assert_ir_blocks_inlined(&text, state.axioms.len());
    }
}
