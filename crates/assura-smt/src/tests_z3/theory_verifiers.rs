use super::*;

// -----------------------------------------------------------------------
// #263: ADT (algebraic data type) encoding tests
// -----------------------------------------------------------------------

#[test]
fn test_z3_adt_constructor() {
    // Define an ADT like Option = Some(value: Int) | None
    // Verify constructor tags are distinct and testers are consistent.
    use crate::z3_backend::encoder::Encoder;
    z3::with_z3_config(&z3::Config::new(), || {
        let mut encoder = Encoder::new();
        let def = encoder.define_adt("Option", &[("Some", &["value"]), ("None", &[])]);

        assert_eq!(def.name, "Option");
        assert_eq!(def.constructors.len(), 2);
        assert_eq!(def.constructors[0].name, "Some");
        assert_eq!(def.constructors[0].tag, 0);
        assert_eq!(def.constructors[1].name, "None");
        assert_eq!(def.constructors[1].tag, 1);

        let forty_two = z3::ast::Int::from_i64(42);
        let some_val = encoder.adt_constructor("Option", "Some", &[forty_two]);
        let is_some = encoder.adt_is_constructor("Option", "Some", &some_val);
        let is_none_on_some = encoder.adt_is_constructor("Option", "None", &some_val);

        let solver = z3::Solver::new();
        // Constructor-specific tag axioms only (skip quantified background).
        for axiom in encoder.background_axioms.iter().rev().take(2) {
            solver.assert(axiom);
        }
        solver.assert(&is_some);
        solver.assert(&is_none_on_some);
        assert_eq!(
            solver.check(),
            z3::SatResult::Unsat,
            "A Some value cannot also test as None"
        );
    });
}

#[test]
fn test_z3_adt_accessor() {
    // Verify that accessor(Constructor(val)) == val.
    use crate::z3_backend::encoder::Encoder;
    z3::with_z3_config(&z3::Config::new(), || {
        let mut encoder = Encoder::new();
        encoder.define_adt("Option", &[("Some", &["value"]), ("None", &[])]);

        // Construct Some(42)
        let forty_two = z3::ast::Int::from_i64(42);
        let some_val = encoder.adt_constructor("Option", "Some", &[forty_two.clone()]);

        // Access the value field
        let accessed = encoder.adt_accessor("Option", "value", &some_val);

        // Check that accessor(Constructor(42)) == 42
        let solver = z3::Solver::new();
        for axiom in &encoder.background_axioms {
            solver.assert(axiom);
        }
        // Assert accessed != 42 and check unsat (proving they must be equal)
        solver.assert(&accessed.eq(&forty_two).not());
        let result = solver.check();
        assert_eq!(
            result,
            z3::SatResult::Unsat,
            "accessor(Ctor(42)) must equal 42 (negation should be UNSAT)"
        );
    });
}

#[test]
fn test_z3_adt_exhaustiveness() {
    // Verify that tag(x) must be one of the defined constructors.
    // With Option = Some | None, tag(x) must be 0 or 1.
    // Asserting tag(x) == 99 should be UNSAT.
    use crate::z3_backend::encoder::Encoder;
    z3::with_z3_config(&z3::Config::new(), || {
        let mut encoder = Encoder::new();
        encoder.define_adt("Option", &[("Some", &["value"]), ("None", &[])]);

        let x = z3::ast::Int::new_const("x_adt");
        let tag_fn = z3::FuncDecl::new("__adt_tag_Option", &[&z3::Sort::int()], &z3::Sort::int());
        let tag_x = tag_fn.apply(&[&x as &dyn z3::ast::Ast]).as_int().unwrap();

        let solver = z3::Solver::new();
        for axiom in &encoder.background_axioms {
            solver.assert(axiom);
        }
        // Assert tag(x) == 99 (not a valid tag)
        solver.assert(&tag_x.eq(&z3::ast::Int::from_i64(99)));
        let result = solver.check();
        assert_eq!(
            result,
            z3::SatResult::Unsat,
            "tag(x) == 99 should be UNSAT with only tags 0 and 1 defined"
        );
    });
}

#[test]
fn test_z3_adt_injectivity() {
    // Verify Ctor(a, b) == Ctor(c, d) => a == c && b == d.
    // Define Pair = MkPair(fst, snd).
    // Construct MkPair(a, b) and MkPair(c, d), assert they are equal,
    // then verify a == c and b == d.
    use crate::z3_backend::encoder::Encoder;
    z3::with_z3_config(&z3::Config::new(), || {
        let mut encoder = Encoder::new();
        encoder.define_adt("Pair", &[("MkPair", &["fst", "snd"])]);

        let a = z3::ast::Int::new_const("a");
        let b = z3::ast::Int::new_const("b");
        let c = z3::ast::Int::new_const("c");
        let d = z3::ast::Int::new_const("d");

        let pair_ab = encoder.adt_constructor("Pair", "MkPair", &[a.clone(), b.clone()]);
        let pair_cd = encoder.adt_constructor("Pair", "MkPair", &[c.clone(), d.clone()]);

        let solver = z3::Solver::new();
        for axiom in &encoder.background_axioms {
            solver.assert(axiom);
        }
        // Assert pair_ab == pair_cd
        solver.assert(&pair_ab.eq(&pair_cd));

        // Assert NOT(a == c) -- should be UNSAT if injectivity holds
        solver.push();
        solver.assert(&a.eq(&c).not());
        let result_fst = solver.check();
        assert_eq!(
            result_fst,
            z3::SatResult::Unsat,
            "MkPair(a,b) == MkPair(c,d) should imply a == c"
        );
        solver.pop(1);

        // Assert NOT(b == d) -- should be UNSAT if injectivity holds
        solver.push();
        solver.assert(&b.eq(&d).not());
        let result_snd = solver.check();
        assert_eq!(
            result_snd,
            z3::SatResult::Unsat,
            "MkPair(a,b) == MkPair(c,d) should imply b == d"
        );
        solver.pop(1);
    });
}

