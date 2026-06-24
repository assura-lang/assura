//! Shell-out CVC5 verification tests (no native feature required).

use crate::VerificationResult;
use crate::cache::SessionCache;
use crate::cvc5_backend::expr_to_smtlib;
use crate::cvc5_backend::verify_contract_cvc5_with_lemmas;
use crate::cvc5_havoc_assume_smtlib::append_havoc_assume_smtlib;
use crate::cvc5_verify_shared::{Cvc5TypeConstraint, collect_cvc5_type_constraints};
use crate::encode_atom_policy::canonical_length_name;
use crate::havoc_assume::{HavocAssumeInput, HavocAssumeSmtlibTarget};
use crate::verify_context::{ContractVerifyContext, LoadedIrContext};
use assura_ast::{BinOp, Clause, ClauseKind, Expr, Literal, Param, Spanned};
use std::collections::HashSet;
use std::process::Command;

fn cvc5_on_path() -> bool {
    Command::new("cvc5")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
}

#[test]
fn shell_method_call_length_uses_canonical_len() {
    let expr = Spanned::no_span(Expr::MethodCall {
        receiver: Box::new(Spanned::no_span(Expr::Ident("raw".into()))),
        method: "length".into(),
        args: vec![],
    });
    assert_eq!(expr_to_smtlib(&expr), Some(canonical_length_name("raw")));
}

#[test]
fn shell_ident_field_length_uses_canonical_len() {
    let expr = Spanned::no_span(Expr::Field(
        Box::new(Spanned::no_span(Expr::Ident("raw".into()))),
        "length".into(),
    ));
    assert_eq!(expr_to_smtlib(&expr), Some(canonical_length_name("raw")));
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
    let n = Spanned::no_span(Expr::Literal(Literal::Int("50".into())));
    let requires = vec![Clause {
        kind: ClauseKind::Requires,
        body: Spanned::no_span(Expr::BinOp {
            lhs: Box::new(Spanned::no_span(Expr::MethodCall {
                receiver: Box::new(Spanned::no_span(Expr::Ident("buf".into()))),
                method: "length".into(),
                args: vec![],
            })),
            op: BinOp::Lte,
            rhs: Box::new(n.clone()),
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
            rhs: Box::new(n),
        }),
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
        param_names: &["buf".into()],
        ir: None,
        enc_ctx: crate::ir_encode::IrEncodeContext::default(),
    };
    append_havoc_assume_smtlib(&mut target, &input);
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
    let mut target = HavocAssumeSmtlibTarget {
        script: &mut script,
        vars: &mut vars,
    };
    let input = HavocAssumeInput {
        requires: &[],
        ensures: &[],
        return_ty: &["Bytes".into()],
        param_names: &["raw".into()],
        ir: Some(&func),
        enc_ctx: crate::ir_encode::IrEncodeContext::default(),
    };
    append_havoc_assume_smtlib(&mut target, &input);
    assert!(
        script.contains("(assert (= __canonical_len_result __canonical_len_raw))"),
        "IR load should tie result length to input, got:\n{script}"
    );
}

#[test]
fn shell_ir_body_verifies_copy_length_contract() {
    if !cvc5_on_path() {
        return;
    }

    use crate::ir::{IrFunction, parse_ir_module};

    let ir_source = r#"
module copy {
  fn #0 : ($0: Bytes) -> Bytes ! pure
  {
    $result = load $0 : Bytes
  }
}
"#;
    let ir: IrFunction = parse_ir_module(ir_source).unwrap().functions[0].clone();

    let raw_len_gt_zero = Spanned::no_span(Expr::BinOp {
        lhs: Box::new(Spanned::no_span(Expr::MethodCall {
            receiver: Box::new(Spanned::no_span(Expr::Ident("raw".into()))),
            method: "length".into(),
            args: vec![],
        })),
        op: BinOp::Gt,
        rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("0".into())))),
    });
    let result_len_le_raw = Spanned::no_span(Expr::BinOp {
        lhs: Box::new(Spanned::no_span(Expr::MethodCall {
            receiver: Box::new(Spanned::no_span(Expr::Ident("result".into()))),
            method: "length".into(),
            args: vec![],
        })),
        op: BinOp::Lte,
        rhs: Box::new(Spanned::no_span(Expr::MethodCall {
            receiver: Box::new(Spanned::no_span(Expr::Ident("raw".into()))),
            method: "length".into(),
            args: vec![],
        })),
    });

    let clauses = vec![
        Clause {
            kind: ClauseKind::Requires,
            body: raw_len_gt_zero,
            effect_variables: vec![],
        },
        Clause {
            kind: ClauseKind::Ensures,
            body: result_len_le_raw,
            effect_variables: vec![],
        },
    ];
    let params = vec![Param {
        name: "raw".into(),
        ty: Some(assura_ast::TypeExpr::Named("Bytes".into())),
    }];

    let mut cache = SessionCache::new();
    let ctx = ContractVerifyContext {
        contract_name: "CopyBytes",
        clauses: &clauses,
        params: &params,
        return_ty: &["Bytes".into()],
        constants: &[],
        ir: Some(LoadedIrContext::with_body(&ir)),
    };
    let results = verify_contract_cvc5_with_lemmas(&ctx, None, &mut cache);

    assert_eq!(results.len(), 1);
    assert!(
        matches!(&results[0], VerificationResult::Verified { .. }),
        "IR load should make result.length <= raw.length() verifiable, got: {:?}",
        results[0]
    );
}
