//! Native CVC5 term encoding (feature = "cvc5-verify").
//!
//! Expression-to-term translation extracted from `cvc5_backend.rs`.

use std::collections::HashMap;

use assura_parser::ast::{BinOp, Clause, Expr, Literal, Pattern, UnaryOp};
use assura_types::checkers::expr_references_var;

use crate::cvc5_builtins::{
    KnownBuiltin, classify_known_builtin, is_bool_field, is_bool_returning_uf, is_size_field,
    pattern_hash_name,
};
use crate::cvc5_raw_ops::{
    apply_raw_op_cvc5, comma_chunk_ranges, find_matching_delim, is_raw_spec_skip_keyword,
    parse_raw_quantifier_slice, raw_op_info, raw_op_is_comparison,
};

use super::{
    flatten_field_chain_cvc5, has_deep_field_chain_cvc5, is_self_rooted_cvc5, sanitize_smtlib_name,
};

/// Encoder state for the native CVC5 backend.
/// Tracks background axioms, string constants, and fresh variable counter.
#[cfg(feature = "cvc5-verify")]
pub(crate) struct Cvc5EncoderState<'a> {
    pub(crate) axioms: Vec<cvc5::Term<'a>>,
    string_constants: Vec<String>,
    fresh_counter: usize,
    /// When true, use native CVC5 string theory (string_sort, StringLength, etc.)
    /// instead of integer encoding.
    use_string_theory: bool,
    /// Shared `__field_len` UF (one declaration per encoder session).
    field_len_fn: Option<cvc5::Term<'a>>,
}

// -------------------------------------------------------------------------
// Havoc+assume encoding (#267)
// -------------------------------------------------------------------------

#[cfg(feature = "cvc5-verify")]
fn get_or_create_int_cvc5<'a>(
    tm: &'a cvc5::TermManager,
    name: &str,
    vars: &mut std::collections::HashMap<String, cvc5::Term<'a>>,
) -> cvc5::Term<'a> {
    vars.entry(name.to_string())
        .or_insert_with(|| tm.mk_const(tm.integer_sort(), name))
        .clone()
}

#[cfg(feature = "cvc5-verify")]
fn field_len_fn_cvc5<'a>(
    tm: &'a cvc5::TermManager,
    state: &mut Cvc5EncoderState<'a>,
) -> cvc5::Term<'a> {
    if let Some(f) = state.field_len_fn.as_ref() {
        return f.clone();
    }
    let len_sort = tm.mk_fun_sort(&[tm.integer_sort()], tm.integer_sort());
    let len_func = tm.mk_const(len_sort, "__field_len");
    state.field_len_fn = Some(len_func.clone());
    len_func
}

#[cfg(feature = "cvc5-verify")]
fn canonical_length_cvc5<'a>(
    tm: &'a cvc5::TermManager,
    name: &str,
    vars: &mut std::collections::HashMap<String, cvc5::Term<'a>>,
    state: &mut Cvc5EncoderState<'a>,
) -> cvc5::Term<'a> {
    let key = format!("__canonical_len_{name}");
    if let Some(v) = vars.get(&key) {
        return v.clone();
    }
    let v = tm.mk_const(tm.integer_sort(), &key);
    let zero = tm.mk_integer(0);
    state
        .axioms
        .push(tm.mk_term(cvc5::Kind::Geq, &[v.clone(), zero]));
    vars.insert(key, v.clone());
    v
}

#[cfg(feature = "cvc5-verify")]
pub(crate) fn apply_havoc_assume_cvc5<'a>(
    tm: &'a cvc5::TermManager,
    requires: &[&Clause],
    ensures: &[&Clause],
    return_ty: &[String],
    param_names: &[String],
    ir: Option<&crate::ir::IrFunction>,
    vars: &mut std::collections::HashMap<String, cvc5::Term<'a>>,
    state: &mut Cvc5EncoderState<'a>,
) {
    use crate::havoc_assume::{infer_length_identity_links, is_collection_return};

    if is_collection_return(return_ty) {
        let len = canonical_length_cvc5(tm, "result", vars, state);
        let zero = tm.mk_integer(0);
        state.axioms.push(tm.mk_term(cvc5::Kind::Geq, &[len, zero]));
    }

    for (result, input) in infer_length_identity_links(requires, ensures) {
        let len_result = canonical_length_cvc5(tm, &result, vars, state);
        let len_input = canonical_length_cvc5(tm, &input, vars, state);
        state
            .axioms
            .push(tm.mk_term(cvc5::Kind::Leq, &[len_result, len_input]));
    }

    if let Some(func) = ir {
        apply_ir_body_constraints_cvc5(tm, func, param_names, vars, state);
    }
}

#[cfg(feature = "cvc5-verify")]
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
fn apply_ir_body_constraints_cvc5<'a>(
    tm: &'a cvc5::TermManager,
    func: &crate::ir::IrFunction,
    contract_param_names: &[String],
    vars: &mut std::collections::HashMap<String, cvc5::Term<'a>>,
    state: &mut Cvc5EncoderState<'a>,
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

    for instr in &func.body {
        let computed = encode_ir_expr_cvc5(tm, &instr.expr, &slots, vars, state);
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

#[cfg(feature = "cvc5-verify")]
fn encode_ir_expr_cvc5<'a>(
    tm: &'a cvc5::TermManager,
    expr: &crate::ir::IrExprKind,
    slots: &std::collections::HashMap<usize, cvc5::Term<'a>>,
    vars: &mut std::collections::HashMap<String, cvc5::Term<'a>>,
    state: &mut Cvc5EncoderState<'a>,
) -> cvc5::Term<'a> {
    use crate::ir::{IrExprKind, IrLiteral};

    match expr {
        IrExprKind::Const(IrLiteral::Int(n)) => tm.mk_integer(*n),
        IrExprKind::Load(slot) => slots.get(slot).cloned().unwrap_or_else(|| {
            let name = format!("__fresh_{}", state.fresh_counter);
            state.fresh_counter += 1;
            tm.mk_const(tm.integer_sort(), &name)
        }),
        IrExprKind::Arith { op, lhs, rhs } => {
            let l = encode_ir_expr_cvc5(tm, &IrExprKind::Load(*lhs), slots, vars, state);
            let r = encode_ir_expr_cvc5(tm, &IrExprKind::Load(*rhs), slots, vars, state);
            mk_ir_arith_cvc5(tm, *op, l, r)
        }
        IrExprKind::Cmp { op, lhs, rhs } => {
            let l = encode_ir_expr_cvc5(tm, &IrExprKind::Load(*lhs), slots, vars, state);
            let r = encode_ir_expr_cvc5(tm, &IrExprKind::Load(*rhs), slots, vars, state);
            mk_ir_cmp_as_int_cvc5(tm, *op, l, r)
        }
        _ => {
            let name = format!("__fresh_{}", state.fresh_counter);
            state.fresh_counter += 1;
            tm.mk_const(tm.integer_sort(), &name)
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
/// Bind pattern variables as fresh CVC5 integer constants so they are
/// available when encoding the match arm body.
///
/// Recursively walks `Constructor` and `Tuple` sub-patterns. Wildcard
/// and literal patterns introduce no new bindings.
#[cfg(feature = "cvc5-verify")]
fn bind_pattern_vars_cvc5<'a>(
    tm: &'a cvc5::TermManager,
    pattern: &Pattern,
    vars: &mut HashMap<String, cvc5::Term<'a>>,
) {
    match pattern {
        Pattern::Ident(name) => {
            if !vars.contains_key(name) {
                let v = tm.mk_const(tm.integer_sort(), name);
                vars.insert(name.clone(), v);
            }
        }
        Pattern::Constructor { fields, .. } => {
            for field in fields {
                bind_pattern_vars_cvc5(tm, field, vars);
            }
        }
        Pattern::Tuple(pats) => {
            for pat in pats {
                bind_pattern_vars_cvc5(tm, pat, vars);
            }
        }
        Pattern::Wildcard | Pattern::Literal(_) => {}
    }
}

#[cfg(feature = "cvc5-verify")]
pub(crate) fn default_cvc5_encoder_state<'a>() -> Cvc5EncoderState<'a> {
    Cvc5EncoderState {
        axioms: Vec::new(),
        string_constants: Vec::new(),
        fresh_counter: 0,
        use_string_theory: false,
        field_len_fn: None,
    }
}

#[cfg(feature = "cvc5-verify")]
fn fresh_int_cvc5<'a>(
    tm: &'a cvc5::TermManager,
    state: &mut Cvc5EncoderState<'a>,
) -> cvc5::Term<'a> {
    let fresh_name = format!("__fresh_{}", state.fresh_counter);
    state.fresh_counter += 1;
    tm.mk_const(tm.integer_sort(), &fresh_name)
}

