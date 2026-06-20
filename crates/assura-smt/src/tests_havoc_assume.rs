use super::*;

// -----------------------------------------------------------------------
// #267: Havoc+assume encoding for result-field verification
// -----------------------------------------------------------------------

#[test]
fn test_z3_result_field_verification() {
    let src = r#"
fn sanitize(raw: Bytes) -> Bytes
  requires raw.length() > 0
  ensures result.length() <= raw.length()
"#;
    let results = verify_source(src);
    let ensures = results.iter().find(|r| match r {
        VerificationResult::Verified { clause_desc, .. }
        | VerificationResult::Counterexample { clause_desc, .. } => clause_desc.contains("ensures"),
        _ => false,
    });
    assert!(
        ensures.is_some(),
        "expected ensures verification result, got: {results:?}"
    );
    assert!(
        matches!(ensures.unwrap(), VerificationResult::Verified { .. }),
        "result.length() <= raw.length() should verify via havoc+assume, got: {:?}",
        ensures.unwrap()
    );
}

#[test]
fn test_result_length_verifies() {
    let fixture = format!(
        "{}/../../tests/fixtures/test_sec.assura",
        env!("CARGO_MANIFEST_DIR")
    );
    let src = std::fs::read_to_string(&fixture).expect("test_sec.assura fixture");
    let file = assura_parser::parse_unwrap(&src);
    let resolved = assura_resolve::resolve(&file).expect("resolve");
    let typed = assura_types::type_check(&resolved).expect("type_check");
    let results = verify(&typed);
    let sanitize_ensures = results.iter().find(|r| match r {
        VerificationResult::Verified { clause_desc, .. }
        | VerificationResult::Counterexample { clause_desc, .. } => {
            clause_desc.contains("sanitize") && clause_desc.contains("ensures")
        }
        _ => false,
    });
    assert!(
        sanitize_ensures.is_some(),
        "expected sanitize ensures result, got: {results:?}"
    );
    assert!(
        matches!(
            sanitize_ensures.unwrap(),
            VerificationResult::Verified { .. }
        ),
        "sanitize ensures should verify (not spurious counterexample), got: {:?}",
        sanitize_ensures.unwrap()
    );
}

#[test]
fn test_z3_ir_body_constrains_result() {
    use crate::ir::{IrFunction, parse_ir_module};
    use crate::z3_backend::verify_contract_impl_with_types_and_ir;
    use assura_parser::ast::{BinOp, Clause, ClauseKind, Expr, Literal, Param};

    let ir_source = r#"
module copy {
  fn #0 : ($0: Bytes) -> Bytes ! pure
  {
    $result = load $0 : Bytes
  }
}
"#;
    let ir: IrFunction = parse_ir_module(ir_source).unwrap().functions[0].clone();

    let raw_len_gt_zero = Expr::BinOp {
        lhs: Box::new(Expr::MethodCall {
            receiver: Box::new(Expr::Ident("raw".into())),
            method: "length".into(),
            args: vec![],
        }),
        op: BinOp::Gt,
        rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
    };
    let result_len_le_raw = Expr::BinOp {
        lhs: Box::new(Expr::MethodCall {
            receiver: Box::new(Expr::Ident("result".into())),
            method: "length".into(),
            args: vec![],
        }),
        op: BinOp::Lte,
        rhs: Box::new(Expr::MethodCall {
            receiver: Box::new(Expr::Ident("raw".into())),
            method: "length".into(),
            args: vec![],
        }),
    };

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
        ty: vec!["Bytes".into()],
        parsed_type: None,
    }];

    let ctx = crate::verify_context::ContractVerifyContext {
        contract_name: "CopyBytes",
        clauses: &clauses,
        params: &params,
        return_ty: &["Bytes".into()],
        constants: &[],
        ir_body: Some(&ir),
        ir_blocks: None,
        ir_bodies: None,
        type_env: None,
    };
    let results = verify_contract_impl_with_types_and_ir(&ctx);
    assert!(!results.is_empty(), "expected verification results");
    assert!(
        matches!(&results[0], VerificationResult::Verified { .. }),
        "IR load $0 should constrain result.length() <= raw.length(), got: {:?}",
        results[0]
    );
}

fn verify_source(source: &str) -> Vec<VerificationResult> {
    let file = assura_parser::parse_unwrap(source);
    let resolved = assura_resolve::resolve(&file).expect("resolve failed in test");
    let typed = assura_types::type_check(&resolved).expect("type_check failed in test");
    verify(&typed)
}
