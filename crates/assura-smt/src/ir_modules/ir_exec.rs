//! Single IR body executor for havoc+assume verification.
//!
//! All solver backends (Z3, CVC5 native, CVC5 SMT-LIB) should apply implementation
//! IR through [`apply_ir_body_constraints`] so IR semantics live in one place.
//! Backends implement [`crate::ir_lower::IrTermBuilder`] (term construction only).

use std::collections::HashMap;

use crate::havoc_assume::{RESULT_SLOT, ir_param_names};
use crate::ir::{IrExprKind, IrFunction};
use crate::ir_encode::slot_type_map;
use crate::ir_lower::{IrSlotContext, IrTermBuilder, encode_ir_expr};

/// Walk `func.body`, bind params/`$result`, emit per-instruction equality axioms,
/// and apply result/post special cases via [`IrTermBuilder`] hooks.
///
/// Callers must pre-populate `slots` with parameter slots and `RESULT_SLOT`
/// (mapped to the contract `result` variable). This function allocates intermediate
/// `__ir_slot_N` temporaries as needed.
pub fn apply_ir_body_constraints<B: IrTermBuilder>(
    builder: &mut B,
    func: &IrFunction,
    contract_param_names: &[String],
    slots: &mut HashMap<usize, B::Term>,
) {
    let slot_to_name: HashMap<usize, String> = ir_param_names(func, contract_param_names)
        .into_iter()
        .collect();
    let slot_types = slot_type_map(func);
    let ctx = IrSlotContext {
        slot_to_name: &slot_to_name,
        slot_types: &slot_types,
    };

    for instr in &func.body {
        if instr.target != RESULT_SLOT && !slots.contains_key(&instr.target) {
            let name = crate::encode_atom_policy::ir_slot_name(instr.target);
            let v = builder.get_or_create_named(&name);
            slots.insert(instr.target, v);
        }
        let computed = encode_ir_expr(builder, &instr.expr, slots, ctx);
        if let Some(target) = slots.get(&instr.target) {
            builder.push_eq_axiom(computed, target.clone());
        }
        if instr.target == RESULT_SLOT
            && let IrExprKind::Load(src) = &instr.expr
            && let Some(param) = slot_to_name.get(src)
        {
            builder.on_result_load_from_param(param);
        }
        if instr.target == RESULT_SLOT
            && let IrExprKind::Construct { type_id, .. } = &instr.expr
        {
            builder.on_result_construct(type_id);
        }
    }

    if let Some(post) = &func.post {
        builder.push_ir_post(post, slots);
    }
}

#[cfg(test)]
mod tests {
    use crate::havoc_assume::ir_param_names;

    /// Convenience for unit tests: allocate param + result slots, then apply body.
    fn apply_ir_body_with_named_slots<B: IrTermBuilder>(
        builder: &mut B,
        func: &IrFunction,
        contract_param_names: &[String],
        mut bind_name: impl FnMut(&mut B, &str) -> B::Term,
    ) -> HashMap<usize, B::Term> {
        let mut slots: HashMap<usize, B::Term> = HashMap::new();
        for (slot, name) in ir_param_names(func, contract_param_names) {
            let v = bind_name(builder, &name);
            slots.insert(slot, v);
        }
        let result = bind_name(builder, "result");
        slots.insert(RESULT_SLOT, result);
        apply_ir_body_constraints(builder, func, contract_param_names, &mut slots);
        slots
    }

    use super::*;
    use crate::ir::parse_ir_module;
    use crate::ir_encode::IrEncodeContext;
    use crate::ir_lower::IrTermBuilder;
    use std::cell::RefCell;

    /// Minimal term = i64; axioms recorded as equality of debug strings.
    #[derive(Clone, Debug, PartialEq, Eq)]
    struct T(i64);

    struct TestBuilder {
        next: i64,
        axioms: RefCell<Vec<(String, String)>>,
        names: RefCell<HashMap<String, T>>,
        posts: RefCell<usize>,
    }

    impl TestBuilder {
        fn new() -> Self {
            Self {
                next: 100,
                axioms: RefCell::new(Vec::new()),
                names: RefCell::new(HashMap::new()),
                posts: RefCell::new(0),
            }
        }
    }

