use super::*;
// T015: Generic type instantiation tests
// -----------------------------------------------------------------------

/// Helper: build a minimal SourceFile with declarations for testing
/// generic instantiation against user-defined types.
fn source_with_decls(
    decls: Vec<assura_parser::ast::Spanned<Decl>>,
) -> assura_parser::ast::SourceFile {
    assura_parser::ast::SourceFile {
        project: None,
        module: None,
        imports: vec![],
        decls,
    }
}

fn spanned_decl(decl: Decl) -> assura_parser::ast::Spanned<Decl> {
    assura_parser::ast::Spanned {
        node: decl,
        span: 0..1,
    }
}

#[test]
fn generic_list_one_arg_ok() {
    let src = source_with_decls(vec![]);
    let result = check_generic_instantiation("List", &[Type::Int], &(0..1), &src);
    result.unwrap();
}

#[test]
fn generic_list_zero_args_a03002() {
    let src = source_with_decls(vec![]);
    let err = check_generic_instantiation("List", &[], &(0..1), &src).unwrap_err();
    assert_eq!(err.code, "A03002");
    assert!(err.message.contains("List"));
    assert!(err.message.contains("expected 1"));
    assert!(err.message.contains("found 0"));
}

#[test]
fn generic_list_two_args_a03002() {
    let src = source_with_decls(vec![]);
    let err =
        check_generic_instantiation("List", &[Type::Int, Type::Bool], &(0..1), &src).unwrap_err();
    assert_eq!(err.code, "A03002");
    assert!(err.message.contains("expected 1"));
    assert!(err.message.contains("found 2"));
}

#[test]
fn generic_map_two_args_ok() {
    let src = source_with_decls(vec![]);
    let result = check_generic_instantiation("Map", &[Type::String, Type::Int], &(0..1), &src);
    result.unwrap();
}

#[test]
fn generic_map_one_arg_a03002() {
    let src = source_with_decls(vec![]);
    let err = check_generic_instantiation("Map", &[Type::String], &(0..1), &src).unwrap_err();
    assert_eq!(err.code, "A03002");
    assert!(err.message.contains("Map"));
    assert!(err.message.contains("expected 2"));
    assert!(err.message.contains("found 1"));
}

#[test]
fn generic_set_one_arg_ok() {
    let src = source_with_decls(vec![]);
    let result = check_generic_instantiation("Set", &[Type::Int], &(0..1), &src);
    result.unwrap();
}

#[test]
fn generic_option_one_arg_ok() {
    let src = source_with_decls(vec![]);
    let result = check_generic_instantiation("Option", &[Type::Bool], &(0..1), &src);
    result.unwrap();
}

#[test]
fn generic_result_two_args_ok() {
    let src = source_with_decls(vec![]);
    let result = check_generic_instantiation("Result", &[Type::Int, Type::String], &(0..1), &src);
    result.unwrap();
}

#[test]
fn generic_result_three_args_a03002() {
    let src = source_with_decls(vec![]);
    let err = check_generic_instantiation(
        "Result",
        &[Type::Int, Type::String, Type::Bool],
        &(0..1),
        &src,
    )
    .unwrap_err();
    assert_eq!(err.code, "A03002");
    assert!(err.message.contains("expected 2"));
    assert!(err.message.contains("found 3"));
}

#[test]
fn generic_sequence_one_arg_ok() {
    let src = source_with_decls(vec![]);
    let result = check_generic_instantiation("Sequence", &[Type::Nat], &(0..1), &src);
    result.unwrap();
}

#[test]
fn generic_user_defined_type_correct_arity() {
    let decls = vec![spanned_decl(Decl::TypeDef(assura_parser::ast::TypeDef {
        name: "Pair".into(),
        type_params: vec!["A".into(), "B".into()],
        body: assura_parser::ast::TypeBody::Empty,
    }))];
    let src = source_with_decls(decls);
    let result = check_generic_instantiation("Pair", &[Type::Int, Type::Bool], &(0..1), &src);
    result.unwrap();
}

#[test]
fn generic_user_defined_type_wrong_arity() {
    let decls = vec![spanned_decl(Decl::TypeDef(assura_parser::ast::TypeDef {
        name: "Pair".into(),
        type_params: vec!["A".into(), "B".into()],
        body: assura_parser::ast::TypeBody::Empty,
    }))];
    let src = source_with_decls(decls);
    let err = check_generic_instantiation("Pair", &[Type::Int], &(0..1), &src).unwrap_err();
    assert_eq!(err.code, "A03002");
    assert!(err.message.contains("Pair"));
    assert!(err.message.contains("expected 2"));
    assert!(err.message.contains("found 1"));
}

#[test]
fn generic_user_defined_enum_correct_arity() {
    let decls = vec![spanned_decl(Decl::EnumDef(assura_parser::ast::EnumDef {
        name: "Maybe".into(),
        type_params: vec!["T".into()],
        variants: vec![],
    }))];
    let src = source_with_decls(decls);
    let result = check_generic_instantiation("Maybe", &[Type::Int], &(0..1), &src);
    result.unwrap();
}

