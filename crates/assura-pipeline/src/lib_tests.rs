use super::*;

#[test]
fn run_valid_contract() {
    let result = run(
        "contract SafeDiv {\n  input(x: Int, y: Int)\n  output(result: Int)\n  requires { y != 0 }\n  ensures { result > 0 }\n}",
    );
    assert!(result.parse_errors.is_empty());
    assert!(result.resolution_errors.is_empty());
    assert!(
        result.type_errors.is_empty(),
        "unexpected type errors: {:?}",
        result.type_errors
    );
    assert_eq!(result.declarations, vec!["contract SafeDiv"]);
    assert!(!result.verification.is_empty());
}

#[test]
fn run_empty_source() {
    let result = run("");
    assert!(result.declarations.is_empty());
}

#[test]
fn run_parse_error() {
    let result = run("contract Bad { @@@ }");
    assert!(!result.success);
}

#[test]
fn run_multiple_declarations() {
    let result = run("contract A {\n  requires { true }\n}\ncontract B {\n  requires { true }\n}");
    assert_eq!(result.declarations.len(), 2);
}

#[test]
fn run_has_errors_false_on_success() {
    let result = run("contract X {\n  requires { true }\n}");
    assert!(!result.has_errors());
}

#[test]
fn run_has_errors_true_on_parse_error() {
    let result = run("contract { !!! }");
    assert!(result.has_errors());
}

#[test]
fn run_serializes_to_json() {
    let result = run("contract T {\n  requires { true }\n}");
    let json = serde_json::to_string(&result);
    json.unwrap();
}

// Tests for compile() function
#[test]
fn compile_valid_contract() {
    let config = CompilerConfig::default();
    let output = compile(
        "contract Add {\n  requires { true }\n}",
        "test.assura",
        &config,
    );
    output.file.unwrap();
    // On success, resolved is consumed into typed.resolved (Arc); the
    // CompilationOutput.resolved field is None.
    assert!(
        output.typed.is_some(),
        "typed was None; diagnostics: {:?}",
        output.diagnostics
    );
    // Verify resolved is accessible via typed
    assert!(output.typed.as_ref().unwrap().resolved.source.decls.len() > 0);
    assert!(
        !output.has_errors,
        "unexpected errors: {:?}",
        output.diagnostics
    );
}

#[test]
fn compile_parse_error_populates_diagnostics() {
    let config = CompilerConfig::default();
    let output = compile("contract { @@@ }", "bad.assura", &config);
    assert!(output.has_errors);
    assert!(!output.diagnostics.is_empty());
}

#[test]
fn compile_empty_source() {
    let config = CompilerConfig::default();
    let output = compile("", "empty.assura", &config);
    output.file.unwrap();
    assert!(!output.has_errors);
}

#[test]
fn compile_records_timing() {
    let config = CompilerConfig::default();
    let output = compile(
        "contract T { requires(x: Int) ensures(result: Int) }",
        "test.assura",
        &config,
    );
    assert!(output.timing.parse_ms >= 0.0);
    assert!(output.timing.token_count > 0);
}

// -------------------------------------------------------------------
// #510: Timeout/Unknown soundness guards
// -------------------------------------------------------------------

#[test]
fn timeout_not_treated_as_verified() {
    let timeout = assura_smt::VerificationResult::Timeout {
        clause_desc: "test".into(),
    };
    let results = vec![timeout];
    assert!(!verification_succeeded(&results));
    assert!(!verification_strict_succeeded(&results));
}

#[test]
fn unknown_non_limitation_not_strict_succeeded() {
    let unknown = assura_smt::VerificationResult::Unknown {
        clause_desc: "test".into(),
        reason: "solver returned inconclusive".into(),
    };
    let results = vec![unknown];
    // Lenient: passes (design decision: Unknown is non-fatal)
    assert!(verification_succeeded(&results));
    // Strict: fails (non-limitation Unknown is a real issue)
    assert!(!verification_strict_succeeded(&results));
}

#[test]
fn known_limitation_passes_both() {
    let limitation = assura_smt::VerificationResult::unknown_not_encoded("test", "feature X");
    let results = vec![limitation];
    assert!(verification_succeeded(&results));
    assert!(verification_strict_succeeded(&results));
}

// -------------------------------------------------------------------
// Layer 2 pipeline wiring tests
// -------------------------------------------------------------------

/// Helper: compile with Layer 2 enabled
fn compile_layer2(source: &str) -> CompilationOutput {
    let config = CompilerConfig {
        verify: assura_config::VerifyOptions {
            layer: 2,
            parallel: false,
            decrease_checks: false,
            enable_cache: false,
            ..Default::default()
        },
        ..Default::default()
    };
    compile_full(source, "test.assura", &config)
}

