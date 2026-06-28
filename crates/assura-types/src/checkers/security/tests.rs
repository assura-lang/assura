use super::*;

use assura_parser::ast::Spanned;

fn span() -> Range<usize> {
    0..10
}

fn ident(s: &str) -> SpExpr {
    Spanned::no_span(Expr::Ident(s.to_string()))
}

fn int_lit(n: i64) -> SpExpr {
    Spanned::no_span(Expr::Literal(Literal::Int(n.to_string())))
}

// ---- ConstantTimeChecker ----

#[test]
fn ct_no_secret_no_error() {
    let checker = ConstantTimeChecker::new();
    let errs = checker.check_branch(&ident("x"), &span());
    assert!(errs.is_empty());
}

#[test]
fn ct_branch_on_secret() {
    let mut checker = ConstantTimeChecker::new();
    checker.mark_secret("key".into());
    let errs = checker.check_branch(&ident("key"), &span());
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code.as_ref(), "A14001");
}

#[test]
fn ct_index_on_secret() {
    let mut checker = ConstantTimeChecker::new();
    checker.mark_secret("secret_idx".into());
    let errs = checker.check_index(&ident("secret_idx"), &span());
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code.as_ref(), "A14002");
}

#[test]
fn ct_check_expr_if_with_secret_condition() {
    let mut checker = ConstantTimeChecker::new();
    checker.mark_secret("s".into());
    let expr = Spanned::no_span(Expr::If {
        cond: Box::new(ident("s")),
        then_branch: Box::new(int_lit(1)),
        else_branch: Some(Box::new(int_lit(0))),
    });
    let errs = checker.check_expr(&expr, &span());
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code.as_ref(), "A14001");
}

#[test]
fn ct_references_secret_in_binop() {
    let mut checker = ConstantTimeChecker::new();
    checker.mark_secret("pw".into());
    let expr = Spanned::no_span(Expr::BinOp {
        lhs: Box::new(ident("pw")),
        op: BinOp::Add,
        rhs: Box::new(int_lit(1)),
    });
    assert!(checker.references_secret(&expr));
}

#[test]
fn ct_no_secret_reference() {
    let checker = ConstantTimeChecker::new();
    assert!(!checker.references_secret(&ident("x")));
}

// ---- StructuralInvariantChecker ----

#[test]
fn si_invariant_on_non_recursive_type() {
    let checker = StructuralInvariantChecker::new();
    let errs = checker.check_invariant_applicability("Flat", &InvariantKind::Acyclic, &span());
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code.as_ref(), "A15001");
}

#[test]
fn si_tree_invariant_needs_two_fields() {
    let mut checker = StructuralInvariantChecker::new();
    checker.register_recursive_type("LinkedList".into(), vec!["next".into()]);
    let errs = checker.check_invariant_applicability(
        "LinkedList",
        &InvariantKind::TreeBalance { max_diff: 1 },
        &span(),
    );
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code.as_ref(), "A15002");
}

#[test]
fn si_tree_invariant_ok_with_two_fields() {
    let mut checker = StructuralInvariantChecker::new();
    checker.register_recursive_type("Tree".into(), vec!["left".into(), "right".into()]);
    let errs = checker.check_invariant_applicability("Tree", &InvariantKind::BstOrdering, &span());
    assert!(errs.is_empty());
}

#[test]
fn si_sort_invariant_needs_one_field() {
    let mut checker = StructuralInvariantChecker::new();
    checker.register_recursive_type("Tree".into(), vec!["left".into(), "right".into()]);
    let errs = checker.check_invariant_applicability(
        "Tree",
        &InvariantKind::Sorted { descending: false },
        &span(),
    );
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code.as_ref(), "A15003");
}

#[test]
fn si_operation_preserves_without_proof() {
    let mut checker = StructuralInvariantChecker::new();
    checker.register_recursive_type("Tree".into(), vec!["left".into(), "right".into()]);
    checker.register_invariant(StructuralInvariant {
        name: "balanced".into(),
        type_name: "Tree".into(),
        kind: InvariantKind::TreeBalance { max_diff: 1 },
    });
    let errs = checker.check_operation_preserves("Tree", "insert", true, false, &span());
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code.as_ref(), "A15004");
}

#[test]
fn si_readonly_operation_no_error() {
    let mut checker = StructuralInvariantChecker::new();
    checker.register_recursive_type("Tree".into(), vec!["left".into(), "right".into()]);
    checker.register_invariant(StructuralInvariant {
        name: "balanced".into(),
        type_name: "Tree".into(),
        kind: InvariantKind::TreeBalance { max_diff: 1 },
    });
    let errs = checker.check_operation_preserves("Tree", "lookup", false, false, &span());
    assert!(errs.is_empty());
}

