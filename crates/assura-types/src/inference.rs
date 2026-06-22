//! Expression type inference.
//!
//! Implements `infer_expr` which infers the type of an Assura expression
//! in a given type environment. Covers literals, variables, field access,
//! binary/unary operations, function calls, quantifiers, and more.

use assura_parser::ast::{BinOp, Expr, Literal, SpExpr, Span, UnaryOp};

use crate::clauses::bind_pattern_vars;
use crate::{Type, TypeEnv, TypeError, parse_type_tokens};

// ---------------------------------------------------------------------------
// Expression type inference
// ---------------------------------------------------------------------------

/// Returns `true` if `ty` is a numeric type.
/// Check if an expression is a literal zero (integer 0 or float 0.0).
pub(crate) fn is_literal_zero(expr: &SpExpr) -> bool {
    match &expr.node {
        Expr::Literal(Literal::Int(s)) => s == "0",
        Expr::Literal(Literal::Float(s)) => s == "0.0" || s == "0",
        _ => false,
    }
}

/// Extract the element type from a collection type.
///
/// Used to type quantifier variables: `forall x in xs` where `xs: List<T>`
/// binds `x` to `T`. For range domains (`a..b` parsed as `BinOp::Range`),
/// the element type is `Int`. For non-collection types, returns `Unknown`.
pub(crate) fn element_type_of(domain_ty: &Type) -> Type {
    match domain_ty {
        Type::List(elem) | Type::Set(elem) | Type::Sequence(elem) => *elem.clone(),
        Type::Map(key, _) => *key.clone(),
        Type::Int | Type::Nat => Type::Int, // range domain
        // String indexable by code points
        Type::String => Type::String,
        _ => Type::Unknown,
    }
}

pub(crate) fn is_numeric(ty: &Type) -> bool {
    match ty {
        Type::Int
        | Type::Nat
        | Type::Float
        | Type::U8
        | Type::U16
        | Type::U32
        | Type::U64
        | Type::I8
        | Type::I16
        | Type::I32
        | Type::I64
        | Type::F32
        | Type::F64 => true,
        // A refined type is numeric if its base type is numeric
        Type::Refined { base, .. } => is_numeric(base),
        // Named types may be numeric aliases; be lenient.
        // Error/Unknown suppress diagnostics.
        Type::Named(_) | Type::Unknown | Type::Error => true,
        _ => false,
    }
}

/// Infer the type of an expression given a type environment.
///
/// Returns `Ok(ty)` with the inferred type, or `Err(TypeError)` when a
/// concrete type mismatch is detected (A03001). Unknown types (from
/// unresolved references) are propagated silently; they never trigger
/// errors.
///
/// Errors produced by this overload use `0..0` spans. For proper source
/// locations, use `infer_expr_spanned` with the enclosing clause span.
pub fn infer_expr(expr: &SpExpr, env: &TypeEnv) -> Result<Type, TypeError> {
    infer_expr_spanned(expr, env, 0..0)
}