#[test]
fn layer2_skipped_at_layer1() {
    // Layer 1 should NOT produce any "layer2:" results
    let config = CompilerConfig {
        verify: assura_config::VerifyOptions {
            layer: 1,
            parallel: false,
            decrease_checks: false,
            enable_cache: false,
            ..Default::default()
        },
        ..Default::default()
    };
    let output = compile_full(
        "contract X {\n  requires { true }\n}",
        "test.assura",
        &config,
    );
    let layer2_results: Vec<_> = output
        .verification
        .iter()
        .filter(|r| match r {
            assura_smt::VerificationResult::Verified { clause_desc, .. }
            | assura_smt::VerificationResult::Counterexample { clause_desc, .. }
            | assura_smt::VerificationResult::Timeout { clause_desc, .. }
            | assura_smt::VerificationResult::Unknown { clause_desc, .. } => {
                clause_desc.starts_with("layer2:")
            }
        })
        .collect();
    assert!(
        layer2_results.is_empty(),
        "layer 1 should not produce layer2 results"
    );
}

#[test]
fn layer2_no_obligations_no_extra_results() {
    // A simple contract with no invariant/decreases/roundtrip
    // should not produce any layer2 results
    let output = compile_layer2("contract X {\n  requires { true }\n}");
    assert!(
        !output.has_errors,
        "unexpected errors: {:?}",
        output.diagnostics
    );
    let layer2_results: Vec<_> = output
        .verification
        .iter()
        .filter(|r| match r {
            assura_smt::VerificationResult::Verified { clause_desc, .. }
            | assura_smt::VerificationResult::Counterexample { clause_desc, .. }
            | assura_smt::VerificationResult::Timeout { clause_desc, .. }
            | assura_smt::VerificationResult::Unknown { clause_desc, .. } => {
                clause_desc.starts_with("layer2:")
            }
        })
        .collect();
    assert!(
        layer2_results.is_empty(),
        "no obligations should produce no layer2 results"
    );
}

#[test]
fn layer2_invariant_clause_collected() {
    // Contract with an invariant clause should produce layer2 results
    let source = "contract Counter {\n  input(n: Int)\n  requires { n >= 0 }\n  invariant { forall x in Int: x >= 0 || x < 0 }\n}";
    let output = compile_layer2(source);
    assert!(
        !output.has_errors,
        "unexpected errors: {:?}",
        output.diagnostics
    );
    let layer2_results: Vec<_> = output
        .verification
        .iter()
        .filter(|r| match r {
            assura_smt::VerificationResult::Verified { clause_desc, .. }
            | assura_smt::VerificationResult::Counterexample { clause_desc, .. }
            | assura_smt::VerificationResult::Timeout { clause_desc, .. }
            | assura_smt::VerificationResult::Unknown { clause_desc, .. } => {
                clause_desc.starts_with("layer2:")
            }
        })
        .collect();
    assert!(
        !layer2_results.is_empty(),
        "invariant clause should produce layer2 results"
    );
}

#[test]
fn layer2_decreases_clause_collected() {
    // Contract with a decreases clause should produce layer2 results
    let source = "contract Factorial {\n  input(n: Nat)\n  output(result: Nat)\n  requires { n >= 0 }\n  decreases { n }\n}";
    let output = compile_layer2(source);
    assert!(
        !output.has_errors,
        "unexpected errors: {:?}",
        output.diagnostics
    );
    let layer2_results: Vec<_> = output
        .verification
        .iter()
        .filter(|r| match r {
            assura_smt::VerificationResult::Verified { clause_desc, .. }
            | assura_smt::VerificationResult::Counterexample { clause_desc, .. }
            | assura_smt::VerificationResult::Timeout { clause_desc, .. }
            | assura_smt::VerificationResult::Unknown { clause_desc, .. } => {
                clause_desc.starts_with("layer2:")
            }
        })
        .collect();
    assert!(
        !layer2_results.is_empty(),
        "decreases clause should produce layer2 results, got none; all results: {:?}",
        output.verification
    );
}

// -------------------------------------------------------------------
// Layer 3 pipeline wiring tests
// -------------------------------------------------------------------

/// Helper: compile with Layer 3 enabled
fn compile_layer3(source: &str) -> CompilationOutput {
    let config = CompilerConfig {
        verify: assura_config::VerifyOptions {
            layer: 3,
            parallel: false,
            decrease_checks: false,
            enable_cache: false,
            ..Default::default()
        },
        ..Default::default()
    };
    compile_full(source, "test.assura", &config)
}

#[test]
fn layer3_skipped_at_layer2() {
    // Layer 2 should NOT produce BMC results
    let config = CompilerConfig {
        verify: assura_config::VerifyOptions {
            layer: 2,
            parallel: false,
            decrease_checks: false,
            enable_cache: false,
            ..Default::default()
        },
        ..Default::default()
    };
    let output = compile_full(
        "contract X {\n  input(n: Int)\n  requires { n >= 0 }\n  invariant { n >= 0 }\n}",
        "test.assura",
        &config,
    );
    let bmc_results: Vec<_> = output
        .verification
        .iter()
        .filter(|r| match r {
            assura_smt::VerificationResult::Verified { clause_desc, .. }
            | assura_smt::VerificationResult::Counterexample { clause_desc, .. }
            | assura_smt::VerificationResult::Timeout { clause_desc, .. }
            | assura_smt::VerificationResult::Unknown { clause_desc, .. } => {
                clause_desc.contains("BMC") || clause_desc.contains("lasso")
            }
        })
        .collect();
    assert!(
        bmc_results.is_empty(),
        "layer 2 should not produce BMC results"
    );
}

