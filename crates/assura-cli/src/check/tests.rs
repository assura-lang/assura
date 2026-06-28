//! Unit tests for check / SMT Unknown classification.

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
    let filename = "test.assura";
    let clause_desc = "TestContract: ensures";

    // Warning path: known limitation -> A05102
    let reason = "clause uses features not yet encoded in SMT (method call)";
    let mut has_errors = false;
    let diag = if assura_smt::is_known_smt_limitation(reason) {
        assura_diagnostics::Diagnostic::warning(
            "A05102",
            format!("verification skipped for {clause_desc}: {reason}"),
            0..0,
        )
        .with_file(filename)
    } else {
        has_errors = true;
        assura_diagnostics::Diagnostic::error(
            "A05103",
            format!("verification inconclusive for {clause_desc}: {reason}"),
            0..0,
        )
        .with_file(filename)
    };
    assert!(!has_errors, "known limitation should not set has_errors");
    assert!(diag.message.starts_with("verification skipped"));
    assert_eq!(diag.code, "A05102", "known limitation should use A05102");

    // Error path: solver inconclusive -> A05103
    let reason2 = "non-linear arithmetic";
    let mut has_errors2 = false;
    let diag2 = if assura_smt::is_known_smt_limitation(reason2) {
        assura_diagnostics::Diagnostic::warning(
            "A05102",
            format!("verification skipped for {clause_desc}: {reason2}"),
            0..0,
        )
        .with_file(filename)
    } else {
        has_errors2 = true;
        assura_diagnostics::Diagnostic::error(
            "A05103",
            format!("verification inconclusive for {clause_desc}: {reason2}"),
            0..0,
        )
        .with_file(filename)
    };
    assert!(has_errors2, "solver inconclusive should set has_errors");
    assert!(diag2.message.starts_with("verification inconclusive"));
    assert_eq!(diag2.code, "A05103", "solver inconclusive should use A05103");
}