#[cfg(feature = "cvc5-verify")]
fn field_len_of_cvc5<'a>(
    tm: &'a cvc5::TermManager,
    state: &mut Cvc5EncoderState<'a>,
    value: &cvc5::Term<'a>,
) -> cvc5::Term<'a> {
    let len_func = field_len_fn_cvc5(tm, state);
    tm.mk_term(cvc5::Kind::ApplyUf, &[len_func, value.clone()])
}

#[cfg(feature = "cvc5-verify")]
fn apply_int_uf_cvc5<'a>(
    tm: &'a cvc5::TermManager,
    name: &str,
    args: &[cvc5::Term<'a>],
    returns_bool: bool,
) -> cvc5::Term<'a> {
    let domain: Vec<cvc5::Sort> = (0..args.len()).map(|_| tm.integer_sort()).collect();
    let codomain = if returns_bool {
        tm.boolean_sort()
    } else {
        tm.integer_sort()
    };
    let func_sort = tm.mk_fun_sort(&domain, codomain);
    let func_const = tm.mk_const(func_sort, name);
    let mut apply_args = vec![func_const];
    apply_args.extend_from_slice(args);
    tm.mk_term(cvc5::Kind::ApplyUf, &apply_args)
}

/// Encode builtins with known semantics (shared by `Call` and `MethodCall`).
#[cfg(feature = "cvc5-verify")]
fn encode_known_builtin_cvc5<'a>(
    tm: &'a cvc5::TermManager,
    op: &str,
    args: &[cvc5::Term<'a>],
    state: &mut Cvc5EncoderState<'a>,
) -> Option<cvc5::Term<'a>> {
    let kind = classify_known_builtin(op, args.len())?;
    match kind {
        KnownBuiltin::Abs => {
            let x = &args[0];
            let zero = tm.mk_integer(0);
            let neg = tm.mk_term(cvc5::Kind::Neg, &[x.clone()]);
            let cond = tm.mk_term(cvc5::Kind::Geq, &[x.clone(), zero]);
            Some(tm.mk_term(cvc5::Kind::Ite, &[cond, x.clone(), neg]))
        }
        KnownBuiltin::Min => {
            let (a, b) = (&args[0], &args[1]);
            let cond = tm.mk_term(cvc5::Kind::Leq, &[a.clone(), b.clone()]);
            Some(tm.mk_term(cvc5::Kind::Ite, &[cond, a.clone(), b.clone()]))
        }
        KnownBuiltin::Max => {
            let (a, b) = (&args[0], &args[1]);
            let cond = tm.mk_term(cvc5::Kind::Geq, &[a.clone(), b.clone()]);
            Some(tm.mk_term(cvc5::Kind::Ite, &[cond, a.clone(), b.clone()]))
        }
        KnownBuiltin::Substring => {
            let str_val = &args[0];
            let start = &args[1];
            let end = &args[2];
            let result = fresh_int_cvc5(tm, state);
            let zero = tm.mk_integer(0);
            state
                .axioms
                .push(tm.mk_term(cvc5::Kind::Geq, &[start.clone(), zero.clone()]));
            state
                .axioms
                .push(tm.mk_term(cvc5::Kind::Leq, &[start.clone(), end.clone()]));
            let len_func = field_len_fn_cvc5(tm, state);
            let str_len = tm.mk_term(cvc5::Kind::ApplyUf, &[len_func.clone(), str_val.clone()]);
            state
                .axioms
                .push(tm.mk_term(cvc5::Kind::Leq, &[end.clone(), str_len]));
            let res_len = tm.mk_term(cvc5::Kind::ApplyUf, &[len_func, result.clone()]);
            let diff = tm.mk_term(cvc5::Kind::Sub, &[end.clone(), start.clone()]);
            state
                .axioms
                .push(tm.mk_term(cvc5::Kind::Equal, &[res_len.clone(), diff]));
            state
                .axioms
                .push(tm.mk_term(cvc5::Kind::Geq, &[res_len, zero]));
            Some(result)
        }
        KnownBuiltin::Concat => {
            let (l, r) = (&args[0], &args[1]);
            let result = fresh_int_cvc5(tm, state);
            let len_func = field_len_fn_cvc5(tm, state);
            let len_l = tm.mk_term(cvc5::Kind::ApplyUf, &[len_func.clone(), l.clone()]);
            let len_r = tm.mk_term(cvc5::Kind::ApplyUf, &[len_func.clone(), r.clone()]);
            let len_result = tm.mk_term(cvc5::Kind::ApplyUf, &[len_func, result.clone()]);
            let zero = tm.mk_integer(0);
            state
                .axioms
                .push(tm.mk_term(cvc5::Kind::Geq, &[len_l.clone(), zero.clone()]));
            state
                .axioms
                .push(tm.mk_term(cvc5::Kind::Geq, &[len_r.clone(), zero.clone()]));
            let sum = tm.mk_term(cvc5::Kind::Add, &[len_l, len_r]);
            state
                .axioms
                .push(tm.mk_term(cvc5::Kind::Equal, &[len_result.clone(), sum]));
            state
                .axioms
                .push(tm.mk_term(cvc5::Kind::Geq, &[len_result, zero]));
            Some(result)
        }
        KnownBuiltin::IndexOf => {
            let str_val = &args[0];
            let result = fresh_int_cvc5(tm, state);
            let neg_one = tm.mk_integer(-1);
            state
                .axioms
                .push(tm.mk_term(cvc5::Kind::Geq, &[result.clone(), neg_one]));
            let str_len = field_len_of_cvc5(tm, state, str_val);
            state
                .axioms
                .push(tm.mk_term(cvc5::Kind::Lt, &[result.clone(), str_len]));
            Some(result)
        }
        KnownBuiltin::CharAt => {
            let str_val = &args[0];
            let idx = &args[1];
            let zero = tm.mk_integer(0);
            state
                .axioms
                .push(tm.mk_term(cvc5::Kind::Geq, &[idx.clone(), zero]));
            let str_len = field_len_of_cvc5(tm, state, str_val);
            state
                .axioms
                .push(tm.mk_term(cvc5::Kind::Lt, &[idx.clone(), str_len]));
            Some(fresh_int_cvc5(tm, state))
        }
        KnownBuiltin::Replace => {
            let result = fresh_int_cvc5(tm, state);
            let res_len = field_len_of_cvc5(tm, state, &result);
            let zero = tm.mk_integer(0);
            state
                .axioms
                .push(tm.mk_term(cvc5::Kind::Geq, &[res_len, zero]));
            Some(result)
        }
        KnownBuiltin::Split => {
            let result = fresh_int_cvc5(tm, state);
            let res_len = field_len_of_cvc5(tm, state, &result);
            let one = tm.mk_integer(1);
            state
                .axioms
                .push(tm.mk_term(cvc5::Kind::Geq, &[res_len, one]));
            Some(result)
        }
        KnownBuiltin::Trim => {
            let str_val = &args[0];
            let result = fresh_int_cvc5(tm, state);
            let len_func = field_len_fn_cvc5(tm, state);
            let str_len = tm.mk_term(cvc5::Kind::ApplyUf, &[len_func.clone(), str_val.clone()]);
            let res_len = tm.mk_term(cvc5::Kind::ApplyUf, &[len_func, result.clone()]);
            let zero = tm.mk_integer(0);
            state
                .axioms
                .push(tm.mk_term(cvc5::Kind::Geq, &[res_len.clone(), zero]));
            state
                .axioms
                .push(tm.mk_term(cvc5::Kind::Leq, &[res_len, str_len]));
            Some(result)
        }
        KnownBuiltin::Set => {
            let arr = &args[0];
            let i = &args[1];
            let v = &args[2];
            let result = fresh_int_cvc5(tm, state);
            let get_sort =
                tm.mk_fun_sort(&[tm.integer_sort(), tm.integer_sort()], tm.integer_sort());
            let get_func = tm.mk_const(get_sort, "get");
            let get_result_i =
                tm.mk_term(cvc5::Kind::ApplyUf, &[get_func, result.clone(), i.clone()]);
            state
                .axioms
                .push(tm.mk_term(cvc5::Kind::Equal, &[get_result_i, v.clone()]));
            let len_func = field_len_fn_cvc5(tm, state);
            let len_result = tm.mk_term(cvc5::Kind::ApplyUf, &[len_func.clone(), result.clone()]);
            let len_arr = tm.mk_term(cvc5::Kind::ApplyUf, &[len_func, arr.clone()]);
            state
                .axioms
                .push(tm.mk_term(cvc5::Kind::Equal, &[len_result.clone(), len_arr]));
            let zero = tm.mk_integer(0);
            state
                .axioms
                .push(tm.mk_term(cvc5::Kind::Geq, &[len_result, zero]));
            Some(result)
        }
        KnownBuiltin::Put => {
            let map = &args[0];
            let k = &args[1];
            let v = &args[2];
            let result = fresh_int_cvc5(tm, state);
            let get_sort =
                tm.mk_fun_sort(&[tm.integer_sort(), tm.integer_sort()], tm.integer_sort());
            let get_func = tm.mk_const(get_sort, "get");
            let get_result_k =
                tm.mk_term(cvc5::Kind::ApplyUf, &[get_func, result.clone(), k.clone()]);
            state
                .axioms
                .push(tm.mk_term(cvc5::Kind::Equal, &[get_result_k, v.clone()]));
            let size_sort = tm.mk_fun_sort(&[tm.integer_sort()], tm.integer_sort());
            let size_func = tm.mk_const(size_sort, "size");
            let size_result = tm.mk_term(cvc5::Kind::ApplyUf, &[size_func.clone(), result.clone()]);
            let size_map = tm.mk_term(cvc5::Kind::ApplyUf, &[size_func, map.clone()]);
            state
                .axioms
                .push(tm.mk_term(cvc5::Kind::Geq, &[size_result.clone(), size_map]));
            let zero = tm.mk_integer(0);
            state
                .axioms
                .push(tm.mk_term(cvc5::Kind::Geq, &[size_result, zero]));
            Some(result)
        }
    }
}