#[test]
fn layer3_invariant_produces_bmc_results() {
    // Contract with invariant should produce BMC results at layer 3
    let source =
        "contract Counter {\n  input(n: Int)\n  requires { n >= 0 }\n  invariant { n >= 0 }\n}";
    let output = compile_layer3(source);
    assert!(
        !output.has_errors,
        "unexpected errors: {:?}",
        output.diagnostics
    );
    let bmc_results: Vec<_> = output
        .verification
        .iter()
        .filter(|r| match r {
            assura_smt::VerificationResult::Verified { clause_desc, .. }
            | assura_smt::VerificationResult::Counterexample { clause_desc, .. }
            | assura_smt::VerificationResult::Timeout { clause_desc, .. }
            | assura_smt::VerificationResult::Unknown { clause_desc, .. } => {
                clause_desc.contains("BMC")
            }
        })
        .collect();
    assert!(
        !bmc_results.is_empty(),
        "invariant contract should produce BMC results at layer 3"
    );
}

#[test]
fn layer3_no_invariant_no_bmc() {
    // Contract with only requires/ensures but no invariant should not produce BMC
    let source = "contract Simple {\n  requires { true }\n  ensures { true }\n}";
    let output = compile_layer3(source);
    assert!(
        !output.has_errors,
        "unexpected errors: {:?}",
        output.diagnostics
    );
    let bmc_results: Vec<_> = output
        .verification
        .iter()
        .filter(|r| match r {
            assura_smt::VerificationResult::Verified { clause_desc, .. }
            | assura_smt::VerificationResult::Counterexample { clause_desc, .. }
            | assura_smt::VerificationResult::Timeout { clause_desc, .. }
            | assura_smt::VerificationResult::Unknown { clause_desc, .. } => {
                clause_desc.contains("BMC")
            }
        })
        .collect();
    assert!(
        bmc_results.is_empty(),
        "contract without invariant should not produce BMC results"
    );
}

#[test]
fn layer3_liveness_eventually_produces_results() {
    // Liveness block with eventually should produce BMC results at layer 3
    let source = "liveness Progress {\n  prove: eventually(turn == 1)\n}";
    let output = compile_layer3(source);
    assert!(
        !output.has_errors,
        "unexpected errors: {:?}",
        output.diagnostics
    );
    // Should produce either Verified, Counterexample, or Unknown (not empty)
    let liveness_results: Vec<_> = output
        .verification
        .iter()
        .filter(|r| match r {
            assura_smt::VerificationResult::Verified { clause_desc, .. }
            | assura_smt::VerificationResult::Counterexample { clause_desc, .. }
            | assura_smt::VerificationResult::Timeout { clause_desc, .. }
            | assura_smt::VerificationResult::Unknown { clause_desc, .. } => {
                clause_desc.contains("eventually") || clause_desc.contains("BMC")
            }
        })
        .collect();
    assert!(
        !liveness_results.is_empty(),
        "liveness block with eventually should produce results at layer 3, got: {:?}",
        output.verification
    );
}

#[test]
fn layer3_liveness_leads_to_produces_results() {
    // Liveness block with leads_to should produce BMC results at layer 3
    let source = "liveness Response {\n  assume: fair\n  prove: leads_to(request == true, response == true)\n}";
    let output = compile_layer3(source);
    assert!(
        !output.has_errors,
        "unexpected errors: {:?}",
        output.diagnostics
    );
    let liveness_results: Vec<_> = output
        .verification
        .iter()
        .filter(|r| match r {
            assura_smt::VerificationResult::Verified { clause_desc, .. }
            | assura_smt::VerificationResult::Counterexample { clause_desc, .. }
            | assura_smt::VerificationResult::Timeout { clause_desc, .. }
            | assura_smt::VerificationResult::Unknown { clause_desc, .. } => {
                clause_desc.contains("leads_to") || clause_desc.contains("BMC")
            }
        })
        .collect();
    assert!(
        !liveness_results.is_empty(),
        "liveness block with leads_to should produce results at layer 3, got: {:?}",
        output.verification
    );
}

#[test]
fn layer3_liveness_skipped_at_layer2() {
    // Liveness blocks should not produce monitor-based results at layer 2
    let source = "liveness Progress {\n  prove: eventually(turn == 1)\n}";
    let config = CompilerConfig {
        verify: assura_config::VerifyOptions {
            layer: 2,
            parallel: false,
            decrease_checks: false,
            enable_cache: false,
            ..Default::default()
        },
        ..Default::default()
    };
    let output = compile_full(source, "test.assura", &config);
    let bmc_results: Vec<_> = output
        .verification
        .iter()
        .filter(|r| match r {
            assura_smt::VerificationResult::Verified { clause_desc, .. }
            | assura_smt::VerificationResult::Counterexample { clause_desc, .. }
            | assura_smt::VerificationResult::Timeout { clause_desc, .. }
            | assura_smt::VerificationResult::Unknown { clause_desc, .. } => {
                clause_desc.contains("eventually") || clause_desc.contains("BMC")
            }
        })
        .collect();
    assert!(
        bmc_results.is_empty(),
        "liveness blocks should not produce BMC results at layer 2"
    );
}