/// Infer the type of an expression with a context/fallback span for error reporting.
///
/// When the expression has a real (non-zero) span from lowering (post 11.04),
/// errors use the expression's own span for precision (e.g. pointing to the
/// bad sub-expression rather than the whole clause/decl).
/// The passed `span` is used as fallback for recovery nodes (span 0..0) or
/// top-level clause context.
pub fn infer_expr_spanned(expr: &SpExpr, env: &TypeEnv, span: Span) -> Result<Type, TypeError> {
    match &expr.node {
        // --- Literals ---
        Expr::Literal(Literal::Int(_)) => Ok(Type::Int),
        Expr::Literal(Literal::Float(_)) => Ok(Type::Float),
        Expr::Literal(Literal::Str(_)) => Ok(Type::String),
        Expr::Literal(Literal::Bool(_)) => Ok(Type::Bool),

        // --- Identifiers ---
        Expr::Ident(name) => {
            // Boolean literals
            if name == "true" || name == "false" {
                return Ok(Type::Bool);
            }
            // Look up in environment (includes `result` when bound by
            // ensures clause context, and `self` if added by method context)
            Ok(env.lookup(name).cloned().unwrap_or(Type::Unknown))
        }

        // --- Binary operations ---
        Expr::BinOp { lhs, op, rhs } => infer_binop(lhs, op, rhs, env, span.clone()),

        // --- Unary operations ---
        Expr::UnaryOp { op, expr: inner } => {
            let inner_ty = infer_expr_spanned(inner, env, inner.span.clone())?;
            match op {
                UnaryOp::Neg => {
                    if inner_ty.is_indeterminate() || is_numeric(&inner_ty) {
                        Ok(inner_ty)
                    } else {
                        Err(TypeError {
                            code: "A03001".into(),
                            message: format!(
                                "unary `-` requires a numeric type, found `{inner_ty}`"
                            ),
                            span: inner.span.clone(),
                            secondary: None,
                        })
                    }
                }
                UnaryOp::Not => {
                    if inner_ty.is_indeterminate() || inner_ty == Type::Bool {
                        Ok(Type::Bool)
                    } else {
                        Err(TypeError {
                            code: "A03001".into(),
                            message: format!("unary `!` requires Bool, found `{inner_ty}`"),
                            span: inner.span.clone(),
                            secondary: None,
                        })
                    }
                }
            }
        }

        // --- If-then-else ---
        Expr::If {
            cond,
            then_branch,
            else_branch,
        } => {
            let cond_ty = infer_expr_spanned(cond, env, cond.span.clone())?;
            if !cond_ty.is_indeterminate() && cond_ty != Type::Bool {
                return Err(TypeError {
                    code: "A03001".into(),
                    message: format!("if condition must be Bool, found `{cond_ty}`"),
                    span: cond.span.clone(),
                    secondary: None,
                });
            }
            let then_ty = infer_expr_spanned(then_branch, env, then_branch.span.clone())?;
            if let Some(else_br) = else_branch {
                let else_ty = infer_expr_spanned(else_br, env, else_br.span.clone())?;
                if then_ty.is_indeterminate() {
                    Ok(else_ty)
                } else if else_ty.is_indeterminate() || types_compatible(&then_ty, &else_ty) {
                    Ok(then_ty)
                } else {
                    Err(TypeError {
                        code: "A03001".into(),
                        message: format!(
                            "if branches have different types: `{then_ty}` vs `{else_ty}`"
                        ),
                        span: then_branch.span.clone(), // or whole if; using then for now
                        secondary: None,
                    })
                }
            } else {
                Ok(then_ty)
            }
        }

        // --- Quantifiers ---
        Expr::Forall { var, domain, body } | Expr::Exists { var, domain, body } => {
            // Infer the domain type to determine the quantified variable's type.
            let domain_ty = infer_expr_spanned(domain, env, domain.span.clone())?;
            let elem_ty = element_type_of(&domain_ty);
            // Bind the quantified variable in a child environment so the
            // body can reference it with the correct type.
            let mut child_env = env.clone();
            child_env.insert(var.clone(), elem_ty);
            let _body_ty = infer_expr_spanned(body, &child_env, body.span.clone())?;
            Ok(Type::Bool)
        }

        // --- old(expr) ---
        Expr::Old(inner) => infer_expr_spanned(inner, env, inner.span.clone()),

        // --- List literal ---
        Expr::List(items) => {
            if items.is_empty() {
                return Ok(Type::List(Box::new(Type::Unknown)));
            }
            let first_ty = infer_expr_spanned(&items[0], env, items[0].span.clone())?;
            // Check remaining items match the first
            for item in &items[1..] {
                let item_ty = infer_expr_spanned(item, env, item.span.clone())?;
                if !item_ty.is_indeterminate()
                    && !first_ty.is_indeterminate()
                    && item_ty != first_ty
                {
                    return Err(TypeError {
                        code: "A03001".into(),
                        message: format!(
                            "list element type mismatch: expected `{first_ty}`, found `{item_ty}`"
                        ),
                        span: item.span.clone(),
                        secondary: None,
                    });
                }
            }
            Ok(Type::List(Box::new(first_ty)))
        }

        // --- Field access ---
        Expr::Field(receiver, field) => {
            let recv_ty = infer_expr_spanned(receiver, env, receiver.span.clone())?;
            // Try to resolve the field on the receiver's type
            let struct_name = match &recv_ty {
                Type::Named(name) => Some(name.as_str()),
                Type::Refined { base, .. } => {
                    if let Type::Named(name) = base.as_ref() {
                        Some(name.as_str())
                    } else {
                        None
                    }
                }
                _ => None,
            };
            if let Some(sname) = struct_name
                && let Some(field_ty) = env.lookup_field(sname, field)
            {
                return Ok(field_ty.clone());
            }
            // Built-in field resolution on known types
            match &recv_ty {
                // List/Sequence: len/length/size -> Nat, head/first/last -> Option<T>
                Type::List(elem) | Type::Sequence(elem) => match field.as_str() {
                    "len" | "length" | "size" | "capacity" => return Ok(Type::Nat),
                    "is_empty" => return Ok(Type::Bool),
                    "head" | "first" | "last" => {
                        return Ok(Type::Option(elem.clone()));
                    }
                    "tail" | "rest" => return Ok(recv_ty),
                    _ => {}
                },
                // String/Bytes: len/length/size -> Nat
                Type::Bytes | Type::String => match field.as_str() {
                    "len" | "length" | "size" | "capacity" => return Ok(Type::Nat),
                    "is_empty" => return Ok(Type::Bool),
                    _ => {}
                },
                // Option<T>: value -> T, is_some/is_none -> Bool
                Type::Option(inner) => match field.as_str() {
                    "value" | "unwrap" => return Ok(*inner.clone()),
                    "is_some" | "is_none" => return Ok(Type::Bool),
                    _ => {}
                },
                // Result<T, E>: value/ok -> T, error/err -> E, is_ok/is_err -> Bool
                Type::Result(ok_ty, err_ty) => match field.as_str() {
                    "value" | "ok" | "unwrap" => return Ok(*ok_ty.clone()),
                    "error" | "err" => return Ok(*err_ty.clone()),
                    "is_ok" | "is_err" => return Ok(Type::Bool),
                    _ => {}
                },
                // Map: len/size -> Nat, is_empty -> Bool, keys/values
                Type::Map(key_ty, val_ty) => match field.as_str() {
                    "len" | "size" => return Ok(Type::Nat),
                    "is_empty" => return Ok(Type::Bool),
                    "keys" => return Ok(Type::List(key_ty.clone())),
                    "values" => return Ok(Type::List(val_ty.clone())),
                    _ => {}
                },
                // Set: len/size -> Nat, is_empty -> Bool
                Type::Set(_) => match field.as_str() {
                    "len" | "size" => return Ok(Type::Nat),
                    "is_empty" => return Ok(Type::Bool),
                    _ => {}
                },
                // Tuple: numeric field access (0, 1, 2, ...)
                Type::Tuple(elems) => {
                    if let Ok(idx) = field.parse::<usize>()
                        && idx < elems.len()
                    {
                        return Ok(elems[idx].clone());
                    }
                }
                _ => {}
            }
            // If the receiver type is known and concrete (struct with
            // registered fields, or a built-in type), emit A03005.
            // For Named types without registered fields we stay lenient.
            if let Some(sname) = struct_name
                && env.struct_fields.contains_key(sname)
            {
                return Err(TypeError {
                    code: "A03005".into(),
                    message: format!("unknown field `{field}` in type `{recv_ty}`"),
                    span: span.clone(),
                    secondary: None,
                });
            }
            match &recv_ty {
                Type::List(_)
                | Type::Sequence(_)
                | Type::Bytes
                | Type::Set(_)
                | Type::Option(_)
                | Type::Result(_, _)
                | Type::Map(_, _) => {
                    return Err(TypeError {
                        code: "A03005".into(),
                        message: format!("unknown field `{field}` on type `{recv_ty}`"),
                        span: span.clone(),
                        secondary: None,
                    });
                }
                _ => {}
            }
            // Suppress cascading on Error
            if recv_ty == Type::Error {
                return Ok(Type::Error);
            }
            Ok(Type::Unknown)
        }

        // --- Method call ---
        Expr::MethodCall {
            receiver,
            method,
            args,
        } => {
            let recv_ty = infer_expr_spanned(receiver, env, receiver.span.clone())?;
            // Infer argument types to surface errors inside them
            for arg in args {
                let _ = infer_expr_spanned(arg, env, arg.span.clone())?;
            }
            // Try to resolve the method as a known function in the env
            if let Some(Type::Fn { ret, .. }) = env.lookup(method) {
                return Ok(*ret.clone());
            }
            // Common collection methods with known return types
            match &recv_ty {
                Type::List(elem) | Type::Sequence(elem) => match method.as_str() {
                    "len" | "length" | "size" | "count" => return Ok(Type::Nat),
                    "get" | "first" | "last" => {
                        return Ok(Type::Option(elem.clone()));
                    }
                    "contains" | "is_empty" | "any" | "all" => return Ok(Type::Bool),
                    "push" | "append" | "remove" | "clear" => return Ok(Type::Unit),
                    "map" | "filter" | "take" | "skip" | "reverse" | "sort" => {
                        return Ok(recv_ty);
                    }
                    _ => {}
                },
                Type::Map(key, val) => match method.as_str() {
                    "get" => return Ok(Type::Option(val.clone())),
                    "contains_key" | "is_empty" => return Ok(Type::Bool),
                    "len" | "size" => return Ok(Type::Nat),
                    "keys" => return Ok(Type::Set(key.clone())),
                    "values" => return Ok(Type::List(val.clone())),
                    "insert" | "remove" | "clear" => return Ok(Type::Unit),
                    _ => {}
                },
                Type::Set(_) => match method.as_str() {
                    "contains" | "is_empty" | "is_subset" | "is_superset" | "is_disjoint" => {
                        return Ok(Type::Bool);
                    }
                    "len" | "size" => return Ok(Type::Nat),
                    "insert" | "remove" | "clear" => return Ok(Type::Unit),
                    "union" | "intersection" | "difference" | "symmetric_difference" => {
                        return Ok(recv_ty);
                    }
                    _ => {}
                },
                Type::String => match method.as_str() {
                    "len" | "length" => return Ok(Type::Nat),
                    "contains" | "starts_with" | "ends_with" | "is_empty" => {
                        return Ok(Type::Bool);
                    }
                    "to_uppercase" | "to_lowercase" | "trim" | "substring" | "replace"
                    | "concat" | "repeat" => {
                        return Ok(Type::String);
                    }
                    "split" | "chars" => return Ok(Type::List(Box::new(Type::String))),
                    "parse_int" => return Ok(Type::Option(Box::new(Type::Int))),
                    _ => {}
                },
                Type::Option(inner) => match method.as_str() {
                    "unwrap" | "unwrap_or" | "unwrap_or_default" | "expect" => {
                        return Ok(*inner.clone());
                    }
                    "is_some" | "is_none" => return Ok(Type::Bool),
                    "map" => {
                        // Option<T>.map(f) -> Option<ReturnType(f)>
                        // If arg is a known function, infer its return type.
                        let mapped_ty = infer_closure_return_from_args(args, env);
                        return Ok(Type::Option(Box::new(mapped_ty)));
                    }
                    "and_then" => {
                        // and_then returns Option<U> where U is from closure
                        let mapped_ty = infer_closure_return_from_args(args, env);
                        if let Type::Option(_) = &mapped_ty {
                            return Ok(mapped_ty);
                        }
                        return Ok(Type::Option(Box::new(mapped_ty)));
                    }
                    "or_else" => return Ok(recv_ty),
                    "filter" => return Ok(recv_ty),
                    "flatten" => {
                        // Option<Option<T>>.flatten() => Option<T>
                        if let Type::Option(inner2) = inner.as_ref() {
                            return Ok(Type::Option(inner2.clone()));
                        }
                        return Ok(recv_ty);
                    }
                    "ok_or" | "ok_or_else" => {
                        // Infer the error type from the first arg if possible
                        let err_ty = if let Some(first_arg) = args.first() {
                            infer_expr_spanned(first_arg, env, first_arg.span.clone())
                                .unwrap_or(Type::Unknown)
                        } else {
                            Type::Unknown
                        };
                        return Ok(Type::Result(inner.clone(), Box::new(err_ty)));
                    }
                    "zip" => {
                        // Option<T>.zip(Option<U>) -> Option<(T, U)>
                        let other_ty = if let Some(first_arg) = args.first() {
                            let arg_ty = infer_expr_spanned(first_arg, env, first_arg.span.clone())
                                .unwrap_or(Type::Unknown);
                            if let Type::Option(other_inner) = arg_ty {
                                *other_inner
                            } else {
                                arg_ty
                            }
                        } else {
                            Type::Unknown
                        };
                        return Ok(Type::Option(Box::new(Type::Tuple(vec![
                            *inner.clone(),
                            other_ty,
                        ]))));
                    }
                    _ => {}
                },
                Type::Bytes => match method.as_str() {
                    "len" | "length" | "size" => return Ok(Type::Nat),
                    "is_empty" => return Ok(Type::Bool),
                    "slice" => return Ok(Type::Bytes),
                    _ => {}
                },
                Type::Result(ok_ty, err_ty) => match method.as_str() {
                    "unwrap" | "unwrap_or" | "unwrap_or_default" | "expect" => {
                        return Ok(*ok_ty.clone());
                    }
                    "unwrap_err" | "expect_err" => return Ok(*err_ty.clone()),
                    "is_ok" | "is_err" => return Ok(Type::Bool),
                    "map" => {
                        // Result<T,E>.map(f) -> Result<ReturnType(f), E>
                        let mapped_ty = infer_closure_return_from_args(args, env);
                        return Ok(Type::Result(Box::new(mapped_ty), err_ty.clone()));
                    }
                    "and_then" => {
                        let mapped_ty = infer_closure_return_from_args(args, env);
                        if let Type::Result(_, _) = &mapped_ty {
                            return Ok(mapped_ty);
                        }
                        return Ok(Type::Result(Box::new(mapped_ty), err_ty.clone()));
                    }
                    "map_err" => {
                        let mapped_ty = infer_closure_return_from_args(args, env);
                        return Ok(Type::Result(ok_ty.clone(), Box::new(mapped_ty)));
                    }
                    "or_else" => {
                        return Ok(Type::Result(ok_ty.clone(), Box::new(Type::Unknown)));
                    }
                    "ok" => return Ok(Type::Option(ok_ty.clone())),
                    "err" => return Ok(Type::Option(err_ty.clone())),
                    _ => {}
                },
                _ => {}
            }
            // For Named types (user-defined), try common standard methods
            // before returning Unknown. The actual type definition may not
            // be registered, but these methods are universally available.
            if matches!(&recv_ty, Type::Named(_)) {
                match method.as_str() {
                    "len" | "length" | "size" | "count" => return Ok(Type::Nat),
                    "is_empty" | "contains" | "any" | "all" => return Ok(Type::Bool),
                    "clone" => return Ok(recv_ty),
                    "to_string" => return Ok(Type::String),
                    _ => {}
                }
            }
            // Emit A03005 for unknown methods on concrete built-in types
            match &recv_ty {
                Type::List(_)
                | Type::Sequence(_)
                | Type::String
                | Type::Bytes
                | Type::Set(_)
                | Type::Option(_)
                | Type::Result(_, _)
                | Type::Map(_, _) => {
                    return Err(TypeError {
                        code: "A03005".into(),
                        message: format!("unknown method `{method}` on type `{recv_ty}`"),
                        span: span.clone(),
                        secondary: None,
                    });
                }
                _ => {}
            }
            // For Error/Unknown receivers, suppress cascading errors
            if recv_ty.is_indeterminate() {
                return Ok(recv_ty);
            }
            // Named types: genuinely unknown, may have methods we don't know about
            Ok(Type::Unknown)
        }

        // --- Function call ---
        Expr::Call { func, args } => infer_call(func, args, env, func.span.clone()),

        // --- Index access ---
        Expr::Index { expr: base, index } => {
            let base_ty = infer_expr_spanned(base, env, base.span.clone())?;
            // Infer index type to surface errors inside it.
            let _index_ty = infer_expr_spanned(index, env, index.span.clone())?;
            match &base_ty {
                Type::List(elem) => Ok(*elem.clone()),
                Type::Map(_key, val) => Ok(*val.clone()),
                Type::Sequence(elem) => Ok(*elem.clone()),
                Type::Bytes => Ok(Type::U8),
                // Tuple indexing with literal index
                Type::Tuple(elems) => {
                    if let Expr::Literal(Literal::Int(idx_str)) = &index.as_ref().node
                        && let Ok(idx) = idx_str.parse::<usize>()
                        && idx < elems.len()
                    {
                        return Ok(elems[idx].clone());
                    }
                    // Non-literal or out-of-bounds: return Unknown
                    Ok(Type::Unknown)
                }
                // Types that cannot be indexed
                Type::Bool | Type::Unit | Type::Never | Type::Float | Type::F32 | Type::F64 => {
                    Err(TypeError {
                        code: "A03005".into(),
                        message: format!("type `{base_ty}` cannot be indexed"),
                        span: span.clone(),
                        secondary: None,
                    })
                }
                // Error: suppress cascading errors
                Type::Error => Ok(Type::Error),
                // Unknown, Named, TypeParam, or user-defined: return Unknown.
                _ => Ok(Type::Unknown),
            }
        }

        // --- Cast: infer from target type annotation ---
        Expr::Cast { expr: inner, ty } => {
            // Type-check the inner expression for side effects
            let _ = infer_expr_spanned(inner, env, inner.span.clone())?;
            // Parse the target type from the cast annotation
            Ok(parse_type_tokens(std::slice::from_ref(ty)))
        }

        // --- Apply lemma: type-check args, result is Bool (adds assumption) ---
        Expr::Apply { args, .. } => {
            for arg in args {
                let _ = infer_expr_spanned(arg, env, arg.span.clone())?;
            }
            // apply expressions contribute assumptions; they have Bool type
            // in the verification domain
            Ok(Type::Bool)
        }

        // --- Ghost block: type-check inner, result is Unit (erased at runtime) ---
        Expr::Ghost(inner) => {
            // Type-check the inner expression (it must be valid in the
            // verification domain) but the ghost block itself evaluates
            // to Unit since it is erased at runtime.
            let _inner_ty = infer_expr_spanned(inner, env, inner.span.clone())?;
            Ok(Type::Unit)
        }

        // --- Match: bind pattern variables and infer all arm types ---
        Expr::Match { scrutinee, arms } => {
            let scrut_ty = infer_expr_spanned(scrutinee, env, scrutinee.span.clone())?;
            let mut result_ty = Type::Unknown;
            for arm in arms {
                // Create a new env with pattern bindings
                let mut arm_env = env.clone();
                bind_pattern_vars(&arm.pattern, &scrut_ty, &mut arm_env);
                let arm_ty = infer_expr_spanned(&arm.body, &arm_env, arm.body.span.clone())?;
                if arm_ty.is_indeterminate() {
                    continue;
                }
                if result_ty.is_indeterminate() {
                    result_ty = arm_ty;
                } else if !types_compatible(&result_ty, &arm_ty) {
                    return Err(TypeError {
                        code: "A03001".into(),
                        message: format!(
                            "match arm type `{arm_ty}` is incompatible with \
                             previous arm type `{result_ty}`"
                        ),
                        span: span.clone(),
                        secondary: None,
                    });
                }
            }
            Ok(result_ty)
        }

        // --- Let binding: bind value type, infer body type ---
        Expr::Let { name, value, body } => {
            let val_ty = infer_expr_spanned(value, env, value.span.clone())?;
            let mut inner_env = env.clone();
            inner_env.insert(name.clone(), val_ty);
            infer_expr_spanned(body, &inner_env, body.span.clone())
        }

        // --- Tuple: infer element types ---
        Expr::Tuple(elems) => {
            let mut elem_types = Vec::with_capacity(elems.len());
            for elem in elems {
                elem_types.push(infer_expr_spanned(elem, env, elem.span.clone())?);
            }
            Ok(Type::Tuple(elem_types))
        }

        // --- Block: infer type of last expression ---
        Expr::Block(exprs) => {
            let mut last_ty = Type::Unit;
            for e in exprs {
                last_ty = infer_expr_spanned(e, env, e.span.clone())?;
            }
            Ok(last_ty)
        }

        // --- Raw: cannot infer from unparsed token sequence ---
        Expr::Raw(_) => Ok(Type::Error),
    }
}

