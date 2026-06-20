//! IR body constraints as SMT-LIB2 for the CVC5 shell-out path (#271).

use std::collections::{HashMap, HashSet};

use crate::cvc5_adt::define_adt_cvc5;
use crate::cvc5_common::canonical_length_smtlib_name;
use crate::cvc5_common::sanitize_smtlib_name;
use crate::havoc_assume::{RESULT_SLOT, ir_param_names};
use crate::ir::{IrArithOp, IrCmpOp, IrExprKind, IrFunction, IrLiteral, IrPred, IrPredArg};
use crate::ir_encode::{IrEncodeContext, is_collection_ir_type, is_length_ir_call, slot_type_map};
use crate::ir_type_ctx::base_type_name;

struct IrSmtlibEncoder {
    fresh_counter: usize,
    declared_adts: std::collections::HashSet<String>,
}

impl IrSmtlibEncoder {
    fn fresh_name(&mut self) -> String {
        let n = self.fresh_counter;
        self.fresh_counter += 1;
        format!("__fresh_{n}")
    }
}

fn declare_int_var(script: &mut String, vars: &mut HashSet<String>, name: &str) {
    let key = sanitize_smtlib_name(name);
    if vars.insert(key.clone()) {
        script.push_str(&format!("(declare-const {key} Int)\n"));
    }
}

fn declare_canonical_len(script: &mut String, vars: &mut HashSet<String>, name: &str) {
    let key = canonical_length_smtlib_name(name);
    if vars.insert(key.clone()) {
        script.push_str(&format!("(declare-const {key} Int)\n"));
    }
}

fn mk_ir_arith_smtlib(op: IrArithOp, l: &str, r: &str) -> String {
    match op {
        IrArithOp::Add => format!("(+ {l} {r})"),
        IrArithOp::Sub => format!("(- {l} {r})"),
        IrArithOp::Mul => format!("(* {l} {r})"),
        IrArithOp::Div => format!("(div {l} {r})"),
        IrArithOp::Mod => format!("(mod {l} {r})"),
    }
}

fn mk_ir_cmp_bool_smtlib(op: IrCmpOp, l: &str, r: &str) -> String {
    match op {
        IrCmpOp::Eq => format!("(= {l} {r})"),
        IrCmpOp::Ne => format!("(not (= {l} {r}))"),
        IrCmpOp::Lt => format!("(< {l} {r})"),
        IrCmpOp::Le => format!("(<= {l} {r})"),
        IrCmpOp::Gt => format!("(> {l} {r})"),
        IrCmpOp::Ge => format!("(>= {l} {r})"),
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "call inlining threads callee slot maps"
)]
fn eval_ir_call_smtlib(
    func: &str,
    args: &[usize],
    slots: &HashMap<usize, String>,
    slot_to_name: &HashMap<usize, String>,
    slot_types: &HashMap<usize, String>,
    enc: &mut IrSmtlibEncoder,
    script: &mut String,
    vars: &mut HashSet<String>,
    enc_ctx: IrEncodeContext<'_>,
) -> Option<String> {
    use crate::ir::IrExprKind;

    let callee = enc_ctx.callee_ir(func)?;
    if callee.params.len() != args.len() {
        return None;
    }

    let prefix = format!("__ir_call_{func}_");
    let mut local: HashMap<usize, String> = HashMap::new();

    for (i, param) in callee.params.iter().enumerate() {
        let arg_val = encode_ir_expr_smtlib(
            &IrExprKind::Load(args[i]),
            slots,
            slot_to_name,
            slot_types,
            enc,
            script,
            vars,
            enc_ctx,
        );
        let name = format!("{prefix}param_{}", param.slot);
        declare_int_var(script, vars, &name);
        let key = sanitize_smtlib_name(&name);
        script.push_str(&format!("(assert (= {arg_val} {key}))\n"));
        local.insert(param.slot, key);
    }

    let result_name = format!("{prefix}result");
    declare_int_var(script, vars, &result_name);
    let result_key = sanitize_smtlib_name(&result_name);
    local.insert(RESULT_SLOT, result_key.clone());

    let callee_slot_types = slot_type_map(callee);
    let callee_names: HashMap<usize, String> = callee
        .params
        .iter()
        .map(|p| (p.slot, format!("{prefix}param_{}", p.slot)))
        .collect();

    for instr in &callee.body {
        if instr.target != RESULT_SLOT && !local.contains_key(&instr.target) {
            let name = format!("{prefix}slot_{}", instr.target);
            declare_int_var(script, vars, &name);
            local.insert(instr.target, sanitize_smtlib_name(&name));
        }
        let computed = encode_ir_expr_smtlib(
            &instr.expr,
            &local,
            &callee_names,
            &callee_slot_types,
            enc,
            script,
            vars,
            enc_ctx,
        );
        if let Some(target) = local.get(&instr.target) {
            script.push_str(&format!("(assert (= {computed} {target}))\n"));
        }
    }

    local.get(&RESULT_SLOT).cloned()
}

