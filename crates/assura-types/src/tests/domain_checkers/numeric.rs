use super::*;

// =======================================================================
// T095: NumericalPrecisionChecker tests
// =======================================================================

#[test]
fn num_precision_loss() {
    let mut np = NumericalPrecisionChecker::new();
    np.declare("x".into(), 64, 1e-15, 0..1);
    let err = np.check_precision_loss("x", 32);
    assert_eq!(err.unwrap().code, "A42001");
}

#[test]
fn num_precision_ok() {
    let mut np = NumericalPrecisionChecker::new();
    np.declare("x".into(), 32, 1e-7, 0..1);
    assert!(np.check_precision_loss("x", 64).is_none());
}

#[test]
fn num_ulp_violation() {
    let mut np = NumericalPrecisionChecker::new();
    np.declare("x".into(), 64, 1e-15, 0..1);
    let err = np.check_ulp_bound("x", 1e-10);
    assert_eq!(err.unwrap().code, "A42002");
}

#[test]
fn num_cancellation() {
    let mut np = NumericalPrecisionChecker::new();
    np.declare("x".into(), 64, 1e-15, 0..1);
    let err = np.check_cancellation("x", 0.9999);
    assert_eq!(err.unwrap().code, "A42003");
}

#[test]
fn num_precision_default() {
    let np = NumericalPrecisionChecker::default();
    assert!(np.check_precision_loss("x", 32).is_none());
}

// =======================================================================
// T096: PrecomputedTableChecker tests
// =======================================================================

#[test]
fn table_incomplete_coverage() {
    let mut tc = PrecomputedTableChecker::new();
    tc.declare_table("crc32".into(), 256, "gen_crc32".into(), 0..1);
    tc.mark_entries_verified("crc32", 100);
    let errs = tc.check_coverage();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code, "A43001");
}

#[test]
fn table_full_coverage_ok() {
    let mut tc = PrecomputedTableChecker::new();
    tc.declare_table("crc32".into(), 256, "gen_crc32".into(), 0..1);
    tc.mark_entries_verified("crc32", 256);
    assert!(tc.check_coverage().is_empty());
}

#[test]
fn table_no_generator() {
    let mut tc = PrecomputedTableChecker::new();
    tc.declare_table("lut".into(), 16, "".into(), 0..1);
    let errs = tc.check_generator();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code, "A43002");
}

#[test]
fn table_zero_size() {
    let mut tc = PrecomputedTableChecker::new();
    tc.declare_table("empty".into(), 0, "gen".into(), 0..1);
    let errs = tc.check_non_empty();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code, "A43003");
}

#[test]
fn table_default() {
    let tc = PrecomputedTableChecker::default();
    assert!(tc.check_non_empty().is_empty());
}