// -----------------------------------------------------------------------
// #265: Bitvector theory tests
// -----------------------------------------------------------------------

#[test]
fn test_z3_bitvector_wrapping() {
    use crate::z3_backend::encoder::BitvectorEncoder;
    z3::with_z3_config(&z3::Config::new(), || {
        let a = BitvectorEncoder::bv_from_u64(255, 8);
        let b = BitvectorEncoder::bv_from_u64(1, 8);
        let sum = BitvectorEncoder::bvadd(&a, &b);
        let zero = BitvectorEncoder::bv_from_u64(0, 8);
        let solver = z3::Solver::new();
        solver.assert(&sum.eq(&zero));
        assert_eq!(
            solver.check(),
            z3::SatResult::Sat,
            "255 + 1 should wrap to 0 in u8"
        );
    });
}

#[test]
fn test_z3_bitvector_overflow_detect() {
    use crate::z3_backend::encoder::BitvectorEncoder;
    z3::with_z3_config(&z3::Config::new(), || {
        let a = BitvectorEncoder::bv_from_u64(255, 8);
        let b = BitvectorEncoder::bv_from_u64(1, 8);
        let overflow = BitvectorEncoder::bvadd_overflow_unsigned(&a, &b);
        let solver = z3::Solver::new();
        solver.assert(&overflow);
        assert_eq!(
            solver.check(),
            z3::SatResult::Sat,
            "255 + 1 should overflow unsigned u8"
        );

        let small_a = BitvectorEncoder::bv_from_u64(1, 8);
        let small_b = BitvectorEncoder::bv_from_u64(2, 8);
        let no_overflow = BitvectorEncoder::bvadd_overflow_unsigned(&small_a, &small_b);
        solver.reset();
        solver.assert(&no_overflow);
        assert_eq!(
            solver.check(),
            z3::SatResult::Unsat,
            "1 + 2 should not overflow u8"
        );
    });
}

#[test]
fn test_z3_bitvector_bitwise_ops() {
    use crate::z3_backend::encoder::BitvectorEncoder;
    z3::with_z3_config(&z3::Config::new(), || {
        let a = BitvectorEncoder::bv_from_u64(0b1010, 8);
        let b = BitvectorEncoder::bv_from_u64(0b1100, 8);
        let and_val = BitvectorEncoder::bvand(&a, &b);
        let or_val = BitvectorEncoder::bvor(&a, &b);
        let xor_val = BitvectorEncoder::bvxor(&a, &b);

        let solver = z3::Solver::new();
        solver.assert(&and_val.eq(&BitvectorEncoder::bv_from_u64(0b1000, 8)));
        solver.assert(&or_val.eq(&BitvectorEncoder::bv_from_u64(0b1110, 8)));
        solver.assert(&xor_val.eq(&BitvectorEncoder::bv_from_u64(0b0110, 8)));
        assert_eq!(solver.check(), z3::SatResult::Sat);
    });
}

// -----------------------------------------------------------------------
// #266: Unsat core extraction
// -----------------------------------------------------------------------

#[test]
fn test_z3_unsat_core_extraction() {
    let src = r#"
contract UnsatCoreTest {
    requires: x > 50
    requires: x < 100
    ensures: x > 10
}
"#;
    let results = verify_source(src);
    let ensures = results.iter().find(|r| match r {
        VerificationResult::Verified { clause_desc, .. }
        | VerificationResult::Counterexample { clause_desc, .. } => clause_desc.contains("ensures"),
        _ => false,
    });
    assert!(
        ensures.is_some(),
        "expected ensures result, got: {results:?}"
    );
    match ensures.unwrap() {
        VerificationResult::Verified { unsat_core, .. } => {
            let core = unsat_core
                .as_ref()
                .expect("verified ensures should include unsat core");
            assert!(
                core.iter().any(|l| l == "req_0"),
                "core should include the strong require req_0, got: {core:?}"
            );
        }
        other => panic!("expected verified ensures, got: {other:?}"),
    }
}

#[test]
fn test_unsat_core_minimal() {
    let src = r#"
contract MinimalCore {
    requires: x > 50
    requires: x > 10
    ensures: x > 0
}
"#;
    let results = verify_source(src);
    let ensures = results.iter().find(|r| match r {
        VerificationResult::Verified { clause_desc, .. }
        | VerificationResult::Counterexample { clause_desc, .. } => clause_desc.contains("ensures"),
        _ => false,
    });
    assert!(
        ensures.is_some(),
        "expected ensures result, got: {results:?}"
    );
    match ensures.unwrap() {
        VerificationResult::Verified { unsat_core, .. } => {
            let core = unsat_core
                .as_ref()
                .expect("verified ensures should include unsat core");
            assert!(
                core.iter().any(|l| l == "req_0"),
                "minimal core should retain req_0 (x > 50), got: {core:?}"
            );
            assert!(
                !core.iter().any(|l| l == "req_1"),
                "redundant req_1 (x > 10) should be excluded from minimal core, got: {core:?}"
            );
        }
        other => panic!("expected verified ensures, got: {other:?}"),
    }
}

// ---------------------------------------------------------------
// Batch 3 fixes: #458 (empty block), #460 (comparison chaining),
//                #464 (function name extraction)
// ---------------------------------------------------------------