fn slot_term(slots: &HashMap<usize, String>, slot: usize, enc: &mut IrSmtlibEncoder) -> String {
    slots
        .get(&slot)
        .cloned()
        .unwrap_or_else(|| enc.fresh_name())
}

fn ensure_struct_adt_smtlib(
    enc: &mut IrSmtlibEncoder,
    script: &mut String,
    type_name: &str,
    field_names: &[&str],
) {
    if !enc.declared_adts.insert(type_name.to_string()) {
        return;
    }
    let accessors: Vec<&str> = field_names.to_vec();
    let (_, lines) = define_adt_cvc5(type_name, &[(type_name, accessors.as_slice())]);
    for line in lines {
        script.push_str(&line);
        if !line.ends_with('\n') {
            script.push('\n');
        }
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "IR block eval threads type context"
)]
fn eval_ir_block_smtlib(
    block_id: usize,
    slots: &HashMap<usize, String>,
    slot_to_name: &HashMap<usize, String>,
    slot_types: &HashMap<usize, String>,
    enc: &mut IrSmtlibEncoder,
    script: &mut String,
    vars: &mut HashSet<String>,
    enc_ctx: IrEncodeContext<'_>,
) -> Option<String> {
    use crate::havoc_assume::RESULT_SLOT;

    let body = enc_ctx.ir_blocks?.get(&block_id)?;
    let mut local = slots.clone();
    let mut last = None;
    for instr in body {
        if instr.target != RESULT_SLOT && !local.contains_key(&instr.target) {
            let name = format!("__ir_block{block_id}_slot_{}", instr.target);
            declare_int_var(script, vars, &name);
            local.insert(instr.target, sanitize_smtlib_name(&name));
        }
        let computed = encode_ir_expr_smtlib(
            &instr.expr,
            &local,
            slot_to_name,
            slot_types,
            enc,
            script,
            vars,
            enc_ctx,
        );
        if let Some(target) = local.get(&instr.target) {
            script.push_str(&format!("(assert (= {computed} {target}))\n"));
        }
        last = local.get(&instr.target).cloned();
    }
    last
}