#[test]
fn generic_user_defined_enum_wrong_arity() {
    let decls = vec![spanned_decl(Decl::EnumDef(assura_parser::ast::EnumDef {
        name: "Maybe".into(),
        type_params: vec!["T".into()],
        variants: vec![],
    }))];
    let src = source_with_decls(decls);
    let err =
        check_generic_instantiation("Maybe", &[Type::Int, Type::Bool], &(0..1), &src).unwrap_err();
    assert_eq!(err.code, "A03002");
    assert!(err.message.contains("Maybe"));
    assert!(err.message.contains("expected 1"));
    assert!(err.message.contains("found 2"));
}

#[test]
fn generic_user_defined_contract_correct_arity() {
    let decls = vec![spanned_decl(Decl::Contract(
        assura_parser::ast::ContractDecl {
            name: "Container".into(),
            type_params: vec!["T".into()],
            clauses: vec![],
            fn_params: vec![],
        },
    ))];
    let src = source_with_decls(decls);
    let result = check_generic_instantiation("Container", &[Type::Int], &(0..1), &src);
    result.unwrap();
}

#[test]
fn generic_user_defined_non_generic_type_zero_args_ok() {
    let decls = vec![spanned_decl(Decl::TypeDef(assura_parser::ast::TypeDef {
        name: "Foo".into(),
        type_params: vec![],
        body: assura_parser::ast::TypeBody::Empty,
    }))];
    let src = source_with_decls(decls);
    let result = check_generic_instantiation("Foo", &[], &(0..1), &src);
    result.unwrap();
}

#[test]
fn generic_user_defined_non_generic_type_with_args_a03002() {
    let decls = vec![spanned_decl(Decl::TypeDef(assura_parser::ast::TypeDef {
        name: "Foo".into(),
        type_params: vec![],
        body: assura_parser::ast::TypeBody::Empty,
    }))];
    let src = source_with_decls(decls);
    let err = check_generic_instantiation("Foo", &[Type::Int], &(0..1), &src).unwrap_err();
    assert_eq!(err.code, "A03002");
    assert!(err.message.contains("expected 0"));
    assert!(err.message.contains("found 1"));
}

#[test]
fn generic_unknown_type_is_lenient() {
    let src = source_with_decls(vec![]);
    // Unknown type name; not our problem (name resolution handles it)
    let result = check_generic_instantiation("UnknownType", &[Type::Int], &(0..1), &src);
    result.unwrap();
}

// -- substitute() tests --

#[test]
fn substitute_type_param() {
    let mut bindings = HashMap::new();
    bindings.insert("T".into(), Type::Int);
    let result = substitute(&Type::TypeParam("T".into()), &bindings);
    assert_eq!(result, Type::Int);
}

#[test]
fn substitute_unbound_type_param_unchanged() {
    let bindings = HashMap::new();
    let result = substitute(&Type::TypeParam("T".into()), &bindings);
    assert_eq!(result, Type::TypeParam("T".into()));
}

#[test]
fn substitute_in_list() {
    let mut bindings = HashMap::new();
    bindings.insert("T".into(), Type::Int);
    let ty = Type::List(Box::new(Type::TypeParam("T".into())));
    let result = substitute(&ty, &bindings);
    assert_eq!(result, Type::List(Box::new(Type::Int)));
}

#[test]
fn substitute_in_map() {
    let mut bindings = HashMap::new();
    bindings.insert("K".into(), Type::String);
    bindings.insert("V".into(), Type::Int);
    let ty = Type::Map(
        Box::new(Type::TypeParam("K".into())),
        Box::new(Type::TypeParam("V".into())),
    );
    let result = substitute(&ty, &bindings);
    assert_eq!(
        result,
        Type::Map(Box::new(Type::String), Box::new(Type::Int))
    );
}

#[test]
fn substitute_in_set() {
    let mut bindings = HashMap::new();
    bindings.insert("T".into(), Type::Bool);
    let ty = Type::Set(Box::new(Type::TypeParam("T".into())));
    let result = substitute(&ty, &bindings);
    assert_eq!(result, Type::Set(Box::new(Type::Bool)));
}

#[test]
fn substitute_in_option() {
    let mut bindings = HashMap::new();
    bindings.insert("T".into(), Type::Float);
    let ty = Type::Option(Box::new(Type::TypeParam("T".into())));
    let result = substitute(&ty, &bindings);
    assert_eq!(result, Type::Option(Box::new(Type::Float)));
}

#[test]
fn substitute_in_result() {
    let mut bindings = HashMap::new();
    bindings.insert("T".into(), Type::Int);
    bindings.insert("E".into(), Type::String);
    let ty = Type::Result(
        Box::new(Type::TypeParam("T".into())),
        Box::new(Type::TypeParam("E".into())),
    );
    let result = substitute(&ty, &bindings);
    assert_eq!(
        result,
        Type::Result(Box::new(Type::Int), Box::new(Type::String))
    );
}