#[cfg(feature = "cvc5-verify")]
fn encode_uf_call_cvc5<'a>(
    tm: &'a cvc5::TermManager,
    f_name: &str,
    encoded_args: &[cvc5::Term<'a>],
    state: &mut Cvc5EncoderState<'a>,
) -> Option<cvc5::Term<'a>> {
    if is_bool_returning_uf(f_name) {
        return Some(apply_int_uf_cvc5(tm, f_name, encoded_args, true));
    }
    if state.use_string_theory
        && matches!(f_name, "len" | "length")
        && encoded_args.len() == 1
        && encoded_args[0].sort().is_string()
    {
        let len = tm.mk_term(cvc5::Kind::StringLength, &[encoded_args[0].clone()]);
        let zero = tm.mk_integer(0);
        state
            .axioms
            .push(tm.mk_term(cvc5::Kind::Geq, &[len.clone(), zero]));
        return Some(len);
    }
    if matches!(f_name, "len" | "length" | "size" | "count" | "capacity") {
        let result = apply_int_uf_cvc5(tm, f_name, encoded_args, false);
        let zero = tm.mk_integer(0);
        state
            .axioms
            .push(tm.mk_term(cvc5::Kind::Geq, &[result.clone(), zero]));
        return Some(result);
    }
    Some(apply_int_uf_cvc5(tm, f_name, encoded_args, false))
}

#[cfg(feature = "cvc5-verify")]
fn encode_length_receiver_cvc5<'a>(
    tm: &'a cvc5::TermManager,
    receiver: &Expr,
    vars: &mut HashMap<String, cvc5::Term<'a>>,
    state: &mut Cvc5EncoderState<'a>,
) -> Option<cvc5::Term<'a>> {
    if let Expr::Ident(name) = receiver {
        return Some(canonical_length_cvc5(tm, name, vars, state));
    }
    let recv_val = encode_expr_cvc5(tm, receiver, vars, state)?;
    let len = field_len_of_cvc5(tm, state, &recv_val);
    let zero = tm.mk_integer(0);
    state
        .axioms
        .push(tm.mk_term(cvc5::Kind::Geq, &[len.clone(), zero]));
    Some(len)
}