#[expect(
    clippy::too_many_arguments,
    reason = "IR expr encoding threads type context"
)]
fn encode_ir_expr_smtlib(
    expr: &IrExprKind,
    slots: &HashMap<usize, String>,
    slot_to_name: &HashMap<usize, String>,
    slot_types: &HashMap<usize, String>,
    enc: &mut IrSmtlibEncoder,
    script: &mut String,
    vars: &mut HashSet<String>,
    enc_ctx: IrEncodeContext<'_>,
) -> String {
    match expr {
        IrExprKind::Const(IrLiteral::Int(n)) => n.to_string(),
        IrExprKind::Const(IrLiteral::Float(f)) => format!("{}", *f as i64),
        IrExprKind::Const(IrLiteral::Bool(b)) => {
            if *b {
                "1".into()
            } else {
                "0".into()
            }
        }
        IrExprKind::Const(IrLiteral::Str(_)) => enc.fresh_name(),
        IrExprKind::Load(slot) => slot_term(slots, *slot, enc),
        IrExprKind::Arith { op, lhs, rhs } => {
            let l = encode_ir_expr_smtlib(
                &IrExprKind::Load(*lhs),
                slots,
                slot_to_name,
                slot_types,
                enc,
                script,
                vars,
                enc_ctx,
            );
            let r = encode_ir_expr_smtlib(
                &IrExprKind::Load(*rhs),
                slots,
                slot_to_name,
                slot_types,
                enc,
                script,
                vars,
                enc_ctx,
            );
            mk_ir_arith_smtlib(*op, &l, &r)
        }
        IrExprKind::Cmp { op, lhs, rhs } => {
            let l = encode_ir_expr_smtlib(
                &IrExprKind::Load(*lhs),
                slots,
                slot_to_name,
                slot_types,
                enc,
                script,
                vars,
                enc_ctx,
            );
            let r = encode_ir_expr_smtlib(
                &IrExprKind::Load(*rhs),
                slots,
                slot_to_name,
                slot_types,
                enc,
                script,
                vars,
                enc_ctx,
            );
            let b = mk_ir_cmp_bool_smtlib(*op, &l, &r);
            format!("(ite {b} 1 0)")
        }
        IrExprKind::Call { func, args } => {
            if is_length_ir_call(func, args.len())
                && let Some(slot) = args.first()
                && let Some(name) = slot_to_name.get(slot)
            {
                declare_canonical_len(script, vars, name);
                return canonical_length_smtlib_name(name);
            }
            let arg_terms: Vec<String> = args
                .iter()
                .map(|a| {
                    encode_ir_expr_smtlib(
                        &IrExprKind::Load(*a),
                        slots,
                        slot_to_name,
                        slot_types,
                        enc,
                        script,
                        vars,
                        enc_ctx,
                    )
                })
                .collect();
            if let Some(builtin) = crate::cvc5_builtins::known_builtin_to_smtlib(func, &arg_terms) {
                return builtin;
            }
            if let Some(inlined) = eval_ir_call_smtlib(
                func,
                args,
                slots,
                slot_to_name,
                slot_types,
                enc,
                script,
                vars,
                enc_ctx,
            ) {
                return inlined;
            }
            let fname = sanitize_smtlib_name(&format!("__ir_call_{func}"));
            format!("({fname} {})", arg_terms.join(" "))
        }
        IrExprKind::Field { slot, index } => {
            if *index == 0
                && let Some(ty) = slot_types.get(slot)
                && is_collection_ir_type(ty)
                && let Some(name) = slot_to_name.get(slot)
            {
                declare_canonical_len(script, vars, name);
                return canonical_length_smtlib_name(name);
            }
            let base = encode_ir_expr_smtlib(
                &IrExprKind::Load(*slot),
                slots,
                slot_to_name,
                slot_types,
                enc,
                script,
                vars,
                enc_ctx,
            );
            if let Some(ir_ty) = slot_types.get(slot)
                && let Some(field_name) = enc_ctx.type_ctx.field_name_at(ir_ty, *index)
            {
                let type_name = base_type_name(ir_ty);
                if let Some(names) = enc_ctx.type_ctx.field_names_for(type_name) {
                    ensure_struct_adt_smtlib(enc, script, type_name, &names);
                    let fname = sanitize_smtlib_name(&format!("__adt_{type_name}_{field_name}"));
                    return format!("({fname} {base})");
                }
            }
            let ty_suffix = slot_types
                .get(slot)
                .map(|t| t.replace('<', "_").replace('>', ""))
                .unwrap_or_else(|| "val".into());
            let fname = sanitize_smtlib_name(&format!("__ir_field_{ty_suffix}_{index}"));
            format!("({fname} {base})")
        }
        IrExprKind::Construct { type_id, fields } => {
            if enc_ctx.type_ctx.has_struct_layout(type_id)
                && let Some(field_names) = enc_ctx.type_ctx.field_names_for(type_id)
            {
                ensure_struct_adt_smtlib(enc, script, type_id, &field_names);
                let mut ordered = fields.clone();
                ordered.sort_by_key(|(idx, _)| *idx);
                let args: Vec<String> = ordered
                    .iter()
                    .map(|(_, s)| {
                        encode_ir_expr_smtlib(
                            &IrExprKind::Load(*s),
                            slots,
                            slot_to_name,
                            slot_types,
                            enc,
                            script,
                            vars,
                            enc_ctx,
                        )
                    })
                    .collect();
                let val = enc.fresh_name();
                declare_int_var(script, vars, &val);
                let tag_fn = sanitize_smtlib_name(&format!("__adt_tag_{type_id}"));
                script.push_str(&format!("(assert (= ({tag_fn} {val}) 0))\n"));
                for (i, accessor) in field_names.iter().enumerate() {
                    if let Some(arg) = args.get(i) {
                        let acc_fn = sanitize_smtlib_name(&format!("__adt_{type_id}_{accessor}"));
                        script.push_str(&format!("(assert (= ({acc_fn} {val}) {arg}))\n"));
                    }
                }
                return val;
            }
            let args: Vec<String> = fields
                .iter()
                .map(|(_, s)| {
                    encode_ir_expr_smtlib(
                        &IrExprKind::Load(*s),
                        slots,
                        slot_to_name,
                        slot_types,
                        enc,
                        script,
                        vars,
                        enc_ctx,
                    )
                })
                .collect();
            let fname = sanitize_smtlib_name(&format!("__ir_construct_{type_id}"));
            format!("({fname} {})", args.join(" "))
        }
        IrExprKind::Cast { slot, .. } | IrExprKind::Transition { slot, .. } => {
            encode_ir_expr_smtlib(
                &IrExprKind::Load(*slot),
                slots,
                slot_to_name,
                slot_types,
                enc,
                script,
                vars,
                enc_ctx,
            )
        }
        IrExprKind::If {
            cond,
            then_block,
            else_block,
        } => {
            let c = encode_ir_expr_smtlib(
                &IrExprKind::Load(*cond),
                slots,
                slot_to_name,
                slot_types,
                enc,
                script,
                vars,
                enc_ctx,
            );
            let then_b = eval_ir_block_smtlib(
                *then_block,
                slots,
                slot_to_name,
                slot_types,
                enc,
                script,
                vars,
                enc_ctx,
            )
            .unwrap_or_else(|| sanitize_smtlib_name(&format!("__ir_block_{then_block}")));
            let else_b = eval_ir_block_smtlib(
                *else_block,
                slots,
                slot_to_name,
                slot_types,
                enc,
                script,
                vars,
                enc_ctx,
            )
            .unwrap_or_else(|| sanitize_smtlib_name(&format!("__ir_block_{else_block}")));
            format!("(ite (distinct {c} 0) ({then_b}) ({else_b}))")
        }
    }
}