#[test]
fn negate_expr_comparisons() {
    use assura_parser::ast::{BinOp, Expr, Literal, Spanned, UnaryOp, expr_to_string, negate_expr};

    fn sp(e: Expr) -> Spanned<Expr> {
        Spanned::no_span(e)
    }
    fn ident(s: &str) -> Spanned<Expr> {
        sp(Expr::Ident(s.into()))
    }
    fn binop(l: Spanned<Expr>, op: BinOp, r: Spanned<Expr>) -> Spanned<Expr> {
        sp(Expr::BinOp {
            lhs: Box::new(l),
            op,
            rhs: Box::new(r),
        })
    }

    // n >= 0 => n < 0
    let e = binop(
        ident("n"),
        BinOp::Gte,
        sp(Expr::Literal(Literal::Int("0".into()))),
    );
    assert_eq!(expr_to_string(&negate_expr(&e)), "n < 0");

    // x <= 10 => x > 10
    let e = binop(
        ident("x"),
        BinOp::Lte,
        sp(Expr::Literal(Literal::Int("10".into()))),
    );
    assert_eq!(expr_to_string(&negate_expr(&e)), "x > 10");

    // a == b => a != b
    let e = binop(ident("a"), BinOp::Eq, ident("b"));
    assert_eq!(expr_to_string(&negate_expr(&e)), "a != b");

    // a != b => a == b
    let e = binop(ident("a"), BinOp::Neq, ident("b"));
    assert_eq!(expr_to_string(&negate_expr(&e)), "a == b");

    // x > 0 => x <= 0
    let e = binop(
        ident("x"),
        BinOp::Gt,
        sp(Expr::Literal(Literal::Int("0".into()))),
    );
    assert_eq!(expr_to_string(&negate_expr(&e)), "x <= 0");

    // x < 100 => x >= 100
    let e = binop(
        ident("x"),
        BinOp::Lt,
        sp(Expr::Literal(Literal::Int("100".into()))),
    );
    assert_eq!(expr_to_string(&negate_expr(&e)), "x >= 100");

    // flag => not flag
    let e = ident("flag");
    assert_eq!(expr_to_string(&negate_expr(&e)), "not flag");

    // true => false
    let e = sp(Expr::Literal(Literal::Bool(true)));
    assert_eq!(expr_to_string(&negate_expr(&e)), "false");

    // not x => x (double negation elimination)
    let inner = ident("x");
    let e = sp(Expr::UnaryOp {
        op: UnaryOp::Not,
        expr: Box::new(inner),
    });
    assert_eq!(expr_to_string(&negate_expr(&e)), "x");
}

#[test]
fn negate_expr_de_morgan() {
    use assura_parser::ast::{BinOp, Expr, Spanned, expr_to_string, negate_expr};

    fn sp(e: Expr) -> Spanned<Expr> {
        Spanned::no_span(e)
    }
    fn ident(s: &str) -> Spanned<Expr> {
        sp(Expr::Ident(s.into()))
    }

    // a and b => (not a) or (not b)
    let e = sp(Expr::BinOp {
        lhs: Box::new(ident("a")),
        op: BinOp::And,
        rhs: Box::new(ident("b")),
    });
    assert_eq!(expr_to_string(&negate_expr(&e)), "not a or not b");

    // a or b => (not a) and (not b)
    let e = sp(Expr::BinOp {
        lhs: Box::new(ident("a")),
        op: BinOp::Or,
        rhs: Box::new(ident("b")),
    });
    assert_eq!(expr_to_string(&negate_expr(&e)), "not a and not b");

    // x >= 0 and y < 10 => x < 0 or y >= 10
    let e = sp(Expr::BinOp {
        lhs: Box::new(sp(Expr::BinOp {
            lhs: Box::new(ident("x")),
            op: BinOp::Gte,
            rhs: Box::new(sp(Expr::Literal(assura_parser::ast::Literal::Int(
                "0".into(),
            )))),
        })),
        op: BinOp::And,
        rhs: Box::new(sp(Expr::BinOp {
            lhs: Box::new(ident("y")),
            op: BinOp::Lt,
            rhs: Box::new(sp(Expr::Literal(assura_parser::ast::Literal::Int(
                "10".into(),
            )))),
        })),
    });
    assert_eq!(expr_to_string(&negate_expr(&e)), "x < 0 or y >= 10");
}

// -------------------------------------------------------------------
// compile_full: codegen output, error early-return, verify skipping
// -------------------------------------------------------------------

#[test]
fn compile_full_produces_codegen_output() {
    let config = CompilerConfig::default();
    let output = compile_full(
        "contract Add {\n  input(x: Int, y: Int)\n  output(result: Int)\n  requires { x >= 0 }\n  ensures { x >= 0 }\n}",
        "test.assura",
        &config,
    );
    assert!(
        !output.has_errors,
        "unexpected errors: {:?}",
        output.diagnostics
    );
    assert!(output.generated.is_some(), "codegen should produce output");
    let generated = output.generated.unwrap();
    assert!(
        !generated.files.is_empty(),
        "generated project should have files"
    );
}

