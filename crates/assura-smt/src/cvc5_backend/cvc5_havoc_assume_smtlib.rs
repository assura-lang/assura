//! Havoc+assume SMT-LIB2 encoding for the CVC5 shell-out path (#267).

use crate::cvc5_common::canonical_length_smtlib_name;
use crate::cvc5_ir_smtlib::append_ir_body_constraints_smtlib;
use crate::havoc_assume::{
    HavocAssumeEffects, HavocAssumeInput, HavocAssumeSmtlibTarget, apply_havoc_assume_policy,
};
use crate::ir::IrFunction;
use crate::ir_encode::IrEncodeContext;

/// Declare canonical length vars and append havoc+assume background axioms.
pub(crate) fn append_havoc_assume_smtlib(
    target: &mut HavocAssumeSmtlibTarget<'_>,
    input: &HavocAssumeInput<'_>,
) {
    struct SmtlibHavocEffects<'t, 's> {
        target: &'t mut HavocAssumeSmtlibTarget<'s>,
    }

    impl HavocAssumeEffects for SmtlibHavocEffects<'_, '_> {
        fn collection_result_nonneg(&mut self) {
            declare_canonical_len(self.target, "result");
            let name = canonical_length_smtlib_name("result");
            self.target
                .script
                .push_str(&format!("(assert (>= {name} 0))\n"));
        }

        fn length_identity_le(&mut self, result_name: &str, input_name: &str) {
            declare_canonical_len(self.target, result_name);
            declare_canonical_len(self.target, input_name);
            let len_result = canonical_length_smtlib_name(result_name);
            let len_input = canonical_length_smtlib_name(input_name);
            self.target
                .script
                .push_str(&format!("(assert (>= {len_result} 0))\n"));
            self.target
                .script
                .push_str(&format!("(assert (>= {len_input} 0))\n"));
            self.target
                .script
                .push_str(&format!("(assert (<= {len_result} {len_input}))\n"));
        }

        fn apply_ir_body(
            &mut self,
            func: &IrFunction,
            param_names: &[String],
            enc_ctx: IrEncodeContext<'_>,
        ) {
            append_ir_body_constraints_smtlib(
                self.target.script,
                self.target.vars,
                func,
                param_names,
                enc_ctx,
            );
        }
    }

    let mut effects = SmtlibHavocEffects { target };
    apply_havoc_assume_policy(input, &mut effects);
}

fn declare_canonical_len(target: &mut HavocAssumeSmtlibTarget<'_>, name: &str) {
    let key = canonical_length_smtlib_name(name);
    if target.vars.insert(key.clone()) {
        target
            .script
            .push_str(&format!("(declare-const {key} Int)\n"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use assura_ast::{BinOp, ClauseKind, Expr, Literal, SpExpr, Spanned};
    use std::collections::HashSet;

    fn len_le(obj: &str, bound: SpExpr) -> SpExpr {
        Spanned::no_span(Expr::BinOp {
            lhs: Box::new(Spanned::no_span(Expr::MethodCall {
                receiver: Box::new(Spanned::no_span(Expr::Ident(obj.into()))),
                method: "length".into(),
                args: vec![],
            })),
            op: BinOp::Lte,
            rhs: Box::new(bound),
        })
    }

    #[test]
    fn havoc_assume_smtlib_collection_return_emits_nonneg() {
        let mut script = String::new();
        let mut vars = HashSet::new();
        let mut target = HavocAssumeSmtlibTarget {
            script: &mut script,
            vars: &mut vars,
        };
        let input = HavocAssumeInput {
            requires: &[],
            ensures: &[],
            return_ty: &["Bytes".into()],
            param_names: &[],
            ir: None,
            enc_ctx: crate::ir_encode::IrEncodeContext::default(),
        };
        append_havoc_assume_smtlib(&mut target, &input);
        assert!(script.contains("(declare-const __canonical_len_result Int)"));
        assert!(script.contains("(assert (>= __canonical_len_result 0))"));
    }

    #[test]
    fn havoc_assume_smtlib_cross_clause_length_link() {
        let n = Spanned::no_span(Expr::Literal(Literal::Int("100".into())));
        let requires = vec![assura_ast::Clause {
            kind: ClauseKind::Requires,
            body: len_le("raw", n.clone()),
            effect_variables: vec![],
        }];
        let ensures = vec![assura_ast::Clause {
            kind: ClauseKind::Ensures,
            body: len_le("result", n),
            effect_variables: vec![],
        }];
        let mut script = String::new();
        let mut vars = HashSet::new();
        let mut target = HavocAssumeSmtlibTarget {
            script: &mut script,
            vars: &mut vars,
        };
        let req_refs: Vec<_> = requires.iter().collect();
        let ens_refs: Vec<_> = ensures.iter().collect();
        let input = HavocAssumeInput {
            requires: &req_refs,
            ensures: &ens_refs,
            return_ty: &["Bytes".into()],
            param_names: &["raw".into()],
            ir: None,
            enc_ctx: crate::ir_encode::IrEncodeContext::default(),
        };
        append_havoc_assume_smtlib(&mut target, &input);
        assert!(script.contains("(assert (<= __canonical_len_result __canonical_len_raw))"));
    }
}