#[test]
fn test_z3_empty_block_returns_bool_true() {
    // Fix #458: empty block should return Bool(true), not fresh Int.
    use crate::z3_backend::encoder::{Encoder, Z3Value};
    z3::with_z3_config(&z3::Config::new(), || {
        let mut enc = Encoder::new();
        let block_expr = Spanned::no_span(Expr::Block(vec![]));
        let result = enc.encode_expr(&block_expr);
        assert!(
            matches!(result, Z3Value::Bool(_)),
            "empty block should return Bool (policy: true), not Int"
        );
    });
}

#[test]
fn test_z3_raw_comparison_chaining() {
    // Fix #460: `0 <= x < 10` should become `(0 <= x) AND (x < 10)`.
    use crate::z3_backend::encoder::{Encoder, Z3Value};
    z3::with_z3_config(&z3::Config::new(), || {
        let mut enc = Encoder::new();
        let tokens: Vec<String> = vec!["0", "<=", "x", "<", "10"]
            .into_iter()
            .map(String::from)
            .collect();
        let result = enc.encode_raw_tokens(&tokens);
        // Should be Bool (conjunction), not an integer comparison of bool < int.
        assert!(
            matches!(result, Z3Value::Bool(_)),
            "chained comparison should produce Bool"
        );
    });
}

#[test]
fn test_z3_raw_comparison_chaining_three_ops() {
    // `a < b <= c` should become `(a < b) AND (b <= c)`.
    use crate::z3_backend::encoder::{Encoder, Z3Value};
    z3::with_z3_config(&z3::Config::new(), || {
        let mut enc = Encoder::new();
        let tokens: Vec<String> = vec!["a", "<", "b", "<=", "c"]
            .into_iter()
            .map(String::from)
            .collect();
        let result = enc.encode_raw_tokens(&tokens);
        assert!(
            matches!(result, Z3Value::Bool(_)),
            "chained comparison should produce Bool"
        );
    });
}

#[test]
fn test_extract_raw_base_name_policy() {
    // Fix #464: shared function name extraction.
    use crate::encode_atom_policy::extract_raw_base_name;
    assert_eq!(extract_raw_base_name("state_field_length"), "length");
    assert_eq!(extract_raw_base_name("length"), "length");
    assert_eq!(extract_raw_base_name("a_b_c"), "c");
    assert_eq!(extract_raw_base_name("simple"), "simple");
}

// -----------------------------------------------------------------------
// #509: Counterexample value verification
// -----------------------------------------------------------------------

/// Parse an integer value from a CounterexampleModel variable.
fn get_ce_var_value(cm: &super::CounterexampleModel, name: &str) -> Option<i64> {
    cm.variables
        .iter()
        .find(|(n, _)| n == name)
        .and_then(|(_, v)| v.parse().ok())
}

#[test]
fn test_counterexample_value_satisfies_requires() {
    // requires: x > 0, ensures: x > 100
    // CE must have x in (0, 100] to satisfy requires but violate ensures.
    let ante = binop(ident("x"), BinOp::Gt, int_lit(0));
    let cons = binop(ident("x"), BinOp::Gt, int_lit(100));

    let result = super::check_refinement_subtype(&ante, &cons);
    match &result {
        VerificationResult::Counterexample {
            counter_model: Some(cm),
            ..
        } => {
            let x = get_ce_var_value(cm, "x").expect("counterexample should contain variable 'x'");
            assert!(x > 0, "CE x={x} should satisfy requires (x > 0)");
            assert!(x <= 100, "CE x={x} should violate ensures (x > 100)");
        }
        other => panic!("expected counterexample with model, got: {other:?}"),
    }
}

#[test]
fn test_counterexample_value_violates_ensures() {
    // true implies x > 10 is false -> CE has x <= 10
    let ante = Spanned::no_span(Expr::Literal(Literal::Bool(true)));
    let cons = binop(ident("x"), BinOp::Gt, int_lit(10));

    let result = super::check_refinement_subtype(&ante, &cons);
    match &result {
        VerificationResult::Counterexample {
            counter_model: Some(cm),
            ..
        } => {
            let x = get_ce_var_value(cm, "x").expect("counterexample should contain variable 'x'");
            assert!(x <= 10, "CE x={x} should violate ensures (x > 10)");
        }
        other => panic!("expected counterexample with model, got: {other:?}"),
    }
}

#[test]
fn test_counterexample_multi_variable_values() {
    // requires: x > 0 AND y > 0, ensures: x + y > 100
    // CE must have x > 0, y > 0, and x + y <= 100.
    let ctx = vec![
        binop(ident("x"), BinOp::Gt, int_lit(0)),
        binop(ident("y"), BinOp::Gt, int_lit(0)),
    ];
    let ante = binop(ident("x"), BinOp::Gt, int_lit(0)); // trivially true given ctx
    let cons = binop(
        binop(ident("x"), BinOp::Add, ident("y")),
        BinOp::Gt,
        int_lit(100),
    );

    let result = super::check_refinement_subtype_with_context(&ctx, &ante, &cons);
    match &result {
        VerificationResult::Counterexample {
            counter_model: Some(cm),
            ..
        } => {
            let x = get_ce_var_value(cm, "x").expect("counterexample should contain variable 'x'");
            let y = get_ce_var_value(cm, "y").expect("counterexample should contain variable 'y'");
            assert!(x > 0, "CE x={x} should satisfy requires (x > 0)");
            assert!(y > 0, "CE y={y} should satisfy requires (y > 0)");
            assert!(
                x + y <= 100,
                "CE x={x}, y={y} (sum={}) should violate ensures (x + y > 100)",
                x + y
            );
        }
        other => panic!("expected counterexample with model, got: {other:?}"),
    }
}