/// Check if two types are compatible for comparison/arithmetic purposes.
///
/// Types are compatible if:
/// - They are equal
/// - Either side is `Unknown`
/// - Either side is a `Named` type (user-defined, not yet resolved)
/// - A `Refined` type's base matches the other type
/// - Both are numeric
pub(crate) fn types_compatible(a: &Type, b: &Type) -> bool {
    if a == b {
        return true;
    }
    if a.is_indeterminate() || b.is_indeterminate() {
        return true;
    }
    // Named types are unresolved user-defined; be lenient
    if matches!(a, Type::Named(_)) || matches!(b, Type::Named(_)) {
        return true;
    }
    // Refined types are compatible with their base type
    if let Type::Refined { base, .. } = a {
        return types_compatible(base, b);
    }
    if let Type::Refined { base, .. } = b {
        return types_compatible(a, base);
    }
    // TypeParams are unresolved; be lenient
    if matches!(a, Type::TypeParam(_)) || matches!(b, Type::TypeParam(_)) {
        return true;
    }
    // Nat is a subtype of Int; they are compatible in arithmetic/comparison
    if (matches!(a, Type::Nat) && matches!(b, Type::Int))
        || (matches!(a, Type::Int) && matches!(b, Type::Nat))
    {
        return true;
    }
    // Both numeric types are compatible (e.g., U32 vs Int in mixed arithmetic)
    if is_numeric(a) && is_numeric(b) {
        return true;
    }
    // Tuple types are compatible if they have the same arity and element types match
    if let (Type::Tuple(a_elems), Type::Tuple(b_elems)) = (a, b) {
        return a_elems.len() == b_elems.len()
            && a_elems
                .iter()
                .zip(b_elems.iter())
                .all(|(ea, eb)| types_compatible(ea, eb));
    }
    false
}

