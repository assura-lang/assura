//! Generic type instantiation and arity checking.

#[cfg(test)]
use std::collections::HashMap;
use std::ops::Range;

use assura_parser::ast::{ClauseKind, Decl};

use crate::convert::type_from_expr;
use crate::{Type, TypeError};

// ---------------------------------------------------------------------------
// Generic type instantiation (T015)
// ---------------------------------------------------------------------------

/// Expected number of type arguments for built-in generic types.
fn builtin_generic_arity(name: &str) -> Option<usize> {
    match name {
        "List" | "Set" | "Option" | "Sequence" => Some(1),
        "Map" | "Result" => Some(2),
        _ => None,
    }
}

/// Check that a generic type instantiation has the correct number of type
/// arguments.
///
/// For built-in generic types (`List`, `Map`, `Set`, `Option`, `Result`,
/// `Sequence`), the expected arity is hardcoded. For user-defined generic
/// types, the expected arity is taken from the `type_params` count in the
/// symbol table (looked up from the source AST declarations).
///
/// Returns `Ok(())` on success, or `Err(TypeError)` with code A03002 if the
/// argument count does not match.
pub(crate) fn check_generic_instantiation(
    type_name: &str,
    type_args: &[Type],
    span: &Range<usize>,
    source: &assura_parser::ast::SourceFile,
) -> Result<(), TypeError> {
    // Try built-in generic arity first
    if let Some(expected) = builtin_generic_arity(type_name) {
        let actual = type_args.len();
        if actual != expected {
            return Err(TypeError {
                code: "A03002".into(),
                message: format!(
                    "wrong number of type arguments for `{type_name}`: \
                     expected {expected}, found {actual}"
                ),
                span: span.clone(),
                secondary: None,
            });
        }
        return Ok(());
    }

    // Look up user-defined type parameter count from source AST
    if let Some(expected) = user_defined_type_param_count(type_name, source) {
        let actual = type_args.len();
        if actual != expected {
            return Err(TypeError {
                code: "A03002".into(),
                message: format!(
                    "wrong number of type arguments for `{type_name}`: \
                     expected {expected}, found {actual}"
                ),
                span: span.clone(),
                secondary: None,
            });
        }
        return Ok(());
    }

    // Unknown type name; not our problem here (name resolution handles it)
    Ok(())
}

/// Look up the number of type parameters for a user-defined type, contract,
/// or enum by scanning the source AST declarations.
fn user_defined_type_param_count(
    name: &str,
    source: &assura_parser::ast::SourceFile,
) -> Option<usize> {
    for decl in &source.decls {
        match &decl.node {
            Decl::TypeDef(t) if t.name == name => return Some(t.type_params.len()),
            Decl::EnumDef(e) if e.name == name => return Some(e.type_params.len()),
            Decl::Contract(c) if c.name == name => return Some(c.type_params.len()),
            _ => {}
        }
    }
    None
}

/// Substitute type parameters with concrete types in a `Type`.
///
/// Given a mapping from type parameter names to concrete types, recursively
/// replaces every `Type::TypeParam(name)` that appears in `bindings` with
/// the corresponding concrete type. Types not in the bindings map are left
/// unchanged.
#[cfg(test)]
pub(crate) fn substitute(ty: &Type, bindings: &HashMap<String, Type>) -> Type {
    match ty {
        Type::TypeParam(name) => bindings.get(name).cloned().unwrap_or_else(|| ty.clone()),
        Type::List(inner) => Type::List(Box::new(substitute(inner, bindings))),
        Type::Set(inner) => Type::Set(Box::new(substitute(inner, bindings))),
        Type::Option(inner) => Type::Option(Box::new(substitute(inner, bindings))),
        Type::Sequence(inner) => Type::Sequence(Box::new(substitute(inner, bindings))),
        Type::Map(k, v) => Type::Map(
            Box::new(substitute(k, bindings)),
            Box::new(substitute(v, bindings)),
        ),
        Type::Result(t, e) => Type::Result(
            Box::new(substitute(t, bindings)),
            Box::new(substitute(e, bindings)),
        ),
        Type::Fn { params, ret } => Type::Fn {
            params: params.iter().map(|p| substitute(p, bindings)).collect(),
            ret: Box::new(substitute(ret, bindings)),
        },
        Type::Refined { base, predicate } => Type::Refined {
            base: Box::new(substitute(base, bindings)),
            predicate: predicate.clone(),
        },
        // All other types are leaves; no substitution needed
        _ => ty.clone(),
    }
}

