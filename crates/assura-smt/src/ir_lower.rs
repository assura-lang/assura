//! Shared IR expression lowering for Z3, CVC5 native, and CVC5 SMT-LIB backends (#290).

use std::collections::HashMap;

use crate::havoc_assume::RESULT_SLOT;
use crate::ir::{IrArithOp, IrCmpOp, IrExprKind, IrLiteral};
use crate::ir_encode::{IrEncodeContext, is_length_ir_call, slot_type_map};

/// Block-local result slot name (#297).
pub fn block_result_name(block_id: usize) -> String {
    format!("__ir_block{block_id}_result")
}

/// Block-local temporary slot name.
pub fn block_slot_name(block_id: usize, slot: usize) -> String {
    format!("__ir_block{block_id}_slot_{slot}")
}

/// Nullary UF fallback when a sibling `fn #N` body is missing from the block map (#296).
pub fn missing_block_uf_name(block_id: usize) -> String {
    format!("__ir_block_{block_id}")
}

/// Prefix for inlined callee IR slots.
pub fn call_prefix(func: &str) -> String {
    format!("__ir_call_{func}_")
}

/// Slot naming/type metadata for one IR lowering scope.
#[derive(Clone, Copy)]
pub struct IrSlotContext<'a> {
    pub slot_to_name: &'a HashMap<usize, String>,
    pub slot_types: &'a HashMap<usize, String>,
}

/// Backend-neutral term construction for shared IR lowering.
pub trait IrTermBuilder {
    type Term: Clone;

    fn int_const(&mut self, n: i64) -> Self::Term;
    fn get_or_create_named(&mut self, name: &str) -> Self::Term;
    fn load_slot(&mut self, slots: &HashMap<usize, Self::Term>, slot: usize) -> Self::Term;
    fn push_eq_axiom(&mut self, lhs: Self::Term, rhs: Self::Term);
    fn arith(&mut self, op: IrArithOp, lhs: Self::Term, rhs: Self::Term) -> Self::Term;
    fn cmp_as_int(&mut self, op: IrCmpOp, lhs: Self::Term, rhs: Self::Term) -> Self::Term;
    fn ite_nonzero(
        &mut self,
        cond: Self::Term,
        then_v: Self::Term,
        else_v: Self::Term,
    ) -> Self::Term;
    fn nullary_uf(&mut self, name: &str) -> Self::Term;
    fn unary_uf(&mut self, name: &str, arg: Self::Term) -> Self::Term;
    fn nary_uf(&mut self, name: &str, args: &[Self::Term]) -> Self::Term;
    fn fresh_int(&mut self) -> Self::Term;

    fn enc_ctx(&self) -> IrEncodeContext<'_>;

    fn canonical_length_for_name(&mut self, name: &str) -> Self::Term;

    fn try_known_builtin(&mut self, func: &str, args: &[Self::Term]) -> Option<Self::Term> {
        let _ = (func, args);
        None
    }

    fn encode_field(
        &mut self,
        slot: usize,
        index: usize,
        slots: &HashMap<usize, Self::Term>,
        ctx: IrSlotContext<'_>,
    ) -> Self::Term;

    fn encode_construct(
        &mut self,
        type_id: &str,
        fields: &[(usize, usize)],
        slots: &HashMap<usize, Self::Term>,
        ctx: IrSlotContext<'_>,
    ) -> Self::Term;

    fn encode_transition(
        &mut self,
        slot: usize,
        state: &str,
        slots: &HashMap<usize, Self::Term>,
    ) -> Self::Term {
        let val = self.load_slot(slots, slot);
        self.unary_uf(&format!("__ir_state_{state}"), val)
    }
}

/// Evaluate a sibling `fn #N` block with a block-local `RESULT_SLOT` (#297).
pub fn eval_ir_block<B: IrTermBuilder>(
    builder: &mut B,
    block_id: usize,
    slots: &HashMap<usize, B::Term>,
    ctx: IrSlotContext<'_>,
) -> Option<B::Term> {
    let body = builder.enc_ctx().ir_blocks?.get(&block_id).cloned()?;
    let mut local = slots.clone();
    let block_result = builder.get_or_create_named(&block_result_name(block_id));
    local.insert(RESULT_SLOT, block_result);
    let mut last = None;
    for instr in body {
        if instr.target != RESULT_SLOT && !local.contains_key(&instr.target) {
            let name = block_slot_name(block_id, instr.target);
            local.insert(instr.target, builder.get_or_create_named(&name));
        }
        let computed = encode_ir_expr(builder, &instr.expr, &local, ctx);
        if let Some(target) = local.get(&instr.target) {
            builder.push_eq_axiom(computed.clone(), target.clone());
        }
        last = local.get(&instr.target).cloned();
    }
    last
}