// ---- SharedMemChecker ----

#[test]
fn sm_read_without_access() {
    let checker = SharedMemChecker::new();
    let errs = checker.check_read("buf", &span());
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code.as_ref(), "A18001");
}

#[test]
fn sm_read_with_shared_read() {
    let mut checker = SharedMemChecker::new();
    checker.set_mode("buf".into(), AccessMode::SharedRead);
    let errs = checker.check_read("buf", &span());
    assert!(errs.is_empty());
}

#[test]
fn sm_write_without_exclusive() {
    let mut checker = SharedMemChecker::new();
    checker.set_mode("buf".into(), AccessMode::SharedRead);
    let errs = checker.check_write("buf", &span());
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code.as_ref(), "A18002");
}

#[test]
fn sm_write_with_exclusive() {
    let mut checker = SharedMemChecker::new();
    checker.set_mode("buf".into(), AccessMode::Exclusive);
    let errs = checker.check_write("buf", &span());
    assert!(errs.is_empty());
}

#[test]
fn sm_data_race_exclusive_exclusive() {
    let checker = SharedMemChecker::new();
    let errs = checker.check_data_race(
        "shared",
        AccessMode::Exclusive,
        AccessMode::Exclusive,
        &span(),
    );
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code.as_ref(), "A18003");
}

#[test]
fn sm_no_race_shared_read() {
    let checker = SharedMemChecker::new();
    let errs = checker.check_data_race(
        "shared",
        AccessMode::SharedRead,
        AccessMode::SharedRead,
        &span(),
    );
    assert!(errs.is_empty());
}

// ---- DeterminismChecker ----

#[test]
fn det_non_deterministic_sources() {
    let checker = DeterminismChecker::new();
    assert!(checker.is_non_deterministic("HashMap"));
    assert!(checker.is_non_deterministic("HashSet"));
    assert!(checker.is_non_deterministic("random"));
    assert!(!checker.is_non_deterministic("BTreeMap"));
}

#[test]
fn det_fn_body_with_hash_map() {
    let mut checker = DeterminismChecker::new();
    checker.mark_deterministic("pure_fn".into());
    let errs = checker.check_fn_body("pure_fn", &["HashMap".into(), "BTreeMap".into()], &span());
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code.as_ref(), "A20001");
}

#[test]
fn det_unmarked_fn_no_error() {
    let checker = DeterminismChecker::new();
    let errs = checker.check_fn_body("any_fn", &["HashMap".into()], &span());
    assert!(errs.is_empty());
}

#[test]
fn det_iteration_over_hashset() {
    let mut checker = DeterminismChecker::new();
    checker.mark_deterministic("pure_fn".into());
    let errs = checker.check_iteration("pure_fn", "HashSet<i32>", &span());
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code.as_ref(), "A20002");
}

#[test]
fn det_custom_non_det_source() {
    let mut checker = DeterminismChecker::new();
    checker.add_non_det_source("getrandom".into());
    assert!(checker.is_non_deterministic("getrandom"));
}

// ---- LockOrderChecker ----

#[test]
fn lock_acquire_in_order() {
    let mut checker = LockOrderChecker::new();
    checker.define_order("A".into(), 1);
    checker.define_order("B".into(), 2);
    let errs = checker.acquire("A", &span());
    assert!(errs.is_empty());
    let errs = checker.acquire("B", &span());
    assert!(errs.is_empty());
}

#[test]
fn lock_acquire_out_of_order() {
    let mut checker = LockOrderChecker::new();
    checker.define_order("A".into(), 1);
    checker.define_order("B".into(), 2);
    let errs = checker.acquire("B", &span());
    assert!(errs.is_empty());
    let errs = checker.acquire("A", &span());
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code.as_ref(), "A21001");
}

#[test]
fn lock_release_out_of_order() {
    let mut checker = LockOrderChecker::new();
    checker.define_order("A".into(), 1);
    checker.define_order("B".into(), 2);
    let _ = checker.acquire("A", &span());
    let _ = checker.acquire("B", &span());
    let errs = checker.release("A", &span());
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code.as_ref(), "A21002");
}

#[test]
fn lock_release_correct_order() {
    let mut checker = LockOrderChecker::new();
    checker.define_order("A".into(), 1);
    checker.define_order("B".into(), 2);
    let _ = checker.acquire("A", &span());
    let _ = checker.acquire("B", &span());
    let errs = checker.release("B", &span());
    assert!(errs.is_empty());
}

#[test]
fn lock_ordering_undefined() {
    let checker = LockOrderChecker::new();
    let errs = checker.check_ordering_defined("unknown_lock", &span());
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code.as_ref(), "A21003");
}