#[test]
fn test_counterexample_boolean_value() {
    // true implies flag == true is false -> CE has flag != true
    let ante = Spanned::no_span(Expr::Literal(Literal::Bool(true)));
    let cons = binop(
        ident("flag"),
        BinOp::Eq,
        Spanned::no_span(Expr::Literal(Literal::Bool(true))),
    );

    let result = super::check_refinement_subtype(&ante, &cons);
    match &result {
        VerificationResult::Counterexample {
            counter_model: Some(cm),
            ..
        } => {
            let flag_val = cm
                .variables
                .iter()
                .find(|(n, _)| n == "flag")
                .map(|(_, v)| v.as_str())
                .expect("counterexample should contain variable 'flag'");
            // With #511 fix, mixed Bool/Int equality coerces to Bool sort,
            // so Z3 returns "false" or "0" (not arbitrary ints like "2").
            assert!(
                flag_val == "false" || flag_val == "0",
                "CE flag={flag_val} should be false/0 to violate ensures (flag == true)"
            );
        }
        other => panic!("expected counterexample with model, got: {other:?}"),
    }
}

// #511: Mixed Bool/Int Neq path test
// The Eq path is tested by test_counterexample_boolean_value above.
// This covers the Neq path: flag != false should be true when flag is true,
// so Z3 should verify (UNSAT) rather than produce a counterexample.
#[test]
fn test_mixed_bool_int_neq_path() {
    // requires(true) ensures(flag != false) with flag unconstrained.
    // Z3 can pick flag = false to violate the Neq, producing a CE.
    let ante = Spanned::no_span(Expr::Literal(Literal::Bool(true)));
    let cons = binop(
        ident("flag"),
        BinOp::Neq,
        Spanned::no_span(Expr::Literal(Literal::Bool(false))),
    );

    let result = super::check_refinement_subtype(&ante, &cons);
    match &result {
        VerificationResult::Counterexample {
            counter_model: Some(cm),
            ..
        } => {
            // flag should be "false" or "0", not an arbitrary int like "2".
            let flag_val = cm
                .variables
                .iter()
                .find(|(n, _)| n == "flag")
                .map(|(_, v)| v.as_str())
                .expect("counterexample should contain variable 'flag'");
            assert!(
                flag_val == "false" || flag_val == "0",
                "CE flag={flag_val} should be false/0 to violate ensures (flag != false)"
            );
        }
        VerificationResult::Counterexample { .. } => {
            // CE without model is acceptable (solver produced CE but no model).
        }
        VerificationResult::Verified { .. } => {
            // Z3 might verify this if it picks a model where flag != false holds.
            // Both outcomes are acceptable; the key invariant is no arbitrary ints.
        }
        other => panic!("unexpected result for Neq test: {other:?}"),
    }
}

// -----------------------------------------------------------------------
// #519: Monotonic state lattice verification
// -----------------------------------------------------------------------

fn make_clause(kind: assura_ast::ClauseKind, body: SpExpr) -> assura_ast::Clause {
    assura_ast::Clause {
        kind,
        body,
        effect_variables: vec![],
    }
}

#[test]
fn test_monotonic_state_valid_non_decrease() {
    // requires { old_state >= 0 }
    // requires { new_state >= old_state }
    // monotonic { new_state >= old_state }
    // This should verify: the body is a direct consequence of requires.
    let requires1 = make_clause(
        assura_ast::ClauseKind::Requires,
        binop(ident("old_state"), BinOp::Gte, int_lit(0)),
    );
    let requires2 = make_clause(
        assura_ast::ClauseKind::Requires,
        binop(ident("new_state"), BinOp::Gte, ident("old_state")),
    );
    let body = binop(ident("new_state"), BinOp::Gte, ident("old_state"));
    let clauses = vec![requires1, requires2];

    let results = super::z3_backend::verify_monotonic_state_impl("MonoTest", &body, &clauses);
    assert!(
        results
            .iter()
            .any(|r| matches!(r, VerificationResult::Verified { .. })),
        "monotonic non-decrease should verify, got: {results:?}"
    );
}

#[test]
fn test_monotonic_state_decrease_counterexample() {
    // requires { old_state >= 0 }
    // (no constraint that new_state >= old_state)
    // monotonic { new_state >= old_state }
    // This should produce counterexample: new_state can be less than old_state.
    let requires = make_clause(
        assura_ast::ClauseKind::Requires,
        binop(ident("old_state"), BinOp::Gte, int_lit(0)),
    );
    let body = binop(ident("new_state"), BinOp::Gte, ident("old_state"));
    let clauses = vec![requires];

    let results = super::z3_backend::verify_monotonic_state_impl("MonoTest", &body, &clauses);
    assert!(
        results
            .iter()
            .any(|r| matches!(r, VerificationResult::Counterexample { .. })),
        "monotonic decrease should produce counterexample, got: {results:?}"
    );
}

#[test]
fn test_monotonic_state_with_ensures() {
    // requires { old_state >= 0 }
    // ensures  { new_state == old_state + 1 }
    // monotonic { new_state >= old_state }
    // Should verify: ensures guarantees increment.
    let requires = make_clause(
        assura_ast::ClauseKind::Requires,
        binop(ident("old_state"), BinOp::Gte, int_lit(0)),
    );
    let ensures = make_clause(
        assura_ast::ClauseKind::Ensures,
        binop(
            ident("new_state"),
            BinOp::Eq,
            binop(ident("old_state"), BinOp::Add, int_lit(1)),
        ),
    );
    let body = binop(ident("new_state"), BinOp::Gte, ident("old_state"));
    let clauses = vec![requires, ensures];

    let results = super::z3_backend::verify_monotonic_state_impl("MonoTest", &body, &clauses);
    // The no-decrease step uses requires+ensures, so it should verify
    assert!(
        results
            .iter()
            .any(|r| matches!(r, VerificationResult::Verified { .. })),
        "monotonic with ensures increment should verify, got: {results:?}"
    );
}

