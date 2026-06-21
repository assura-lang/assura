//! Z3 havoc+assume encoding: structural result axioms, cross-clause
//! length inference, and IR body constraints (#267).

use super::encoder::Encoder;
use crate::cvc5_builtins::{KnownBuiltin, classify_known_builtin, pattern_hash_name};
use crate::havoc_assume::{
    HavocAssumeInput, RESULT_SLOT, infer_length_identity_links, ir_param_names,
    is_collection_return,
};
use crate::ir::{IrArithOp, IrCmpOp, IrExprKind, IrFunction, IrLiteral, IrPred, IrPredArg};
use crate::ir_encode::{IrEncodeContext, is_collection_ir_type, slot_type_map};
use crate::ir_lower::{IrSlotContext, IrTermBuilder, encode_ir_expr};
use crate::ir_type_ctx::base_type_name;
use assura_ast::Clause;
use std::collections::HashMap;
use z3::ast;

struct Z3IrBuilder<'a, 'b> {
    encoder: &'a mut Encoder,
    #[allow(dead_code)]
    slot_to_name: &'b HashMap<usize, String>,
    #[allow(dead_code)]
    slot_types: &'b HashMap<usize, String>,
    enc_ctx: IrEncodeContext<'b>,
}

impl IrTermBuilder for Z3IrBuilder<'_, '_> {
    type Term = ast::Int;

    fn int_const(&mut self, n: i64) -> Self::Term {
        ast::Int::from_i64(n)
    }

    fn get_or_create_named(&mut self, name: &str) -> Self::Term {
        self.encoder.get_or_create_int(name)
    }

    fn load_slot(&mut self, slots: &HashMap<usize, Self::Term>, slot: usize) -> Self::Term {
        slots
            .get(&slot)
            .cloned()
            .unwrap_or_else(|| self.fresh_int())
    }

    fn push_eq_axiom(&mut self, lhs: Self::Term, rhs: Self::Term) {
        self.encoder.background_axioms.push(lhs.eq(&rhs));
    }

    fn arith(&mut self, op: IrArithOp, lhs: Self::Term, rhs: Self::Term) -> Self::Term {
        match op {
            IrArithOp::Add => ast::Int::add(&[&lhs, &rhs]),
            IrArithOp::Sub => ast::Int::sub(&[&lhs, &rhs]),
            IrArithOp::Mul => ast::Int::mul(&[&lhs, &rhs]),
            IrArithOp::Div => lhs.div(&rhs),
            IrArithOp::Mod => lhs.modulo(&rhs),
        }
    }

    fn cmp_as_int(&mut self, op: IrCmpOp, lhs: Self::Term, rhs: Self::Term) -> Self::Term {
        let b = match op {
            IrCmpOp::Eq => lhs.eq(&rhs),
            IrCmpOp::Ne => lhs.eq(&rhs).not(),
            IrCmpOp::Lt => lhs.lt(&rhs),
            IrCmpOp::Le => lhs.le(&rhs),
            IrCmpOp::Gt => lhs.gt(&rhs),
            IrCmpOp::Ge => lhs.ge(&rhs),
        };
        b.ite(&ast::Int::from_i64(1), &ast::Int::from_i64(0))
    }

    fn ite_nonzero(
        &mut self,
        cond: Self::Term,
        then_v: Self::Term,
        else_v: Self::Term,
    ) -> Self::Term {
        let cond_bool = cond.eq(ast::Int::from_i64(0)).not();
        cond_bool.ite(&then_v, &else_v)
    }

    fn nullary_uf(&mut self, name: &str) -> Self::Term {
        self.nary_uf(name, &[])
    }

    fn unary_uf(&mut self, name: &str, arg: Self::Term) -> Self::Term {
        self.nary_uf(name, &[arg])
    }

    fn nary_uf(&mut self, name: &str, args: &[Self::Term]) -> Self::Term {
        let decl = self.encoder.make_func(name, args.len());
        let ast_args: Vec<&dyn z3::ast::Ast> =
            args.iter().map(|i| i as &dyn z3::ast::Ast).collect();
        decl.apply(&ast_args)
            .as_int()
            .unwrap_or_else(|| self.fresh_int())
    }

    fn fresh_int(&mut self) -> Self::Term {
        self.encoder.fresh_int()
    }