/// Encode an AST expression as a CVC5 Term using the native API.
///
/// `state` collects background axioms and tracks string constants
/// so that `check_clause_cvc5_native` can assert them before check_sat.
#[cfg(feature = "cvc5-verify")]
pub(crate) fn encode_expr_cvc5<'a>(
    tm: &'a cvc5::TermManager,
    expr: &Expr,
    vars: &mut HashMap<String, cvc5::Term<'a>>,
    state: &mut Cvc5EncoderState<'a>,
) -> Option<cvc5::Term<'a>> {
    match expr {
        Expr::Literal(Literal::Int(n)) => {
            let val: i64 = n.parse().ok()?;
            Some(tm.mk_integer(val))
        }
        Expr::Literal(Literal::Bool(b)) => Some(tm.mk_boolean(*b)),
        Expr::Literal(Literal::Float(f_str)) => {
            // Rational approximation matching Z3 backend (Real sort)
            let f: f64 = f_str.parse().unwrap_or(0.0);
            let denom = 1_000_000i64;
            let numer = (f * denom as f64) as i64;
            Some(tm.mk_real_from_rational(numer, denom))
        }
        Expr::Literal(Literal::Str(s)) => {
            if state.use_string_theory {
                // Native CVC5 string theory: use string_sort and mk_string.
                // CVC5 handles equality, length, and distinctness natively.
                let str_val = tm.mk_string(s, false);
                // Background axiom: length is known at compile time
                let len = tm.mk_term(cvc5::Kind::StringLength, &[str_val.clone()]);
                let expected_len = tm.mk_integer(s.len() as i64);
                let len_eq = tm.mk_term(cvc5::Kind::Equal, &[len, expected_len]);
                state.axioms.push(len_eq);
                Some(str_val)
            } else {
                // Integer encoding (default): named integer constant matching Z3 pattern
                let const_name = format!("__str_{s}");
                let str_val = tm.mk_const(tm.integer_sort(), &const_name);
                // Pairwise distinctness from previously seen string constants
                if !state.string_constants.contains(&const_name) {
                    for prev in &state.string_constants {
                        let prev_val = tm.mk_const(tm.integer_sort(), prev);
                        let eq = tm.mk_term(cvc5::Kind::Equal, &[str_val.clone(), prev_val]);
                        let neq = tm.mk_term(cvc5::Kind::Not, &[eq]);
                        state.axioms.push(neq);
                    }
                    state.string_constants.push(const_name);
                }
                // String length axiom: len("hello") == 5
                let len_name = "__field_len";
                let len_sort = tm.mk_fun_sort(&[tm.integer_sort()], tm.integer_sort());
                let len_func = tm.mk_const(len_sort, len_name);
                let len_result = tm.mk_term(cvc5::Kind::ApplyUf, &[len_func, str_val.clone()]);
                let str_len = tm.mk_integer(s.len() as i64);
                let len_eq = tm.mk_term(cvc5::Kind::Equal, &[len_result, str_len]);
                state.axioms.push(len_eq);
                Some(str_val)
            }
        }
        Expr::Ident(name) => {
            let key = if name == "result" {
                "__result".to_string()
            } else {
                sanitize_smtlib_name(name)
            };
            vars.get(&key)
                .cloned()
                .or_else(|| Some(tm.mk_const(tm.integer_sort(), &key)))
        }
        Expr::BinOp { op, lhs, rhs } => {
            let l = encode_expr_cvc5(tm, lhs, vars, state)?;
            let r = encode_expr_cvc5(tm, rhs, vars, state)?;
            let kind = match op {
                BinOp::Add => cvc5::Kind::Add,
                BinOp::Sub => cvc5::Kind::Sub,
                BinOp::Mul => cvc5::Kind::Mult,
                BinOp::Div => cvc5::Kind::IntsDivision,
                BinOp::Mod => cvc5::Kind::IntsModulus,
                BinOp::Eq => cvc5::Kind::Equal,
                BinOp::Neq => {
                    let eq = tm.mk_term(cvc5::Kind::Equal, &[l, r]);
                    return Some(tm.mk_term(cvc5::Kind::Not, &[eq]));
                }
                BinOp::Lt => cvc5::Kind::Lt,
                BinOp::Lte => cvc5::Kind::Leq,
                BinOp::Gt => cvc5::Kind::Gt,
                BinOp::Gte => cvc5::Kind::Geq,
                BinOp::And => cvc5::Kind::And,
                BinOp::Or => cvc5::Kind::Or,
                BinOp::Implies => cvc5::Kind::Implies,
                BinOp::Range => {
                    // Range (a..b): create a fresh Int constrained to [lhs, rhs)
                    let fresh_name = format!("__fresh_{}", state.fresh_counter);
                    state.fresh_counter += 1;
                    let fresh = tm.mk_const(tm.integer_sort(), &fresh_name);
                    let ge_lo = tm.mk_term(cvc5::Kind::Geq, &[fresh.clone(), l]);
                    let lt_hi = tm.mk_term(cvc5::Kind::Lt, &[fresh.clone(), r]);
                    let in_range = tm.mk_term(cvc5::Kind::And, &[ge_lo, lt_hi]);
                    state.axioms.push(in_range);
                    return Some(fresh);
                }
                BinOp::In => {
                    // In (elem in collection): UF __contains(collection, elem) -> Bool
                    let func_sort =
                        tm.mk_fun_sort(&[tm.integer_sort(), tm.integer_sort()], tm.boolean_sort());
                    let contains = tm.mk_const(func_sort, "__contains");
                    return Some(tm.mk_term(cvc5::Kind::ApplyUf, &[contains, r, l]));
                }
                BinOp::NotIn => {
                    // NotIn: negation of In
                    let func_sort =
                        tm.mk_fun_sort(&[tm.integer_sort(), tm.integer_sort()], tm.boolean_sort());
                    let contains = tm.mk_const(func_sort, "__contains");
                    let in_result = tm.mk_term(cvc5::Kind::ApplyUf, &[contains, r, l]);
                    return Some(tm.mk_term(cvc5::Kind::Not, &[in_result]));
                }
                BinOp::Concat => {
                    // Concat (a ++ b): fresh value with length axiom
                    let fresh_name = format!("__fresh_{}", state.fresh_counter);
                    state.fresh_counter += 1;
                    let result = tm.mk_const(tm.integer_sort(), &fresh_name);
                    let len_func = field_len_fn_cvc5(tm, state);
                    let len_l = tm.mk_term(cvc5::Kind::ApplyUf, &[len_func.clone(), l]);
                    let len_r = tm.mk_term(cvc5::Kind::ApplyUf, &[len_func.clone(), r]);
                    let len_result = tm.mk_term(cvc5::Kind::ApplyUf, &[len_func, result.clone()]);
                    let sum = tm.mk_term(cvc5::Kind::Add, &[len_l.clone(), len_r.clone()]);
                    let len_eq = tm.mk_term(cvc5::Kind::Equal, &[len_result.clone(), sum]);
                    state.axioms.push(len_eq);
                    let zero = tm.mk_integer(0);
                    state
                        .axioms
                        .push(tm.mk_term(cvc5::Kind::Geq, &[len_l, zero.clone()]));
                    state
                        .axioms
                        .push(tm.mk_term(cvc5::Kind::Geq, &[len_r, zero.clone()]));
                    state
                        .axioms
                        .push(tm.mk_term(cvc5::Kind::Geq, &[len_result, zero]));
                    return Some(result);
                }
            };
            Some(tm.mk_term(kind, &[l, r]))
        }
        Expr::UnaryOp { op, expr: inner } => {
            let e = encode_expr_cvc5(tm, inner, vars, state)?;
            match op {
                UnaryOp::Not => Some(tm.mk_term(cvc5::Kind::Not, &[e])),
                UnaryOp::Neg => Some(tm.mk_term(cvc5::Kind::Neg, &[e])),
            }
        }
        Expr::If {
            cond,
            then_branch,
            else_branch,
        } => {
            let c = encode_expr_cvc5(tm, cond, vars, state)?;
            let t = encode_expr_cvc5(tm, then_branch, vars, state)?;
            if let Some(eb) = else_branch {
                let e = encode_expr_cvc5(tm, eb, vars, state)?;
                // Sort promotion: if one branch is Real and the other Integer, promote
                let (t_final, e_final) = if t.sort().is_real() && e.sort().is_integer() {
                    (t, tm.mk_term(cvc5::Kind::ToReal, &[e]))
                } else if t.sort().is_integer() && e.sort().is_real() {
                    (tm.mk_term(cvc5::Kind::ToReal, &[t]), e)
                } else {
                    (t, e)
                };
                Some(tm.mk_term(cvc5::Kind::Ite, &[c, t_final, e_final]))
            } else {
                Some(tm.mk_term(cvc5::Kind::Implies, &[c, t]))
            }
        }
        Expr::Forall { var, domain, body } => {
            let v_name = sanitize_smtlib_name(var);
            let bound_var = tm.mk_var(tm.integer_sort(), &v_name);
            let mut local_vars = vars.clone();
            local_vars.insert(v_name.clone(), bound_var.clone());
            let b = encode_expr_cvc5(tm, body, &mut local_vars, state)?;
            let guarded = guard_quantifier_body_cvc5(tm, domain, &bound_var, b, true, vars, state);
            let bound_list = tm.mk_term(cvc5::Kind::VariableList, &[bound_var.clone()]);
            let trigger_terms = infer_quantifier_patterns_cvc5(tm, body, &v_name, &bound_var);
            if trigger_terms.is_empty() {
                Some(tm.mk_term(cvc5::Kind::Forall, &[bound_list, guarded]))
            } else {
                let inst_pattern = tm.mk_term(cvc5::Kind::InstPattern, &trigger_terms);
                Some(tm.mk_term(cvc5::Kind::Forall, &[bound_list, guarded, inst_pattern]))
            }
        }
        Expr::Exists { var, domain, body } => {
            let v_name = sanitize_smtlib_name(var);
            let bound_var = tm.mk_var(tm.integer_sort(), &v_name);
            let mut local_vars = vars.clone();
            local_vars.insert(v_name.clone(), bound_var.clone());
            let b = encode_expr_cvc5(tm, body, &mut local_vars, state)?;
            let guarded = guard_quantifier_body_cvc5(tm, domain, &bound_var, b, false, vars, state);
            let bound_list = tm.mk_term(cvc5::Kind::VariableList, &[bound_var.clone()]);
            let trigger_terms = infer_quantifier_patterns_cvc5(tm, body, &v_name, &bound_var);
            if trigger_terms.is_empty() {
                Some(tm.mk_term(cvc5::Kind::Exists, &[bound_list, guarded]))
            } else {
                let inst_pattern = tm.mk_term(cvc5::Kind::InstPattern, &trigger_terms);
                Some(tm.mk_term(cvc5::Kind::Exists, &[bound_list, guarded, inst_pattern]))
            }
        }
        Expr::Call { func, args } => {
            if let Expr::Ident(name) = func.as_ref() {
                let f_name = sanitize_smtlib_name(name);
                if args.is_empty() {
                    return vars
                        .get(&f_name)
                        .cloned()
                        .or_else(|| Some(tm.mk_const(tm.integer_sort(), &f_name)));
                }
                let encoded_args: Option<Vec<cvc5::Term>> = args
                    .iter()
                    .map(|a| encode_expr_cvc5(tm, a, vars, state))
                    .collect();
                let encoded_args = encoded_args?;
                if let Some(term) =
                    encode_known_builtin_cvc5(tm, f_name.as_str(), &encoded_args, state)
                {
                    return Some(term);
                }
                encode_uf_call_cvc5(tm, &f_name, &encoded_args, state)
            } else {
                None
            }
        }
        // old(expr): add __old suffix for Ident, recurse for Field/MethodCall
        Expr::Old(inner) => match inner.as_ref() {
            Expr::Ident(name) => {
                let old_name = format!("{name}__old");
                let key = sanitize_smtlib_name(&old_name);
                Some(
                    vars.get(&key)
                        .cloned()
                        .unwrap_or_else(|| tm.mk_const(tm.integer_sort(), &key)),
                )
            }
            Expr::Field(obj, field) => {
                // Deep chain flattening for old(a.b.c) -> a__b__c__old (#250)
                let full_expr = Expr::Field(obj.clone(), field.clone());
                if has_deep_field_chain_cvc5(&full_expr) || is_self_rooted_cvc5(obj) {
                    let flat_name = flatten_field_chain_cvc5(&full_expr);
                    return Some(tm.mk_const(tm.integer_sort(), &format!("{flat_name}__old")));
                }
                let old_obj = encode_expr_cvc5(tm, &Expr::Old(obj.clone()), vars, state)?;
                let func_name = format!("__field_{field}");
                let func_sort = tm.mk_fun_sort(&[tm.integer_sort()], tm.integer_sort());
                let func_const = tm.mk_const(func_sort, &func_name);
                Some(tm.mk_term(cvc5::Kind::ApplyUf, &[func_const, old_obj]))
            }
            Expr::MethodCall {
                receiver, method, ..
            } => {
                let old_recv = encode_expr_cvc5(tm, &Expr::Old(receiver.clone()), vars, state)?;
                let func_sort = tm.mk_fun_sort(&[tm.integer_sort()], tm.integer_sort());
                let func_const = tm.mk_const(func_sort, method);
                Some(tm.mk_term(cvc5::Kind::ApplyUf, &[func_const, old_recv]))
            }
            _ => encode_expr_cvc5(tm, inner, vars, state),
        },
        Expr::Paren(inner) | Expr::Ghost(inner) => encode_expr_cvc5(tm, inner, vars, state),
        Expr::Cast { expr: inner, .. } => encode_expr_cvc5(tm, inner, vars, state),
        Expr::Let {
            name, value, body, ..
        } => {
            let v = encode_expr_cvc5(tm, value, vars, state)?;
            let mut local_vars = vars.clone();
            local_vars.insert(sanitize_smtlib_name(name), v);
            encode_expr_cvc5(tm, body, &mut local_vars, state)
        }
        Expr::Match {
            scrutinee, arms, ..
        } => {
            if arms.is_empty() {
                return None;
            }
            let s = encode_expr_cvc5(tm, scrutinee, vars, state)?;
            let mut result: Option<cvc5::Term> = None;
            for arm in arms.iter().rev() {
                match &arm.pattern {
                    Pattern::Wildcard => {
                        let body = encode_expr_cvc5(tm, &arm.body, vars, state)?;
                        result = Some(body);
                    }
                    Pattern::Ident(name) => {
                        // Bind the name as a fresh variable
                        let mut local_vars = vars.clone();
                        bind_pattern_vars_cvc5(tm, &arm.pattern, &mut local_vars);
                        let body = encode_expr_cvc5(tm, &arm.body, &mut local_vars, state)?;
                        // Uppercase-initial ident = constructor name -> hash match
                        if name.starts_with(|c: char| c.is_uppercase()) {
                            let tag_hash = pattern_hash_name(name);
                            let tag_val = tm.mk_integer(tag_hash);
                            let cond = tm.mk_term(cvc5::Kind::Equal, &[s.clone(), tag_val]);
                            if let Some(default) = result.as_ref() {
                                result = Some(
                                    tm.mk_term(cvc5::Kind::Ite, &[cond, body, default.clone()]),
                                );
                            } else {
                                result = Some(body);
                            }
                        } else {
                            // Lowercase ident = variable binding = catch-all
                            result = Some(body);
                        }
                    }
                    Pattern::Literal(lit) => {
                        let body = encode_expr_cvc5(tm, &arm.body, vars, state)?;
                        let lit_term = match lit {
                            Literal::Int(n) => {
                                let val: i64 = n.parse().ok()?;
                                tm.mk_integer(val)
                            }
                            Literal::Bool(b) => tm.mk_boolean(*b),
                            _ => return None,
                        };
                        let default = result.as_ref()?.clone();
                        let cond = tm.mk_term(cvc5::Kind::Equal, &[s.clone(), lit_term]);
                        result = Some(tm.mk_term(cvc5::Kind::Ite, &[cond, body, default]));
                    }
                    Pattern::Constructor { name, fields } => {
                        // Hash-based tag matching (same as Z3 backend)
                        let tag_hash = pattern_hash_name(name);
                        let tag_val = tm.mk_integer(tag_hash);
                        let cond = tm.mk_term(cvc5::Kind::Equal, &[s.clone(), tag_val]);
                        // Bind field variables as fresh integer constants
                        let mut local_vars = vars.clone();
                        for field in fields {
                            bind_pattern_vars_cvc5(tm, field, &mut local_vars);
                        }
                        let body = encode_expr_cvc5(tm, &arm.body, &mut local_vars, state)?;
                        let default = result.as_ref()?.clone();
                        result = Some(tm.mk_term(cvc5::Kind::Ite, &[cond, body, default]));
                    }
                    Pattern::Tuple(pats) => {
                        // Bind each tuple element as a fresh variable
                        let mut local_vars = vars.clone();
                        for pat in pats {
                            bind_pattern_vars_cvc5(tm, pat, &mut local_vars);
                        }
                        let body = encode_expr_cvc5(tm, &arm.body, &mut local_vars, state)?;
                        // Tuple match is structural (always matches)
                        result = Some(body);
                    }
                }
            }
            result
        }
        // Field access: flatten deep chains or self-rooted, else UF
        Expr::Field(obj, field) => {
            if matches!(field.as_str(), "len" | "length")
                && let Expr::Ident(name) = obj.as_ref()
            {
                return Some(canonical_length_cvc5(tm, name, vars, state));
            }

            // Deep field chain flattening (#250): state.head.extra.max -> state__head__extra__max
            let full_expr = Expr::Field(Box::new(obj.as_ref().clone()), field.clone());
            if has_deep_field_chain_cvc5(&full_expr) || is_self_rooted_cvc5(obj) {
                let flat_name = flatten_field_chain_cvc5(&full_expr);
                // Boolean-valued fields at any depth
                if is_bool_field(field) {
                    return Some(tm.mk_const(tm.boolean_sort(), &flat_name));
                }
                // Size fields at any depth get non-negativity axiom
                if is_size_field(field) {
                    let v = get_or_create_int_cvc5(tm, &flat_name, vars);
                    let zero = tm.mk_integer(0);
                    state
                        .axioms
                        .push(tm.mk_term(cvc5::Kind::Geq, &[v.clone(), zero]));
                    return Some(v);
                }
                // General field: Integer variable
                return Some(get_or_create_int_cvc5(tm, &flat_name, vars));
            }
            // Shallow field access: UF __field_name(receiver)
            let obj_val = encode_expr_cvc5(tm, obj, vars, state)?;

            // Native string theory: .length() on a string-sorted term uses StringLength
            if state.use_string_theory
                && matches!(field.as_str(), "len" | "length")
                && obj_val.sort().is_string()
            {
                let len = tm.mk_term(cvc5::Kind::StringLength, &[obj_val]);
                let zero = tm.mk_integer(0);
                state
                    .axioms
                    .push(tm.mk_term(cvc5::Kind::Geq, &[len.clone(), zero]));
                return Some(len);
            }

            let func_name = format!("__field_{field}");
            // Boolean fields return Bool sort
            if is_bool_field(field) {
                let func_sort = tm.mk_fun_sort(&[tm.integer_sort()], tm.boolean_sort());
                let func_const = tm.mk_const(func_sort, &func_name);
                return Some(tm.mk_term(cvc5::Kind::ApplyUf, &[func_const, obj_val]));
            }
            // Size fields get non-negativity axiom
            if is_size_field(field) {
                let func_sort = tm.mk_fun_sort(&[tm.integer_sort()], tm.integer_sort());
                let func_const = tm.mk_const(func_sort, &func_name);
                let result = tm.mk_term(cvc5::Kind::ApplyUf, &[func_const, obj_val]);
                let zero = tm.mk_integer(0);
                state
                    .axioms
                    .push(tm.mk_term(cvc5::Kind::Geq, &[result.clone(), zero]));
                return Some(result);
            }
            let func_sort = tm.mk_fun_sort(&[tm.integer_sort()], tm.integer_sort());
            let func_const = tm.mk_const(func_sort, &func_name);
            Some(tm.mk_term(cvc5::Kind::ApplyUf, &[func_const, obj_val]))
        }
        // Index: UF __index(collection, index) with bounds axioms
        Expr::Index { expr: coll, index } => {
            let coll_val = encode_expr_cvc5(tm, coll, vars, state)?;
            let idx_val = encode_expr_cvc5(tm, index, vars, state)?;
            let zero = tm.mk_integer(0);
            // 0 <= index
            state
                .axioms
                .push(tm.mk_term(cvc5::Kind::Geq, &[idx_val.clone(), zero.clone()]));
            // len(collection) via UF
            let len_sort = tm.mk_fun_sort(&[tm.integer_sort()], tm.integer_sort());
            let len_func = tm.mk_const(len_sort, "__len");
            let len_val = tm.mk_term(cvc5::Kind::ApplyUf, &[len_func, coll_val.clone()]);
            // len >= 0
            state
                .axioms
                .push(tm.mk_term(cvc5::Kind::Geq, &[len_val.clone(), zero]));
            // index < len
            state
                .axioms
                .push(tm.mk_term(cvc5::Kind::Lt, &[idx_val.clone(), len_val]));
            // UF __index(coll, idx)
            let idx_sort =
                tm.mk_fun_sort(&[tm.integer_sort(), tm.integer_sort()], tm.integer_sort());
            let idx_func = tm.mk_const(idx_sort, "__index");
            Some(tm.mk_term(cvc5::Kind::ApplyUf, &[idx_func, coll_val, idx_val]))
        }
        // Block: encode all expressions, return last
        Expr::Block(body) => {
            if body.is_empty() {
                return Some(tm.mk_boolean(true));
            }
            let mut result = None;
            for e in body {
                result = encode_expr_cvc5(tm, e, vars, state);
            }
            result
        }
        // Raw tokens: basic parsing (single token bools/ints/idents)
        Expr::Raw(tokens) => {
            if tokens.is_empty() {
                return Some(tm.mk_boolean(true));
            }
            if tokens.len() == 1 {
                let t = &tokens[0];
                if t == "true" {
                    return Some(tm.mk_boolean(true));
                }
                if t == "false" {
                    return Some(tm.mk_boolean(false));
                }
                if let Ok(n) = t.parse::<i64>() {
                    return Some(tm.mk_integer(n));
                }
                let key = sanitize_smtlib_name(t);
                return vars
                    .get(&key)
                    .cloned()
                    .or_else(|| Some(tm.mk_const(tm.integer_sort(), &key)));
            }
            // Multi-token: try to parse as infix expression
            encode_raw_tokens_cvc5(tm, tokens, vars, state)
        }
        // Tuple: fresh Int with element-access axioms
        Expr::Tuple(elems) => {
            let tuple_name = format!("__tuple_{}", state.fresh_counter);
            state.fresh_counter += 1;
            let tuple_val = tm.mk_const(tm.integer_sort(), &tuple_name);
            let arity = elems.len();
            for (i, elem) in elems.iter().enumerate() {
                if let Some(elem_val) = encode_expr_cvc5(tm, elem, vars, state) {
                    let accessor_name = format!("__tuple_{arity}_{i}");
                    let acc_sort = tm.mk_fun_sort(&[tm.integer_sort()], tm.integer_sort());
                    let acc_func = tm.mk_const(acc_sort, &accessor_name);
                    let accessed = tm.mk_term(cvc5::Kind::ApplyUf, &[acc_func, tuple_val.clone()]);
                    state
                        .axioms
                        .push(tm.mk_term(cvc5::Kind::Equal, &[accessed, elem_val]));
                }
            }
            Some(tuple_val)
        }
        // MethodCall: prepend receiver, call UF
        Expr::MethodCall {
            receiver,
            method,
            args,
        } => {
            if matches!(method.as_str(), "length" | "len") && args.is_empty() {
                return encode_length_receiver_cvc5(tm, receiver, vars, state);
            }

            let recv_val = encode_expr_cvc5(tm, receiver, vars, state)?;
            let mut all_encoded = vec![recv_val];
            for arg in args {
                all_encoded.push(encode_expr_cvc5(tm, arg, vars, state)?);
            }
            let f_name = sanitize_smtlib_name(method);
            if let Some(term) = encode_known_builtin_cvc5(tm, f_name.as_str(), &all_encoded, state)
            {
                return Some(term);
            }
            encode_uf_call_cvc5(tm, &f_name, &all_encoded, state)
        }
        // List: fresh Int with element-access and length axioms
        Expr::List(elems) => {
            let list_name = format!("__list_{}", state.fresh_counter);
            state.fresh_counter += 1;
            let list_val = tm.mk_const(tm.integer_sort(), &list_name);
            let get_sort =
                tm.mk_fun_sort(&[tm.integer_sort(), tm.integer_sort()], tm.integer_sort());
            let get_func = tm.mk_const(get_sort, "__list_get");
            for (i, elem) in elems.iter().enumerate() {
                if let Some(elem_val) = encode_expr_cvc5(tm, elem, vars, state) {
                    let idx = tm.mk_integer(i as i64);
                    let accessed = tm.mk_term(
                        cvc5::Kind::ApplyUf,
                        &[get_func.clone(), list_val.clone(), idx],
                    );
                    state
                        .axioms
                        .push(tm.mk_term(cvc5::Kind::Equal, &[accessed, elem_val]));
                }
            }
            // Assert length
            let len_func = field_len_fn_cvc5(tm, state);
            let len_result = tm.mk_term(cvc5::Kind::ApplyUf, &[len_func, list_val.clone()]);
            let expected_len = tm.mk_integer(elems.len() as i64);
            state
                .axioms
                .push(tm.mk_term(cvc5::Kind::Equal, &[len_result, expected_len]));
            Some(list_val)
        }
        // Apply: encode args for side effects, return named bool
        Expr::Apply { lemma_name, args } => {
            for arg in args {
                let _ = encode_expr_cvc5(tm, arg, vars, state);
            }
            let apply_name = format!("__apply_{lemma_name}");
            Some(tm.mk_const(tm.boolean_sort(), &apply_name))
        }
    }
}

