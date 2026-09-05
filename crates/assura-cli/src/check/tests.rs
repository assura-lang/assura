//! Unit tests for check / SMT Unknown classification.

#[test]
fn success_summary_distinguishes_vacuous_cases() {
    use super::report::success_summary_message;

    assert!(
        success_summary_message(true, false, false, 0)
            .contains("no contracts or functions to verify")
    );
    assert!(success_summary_message(false, true, false, 0).contains("no verifiable clauses"));
    assert!(success_summary_message(false, true, true, 0).contains("no SMT proof obligations"));
    assert_eq!(
        success_summary_message(false, false, false, 0),
        "check passed (no errors)"
    );
    assert_eq!(
        success_summary_message(false, false, false, 2),
        "check passed (2 warnings)"
    );
}

#[test]
fn unknown_classification_known_limitation_is_warning() {
    assert!(assura_smt::is_known_smt_limitation(
        "clause uses features not yet encoded in SMT (method call, deep field chain)"
    ));
}

#[test]
fn unknown_classification_solver_reason_is_error() {
    assert!(!assura_smt::is_known_smt_limitation(
        "non-linear arithmetic"
    ));
    assert!(!assura_smt::is_known_smt_limitation(
        "Z3 not available (compiled without z3-verify feature)"
    ));
    assert!(!assura_smt::is_known_smt_limitation(
        "could not encode clause to SMT-LIB2"
    ));
    assert!(!assura_smt::is_known_smt_limitation(
        "no result from solver"
    ));
}

#[test]
fn unknown_classification_boundary_near_miss() {
    assert!(!assura_smt::is_known_smt_limitation(
        "clause not encoded in SMT yet"
    ));
    assert!(!assura_smt::is_known_smt_limitation(
        "not yet supported in SMT"
    ));
    assert!(!assura_smt::is_known_smt_limitation("features not encoded"));
}

#[test]
fn unknown_classification_diagnostic_output() {
    use super::report::unknown_limitation_diagnostic;
    use assura_diagnostics::Severity;

    let filename = "test.assura";
    let clause_desc = "TestContract: ensures";
    let known = "clause uses features not yet encoded in SMT (method call)";

    let warn = unknown_limitation_diagnostic(filename, clause_desc, known, 0..0, false);
    assert_eq!(warn.code, "A05102");
    assert_eq!(warn.severity, Severity::Warning);
    assert!(
        warn.message.starts_with("verification skipped"),
        "got: {}",
        warn.message
    );

    let incon =
        unknown_limitation_diagnostic(filename, clause_desc, "non-linear arithmetic", 0..0, false);
    assert_eq!(incon.code, "A05103");
    assert_eq!(incon.severity, Severity::Error);
    assert!(
        incon.message.starts_with("verification inconclusive"),
        "got: {}",
        incon.message
    );

    let strict = unknown_limitation_diagnostic(filename, clause_desc, known, 0..0, true);
    assert_eq!(strict.code, "A05102");
    assert_eq!(strict.severity, Severity::Error);
    assert!(
        strict.message.contains("--strict"),
        "strict known limitation should mention --strict, got: {}",
        strict.message
    );
}

#[test]
fn unknown_limitation_unconstrained_result_is_a05102_warning_with_ir_help() {
    use super::report::unknown_limitation_diagnostic;
    use assura_diagnostics::Severity;

    let diag = unknown_limitation_diagnostic(
        "test.assura",
        "SafeDiv::ensures",
        "result is unconstrained (not yet encoded in SMT)",
        0..0,
        false,
    );
    assert_eq!(diag.code, "A05102");
    assert_eq!(diag.severity, Severity::Warning);
    assert!(
        diag.message.starts_with("verification skipped"),
        "expected skipped warning, got: {}",
        diag.message
    );
    let sug = diag
        .suggestion
        .as_ref()
        .expect("unconstrained-result should attach write-IR help");
    let blob = format!("{} {}", sug.message, sug.replacement);
    assert!(
        blob.contains("write-ir") || blob.contains("unconstrained") || blob.contains("IR"),
        "help should mention write-ir / unconstrained / IR, got: {blob}"
    );

    let generic = unknown_limitation_diagnostic(
        "test.assura",
        "Foo::ensures",
        "clause uses features not yet encoded in SMT (method call)",
        0..0,
        false,
    );
    assert_eq!(generic.code, "A05102");
    assert_eq!(generic.severity, Severity::Warning);
    assert!(
        generic.suggestion.is_none(),
        "generic encoder-gap skip should not require IR help"
    );
}