/// Inline a callee IR body when present in `enc_ctx.ir_bodies`.
pub fn eval_ir_call<B: IrTermBuilder>(
    builder: &mut B,
    func: &str,
    args: &[usize],
    slots: &HashMap<usize, B::Term>,
    _outer_ctx: IrSlotContext<'_>,
) -> Option<B::Term> {
    let enc_ctx = builder.enc_ctx();
    let callee = enc_ctx.callee_ir(func)?.clone();
    if callee.params.len() != args.len() {
        return None;
    }

    let prefix = call_prefix(func);
    let mut local: HashMap<usize, B::Term> = HashMap::new();

    for (i, param) in callee.params.iter().enumerate() {
        let arg_val = builder.load_slot(slots, args[i]);
        let name = format!("{prefix}param_{}", param.slot);
        let slot_var = builder.get_or_create_named(&name);
        builder.push_eq_axiom(arg_val, slot_var.clone());
        local.insert(param.slot, slot_var);
    }

    let result_var = builder.get_or_create_named(&format!("{prefix}result"));
    local.insert(RESULT_SLOT, result_var);

    let callee_slot_types = slot_type_map(&callee);
    let callee_names: HashMap<usize, String> = callee
        .params
        .iter()
        .map(|p| (p.slot, format!("{prefix}param_{}", p.slot)))
        .collect();
    let callee_ctx = IrSlotContext {
        slot_to_name: &callee_names,
        slot_types: &callee_slot_types,
    };

    for instr in &callee.body {
        if instr.target != RESULT_SLOT && !local.contains_key(&instr.target) {
            let name = format!("{prefix}slot_{}", instr.target);
            local.insert(instr.target, builder.get_or_create_named(&name));
        }
        let computed = encode_ir_expr(builder, &instr.expr, &local, callee_ctx);
        if let Some(target) = local.get(&instr.target) {
            builder.push_eq_axiom(computed.clone(), target.clone());
        }
    }

    local.get(&RESULT_SLOT).cloned()
}

