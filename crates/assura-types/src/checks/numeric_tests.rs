use super::*;

fn parse_source(src: &str) -> assura_parser::ast::SourceFile {
    let (sf, errs) = assura_parser::parse(src);
    assert!(errs.is_empty(), "parse errors: {errs:?}");
    sf.unwrap()
}

// -----------------------------------------------------------------------
// run_numerical_precision_checks
// -----------------------------------------------------------------------

#[test]
fn numerical_precision_no_annotation_produces_no_errors() {
    let src = r#"contract Simple { requires { true } }"#;
    let sf = parse_source(src);
    let errors = run_numerical_precision_checks(&sf);
    assert!(
        errors.is_empty(),
        "no precision annotation should produce no errors: {errors:?}"
    );
}

#[test]
fn numerical_precision_cancellation_detected() {
    // `precision x` declares a tracked variable; `ensures { x > 0 }`
    // references it, triggering the catastrophic cancellation check.
    let src = r#"contract Compute { precision x ensures { x > 0 } }"#;
    let sf = parse_source(src);
    let errors = run_numerical_precision_checks(&sf);
    assert!(
        errors.iter().any(|e| e.code == "A42003"),
        "expected A42003 for catastrophic cancellation, got: {errors:?}"
    );
}

// -----------------------------------------------------------------------
// run_precomputed_table_checks
// -----------------------------------------------------------------------

#[test]
fn precomputed_table_no_annotation_produces_no_errors() {
    let src = r#"contract Simple { requires { true } }"#;
    let sf = parse_source(src);
    let errors = run_precomputed_table_checks(&sf);
    assert!(
        errors.is_empty(),
        "no precomputed_table annotation should produce no errors: {errors:?}"
    );
}

#[test]
fn precomputed_table_no_generator_detected() {
    // `precomputed_table crc_table` declares a table with no generator
    // function, which should trigger A43002.
    let src = r#"contract Lookup { precomputed_table crc_table }"#;
    let sf = parse_source(src);
    let errors = run_precomputed_table_checks(&sf);
    assert!(
        errors.iter().any(|e| e.code == "A43002"),
        "expected A43002 for table without generator function, got: {errors:?}"
    );
}

#[test]
fn precomputed_table_also_flags_coverage() {
    // A bare `precomputed_table name` also gets default size (256) with
    // 0 verified entries, so A43001 (incomplete coverage) is expected too.
    let src = r#"contract Lookup { precomputed_table crc_table }"#;
    let sf = parse_source(src);
    let errors = run_precomputed_table_checks(&sf);
    assert!(
        errors.iter().any(|e| e.code == "A43001"),
        "expected A43001 for incomplete table coverage, got: {errors:?}"
    );
}

// -----------------------------------------------------------------------
// run_collection_contract_checks
// -----------------------------------------------------------------------

#[test]
fn collection_no_known_operation_produces_no_errors() {
    let src = r#"contract Unrelated { requires { true } ensures { true } }"#;
    let sf = parse_source(src);
    let errors = run_collection_contract_checks(&sf);
    assert!(
        errors.is_empty(),
        "non-collection contract should produce no errors: {errors:?}"
    );
}

#[test]
fn collection_sort_without_len_postcondition_detected() {
    // A contract named `sort` (length-preserving op) without an ensures
    // clause mentioning `len` should produce A03007.
    let src = r#"
        contract Sort {
            requires { true }
            ensures { true }
        }
    "#;
    let sf = parse_source(src);
    let errors = run_collection_contract_checks(&sf);
    assert!(
        errors.iter().any(|e| e.code == "A03007"),
        "sort without len postcondition should produce A03007: {errors:?}"
    );
}

#[test]
fn collection_sort_with_len_postcondition_no_error() {
    // A contract named `sort` WITH an ensures clause mentioning `len`
    // should not produce A03007.
    let src = r#"
        contract Sort {
            input(items: List<Int>)
            ensures { len(items) == len(items) }
        }
    "#;
    let sf = parse_source(src);
    let errors = run_collection_contract_checks(&sf);
    assert!(
        !errors.iter().any(|e| e.code == "A03007"),
        "sort with len postcondition should not produce A03007: {errors:?}"
    );
}

// -----------------------------------------------------------------------
// run_fixed_width_checks
// -----------------------------------------------------------------------

#[test]
fn fixed_width_no_fw_params_produces_no_errors() {
    let src = r#"contract Simple { requires { true } }"#;
    let sf = parse_source(src);
    let env = TypeEnv::new();
    let errors = run_fixed_width_checks(&sf, &env);
    assert!(
        errors.is_empty(),
        "contract without fixed-width params should produce no errors: {errors:?}"
    );
}

#[test]
fn fixed_width_overflow_on_u8_addition() {
    // An extern fn with two U8 params and an ensures clause adding them
    // should detect potential overflow (A10101).
    let src = r#"
        extern fn add_bytes(a: U8, b: U8) -> U8
            ensures { a + b > 0 }
    "#;
    let sf = parse_source(src);
    let env = TypeEnv::new();
    let errors = run_fixed_width_checks(&sf, &env);
    assert!(
        errors.iter().any(|e| e.code == "A10101"),
        "U8 + U8 should flag potential overflow A10101: {errors:?}"
    );
}

