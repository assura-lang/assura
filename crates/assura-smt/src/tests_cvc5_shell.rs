//! Shell-out CVC5 verification tests (no native feature required).

use crate::cvc5_backend::expr_to_smtlib;
use crate::cvc5_havoc_assume_smtlib::{append_havoc_assume_smtlib, canonical_length_smtlib_name};
use crate::cvc5_verify_shared::{Cvc5TypeConstraint, collect_cvc5_type_constraints};
use assura_parser::ast::{BinOp, Clause, ClauseKind, Expr, Literal};
use std::collections::HashSet;

#[test]
fn shell_method_call_length_uses_canonical_len() {
    let expr = Expr::MethodCall {
        receiver: Box::new(Expr::Ident("raw".into())),
        method: "length".into(),
        args: vec![],
    };
    assert_eq!(
        expr_to_smtlib(&expr),
        Some(canonical_length_smtlib_name("raw"))
    );
}

#[test]
fn shell_ident_field_length_uses_canonical_len() {
    let expr = Expr::Field(Box::new(Expr::Ident("raw".into())), "length".into());
    assert_eq!(
        expr_to_smtlib(&expr),
        Some(canonical_length_smtlib_name("raw"))
    );
}

#[test]
fn shell_shared_type_constraints_match_shell_script_rules() {
    let mut vars = HashSet::new();
    vars.insert("__result".into());
    vars.insert("max_size".into());
    let constraints =
        collect_cvc5_type_constraints(&vars, &[], &["Nat".into()], &[("max_size".into(), 42)], &[]);
    assert!(constraints.contains(&Cvc5TypeConstraint::NatNonNegative("__result".into())));
    assert!(constraints.contains(&Cvc5TypeConstraint::ConstantEq("max_size".into(), 42)));
}

#[test]
fn shell_havoc_assume_script_emits_length_link_axiom() {
    let n = Expr::Literal(Literal::Int("50".into()));
    let requires = vec![Clause {
        kind: ClauseKind::Requires,
        body: Expr::BinOp {
            lhs: Box::new(Expr::MethodCall {
                receiver: Box::new(Expr::Ident("buf".into())),
                method: "length".into(),
                args: vec![],
            }),
            op: BinOp::Lte,
            rhs: Box::new(n.clone()),
        },
        effect_variables: vec![],
    }];
    let ensures = vec![Clause {
        kind: ClauseKind::Ensures,
        body: Expr::BinOp {
            lhs: Box::new(Expr::MethodCall {
                receiver: Box::new(Expr::Ident("result".into())),
                method: "length".into(),
                args: vec![],
            }),
            op: BinOp::Lte,
            rhs: Box::new(n),
        },
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
        &["buf".into()],
        None,
    );
    assert!(script.contains("(assert (<= __canonical_len_result __canonical_len_buf))"));
}

#[test]
fn shell_havoc_assume_with_ir_emits_load_length_identity() {
    use crate::ir::parse_ir_module;

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
    append_havoc_assume_smtlib(
        &mut script,
        &mut vars,
        &[],
        &[],
        &["Bytes".into()],
        &["raw".into()],
        Some(&func),
    );
    assert!(
        script.contains("(assert (= __canonical_len_result __canonical_len_raw))"),
        "IR load should tie result length to input, got:\n{script}"
    );
}