    fn enc_ctx(&self) -> IrEncodeContext<'_> {
        self.enc_ctx
    }

    fn canonical_length_for_name(&mut self, name: &str) -> Self::Term {
        self.encoder.canonical_length(name)
    }

    fn try_known_builtin(&mut self, func: &str, args: &[Self::Term]) -> Option<Self::Term> {
        let kind = classify_known_builtin(func, args.len())?;
        let zero = ast::Int::from_i64(0);
        Some(match kind {
            KnownBuiltin::Abs => {
                let x = &args[0];
                let neg = ast::Int::sub(&[zero.clone(), x.clone()]);
                x.ge(&zero).ite(x, &neg)
            }
            KnownBuiltin::Min => {
                let (a, b) = (&args[0], &args[1]);
                a.le(b).ite(a, b)
            }
            KnownBuiltin::Max => {
                let (a, b) = (&args[0], &args[1]);
                a.ge(b).ite(a, b)
            }
            KnownBuiltin::Concat => ast::Int::add(&[&args[0], &args[1]]),
            _ => return None,
        })
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
            && let Some(name) = ctx.slot_to_name.get(&slot)
        {
            return self.encoder.canonical_length(name);
        }
        let base = self.load_slot(slots, slot);
        if let Some(ir_ty) = ctx.slot_types.get(&slot)
            && let Some(field_name) = self.enc_ctx.type_ctx.field_name_at(ir_ty, index)
        {
            let type_name = base_type_name(ir_ty);
            if let Some(names) = self.enc_ctx.type_ctx.field_names_for(type_name) {
                self.encoder.ensure_struct_adt(
                    type_name,
                    &names.into_iter().map(str::to_string).collect::<Vec<_>>(),
                );
                return self.encoder.adt_accessor(type_name, field_name, &base);
            }
        }
        let ty_suffix = ctx
            .slot_types
            .get(&slot)
            .map(|t| t.replace('<', "_").replace('>', ""))
            .unwrap_or_else(|| "val".into());
        self.unary_uf(&format!("__ir_field_{ty_suffix}_{index}"), base)
    }

    fn encode_construct(
        &mut self,
        type_id: &str,
        fields: &[(usize, usize)],
        slots: &HashMap<usize, Self::Term>,
        _ctx: IrSlotContext<'_>,
    ) -> Self::Term {
        if self.enc_ctx.type_ctx.has_struct_layout(type_id)
            && let Some(field_names) = self.enc_ctx.type_ctx.field_names_for(type_id)
        {
            self.encoder.ensure_struct_adt(
                type_id,
                &field_names
                    .into_iter()
                    .map(str::to_string)
                    .collect::<Vec<_>>(),
            );
            let mut ordered = fields.to_vec();
            ordered.sort_by_key(|(idx, _)| *idx);
            let arg_ints: Vec<ast::Int> = ordered
                .iter()
                .map(|(_, s)| self.load_slot(slots, *s))
                .collect();
            return self.encoder.adt_constructor(type_id, type_id, &arg_ints);
        }
        let arg_ints: Vec<ast::Int> = fields
            .iter()
            .map(|(_, s)| self.load_slot(slots, *s))
            .collect();
        self.nary_uf(&format!("__ir_construct_{type_id}"), &arg_ints)
    }
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
    let slot_types = slot_type_map(func);
    let mut builder = Z3IrBuilder {
        encoder,
        slot_to_name: &slot_to_name,
        slot_types: &slot_types,
        enc_ctx,
    };

    let ctx = IrSlotContext {
        slot_to_name: &slot_to_name,
        slot_types: &slot_types,
    };
    for instr in &func.body {
        if instr.target != RESULT_SLOT && !slots.contains_key(&instr.target) {
            let name = format!("__ir_slot_{}", instr.target);
            let v = builder.get_or_create_named(&name);
            slots.insert(instr.target, v);
        }
        let computed = encode_ir_expr(&mut builder, &instr.expr, &slots, ctx);
        if let Some(target) = slots.get(&instr.target) {
            builder.push_eq_axiom(computed, target.clone());
        }
        if instr.target == RESULT_SLOT
            && let IrExprKind::Load(src) = &instr.expr
            && let Some(param) = slot_to_name.get(src)
        {
            let len_result = builder.encoder.canonical_length("result");
            let len_param = builder.encoder.canonical_length(param);
            builder
                .encoder
                .background_axioms
                .push(len_result.eq(&len_param));
        }
        if instr.target == RESULT_SLOT
            && let IrExprKind::Construct { type_id, .. } = &instr.expr
        {
            let tag = pattern_hash_name(type_id);
            let tag_val = builder
                .encoder
                .get_or_create_int(&format!("__ir_tag_{type_id}"));
            builder
                .encoder
                .background_axioms
                .push(tag_val.eq(ast::Int::from_i64(tag)));
        }
    }

    if let Some(post) = &func.post
        && let Some(pred) = encode_ir_pred_z3(builder.encoder, post, &slots)
    {
        builder.encoder.background_axioms.push(pred);
    }
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
    use assura_ast::{BinOp, ClauseKind, Expr, Literal, Spanned};
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
                body: Spanned::no_span(Expr::BinOp {
                    lhs: Box::new(Spanned::no_span(Expr::MethodCall {
                        receiver: Box::new(Spanned::no_span(Expr::Ident("raw".into()))),
                        method: "length".into(),
                        args: vec![],
                    })),
                    op: BinOp::Lte,
                    rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("100".into())))),
                }),
                effect_variables: vec![],
            }];
            let ensures = vec![Clause {
                kind: ClauseKind::Ensures,
                body: Spanned::no_span(Expr::BinOp {
                    lhs: Box::new(Spanned::no_span(Expr::MethodCall {
                        receiver: Box::new(Spanned::no_span(Expr::Ident("result".into()))),
                        method: "length".into(),
                        args: vec![],
                    })),
                    op: BinOp::Lte,
                    rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("100".into())))),
                }),
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