// -----------------------------------------------------------------------
// check_domain_size (A43005)
// -----------------------------------------------------------------------

#[test]
fn precomputed_table_domain_size_nonstandard_flagged() {
    // A table with a non-standard size (100) should produce A43005.
    let mut checker = PrecomputedTableChecker::new();
    checker.declare_table("odd_table".into(), 100, "gen_fn".into(), 0..10);
    let errors = checker.check_domain_size(None);
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A43005");
    assert!(errors[0].message.contains("100"));
}

#[test]
fn precomputed_table_domain_size_standard_ok() {
    // Standard sizes (256, 16, 128, 65536) should not trigger A43005.
    for size in [16, 128, 256, 65536] {
        let mut checker = PrecomputedTableChecker::new();
        checker.declare_table("std_table".into(), size, "gen_fn".into(), 0..10);
        let errors = checker.check_domain_size(None);
        assert!(
            errors.is_empty(),
            "standard size {size} should not trigger A43005: {errors:?}"
        );
    }
}

#[test]
fn precomputed_table_domain_size_explicit_mismatch() {
    // Explicit expected domain size (256) vs actual (100) should flag.
    let mut checker = PrecomputedTableChecker::new();
    checker.declare_table("crc_table".into(), 100, "compute_crc".into(), 0..10);
    let errors = checker.check_domain_size(Some(256));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A43005");
    assert!(errors[0].message.contains("256"));
}

#[test]
fn precomputed_table_domain_size_explicit_match() {
    // When actual matches expected, no error.
    let mut checker = PrecomputedTableChecker::new();
    checker.declare_table("byte_table".into(), 256, "gen".into(), 0..10);
    let errors = checker.check_domain_size(Some(256));
    assert!(errors.is_empty());
}

// -----------------------------------------------------------------------
// smt_obligations (table verification obligations)
// -----------------------------------------------------------------------

#[test]
fn smt_obligations_empty_for_no_generator() {
    let mut checker = PrecomputedTableChecker::new();
    checker.declare_table("bare_table".into(), 256, String::new(), 0..10);
    let obs = checker.smt_obligations();
    assert!(
        obs.is_empty(),
        "table without generator should have no SMT obligation"
    );
}

#[test]
fn smt_obligations_returns_obligation_with_generator() {
    let mut checker = PrecomputedTableChecker::new();
    checker.declare_table("crc32_table".into(), 256, "compute_crc32".into(), 0..10);
    let obs = checker.smt_obligations();
    assert_eq!(obs.len(), 1);
    assert_eq!(obs[0].table_name, "crc32_table");
    assert_eq!(obs[0].generator_fn, "compute_crc32");
    assert_eq!(obs[0].domain_size, 256);
}

#[test]
fn smt_obligations_skips_zero_size_tables() {
    let mut checker = PrecomputedTableChecker::new();
    checker.declare_table("empty".into(), 0, "gen".into(), 0..5);
    let obs = checker.smt_obligations();
    assert!(
        obs.is_empty(),
        "zero-size table should produce no obligation"
    );
}

// -----------------------------------------------------------------------
// collect_table_smt_obligations (source-level integration)
// -----------------------------------------------------------------------

#[test]
fn collect_table_smt_obligations_no_tables() {
    let src = r#"contract Simple { requires { true } }"#;
    let sf = parse_source(src);
    let obs = collect_table_smt_obligations(&sf);
    assert!(obs.is_empty());
}

#[test]
fn collect_table_smt_obligations_with_bare_table() {
    // A bare `precomputed_table name` has no generator function,
    // so it should produce no SMT obligations.
    let src = r#"contract Lookup { precomputed_table crc_table }"#;
    let sf = parse_source(src);
    let obs = collect_table_smt_obligations(&sf);
    assert!(
        obs.is_empty(),
        "bare table without generator should have no SMT obligation"
    );
}

// -----------------------------------------------------------------------
// run_precomputed_table_checks now includes domain_size (A43005)
// -----------------------------------------------------------------------

#[test]
fn precomputed_table_checks_include_domain_size_error() {
    // A bare `precomputed_table name` gets default size (256) which IS
    // a standard domain size, so no A43005. But it still gets A43001
    // (coverage) and A43002 (no generator).
    let src = r#"contract Lookup { precomputed_table crc_table }"#;
    let sf = parse_source(src);
    let errors = run_precomputed_table_checks(&sf);
    // A43001 (coverage) + A43002 (no generator) expected, but NOT A43005
    // because 256 is a standard domain size.
    assert!(errors.iter().any(|e| e.code == "A43001"));
    assert!(errors.iter().any(|e| e.code == "A43002"));
    assert!(
        !errors.iter().any(|e| e.code == "A43005"),
        "256 is standard, should not get A43005"
    );
}
