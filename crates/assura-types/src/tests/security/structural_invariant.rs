use super::*;

// --- T063: Recursive structural invariant tests ---

#[test]
fn struct_inv_tree_balance_valid() {
    let mut checker = StructuralInvariantChecker::new();
    checker.register_recursive_type("AVLTree".into(), vec!["left".into(), "right".into()]);
    let errors = checker.check_invariant_applicability(
        "AVLTree",
        &InvariantKind::TreeBalance { max_diff: 1 },
        &(0..1),
    );
    assert!(errors.is_empty());
}

#[test]
fn struct_inv_on_non_recursive_a15001() {
    let checker = StructuralInvariantChecker::new();
    let errors = checker.check_invariant_applicability(
        "Point",
        &InvariantKind::Sorted { descending: false },
        &(0..1),
    );
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A15001");
}

#[test]
fn struct_inv_tree_on_list_a15002() {
    let mut checker = StructuralInvariantChecker::new();
    checker.register_recursive_type("LinkedList".into(), vec!["next".into()]);
    let errors =
        checker.check_invariant_applicability("LinkedList", &InvariantKind::BstOrdering, &(0..1));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A15002");
}

#[test]
fn struct_inv_sort_on_tree_a15003() {
    let mut checker = StructuralInvariantChecker::new();
    checker.register_recursive_type("BTree".into(), vec!["left".into(), "right".into()]);
    let errors = checker.check_invariant_applicability(
        "BTree",
        &InvariantKind::Sorted { descending: false },
        &(0..1),
    );
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A15003");
}

#[test]
fn struct_inv_acyclic_valid_for_any_recursive() {
    let mut checker = StructuralInvariantChecker::new();
    checker.register_recursive_type("Graph".into(), vec!["children".into()]);
    let errors = checker.check_invariant_applicability("Graph", &InvariantKind::Acyclic, &(0..1));
    assert!(errors.is_empty());
}

#[test]
fn struct_inv_operation_no_proof_a15004() {
    let mut checker = StructuralInvariantChecker::new();
    checker.register_recursive_type("BST".into(), vec!["left".into(), "right".into()]);
    checker.register_invariant(StructuralInvariant {
        name: "bst_order".into(),
        type_name: "BST".into(),
        kind: InvariantKind::BstOrdering,
    });
    let errors = checker.check_operation_preserves(
        "BST",
        "insert",
        true,  // modifies structure
        false, // no preservation proof
        &(0..1),
    );
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A15004");
}

#[test]
fn struct_inv_operation_with_proof_ok() {
    let mut checker = StructuralInvariantChecker::new();
    checker.register_recursive_type("BST".into(), vec!["left".into(), "right".into()]);
    checker.register_invariant(StructuralInvariant {
        name: "bst_order".into(),
        type_name: "BST".into(),
        kind: InvariantKind::BstOrdering,
    });
    let errors = checker.check_operation_preserves(
        "BST",
        "insert",
        true, // modifies structure
        true, // has preservation proof
        &(0..1),
    );
    assert!(errors.is_empty());
}

#[test]
fn struct_inv_readonly_trivially_preserves() {
    let mut checker = StructuralInvariantChecker::new();
    checker.register_recursive_type("BST".into(), vec!["left".into(), "right".into()]);
    checker.register_invariant(StructuralInvariant {
        name: "bst_order".into(),
        type_name: "BST".into(),
        kind: InvariantKind::BstOrdering,
    });
    let errors = checker.check_operation_preserves(
        "BST",
        "search",
        false, // read-only
        false, // no proof needed
        &(0..1),
    );
    assert!(errors.is_empty(), "read-only ops preserve invariants");
}

#[test]
fn struct_inv_kind_display() {
    assert_eq!(
        InvariantKind::TreeBalance { max_diff: 1 }.to_string(),
        "tree_balance(max_diff=1)"
    );
    assert_eq!(
        InvariantKind::Sorted { descending: false }.to_string(),
        "sorted(asc)"
    );
    assert_eq!(InvariantKind::Acyclic.to_string(), "acyclic");
    assert_eq!(InvariantKind::BstOrdering.to_string(), "bst_ordering");
    assert_eq!(
        InvariantKind::HeapProperty { min_heap: true }.to_string(),
        "min_heap"
    );
}

#[test]
fn struct_inv_get_invariants() {
    let mut checker = StructuralInvariantChecker::new();
    checker.register_recursive_type("AVL".into(), vec!["left".into(), "right".into()]);
    checker.register_invariant(StructuralInvariant {
        name: "balance".into(),
        type_name: "AVL".into(),
        kind: InvariantKind::TreeBalance { max_diff: 1 },
    });
    checker.register_invariant(StructuralInvariant {
        name: "order".into(),
        type_name: "AVL".into(),
        kind: InvariantKind::BstOrdering,
    });
    assert_eq!(checker.get_invariants("AVL").len(), 2);
    assert!(checker.get_invariants("Unknown").is_empty());
}

