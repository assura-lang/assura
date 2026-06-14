//! Expression type inference.
//!
//! Implements `infer_expr` which infers the type of an Assura expression
//! in a given type environment. Covers literals, variables, field access,
//! binary/unary operations, function calls, quantifiers, and more.

use assura_parser::ast::{BinOp, Expr, Literal, UnaryOp};

use crate::clauses::bind_pattern_vars;
use crate::{Type, TypeEnv, TypeError, parse_type_tokens};

/// Infer the type of an `HirExpr` by converting to AST `Expr` first.
/// This is a bridge during the HIR migration; eventually `infer_expr`
/// will be rewritten to accept `HirExpr` natively.
pub fn infer_hir_expr(hir_expr: &assura_hir::HirExpr, env: &TypeEnv) -> Result<Type, TypeError> {
    let ast_expr = hir_expr.to_ast_expr();
    infer_expr(&ast_expr, env)
}

// ---------------------------------------------------------------------------
// Expression type inference
// ---------------------------------------------------------------------------

/// Returns `true` if `ty` is a numeric type.
/// Check if an expression is a literal zero (integer 0 or float 0.0).
pub(crate) fn is_literal_zero(expr: &Expr) -> bool {
    match expr {
        Expr::Literal(Literal::Int(s)) => s == "0",
        Expr::Literal(Literal::Float(s)) => s == "0.0" || s == "0",
        Expr::Paren(inner) => is_literal_zero(inner),
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
        // Named types may be numeric aliases; be lenient
        Type::Named(_) | Type::Unknown => true,
        _ => false,
    }
}

