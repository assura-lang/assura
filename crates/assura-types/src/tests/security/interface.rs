use super::*;

// --- T062: Interface contract tests ---

#[test]
fn interface_missing_method_a13001() {
    let mut checker = InterfaceChecker::new();
    checker.register_interface(InterfaceContract {
        name: "Serializable".into(),
        methods: vec![
            InterfaceMethod {
                name: "serialize".into(),
                param_types: vec![],
                return_type: Type::Bytes,
                has_requires: false,
                has_ensures: true,
                no_reentrancy: false,
            },
            InterfaceMethod {
                name: "deserialize".into(),
                param_types: vec![Type::Bytes],
                return_type: Type::Named("Self".into()),
                has_requires: true,
                has_ensures: true,
                no_reentrancy: false,
            },
        ],
        extends: vec![],
    });

    // Only implement serialize, not deserialize
    let errors = checker.check_impl("MyType", "Serializable", &["serialize".into()], &(0..1));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A13001");
    assert!(errors[0].message.contains("deserialize"));
}

#[test]
fn interface_all_methods_implemented_ok() {
    let mut checker = InterfaceChecker::new();
    checker.register_interface(InterfaceContract {
        name: "Hashable".into(),
        methods: vec![InterfaceMethod {
            name: "hash".into(),
            param_types: vec![],
            return_type: Type::U64,
            has_requires: false,
            has_ensures: true,
            no_reentrancy: false,
        }],
        extends: vec![],
    });

    let errors = checker.check_impl("MyType", "Hashable", &["hash".into()], &(0..1));
    assert!(errors.is_empty());
}

#[test]
fn interface_signature_param_count_mismatch_a13002() {
    let mut checker = InterfaceChecker::new();
    checker.register_interface(InterfaceContract {
        name: "Comparable".into(),
        methods: vec![InterfaceMethod {
            name: "compare".into(),
            param_types: vec![Type::Int, Type::Int],
            return_type: Type::Bool,
            has_requires: false,
            has_ensures: false,
            no_reentrancy: false,
        }],
        extends: vec![],
    });

    let errors = checker.check_method_signature(
        "Comparable",
        "compare",
        &[Type::Int], // only 1 param
        &Type::Bool,
        &(0..1),
    );
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A13002");
}

#[test]
fn interface_signature_return_type_mismatch_a13002() {
    let mut checker = InterfaceChecker::new();
    checker.register_interface(InterfaceContract {
        name: "Comparable".into(),
        methods: vec![InterfaceMethod {
            name: "compare".into(),
            param_types: vec![Type::Int],
            return_type: Type::Bool,
            has_requires: false,
            has_ensures: false,
            no_reentrancy: false,
        }],
        extends: vec![],
    });

    let errors = checker.check_method_signature(
        "Comparable",
        "compare",
        &[Type::Int],
        &Type::Int, // wrong return type
        &(0..1),
    );
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A13002");
    assert!(errors[0].message.contains("return type"));
}

#[test]
fn interface_reentrancy_violation_a13003() {
    let mut checker = InterfaceChecker::new();
    checker.register_interface(InterfaceContract {
        name: "Callback".into(),
        methods: vec![InterfaceMethod {
            name: "on_event".into(),
            param_types: vec![],
            return_type: Type::Unit,
            has_requires: false,
            has_ensures: false,
            no_reentrancy: true,
        }],
        extends: vec![],
    });

    let errors = checker.check_reentrancy("Callback", "on_event", true, &(0..1));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A13003");
}

#[test]
fn interface_reentrancy_no_flag_ok() {
    let mut checker = InterfaceChecker::new();
    checker.register_interface(InterfaceContract {
        name: "Callback".into(),
        methods: vec![InterfaceMethod {
            name: "on_event".into(),
            param_types: vec![],
            return_type: Type::Unit,
            has_requires: false,
            has_ensures: false,
            no_reentrancy: false,
        }],
        extends: vec![],
    });

    let errors = checker.check_reentrancy("Callback", "on_event", true, &(0..1));
    assert!(errors.is_empty(), "method allows reentrancy");
}

#[test]
fn interface_super_interface_inheritance() {
    let mut checker = InterfaceChecker::new();
    checker.register_interface(InterfaceContract {
        name: "Eq".into(),
        methods: vec![InterfaceMethod {
            name: "equals".into(),
            param_types: vec![Type::Named("Self".into())],
            return_type: Type::Bool,
            has_requires: false,
            has_ensures: false,
            no_reentrancy: false,
        }],
        extends: vec![],
    });
    checker.register_interface(InterfaceContract {
        name: "Ord".into(),
        methods: vec![InterfaceMethod {
            name: "compare_to".into(),
            param_types: vec![Type::Named("Self".into())],
            return_type: Type::Int,
            has_requires: false,
            has_ensures: false,
            no_reentrancy: false,
        }],
        extends: vec!["Eq".into()],
    });

    // Implement compare_to but not equals -> A13001 for missing super method
    let errors = checker.check_impl("MyType", "Ord", &["compare_to".into()], &(0..1));
    assert_eq!(errors.len(), 1);
    assert!(errors[0].message.contains("equals"));
    assert!(errors[0].message.contains("Eq"));
}

#[test]
fn interface_unknown_interface_a13001() {
    let checker = InterfaceChecker::new();
    let errors = checker.check_impl("MyType", "Unknown", &[], &(0..1));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A13001");
    assert!(errors[0].message.contains("Unknown"));
}

