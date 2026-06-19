//! IR body constraints as SMT-LIB2 for the CVC5 shell-out path (#271).

use std::collections::{HashMap, HashSet};

use crate::cvc5_common::sanitize_smtlib_name;
use crate::cvc5_havoc_assume_smtlib::canonical_length_smtlib_name;
use crate::havoc_assume::{RESULT_SLOT, ir_param_names};
use crate::ir::{IrArithOp, IrCmpOp, IrExprKind, IrFunction, IrLiteral, IrPred, IrPredArg};

struct IrSmtlibEncoder {
    fresh_counter: usize,
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

fn slot_term(slots: &HashMap<usize, String>, slot: usize, enc: &mut IrSmtlibEncoder) -> String {
    slots
        .get(&slot)
        .cloned()
        .unwrap_or_else(|| enc.fresh_name())
}

fn encode_ir_expr_smtlib(
    expr: &IrExprKind,
    slots: &HashMap<usize, String>,
    enc: &mut IrSmtlibEncoder,
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
            let l = encode_ir_expr_smtlib(&IrExprKind::Load(*lhs), slots, enc);
            let r = encode_ir_expr_smtlib(&IrExprKind::Load(*rhs), slots, enc);
            mk_ir_arith_smtlib(*op, &l, &r)
        }
        IrExprKind::Cmp { op, lhs, rhs } => {
            let l = encode_ir_expr_smtlib(&IrExprKind::Load(*lhs), slots, enc);
            let r = encode_ir_expr_smtlib(&IrExprKind::Load(*rhs), slots, enc);
            let b = mk_ir_cmp_bool_smtlib(*op, &l, &r);
            format!("(ite {b} 1 0)")
        }
        IrExprKind::Call { func, args } => {
            let arg_terms: Vec<String> = args
                .iter()
                .map(|a| encode_ir_expr_smtlib(&IrExprKind::Load(*a), slots, enc))
                .collect();
            let fname = sanitize_smtlib_name(&format!("__ir_call_{func}"));
            format!("({fname} {})", arg_terms.join(" "))
        }
        IrExprKind::Field { slot, index } => {
            let base = encode_ir_expr_smtlib(&IrExprKind::Load(*slot), slots, enc);
            let fname = sanitize_smtlib_name(&format!("__ir_field_{index}"));
            format!("({fname} {base})")
        }
        IrExprKind::Construct { type_id, fields } => {
            let args: Vec<String> = fields
                .iter()
                .map(|(_, s)| encode_ir_expr_smtlib(&IrExprKind::Load(*s), slots, enc))
                .collect();
            let fname = sanitize_smtlib_name(&format!("__ir_construct_{type_id}"));
            format!("({fname} {})", args.join(" "))
        }
        IrExprKind::Cast { slot, .. } | IrExprKind::Transition { slot, .. } => {
            encode_ir_expr_smtlib(&IrExprKind::Load(*slot), slots, enc)
        }
        IrExprKind::If {
            cond,
            then_block,
            else_block,
        } => {
            let c = encode_ir_expr_smtlib(&IrExprKind::Load(*cond), slots, enc);
            let then_b = sanitize_smtlib_name(&format!("__ir_block_{then_block}"));
            let else_b = sanitize_smtlib_name(&format!("__ir_block_{else_block}"));
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
) {
    let mut enc = IrSmtlibEncoder { fresh_counter: 0 };
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

    for instr in &func.body {
        if instr.target != RESULT_SLOT && !slots.contains_key(&instr.target) {
            let name = format!("__ir_slot_{}", instr.target);
            declare_int_var(script, vars, &name);
            slots.insert(instr.target, sanitize_smtlib_name(&name));
        }
        let computed = encode_ir_expr_smtlib(&instr.expr, &slots, &mut enc);
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
        append_ir_body_constraints_smtlib(&mut script, &mut vars, &func, &["x".into()]);
        assert!(
            script.contains("__ir_call_is_valid"),
            "expected UF call in script, got:\n{script}"
        );
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
        append_ir_body_constraints_smtlib(&mut script, &mut vars, &func, &["raw".into()]);
        assert!(
            script.contains("(assert (= __canonical_len_result __canonical_len_raw))"),
            "expected length identity from IR load, got:\n{script}"
        );
    }
}