/// Build a domain guard for quantifier bodies (CVC5 native API).
///
/// For range domains (`lo..hi`):
/// - `is_forall=true`:  `(lo <= x && x < hi) => body`
/// - `is_forall=false`: `(lo <= x && x < hi) && body`
///
/// For non-range domains (collections, identifiers), encode
/// membership as an uninterpreted `__domain_contains(domain, x)` predicate.
#[cfg(feature = "cvc5-verify")]
fn guard_quantifier_body_cvc5<'a>(
    tm: &'a cvc5::TermManager,
    domain: &Expr,
    bound_var: &cvc5::Term<'a>,
    body: cvc5::Term<'a>,
    is_forall: bool,
    outer_vars: &mut HashMap<String, cvc5::Term<'a>>,
    state: &mut Cvc5EncoderState<'a>,
) -> cvc5::Term<'a> {
    if let Expr::BinOp {
        op: BinOp::Range,
        lhs: lo,
        rhs: hi,
    } = domain
    {
        // Range domain: lo <= x && x < hi
        let lo_val =
            encode_expr_cvc5(tm, lo, outer_vars, state).unwrap_or_else(|| tm.mk_integer(0));
        let hi_val =
            encode_expr_cvc5(tm, hi, outer_vars, state).unwrap_or_else(|| tm.mk_integer(0));
        let ge_lo = tm.mk_term(cvc5::Kind::Geq, &[bound_var.clone(), lo_val]);
        let lt_hi = tm.mk_term(cvc5::Kind::Lt, &[bound_var.clone(), hi_val]);
        let in_range = tm.mk_term(cvc5::Kind::And, &[ge_lo, lt_hi]);
        if is_forall {
            tm.mk_term(cvc5::Kind::Implies, &[in_range, body])
        } else {
            tm.mk_term(cvc5::Kind::And, &[in_range, body])
        }
    } else {
        // Non-range domain: __domain_contains(domain, x) UF
        let domain_val = encode_expr_cvc5(tm, domain, outer_vars, state)
            .unwrap_or_else(|| tm.mk_const(tm.integer_sort(), "__domain_unknown"));
        let contains_sort =
            tm.mk_fun_sort(&[tm.integer_sort(), tm.integer_sort()], tm.boolean_sort());
        let contains_fn = tm.mk_const(contains_sort, "__domain_contains");
        let membership = tm.mk_term(
            cvc5::Kind::ApplyUf,
            &[contains_fn, domain_val, bound_var.clone()],
        );
        if is_forall {
            tm.mk_term(cvc5::Kind::Implies, &[membership, body])
        } else {
            tm.mk_term(cvc5::Kind::And, &[membership, body])
        }
    }
}

