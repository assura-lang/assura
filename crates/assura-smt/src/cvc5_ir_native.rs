//! Native CVC5 encoding for havoc-assume IR bodies.

#[cfg(feature = "cvc5-verify")]
use crate::cvc5_common::sanitize_smtlib_name;
#[cfg(feature = "cvc5-verify")]
use crate::cvc5_encoder_state::{Cvc5EncoderState, canonical_length_cvc5};
#[cfg(feature = "cvc5-verify")]
use crate::cvc5_native_builtins::encode_known_builtin_cvc5;
#[cfg(feature = "cvc5-verify")]
use crate::ir_encode::{IrEncodeContext, is_collection_ir_type, slot_type_map};
#[cfg(feature = "cvc5-verify")]
use crate::ir_lower::{IrSlotContext, IrTermBuilder, encode_ir_expr};
use crate::ir_type_ctx::base_type_name;

#[cfg(feature = "cvc5-verify")]
struct Cvc5IrBuilder<'a, 'b> {
    tm: &'a cvc5::TermManager,
    slots: HashMap<usize, cvc5::Term<'a>>,
    vars: &'a mut HashMap<String, cvc5::Term<'a>>,
    state: &'a mut Cvc5EncoderState<'a>,
    slot_to_name: &'b HashMap<usize, String>,
    slot_types: &'b HashMap<usize, String>,
    enc_ctx: IrEncodeContext<'b>,
}

#[cfg(feature = "cvc5-verify")]
use std::collections::HashMap;