/// Instantiate a built-in generic type with concrete type arguments.
///
/// Given a built-in generic name and validated type arguments, returns the
/// fully instantiated `Type`. Panics if the argument count is wrong (caller
/// should validate via `check_generic_instantiation` first).
#[cfg(test)]
pub(crate) fn instantiate_builtin_generic(name: &str, args: Vec<Type>) -> Option<Type> {
    match name {
        "List" => Some(Type::List(Box::new(args.into_iter().next()?))),
        "Set" => Some(Type::Set(Box::new(args.into_iter().next()?))),
        "Option" => Some(Type::Option(Box::new(args.into_iter().next()?))),
        "Sequence" => Some(Type::Sequence(Box::new(args.into_iter().next()?))),
        "Map" => {
            let mut it = args.into_iter();
            let k = it.next()?;
            let v = it.next()?;
            Some(Type::Map(Box::new(k), Box::new(v)))
        }
        "Result" => {
            let mut it = args.into_iter();
            let t = it.next()?;
            let e = it.next()?;
            Some(Type::Result(Box::new(t), Box::new(e)))
        }
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// T015: Generic instantiation arity checking (run on all type exprs)
// ---------------------------------------------------------------------------

/// Walk all declarations and check that generic type instantiations
/// (e.g. `List<Int, Bool>`) have the correct number of type arguments.
pub(crate) fn run_generic_instantiation_checks(
    source: &assura_parser::ast::SourceFile,
) -> Vec<TypeError> {
    use assura_parser::ast::TypeExpr;
    let mut errors = Vec::new();

    fn check_type_expr(
        te: &TypeExpr,
        span: &Range<usize>,
        source: &assura_parser::ast::SourceFile,
        errors: &mut Vec<TypeError>,
    ) {
        if let TypeExpr::Generic(name, args) = te {
            // Check arity
            let type_args: Vec<Type> = args.iter().map(type_from_expr).collect();
            if let Err(e) = check_generic_instantiation(name, &type_args, span, source) {
                errors.push(e);
            }
            // Recurse into type arguments
            for arg in args {
                check_type_expr(arg, span, source, errors);
            }
        }
        if let TypeExpr::Fn { params, ret } = te {
            for p in params {
                check_type_expr(p, span, source, errors);
            }
            check_type_expr(ret, span, source, errors);
        }
        if let TypeExpr::Refined { base, .. } = te {
            check_type_expr(base, span, source, errors);
        }
    }

    fn check_params(
        params: &[assura_parser::ast::Param],
        span: &Range<usize>,
        source: &assura_parser::ast::SourceFile,
        errors: &mut Vec<TypeError>,
    ) {
        for p in params {
            if let Some(te) = &p.ty {
                check_type_expr(te, span, source, errors);
            }
        }
    }

    fn check_fields(
        fields: &[assura_parser::ast::FieldDef],
        span: &Range<usize>,
        source: &assura_parser::ast::SourceFile,
        errors: &mut Vec<TypeError>,
    ) {
        for f in fields {
            if let Some(te) = &f.ty {
                check_type_expr(te, span, source, errors);
            }
        }
    }

    for decl in &source.decls {
        let span = &decl.span;
        match &decl.node {
            Decl::Contract(c) => {
                for clause in &c.clauses {
                    if let ClauseKind::Input | ClauseKind::Output = &clause.kind {
                        // Params may be in clause bodies; handled by param extraction
                    }
                }
            }
            Decl::TypeDef(td) => {
                if let assura_parser::ast::TypeBody::Struct(fields) = &td.body {
                    check_fields(fields, span, source, &mut errors);
                }
            }
            Decl::FnDef(f) => {
                check_params(&f.params, span, source, &mut errors);
                if let Some(te) = &f.return_ty {
                    check_type_expr(te, span, source, &mut errors);
                }
            }
            Decl::Extern(e) => {
                check_params(&e.params, span, source, &mut errors);
                if let Some(te) = &e.return_ty {
                    check_type_expr(te, span, source, &mut errors);
                }
            }
            _ => {}
        }
    }

    errors
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- builtin_generic_arity (private helper) ----

    #[test]
    fn builtin_generic_arity_single_param_types() {
        assert_eq!(builtin_generic_arity("List"), Some(1));
        assert_eq!(builtin_generic_arity("Set"), Some(1));
        assert_eq!(builtin_generic_arity("Option"), Some(1));
        assert_eq!(builtin_generic_arity("Sequence"), Some(1));
    }

    #[test]
    fn builtin_generic_arity_two_param_types() {
        assert_eq!(builtin_generic_arity("Map"), Some(2));
        assert_eq!(builtin_generic_arity("Result"), Some(2));
    }

    #[test]
    fn builtin_generic_arity_non_generic_returns_none() {
        assert_eq!(builtin_generic_arity("Int"), None);
        assert_eq!(builtin_generic_arity("Bool"), None);
        assert_eq!(builtin_generic_arity("String"), None);
        assert_eq!(builtin_generic_arity("FooBar"), None);
        assert_eq!(builtin_generic_arity(""), None);
    }

    // ---- substitute ----

    #[test]
    fn substitute_multiple_params() {
        let mut bindings = HashMap::new();
        bindings.insert("A".into(), Type::Int);
        bindings.insert("B".into(), Type::Bool);
        // Map<A, B> -> Map<Int, Bool>
        let ty = Type::Map(
            Box::new(Type::TypeParam("A".into())),
            Box::new(Type::TypeParam("B".into())),
        );
        let result = substitute(&ty, &bindings);
        assert_eq!(result, Type::Map(Box::new(Type::Int), Box::new(Type::Bool)));
    }

    #[test]
    fn substitute_empty_bindings_identity() {
        let bindings = HashMap::new();
        let ty = Type::List(Box::new(Type::TypeParam("T".into())));
        let result = substitute(&ty, &bindings);
        // No bindings: TypeParam stays as-is
        assert_eq!(result, Type::List(Box::new(Type::TypeParam("T".into()))));
    }

    #[test]
    fn substitute_tuple_type() {
        let mut bindings = HashMap::new();
        bindings.insert("T".into(), Type::Nat);
        // Tuple is a leaf in substitute, so TypeParams inside remain
        let ty = Type::Tuple(vec![Type::Int, Type::Bool]);
        let result = substitute(&ty, &bindings);
        // Tuple is handled by the `_ => ty.clone()` arm
        assert_eq!(result, Type::Tuple(vec![Type::Int, Type::Bool]));
    }

    // ---- instantiate_builtin_generic ----

    #[test]
    fn instantiate_returns_none_for_non_builtin() {
        assert_eq!(instantiate_builtin_generic("MyType", vec![Type::Int]), None);
    }

    #[test]
    fn instantiate_result_with_concrete_types() {
        let result = instantiate_builtin_generic("Result", vec![Type::Nat, Type::String]);
        assert_eq!(
            result,
            Some(Type::Result(Box::new(Type::Nat), Box::new(Type::String)))
        );
    }

    #[test]
    fn instantiate_list_with_nested_generic() {
        // List<Option<Int>>
        let inner = Type::Option(Box::new(Type::Int));
        let result = instantiate_builtin_generic("List", vec![inner.clone()]);
        assert_eq!(result, Some(Type::List(Box::new(inner))));
    }

    // ---- check_generic_instantiation ----

    fn empty_source() -> assura_parser::ast::SourceFile {
        assura_parser::ast::SourceFile {
            project: None,
            module: None,
            imports: vec![],
            decls: vec![],
        }
    }

    #[test]
    fn check_generic_correct_arity_ok() {
        let src = empty_source();
        check_generic_instantiation("List", &[Type::Int], &(0..1), &src).unwrap();
        assert!(
            check_generic_instantiation("Map", &[Type::String, Type::Int], &(0..1), &src).is_ok()
        );
    }

    #[test]
    fn check_generic_wrong_arity_a03002() {
        let src = empty_source();
        let err = check_generic_instantiation("List", &[], &(0..5), &src).unwrap_err();
        assert_eq!(err.code, "A03002");
        assert!(err.message.contains("expected 1"));
        assert!(err.message.contains("found 0"));
        assert_eq!(err.span, 0..5);
    }

    #[test]
    fn check_generic_unknown_type_lenient() {
        let src = empty_source();
        // Unknown type names pass through (name resolution handles them)
        let result =
            check_generic_instantiation("UnknownType", &[Type::Int, Type::Bool], &(0..1), &src);
        result.unwrap();
    }
}