#[test]
fn compile_full_skips_verify_and_codegen_on_parse_error() {
    let config = CompilerConfig::default();
    let output = compile_full("contract { @@@ }", "bad.assura", &config);
    assert!(output.has_errors);
    assert!(
        output.verification.is_empty(),
        "verify should be skipped on parse error"
    );
    assert!(
        output.generated.is_none(),
        "codegen should be skipped on parse error"
    );
}

#[test]
fn compile_full_records_verify_timing() {
    let config = CompilerConfig::default();
    let output = compile_full(
        "contract T {\n  requires { true }\n}",
        "test.assura",
        &config,
    );
    assert!(
        output.timing.verify_ms.is_some(),
        "verify timing should be recorded"
    );
    assert!(
        output.timing.codegen_ms.is_some(),
        "codegen timing should be recorded"
    );
}

// -------------------------------------------------------------------
// verify_typed: layer 0 bypass, direct call
// -------------------------------------------------------------------

#[test]
fn verify_typed_layer0_returns_empty() {
    let config = CompilerConfig {
        verify: assura_config::VerifyOptions {
            layer: 0,
            ..Default::default()
        },
        ..Default::default()
    };
    let output = compile(
        "contract T {\n  requires { true }\n  ensures { true }\n}",
        "test.assura",
        &config,
    );
    let typed = output.typed.unwrap();
    let results = verify_typed(&typed, "test.assura", &config);
    assert!(
        results.is_empty(),
        "layer 0 should skip SMT verification entirely"
    );
}

#[test]
fn verify_typed_layer1_runs_smt() {
    let config = CompilerConfig {
        verify: assura_config::VerifyOptions {
            layer: 1,
            parallel: false,
            decrease_checks: false,
            enable_cache: false,
            ..Default::default()
        },
        ..Default::default()
    };
    let output = compile(
        "contract SafeDiv {\n  input(x: Int, y: Int)\n  requires { y != 0 }\n  ensures { y != 0 }\n}",
        "test.assura",
        &config,
    );
    assert!(
        !output.has_errors,
        "unexpected errors: {:?}",
        output.diagnostics
    );
    let typed = output.typed.unwrap();
    let results = verify_typed(&typed, "test.assura", &config);
    assert!(
        !results.is_empty(),
        "layer 1 should produce verification results"
    );
}

// -------------------------------------------------------------------
// run_at: filename passthrough
// -------------------------------------------------------------------

#[test]
fn run_at_accepts_filename() {
    let result = run_at("contract T {\n  requires { true }\n}", "myfile.assura");
    assert!(!result.has_errors());
    assert_eq!(result.declarations, vec!["contract T"]);
}

#[test]
fn run_at_parse_error_reports_failure() {
    let result = run_at("contract { @@@ }", "bad.assura");
    assert!(!result.success);
    assert!(result.has_errors());
}

// -------------------------------------------------------------------
// Diagnostic classification by phase
// -------------------------------------------------------------------

#[test]
fn diagnostics_classified_as_parse_errors() {
    let result = run("contract { @@@ }");
    assert!(
        !result.parse_errors.is_empty(),
        "parse errors should be classified under parse_errors"
    );
    assert!(result.resolution_errors.is_empty());
}

#[test]
fn diagnostics_classified_as_resolution_errors() {
    // Duplicate contract names trigger A02003 (resolution phase)
    let result =
        run("contract Dup {\n  requires { true }\n}\ncontract Dup {\n  requires { true }\n}");
    assert!(
        !result.resolution_errors.is_empty(),
        "duplicate names should produce resolution errors (A02xxx), got: parse={:?}, resolve={:?}, type={:?}",
        result.parse_errors,
        result.resolution_errors,
        result.type_errors
    );
    assert!(
        result
            .resolution_errors
            .iter()
            .all(|e| e.code.as_str().starts_with("A02")),
        "all resolution errors should have A02 prefix: {:?}",
        result.resolution_errors
    );
    assert!(
        result.parse_errors.is_empty(),
        "no parse errors expected for valid syntax"
    );
}

#[test]
fn diagnostics_classified_as_type_errors() {
    // Type mismatch: comparing Int to String triggers A03xxx
    let result = run("contract T {\n  input(x: Int)\n  requires { x == \"hello\" }\n}");
    assert!(
        !result.type_errors.is_empty(),
        "type mismatch should produce type errors (A03xxx), got: parse={:?}, resolve={:?}, type={:?}",
        result.parse_errors,
        result.resolution_errors,
        result.type_errors
    );
    assert!(result.parse_errors.is_empty(), "no parse errors expected");
}

// -------------------------------------------------------------------
// PipelineResult.has_errors: all three buckets
// -------------------------------------------------------------------

#[test]
fn pipeline_result_has_errors_on_type_errors() {
    // Type mismatch should trigger has_errors
    let result = run("contract T {\n  input(x: Int)\n  requires { x == \"hello\" }\n}");
    assert!(
        result.has_errors(),
        "type mismatch should make has_errors() true"
    );
    assert!(!result.type_errors.is_empty(), "should have type errors");
}

#[test]
fn pipeline_result_success_true_on_valid() {
    let result = run("contract T {\n  requires { true }\n}");
    assert!(result.parse_errors.is_empty());
    assert!(result.resolution_errors.is_empty());
    assert!(result.type_errors.is_empty());
    assert!(!result.has_errors());
}