#[cfg(feature = "cvc5-verify")]
impl<'a> Cvc5IrBuilder<'a, '_> {
    fn mk_named_const(&mut self, name: &str) -> cvc5::Term<'a> {
        let key = sanitize_smtlib_name(name);
        self.vars
            .entry(key.clone())
            .or_insert_with(|| self.tm.mk_const(self.tm.integer_sort(), &key))
            .clone()
    }

    fn mk_fresh_const(&mut self) -> cvc5::Term<'a> {
        let name = format!("__fresh_{}", self.state.fresh_counter);
        self.state.fresh_counter += 1;
        self.tm.mk_const(self.tm.integer_sort(), &name)
    }

    fn mk_unary_uf(&mut self, name: &str, arg: cvc5::Term<'a>) -> cvc5::Term<'a> {
        self.mk_nary_uf(name, &[arg])
    }

    fn mk_nary_uf(&mut self, name: &str, args: &[cvc5::Term<'a>]) -> cvc5::Term<'a> {
        let key = sanitize_smtlib_name(name);
        let domain: Vec<cvc5::Sort<'_>> = (0..args.len()).map(|_| self.tm.integer_sort()).collect();
        let fun_sort = if domain.is_empty() {
            self.tm.integer_sort()
        } else {
            self.tm.mk_fun_sort(&domain, self.tm.integer_sort())
        };
        let decl = self.tm.mk_const(fun_sort, &key);
        if args.is_empty() {
            decl
        } else {
            let mut apply_args = Vec::with_capacity(1 + args.len());
            apply_args.push(decl);
            apply_args.extend_from_slice(args);
            self.tm.mk_term(cvc5::Kind::ApplyUf, &apply_args)
        }
    }
}

#[cfg(feature = "cvc5-verify")]
impl<'a> IrTermBuilder for Cvc5IrBuilder<'a, '_> {
    type Term = cvc5::Term<'a>;

    fn int_const(&mut self, n: i64) -> Self::Term {
        self.tm.mk_integer(n)
    }

    fn get_or_create_named(&mut self, name: &str) -> Self::Term {
        self.mk_named_const(name)
    }

    fn load_slot(&mut self, slots: &HashMap<usize, Self::Term>, slot: usize) -> Self::Term {
        slots
            .get(&slot)
            .cloned()
            .unwrap_or_else(|| self.mk_fresh_const())
    }

    fn push_eq_axiom(&mut self, lhs: Self::Term, rhs: Self::Term) {
        self.state
            .axioms
            .push(self.tm.mk_term(cvc5::Kind::Equal, &[lhs, rhs]));
    }

    fn arith(&mut self, op: crate::ir::IrArithOp, lhs: Self::Term, rhs: Self::Term) -> Self::Term {
        use crate::ir::IrArithOp;
        match op {
            IrArithOp::Add => self.tm.mk_term(cvc5::Kind::Add, &[lhs, rhs]),
            IrArithOp::Sub => self.tm.mk_term(cvc5::Kind::Sub, &[lhs, rhs]),
            IrArithOp::Mul => self.tm.mk_term(cvc5::Kind::Mult, &[lhs, rhs]),
            IrArithOp::Div => self.tm.mk_term(cvc5::Kind::IntsDivision, &[lhs, rhs]),
            IrArithOp::Mod => self.tm.mk_term(cvc5::Kind::IntsModulus, &[lhs, rhs]),
        }
    }

    fn cmp_as_int(
        &mut self,
        op: crate::ir::IrCmpOp,
        lhs: Self::Term,
        rhs: Self::Term,
    ) -> Self::Term {
        let b = match op {
            crate::ir::IrCmpOp::Eq => self.tm.mk_term(cvc5::Kind::Equal, &[lhs, rhs]),
            crate::ir::IrCmpOp::Ne => self.tm.mk_term(
                cvc5::Kind::Not,
                &[self.tm.mk_term(cvc5::Kind::Equal, &[lhs, rhs])],
            ),
            crate::ir::IrCmpOp::Lt => self.tm.mk_term(cvc5::Kind::Lt, &[lhs, rhs]),
            crate::ir::IrCmpOp::Le => self.tm.mk_term(cvc5::Kind::Leq, &[lhs, rhs]),
            crate::ir::IrCmpOp::Gt => self.tm.mk_term(cvc5::Kind::Gt, &[lhs, rhs]),
            crate::ir::IrCmpOp::Ge => self.tm.mk_term(cvc5::Kind::Geq, &[lhs, rhs]),
        };
        self.tm.mk_term(
            cvc5::Kind::Ite,
            &[b, self.tm.mk_integer(1), self.tm.mk_integer(0)],
        )
    }

    fn ite_nonzero(
        &mut self,
        cond: Self::Term,
        then_v: Self::Term,
        else_v: Self::Term,
    ) -> Self::Term {
        let zero = self.tm.mk_integer(0);
        let cond_bool = self.tm.mk_term(cvc5::Kind::Distinct, &[cond, zero]);
        self.tm
            .mk_term(cvc5::Kind::Ite, &[cond_bool, then_v, else_v])
    }

    fn nullary_uf(&mut self, name: &str) -> Self::Term {
        self.mk_nary_uf(name, &[])
    }

    fn unary_uf(&mut self, name: &str, arg: Self::Term) -> Self::Term {
        self.mk_unary_uf(name, arg)
    }

    fn nary_uf(&mut self, name: &str, args: &[Self::Term]) -> Self::Term {
        self.mk_nary_uf(name, args)
    }

    fn fresh_int(&mut self) -> Self::Term {
        self.mk_fresh_const()
    }

    fn enc_ctx(&self) -> IrEncodeContext<'_> {
        self.enc_ctx
    }

    fn canonical_length_for_name(&mut self, name: &str) -> Self::Term {
        canonical_length_cvc5(self.tm, name, self.vars, self.state)
    }

    fn try_known_builtin(&mut self, func: &str, args: &[Self::Term]) -> Option<Self::Term> {
        encode_known_builtin_cvc5(self.tm, func, args, self.state)
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
            return self.mk_unary_uf(&format!("__adt_{type_name}_{field_name}"), base);
        }
        let ty_suffix = ctx
            .slot_types
            .get(&slot)
            .map(|t| t.replace('<', "_").replace('>', ""))
            .unwrap_or_else(|| "val".into());
        self.mk_unary_uf(&format!("__ir_field_{ty_suffix}_{index}"), base)
    }

    fn encode_construct(
        &mut self,
        type_id: &str,
        fields: &[(usize, usize)],
        slots: &HashMap<usize, Self::Term>,
        _ctx: IrSlotContext<'_>,
    ) -> Self::Term {
        if self.enc_ctx.type_ctx.has_struct_layout(type_id) {
            return encode_ir_construct_typed_cvc5(self, type_id, fields, slots);
        }
        let args: Vec<cvc5::Term<'a>> = fields
            .iter()
            .map(|(_, s)| self.load_slot(slots, *s))
            .collect();
        self.mk_nary_uf(&format!("__ir_construct_{type_id}"), &args)
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
    builder: &mut Cvc5IrBuilder<'a, '_>,
    type_id: &str,
    fields: &[(usize, usize)],
    slots: &HashMap<usize, cvc5::Term<'a>>,
) -> cvc5::Term<'a> {
    use crate::cvc5_adt::adt_constructor_cvc5_native;

    let field_names: Vec<&str> = builder
        .enc_ctx
        .type_ctx
        .field_names_for(type_id)
        .unwrap_or_default();
    ensure_struct_adt_cvc5(builder.tm, builder.state, type_id, &field_names);

    let mut ordered = fields.to_vec();
    ordered.sort_by_key(|(idx, _)| *idx);
    let arg_terms: Vec<cvc5::Term<'a>> = ordered
        .iter()
        .map(|(_, s)| builder.load_slot(slots, *s))
        .collect();

    let ctor = builder
        .state
        .struct_adt_defs
        .get(type_id)
        .and_then(|d| d.constructors.first())
        .expect("struct ADT has one constructor")
        .clone();
    let symbols = builder
        .state
        .struct_adt_symbols
        .get(type_id)
        .expect("struct ADT symbols");

    adt_constructor_cvc5_native(
        builder.tm,
        symbols,
        &ctor,
        &arg_terms,
        &mut builder.state.axioms,
        &mut builder.state.fresh_counter,
    )
}

