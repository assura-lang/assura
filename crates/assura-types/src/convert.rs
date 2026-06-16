//! Type conversion functions.
//!
//! Converts between AST TypeExpr, HIR HirType, raw token sequences,
//! and the type checkers Type enum.

use crate::Type;
use crate::types::builtin_type;

// ---------------------------------------------------------------------------
// TypeExpr -> Type conversion
// ---------------------------------------------------------------------------

/// Convert a structured `TypeExpr` (from the parser) to a type-checker `Type`.
///
/// This provides a cleaner, faster path than re-parsing raw tokens.
pub(crate) fn type_from_expr(expr: &assura_parser::ast::TypeExpr) -> Type {
    use assura_parser::ast::TypeExpr;
    match expr {
        TypeExpr::Unit => Type::Unit,
        TypeExpr::Named(name) => builtin_type(name).unwrap_or_else(|| Type::Named(name.clone())),
        TypeExpr::Generic(name, args) => {
            let type_args: Vec<Type> = args.iter().map(type_from_expr).collect();
            match name.as_str() {
                "List" | "Vec" => Type::List(Box::new(
                    type_args.into_iter().next().unwrap_or(Type::Unknown),
                )),
                "Sequence" => Type::Sequence(Box::new(
                    type_args.into_iter().next().unwrap_or(Type::Unknown),
                )),
                "Set" => Type::Set(Box::new(
                    type_args.into_iter().next().unwrap_or(Type::Unknown),
                )),
                "Option" => Type::Option(Box::new(
                    type_args.into_iter().next().unwrap_or(Type::Unknown),
                )),
                "Map" => {
                    let mut it = type_args.into_iter();
                    Type::Map(
                        Box::new(it.next().unwrap_or(Type::Unknown)),
                        Box::new(it.next().unwrap_or(Type::Unknown)),
                    )
                }
                "Result" => {
                    let mut it = type_args.into_iter();
                    Type::Result(
                        Box::new(it.next().unwrap_or(Type::Unknown)),
                        Box::new(it.next().unwrap_or(Type::Unknown)),
                    )
                }
                _ => Type::Named(name.clone()),
            }
        }
        TypeExpr::Tuple(elems) => Type::Tuple(elems.iter().map(type_from_expr).collect()),
        TypeExpr::Fn { params, ret } => Type::Fn {
            params: params.iter().map(type_from_expr).collect(),
            ret: Box::new(type_from_expr(ret)),
        },
        TypeExpr::Refined { base, predicate } => Type::Refined {
            base: Box::new(type_from_expr(base)),
            predicate: predicate.clone(),
        },
    }
}

/// Try to resolve a type from a parsed_type first, falling back to raw token parsing.
pub(crate) fn resolve_type(
    parsed_type: Option<&assura_parser::ast::TypeExpr>,
    tokens: &[String],
) -> Type {
    if let Some(te) = parsed_type {
        type_from_expr(te)
    } else {
        parse_type_tokens(tokens)
    }
}

/// Convert an `HirType` to the type checker's `Type`.
pub(crate) fn type_from_hir_type(hir_ty: &assura_hir::HirType) -> Type {
    use assura_hir::HirType;
    match hir_ty {
        HirType::Unit => Type::Unit,
        HirType::Named(name) => builtin_type(name).unwrap_or_else(|| Type::Named(name.clone())),
        HirType::Generic(name, args) => {
            let type_args: Vec<Type> = args.iter().map(type_from_hir_type).collect();
            match name.as_str() {
                "List" | "Vec" => Type::List(Box::new(
                    type_args.into_iter().next().unwrap_or(Type::Unknown),
                )),
                "Sequence" => Type::Sequence(Box::new(
                    type_args.into_iter().next().unwrap_or(Type::Unknown),
                )),
                "Set" => Type::Set(Box::new(
                    type_args.into_iter().next().unwrap_or(Type::Unknown),
                )),
                "Option" => Type::Option(Box::new(
                    type_args.into_iter().next().unwrap_or(Type::Unknown),
                )),
                "Map" => {
                    let mut it = type_args.into_iter();
                    Type::Map(
                        Box::new(it.next().unwrap_or(Type::Unknown)),
                        Box::new(it.next().unwrap_or(Type::Unknown)),
                    )
                }
                "Result" => {
                    let mut it = type_args.into_iter();
                    Type::Result(
                        Box::new(it.next().unwrap_or(Type::Unknown)),
                        Box::new(it.next().unwrap_or(Type::Unknown)),
                    )
                }
                _ => Type::Named(name.clone()),
            }
        }
        HirType::Tuple(elems) => Type::Tuple(elems.iter().map(type_from_hir_type).collect()),
        HirType::Fn { params, ret } => Type::Fn {
            params: params.iter().map(type_from_hir_type).collect(),
            ret: Box::new(type_from_hir_type(ret)),
        },
        HirType::Refined { base, predicate } => Type::Refined {
            base: Box::new(type_from_hir_type(base)),
            predicate: predicate.clone(),
        },
        HirType::Unresolved(tokens) => parse_type_tokens(tokens),
    }
}