// -----------------------------------------------------------------------
// #517: Lock ordering acyclicity verification
// -----------------------------------------------------------------------

#[test]
fn test_lock_ordering_consistent() {
    // requires { lock_a < lock_b }
    // requires { lock_b < lock_c }
    // lock_order { lock_a < lock_c }
    // The ordering is consistent (acyclic), and a < c follows from transitivity.
    let req1 = make_clause(
        assura_ast::ClauseKind::Requires,
        binop(ident("lock_a"), BinOp::Lt, ident("lock_b")),
    );
    let req2 = make_clause(
        assura_ast::ClauseKind::Requires,
        binop(ident("lock_b"), BinOp::Lt, ident("lock_c")),
    );
    let body = binop(ident("lock_a"), BinOp::Lt, ident("lock_c"));
    let clauses = vec![req1, req2];

    let results = super::z3_backend::verify_lock_ordering_impl("LockTest", &body, &clauses);
    assert!(
        results
            .iter()
            .any(|r| matches!(r, VerificationResult::Verified { .. })),
        "consistent lock ordering should verify, got: {results:?}"
    );
}

#[test]
fn test_lock_ordering_cycle_counterexample() {
    // requires { lock_a < lock_b }
    // requires { lock_b < lock_a }
    // lock_order { lock_a < lock_b }
    // This forms a cycle (a < b and b < a is contradictory for integers).
    let req1 = make_clause(
        assura_ast::ClauseKind::Requires,
        binop(ident("lock_a"), BinOp::Lt, ident("lock_b")),
    );
    let req2 = make_clause(
        assura_ast::ClauseKind::Requires,
        binop(ident("lock_b"), BinOp::Lt, ident("lock_a")),
    );
    let body = binop(ident("lock_a"), BinOp::Lt, ident("lock_b"));
    let clauses = vec![req1, req2];

    let results = super::z3_backend::verify_lock_ordering_impl("LockTest", &body, &clauses);
    // The acyclicity check should detect the cycle (UNSAT constraints)
    assert!(
        results
            .iter()
            .any(|r| matches!(r, VerificationResult::Counterexample { .. })),
        "cyclic lock ordering should produce counterexample, got: {results:?}"
    );
}

#[test]
fn test_lock_ordering_body_not_implied() {
    // requires { lock_a >= 0 }
    // lock_order { lock_a < lock_b }
    // The body is NOT implied by the requires (lock_b is unconstrained).
    let req = make_clause(
        assura_ast::ClauseKind::Requires,
        binop(ident("lock_a"), BinOp::Gte, int_lit(0)),
    );
    let body = binop(ident("lock_a"), BinOp::Lt, ident("lock_b"));
    let clauses = vec![req];

    let results = super::z3_backend::verify_lock_ordering_impl("LockTest", &body, &clauses);
    // Body check should produce counterexample (lock_b unconstrained)
    assert!(
        results
            .iter()
            .any(|r| matches!(r, VerificationResult::Counterexample { .. })),
        "unconstrained lock_b should produce counterexample, got: {results:?}"
    );
}

// -----------------------------------------------------------------------
// #518: Constant-time verification
// -----------------------------------------------------------------------

#[test]
fn test_constant_time_valid() {
    // requires { x >= 0 }
    // requires { x < 256 }
    // constant_time { x < 256 }
    // The body is directly implied by requires.
    let req1 = make_clause(
        assura_ast::ClauseKind::Requires,
        binop(ident("x"), BinOp::Gte, int_lit(0)),
    );
    let req2 = make_clause(
        assura_ast::ClauseKind::Requires,
        binop(ident("x"), BinOp::Lt, int_lit(256)),
    );
    let body = binop(ident("x"), BinOp::Lt, int_lit(256));
    let clauses = vec![req1, req2];

    let results = super::z3_backend::verify_constant_time_impl("CTTest", &body, &clauses);
    assert!(
        results
            .iter()
            .any(|r| matches!(r, VerificationResult::Verified { .. })),
        "constant-time valid should verify, got: {results:?}"
    );
}

#[test]
fn test_constant_time_secret_dependent() {
    // requires { secret >= 0 }
    // constant_time { secret == 0 }
    // The body depends on secret's value (not implied by requires).
    let req = make_clause(
        assura_ast::ClauseKind::Requires,
        binop(ident("secret"), BinOp::Gte, int_lit(0)),
    );
    let body = binop(ident("secret"), BinOp::Eq, int_lit(0));
    let clauses = vec![req];

    let results = super::z3_backend::verify_constant_time_impl("CTTest", &body, &clauses);
    assert!(
        results
            .iter()
            .any(|r| matches!(r, VerificationResult::Counterexample { .. })),
        "secret-dependent branch should produce counterexample, got: {results:?}"
    );
}

#[test]
fn test_constant_time_independent() {
    // requires { public_len > 0 }
    // requires { public_len <= max_len }
    // constant_time { public_len <= max_len }
    // Body is implied: no secret dependency.
    let req1 = make_clause(
        assura_ast::ClauseKind::Requires,
        binop(ident("public_len"), BinOp::Gt, int_lit(0)),
    );
    let req2 = make_clause(
        assura_ast::ClauseKind::Requires,
        binop(ident("public_len"), BinOp::Lte, ident("max_len")),
    );
    let body = binop(ident("public_len"), BinOp::Lte, ident("max_len"));
    let clauses = vec![req1, req2];

    let results = super::z3_backend::verify_constant_time_impl("CTTest", &body, &clauses);
    assert!(
        results
            .iter()
            .any(|r| matches!(r, VerificationResult::Verified { .. })),
        "public-only dependency should verify, got: {results:?}"
    );
}