/// Infer CVC5 trigger patterns from function calls in a quantifier body
/// that reference the bound variable. Returns `InstPattern` terms for
/// e-matching hints that help the solver instantiate quantifiers efficiently.
///
/// First checks the `TriggerManager` for user-provided triggers, then falls
/// back to scanning the body for `Expr::Call` expressions referencing the
/// bound variable.
#[cfg(feature = "cvc5-verify")]
pub(crate) fn infer_quantifier_patterns_cvc5<'a>(
    tm: &'a cvc5::TermManager,
    body: &Expr,
    bound_var_name: &str,
    bound_cvc5: &cvc5::Term<'a>,
) -> Vec<cvc5::Term<'a>> {
    let mut patterns = Vec::new();

    // Check TriggerManager for user-provided or inferred triggers
    let trigger_mgr = crate::advanced::TriggerManager::new();
    let body_str = format!("{body:?}");
    if let Some(trigger) = trigger_mgr.infer_trigger(&body_str) {
        for term in &trigger.terms {
            if let Some(fname) = term.split('(').next() {
                let fname = fname.trim();
                let fun_sort = tm.mk_fun_sort(&[tm.integer_sort()], tm.integer_sort());
                let func = tm.mk_const(fun_sort, fname);
                let app = tm.mk_term(cvc5::Kind::ApplyUf, &[func, bound_cvc5.clone()]);
                patterns.push(app);
            }
        }
    }

    // Direct scan: look for Call expressions that reference the bound variable
    if patterns.is_empty() {
        collect_trigger_calls_cvc5(tm, body, bound_var_name, bound_cvc5, &mut patterns);
    }

    patterns
}