/// Infer the type of an expression given a type environment.
///
/// Returns `Ok(ty)` with the inferred type, or `Err(TypeError)` when a
/// concrete type mismatch is detected (A03001). Unknown types (from
/// unresolved references) are propagated silently; they never trigger
/// errors.
pub fn infer_expr(expr: &Expr, env: &TypeEnv) -> Result<Type, TypeError> {
    match expr {
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
        Expr::BinOp { lhs, op, rhs } => infer_binop(lhs, op, rhs, env),

        // --- Unary operations ---
        Expr::UnaryOp { op, expr: inner } => {
            let inner_ty = infer_expr(inner, env)?;
            match op {
                UnaryOp::Neg => {
                    if inner_ty == Type::Unknown || is_numeric(&inner_ty) {
                        Ok(inner_ty)
                    } else {
                        Err(TypeError {
                            code: "A03001".into(),
                            message: format!(
                                "unary `-` requires a numeric type, found `{inner_ty}`"
                            ),
                            span: 0..0,
                            secondary: None,
                        })
                    }
                }
                UnaryOp::Not => {
                    if inner_ty == Type::Unknown || inner_ty == Type::Bool {
                        Ok(Type::Bool)
                    } else {
                        Err(TypeError {
                            code: "A03001".into(),
                            message: format!("unary `!` requires Bool, found `{inner_ty}`"),
                            span: 0..0,
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
            let cond_ty = infer_expr(cond, env)?;
            if cond_ty != Type::Unknown && cond_ty != Type::Bool {
                return Err(TypeError {
                    code: "A03001".into(),
                    message: format!("if condition must be Bool, found `{cond_ty}`"),
                    span: 0..0,
                    secondary: None,
                });
            }
            let then_ty = infer_expr(then_branch, env)?;
            if let Some(else_br) = else_branch {
                let else_ty = infer_expr(else_br, env)?;
                if then_ty == Type::Unknown {
                    Ok(else_ty)
                } else if else_ty == Type::Unknown || types_compatible(&then_ty, &else_ty) {
                    Ok(then_ty)
                } else {
                    Err(TypeError {
                        code: "A03001".into(),
                        message: format!(
                            "if branches have different types: `{then_ty}` vs `{else_ty}`"
                        ),
                        span: 0..0,
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
            let domain_ty = infer_expr(domain, env)?;
            let elem_ty = element_type_of(&domain_ty);
            // Bind the quantified variable in a child environment so the
            // body can reference it with the correct type.
            let mut child_env = env.clone();
            child_env.insert(var.clone(), elem_ty);
            let _body_ty = infer_expr(body, &child_env)?;
            Ok(Type::Bool)
        }

        // --- old(expr) ---
        Expr::Old(inner) => infer_expr(inner, env),

        // --- Parenthesized ---
        Expr::Paren(inner) => infer_expr(inner, env),

        // --- List literal ---
        Expr::List(items) => {
            if items.is_empty() {
                return Ok(Type::List(Box::new(Type::Unknown)));
            }
            let first_ty = infer_expr(&items[0], env)?;
            // Check remaining items match the first
            for item in &items[1..] {
                let item_ty = infer_expr(item, env)?;
                if item_ty != Type::Unknown && first_ty != Type::Unknown && item_ty != first_ty {
                    return Err(TypeError {
                        code: "A03001".into(),
                        message: format!(
                            "list element type mismatch: expected `{first_ty}`, found `{item_ty}`"
                        ),
                        span: 0..0,
                        secondary: None,
                    });
                }
            }
            Ok(Type::List(Box::new(first_ty)))
        }

        // --- Field access ---
        Expr::Field(receiver, field) => {
            let recv_ty = infer_expr(receiver, env)?;
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
                    span: 0..0,
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
                        span: 0..0,
                        secondary: None,
                    });
                }
                _ => {}
            }
            Ok(Type::Unknown)
        }

        // --- Method call ---
        Expr::MethodCall {
            receiver,
            method,
            args,
        } => {
            let recv_ty = infer_expr(receiver, env)?;
            // Infer argument types to surface errors inside them
            for arg in args {
                let _ = infer_expr(arg, env)?;
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
                    "map" | "and_then" | "or_else" | "filter" => {
                        return Ok(Type::Option(Box::new(Type::Unknown)));
                    }
                    "flatten" => {
                        // Option<Option<T>>.flatten() => Option<T>
                        if let Type::Option(inner2) = inner.as_ref() {
                            return Ok(Type::Option(inner2.clone()));
                        }
                        return Ok(Type::Option(Box::new(Type::Unknown)));
                    }
                    "ok_or" | "ok_or_else" => {
                        return Ok(Type::Result(inner.clone(), Box::new(Type::Unknown)));
                    }
                    "zip" => return Ok(Type::Option(Box::new(Type::Unknown))),
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
                    "map" | "and_then" => {
                        return Ok(Type::Result(Box::new(Type::Unknown), err_ty.clone()));
                    }
                    "map_err" | "or_else" => {
                        return Ok(Type::Result(ok_ty.clone(), Box::new(Type::Unknown)));
                    }
                    "ok" => return Ok(Type::Option(ok_ty.clone())),
                    "err" => return Ok(Type::Option(err_ty.clone())),
                    _ => {}
                },
                _ => {}
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
                        span: 0..0,
                        secondary: None,
                    });
                }
                _ => {}
            }
            Ok(Type::Unknown)
        }

        // --- Function call ---
        Expr::Call { func, args } => infer_call(func, args, env),

        // --- Index access ---
        Expr::Index { expr: base, index } => {
            let base_ty = infer_expr(base, env)?;
            // Infer index type to surface errors inside it.
            let _index_ty = infer_expr(index, env)?;
            match &base_ty {
                Type::List(elem) => Ok(*elem.clone()),
                Type::Map(_key, val) => Ok(*val.clone()),
                Type::Sequence(elem) => Ok(*elem.clone()),
                Type::Bytes => Ok(Type::U8),
                // Tuple indexing with literal index
                Type::Tuple(elems) => {
                    if let Expr::Literal(Literal::Int(idx_str)) = index.as_ref()
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
                        span: 0..0,
                        secondary: None,
                    })
                }
                // Unknown, Named, TypeParam, or user-defined: return Unknown.
                _ => Ok(Type::Unknown),
            }
        }

        // --- Cast: infer from target type annotation ---
        Expr::Cast { expr: inner, ty } => {
            // Type-check the inner expression for side effects
            let _ = infer_expr(inner, env)?;
            // Parse the target type from the cast annotation
            Ok(parse_type_tokens(std::slice::from_ref(ty)))
        }

        // --- Apply lemma: type-check args, result is Bool (adds assumption) ---
        Expr::Apply { args, .. } => {
            for arg in args {
                let _ = infer_expr(arg, env)?;
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
            let _inner_ty = infer_expr(inner, env)?;
            Ok(Type::Unit)
        }

        // --- Match: bind pattern variables and infer all arm types ---
        Expr::Match { scrutinee, arms } => {
            let scrut_ty = infer_expr(scrutinee, env)?;
            let mut result_ty = Type::Unknown;
            for arm in arms {
                // Create a new env with pattern bindings
                let mut arm_env = env.clone();
                bind_pattern_vars(&arm.pattern, &scrut_ty, &mut arm_env);
                let arm_ty = infer_expr(&arm.body, &arm_env)?;
                if arm_ty == Type::Unknown {
                    continue;
                }
                if result_ty == Type::Unknown {
                    result_ty = arm_ty;
                } else if !types_compatible(&result_ty, &arm_ty) {
                    return Err(TypeError {
                        code: "A03001".into(),
                        message: format!(
                            "match arm type `{arm_ty}` is incompatible with \
                             previous arm type `{result_ty}`"
                        ),
                        span: 0..0,
                        secondary: None,
                    });
                }
            }
            Ok(result_ty)
        }

        // --- Let binding: bind value type, infer body type ---
        Expr::Let { name, value, body } => {
            let val_ty = infer_expr(value, env)?;
            let mut inner_env = env.clone();
            inner_env.insert(name.clone(), val_ty);
            infer_expr(body, &inner_env)
        }

        // --- Tuple: infer element types ---
        Expr::Tuple(elems) => {
            let mut elem_types = Vec::with_capacity(elems.len());
            for elem in elems {
                elem_types.push(infer_expr(elem, env)?);
            }
            Ok(Type::Tuple(elem_types))
        }

        // --- Block: infer type of last expression ---
        Expr::Block(exprs) => {
            let mut last_ty = Type::Unknown;
            for e in exprs {
                last_ty = infer_expr(e, env)?;
            }
            Ok(last_ty)
        }

        // --- Raw: cannot infer from token sequence ---
        Expr::Raw(_) => Ok(Type::Unknown),
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
    if *a == Type::Unknown || *b == Type::Unknown {
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
fn infer_binop(lhs: &Expr, op: &BinOp, rhs: &Expr, env: &TypeEnv) -> Result<Type, TypeError> {
    let lhs_ty = infer_expr(lhs, env)?;
    let rhs_ty = infer_expr(rhs, env)?;

    // If either side is Unknown, be lenient
    if lhs_ty == Type::Unknown || rhs_ty == Type::Unknown {
        return match op {
            BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Mod | BinOp::Concat => {
                // Return whichever side is known, or Unknown
                if lhs_ty != Type::Unknown {
                    Ok(lhs_ty)
                } else {
                    Ok(rhs_ty)
                }
            }
            BinOp::Eq
            | BinOp::Neq
            | BinOp::Lt
            | BinOp::Lte
            | BinOp::Gt
            | BinOp::Gte
            | BinOp::And
            | BinOp::Or
            | BinOp::Implies
            | BinOp::In
            | BinOp::NotIn => Ok(Type::Bool),
            BinOp::Range => Ok(Type::List(Box::new(Type::Int))),
        };
    }

    match op {
        // Arithmetic: both operands same numeric type, result same type
        BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Mod => {
            // Division/modulo by literal zero
            if matches!(op, BinOp::Div | BinOp::Mod) && is_literal_zero(rhs) {
                return Err(TypeError {
                    code: "A03010".into(),
                    message: format!(
                        "{} by zero",
                        if matches!(op, BinOp::Div) {
                            "division"
                        } else {
                            "modulo"
                        }
                    ),
                    span: 0..0,
                    secondary: None,
                });
            }
            if !is_numeric(&lhs_ty) {
                return Err(TypeError {
                    code: "A03001".into(),
                    message: format!(
                        "arithmetic operator requires numeric types, found `{lhs_ty}`"
                    ),
                    span: 0..0,
                    secondary: None,
                });
            }
            if !types_compatible(&lhs_ty, &rhs_ty) {
                return Err(TypeError {
                    code: "A03001".into(),
                    message: format!("type mismatch in arithmetic: `{lhs_ty}` vs `{rhs_ty}`"),
                    span: 0..0,
                    secondary: None,
                });
            }
            Ok(lhs_ty)
        }

        // Comparison: operands compatible types, result Bool
        BinOp::Eq | BinOp::Neq | BinOp::Lt | BinOp::Lte | BinOp::Gt | BinOp::Gte => {
            if !types_compatible(&lhs_ty, &rhs_ty) {
                return Err(TypeError {
                    code: "A03001".into(),
                    message: format!(
                        "comparison requires same types, found `{lhs_ty}` vs `{rhs_ty}`"
                    ),
                    span: 0..0,
                    secondary: None,
                });
            }
            Ok(Type::Bool)
        }

        // Logical: both Bool, result Bool
        BinOp::And | BinOp::Or | BinOp::Implies => {
            if lhs_ty != Type::Bool {
                return Err(TypeError {
                    code: "A03001".into(),
                    message: format!("logical operator requires Bool, found `{lhs_ty}`"),
                    span: 0..0,
                    secondary: None,
                });
            }
            if rhs_ty != Type::Bool {
                return Err(TypeError {
                    code: "A03001".into(),
                    message: format!("logical operator requires Bool, found `{rhs_ty}`"),
                    span: 0..0,
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
                    span: 0..0,
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
                    span: 0..0,
                    secondary: None,
                });
            }
            if !is_numeric(&rhs_ty) {
                return Err(TypeError {
                    code: "A03001".into(),
                    message: format!("range requires numeric operands, found `{rhs_ty}`"),
                    span: 0..0,
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
                | Type::Unknown => {}
                _ => {
                    return Err(TypeError {
                        code: "A03001".into(),
                        message: format!(
                            "`in` requires a collection on the right side, found `{rhs_ty}`"
                        ),
                        span: 0..0,
                        secondary: None,
                    });
                }
            }
            Ok(Type::Bool)
        }
    }
}

/// Infer the result type of a function call expression.
fn infer_call(func: &Expr, args: &[Expr], env: &TypeEnv) -> Result<Type, TypeError> {
    let func_ty = infer_expr(func, env)?;

    // Infer argument types eagerly so errors inside arguments are surfaced
    // even when the callee type is Unknown.
    let mut arg_types = Vec::with_capacity(args.len());
    for arg in args {
        arg_types.push(infer_expr(arg, env)?);
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
                    span: 0..0,
                    secondary: None,
                });
            }
            Ok(*ret)
        }
        // Unknown callee: try to infer from function name or argument types.
        Type::Unknown => {
            if let Expr::Ident(name) = func
                && let Some(ty) = infer_builtin_call_type(name, &arg_types)
            {
                return Ok(ty);
            }
            Ok(Type::Unknown)
        }
        // Named type: could be a constructor. Return the Named type itself.
        Type::Named(name) => Ok(Type::Named(name)),
        Type::TypeParam(_) => Ok(Type::Unknown),
        // Definitely not callable.
        other => Err(TypeError {
            code: "A03005".into(),
            message: format!("type `{other}` is not callable"),
            span: 0..0,
            secondary: None,
        }),
    }
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