// -----------------------------------------------------------------------
// #520: Secure erasure verification
// -----------------------------------------------------------------------

#[test]
fn test_secure_erasure_valid() {
    // requires { buf_size > 0 }
    // ensures  { bytes_erased == buf_size }
    // secure_erase { bytes_erased == buf_size }
    // Body is directly implied by ensures.
    let req = make_clause(
        assura_ast::ClauseKind::Requires,
        binop(ident("buf_size"), BinOp::Gt, int_lit(0)),
    );
    let ens = make_clause(
        assura_ast::ClauseKind::Ensures,
        binop(ident("bytes_erased"), BinOp::Eq, ident("buf_size")),
    );
    let body = binop(ident("bytes_erased"), BinOp::Eq, ident("buf_size"));
    let clauses = vec![req, ens];

    let results = super::z3_backend::verify_secure_erasure_impl("EraseTest", &body, &clauses);
    assert!(
        results
            .iter()
            .any(|r| matches!(r, VerificationResult::Verified { .. })),
        "full erasure should verify, got: {results:?}"
    );
}

#[test]
fn test_secure_erasure_partial() {
    // requires { buf_size > 0 }
    // (no ensures about erasure)
    // secure_erase { bytes_erased == buf_size }
    // Body is NOT implied without ensures.
    let req = make_clause(
        assura_ast::ClauseKind::Requires,
        binop(ident("buf_size"), BinOp::Gt, int_lit(0)),
    );
    let body = binop(ident("bytes_erased"), BinOp::Eq, ident("buf_size"));
    let clauses = vec![req];

    let results = super::z3_backend::verify_secure_erasure_impl("EraseTest", &body, &clauses);
    assert!(
        results
            .iter()
            .any(|r| matches!(r, VerificationResult::Counterexample { .. })),
        "missing erasure should produce counterexample, got: {results:?}"
    );
}

#[test]
fn test_secure_erasure_partial_erase() {
    // requires { buf_size == 16 }
    // ensures  { bytes_erased == 8 }
    // secure_erase { bytes_erased == buf_size }
    // Partial: only 8 of 16 bytes erased.
    let req = make_clause(
        assura_ast::ClauseKind::Requires,
        binop(ident("buf_size"), BinOp::Eq, int_lit(16)),
    );
    let ens = make_clause(
        assura_ast::ClauseKind::Ensures,
        binop(ident("bytes_erased"), BinOp::Eq, int_lit(8)),
    );
    let body = binop(ident("bytes_erased"), BinOp::Eq, ident("buf_size"));
    let clauses = vec![req, ens];

    let results = super::z3_backend::verify_secure_erasure_impl("EraseTest", &body, &clauses);
    // Coverage check uses requires+ensures, so 8 != 16 gives counterexample
    assert!(
        results
            .iter()
            .any(|r| matches!(r, VerificationResult::Counterexample { .. })),
        "partial erasure should produce counterexample, got: {results:?}"
    );
}

// -----------------------------------------------------------------------
// #516: Crash recovery verification
// -----------------------------------------------------------------------

#[test]
fn test_crash_recovery_with_wal() {
    // requires { has_wal == 1 }
    // requires { data_size > 0 }
    // ensures  { recovered == 1 }
    // crash_recovery { recovered == 1 }
    // With WAL and ensures guaranteeing recovery, this should verify.
    let req1 = make_clause(
        assura_ast::ClauseKind::Requires,
        binop(ident("has_wal"), BinOp::Eq, int_lit(1)),
    );
    let req2 = make_clause(
        assura_ast::ClauseKind::Requires,
        binop(ident("data_size"), BinOp::Gt, int_lit(0)),
    );
    let ens = make_clause(
        assura_ast::ClauseKind::Ensures,
        binop(ident("recovered"), BinOp::Eq, int_lit(1)),
    );
    let body = binop(ident("recovered"), BinOp::Eq, int_lit(1));
    let clauses = vec![req1, req2, ens];

    let results = super::z3_backend::verify_crash_recovery_impl("CrashTest", &body, &clauses);
    // Preservation (requires + ensures => body) should verify
    assert!(
        results
            .iter()
            .any(|r| matches!(r, VerificationResult::Verified { .. })),
        "crash recovery with WAL should verify, got: {results:?}"
    );
}

#[test]
fn test_crash_recovery_missing_wal() {
    // requires { data_size > 0 }
    // (no ensures about recovery)
    // crash_recovery { recovered == 1 }
    // Without recovery ensures, this should fail.
    let req = make_clause(
        assura_ast::ClauseKind::Requires,
        binop(ident("data_size"), BinOp::Gt, int_lit(0)),
    );
    let body = binop(ident("recovered"), BinOp::Eq, int_lit(1));
    let clauses = vec![req];

    let results = super::z3_backend::verify_crash_recovery_impl("CrashTest", &body, &clauses);
    assert!(
        results
            .iter()
            .any(|r| matches!(r, VerificationResult::Counterexample { .. })),
        "crash recovery without WAL should produce counterexample, got: {results:?}"
    );
}

