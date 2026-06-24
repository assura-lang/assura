//! IR body constraints as SMT-LIB2 for the CVC5 shell-out path (#271).

use std::collections::{HashMap, HashSet};

use crate::cvc5_adt::define_adt_cvc5;
use crate::cvc5_common::canonical_length_smtlib_name;
use crate::cvc5_common::sanitize_smtlib_name;
use crate::havoc_assume::{RESULT_SLOT, ir_param_names};
use crate::ir::{IrArithOp, IrCmpOp, IrFunction, IrLiteral, IrPred, IrPredArg};
use crate::ir_encode::{IrEncodeContext, is_collection_ir_type, slot_type_map};
use crate::ir_lower::{IrSlotContext, IrTermBuilder};
use crate::ir_type_ctx::base_type_name;

struct IrSmtlibEncoder {
    fresh_counter: usize,
    declared_adts: HashSet<String>,
}

struct SmtlibIrBuilder<'a, 'b> {
    enc: &'a mut IrSmtlibEncoder,
    script: &'a mut String,
    vars: &'a mut HashSet<String>,
    /// Retained so field/construct helpers match Z3/CVC5 builder shape (ctx uses copies).
    #[allow(dead_code)]
    slot_to_name: &'b HashMap<usize, String>,
    #[allow(dead_code)]
    slot_types: &'b HashMap<usize, String>,
    enc_ctx: IrEncodeContext<'b>,
}

