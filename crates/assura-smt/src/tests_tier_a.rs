//! Tier A verification depth: IR body vertical, quantifier/triggers, frame/old.
//!
//! These tests lock in the product-leverage improvements on top of the
//! existing havoc_assume / frame / trigger infrastructure.

use super::*;

// -----------------------------------------------------------------------
// A1: IR implementation body constrains `result` in ensures
// -----------------------------------------------------------------------

#[cfg(feature = "z3-verify")]
#[test]
fn tier_a1_ir_arith_body_verifies_result_eq_param_plus_one() {
    use crate::ir::{IrFunction, parse_ir_module};
    use crate::z3_backend::verify_contract_impl_with_types_and_ir;
    use assura_ast::{BinOp, Clause, ClauseKind, Expr, Literal, Param, Spanned};

    // IR: $result = $0 + 1  (identity-plus-one on first param `x`)
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
    let ir: IrFunction = parse_ir_module(ir_source).unwrap().functions[0].clone();

    // ensures { result == x + 1 }
    let result_eq_x_plus_one = Expr::BinOp {
        lhs: Box::new(Spanned::no_span(Expr::Ident("result".into()))),
        op: BinOp::Eq,
        rhs: Box::new(Spanned::no_span(Expr::BinOp {
            lhs: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
            op: BinOp::Add,
            rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("1".into())))),
        })),
    };

    let clauses = vec![Clause {
        kind: ClauseKind::Ensures,
        body: Spanned::no_span(result_eq_x_plus_one),
        effect_variables: vec![],
    }];
    let params = vec![Param {
        name: "x".into(),
        ty: Some(assura_ast::TypeExpr::Named("Int".into())),
    }];

    let ctx = crate::verify_context::ContractVerifyContext {
        contract_name: "IncOne",
        clauses: &clauses,
        params: &params,
        return_ty: &["Int".into()],
        constants: &[],
        ir: Some(crate::verify_context::LoadedIrContext::with_body(&ir)),
    };
    let results = verify_contract_impl_with_types_and_ir(&ctx);
    assert!(
        !results.is_empty(),
        "expected verification results for IR arithmetic body"
    );
    assert!(
        matches!(&results[0], VerificationResult::Verified { .. }),
        "IR $result = x + 1 should make ensures result == x + 1 verify, got: {:?}",
        results[0]
    );
}

#[cfg(feature = "z3-verify")]
#[test]
fn tier_a1_ir_identity_body_counterexample_on_wrong_ensures() {
    use crate::ir::{IrFunction, parse_ir_module};
    use crate::z3_backend::verify_contract_impl_with_types_and_ir;
    use assura_ast::{BinOp, Clause, ClauseKind, Expr, Literal, Param, Spanned};

    // IR copies x to result, but ensures claims result == x + 1 (false).
    let ir_source = r#"
module id {
  fn #0 : ($0: Int) -> Int ! pure
  {
    $result = load $0 : Int
  }
}
"#;
    let ir: IrFunction = parse_ir_module(ir_source).unwrap().functions[0].clone();

    let result_eq_x_plus_one = Expr::BinOp {
        lhs: Box::new(Spanned::no_span(Expr::Ident("result".into()))),
        op: BinOp::Eq,
        rhs: Box::new(Spanned::no_span(Expr::BinOp {
            lhs: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
            op: BinOp::Add,
            rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("1".into())))),
        })),
    };

    let clauses = vec![Clause {
        kind: ClauseKind::Ensures,
        body: Spanned::no_span(result_eq_x_plus_one),
        effect_variables: vec![],
    }];
    let params = vec![Param {
        name: "x".into(),
        ty: Some(assura_ast::TypeExpr::Named("Int".into())),
    }];

    let ctx = crate::verify_context::ContractVerifyContext {
        contract_name: "IdWrongEnsures",
        clauses: &clauses,
        params: &params,
        return_ty: &["Int".into()],
        constants: &[],
        ir: Some(crate::verify_context::LoadedIrContext::with_body(&ir)),
    };
    let results = verify_contract_impl_with_types_and_ir(&ctx);
    assert!(
        matches!(&results[0], VerificationResult::Counterexample { .. }),
        "identity IR should not satisfy result == x + 1, got: {:?}",
        results[0]
    );
}