// ---- SecureErasureChecker ----

#[test]
fn se_sensitive_not_zeroized() {
    let mut checker = SecureErasureChecker::new();
    checker.mark_sensitive("key".into());
    let errs = checker.check_scope_exit("key", &span());
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code.as_ref(), "A16001");
}

#[test]
fn se_sensitive_zeroized() {
    let mut checker = SecureErasureChecker::new();
    checker.mark_sensitive("key".into());
    checker.mark_zeroized("key".into());
    let errs = checker.check_scope_exit("key", &span());
    assert!(errs.is_empty());
}

#[test]
fn se_copy_to_non_sensitive() {
    let mut checker = SecureErasureChecker::new();
    checker.mark_sensitive("key".into());
    let errs = checker.check_copy("key", "buf", false, &span());
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code.as_ref(), "A16002");
}

#[test]
fn se_copy_to_sensitive_ok() {
    let mut checker = SecureErasureChecker::new();
    checker.mark_sensitive("key".into());
    let errs = checker.check_copy("key", "key_copy", true, &span());
    assert!(errs.is_empty());
}

#[test]
fn se_return_without_annotation() {
    let mut checker = SecureErasureChecker::new();
    checker.mark_sensitive("key".into());
    let errs = checker.check_return("key", false, &span());
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code.as_ref(), "A16003");
}

#[test]
fn se_check_all_erased() {
    let mut checker = SecureErasureChecker::new();
    checker.mark_sensitive("a".into());
    checker.mark_sensitive("b".into());
    checker.mark_zeroized("a".into());
    let errs = checker.check_all_erased(&span());
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code.as_ref(), "A16001");
    assert!(
        errs[0].message.contains("`b`") || errs[0].message.contains("'b'"),
        "error should name the un-erased variable `b`, got: {}",
        errs[0].message
    );
}

#[test]
fn se_sensitive_names() {
    let mut checker = SecureErasureChecker::new();
    checker.mark_sensitive("x".into());
    checker.mark_sensitive("y".into());
    let names = checker.sensitive_names();
    assert_eq!(names.len(), 2);
}

// ---- CryptoConformanceChecker ----

#[test]
fn crypto_correct_key_size() {
    let checker = CryptoConformanceChecker::new();
    let errs = checker.check_key_size("AES-128-GCM", 128, &span());
    assert!(errs.is_empty());
}

#[test]
fn crypto_wrong_key_size() {
    let checker = CryptoConformanceChecker::new();
    let errs = checker.check_key_size("AES-128-GCM", 256, &span());
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code.as_ref(), "A17001");
}

#[test]
fn crypto_correct_nonce_size() {
    let checker = CryptoConformanceChecker::new();
    let errs = checker.check_nonce_size("AES-256-GCM", 12, &span());
    assert!(errs.is_empty());
}

#[test]
fn crypto_wrong_nonce_size() {
    let checker = CryptoConformanceChecker::new();
    let errs = checker.check_nonce_size("AES-256-GCM", 16, &span());
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code.as_ref(), "A17002");
}

#[test]
fn crypto_nonce_not_unique() {
    let checker = CryptoConformanceChecker::new();
    let errs = checker.check_nonce_uniqueness("static_nonce", false, false, &span());
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code.as_ref(), "A17003");
}

#[test]
fn crypto_nonce_counter_ok() {
    let checker = CryptoConformanceChecker::new();
    let errs = checker.check_nonce_uniqueness("counter", true, false, &span());
    assert!(errs.is_empty());
}

#[test]
fn crypto_tag_not_verified() {
    let checker = CryptoConformanceChecker::new();
    let errs = checker.check_tag_verification(false, &span());
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code.as_ref(), "A17004");
}

#[test]
fn crypto_tag_verified_ok() {
    let checker = CryptoConformanceChecker::new();
    let errs = checker.check_tag_verification(true, &span());
    assert!(errs.is_empty());
}

#[test]
fn crypto_custom_spec() {
    let mut checker = CryptoConformanceChecker::new();
    checker.register_spec(CryptoSpec {
        name: "MyAlgo".into(),
        key_size_bits: vec![512],
        block_size_bytes: Some(64),
        nonce_size_bytes: Some(24),
        tag_size_bytes: Some(32),
    });
    let errs = checker.check_key_size("MyAlgo", 512, &span());
    assert!(errs.is_empty());
    let errs = checker.check_key_size("MyAlgo", 256, &span());
    assert_eq!(errs.len(), 1);
}

#[test]
fn crypto_unknown_algorithm_no_error() {
    let checker = CryptoConformanceChecker::new();
    let errs = checker.check_key_size("UnknownAlgo", 42, &span());
    assert!(errs.is_empty());
}