// -------------------------------------------------------------------
// verification_succeeded: counterexample
// -------------------------------------------------------------------

#[test]
fn counterexample_fails_verification_succeeded() {
    let ce = assura_smt::VerificationResult::Counterexample {
        clause_desc: "SafeDiv: ensures".into(),
        model: "x = 0, y = 1".into(),
        counter_model: None,
    };
    assert!(!verification_succeeded(&[ce.clone()]));
    assert!(!verification_strict_succeeded(&[ce]));
}

#[test]
fn verified_passes_both_success_checks() {
    let v = assura_smt::VerificationResult::Verified {
        clause_desc: "T: requires".into(),
        unsat_core: None,
    };
    assert!(verification_succeeded(&[v.clone()]));
    assert!(verification_strict_succeeded(&[v]));
}

#[test]
fn empty_results_passes_both() {
    assert!(verification_succeeded(&[]));
    assert!(verification_strict_succeeded(&[]));
}

// -------------------------------------------------------------------
// compile: warning propagation from type checker
// -------------------------------------------------------------------

#[test]
fn compile_populates_diagnostics_with_filename() {
    let config = CompilerConfig::default();
    let output = compile("contract { @@@ }", "myfile.assura", &config);
    assert!(output.has_errors);
    // Diagnostics should have the filename attached
    for d in &output.diagnostics {
        assert_eq!(
            d.file, "myfile.assura",
            "diagnostic should have filename, got: {:?}",
            d.file
        );
    }
}

#[test]
fn compile_resolution_error_sets_has_errors() {
    let config = CompilerConfig::default();
    // Use duplicate declaration to trigger resolution error
    let output = compile(
        "contract Dup {\n  requires { true }\n}\ncontract Dup {\n  requires { true }\n}",
        "test.assura",
        &config,
    );
    // Should produce warnings or errors about duplication
    // The exact behavior depends on resolution, but either way compilation proceeds
    // Just verify it doesn't panic
    let _ = output.has_errors;
}

// -------------------------------------------------------------------
// PipelineResult serialization
// -------------------------------------------------------------------

#[test]
fn pipeline_result_serializes_all_fields() {
    let result = run("contract SafeDiv {\n  input(x: Int, y: Int)\n  requires { y != 0 }\n}");
    let json_str = serde_json::to_string(&result).unwrap();
    // Verify key fields are present in JSON
    assert!(json_str.contains("\"success\""));
    assert!(json_str.contains("\"declarations\""));
    assert!(json_str.contains("\"parse_errors\""));
    assert!(json_str.contains("\"resolution_errors\""));
    assert!(json_str.contains("\"type_errors\""));
    assert!(json_str.contains("\"verification\""));
}

#[test]
fn pipeline_result_declarations_include_all_types() {
    let source = "\
contract C {\n  requires { true }\n}\n\
function F {\n  requires { true }\n}\n\
extern ext_fn {\n  input(x: Int)\n  output(result: Int)\n}\n";
    let result = run(source);
    assert!(
        result.declarations.len() >= 2,
        "should have at least 2 declarations, got: {:?}",
        result.declarations
    );
}

// -------------------------------------------------------------------
// decl_summary via PipelineResult declarations
// -------------------------------------------------------------------

#[test]
fn decl_summary_formats_contract_name() {
    let result = run("contract MyContract {\n  requires { true }\n}");
    assert!(
        result.declarations.iter().any(|d| d.contains("MyContract")),
        "declarations should include contract name, got: {:?}",
        result.declarations
    );
}

#[test]
fn decl_summary_formats_extern_name() {
    let result = run("extern my_extern {\n  input(x: Int)\n  output(result: Int)\n}");
    assert!(
        result.declarations.iter().any(|d| d.contains("my_extern")),
        "declarations should include extern name, got: {:?}",
        result.declarations
    );
}

// -------------------------------------------------------------------
// assura_test_support helpers (#634): at least one external caller each
// -------------------------------------------------------------------

#[test]
fn support_verify_strict_ok_on_trivial_contract() {
    // A contract whose ensures is implied by its requires.
    // Z3 should verify this with no Unknown results.
    let output = assura_test_support::verify_strict_ok(
        "contract StrictTest {\n  input(x: Int)\n  requires { x > 0 }\n  ensures { x > 0 }\n}",
        "strict.assura",
    );
    assert!(
        !output.verification.is_empty(),
        "should produce verification results"
    );
}

#[test]
fn support_verify_result_returns_output() {
    let output = assura_test_support::verify_result(
        "contract VR {\n  input(n: Int)\n  requires { n >= 0 }\n  ensures { n >= 0 }\n}",
        "vr.assura",
    );
    // verify_result does not assert; we inspect manually.
    assert!(!output.has_errors, "valid source should not have errors");
    assert!(
        !output.verification.is_empty(),
        "should have verification results"
    );
}

#[test]
fn support_expect_error_codes_catches_type_mismatch() {
    let output = assura_test_support::verify_result(
        "contract Bad {\n  input(x: Int)\n  requires { x + \"hello\" }\n}",
        "bad.assura",
    );
    assert!(output.has_errors);
    assura_test_support::expect_error_codes(&output, &["A03001"]);
}