// -----------------------------------------------------------------------
// A2: Quantifier trigger depth (AST inference + manager registration)
// -----------------------------------------------------------------------

#[test]
fn tier_a2_trigger_manager_infers_method_call_pattern() {
    use assura_ast::{Expr, Spanned};
    let mut tm = TriggerManager::new();
    tm.register_function("length".into());
    let body = Spanned::no_span(Expr::BinOp {
        lhs: Box::new(Spanned::no_span(Expr::MethodCall {
            receiver: Box::new(Spanned::no_span(Expr::Ident("xs".into()))),
            method: "length".into(),
            args: vec![],
        })),
        op: assura_ast::BinOp::Gte,
        rhs: Box::new(Spanned::no_span(Expr::Literal(assura_ast::Literal::Int(
            "0".into(),
        )))),
    });
    // Bound var unused in method receiver case — empty string allows any mention
    let t = tm
        .infer_trigger_from_expr(&body, "xs")
        .expect("method length on xs should yield a trigger");
    assert!(
        t.terms.iter().any(|term| term.contains("length")),
        "expected length trigger term, got {:?}",
        t.terms
    );
}

#[test]
fn tier_a2_trigger_validate_records_warnings() {
    let mut tm = TriggerManager::new();
    tm.register_function("known_fn".into());
    let pat = TriggerPattern {
        terms: vec!["ghost_fn(x)".into()],
        is_user_provided: true,
    };
    let warnings = tm.validate_trigger(&pat);
    assert_eq!(warnings.len(), 1);
    let taken = tm.take_last_warnings();
    assert_eq!(taken.len(), 1, "first take drains stored warnings");
    let taken2 = tm.take_last_warnings();
    assert!(taken2.is_empty(), "second take should be empty");
    // Re-validate and take again
    let _ = tm.validate_trigger(&pat);
    let taken3 = tm.take_last_warnings();
    assert_eq!(taken3.len(), 1);
}

// -----------------------------------------------------------------------
// A3: Frame/old/modifies with param candidates
// -----------------------------------------------------------------------

#[cfg(feature = "z3-verify")]
#[test]
fn tier_a3_frame_preserves_unmodified_param_not_in_ensures_text() {
    // modifies { x }, ensures only constrains x. Param y is not mentioned in
    // ensures but is a candidate; frame axiom y == old(y) is injected.
    // We verify an explicit second ensures on y via a contract that only
    // mentions y in ensures (existing test covers that). Here we prove that
    // candidate-based framing includes y when ensures only mentions x.
    use assura_ast::{BinOp, Expr, Literal, Spanned};
    use assura_types::FrameChecker;

    let modifies_body = Spanned::no_span(Expr::Ident("x".into()));
    let checker = FrameChecker::new(&[&modifies_body]);
    let ensures_x_gt_zero = Spanned::no_span(Expr::BinOp {
        lhs: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
        op: BinOp::Gt,
        rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("0".into())))),
    });
    let params = vec!["x".into(), "y".into()];
    let frame_vars = checker.frame_axiom_vars_with_candidates(&ensures_x_gt_zero, &params);
    assert!(
        frame_vars.contains(&"y".to_string()),
        "param y should receive frame axiom even when only x appears in ensures, got {frame_vars:?}"
    );
    assert!(
        !frame_vars.contains(&"x".to_string()),
        "modified x must not receive frame axiom"
    );

    // End-to-end: modifies { x }, ensures { y == old(y) } with param y still verifies.
    let src = r#"
        contract FrameCandidateParam {
            input { x: Int, y: Int }
            modifies { x }
            ensures { y == old(y) }
        }
    "#;
    let typed = assura_test_support::typecheck_ok(src);
    let results = verify(&typed);
    assert!(
        results
            .iter()
            .any(|r| matches!(r, VerificationResult::Verified { .. })),
        "y == old(y) with modifies {{ x }} should verify, got: {results:?}"
    );
}

