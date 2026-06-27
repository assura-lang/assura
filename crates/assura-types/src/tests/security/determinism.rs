use super::*;

// --- T067: Determinism checker tests ---

#[test]
fn determinism_hashmap_a20001() {
    let mut checker = DeterminismChecker::new();
    checker.mark_deterministic("compute".into());
    let errors = checker.check_fn_body("compute", &["HashMap".into(), "Vec".into()], &(0..1));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A20001");
}

#[test]
fn determinism_btreemap_ok() {
    let mut checker = DeterminismChecker::new();
    checker.mark_deterministic("compute".into());
    let errors = checker.check_fn_body("compute", &["BTreeMap".into(), "Vec".into()], &(0..1));
    assert!(errors.is_empty());
}

#[test]
fn determinism_non_det_fn_ok() {
    let checker = DeterminismChecker::new();
    // Not marked deterministic
    let errors = checker.check_fn_body("random_pick", &["random".into()], &(0..1));
    assert!(errors.is_empty(), "non-deterministic fn allows random");
}

#[test]
fn determinism_iteration_a20002() {
    let mut checker = DeterminismChecker::new();
    checker.mark_deterministic("process".into());
    let errors = checker.check_iteration("process", "HashMap<K,V>", &(0..1));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A20002");
}

#[test]
fn determinism_btree_iteration_ok() {
    let mut checker = DeterminismChecker::new();
    checker.mark_deterministic("process".into());
    let errors = checker.check_iteration("process", "BTreeMap<K,V>", &(0..1));
    assert!(errors.is_empty());
}

#[test]
fn determinism_random_a20001() {
    let mut checker = DeterminismChecker::new();
    checker.mark_deterministic("seed_fn".into());
    let errors = checker.check_fn_body("seed_fn", &["thread_rng".into()], &(0..1));
    assert_eq!(errors.len(), 1);
}