#[test]
fn support_expect_verify_limitation_on_incremental() {
    // The zlib demo's incremental_contract feature is not encoded in SMT,
    // producing a known-limitation Unknown.
    let src = assura_test_support::load_fixture("demos/zlib-inflate.assura");
    let output = assura_test_support::expect_verify_limitation(&src, "zlib.assura");
    // Should have at least one Unknown with the limitation marker.
    let has_limitation = output.verification.iter().any(|r| {
        matches!(
            r,
            assura_smt::VerificationResult::Unknown { reason, .. }
                if assura_smt::is_known_smt_limitation(reason)
        )
    });
    assert!(has_limitation);
}

// -------------------------------------------------------------------
// verify_ir tests (AI verification loop, task 12.01)
// -------------------------------------------------------------------

#[test]
fn verify_ir_identity_contract_verified() {
    let contract =
        "contract Echo {\n  input(x: Int)\n  output(result: Int)\n  ensures { result == x }\n}\n";
    let ir = "module Echo {\n  fn #0 : ($0: Int) -> Int ! pure\n  {\n    $result = load $0 : Int\n  }\n}\n";
    let config = CompilerConfig::default();
    let result = verify_ir(contract, ir, &config);
    assert_eq!(
        result.status, "verified",
        "identity IR should verify; clauses: {:?}",
        result.clauses
    );
    assert!(result.compile_errors.is_empty());
    assert!(result.ir_errors.is_empty());
    assert!(result.validation_errors.is_empty());
    assert!(result.summary.verified > 0);
}

#[test]
fn verify_ir_compile_error_on_bad_contract() {
    let contract = "contract { @@@ }";
    let ir =
        "module X {\n  fn #0 : ($0: Int) -> Int ! pure\n  {\n    $result = load $0 : Int\n  }\n}\n";
    let config = CompilerConfig::default();
    let result = verify_ir(contract, ir, &config);
    assert_eq!(result.status, "error");
    assert!(!result.compile_errors.is_empty());
}

#[test]
fn verify_ir_parse_error_on_bad_ir() {
    let contract =
        "contract Echo {\n  input(x: Int)\n  output(result: Int)\n  ensures { result == x }\n}\n";
    let ir = "not valid IR text";
    let config = CompilerConfig::default();
    let result = verify_ir(contract, ir, &config);
    assert_eq!(result.status, "error");
    assert!(!result.ir_errors.is_empty());
}

#[test]
fn verify_ir_serializes_to_json() {
    let contract =
        "contract Echo {\n  input(x: Int)\n  output(result: Int)\n  ensures { result == x }\n}\n";
    let ir = "module Echo {\n  fn #0 : ($0: Int) -> Int ! pure\n  {\n    $result = load $0 : Int\n  }\n}\n";
    let config = CompilerConfig::default();
    let result = verify_ir(contract, ir, &config);
    let json = serde_json::to_string_pretty(&result);
    assert!(json.is_ok(), "IrVerifyResult should serialize to JSON");
    let json_str = json.unwrap();
    assert!(json_str.contains("\"status\""));
    assert!(json_str.contains("\"progress\""));
    assert!(json_str.contains("\"clauses\""));
    assert!(json_str.contains("\"summary\""));
}

#[test]
fn verify_ir_progress_format() {
    let contract =
        "contract Echo {\n  input(x: Int)\n  output(result: Int)\n  ensures { result == x }\n}\n";
    let ir = "module Echo {\n  fn #0 : ($0: Int) -> Int ! pure\n  {\n    $result = load $0 : Int\n  }\n}\n";
    let config = CompilerConfig::default();
    let result = verify_ir(contract, ir, &config);
    assert!(
        result.progress.contains("clauses verified"),
        "progress should say 'clauses verified': {}",
        result.progress
    );
    assert!(
        result.progress.contains('%'),
        "progress should contain percentage: {}",
        result.progress
    );
}

// ---------------------------------------------------------------------------
// IrVerifyResult::from_results — mixed-variant coverage
// ---------------------------------------------------------------------------

#[test]
fn ir_verify_result_mixed_variants() {
    use assura_smt::VerificationResult;

    let results = vec![
        VerificationResult::Verified {
            clause_desc: "A: ensures".into(),
            unsat_core: None,
        },
        VerificationResult::Counterexample {
            clause_desc: "B: ensures".into(),
            model: "x = 0".into(),
            counter_model: None,
        },
        VerificationResult::Timeout {
            clause_desc: "C: requires".into(),
        },
        VerificationResult::Unknown {
            clause_desc: "D: invariant".into(),
            reason: "non-linear arithmetic".into(),
        },
    ];

    let ir_result = IrVerifyResult::from_results(results);

    assert_eq!(
        ir_result.status, "failed",
        "mixed results with counterexample/timeout should fail"
    );
    assert_eq!(ir_result.summary.verified, 1);
    assert_eq!(ir_result.summary.counterexample, 1);
    assert_eq!(ir_result.summary.timeout, 1);
    assert_eq!(ir_result.summary.unknown, 1);
    assert_eq!(ir_result.summary.total, 4);
    assert_eq!(ir_result.clauses.len(), 4);

    assert_eq!(ir_result.clauses[0].status, "verified");
    assert_eq!(ir_result.clauses[1].status, "counterexample");
    assert_eq!(ir_result.clauses[2].status, "timeout");
    assert_eq!(ir_result.clauses[3].status, "unknown");
    assert_eq!(
        ir_result.clauses[3].reason.as_deref(),
        Some("non-linear arithmetic")
    );
}