/// Recursively scan an expression for function calls containing the
/// bound variable, and create CVC5 trigger terms from them.
#[cfg(feature = "cvc5-verify")]
fn collect_trigger_calls_cvc5<'a>(
    tm: &'a cvc5::TermManager,
    expr: &Expr,
    bound_var: &str,
    bound_cvc5: &cvc5::Term<'a>,
    patterns: &mut Vec<cvc5::Term<'a>>,
) {
    match expr {
        Expr::Call { func, args } => {
            let refs_bound = args.iter().any(|a| expr_references_var(a, bound_var));
            if refs_bound {
                if let Expr::Ident(fname) = func.as_ref() {
                    let arity = args.len();
                    let param_sorts: Vec<cvc5::Sort> =
                        (0..arity).map(|_| tm.integer_sort()).collect();
                    let fun_sort = tm.mk_fun_sort(&param_sorts, tm.integer_sort());
                    let func_decl = tm.mk_const(fun_sort, fname.as_str());
                    let mut uf_args = vec![func_decl];
                    for a in args {
                        if expr_references_var(a, bound_var) {
                            uf_args.push(bound_cvc5.clone());
                        } else {
                            uf_args.push(tm.mk_const(tm.integer_sort(), "__trigger_other"));
                        }
                    }
                    let app = tm.mk_term(cvc5::Kind::ApplyUf, &uf_args);
                    patterns.push(app);
                }
            }
            for a in args {
                collect_trigger_calls_cvc5(tm, a, bound_var, bound_cvc5, patterns);
            }
        }
        Expr::MethodCall { receiver, args, .. } => {
            collect_trigger_calls_cvc5(tm, receiver, bound_var, bound_cvc5, patterns);
            for a in args {
                collect_trigger_calls_cvc5(tm, a, bound_var, bound_cvc5, patterns);
            }
        }
        Expr::BinOp { lhs, rhs, .. } => {
            collect_trigger_calls_cvc5(tm, lhs, bound_var, bound_cvc5, patterns);
            collect_trigger_calls_cvc5(tm, rhs, bound_var, bound_cvc5, patterns);
        }
        Expr::UnaryOp { expr: e, .. } | Expr::Paren(e) | Expr::Old(e) | Expr::Ghost(e) => {
            collect_trigger_calls_cvc5(tm, e, bound_var, bound_cvc5, patterns);
        }
        Expr::If {
            cond,
            then_branch,
            else_branch,
        } => {
            collect_trigger_calls_cvc5(tm, cond, bound_var, bound_cvc5, patterns);
            collect_trigger_calls_cvc5(tm, then_branch, bound_var, bound_cvc5, patterns);
            if let Some(eb) = else_branch {
                collect_trigger_calls_cvc5(tm, eb, bound_var, bound_cvc5, patterns);
            }
        }
        Expr::Index { expr: e, index } => {
            collect_trigger_calls_cvc5(tm, e, bound_var, bound_cvc5, patterns);
            collect_trigger_calls_cvc5(tm, index, bound_var, bound_cvc5, patterns);
        }
        _ => {}
    }
}

/// Encode multi-token raw expressions for the native CVC5 backend.
///
/// Uses a full precedence-climbing (Pratt) parser supporting:
/// - 8 precedence levels (matching the AST expression parser)
/// - Parenthesized sub-expressions
/// - `old(expr)` syntax
/// - `forall`/`exists` quantifiers: `forall x in domain { body }`
/// - Comparison chaining: `a < b < c` desugars to `a < b && b < c`
/// - Prefix operators: `!`, `-`, `not`
/// - Dot-separated field access: `x.y.z` -> `x__y__z`
/// - Function calls: `f(a, b)` with built-in semantics for abs/min/max
#[cfg(feature = "cvc5-verify")]
fn encode_raw_tokens_cvc5<'a>(
    tm: &'a cvc5::TermManager,
    tokens: &[String],
    vars: &mut HashMap<String, cvc5::Term<'a>>,
    state: &mut Cvc5EncoderState<'a>,
) -> Option<cvc5::Term<'a>> {
    if tokens.is_empty() {
        return Some(tm.mk_boolean(true));
    }
    let (val, _pos) = parse_raw_expr_cvc5(tm, tokens, 0, 0, vars, state)?;
    Some(val)
}

/// Precedence-climbing expression parser for raw CVC5 tokens.
///
/// Returns `(term, next_position)`. Recurses with higher `min_prec` for
/// tighter-binding operators. Supports comparison chaining.
#[cfg(feature = "cvc5-verify")]
fn parse_raw_expr_cvc5<'a>(
    tm: &'a cvc5::TermManager,
    tokens: &[String],
    pos: usize,
    min_prec: u8,
    vars: &mut HashMap<String, cvc5::Term<'a>>,
    state: &mut Cvc5EncoderState<'a>,
) -> Option<(cvc5::Term<'a>, usize)> {
    let (mut lhs, mut pos) = parse_raw_atom_cvc5(tm, tokens, pos, vars, state)?;

    while pos < tokens.len() {
        let Some((op_prec, op_kind)) = raw_op_info(tokens[pos].as_str()) else {
            break;
        };
        if op_prec < min_prec {
            break;
        }

        pos += 1; // consume operator

        let (rhs, next_pos) = parse_raw_expr_cvc5(tm, tokens, pos, op_prec + 1, vars, state)?;
        pos = next_pos;

        // Comparison chaining: if we just parsed `a < b` and the next
        // op is also a comparison, desugar `a < b < c` into `a < b && b < c`.
        if raw_op_is_comparison(op_kind)
            && pos < tokens.len()
            && let Some((next_prec, next_op)) = raw_op_info(tokens[pos].as_str())
            && raw_op_is_comparison(next_op)
            && next_prec >= min_prec
        {
            let left_cmp = apply_raw_op_cvc5(tm, op_kind, lhs, rhs.clone());
            pos += 1; // consume next operator
            let (rhs2, next_pos2) =
                parse_raw_expr_cvc5(tm, tokens, pos, next_prec + 1, vars, state)?;
            pos = next_pos2;
            let right_cmp = apply_raw_op_cvc5(tm, next_op, rhs, rhs2);
            lhs = tm.mk_term(cvc5::Kind::And, &[left_cmp, right_cmp]);
            continue;
        }

        lhs = apply_raw_op_cvc5(tm, op_kind, lhs, rhs);
    }

    Some((lhs, pos))
}