/// Infer the result type of a binary operation.
fn infer_binop(
    lhs: &SpExpr,
    op: &BinOp,
    rhs: &SpExpr,
    env: &TypeEnv,
    span: Span,
) -> Result<Type, TypeError> {
    let lhs_ty = infer_expr_spanned(lhs, env, lhs.span.clone())?;
    let rhs_ty = infer_expr_spanned(rhs, env, rhs.span.clone())?;

    // If either side is indeterminate, be lenient
    if lhs_ty.is_indeterminate() || rhs_ty.is_indeterminate() {
        return match op {
            _ if op.is_arithmetic() || *op == BinOp::Concat => {
                // Return whichever side is known, or Unknown
                if !lhs_ty.is_indeterminate() {
                    Ok(lhs_ty)
                } else if !rhs_ty.is_indeterminate() {
                    Ok(rhs_ty)
                } else {
                    Ok(Type::Unknown)
                }
            }
            _ if op.is_comparison() || op.is_logical() || op.is_membership() => Ok(Type::Bool),
            BinOp::Range => Ok(Type::List(Box::new(Type::Int))),
            _ => unreachable!("BinOp should be covered by guards or Range"),
        };
    }

    match op {
        // Arithmetic: both operands same numeric type, result same type
        _ if op.is_arithmetic() => {
            // Division/modulo by literal zero
            if op.is_division_like() && is_literal_zero(rhs) {
                return Err(TypeError {
                    code: "A03010".into(),
                    message: format!(
                        "{} by zero",
                        if *op == BinOp::Div {
                            "division"
                        } else {
                            "modulo"
                        }
                    ),
                    span: span.clone(),
                    secondary: None,
                });
            }
            if !is_numeric(&lhs_ty) {
                return Err(TypeError {
                    code: "A03001".into(),
                    message: format!(
                        "arithmetic operator requires numeric types, found `{lhs_ty}`"
                    ),
                    span: lhs.span.clone(),
                    secondary: None,
                });
            }
            if !types_compatible(&lhs_ty, &rhs_ty) {
                // Prefer the rhs span for the "bad" operand in mismatch (for precise
                // sub-expr diagnostics per 11.04/333).
                return Err(TypeError {
                    code: "A03001".into(),
                    message: format!("type mismatch in arithmetic: `{lhs_ty}` vs `{rhs_ty}`"),
                    span: rhs.span.clone(),
                    secondary: None,
                });
            }
            Ok(lhs_ty)
        }

        // Comparison: operands compatible types, result Bool
        _ if op.is_comparison() => {
            if !types_compatible(&lhs_ty, &rhs_ty) {
                return Err(TypeError {
                    code: "A03001".into(),
                    message: format!(
                        "comparison requires same types, found `{lhs_ty}` vs `{rhs_ty}`"
                    ),
                    span: rhs.span.clone(),
                    secondary: None,
                });
            }
            Ok(Type::Bool)
        }

        // Logical: both Bool, result Bool
        _ if op.is_logical() => {
            if lhs_ty != Type::Bool {
                return Err(TypeError {
                    code: "A03001".into(),
                    message: format!("logical operator requires Bool, found `{lhs_ty}`"),
                    span: span.clone(),
                    secondary: None,
                });
            }
            if rhs_ty != Type::Bool {
                return Err(TypeError {
                    code: "A03001".into(),
                    message: format!("logical operator requires Bool, found `{rhs_ty}`"),
                    span: span.clone(),
                    secondary: None,
                });
            }
            Ok(Type::Bool)
        }

        // Concat: both same type or compatible (String, List, Bytes), result same type
        BinOp::Concat => {
            if types_compatible(&lhs_ty, &rhs_ty) {
                Ok(lhs_ty)
            } else {
                Err(TypeError {
                    code: "A03001".into(),
                    message: format!(
                        "concat requires compatible types, found `{lhs_ty}` vs `{rhs_ty}`"
                    ),
                    span: span.clone(),
                    secondary: None,
                })
            }
        }

        // Range: both Int/Nat, result is a List<Int> (iterable range)
        BinOp::Range => {
            if !is_numeric(&lhs_ty) {
                return Err(TypeError {
                    code: "A03001".into(),
                    message: format!("range requires numeric operands, found `{lhs_ty}`"),
                    span: span.clone(),
                    secondary: None,
                });
            }
            if !is_numeric(&rhs_ty) {
                return Err(TypeError {
                    code: "A03001".into(),
                    message: format!("range requires numeric operands, found `{rhs_ty}`"),
                    span: span.clone(),
                    secondary: None,
                });
            }
            Ok(Type::List(Box::new(Type::Int)))
        }

        // In/NotIn: rhs should be a collection type, result Bool
        BinOp::In | BinOp::NotIn => {
            match &rhs_ty {
                Type::List(_)
                | Type::Set(_)
                | Type::Sequence(_)
                | Type::Map(_, _)
                | Type::String
                | Type::Named(_)
                | Type::Unknown
                | Type::Error => {}
                _ => {
                    return Err(TypeError {
                        code: "A03001".into(),
                        message: format!(
                            "`in` requires a collection on the right side, found `{rhs_ty}`"
                        ),
                        span: span.clone(),
                        secondary: None,
                    });
                }
            }
            Ok(Type::Bool)
        }

        _ => unreachable!(
            "all BinOp variants should be handled by category guards or specific arms: {:?}",
            op
        ),
    }
}