fn encode_ir_pred_arg_smtlib(
    arg: &IrPredArg,
    slots: &HashMap<usize, String>,
    enc: &mut IrSmtlibEncoder,
) -> String {
    match arg {
        IrPredArg::Slot(n) => slot_term(slots, *n, enc),
        IrPredArg::SlotResult => slots
            .get(&RESULT_SLOT)
            .cloned()
            .unwrap_or_else(|| sanitize_smtlib_name("result")),
        IrPredArg::Lit(IrLiteral::Int(n)) => n.to_string(),
        IrPredArg::Lit(IrLiteral::Float(f)) => format!("{}", *f as i64),
        IrPredArg::Lit(IrLiteral::Bool(b)) => {
            if *b {
                "1".into()
            } else {
                "0".into()
            }
        }
        IrPredArg::Lit(IrLiteral::Str(_)) => enc.fresh_name(),
        IrPredArg::Arith { op, lhs, rhs } => {
            let l = encode_ir_pred_arg_smtlib(lhs, slots, enc);
            let r = encode_ir_pred_arg_smtlib(rhs, slots, enc);
            mk_ir_arith_smtlib(*op, &l, &r)
        }
    }
}

fn encode_ir_pred_smtlib(
    pred: &IrPred,
    slots: &HashMap<usize, String>,
    enc: &mut IrSmtlibEncoder,
) -> Option<String> {
    match pred {
        IrPred::True => Some("true".into()),
        IrPred::False => Some("false".into()),
        IrPred::Cmp { op, lhs, rhs } => {
            let l = encode_ir_pred_arg_smtlib(lhs, slots, enc);
            let r = encode_ir_pred_arg_smtlib(rhs, slots, enc);
            Some(mk_ir_cmp_bool_smtlib(*op, &l, &r))
        }
        IrPred::And(a, b) => {
            let la = encode_ir_pred_smtlib(a, slots, enc)?;
            let lb = encode_ir_pred_smtlib(b, slots, enc)?;
            Some(format!("(and {la} {lb})"))
        }
        IrPred::Or(a, b) => {
            let la = encode_ir_pred_smtlib(a, slots, enc)?;
            let lb = encode_ir_pred_smtlib(b, slots, enc)?;
            Some(format!("(or {la} {lb})"))
        }
        IrPred::Not(inner) => {
            encode_ir_pred_smtlib(inner, slots, enc).map(|p| format!("(not {p})"))
        }
    }
}