/// Parse a single atom from raw CVC5 tokens.
///
/// Handles: parenthesized expressions, `old(expr)`, `forall`/`exists`,
/// prefix operators (`!`, `-`, `not`), boolean/integer literals,
/// `result` keyword, specification keywords (skipped), dot-separated
/// field access, and function calls with built-in semantics.
#[cfg(feature = "cvc5-verify")]
fn parse_raw_atom_cvc5<'a>(
    tm: &'a cvc5::TermManager,
    tokens: &[String],
    start: usize,
    vars: &mut HashMap<String, cvc5::Term<'a>>,
    state: &mut Cvc5EncoderState<'a>,
) -> Option<(cvc5::Term<'a>, usize)> {
    if start >= tokens.len() {
        // Past end: vacuously true
        return Some((tm.mk_boolean(true), start));
    }

    let tok = &tokens[start];

    // --- Unary not ---
    if tok == "not" || tok == "!" {
        let (val, next) = parse_raw_atom_cvc5(tm, tokens, start + 1, vars, state)?;
        return Some((tm.mk_term(cvc5::Kind::Not, &[val]), next));
    }

    // --- Unary minus ---
    if tok == "-" {
        let (val, next) = parse_raw_atom_cvc5(tm, tokens, start + 1, vars, state)?;
        return Some((tm.mk_term(cvc5::Kind::Neg, &[val]), next));
    }

    // --- Parenthesized expression ---
    if tok == "(" {
        let (val, end) = parse_raw_expr_cvc5(tm, tokens, start + 1, 0, vars, state)?;
        // skip closing ')'
        let next = if end < tokens.len() && tokens[end] == ")" {
            end + 1
        } else {
            end
        };
        return Some((val, next));
    }

    // --- Boolean literals ---
    if tok == "true" {
        return Some((tm.mk_boolean(true), start + 1));
    }
    if tok == "false" {
        return Some((tm.mk_boolean(false), start + 1));
    }

    // --- `result` keyword ---
    if tok == "result" {
        let key = "__result";
        let v = vars
            .get(key)
            .cloned()
            .unwrap_or_else(|| tm.mk_const(tm.integer_sort(), key));
        return Some((v, start + 1));
    }

    // --- `old(expr)` ---
    if tok == "old" && start + 1 < tokens.len() && tokens[start + 1] == "(" {
        let p = find_matching_delim(tokens, start + 1, "(", ")")?;
        let end = p + 1; // skip closing ')'
        let inner_tokens = &tokens[start + 2..p];

        // old(x) -> x__old
        if inner_tokens.len() == 1 {
            let old_name = format!("{}__old", sanitize_smtlib_name(&inner_tokens[0]));
            let v = vars
                .get(&old_name)
                .cloned()
                .unwrap_or_else(|| tm.mk_const(tm.integer_sort(), &old_name));
            return Some((v, end));
        }
        // old(x.field) -> x__old with field access UF
        if inner_tokens.len() == 3 && inner_tokens[1] == "." {
            let old_name = format!("{}__old", sanitize_smtlib_name(&inner_tokens[0]));
            let old_var = vars
                .get(&old_name)
                .cloned()
                .unwrap_or_else(|| tm.mk_const(tm.integer_sort(), &old_name));
            let field = sanitize_smtlib_name(&inner_tokens[2]);
            let func_name = format!("__field_{field}");
            let fun_sort = tm.mk_fun_sort(&[tm.integer_sort()], tm.integer_sort());
            let func = tm.mk_const(fun_sort, &func_name);
            let result = tm.mk_term(cvc5::Kind::ApplyUf, &[func, old_var]);
            return Some((result, end));
        }
        // General old(expr): parse inner expression, remap vars to __old
        // (simplified: parse as-is, create __old-suffixed vars for idents)
        let mut old_vars = vars.clone();
        for inner_tok in inner_tokens {
            if inner_tok
                .chars()
                .next()
                .is_some_and(|c| c.is_alphabetic() || c == '_')
                && !matches!(
                    inner_tok.as_str(),
                    "true"
                        | "false"
                        | "old"
                        | "forall"
                        | "exists"
                        | "result"
                        | "not"
                        | "and"
                        | "or"
                        | "implies"
                        | "mod"
                        | "div"
                        | "in"
                )
            {
                let sane = sanitize_smtlib_name(inner_tok);
                let old_key = format!("{sane}__old");
                if !old_vars.contains_key(&old_key) {
                    let term = tm.mk_const(tm.integer_sort(), &old_key);
                    old_vars.insert(old_key, term);
                }
            }
        }
        if let Some((val, _)) = parse_raw_expr_cvc5(tm, inner_tokens, 0, 0, &mut old_vars, state) {
            return Some((val, end));
        }
        // Fallback: fresh integer
        let fresh_name = format!("__old_fresh_{}", state.fresh_counter);
        state.fresh_counter += 1;
        return Some((tm.mk_const(tm.integer_sort(), &fresh_name), end));
    }

    // --- `forall`/`exists` quantifiers: `forall x in domain { body }` ---
    if let Some(slice) = parse_raw_quantifier_slice(tokens, start) {
        let var_name = sanitize_smtlib_name(&tokens[slice.var_token_idx]);

        // Bind quantifier variable
        let bound = tm.mk_var(tm.integer_sort(), &var_name);
        let mut local_vars = vars.clone();
        local_vars.insert(var_name.clone(), bound.clone());

        // Parse body
        let body_tokens = &tokens[slice.body_start..slice.body_end];
        if let Some((body_val, _)) =
            parse_raw_expr_cvc5(tm, body_tokens, 0, 0, &mut local_vars, state)
        {
            let var_list = tm.mk_term(cvc5::Kind::VariableList, &[bound]);
            let kind = if slice.is_forall {
                cvc5::Kind::Forall
            } else {
                cvc5::Kind::Exists
            };
            let quantified = tm.mk_term(kind, &[var_list, body_val]);
            return Some((quantified, slice.final_pos));
        }

        return Some((tm.mk_boolean(true), slice.final_pos));
    }

    // --- Integer literal ---
    if let Ok(n) = tok.parse::<i64>() {
        return Some((tm.mk_integer(n), start + 1));
    }

    // --- Skip specification keywords (taint/ghost/region/validate) ---
    if is_raw_spec_skip_keyword(tok) {
        return parse_raw_atom_cvc5(tm, tokens, start + 1, vars, state);
    }

    // --- Identifier (possibly with dot-separated field access) ---
    let mut name = sanitize_smtlib_name(tok);
    let mut next = start + 1;
    // Collapse `x.y.z` chains into `x__y__z`
    while next + 1 < tokens.len() && tokens[next] == "." {
        name.push_str("__");
        name.push_str(&sanitize_smtlib_name(&tokens[next + 1]));
        next += 2;
    }

    // --- #262: Typestate annotation: `Type @ State` ---
    // After collapsing dot chains, if the next token is `@` followed
    // by a state name, encode as integer equality:
    //   __typestate_<name> == hash(state_name)
    if next + 1 < tokens.len() && tokens[next] == "@" {
        let state_name = &tokens[next + 1];
        let ts_var_name = format!("__typestate_{name}");
        let ts_var = vars
            .entry(ts_var_name)
            .or_insert_with(|| tm.mk_const(tm.integer_sort(), &format!("__typestate_{name}")))
            .clone();
        let state_val = tm.mk_integer(pattern_hash_name(state_name));
        return Some((
            tm.mk_term(cvc5::Kind::Equal, &[ts_var, state_val]),
            next + 2,
        ));
    }

    // Check for function call: `name(args)`
    if next < tokens.len() && tokens[next] == "(" {
        let p = find_matching_delim(tokens, next, "(", ")")?;

        // Parse arguments by splitting on commas at depth 0
        let arg_tokens = &tokens[next + 1..p];
        let mut arg_vals: Vec<cvc5::Term<'a>> = Vec::new();
        for (lo, hi) in comma_chunk_ranges(arg_tokens) {
            let chunk = &arg_tokens[lo..hi];
            if !chunk.is_empty()
                && let Some((v, _)) = parse_raw_expr_cvc5(tm, chunk, 0, 0, vars, state)
            {
                arg_vals.push(v);
            }
        }
        let end = p + 1; // skip closing ')'

        // Extract base function name (last segment after dots)
        let func_name = name.rsplit("__").next().unwrap_or(&name);

        // Built-in functions
        match func_name {
            "abs" if arg_vals.len() == 1 => {
                let x = arg_vals[0].clone();
                let zero = tm.mk_integer(0);
                let neg_x = tm.mk_term(cvc5::Kind::Neg, &[x.clone()]);
                let cond = tm.mk_term(cvc5::Kind::Geq, &[x.clone(), zero]);
                return Some((tm.mk_term(cvc5::Kind::Ite, &[cond, x, neg_x]), end));
            }
            "min" if arg_vals.len() == 2 => {
                let (a, b) = (arg_vals[0].clone(), arg_vals[1].clone());
                let cond = tm.mk_term(cvc5::Kind::Leq, &[a.clone(), b.clone()]);
                return Some((tm.mk_term(cvc5::Kind::Ite, &[cond, a, b]), end));
            }
            "max" if arg_vals.len() == 2 => {
                let (a, b) = (arg_vals[0].clone(), arg_vals[1].clone());
                let cond = tm.mk_term(cvc5::Kind::Geq, &[a.clone(), b.clone()]);
                return Some((tm.mk_term(cvc5::Kind::Ite, &[cond, a, b]), end));
            }
            "length" if arg_vals.is_empty() => {
                // x.length() -> UF with length >= 0 axiom
                let uf_sort = tm.mk_fun_sort(&[tm.integer_sort()], tm.integer_sort());
                let uf = tm.mk_const(uf_sort, "__length");
                let base_var = vars
                    .get(&name)
                    .cloned()
                    .unwrap_or_else(|| tm.mk_const(tm.integer_sort(), &name));
                let result = tm.mk_term(cvc5::Kind::ApplyUf, &[uf, base_var]);
                let zero = tm.mk_integer(0);
                let axiom = tm.mk_term(cvc5::Kind::Geq, &[result.clone(), zero]);
                state.axioms.push(axiom);
                return Some((result, end));
            }
            _ => {
                // Generic UF
                if arg_vals.is_empty() {
                    let v = vars
                        .get(&name)
                        .cloned()
                        .unwrap_or_else(|| tm.mk_const(tm.integer_sort(), &name));
                    return Some((v, end));
                }
                let n_args = arg_vals.len();
                let domain: Vec<_> = (0..n_args).map(|_| tm.integer_sort()).collect();
                let fun_sort = tm.mk_fun_sort(&domain, tm.integer_sort());
                let func = tm.mk_const(fun_sort, &name);
                let mut all_args = vec![func];
                all_args.extend(arg_vals);
                return Some((tm.mk_term(cvc5::Kind::ApplyUf, &all_args), end));
            }
        }
    }

    // Plain identifier
    let v = vars
        .get(&name)
        .cloned()
        .unwrap_or_else(|| tm.mk_const(tm.integer_sort(), &name));
    Some((v, next))
}
