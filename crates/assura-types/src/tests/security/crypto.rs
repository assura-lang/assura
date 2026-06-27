use super::*;

// --- T061: Cryptographic conformance tests ---

#[test]
fn crypto_correct_key_size_ok() {
    let checker = CryptoConformanceChecker::new();
    let errors = checker.check_key_size("AES-128-GCM", 128, &(0..1));
    assert!(errors.is_empty());
}

#[test]
fn crypto_wrong_key_size_a17001() {
    let checker = CryptoConformanceChecker::new();
    let errors = checker.check_key_size("AES-128-GCM", 256, &(0..1));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A17001");
}

#[test]
fn crypto_correct_nonce_size_ok() {
    let checker = CryptoConformanceChecker::new();
    let errors = checker.check_nonce_size("AES-256-GCM", 12, &(0..1));
    assert!(errors.is_empty());
}

#[test]
fn crypto_wrong_nonce_size_a17002() {
    let checker = CryptoConformanceChecker::new();
    let errors = checker.check_nonce_size("AES-256-GCM", 16, &(0..1));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A17002");
}

#[test]
fn crypto_nonce_not_unique_a17003() {
    let checker = CryptoConformanceChecker::new();
    let errors = checker.check_nonce_uniqueness("fixed_nonce", false, false, &(0..1));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A17003");
}

#[test]
fn crypto_counter_nonce_ok() {
    let checker = CryptoConformanceChecker::new();
    let errors = checker.check_nonce_uniqueness("counter", true, false, &(0..1));
    assert!(errors.is_empty());
}

#[test]
fn crypto_tag_not_verified_a17004() {
    let checker = CryptoConformanceChecker::new();
    let errors = checker.check_tag_verification(false, &(0..1));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A17004");
}

#[test]
fn crypto_tag_verified_ok() {
    let checker = CryptoConformanceChecker::new();
    let errors = checker.check_tag_verification(true, &(0..1));
    assert!(errors.is_empty());
}

#[test]
fn crypto_chacha20_key_size() {
    let checker = CryptoConformanceChecker::new();
    let errors = checker.check_key_size("ChaCha20-Poly1305", 256, &(0..1));
    assert!(errors.is_empty());
    let errors = checker.check_key_size("ChaCha20-Poly1305", 128, &(0..1));
    assert_eq!(errors.len(), 1);
}

#[test]
fn crypto_custom_spec() {
    let mut checker = CryptoConformanceChecker::new();
    checker.register_spec(CryptoSpec {
        name: "XSalsa20".into(),
        key_size_bits: vec![256],
        block_size_bytes: None,
        nonce_size_bytes: Some(24),
        tag_size_bytes: None,
    });
    let errors = checker.check_nonce_size("XSalsa20", 24, &(0..1));
    assert!(errors.is_empty());
    let errors = checker.check_nonce_size("XSalsa20", 12, &(0..1));
    assert_eq!(errors.len(), 1);
}