/// Lower an IR expression to a solver term.
pub fn encode_ir_expr<B: IrTermBuilder>(
    builder: &mut B,
    expr: &IrExprKind,
    slots: &HashMap<usize, B::Term>,
    ctx: IrSlotContext<'_>,
) -> B::Term {
    match expr {
        IrExprKind::Const(IrLiteral::Int(n)) => builder.int_const(*n),
        IrExprKind::Const(IrLiteral::Float(f)) => builder.int_const(*f as i64),
        IrExprKind::Const(IrLiteral::Bool(b)) => builder.int_const(if *b { 1 } else { 0 }),
        IrExprKind::Const(IrLiteral::Str(_)) => builder.fresh_int(),
        IrExprKind::Load(slot) => builder.load_slot(slots, *slot),
        IrExprKind::Arith { op, lhs, rhs } => {
            let l = builder.load_slot(slots, *lhs);
            let r = builder.load_slot(slots, *rhs);
            builder.arith(*op, l, r)
        }
        IrExprKind::Cmp { op, lhs, rhs } => {
            let l = builder.load_slot(slots, *lhs);
            let r = builder.load_slot(slots, *rhs);
            builder.cmp_as_int(*op, l, r)
        }
        IrExprKind::Call { func, args } => {
            if is_length_ir_call(func, args.len())
                && let Some(slot) = args.first()
                && let Some(name) = ctx.slot_to_name.get(slot)
            {
                return builder.canonical_length_for_name(name);
            }
            if let Some(inlined) = eval_ir_call(builder, func, args, slots, ctx) {
                return inlined;
            }
            let arg_terms: Vec<B::Term> =
                args.iter().map(|a| builder.load_slot(slots, *a)).collect();
            if let Some(builtin) = builder.try_known_builtin(func, &arg_terms) {
                return builtin;
            }
            builder.nary_uf(&format!("__ir_call_{func}"), &arg_terms)
        }
        IrExprKind::Field { slot, index } => builder.encode_field(*slot, *index, slots, ctx),
        IrExprKind::Construct { type_id, fields } => {
            builder.encode_construct(type_id, fields, slots, ctx)
        }
        IrExprKind::Cast { slot, .. } => builder.load_slot(slots, *slot),
        IrExprKind::Transition { slot, state } => builder.encode_transition(*slot, state, slots),
        IrExprKind::If {
            cond,
            then_block,
            else_block,
        } => {
            let cond_val = builder.load_slot(slots, *cond);
            let then_val = eval_ir_block(builder, *then_block, slots, ctx)
                .unwrap_or_else(|| builder.nullary_uf(&missing_block_uf_name(*then_block)));
            let else_val = eval_ir_block(builder, *else_block, slots, ctx)
                .unwrap_or_else(|| builder.nullary_uf(&missing_block_uf_name(*else_block)));
            builder.ite_nonzero(cond_val, then_val, else_val)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::parse_ir_module;
    use crate::ir_encode::branch_if_else_ir_fixture;

    #[derive(Clone, Default)]
    struct MockTerm(String);

    struct MockIrBuilder<'a> {
        enc_ctx: IrEncodeContext<'a>,
        slot_to_name: HashMap<usize, String>,
        slot_types: HashMap<usize, String>,
        named: Vec<String>,
        eq_axioms: Vec<(String, String)>,
        nullary_ufs: Vec<String>,
        fresh: usize,
    }

    impl<'a> MockIrBuilder<'a> {
        fn new(enc_ctx: IrEncodeContext<'a>) -> Self {
            Self {
                enc_ctx,
                slot_to_name: HashMap::new(),
                slot_types: HashMap::new(),
                named: Vec::new(),
                eq_axioms: Vec::new(),
                nullary_ufs: Vec::new(),
                fresh: 0,
            }
        }
    }

    impl IrTermBuilder for MockIrBuilder<'_> {
        type Term = MockTerm;

        fn int_const(&mut self, n: i64) -> Self::Term {
            MockTerm(n.to_string())
        }

        fn get_or_create_named(&mut self, name: &str) -> Self::Term {
            self.named.push(name.to_string());
            MockTerm(name.to_string())
        }

        fn load_slot(&mut self, slots: &HashMap<usize, Self::Term>, slot: usize) -> Self::Term {
            slots
                .get(&slot)
                .cloned()
                .unwrap_or_else(|| MockTerm(format!("fresh_{}", self.fresh)))
        }

        fn push_eq_axiom(&mut self, lhs: Self::Term, rhs: Self::Term) {
            self.eq_axioms.push((lhs.0, rhs.0));
        }

        fn arith(&mut self, op: IrArithOp, lhs: Self::Term, rhs: Self::Term) -> Self::Term {
            MockTerm(format!("arith_{op:?}_{}_{}", lhs.0, rhs.0))
        }

        fn cmp_as_int(&mut self, op: IrCmpOp, lhs: Self::Term, rhs: Self::Term) -> Self::Term {
            MockTerm(format!("cmp_{op:?}_{}_{}", lhs.0, rhs.0))
        }

        fn ite_nonzero(
            &mut self,
            cond: Self::Term,
            then_v: Self::Term,
            else_v: Self::Term,
        ) -> Self::Term {
            MockTerm(format!("ite_{}_{}_{}", cond.0, then_v.0, else_v.0))
        }

        fn nullary_uf(&mut self, name: &str) -> Self::Term {
            self.nullary_ufs.push(name.to_string());
            MockTerm(name.to_string())
        }

        fn unary_uf(&mut self, name: &str, arg: Self::Term) -> Self::Term {
            MockTerm(format!("{name}({})", arg.0))
        }

        fn nary_uf(&mut self, name: &str, args: &[Self::Term]) -> Self::Term {
            let joined = args
                .iter()
                .map(|t| t.0.as_str())
                .collect::<Vec<_>>()
                .join(",");
            MockTerm(format!("{name}({joined})"))
        }

        fn fresh_int(&mut self) -> Self::Term {
            let n = self.fresh;
            self.fresh += 1;
            MockTerm(format!("fresh_{n}"))
        }

        fn enc_ctx(&self) -> IrEncodeContext<'_> {
            self.enc_ctx
        }

        fn canonical_length_for_name(&mut self, name: &str) -> Self::Term {
            MockTerm(format!("len_{name}"))
        }

        fn encode_field(
            &mut self,
            slot: usize,
            index: usize,
            slots: &HashMap<usize, Self::Term>,
            _ctx: IrSlotContext<'_>,
        ) -> Self::Term {
            let base = self.load_slot(slots, slot);
            MockTerm(format!("field_{index}({})", base.0))
        }

        fn encode_construct(
            &mut self,
            type_id: &str,
            fields: &[(usize, usize)],
            slots: &HashMap<usize, Self::Term>,
            _ctx: IrSlotContext<'_>,
        ) -> Self::Term {
            let args = fields
                .iter()
                .map(|(_, s)| self.load_slot(slots, *s).0)
                .collect::<Vec<_>>()
                .join(",");
            MockTerm(format!("construct_{type_id}({args})"))
        }
    }

    #[test]
    fn ir_lower_block_result_scoped() {
        let (func, blocks) = branch_if_else_ir_fixture();
        let enc_ctx = IrEncodeContext::new(None, None, Some(&blocks));
        let mut builder = MockIrBuilder::new(enc_ctx);
        builder.slot_to_name.insert(0, "x".into());

        let outer_result = builder.get_or_create_named("result");
        let mut slots = HashMap::new();
        slots.insert(0, MockTerm("x".into()));
        slots.insert(RESULT_SLOT, outer_result);

        let slot_to_name = builder.slot_to_name.clone();
        let slot_types = builder.slot_types.clone();
        let ctx = IrSlotContext {
            slot_to_name: &slot_to_name,
            slot_types: &slot_types,
        };
        let if_expr = func.body[0].expr.clone();
        let _ = encode_ir_expr(&mut builder, &if_expr, &slots, ctx);

        assert!(
            builder.named.iter().any(|n| n == "__ir_block1_result"),
            "then-branch should declare block-local result, named={:?}",
            builder.named
        );
        assert!(
            builder.named.iter().any(|n| n == "__ir_block2_result"),
            "else-branch should declare block-local result, named={:?}",
            builder.named
        );
        assert!(
            !builder
                .eq_axioms
                .iter()
                .any(|(l, r)| l == "x" && r == "result"),
            "must not bind x to main result from branch blocks, axioms={:?}",
            builder.eq_axioms
        );
        assert!(
            !builder
                .eq_axioms
                .iter()
                .any(|(l, r)| l == "0" && r == "result"),
            "must not bind 0 to main result from branch blocks, axioms={:?}",
            builder.eq_axioms
        );
    }

    #[test]
    fn ir_lower_missing_block_uf_name() {
        let func = crate::ir_encode::branch_if_else_missing_blocks_fixture();
        let enc_ctx = IrEncodeContext::default();
        let mut builder = MockIrBuilder::new(enc_ctx);
        builder.slot_to_name.insert(0, "x".into());

        let mut slots = HashMap::new();
        slots.insert(0, MockTerm("x".into()));

        let slot_to_name = builder.slot_to_name.clone();
        let slot_types = builder.slot_types.clone();
        let ctx = IrSlotContext {
            slot_to_name: &slot_to_name,
            slot_types: &slot_types,
        };
        let if_expr = func.body[0].expr.clone();
        let _ = encode_ir_expr(&mut builder, &if_expr, &slots, ctx);

        assert_eq!(
            builder.nullary_ufs,
            vec![missing_block_uf_name(99), missing_block_uf_name(100),]
        );
    }

    #[test]
    fn naming_helpers_match_legacy_formats() {
        assert_eq!(block_result_name(1), "__ir_block1_result");
        assert_eq!(block_slot_name(2, 3), "__ir_block2_slot_3");
        assert_eq!(missing_block_uf_name(99), "__ir_block_99");
        assert_eq!(call_prefix("double"), "__ir_call_double_");
    }

    #[test]
    fn eval_ir_call_uses_call_prefix() {
        let main = parse_ir_module(
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
        .unwrap();
        let helper = parse_ir_module(
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
        .unwrap();
        let mut bodies = HashMap::new();
        bodies.insert("double".into(), helper.functions[0].clone());
        let enc_ctx = IrEncodeContext::new(None, Some(&bodies), None);
        let mut builder = MockIrBuilder::new(enc_ctx);

        let mut slots = HashMap::new();
        slots.insert(0, MockTerm("x".into()));

        let slot_to_name = builder.slot_to_name.clone();
        let slot_types = builder.slot_types.clone();
        let ctx = IrSlotContext {
            slot_to_name: &slot_to_name,
            slot_types: &slot_types,
        };
        let call_expr = main.functions[0].body[0].expr.clone();
        let _ = encode_ir_expr(&mut builder, &call_expr, &slots, ctx);

        assert!(
            builder
                .named
                .iter()
                .any(|n| n == "__ir_call_double_param_0"),
            "call inlining should use call_prefix, named={:?}",
            builder.named
        );
    }
}