impl IrSmtlibEncoder {
    fn fresh_name(&mut self) -> String {
        let n = self.fresh_counter;
        self.fresh_counter += 1;
        crate::encode_atom_policy::fresh_temp_name(n)
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
        IrArithOp::Mod => format!("(mod {l} {r})"),
        IrArithOp::Mul => format!("(* {l} {r})"),
        IrArithOp::Div => format!("(div {l} {r})"),
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

impl IrTermBuilder for SmtlibIrBuilder<'_, '_> {
    type Term = String;

    fn int_const(&mut self, n: i64) -> Self::Term {
        n.to_string()
    }

    fn get_or_create_named(&mut self, name: &str) -> Self::Term {
        declare_int_var(self.script, self.vars, name);
        sanitize_smtlib_name(name)
    }

    fn load_slot(&mut self, slots: &HashMap<usize, Self::Term>, slot: usize) -> Self::Term {
        slots.get(&slot).cloned().unwrap_or_else(|| {
            let name = self.enc.fresh_name();
            declare_int_var(self.script, self.vars, &name);
            name
        })
    }

    fn push_eq_axiom(&mut self, lhs: Self::Term, rhs: Self::Term) {
        self.script.push_str(&format!("(assert (= {lhs} {rhs}))\n"));
    }

    fn arith(&mut self, op: IrArithOp, lhs: Self::Term, rhs: Self::Term) -> Self::Term {
        mk_ir_arith_smtlib(op, &lhs, &rhs)
    }

    fn cmp_as_int(&mut self, op: IrCmpOp, lhs: Self::Term, rhs: Self::Term) -> Self::Term {
        let b = mk_ir_cmp_bool_smtlib(op, &lhs, &rhs);
        format!("(ite {b} 1 0)")
    }

    fn ite_nonzero(
        &mut self,
        cond: Self::Term,
        then_v: Self::Term,
        else_v: Self::Term,
    ) -> Self::Term {
        format!("(ite (distinct {cond} 0) {then_v} {else_v})")
    }

    fn nullary_uf(&mut self, name: &str) -> Self::Term {
        declare_int_var(self.script, self.vars, name);
        sanitize_smtlib_name(name)
    }

    fn unary_uf(&mut self, name: &str, arg: Self::Term) -> Self::Term {
        let fname = sanitize_smtlib_name(name);
        format!("({fname} {arg})")
    }

    fn nary_uf(&mut self, name: &str, args: &[Self::Term]) -> Self::Term {
        let fname = sanitize_smtlib_name(name);
        format!("({fname} {})", args.join(" "))
    }

    fn fresh_int(&mut self) -> Self::Term {
        let name = self.enc.fresh_name();
        declare_int_var(self.script, self.vars, &name);
        name
    }

    fn enc_ctx(&self) -> IrEncodeContext<'_> {
        self.enc_ctx
    }

    fn canonical_length_for_name(&mut self, name: &str) -> Self::Term {
        declare_canonical_len(self.script, self.vars, name);
        canonical_length_smtlib_name(name)
    }

    fn on_result_construct(&mut self, type_id: &str) {
        let tag = crate::cvc5_builtins::pattern_hash_name(type_id);
        let tag_name = sanitize_smtlib_name(&crate::encode_atom_policy::ir_tag_name(type_id));
        declare_int_var(self.script, self.vars, &tag_name);
        self.script
            .push_str(&format!("(assert (= {tag_name} {tag}))\n"));
    }

    fn push_ir_post(&mut self, pred: &crate::ir::IrPred, slots: &HashMap<usize, Self::Term>) {
        if let Some(p) = encode_ir_pred_smtlib(pred, slots, self.enc) {
            self.script.push_str(&format!("(assert {p})\n"));
        }
    }

    fn try_known_builtin(&mut self, func: &str, args: &[Self::Term]) -> Option<Self::Term> {
        crate::cvc5_builtins::known_builtin_to_smtlib(func, args)
    }

    fn encode_field(
        &mut self,
        slot: usize,
        index: usize,
        slots: &HashMap<usize, Self::Term>,
        ctx: IrSlotContext<'_>,
    ) -> Self::Term {
        if index == 0
            && let Some(ty) = ctx.slot_types.get(&slot)
            && is_collection_ir_type(ty)
            && let Some(name) = ctx.slot_to_name.get(&slot).cloned()
        {
            return self.canonical_length_for_name(&name);
        }
        let base = self.load_slot(slots, slot);
        if let Some(ir_ty) = ctx.slot_types.get(&slot)
            && let Some(field_name) = self.enc_ctx.type_ctx.field_name_at(ir_ty, index)
        {
            let type_name = base_type_name(ir_ty);
            if let Some(names) = self.enc_ctx.type_ctx.field_names_for(type_name) {
                let field_name_refs: Vec<&str> = names.to_vec();
                ensure_struct_adt_smtlib(self.enc, self.script, type_name, &field_name_refs);
                let fname = sanitize_smtlib_name(&crate::encode_atom_policy::adt_accessor_uf_name(
                    type_name, field_name,
                ));
                return format!("({fname} {base})");
            }
        }
        let ty_suffix = ctx
            .slot_types
            .get(&slot)
            .map(|t| t.replace('<', "_").replace('>', ""))
            .unwrap_or_else(|| "val".into());
        let fname = sanitize_smtlib_name(&crate::encode_atom_policy::ir_field_uf_name(
            &ty_suffix, index,
        ));
        format!("({fname} {base})")
    }

    fn encode_construct(
        &mut self,
        type_id: &str,
        fields: &[(usize, usize)],
        slots: &HashMap<usize, Self::Term>,
        _ctx: IrSlotContext<'_>,
    ) -> Self::Term {
        let has_layout = self.enc_ctx.type_ctx.has_struct_layout(type_id);
        let typed_field_names: Option<Vec<String>> = if has_layout {
            self.enc_ctx
                .type_ctx
                .field_names_for(type_id)
                .map(|names| names.into_iter().map(str::to_string).collect())
        } else {
            None
        };
        if let Some(field_names) = typed_field_names {
            let field_name_refs: Vec<&str> = field_names.iter().map(|s| s.as_str()).collect();
            ensure_struct_adt_smtlib(self.enc, self.script, type_id, &field_name_refs);
            let mut ordered = fields.to_vec();
            ordered.sort_by_key(|(idx, _)| *idx);
            let args: Vec<String> = ordered
                .iter()
                .map(|(_, s)| self.load_slot(slots, *s))
                .collect();
            let val = self.enc.fresh_name();
            declare_int_var(self.script, self.vars, &val);
            let tag_fn = sanitize_smtlib_name(&crate::encode_atom_policy::adt_tag_uf_name(type_id));
            self.script
                .push_str(&format!("(assert (= ({tag_fn} {val}) 0))\n"));
            for (i, accessor) in field_names.iter().enumerate() {
                if let Some(arg) = args.get(i) {
                    let acc_fn = sanitize_smtlib_name(
                        &crate::encode_atom_policy::adt_accessor_uf_name(type_id, accessor),
                    );
                    self.script
                        .push_str(&format!("(assert (= ({acc_fn} {val}) {arg}))\n"));
                }
            }
            return val;
        }
        let args: Vec<String> = fields
            .iter()
            .map(|(_, s)| self.load_slot(slots, *s))
            .collect();
        let fname = sanitize_smtlib_name(&crate::encode_atom_policy::ir_construct_uf_name(type_id));
        format!("({fname} {})", args.join(" "))
    }
}

fn encode_ir_pred_arg_smtlib(
    arg: &IrPredArg,
    slots: &HashMap<usize, String>,
    enc: &mut IrSmtlibEncoder,
) -> String {
    match arg {
        IrPredArg::Slot(n) => slots.get(n).cloned().unwrap_or_else(|| enc.fresh_name()),
        IrPredArg::SlotResult => slots
            .get(&RESULT_SLOT)
            .cloned()
            .unwrap_or_else(|| crate::encode_atom_policy::RESULT_VAR_NAME.to_string()),
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
        declared_adts: HashSet::new(),
    };
    let mut slots: HashMap<usize, String> = HashMap::new();

    for (slot, name) in ir_param_names(func, contract_param_names) {
        declare_int_var(script, vars, &name);
        slots.insert(slot, sanitize_smtlib_name(&name));
    }

    declare_int_var(script, vars, "result");
    slots.insert(
        RESULT_SLOT,
        crate::encode_atom_policy::RESULT_VAR_NAME.to_string(),
    );

    let slot_to_name: HashMap<usize, String> = ir_param_names(func, contract_param_names)
        .into_iter()
        .collect();
    let slot_types = slot_type_map(func);

    let mut builder = SmtlibIrBuilder {
        enc: &mut enc,
        script,
        vars,
        slot_to_name: &slot_to_name,
        slot_types: &slot_types,
        enc_ctx,
    };

    crate::ir_exec::apply_ir_body_constraints(&mut builder, func, contract_param_names, &mut slots);
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
