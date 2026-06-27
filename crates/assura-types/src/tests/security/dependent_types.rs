use super::*;

// --- T052: Dependent type tests ---

#[test]
fn dep_type_nat_index_valid() {
    let checker = DependentTypeChecker::new();
    let errors = checker.validate_index("n", "Nat", &(0..1));
    assert!(errors.is_empty(), "Nat should be a valid index type");
}

#[test]
fn dep_type_bool_index_valid() {
    let checker = DependentTypeChecker::new();
    let errors = checker.validate_index("flag", "Bool", &(0..1));
    assert!(errors.is_empty(), "Bool should be a valid index type");
}

#[test]
fn dep_type_enum_index_valid() {
    let mut checker = DependentTypeChecker::new();
    checker.register_enum("Mode".into(), vec!["Read".into(), "Write".into()]);
    let errors = checker.validate_index("mode", "Mode", &(0..1));
    assert!(errors.is_empty(), "known enum should be a valid index type");
}

#[test]
fn dep_type_unknown_type_a03006() {
    let checker = DependentTypeChecker::new();
    let errors = checker.validate_index("x", "String", &(0..1));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A03006");
}

#[test]
fn dep_type_nat_arithmetic_valid() {
    let mut checker = DependentTypeChecker::new();
    checker.bind_index("n".into(), DepIndex::Nat("n".into()));
    // n + 1 is a valid Nat expression
    let expr = Spanned::no_span(AstExpr::BinOp {
        lhs: Box::new(Spanned::no_span(AstExpr::Ident("n".into()))),
        op: AstBinOp::Add,
        rhs: Box::new(Spanned::no_span(AstExpr::Literal(AstLit::Int("1".into())))),
    });
    let errors = checker.check_index_expr(&expr, &DepIndex::Nat("n".into()), &(0..1));
    assert!(errors.is_empty(), "n + 1 should be valid Nat arithmetic");
}

#[test]
fn dep_type_bool_arithmetic_rejected() {
    let mut checker = DependentTypeChecker::new();
    checker.bind_index("flag".into(), DepIndex::Bool("flag".into()));
    // flag + 1 is NOT valid for a Bool index
    let expr = Spanned::no_span(AstExpr::BinOp {
        lhs: Box::new(Spanned::no_span(AstExpr::Ident("flag".into()))),
        op: AstBinOp::Add,
        rhs: Box::new(Spanned::no_span(AstExpr::Literal(AstLit::Int("1".into())))),
    });
    let errors = checker.check_index_expr(&expr, &DepIndex::Bool("flag".into()), &(0..1));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A03008");
}

#[test]
fn dep_type_enum_variant_valid() {
    let mut checker = DependentTypeChecker::new();
    checker.register_enum("Mode".into(), vec!["Read".into(), "Write".into()]);
    checker.bind_index(
        "m".into(),
        DepIndex::Enum {
            name: "m".into(),
            enum_type: "Mode".into(),
        },
    );
    let expr = Spanned::no_span(AstExpr::Ident("Read".into()));
    let idx = DepIndex::Enum {
        name: "m".into(),
        enum_type: "Mode".into(),
    };
    let errors = checker.check_index_expr(&expr, &idx, &(0..1));
    assert!(errors.is_empty(), "enum variant should be valid");
}

#[test]
fn dep_type_equality_matching() {
    let checker = DependentTypeChecker::new();
    let t1 = DepType {
        base: Type::List(Box::new(Type::Int)),
        indices: vec![DepIndex::Nat("n".into())],
    };
    let t2 = DepType {
        base: Type::List(Box::new(Type::Int)),
        indices: vec![DepIndex::Nat("m".into())],
    };
    let errors = checker.check_dep_type_eq(&t1, &t2, &(0..1));
    assert!(errors.is_empty(), "same structure should match");
}

#[test]
fn dep_type_equality_base_mismatch() {
    let checker = DependentTypeChecker::new();
    let t1 = DepType {
        base: Type::List(Box::new(Type::Int)),
        indices: vec![DepIndex::Nat("n".into())],
    };
    let t2 = DepType {
        base: Type::List(Box::new(Type::Float)),
        indices: vec![DepIndex::Nat("n".into())],
    };
    let errors = checker.check_dep_type_eq(&t1, &t2, &(0..1));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A03010");
}

#[test]
fn dep_type_equality_index_count_mismatch() {
    let checker = DependentTypeChecker::new();
    let t1 = DepType {
        base: Type::Int,
        indices: vec![DepIndex::Nat("n".into())],
    };
    let t2 = DepType {
        base: Type::Int,
        indices: vec![DepIndex::Nat("n".into()), DepIndex::Bool("b".into())],
    };
    let errors = checker.check_dep_type_eq(&t1, &t2, &(0..1));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A03010");
}

#[test]
fn dep_type_index_erasure_ghost_ok() {
    let mut checker = DependentTypeChecker::new();
    checker.bind_index("n".into(), DepIndex::Nat("n".into()));
    let expr = Spanned::no_span(AstExpr::Ident("n".into()));
    let errors = checker.check_index_erasure(&expr, true, &(0..1));
    assert!(errors.is_empty(), "index in ghost context is ok");
}

#[test]
fn dep_type_index_erasure_runtime_error() {
    let mut checker = DependentTypeChecker::new();
    checker.bind_index("n".into(), DepIndex::Nat("n".into()));
    let expr = Spanned::no_span(AstExpr::Ident("n".into()));
    let errors = checker.check_index_erasure(&expr, false, &(0..1));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A03012");
}

#[test]
fn dep_type_index_kind_mismatch() {
    let checker = DependentTypeChecker::new();
    let t1 = DepType {
        base: Type::Int,
        indices: vec![DepIndex::Nat("n".into())],
    };
    let t2 = DepType {
        base: Type::Int,
        indices: vec![DepIndex::Bool("b".into())],
    };
    let errors = checker.check_dep_type_eq(&t1, &t2, &(0..1));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A03011");
}

#[test]
fn dep_type_display() {
    assert_eq!(DepIndex::Nat("n".into()).to_string(), "n: Nat");
    assert_eq!(DepIndex::Bool("flag".into()).to_string(), "flag: Bool");
    assert_eq!(
        DepIndex::Enum {
            name: "m".into(),
            enum_type: "Mode".into()
        }
        .to_string(),
        "m: Mode"
    );
}

