use super::*;

// --- T060: Secure erasure tests ---

#[test]
fn secure_erasure_not_zeroized_a16001() {
    let mut checker = SecureErasureChecker::new();
    checker.mark_sensitive("private_key".into());
    let errors = checker.check_scope_exit("private_key", &(0..1));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A16001");
}

#[test]
fn secure_erasure_zeroized_ok() {
    let mut checker = SecureErasureChecker::new();
    checker.mark_sensitive("private_key".into());
    checker.mark_zeroized("private_key".into());
    let errors = checker.check_scope_exit("private_key", &(0..1));
    assert!(errors.is_empty());
}

#[test]
fn secure_erasure_non_sensitive_ok() {
    let checker = SecureErasureChecker::new();
    let errors = checker.check_scope_exit("public_data", &(0..1));
    assert!(errors.is_empty());
}

#[test]
fn secure_erasure_copy_to_non_sensitive_a16002() {
    let mut checker = SecureErasureChecker::new();
    checker.mark_sensitive("key".into());
    let errors = checker.check_copy("key", "backup", false, &(0..1));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A16002");
}

#[test]
fn secure_erasure_copy_to_sensitive_ok() {
    let mut checker = SecureErasureChecker::new();
    checker.mark_sensitive("key".into());
    let errors = checker.check_copy("key", "key_copy", true, &(0..1));
    assert!(errors.is_empty());
}

#[test]
fn secure_erasure_return_not_sensitive_a16003() {
    let mut checker = SecureErasureChecker::new();
    checker.mark_sensitive("derived_key".into());
    let errors = checker.check_return("derived_key", false, &(0..1));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A16003");
}

#[test]
fn secure_erasure_check_all_erased() {
    let mut checker = SecureErasureChecker::new();
    checker.mark_sensitive("key1".into());
    checker.mark_sensitive("key2".into());
    checker.mark_zeroized("key1".into());
    let errors = checker.check_all_erased(&(0..1));
    assert_eq!(errors.len(), 1); // key2 not zeroized
}