/// Infer the result type of a function call expression.
fn infer_call(
    func: &SpExpr,
    args: &[SpExpr],
    env: &TypeEnv,
    span: Span,
) -> Result<Type, TypeError> {
    let func_ty = infer_expr_spanned(func, env, func.span.clone())?;

    // Infer argument types eagerly so errors inside arguments are surfaced
    // even when the callee type is Unknown.
    let mut arg_types = Vec::with_capacity(args.len());
    for arg in args {
        arg_types.push(infer_expr_spanned(arg, env, arg.span.clone())?);
    }

    match func_ty {
        Type::Fn { params, ret } => {
            // If params is non-empty, check argument count.
            // (params may be empty when the function was registered with
            // placeholder params from the symbol table.)
            if !params.is_empty() && params.len() != arg_types.len() {
                return Err(TypeError {
                    code: "A03002".into(),
                    message: format!(
                        "function expects {} argument(s), but {} were provided",
                        params.len(),
                        arg_types.len()
                    ),
                    span: span.clone(),
                    secondary: None,
                });
            }
            Ok(*ret)
        }
        // Unknown callee: try to infer from function name or argument types.
        Type::Unknown => {
            if let Expr::Ident(name) = &func.node
                && let Some(ty) = infer_builtin_call_type(name, &arg_types)
            {
                return Ok(ty);
            }
            Ok(Type::Unknown)
        }
        // Error: suppress cascading
        Type::Error => Ok(Type::Error),
        // Named type: could be a constructor. Return the Named type itself.
        Type::Named(name) => Ok(Type::Named(name)),
        // TypeParam: calling a type param (e.g. `T(args)`) is a
        // constructor-style pattern; return the type param itself.
        Type::TypeParam(name) => Ok(Type::TypeParam(name)),
        // Definitely not callable.
        other => Err(TypeError {
            code: "A03005".into(),
            message: format!("type `{other}` is not callable"),
            span: span.clone(),
            secondary: None,
        }),
    }
}

