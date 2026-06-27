use super::*;

// -----------------------------------------------------------------------
// T039: Refinement type subtyping as SMT queries
// -----------------------------------------------------------------------

#[test]
fn test_refinement_subtype_holds() {
    // x > 0 implies x >= 0 -> Verified
    let ante = binop(ident("x"), BinOp::Gt, int_lit(0));
    let cons = binop(ident("x"), BinOp::Gte, int_lit(0));

    let result = super::check_refinement_subtype(&ante, &cons);
    assert!(
        matches!(result, VerificationResult::Verified { .. }),
        "x > 0 should imply x >= 0, got: {result:?}"
    );
}

#[test]
fn test_refinement_subtype_fails() {
    // x > 0 does NOT imply x > 10 -> Counterexample
    let ante = binop(ident("x"), BinOp::Gt, int_lit(0));
    let cons = binop(ident("x"), BinOp::Gt, int_lit(10));

    let result = super::check_refinement_subtype(&ante, &cons);
    assert!(
        matches!(result, VerificationResult::Counterexample { .. }),
        "x > 0 should NOT imply x > 10, got: {result:?}"
    );
}

#[test]
fn test_refinement_with_context() {
    // Context: n > 5, n <= 10. Antecedent: x < n. Consequent: x < 10.
    // With n bounded above by 10, x < n implies x < 10. -> Verified
    let ctx = vec![
        binop(ident("n"), BinOp::Gt, int_lit(5)),
        binop(ident("n"), BinOp::Lte, int_lit(10)),
    ];
    let ante = binop(ident("x"), BinOp::Lt, ident("n"));
    let cons = binop(ident("x"), BinOp::Lt, int_lit(10));

    let result = super::check_refinement_subtype_with_context(&ctx, &ante, &cons);
    assert!(
        matches!(result, VerificationResult::Verified { .. }),
        "with n > 5 and n <= 10, x < n should imply x < 10, got: {result:?}"
    );
}

// -----------------------------------------------------------------------
// T040: Counterexample extraction
// -----------------------------------------------------------------------

#[test]
fn test_counterexample_has_model() {
    // true does NOT imply x > 0 -> counterexample with x value
    let ante = Spanned::no_span(Expr::Literal(Literal::Bool(true)));
    let cons = binop(ident("x"), BinOp::Gt, int_lit(0));

    let result = super::check_refinement_subtype(&ante, &cons);
    match &result {
        VerificationResult::Counterexample {
            counter_model: Some(cm),
            ..
        } => {
            assert!(
                !cm.variables.is_empty(),
                "counterexample model should have variables"
            );
            // The model should contain 'x' with some integer value
            let has_x = cm.variables.iter().any(|(name, _)| name == "x");
            assert!(
                has_x,
                "counterexample should contain variable 'x', got: {cm:?}"
            );
        }
        other => panic!("expected counterexample with model, got: {other:?}"),
    }
}

#[test]
fn test_counterexample_json() {
    // Build a CounterexampleModel directly and test JSON output
    let cm = super::CounterexampleModel {
        variables: vec![
            ("b".to_string(), "-1".to_string()),
            ("x".to_string(), "0".to_string()),
        ],
    };
    let json = cm.to_json();
    assert!(
        json.contains("\"variables\""),
        "JSON should have variables key"
    );
    assert!(
        json.contains("\"x\": \"0\""),
        "JSON should contain x=0, got: {json}"
    );
    assert!(
        json.contains("\"b\": \"-1\""),
        "JSON should contain b=-1, got: {json}"
    );

    // Verify it's parseable JSON by checking structural correctness
    assert!(json.starts_with('{'), "JSON should start with open brace");
    assert!(json.ends_with('}'), "JSON should end with close brace");
}
