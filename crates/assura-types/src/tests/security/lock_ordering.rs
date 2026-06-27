use super::*;

// --- T068: Lock ordering tests ---

#[test]
fn lock_order_correct_ok() {
    let mut checker = LockOrderChecker::new();
    checker.define_order("db".into(), 1);
    checker.define_order("cache".into(), 2);
    let errors = checker.acquire("db", &(0..1));
    assert!(errors.is_empty());
    let errors = checker.acquire("cache", &(0..1));
    assert!(errors.is_empty());
}

#[test]
fn lock_order_violation_a21001() {
    let mut checker = LockOrderChecker::new();
    checker.define_order("db".into(), 1);
    checker.define_order("cache".into(), 2);
    let errors = checker.acquire("cache", &(0..1));
    assert!(errors.is_empty());
    let errors = checker.acquire("db", &(0..1)); // wrong order
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A21001");
}

#[test]
fn lock_order_release_correct() {
    let mut checker = LockOrderChecker::new();
    checker.define_order("a".into(), 1);
    checker.define_order("b".into(), 2);
    checker.acquire("a", &(0..1));
    checker.acquire("b", &(0..1));
    let errors = checker.release("b", &(0..1)); // correct: LIFO
    assert!(errors.is_empty());
}

#[test]
fn lock_order_release_wrong_a21002() {
    let mut checker = LockOrderChecker::new();
    checker.define_order("a".into(), 1);
    checker.define_order("b".into(), 2);
    checker.acquire("a", &(0..1));
    checker.acquire("b", &(0..1));
    let errors = checker.release("a", &(0..1)); // wrong: b still held
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A21002");
}

#[test]
fn lock_order_undefined_a21003() {
    let checker = LockOrderChecker::new();
    let errors = checker.check_ordering_defined("unknown_lock", &(0..1));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A21003");
}

#[test]
fn lock_order_defined_ok() {
    let mut checker = LockOrderChecker::new();
    checker.define_order("db".into(), 1);
    let errors = checker.check_ordering_defined("db", &(0..1));
    assert!(errors.is_empty());
}