#[cfg(feature = "cvc5-verify")]
fn encode_ir_pred_arg_cvc5<'a>(
    tm: &'a cvc5::TermManager,
    arg: &crate::ir::IrPredArg,
    slots: &HashMap<usize, cvc5::Term<'a>>,
    vars: &mut HashMap<String, cvc5::Term<'a>>,
    state: &mut Cvc5EncoderState<'a>,
) -> cvc5::Term<'a> {
    use crate::havoc_assume::RESULT_SLOT;
    use crate::ir::{IrArithOp, IrLiteral, IrPredArg};

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
            match op {
                IrArithOp::Add => tm.mk_term(cvc5::Kind::Add, &[l, r]),
                IrArithOp::Sub => tm.mk_term(cvc5::Kind::Sub, &[l, r]),
                IrArithOp::Mul => tm.mk_term(cvc5::Kind::Mult, &[l, r]),
                IrArithOp::Div => tm.mk_term(cvc5::Kind::IntsDivision, &[l, r]),
                IrArithOp::Mod => tm.mk_term(cvc5::Kind::IntsModulus, &[l, r]),
            }
        }
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
fn encode_ir_pred_cvc5<'a>(
    tm: &'a cvc5::TermManager,
    pred: &crate::ir::IrPred,
    slots: &HashMap<usize, cvc5::Term<'a>>,
    vars: &mut HashMap<String, cvc5::Term<'a>>,
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
    vars: &mut HashMap<String, cvc5::Term<'a>>,
    state: &mut Cvc5EncoderState<'a>,
    enc_ctx: IrEncodeContext<'a>,
) {
    use crate::havoc_assume::{RESULT_SLOT, ir_param_names};
    use crate::ir::IrExprKind;

    let mut slots: HashMap<usize, cvc5::Term<'a>> = HashMap::new();

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

    let slot_to_name: HashMap<usize, String> = ir_param_names(func, contract_param_names)
        .into_iter()
        .collect();
    let slot_types = slot_type_map(func);

    let ctx = IrSlotContext {
        slot_to_name: &slot_to_name,
        slot_types: &slot_types,
    };
    let mut builder = Cvc5IrBuilder {
        tm,
        slots: slots.clone(),
        vars,
        state,
        slot_to_name: &slot_to_name,
        slot_types: &slot_types,
        enc_ctx,
    };

    for instr in &func.body {
        if instr.target != RESULT_SLOT && !builder.slots.contains_key(&instr.target) {
            let key = sanitize_smtlib_name(&format!("__ir_slot_{}", instr.target));
            let v = builder
                .vars
                .entry(key.clone())
                .or_insert_with(|| tm.mk_const(tm.integer_sort(), &key))
                .clone();
            builder.slots.insert(instr.target, v);
        }
        let computed = encode_ir_expr(&mut builder, &instr.expr, &builder.slots, ctx);
        if let Some(target) = builder.slots.get(&instr.target) {
            builder
                .state
                .axioms
                .push(tm.mk_term(cvc5::Kind::Equal, &[computed, target.clone()]));
        }
        if instr.target == RESULT_SLOT
            && let IrExprKind::Load(src) = &instr.expr
            && let Some(param) = builder.slot_to_name.get(src)
        {
            let len_result = canonical_length_cvc5(tm, "result", builder.vars, builder.state);
            let len_param = canonical_length_cvc5(tm, param, builder.vars, builder.state);
            builder
                .state
                .axioms
                .push(tm.mk_term(cvc5::Kind::Equal, &[len_result, len_param]));
        }
        // Construct tag axiom: align with Z3 backend (#303)
        if instr.target == RESULT_SLOT
            && let IrExprKind::Construct { type_id, .. } = &instr.expr
        {
            let tag = crate::cvc5_builtins::pattern_hash_name(type_id);
            let tag_key = sanitize_smtlib_name(&format!("__ir_tag_{type_id}"));
            let tag_var = builder
                .vars
                .entry(tag_key.clone())
                .or_insert_with(|| tm.mk_const(tm.integer_sort(), &tag_key))
                .clone();
            let tag_lit = tm.mk_integer(tag);
            builder
                .state
                .axioms
                .push(tm.mk_term(cvc5::Kind::Equal, &[tag_var, tag_lit]));
        }
    }

    if let Some(post) = &func.post
        && let Some(pred) =
            encode_ir_pred_cvc5(tm, post, &builder.slots, builder.vars, builder.state)
    {
        builder.state.axioms.push(pred);
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
        let slots = HashMap::new();
        let mut vars = HashMap::new();
        let expr = IrExprKind::Arith {
            op: IrArithOp::Add,
            lhs: 0,
            rhs: 1,
        };
        let slot_to_name = HashMap::new();
        let slot_types = HashMap::new();
        let ctx = IrSlotContext {
            slot_to_name: &slot_to_name,
            slot_types: &slot_types,
        };
        let mut builder = Cvc5IrBuilder {
            tm: &tm,
            slots,
            vars: &mut vars,
            state: &mut state,
            slot_to_name: &slot_to_name,
            slot_types: &slot_types,
            enc_ctx: IrEncodeContext::default(),
        };
        let _ = encode_ir_expr(&mut builder, &expr, &builder.slots, ctx);
    }

    #[test]
    fn cvc5_ir_call_inlines_callee_sidecar() {
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

        let tm = cvc5::TermManager::new();
        let mut state = default_cvc5_encoder_state();
        let mut vars = HashMap::new();
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
        let mut vars = HashMap::new();
        apply_ir_body_constraints_cvc5(&tm, &func, &["x".into()], &mut vars, &mut state, enc_ctx);

        let text: String = state
            .axioms
            .iter()
            .map(|t| t.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        crate::ir_encode::assert_ir_blocks_inlined(&text, state.axioms.len());
    }
}