/// End-to-end: load real `.ir` sidecar from disk and verify via `Verifier::source`.
#[cfg(feature = "z3-verify")]
#[test]
fn tier_a_ir_sidecar_from_disk_via_verifier_source() {
    use std::io::Write;

    let dir = std::env::temp_dir().join(format!("assura-tier-a-ir-sidecar-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    let assura_path = dir.join("CopyBytes.assura");
    let mut f = std::fs::File::create(&assura_path).unwrap();
    writeln!(
        f,
        r#"fn CopyBytes(raw: Bytes) -> Bytes
  requires {{ raw.length() > 0 }}
  ensures  {{ result.length() <= raw.length() }}
  effects  {{ pure }}
"#
    )
    .unwrap();

    std::fs::write(
        dir.join("CopyBytes.ir"),
        r#"
module copy {
  fn #0 : ($0: Bytes) -> Bytes ! pure
  {
    $result = load $0 : Bytes
  }
}
"#,
    )
    .unwrap();

    let src = std::fs::read_to_string(&assura_path).unwrap();
    let typed = assura_test_support::typecheck_ok(&src);
    let loaded = crate::ir_loader::LoadedVerifyExtras::load(&assura_path, &typed);
    assert!(
        !loaded.is_empty(),
        "expected CopyBytes.ir sidecar next to source"
    );
    assert!(loaded.loaded_names().contains(&"CopyBytes".to_string()));

    let results = Verifier::new(&typed).source(&assura_path).verify();
    let ensures = results.iter().find(|r| match r {
        VerificationResult::Verified { clause_desc, .. }
        | VerificationResult::Counterexample { clause_desc, .. } => {
            clause_desc.contains("ensures") || clause_desc.contains("Ensures")
        }
        _ => false,
    });
    assert!(
        ensures.is_some(),
        "expected ensures result with IR sidecar, got: {results:?}"
    );
    assert!(
        matches!(ensures.unwrap(), VerificationResult::Verified { .. }),
        "CopyBytes.ir identity should verify result.length() <= raw.length(), got: {:?}",
        ensures.unwrap()
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[cfg(feature = "z3-verify")]
#[test]
fn tier_a3_old_ident_in_ensures_with_modifies_verifies_tautology() {
    let src = r#"
        contract OldFrameTierA {
            input { a: Int, b: Int }
            modifies { a }
            ensures { old(b) >= 0 || old(b) < 0 }
        }
    "#;
    let typed = assura_test_support::typecheck_ok(src);
    let results = verify(&typed);
    let ensures = results.iter().find(|r| match r {
        VerificationResult::Verified { clause_desc, .. }
        | VerificationResult::Counterexample { clause_desc, .. } => clause_desc.contains("ensures"),
        _ => false,
    });
    assert!(
        ensures.is_some(),
        "expected ensures result, got {results:?}"
    );
    assert!(
        matches!(ensures.unwrap(), VerificationResult::Verified { .. }),
        "old(b) tautology under modifies {{ a }} should verify, got: {:?}",
        ensures.unwrap()
    );
}

/// Registered functions pass `validate_trigger` (no false A-style warnings).
#[test]
fn tier_a2_trigger_manager_registers_known_fn_and_validates() {
    use crate::advanced::{TriggerManager, TriggerPattern};

    let mut tm = TriggerManager::new();
    tm.register_function("len".into());
    let pat = TriggerPattern {
        terms: vec!["len(xs)".into()],
        is_user_provided: true,
    };
    let warnings = tm.validate_trigger(&pat);
    assert!(
        warnings.is_empty(),
        "registered function should pass validate_trigger, got: {warnings:?}"
    );
}

/// Unmodified parameters are framed when modifies is declared (Z3/CVC5 inject
/// `x == x__old` for candidates not in the modifies set).
#[test]
fn tier_a3_frame_checker_frames_unmodified_candidates() {
    use assura_ast::{Expr, Spanned};
    use assura_types::FrameChecker;

    let modifies_a = Spanned::no_span(Expr::Ident("a".into()));
    let checker = FrameChecker::new(&[&modifies_a]);
    let ensures_body = Spanned::no_span(Expr::Ident("ok".into()));
    let vars = checker
        .frame_axiom_vars_with_candidates(&ensures_body, &["a".into(), "b".into(), "ok".into()]);
    assert!(
        vars.iter().any(|v| v == "b"),
        "unmodified candidate `b` should get a frame axiom, got: {vars:?}"
    );
    assert!(
        !vars.iter().any(|v| v == "a"),
        "modified `a` should not be framed, got: {vars:?}"
    );
}
