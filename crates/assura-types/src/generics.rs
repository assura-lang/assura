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
            if let Some(te) = &p.parsed_type {
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
            if let Some(te) = &f.parsed_type {
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
                if let Some(te) = &f.return_type_expr {
                    check_type_expr(te, span, source, &mut errors);
                }
            }
            Decl::Extern(e) => {
                check_params(&e.params, span, source, &mut errors);
                if let Some(te) = &e.return_type_expr {
                    check_type_expr(te, span, source, &mut errors);
                }
            }
            _ => {}
        }
    }

    errors
}
