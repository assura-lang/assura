use super::*;
use crate::*;

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
    output.resolved.unwrap();
    assert!(
        output.typed.is_some(),
        "typed was None; diagnostics: {:?}",
        output.diagnostics
    );
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