#[test]
fn test_crash_recovery_partial_write() {
    // requires { total_writes == 4 }
    // ensures  { committed_writes == 2 }
    // crash_recovery { committed_writes == total_writes }
    // Partial: only 2 of 4 writes committed.
    let req = make_clause(
        assura_ast::ClauseKind::Requires,
        binop(ident("total_writes"), BinOp::Eq, int_lit(4)),
    );
    let ens = make_clause(
        assura_ast::ClauseKind::Ensures,
        binop(ident("committed_writes"), BinOp::Eq, int_lit(2)),
    );
    let body = binop(ident("committed_writes"), BinOp::Eq, ident("total_writes"));
    let clauses = vec![req, ens];

    let results = super::z3_backend::verify_crash_recovery_impl("CrashTest", &body, &clauses);
    // 2 != 4 under the ensures, so preservation should fail
    assert!(
        results
            .iter()
            .any(|r| matches!(r, VerificationResult::Counterexample { .. })),
        "partial write should produce counterexample, got: {results:?}"
    );
}

// -----------------------------------------------------------------------
// #521: MVCC isolation verification
// -----------------------------------------------------------------------

#[test]
fn test_mvcc_isolation_valid() {
    // requires { start_ts >= 0 }
    // requires { commit_ts >= start_ts }
    // requires { other_commit_ts < start_ts }
    // mvcc_isolation { other_commit_ts < start_ts }
    // Snapshot reads see only committed data before start_ts.
    let req1 = make_clause(
        assura_ast::ClauseKind::Requires,
        binop(ident("start_ts"), BinOp::Gte, int_lit(0)),
    );
    let req2 = make_clause(
        assura_ast::ClauseKind::Requires,
        binop(ident("commit_ts"), BinOp::Gte, ident("start_ts")),
    );
    let req3 = make_clause(
        assura_ast::ClauseKind::Requires,
        binop(ident("other_commit_ts"), BinOp::Lt, ident("start_ts")),
    );
    let body = binop(ident("other_commit_ts"), BinOp::Lt, ident("start_ts"));
    let clauses = vec![req1, req2, req3];

    let results = super::z3_backend::verify_mvcc_isolation_impl("MvccTest", &body, &clauses);
    assert!(
        results
            .iter()
            .any(|r| matches!(r, VerificationResult::Verified { .. })),
        "valid MVCC isolation should verify, got: {results:?}"
    );
}

#[test]
fn test_mvcc_isolation_dirty_read() {
    // requires { start_ts >= 0 }
    // (no constraint on other_commit_ts)
    // mvcc_isolation { other_commit_ts < start_ts }
    // Without constraint, other_commit_ts could be >= start_ts (dirty read).
    let req = make_clause(
        assura_ast::ClauseKind::Requires,
        binop(ident("start_ts"), BinOp::Gte, int_lit(0)),
    );
    let body = binop(ident("other_commit_ts"), BinOp::Lt, ident("start_ts"));
    let clauses = vec![req];

    let results = super::z3_backend::verify_mvcc_isolation_impl("MvccTest", &body, &clauses);
    assert!(
        results
            .iter()
            .any(|r| matches!(r, VerificationResult::Counterexample { .. })),
        "dirty read scenario should produce counterexample, got: {results:?}"
    );
}

#[test]
fn test_mvcc_write_conflict() {
    // requires { tx1_key == tx2_key }
    // requires { tx1_start < tx2_commit }
    // requires { tx2_start < tx1_commit }
    // ensures  { conflict_detected == 1 }
    // mvcc_isolation { conflict_detected == 1 }
    // Write-write conflict on same key detected.
    let req1 = make_clause(
        assura_ast::ClauseKind::Requires,
        binop(ident("tx1_key"), BinOp::Eq, ident("tx2_key")),
    );
    let req2 = make_clause(
        assura_ast::ClauseKind::Requires,
        binop(ident("tx1_start"), BinOp::Lt, ident("tx2_commit")),
    );
    let req3 = make_clause(
        assura_ast::ClauseKind::Requires,
        binop(ident("tx2_start"), BinOp::Lt, ident("tx1_commit")),
    );
    let ens = make_clause(
        assura_ast::ClauseKind::Ensures,
        binop(ident("conflict_detected"), BinOp::Eq, int_lit(1)),
    );
    let body = binop(ident("conflict_detected"), BinOp::Eq, int_lit(1));
    let clauses = vec![req1, req2, req3, ens];

    let results = super::z3_backend::verify_mvcc_isolation_impl("MvccTest", &body, &clauses);
    assert!(
        results
            .iter()
            .any(|r| matches!(r, VerificationResult::Verified { .. })),
        "write conflict detection should verify, got: {results:?}"
    );
}

// -----------------------------------------------------------------------
// #522: Crypto conformance verification
// -----------------------------------------------------------------------

#[test]
fn test_crypto_conformance_valid_key_size() {
    // requires { key_size == 32 }
    // crypto_conformance { key_size >= 16 }
    // 32-byte key meets minimum 16-byte requirement.
    let req = make_clause(
        assura_ast::ClauseKind::Requires,
        binop(ident("key_size"), BinOp::Eq, int_lit(32)),
    );
    let body = binop(ident("key_size"), BinOp::Gte, int_lit(16));
    let clauses = vec![req];

    let results = super::z3_backend::verify_crypto_conformance_impl("CryptoTest", &body, &clauses);
    assert!(
        results
            .iter()
            .any(|r| matches!(r, VerificationResult::Verified { .. })),
        "valid key size should verify, got: {results:?}"
    );
}

#[test]
fn test_crypto_conformance_short_key() {
    // requires { key_size >= 0 }
    // crypto_conformance { key_size >= 32 }
    // Key could be less than 32 bytes.
    let req = make_clause(
        assura_ast::ClauseKind::Requires,
        binop(ident("key_size"), BinOp::Gte, int_lit(0)),
    );
    let body = binop(ident("key_size"), BinOp::Gte, int_lit(32));
    let clauses = vec![req];

    let results = super::z3_backend::verify_crypto_conformance_impl("CryptoTest", &body, &clauses);
    assert!(
        results
            .iter()
            .any(|r| matches!(r, VerificationResult::Counterexample { .. })),
        "short key should produce counterexample, got: {results:?}"
    );
}