#[test]
fn substitute_in_sequence() {
    let mut bindings = HashMap::new();
    bindings.insert("T".into(), Type::Nat);
    let ty = Type::Sequence(Box::new(Type::TypeParam("T".into())));
    let result = substitute(&ty, &bindings);
    assert_eq!(result, Type::Sequence(Box::new(Type::Nat)));
}

#[test]
fn substitute_in_fn_type() {
    let mut bindings = HashMap::new();
    bindings.insert("T".into(), Type::Int);
    bindings.insert("U".into(), Type::Bool);
    let ty = Type::Fn {
        params: vec![Type::TypeParam("T".into()), Type::TypeParam("U".into())],
        ret: Box::new(Type::TypeParam("T".into())),
    };
    let result = substitute(&ty, &bindings);
    assert_eq!(
        result,
        Type::Fn {
            params: vec![Type::Int, Type::Bool],
            ret: Box::new(Type::Int),
        }
    );
}

#[test]
fn substitute_in_refined_type() {
    let mut bindings = HashMap::new();
    bindings.insert("T".into(), Type::Int);
    let ty = Type::refined_from_str(Type::TypeParam("T".into()), "v", "v > 0");
    let result = substitute(&ty, &bindings);
    assert_eq!(
        result,
        Type::refined_from_str(Type::Int, "v", "v > 0")
    );
}

#[test]
fn substitute_nested_generics() {
    let mut bindings = HashMap::new();
    bindings.insert("T".into(), Type::Int);
    // List<Option<T>> -> List<Option<Int>>
    let ty = Type::List(Box::new(Type::Option(Box::new(Type::TypeParam(
        "T".into(),
    )))));
    let result = substitute(&ty, &bindings);
    assert_eq!(
        result,
        Type::List(Box::new(Type::Option(Box::new(Type::Int))))
    );
}

#[test]
fn substitute_leaves_concrete_types_unchanged() {
    let mut bindings = HashMap::new();
    bindings.insert("T".into(), Type::Bool);
    // Concrete types should be unchanged
    assert_eq!(substitute(&Type::Int, &bindings), Type::Int);
    assert_eq!(substitute(&Type::Bool, &bindings), Type::Bool);
    assert_eq!(substitute(&Type::String, &bindings), Type::String);
    assert_eq!(substitute(&Type::Unknown, &bindings), Type::Unknown);
    assert_eq!(
        substitute(&Type::Named("Foo".into()), &bindings),
        Type::Named("Foo".into())
    );
}

#[test]
fn substitute_partial_bindings() {
    let mut bindings = HashMap::new();
    bindings.insert("K".into(), Type::String);
    // Map<K, V> with only K bound -> Map<String, V>
    let ty = Type::Map(
        Box::new(Type::TypeParam("K".into())),
        Box::new(Type::TypeParam("V".into())),
    );
    let result = substitute(&ty, &bindings);
    assert_eq!(
        result,
        Type::Map(
            Box::new(Type::String),
            Box::new(Type::TypeParam("V".into()))
        )
    );
}

// -- instantiate_builtin_generic() tests --

#[test]
fn instantiate_list() {
    let result = instantiate_builtin_generic("List", vec![Type::Int]);
    assert_eq!(result, Some(Type::List(Box::new(Type::Int))));
}

#[test]
fn instantiate_map() {
    let result = instantiate_builtin_generic("Map", vec![Type::String, Type::Int]);
    assert_eq!(
        result,
        Some(Type::Map(Box::new(Type::String), Box::new(Type::Int)))
    );
}

#[test]
fn instantiate_set() {
    let result = instantiate_builtin_generic("Set", vec![Type::Bool]);
    assert_eq!(result, Some(Type::Set(Box::new(Type::Bool))));
}

#[test]
fn instantiate_option() {
    let result = instantiate_builtin_generic("Option", vec![Type::Float]);
    assert_eq!(result, Some(Type::Option(Box::new(Type::Float))));
}

#[test]
fn instantiate_result() {
    let result = instantiate_builtin_generic("Result", vec![Type::Int, Type::String]);
    assert_eq!(
        result,
        Some(Type::Result(Box::new(Type::Int), Box::new(Type::String)))
    );
}

#[test]
fn instantiate_sequence() {
    let result = instantiate_builtin_generic("Sequence", vec![Type::Nat]);
    assert_eq!(result, Some(Type::Sequence(Box::new(Type::Nat))));
}

#[test]
fn instantiate_unknown_name_returns_none() {
    let result = instantiate_builtin_generic("Foo", vec![Type::Int]);
    assert_eq!(result, None);
}

#[test]
fn instantiate_non_generic_builtin_returns_none() {
    let result = instantiate_builtin_generic("Int", vec![]);
    assert_eq!(result, None);
}

// -----------------------------------------------------------------------