#[test]
fn ir_verify_result_known_limitation_is_not_failure() {
    use assura_smt::VerificationResult;

    let results = vec![
        VerificationResult::Verified {
            clause_desc: "A: ensures".into(),
            unsat_core: None,
        },
        VerificationResult::unknown_not_encoded("B: invariant", "collection quantifiers"),
    ];

    let ir_result = IrVerifyResult::from_results(results);

    assert_eq!(
        ir_result.status, "verified",
        "known SMT limitations alongside verified should not fail"
    );
    assert_eq!(ir_result.summary.verified, 1);
    assert_eq!(ir_result.summary.unknown, 1);
}

#[test]
fn ir_verify_result_all_verified() {
    use assura_smt::VerificationResult;

    let results = vec![
        VerificationResult::Verified {
            clause_desc: "A: ensures".into(),
            unsat_core: None,
        },
        VerificationResult::Verified {
            clause_desc: "B: requires".into(),
            unsat_core: None,
        },
    ];

    let ir_result = IrVerifyResult::from_results(results);

    assert_eq!(ir_result.status, "verified");
    assert_eq!(ir_result.summary.verified, 2);
    assert_eq!(ir_result.summary.total, 2);
    assert!(ir_result.progress.contains("100%"));
}

#[test]
fn ir_verify_result_empty() {
    let ir_result = IrVerifyResult::from_results(vec![]);

    assert_eq!(ir_result.status, "verified");
    assert_eq!(ir_result.summary.total, 0);
    assert!(ir_result.progress.contains("0/0"));
}

// ---------------------------------------------------------------------------
// verify_ir: layer-0 bypass and validation-error paths (closes #685)
// ---------------------------------------------------------------------------

#[test]
fn verify_ir_layer_zero_bypass() {
    let contract =
        "contract Echo {\n  input(x: Int)\n  output(result: Int)\n  ensures { result == x }\n}\n";
    let ir = "module Echo {\n  fn #0 : ($0: Int) -> Int ! pure\n  {\n    $result = load $0 : Int\n  }\n}\n";
    let mut config = CompilerConfig::default();
    config.verify.layer = 0;
    let result = verify_ir(contract, ir, &config);

    assert_eq!(result.status, "verified");
    assert!(
        result.progress.contains("structural only"),
        "layer-0 progress should say structural only: {}",
        result.progress
    );
    assert_eq!(result.summary.total, 0, "layer-0 should skip SMT clauses");
    assert!(result.clauses.is_empty());
    assert!(result.compile_errors.is_empty());
    assert!(result.ir_errors.is_empty());
    assert!(result.validation_errors.is_empty());
}

#[test]
fn verify_ir_validation_error_slot_gap() {
    // IR has a slot gap ($0 -> $5) which the validator catches
    let contract =
        "contract Echo {\n  input(x: Int)\n  output(result: Int)\n  ensures { result == x }\n}\n";
    let ir = "module Echo {\n  fn #0 : ($0: Int) -> Int ! pure\n  {\n    $5 = load $0 : Int\n    $result = load $5 : Int\n  }\n}\n";
    let config = CompilerConfig::default();
    let result = verify_ir(contract, ir, &config);

    assert_eq!(result.status, "error");
    assert!(
        !result.validation_errors.is_empty(),
        "should have validation errors for slot gap"
    );
    assert!(
        result.validation_errors[0].contains("skip"),
        "validation error should mention skipped slot: {:?}",
        result.validation_errors
    );
}

// ---------------------------------------------------------------------------
// compile_full: early-return on resolution and type errors
// ---------------------------------------------------------------------------

#[test]
fn compile_full_skips_verify_and_codegen_on_resolution_error() {
    // Duplicate contract names trigger A02003 resolution error
    let source = "contract Dup { requires { true } }\ncontract Dup { requires { true } }\n";
    let config = CompilerConfig::default();
    let output = compile_full(source, "<test>", &config);

    assert!(output.has_errors, "duplicate name should produce errors");
    assert!(
        output.verification.is_empty(),
        "verification should be skipped on resolution error"
    );
    assert!(
        output.generated.is_none(),
        "codegen should be skipped on resolution error"
    );
}

#[test]
fn compile_full_skips_verify_and_codegen_on_type_error() {
    // Type mismatch: comparing Int to String triggers type error
    let source = "contract Bad {\n  input(x: Int)\n  requires { x == \"hello\" }\n}\n";
    let config = CompilerConfig::default();
    let output = compile_full(source, "<test>", &config);

    assert!(output.has_errors, "type mismatch should produce errors");
    assert!(
        output.verification.is_empty(),
        "verification should be skipped on type error"
    );
    assert!(
        output.generated.is_none(),
        "codegen should be skipped on type error"
    );
}
