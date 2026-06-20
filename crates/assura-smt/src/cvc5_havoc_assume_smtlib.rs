//! Havoc+assume SMT-LIB2 encoding for the CVC5 shell-out path (#267).

use std::collections::HashSet;

use assura_parser::ast::Clause;

use assura_types::TypeEnv;

use crate::cvc5_common::canonical_length_smtlib_name;
use crate::cvc5_ir_smtlib::append_ir_body_constraints_smtlib;
use crate::havoc_assume::{infer_length_identity_links, is_collection_return};
use crate::ir::IrFunction;

/// Declare canonical length vars and append havoc+assume background axioms.
#[expect(
    clippy::too_many_arguments,
    reason = "mirrors apply_havoc_assume_cvc5 arity"
)]
pub(crate) fn append_havoc_assume_smtlib(
    script: &mut String,
    vars: &mut HashSet<String>,
    requires: &[&Clause],
    ensures: &[&Clause],
    return_ty: &[String],
    param_names: &[String],
    ir: Option<&IrFunction>,
    ir_blocks: Option<&std::collections::HashMap<usize, Vec<crate::ir::IrInstr>>>,
    ir_bodies: Option<&std::collections::HashMap<String, IrFunction>>,
    type_env: Option<&TypeEnv>,
) {
    if is_collection_return(return_ty) {
        declare_canonical_len(script, vars, "result");
        let name = canonical_length_smtlib_name("result");
        script.push_str(&format!("(assert (>= {name} 0))\n"));
    }

    for (result, input) in infer_length_identity_links(requires, ensures) {
        declare_canonical_len(script, vars, &result);
        declare_canonical_len(script, vars, &input);
        let len_result = canonical_length_smtlib_name(&result);
        let len_input = canonical_length_smtlib_name(&input);
        script.push_str(&format!("(assert (>= {len_result} 0))\n"));
        script.push_str(&format!("(assert (>= {len_input} 0))\n"));
        script.push_str(&format!("(assert (<= {len_result} {len_input}))\n"));
    }

    if let Some(func) = ir {
        append_ir_body_constraints_smtlib(
            script,
            vars,
            func,
            param_names,
            crate::ir_encode::IrEncodeContext::new(type_env, ir_bodies, ir_blocks),
        );
    }
}

fn declare_canonical_len(script: &mut String, vars: &mut HashSet<String>, name: &str) {
    let key = canonical_length_smtlib_name(name);
    if vars.insert(key.clone()) {
        script.push_str(&format!("(declare-const {key} Int)\n"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use assura_parser::ast::{BinOp, ClauseKind, Expr, Literal};

    fn len_le(obj: &str, bound: Expr) -> Expr {
        Expr::BinOp {
            lhs: Box::new(Expr::MethodCall {
                receiver: Box::new(Expr::Ident(obj.into())),
                method: "length".into(),
                args: vec![],
            }),
            op: BinOp::Lte,
            rhs: Box::new(bound),
        }
    }

    #[test]
    fn havoc_assume_smtlib_collection_return_emits_nonneg() {
        let mut script = String::new();
        let mut vars = HashSet::new();
        append_havoc_assume_smtlib(
            &mut script,
            &mut vars,
            &[],
            &[],
            &["Bytes".into()],
            &[],
            None,
            None,
            None,
            None,
        );
        assert!(script.contains("(declare-const __canonical_len_result Int)"));
        assert!(script.contains("(assert (>= __canonical_len_result 0))"));
    }

    #[test]
    fn havoc_assume_smtlib_cross_clause_length_link() {
        let n = Expr::Literal(Literal::Int("100".into()));
        let requires = vec![Clause {
            kind: ClauseKind::Requires,
            body: len_le("raw", n.clone()),
            effect_variables: vec![],
        }];
        let ensures = vec![Clause {
            kind: ClauseKind::Ensures,
            body: len_le("result", n),
            effect_variables: vec![],
        }];
        let mut script = String::new();
        let mut vars = HashSet::new();
        append_havoc_assume_smtlib(
            &mut script,
            &mut vars,
            &requires.iter().collect::<Vec<_>>(),
            &ensures.iter().collect::<Vec<_>>(),
            &["Bytes".into()],
            &["raw".into()],
            None,
            None,
            None,
            None,
        );
        assert!(script.contains("(assert (<= __canonical_len_result __canonical_len_raw))"));
    }
}