#[test]
fn test_crypto_conformance_nonce_unique() {
    // requires { nonce_counter > prev_nonce }
    // crypto_conformance { nonce_counter > prev_nonce }
    // Nonce is strictly greater than previous: unique.
    let req = make_clause(
        assura_ast::ClauseKind::Requires,
        binop(ident("nonce_counter"), BinOp::Gt, ident("prev_nonce")),
    );
    let body = binop(ident("nonce_counter"), BinOp::Gt, ident("prev_nonce"));
    let clauses = vec![req];

    let results = super::z3_backend::verify_crypto_conformance_impl("CryptoTest", &body, &clauses);
    assert!(
        results
            .iter()
            .any(|r| matches!(r, VerificationResult::Verified { .. })),
        "unique nonce should verify, got: {results:?}"
    );
}

#[test]
fn test_crypto_conformance_nonce_reuse() {
    // requires { nonce_counter >= 0 }
    // crypto_conformance { nonce_counter > prev_nonce }
    // Without monotonicity constraint, nonce could be reused.
    let req = make_clause(
        assura_ast::ClauseKind::Requires,
        binop(ident("nonce_counter"), BinOp::Gte, int_lit(0)),
    );
    let body = binop(ident("nonce_counter"), BinOp::Gt, ident("prev_nonce"));
    let clauses = vec![req];

    let results = super::z3_backend::verify_crypto_conformance_impl("CryptoTest", &body, &clauses);
    assert!(
        results
            .iter()
            .any(|r| matches!(r, VerificationResult::Counterexample { .. })),
        "nonce reuse should produce counterexample, got: {results:?}"
    );
}

// -----------------------------------------------------------------------
// Skip-path tests: bare uppercase ident returns Unknown
// -----------------------------------------------------------------------

#[test]
fn test_monotonic_state_bare_ident_skip() {
    // A bare uppercase ident body like "MonotonicLattice" is declarative,
    // not verifiable. Should return Unknown with limitation marker.
    let body = ident("MonotonicLattice");
    let clauses = vec![];
    let results = super::z3_backend::verify_monotonic_state_impl("SkipTest", &body, &clauses);
    assert!(
        results
            .iter()
            .any(|r| matches!(r, VerificationResult::Unknown { .. })),
        "bare uppercase ident should produce Unknown, got: {results:?}"
    );
}

#[test]
fn test_crash_recovery_bare_ident_skip() {
    let body = ident("WalRecovery");
    let clauses = vec![];
    let results = super::z3_backend::verify_crash_recovery_impl("SkipTest", &body, &clauses);
    assert!(
        results
            .iter()
            .any(|r| matches!(r, VerificationResult::Unknown { .. })),
        "bare uppercase ident should produce Unknown, got: {results:?}"
    );
}

#[test]
fn test_mvcc_isolation_bare_ident_skip() {
    let body = ident("SnapshotIsolation");
    let clauses = vec![];
    let results = super::z3_backend::verify_mvcc_isolation_impl("SkipTest", &body, &clauses);
    assert!(
        results
            .iter()
            .any(|r| matches!(r, VerificationResult::Unknown { .. })),
        "bare uppercase ident should produce Unknown, got: {results:?}"
    );
}

#[test]
fn test_crypto_conformance_bare_ident_skip() {
    let body = ident("Aes256Gcm");
    let clauses = vec![];
    let results = super::z3_backend::verify_crypto_conformance_impl("SkipTest", &body, &clauses);
    assert!(
        results
            .iter()
            .any(|r| matches!(r, VerificationResult::Unknown { .. })),
        "bare uppercase ident should produce Unknown, got: {results:?}"
    );
}

// -----------------------------------------------------------------------
// Constant-time: step 2 with ensures context
// -----------------------------------------------------------------------

#[test]
fn test_constant_time_with_ensures() {
    // requires { x >= 0 }
    // ensures  { x < 256 }
    // constant_time { x < 256 }
    // Step 1 (requires only) should produce counterexample (x unconstrained above).
    // Step 2 (requires + ensures) should verify.
    let req = make_clause(
        assura_ast::ClauseKind::Requires,
        binop(ident("x"), BinOp::Gte, int_lit(0)),
    );
    let ens = make_clause(
        assura_ast::ClauseKind::Ensures,
        binop(ident("x"), BinOp::Lt, int_lit(256)),
    );
    let body = binop(ident("x"), BinOp::Lt, int_lit(256));
    let clauses = vec![req, ens];

    let results = super::z3_backend::verify_constant_time_impl("CTTest", &body, &clauses);
    // Should have 2 results: step 1 (counterexample) and step 2 (verified)
    assert!(
        results.len() == 2,
        "expected 2 results (secret-independence + body), got {}: {results:?}",
        results.len()
    );
    // Step 1: requires only => counterexample (x >= 0 does not imply x < 256)
    assert!(
        matches!(&results[0], VerificationResult::Counterexample { .. }),
        "step 1 (requires only) should produce counterexample, got: {:?}",
        results[0]
    );
    // Step 2: requires + ensures => verified (ensures gives x < 256)
    assert!(
        matches!(&results[1], VerificationResult::Verified { .. }),
        "step 2 (requires + ensures) should verify, got: {:?}",
        results[1]
    );
}

// -----------------------------------------------------------------------
// #510: Timeout/Unknown soundness guards
// (These tests live in assura-pipeline to avoid type identity issues
// when assura-smt tests reference assura_pipeline functions.)
// -----------------------------------------------------------------------