// ---------------------------------------------------------------------------
// Type token parsing
// ---------------------------------------------------------------------------

/// Parse a raw token sequence (e.g. `["List", "<", "Int", ">"]`) into a
/// structured `Type`. Handles base types, generic containers, refinement
/// types, taint annotations, reference/mutable types, and union error types.
pub(crate) fn parse_type_tokens(tokens: &[String]) -> Type {
    if tokens.is_empty() {
        return Type::Unit;
    }

    // Strip taint annotations (everything from "@" onward)
    let clean: Vec<&str> = tokens
        .iter()
        .map(|s| s.as_str())
        .take_while(|t| *t != "@")
        .collect();
    if clean.is_empty() {
        return Type::Unit;
    }

    // Strip leading & or &mut (references)
    let clean = if clean.first() == Some(&"&") {
        if clean.get(1) == Some(&"mut") {
            &clean[2..]
        } else {
            &clean[1..]
        }
    } else {
        &clean[..]
    };
    if clean.is_empty() {
        return Type::Unknown;
    }

    // Refinement type: { x : T | P }
    if clean.first() == Some(&"{") {
        // Find the colon to extract the base type
        if let Some(colon_pos) = clean.iter().position(|t| *t == ":") {
            let after_colon: Vec<&str> = clean[colon_pos + 1..]
                .iter()
                .take_while(|t| **t != "|" && **t != "}")
                .copied()
                .collect();
            let owned: Vec<String> = after_colon.iter().map(|s| s.to_string()).collect();
            let base = parse_type_tokens(&owned);

            // Extract predicate: everything between | and }
            let predicate = if let Some(pipe_pos) = clean.iter().position(|t| *t == "|") {
                clean[pipe_pos + 1..]
                    .iter()
                    .take_while(|t| **t != "}")
                    .copied()
                    .collect::<Vec<&str>>()
                    .join(" ")
            } else {
                String::new()
            };

            return Type::Refined {
                base: Box::new(base),
                predicate,
            };
        }
        return Type::Unknown;
    }

    // Handle union error types: T | E -> Result<T, E> at top level
    let mut depth = 0i32;
    let mut pipe_pos = None;
    for (i, tok) in clean.iter().enumerate() {
        match *tok {
            "<" => depth += 1,
            ">" if depth > 0 => depth -= 1,
            "|" if depth == 0 => {
                pipe_pos = Some(i);
                break;
            }
            _ => {}
        }
    }
    if let Some(pp) = pipe_pos {
        let ok_tokens: Vec<String> = clean[..pp].iter().map(|s| s.to_string()).collect();
        let err_tokens: Vec<String> = clean[pp + 1..].iter().map(|s| s.to_string()).collect();
        let ok_ty = parse_type_tokens(&ok_tokens);
        let err_ty = parse_type_tokens(&err_tokens);
        return Type::Result(Box::new(ok_ty), Box::new(err_ty));
    }

    let head = clean[0];

    // Function type: fn ( A , B ) -> C
    if head == "fn" && clean.len() >= 3 && clean[1] == "(" {
        // Find matching closing paren
        let mut depth = 0i32;
        let mut close_paren = None;
        for (i, tok) in clean[1..].iter().enumerate() {
            match *tok {
                "(" => depth += 1,
                ")" => {
                    depth -= 1;
                    if depth == 0 {
                        close_paren = Some(i + 1); // offset by 1 for the slice
                        break;
                    }
                }
                _ => {}
            }
        }
        if let Some(cp) = close_paren {
            // Parse parameter types from between ( and )
            let param_tokens = &clean[2..cp];
            let mut params: Vec<Type> = Vec::new();
            let mut current: Vec<String> = Vec::new();
            let mut d = 0i32;
            for tok in param_tokens {
                match *tok {
                    "(" | "<" => {
                        d += 1;
                        current.push(tok.to_string());
                    }
                    ")" | ">" => {
                        d -= 1;
                        current.push(tok.to_string());
                    }
                    "," if d == 0 => {
                        if !current.is_empty() {
                            params.push(parse_type_tokens(&current));
                            current.clear();
                        }
                    }
                    _ => current.push(tok.to_string()),
                }
            }
            if !current.is_empty() {
                params.push(parse_type_tokens(&current));
            }

            // Check for -> return type after the closing paren
            let after_paren = &clean[cp + 1..];
            let ret = if after_paren.len() >= 2 && after_paren[0] == "->" {
                let ret_tokens: Vec<String> =
                    after_paren[1..].iter().map(|s| s.to_string()).collect();
                Box::new(parse_type_tokens(&ret_tokens))
            } else {
                Box::new(Type::Unit)
            };

            return Type::Fn { params, ret };
        }
    }

    // Single-token base types
    if clean.len() == 1 {
        if let Some(ty) = builtin_type(head) {
            return ty;
        }
        return Type::Named(head.to_string());
    }

    // Generic container: Name < Args... >
    if clean.len() >= 3 && clean[1] == "<" {
        // Collect type arguments between < and >
        let inner = &clean[2..];
        // Strip trailing >
        let inner = if inner.last() == Some(&">") {
            &inner[..inner.len() - 1]
        } else {
            inner
        };

        // Split on commas at depth 0
        let mut args: Vec<Type> = Vec::new();
        let mut current: Vec<String> = Vec::new();
        let mut d = 0i32;
        for tok in inner {
            match *tok {
                "<" => {
                    d += 1;
                    current.push(tok.to_string());
                }
                ">" => {
                    d -= 1;
                    current.push(tok.to_string());
                }
                "," if d == 0 => {
                    if !current.is_empty() {
                        args.push(parse_type_tokens(&current));
                        current.clear();
                    }
                }
                _ => current.push(tok.to_string()),
            }
        }
        if !current.is_empty() {
            args.push(parse_type_tokens(&current));
        }

        return match head {
            "List" => Type::List(Box::new(args.into_iter().next().unwrap_or(Type::Unknown))),
            "Sequence" => {
                Type::Sequence(Box::new(args.into_iter().next().unwrap_or(Type::Unknown)))
            }
            "Set" => Type::Set(Box::new(args.into_iter().next().unwrap_or(Type::Unknown))),
            "Option" => Type::Option(Box::new(args.into_iter().next().unwrap_or(Type::Unknown))),
            "Map" => {
                let mut it = args.into_iter();
                let k = it.next().unwrap_or(Type::Unknown);
                let v = it.next().unwrap_or(Type::Unknown);
                Type::Map(Box::new(k), Box::new(v))
            }
            "Result" => {
                let mut it = args.into_iter();
                let ok = it.next().unwrap_or(Type::Unknown);
                let err = it.next().unwrap_or(Type::Unknown);
                Type::Result(Box::new(ok), Box::new(err))
            }
            "Vec" => Type::List(Box::new(args.into_iter().next().unwrap_or(Type::Unknown))),
            _ => Type::Named(head.to_string()),
        };
    }

    // Tuple type: ( A, B, C )
    if head == "(" && clean.last() == Some(&")") {
        let inner = &clean[1..clean.len() - 1];
        if inner.is_empty() {
            return Type::Unit;
        }
        // Split on commas at depth 0
        let mut elems: Vec<Type> = Vec::new();
        let mut current: Vec<String> = Vec::new();
        let mut d = 0i32;
        for tok in inner {
            match *tok {
                "(" | "<" => {
                    d += 1;
                    current.push(tok.to_string());
                }
                ")" | ">" => {
                    d -= 1;
                    current.push(tok.to_string());
                }
                "," if d == 0 => {
                    if !current.is_empty() {
                        elems.push(parse_type_tokens(&current));
                        current.clear();
                    }
                }
                _ => current.push(tok.to_string()),
            }
        }
        if !current.is_empty() {
            elems.push(parse_type_tokens(&current));
        }
        return Type::Tuple(elems);
    }

    // Fallback: treat as named type
    if let Some(ty) = builtin_type(head) {
        return ty;
    }
    Type::Named(head.to_string())
}