#[test]
fn project_unknown_arm_calls_shared_limitation_helper() {
    // Helper-only tests stay green if project.rs inlines a bare A05102.
    let src = include_str!("project.rs");
    assert!(
        src.contains("unknown_limitation_diagnostic("),
        "project Unknown arm must call unknown_limitation_diagnostic"
    );
}

fn typecheck_unc_contract() -> assura_types::TypedFile {
    let src = "contract Unc { input(x: Int) output(result: Int) requires { x > 0 } ensures { result >= 0 } }";
    let file = assura_parser::parse_unwrap(src);
    let resolved = assura_resolve::resolve(&file).expect("resolve Unc");
    assura_types::type_check(resolved).expect("typecheck Unc")
}

#[test]
fn project_a04008_from_typed_warnings() {
    use assura_diagnostics::Severity;

    let typed = typecheck_unc_contract();
    let diags = super::typed_warnings_to_diags(&typed.warnings, "unc.assura");
    let a04008 = diags
        .iter()
        .find(|d| d.code == "A04008")
        .expect("expected A04008 from unconstrained result");
    assert_eq!(a04008.severity, Severity::Warning);
    assert_eq!(a04008.file, "unc.assura");
    assert_ne!(a04008.primary, 0..0, "A04008 span must not be 0..0");
}

#[test]
fn project_a04008_absent_without_ensures() {
    let src = "contract OnlyReq { input(x: Int) output(result: Int) requires { x > 0 } }";
    let file = assura_parser::parse_unwrap(src);
    let resolved = assura_resolve::resolve(&file).expect("resolve OnlyReq");
    let typed = assura_types::type_check(resolved).expect("typecheck OnlyReq");
    let diags = super::typed_warnings_to_diags(&typed.warnings, "onlyreq.assura");
    assert!(
        diags.is_empty(),
        "requires-only contract must not emit A04008, got: {diags:?}"
    );
}

#[test]
fn project_a04008_suppressed_after_verified_ensures() {
    let typed = typecheck_unc_contract();
    let mut diags = super::typed_warnings_to_diags(&typed.warnings, "unc.assura");
    assert!(
        diags.iter().any(|d| d.code == "A04008"),
        "precondition: A04008 must be present before suppress, got: {diags:?}"
    );
    let results = vec![assura_smt::VerificationResult::Verified {
        clause_desc: "Unc::ensures".into(),
        unsat_core: None,
    }];
    super::suppress_a04008_for_verified_ensures(&mut diags, &results);
    assert!(
        diags.iter().all(|d| d.code != "A04008"),
        "Verified Unc::ensures must drop A04008, got: {diags:?}"
    );
}

#[test]
fn project_smt_diagnostics_use_clause_spans() {
    let src = include_str!("project.rs");
    assert!(
        src.contains("lookup_clause_span"),
        "project.rs must look up declaration spans for SMT diagnostics"
    );
    let a05100 = src.split("\"A05100\"").nth(1).expect("A05100 diagnostic");
    let a05100_span = a05100.split(".with_file").next().unwrap_or(a05100);
    assert!(
        !a05100_span.contains("0..0"),
        "A05100 must use lookup_clause_span, not 0..0: {a05100_span}"
    );
    let a05101 = src.split("\"A05101\"").nth(1).expect("A05101 diagnostic");
    let a05101_span = a05101.split(".with_file").next().unwrap_or(a05101);
    assert!(
        !a05101_span.contains("0..0"),
        "A05101 must use lookup_clause_span, not 0..0: {a05101_span}"
    );
    assert!(
        !src.contains("reason,\n                                    0..0,"),
        "Unknown arm must pass lookup_clause_span, not 0..0"
    );
}