/// Append IR implementation-body constraints as `(assert ...)` axioms.
pub(crate) fn append_ir_body_constraints_smtlib(
    script: &mut String,
    vars: &mut HashSet<String>,
    func: &IrFunction,
    contract_param_names: &[String],
    enc_ctx: IrEncodeContext<'_>,
) {
    let mut enc = IrSmtlibEncoder {
        fresh_counter: 0,
        declared_adts: std::collections::HashSet::new(),
    };
    let mut slots: HashMap<usize, String> = HashMap::new();

    for (slot, name) in ir_param_names(func, contract_param_names) {
        declare_int_var(script, vars, &name);
        slots.insert(slot, sanitize_smtlib_name(&name));
    }

    declare_int_var(script, vars, "result");
    slots.insert(RESULT_SLOT, sanitize_smtlib_name("result"));

    let slot_to_name: HashMap<usize, String> = ir_param_names(func, contract_param_names)
        .into_iter()
        .collect();
    let slot_types = slot_type_map(func);

    for instr in &func.body {
        if instr.target != RESULT_SLOT && !slots.contains_key(&instr.target) {
            let name = format!("__ir_slot_{}", instr.target);
            declare_int_var(script, vars, &name);
            slots.insert(instr.target, sanitize_smtlib_name(&name));
        }
        let computed = encode_ir_expr_smtlib(
            &instr.expr,
            &slots,
            &slot_to_name,
            &slot_types,
            &mut enc,
            script,
            vars,
            enc_ctx,
        );
        if let Some(target) = slots.get(&instr.target) {
            script.push_str(&format!("(assert (= {computed} {target}))\n"));
        }
        if instr.target == RESULT_SLOT
            && let IrExprKind::Load(src) = &instr.expr
            && let Some(param) = slot_to_name.get(src)
        {
            let len_result = canonical_length_smtlib_name("result");
            let len_param = canonical_length_smtlib_name(param);
            declare_int_var(script, vars, "result");
            declare_int_var(script, vars, param);
            script.push_str(&format!("(assert (= {len_result} {len_param}))\n"));
        }
    }

    if let Some(post) = &func.post
        && let Some(pred) = encode_ir_pred_smtlib(post, &slots, &mut enc)
    {
        script.push_str(&format!("(assert {pred})\n"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::parse_ir_module;

    #[test]
    fn ir_call_emits_uninterpreted_function_application() {
        let ir_source = r#"
module test {
  fn #0 : ($0: Int) -> Bool ! pure
  {
    $1 = call is_valid ($0) : Bool
    $result = load $1 : Bool
  }
}
"#;
        let func = parse_ir_module(ir_source).unwrap().functions[0].clone();
        let mut script = String::new();
        let mut vars = HashSet::new();
        append_ir_body_constraints_smtlib(
            &mut script,
            &mut vars,
            &func,
            &["x".into()],
            IrEncodeContext::default(),
        );
        assert!(
            script.contains("__ir_call_is_valid"),
            "expected UF call in script, got:\n{script}"
        );
    }

    #[test]
    fn ir_call_inlines_callee_sidecar() {
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

        let mut script = String::new();
        let mut vars = HashSet::new();
        append_ir_body_constraints_smtlib(
            &mut script,
            &mut vars,
            &main_ir,
            &["x".into()],
            IrEncodeContext::new(None, Some(&bodies), None),
        );
        assert!(
            script.contains("__ir_call_double_"),
            "call double should inline callee IR with prefixed slots, got:\n{script}"
        );
    }

    #[test]
    fn ir_blocks_inlines_sibling_functions() {
        let (func, blocks) = crate::ir_encode::branch_if_else_ir_fixture();

        let mut script = String::new();
        let mut vars = HashSet::new();
        append_ir_body_constraints_smtlib(
            &mut script,
            &mut vars,
            &func,
            &["x".into()],
            IrEncodeContext::new(None, None, Some(&blocks)),
        );
        let axiom_lines = script.lines().filter(|l| l.contains("(assert")).count();
        crate::ir_encode::assert_ir_blocks_inlined(&script, axiom_lines);
    }

    #[test]
    fn ir_load_result_emits_length_identity_axiom() {
        let ir_source = r#"
module copy {
  fn #0 : ($0: Bytes) -> Bytes ! pure
  {
    $result = load $0 : Bytes
  }
}
"#;
        let func = parse_ir_module(ir_source).unwrap().functions[0].clone();
        let mut script = String::new();
        let mut vars = HashSet::new();
        append_ir_body_constraints_smtlib(
            &mut script,
            &mut vars,
            &func,
            &["raw".into()],
            IrEncodeContext::default(),
        );
        assert!(
            script.contains("(assert (= __canonical_len_result __canonical_len_raw))"),
            "expected length identity from IR load, got:\n{script}"
        );
    }
}
