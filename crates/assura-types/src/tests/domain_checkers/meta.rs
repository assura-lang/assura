use super::*;

// =======================================================================
// T077: AxiomaticDefChecker tests
// =======================================================================

#[test]
fn axiom_undefined_reference() {
    let mut checker = AxiomaticDefChecker::new();
    checker.declare_axiom(AxiomDef {
        name: "ax1".into(),
        span: 0..1,
        references: vec!["foo".into()],
    });
    let errors = checker.check_references(&[]);
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A31001");
}

#[test]
fn axiom_known_reference_ok() {
    let mut checker = AxiomaticDefChecker::new();
    checker.declare_axiom(AxiomDef {
        name: "ax1".into(),
        span: 0..1,
        references: vec!["foo".into()],
    });
    assert!(checker.check_references(&["foo"]).is_empty());
}

#[test]
fn axiom_unused() {
    let mut checker = AxiomaticDefChecker::new();
    checker.declare_axiom(AxiomDef {
        name: "unused_ax".into(),
        span: 0..1,
        references: vec![],
    });
    let errors = checker.check_unused();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A31003");
}

#[test]
fn axiom_used_ok() {
    let mut checker = AxiomaticDefChecker::new();
    checker.declare_axiom(AxiomDef {
        name: "ax1".into(),
        span: 0..1,
        references: vec![],
    });
    checker.mark_used("ax1");
    assert!(checker.check_unused().is_empty());
}

#[test]
fn axiom_circular() {
    let mut checker = AxiomaticDefChecker::new();
    checker.declare_axiom(AxiomDef {
        name: "a".into(),
        span: 0..1,
        references: vec!["b".into()],
    });
    checker.declare_axiom(AxiomDef {
        name: "b".into(),
        span: 0..1,
        references: vec!["a".into()],
    });
    let errors = checker.check_circular();
    assert!(!errors.is_empty());
    assert!(errors.iter().any(|e| e.code == "A31002"));
}

#[test]
fn axiom_default() {
    let checker = AxiomaticDefChecker::default();
    assert!(checker.check_unused().is_empty());
}

// =======================================================================
// T079: OpaqueFunctionChecker tests
// =======================================================================

#[test]
fn opaque_call_without_contract() {
    let mut checker = OpaqueFunctionChecker::new();
    checker.declare_opaque("secret_fn".into(), false, 0..1);
    let err = checker.check_call("secret_fn", &(5..6));
    assert_eq!(err.unwrap().code, "A32001");
}

#[test]
fn opaque_call_with_contract_ok() {
    let mut checker = OpaqueFunctionChecker::new();
    checker.declare_opaque("secret_fn".into(), true, 0..1);
    assert!(checker.check_call("secret_fn", &(5..6)).is_none());
}

#[test]
fn opaque_body_access_without_reveal() {
    let mut checker = OpaqueFunctionChecker::new();
    checker.declare_opaque("hidden".into(), true, 0..1);
    let err = checker.check_body_access("hidden", &(5..6));
    assert_eq!(err.unwrap().code, "A32002");
}

#[test]
fn opaque_reveal_outside_proof() {
    let mut checker = OpaqueFunctionChecker::new();
    checker.declare_opaque("hidden".into(), true, 0..1);
    let err = checker.reveal("hidden", &(5..6));
    assert_eq!(err.unwrap().code, "A32003");
}

#[test]
fn opaque_reveal_in_proof_ok() {
    let mut checker = OpaqueFunctionChecker::new();
    checker.declare_opaque("hidden".into(), true, 0..1);
    checker.enter_proof();
    assert!(checker.reveal("hidden", &(5..6)).is_none());
    // After reveal, body access is allowed
    assert!(checker.check_body_access("hidden", &(10..11)).is_none());
}

#[test]
fn opaque_is_opaque() {
    let mut checker = OpaqueFunctionChecker::new();
    assert!(!checker.is_opaque("f"));
    checker.declare_opaque("f".into(), true, 0..1);
    assert!(checker.is_opaque("f"));
}

#[test]
fn opaque_non_opaque_call_ok() {
    let checker = OpaqueFunctionChecker::new();
    assert!(checker.check_call("regular_fn", &(0..1)).is_none());
}

#[test]
fn opaque_default() {
    let checker = OpaqueFunctionChecker::default();
    assert!(!checker.is_opaque("x"));
}

// =======================================================================
// T083: TestGenerator tests
// =======================================================================

#[test]
fn test_gen_property_test() {
    let tgen = TestGenerator::new();
    let contract = TestableContract {
        name: "safe_div".into(),
        params: vec![("a".into(), Type::Int), ("b".into(), Type::Int)],
        requires: vec!["b != 0".into()],
        ensures: vec!["result * b + (a % b) == a".into()],
    };
    let test = tgen.generate_property_test(&contract);
    assert_eq!(test.kind, TestKind::Property);
    assert!(test.body.contains("proptest!"));
    assert!(test.body.contains("prop_assume!"));
    assert!(test.body.contains("b != 0"));
}

#[test]
fn test_gen_boundary_values() {
    let tgen = TestGenerator::new();
    let contract = TestableContract {
        name: "check".into(),
        params: vec![("x".into(), Type::U8)],
        requires: vec![],
        ensures: vec![],
    };
    let tests = tgen.generate_boundary_tests(&contract);
    assert_eq!(tests.len(), 3); // 0, 1, 255
    assert!(tests.iter().all(|t| t.kind == TestKind::Boundary));
}

#[test]
fn test_gen_smoke_test() {
    let tgen = TestGenerator::new();
    let contract = TestableContract {
        name: "foo".into(),
        params: vec![],
        requires: vec![],
        ensures: vec![],
    };
    let test = tgen.generate_smoke_test(&contract);
    assert_eq!(test.kind, TestKind::Smoke);
    assert!(test.body.contains("smoke_foo"));
}

#[test]
fn test_gen_generate_all() {
    let mut tgen = TestGenerator::new();
    tgen.add_contract(TestableContract {
        name: "add".into(),
        params: vec![("a".into(), Type::I32), ("b".into(), Type::I32)],
        requires: vec![],
        ensures: vec!["result == a + b".into()],
    });
    let all = tgen.generate_all();
    // 1 property + 10 boundary (5 per I32 param * 2) + 1 smoke
    assert_eq!(all.len(), 12);
}

#[test]
fn test_gen_no_requires() {
    let tgen = TestGenerator::new();
    let contract = TestableContract {
        name: "no_pre".into(),
        params: vec![("x".into(), Type::Bool)],
        requires: vec![],
        ensures: vec!["result".into()],
    };
    let test = tgen.generate_property_test(&contract);
    assert!(!test.body.contains("prop_assume!"));
}

#[test]
fn test_gen_default() {
    let tgen = TestGenerator::default();
    assert!(tgen.generate_all().is_empty());
}