/// Attempt to infer a return type from closure/function arguments passed
/// to higher-order methods like `map`, `and_then`, etc.
///
/// If the first argument is a known function name in the environment, return
/// its declared return type. Otherwise return `Type::Unknown`.
fn infer_closure_return_from_args(args: &[SpExpr], env: &TypeEnv) -> Type {
    if let Some(first_arg) = args.first() {
        // If the argument is an identifier that resolves to a function, use its return type
        if let Expr::Ident(name) = &first_arg.node
            && let Some(Type::Fn { ret, .. }) = env.lookup(name)
        {
            return *ret.clone();
        }
        // Try inferring the expression type
        if let Ok(Type::Fn { ret, .. }) = infer_expr(first_arg, env) {
            return *ret;
        }
    }
    Type::Unknown
}

/// Infer the return type of a well-known builtin function call.
fn infer_builtin_call_type(name: &str, arg_types: &[Type]) -> Option<Type> {
    match name {
        // Collection operations
        "len" | "length" | "size" | "count" => Some(Type::Nat),
        // Type predicates
        "is_empty" | "contains" | "is_valid" | "is_some" | "is_none" | "is_ok" | "is_err" => {
            Some(Type::Bool)
        }
        // Numeric
        "abs" => arg_types.first().cloned(),
        "min" | "max" => arg_types.first().cloned(),
        "sqrt" => Some(Type::Float),
        "floor" | "ceil" | "round" => arg_types.first().cloned(),
        "to_string" | "to_str" => Some(Type::String),
        "to_int" | "parse_int" => Some(Type::Int),
        // Option/Result unwrapping
        "unwrap" => {
            if let Some(Type::Option(inner)) = arg_types.first() {
                Some(*inner.clone())
            } else if let Some(Type::Result(ok, _)) = arg_types.first() {
                Some(*ok.clone())
            } else {
                None
            }
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use assura_parser::ast::{BinOp, Expr, Literal, SpExpr, Spanned, UnaryOp};

    fn mk_int(s: &str) -> SpExpr {
        Spanned::no_span(Expr::Literal(Literal::Int(s.into())))
    }

    fn mk_float(s: &str) -> SpExpr {
        Spanned::no_span(Expr::Literal(Literal::Float(s.into())))
    }

    fn mk_bool(b: bool) -> SpExpr {
        Spanned::no_span(Expr::Literal(Literal::Bool(b)))
    }

    fn mk_str(s: &str) -> SpExpr {
        Spanned::no_span(Expr::Literal(Literal::Str(s.into())))
    }

    fn mk_ident(s: &str) -> SpExpr {
        Spanned::no_span(Expr::Ident(s.into()))
    }

    fn mk_binop(lhs: SpExpr, op: BinOp, rhs: SpExpr) -> SpExpr {
        Spanned::no_span(Expr::BinOp {
            lhs: Box::new(lhs),
            op,
            rhs: Box::new(rhs),
        })
    }

    // --- Literal inference ---

    #[test]
    fn literal_int() {
        assert_eq!(
            infer_expr(&mk_int("42"), &TypeEnv::new()).unwrap(),
            Type::Int
        );
    }

    #[test]
    fn literal_float() {
        assert_eq!(
            infer_expr(&mk_float("3.14"), &TypeEnv::new()).unwrap(),
            Type::Float
        );
    }

    #[test]
    fn literal_bool() {
        assert_eq!(
            infer_expr(&mk_bool(true), &TypeEnv::new()).unwrap(),
            Type::Bool
        );
    }

    #[test]
    fn literal_string() {
        assert_eq!(
            infer_expr(&mk_str("hello"), &TypeEnv::new()).unwrap(),
            Type::String
        );
    }

    // --- Identifier inference ---

    #[test]
    fn ident_known() {
        let mut env = TypeEnv::new();
        env.insert("x".into(), Type::Int);
        assert_eq!(infer_expr(&mk_ident("x"), &env).unwrap(), Type::Int);
    }

    #[test]
    fn ident_unknown_returns_unknown() {
        assert_eq!(
            infer_expr(&mk_ident("missing"), &TypeEnv::new()).unwrap(),
            Type::Unknown
        );
    }

    #[test]
    fn ident_true_is_bool() {
        assert_eq!(
            infer_expr(&mk_ident("true"), &TypeEnv::new()).unwrap(),
            Type::Bool
        );
    }

    #[test]
    fn ident_false_is_bool() {
        assert_eq!(
            infer_expr(&mk_ident("false"), &TypeEnv::new()).unwrap(),
            Type::Bool
        );
    }

    // --- Binary operator inference ---

    #[test]
    fn binop_add_int() {
        let expr = mk_binop(mk_int("1"), BinOp::Add, mk_int("2"));
        assert_eq!(infer_expr(&expr, &TypeEnv::new()).unwrap(), Type::Int);
    }

    #[test]
    fn binop_add_float() {
        let expr = mk_binop(mk_float("1.0"), BinOp::Add, mk_float("2.0"));
        assert_eq!(infer_expr(&expr, &TypeEnv::new()).unwrap(), Type::Float);
    }

    #[test]
    fn binop_comparison_returns_bool() {
        let expr = mk_binop(mk_int("1"), BinOp::Gt, mk_int("2"));
        assert_eq!(infer_expr(&expr, &TypeEnv::new()).unwrap(), Type::Bool);
    }

    #[test]
    fn binop_equality_returns_bool() {
        let expr = mk_binop(mk_int("1"), BinOp::Eq, mk_int("2"));
        assert_eq!(infer_expr(&expr, &TypeEnv::new()).unwrap(), Type::Bool);
    }

    #[test]
    fn binop_and_returns_bool() {
        let expr = mk_binop(mk_bool(true), BinOp::And, mk_bool(false));
        assert_eq!(infer_expr(&expr, &TypeEnv::new()).unwrap(), Type::Bool);
    }

    #[test]
    fn binop_or_returns_bool() {
        let expr = mk_binop(mk_bool(true), BinOp::Or, mk_bool(false));
        assert_eq!(infer_expr(&expr, &TypeEnv::new()).unwrap(), Type::Bool);
    }

    #[test]
    fn binop_type_mismatch_a03001() {
        let expr = mk_binop(mk_int("1"), BinOp::Add, mk_str("hello"));
        let err = infer_expr(&expr, &TypeEnv::new()).unwrap_err();
        assert_eq!(err.code, "A03001");
    }

    // --- Unary operator inference ---

    #[test]
    fn unary_neg_int() {
        let expr = Spanned::no_span(Expr::UnaryOp {
            op: UnaryOp::Neg,
            expr: Box::new(mk_int("5")),
        });
        assert_eq!(infer_expr(&expr, &TypeEnv::new()).unwrap(), Type::Int);
    }

    #[test]
    fn unary_not_bool() {
        let expr = Spanned::no_span(Expr::UnaryOp {
            op: UnaryOp::Not,
            expr: Box::new(mk_bool(true)),
        });
        assert_eq!(infer_expr(&expr, &TypeEnv::new()).unwrap(), Type::Bool);
    }

    #[test]
    fn unary_neg_string_error() {
        let expr = Spanned::no_span(Expr::UnaryOp {
            op: UnaryOp::Neg,
            expr: Box::new(mk_str("hello")),
        });
        let err = infer_expr(&expr, &TypeEnv::new()).unwrap_err();
        assert_eq!(err.code, "A03001");
    }

    #[test]
    fn unary_not_int_error() {
        let expr = Spanned::no_span(Expr::UnaryOp {
            op: UnaryOp::Not,
            expr: Box::new(mk_int("5")),
        });
        let err = infer_expr(&expr, &TypeEnv::new()).unwrap_err();
        assert_eq!(err.code, "A03001");
    }

    // --- Paren ---

    #[test]
    fn int_literal_type() {
        let expr = mk_int("1");
        assert_eq!(infer_expr(&expr, &TypeEnv::new()).unwrap(), Type::Int);
    }

    // --- If-then-else ---

    #[test]
    fn if_then_else_matching_branches() {
        let expr = Spanned::no_span(Expr::If {
            cond: Box::new(mk_bool(true)),
            then_branch: Box::new(mk_int("1")),
            else_branch: Some(Box::new(mk_int("2"))),
        });
        assert_eq!(infer_expr(&expr, &TypeEnv::new()).unwrap(), Type::Int);
    }

    // --- Helper functions ---

    #[test]
    fn is_literal_zero_int_zero() {
        assert!(is_literal_zero(&mk_int("0")));
    }

    #[test]
    fn is_literal_zero_int_nonzero() {
        assert!(!is_literal_zero(&mk_int("1")));
    }

    #[test]
    fn is_literal_zero_float_zero() {
        assert!(is_literal_zero(&mk_float("0.0")));
    }

    #[test]
    fn is_numeric_basic_types() {
        assert!(is_numeric(&Type::Int));
        assert!(is_numeric(&Type::Nat));
        assert!(is_numeric(&Type::Float));
        assert!(is_numeric(&Type::U32));
        assert!(is_numeric(&Type::I64));
    }

    #[test]
    fn is_numeric_non_numeric() {
        assert!(!is_numeric(&Type::Bool));
        assert!(!is_numeric(&Type::String));
        assert!(!is_numeric(&Type::Unit));
    }

    #[test]
    fn is_numeric_refined_base() {
        let ty = Type::Refined {
            base: Box::new(Type::Int),
            predicate: "x > 0".into(),
        };
        assert!(is_numeric(&ty));
    }

    #[test]
    fn element_type_of_list() {
        assert_eq!(element_type_of(&Type::List(Box::new(Type::Int))), Type::Int);
    }

    #[test]
    fn element_type_of_set() {
        assert_eq!(
            element_type_of(&Type::Set(Box::new(Type::String))),
            Type::String
        );
    }

    #[test]
    fn element_type_of_map_returns_key() {
        assert_eq!(
            element_type_of(&Type::Map(Box::new(Type::String), Box::new(Type::Int))),
            Type::String
        );
    }

    #[test]
    fn element_type_of_int_range() {
        assert_eq!(element_type_of(&Type::Int), Type::Int);
    }

    #[test]
    fn element_type_of_unknown() {
        assert_eq!(element_type_of(&Type::Bool), Type::Unknown);
    }

    // --- Method call type inference ---

    #[test]
    fn method_call_len_on_list() {
        let mut env = TypeEnv::new();
        env.insert("xs".into(), Type::List(Box::new(Type::Int)));
        let expr = Spanned::no_span(Expr::MethodCall {
            receiver: Box::new(mk_ident("xs")),
            method: "len".into(),
            args: vec![],
        });
        assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Nat);
    }

    #[test]
    fn method_call_contains_on_list() {
        let mut env = TypeEnv::new();
        env.insert("xs".into(), Type::List(Box::new(Type::Int)));
        let expr = Spanned::no_span(Expr::MethodCall {
            receiver: Box::new(mk_ident("xs")),
            method: "contains".into(),
            args: vec![mk_int("1")],
        });
        assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Bool);
    }

    #[test]
    fn method_call_on_unknown_returns_unknown() {
        let expr = Spanned::no_span(Expr::MethodCall {
            receiver: Box::new(mk_ident("unknown_var")),
            method: "foo".into(),
            args: vec![],
        });
        assert_eq!(infer_expr(&expr, &TypeEnv::new()).unwrap(), Type::Unknown);
    }
}