    impl IrTermBuilder for TestBuilder {
        type Term = T;

        fn int_const(&mut self, n: i64) -> Self::Term {
            T(n)
        }

        fn get_or_create_named(&mut self, name: &str) -> Self::Term {
            let mut names = self.names.borrow_mut();
            if let Some(t) = names.get(name) {
                return t.clone();
            }
            let t = T(self.next);
            self.next += 1;
            names.insert(name.to_string(), t.clone());
            t
        }

        fn load_slot(&mut self, slots: &HashMap<usize, Self::Term>, slot: usize) -> Self::Term {
            slots
                .get(&slot)
                .cloned()
                .unwrap_or_else(|| self.fresh_int())
        }

        fn push_eq_axiom(&mut self, lhs: Self::Term, rhs: Self::Term) {
            self.axioms
                .borrow_mut()
                .push((format!("{:?}", lhs), format!("{:?}", rhs)));
        }

        fn arith(
            &mut self,
            _op: crate::ir::IrArithOp,
            lhs: Self::Term,
            rhs: Self::Term,
        ) -> Self::Term {
            T(lhs.0.wrapping_add(rhs.0).wrapping_mul(31))
        }

        fn cmp_as_int(
            &mut self,
            _op: crate::ir::IrCmpOp,
            _lhs: Self::Term,
            _rhs: Self::Term,
        ) -> Self::Term {
            T(0)
        }

        fn ite_nonzero(
            &mut self,
            _cond: Self::Term,
            then_v: Self::Term,
            _else_v: Self::Term,
        ) -> Self::Term {
            then_v
        }

        fn nullary_uf(&mut self, _name: &str) -> Self::Term {
            self.fresh_int()
        }

        fn unary_uf(&mut self, _name: &str, _arg: Self::Term) -> Self::Term {
            self.fresh_int()
        }

        fn nary_uf(&mut self, _name: &str, _args: &[Self::Term]) -> Self::Term {
            self.fresh_int()
        }

        fn fresh_int(&mut self) -> Self::Term {
            let t = T(self.next);
            self.next += 1;
            t
        }

        fn enc_ctx(&self) -> IrEncodeContext<'_> {
            IrEncodeContext::new(None, None, None)
        }

        fn canonical_length_for_name(&mut self, name: &str) -> Self::Term {
            self.get_or_create_named(&crate::encode_atom_policy::ir_exec_len_name(name))
        }

        fn encode_field(
            &mut self,
            _slot: usize,
            _index: usize,
            _slots: &HashMap<usize, Self::Term>,
            _ctx: IrSlotContext<'_>,
        ) -> Self::Term {
            self.fresh_int()
        }

        fn encode_construct(
            &mut self,
            _type_id: &str,
            _fields: &[(usize, usize)],
            _slots: &HashMap<usize, Self::Term>,
            _ctx: IrSlotContext<'_>,
        ) -> Self::Term {
            self.fresh_int()
        }

        fn push_ir_post(&mut self, _pred: &crate::ir::IrPred, _slots: &HashMap<usize, Self::Term>) {
            *self.posts.borrow_mut() += 1;
        }
    }

    #[test]
    fn inc_one_body_emits_slot_and_result_axioms() {
        let ir_source = r#"
module inc {
  fn #0 : ($0: Int) -> Int ! pure
  {
    $1 = const 1 : Int
    $2 = arith add $0 $1 : Int
    $result = load $2 : Int
  }
}
"#;
        let func = parse_ir_module(ir_source).unwrap().functions[0].clone();
        let mut builder = TestBuilder::new();
        let params = vec!["x".into()];
        apply_ir_body_with_named_slots(&mut builder, &func, &params, |b, name| {
            b.get_or_create_named(name)
        });
        let axioms = builder.axioms.borrow();
        // const, arith, load => at least 3 eq axioms
        assert!(
            axioms.len() >= 3,
            "expected IR instruction axioms, got {}",
            axioms.len()
        );
        assert!(builder.names.borrow().contains_key("x"));
        assert!(builder.names.borrow().contains_key("result"));
        assert!(builder.names.borrow().contains_key("__ir_slot_1"));
        assert!(builder.names.borrow().contains_key("__ir_slot_2"));
    }
}
