//! Type conversion functions.
//!
//! Converts between AST TypeExpr, raw token sequences,
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
        TypeExpr::Refined { base, predicate } => {
            // Extract bound variable name from the predicate text (first token
            // before any operator). Default to "x" if we cannot determine it.
            let bound_var = predicate
                .split_whitespace()
                .next()
                .filter(|s| s.chars().all(|c| c.is_alphanumeric() || c == '_'))
                .unwrap_or("x")
                .to_string();
            let tokens: Vec<String> = if predicate.is_empty() {
                vec![]
            } else {
                predicate.split_whitespace().map(String::from).collect()
            };
            Type::Refined {
                base: Box::new(type_from_expr(base)),
                predicate: Box::new(assura_parser::ast::Spanned::no_span(
                    assura_parser::ast::Expr::Raw(tokens),
                )),
                bound_var,
            }
        }
    }
}

/// Resolve a type from an `Option<TypeExpr>`, returning `Type::Unit` if `None`.
pub(crate) fn resolve_type_opt(type_expr: Option<&assura_parser::ast::TypeExpr>) -> Type {
    match type_expr {
        Some(te) => type_from_expr(te),
        None => Type::Unit,
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
            let pred_tokens: Vec<String> =
                if let Some(pipe_pos) = clean.iter().position(|t| *t == "|") {
                    clean[pipe_pos + 1..]
                        .iter()
                        .take_while(|t| **t != "}")
                        .map(|s| s.to_string())
                        .collect()
                } else {
                    vec![]
                };

            // Extract bound variable name (token before the colon)
            let bound_var = if colon_pos > 1 {
                clean[colon_pos - 1].to_string()
            } else {
                "x".to_string()
            };

            return Type::Refined {
                base: Box::new(base),
                predicate: Box::new(assura_parser::ast::Spanned::no_span(
                    assura_parser::ast::Expr::Raw(pred_tokens),
                )),
                bound_var,
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

#[cfg(test)]
mod tests {
    use super::*;
    use assura_parser::ast::TypeExpr;

    /// Helper: build a `Vec<String>` from a slice of `&str`.
    fn tokens(s: &[&str]) -> Vec<String> {
        s.iter().map(|t| t.to_string()).collect()
    }

    // -----------------------------------------------------------------------
    // type_from_expr
    // -----------------------------------------------------------------------

    #[test]
    fn type_from_expr_unit() {
        assert_eq!(type_from_expr(&TypeExpr::Unit), Type::Unit);
    }

    #[test]
    fn type_from_expr_named_int() {
        assert_eq!(type_from_expr(&TypeExpr::Named("Int".into())), Type::Int,);
    }

    #[test]
    fn type_from_expr_named_bool() {
        assert_eq!(type_from_expr(&TypeExpr::Named("Bool".into())), Type::Bool,);
    }

    #[test]
    fn type_from_expr_named_custom() {
        assert_eq!(
            type_from_expr(&TypeExpr::Named("CustomType".into())),
            Type::Named("CustomType".into()),
        );
    }

    #[test]
    fn type_from_expr_generic_list() {
        let expr = TypeExpr::Generic("List".into(), vec![TypeExpr::Named("Int".into())]);
        assert_eq!(type_from_expr(&expr), Type::List(Box::new(Type::Int)),);
    }

    #[test]
    fn type_from_expr_generic_map() {
        let expr = TypeExpr::Generic(
            "Map".into(),
            vec![
                TypeExpr::Named("String".into()),
                TypeExpr::Named("Int".into()),
            ],
        );
        assert_eq!(
            type_from_expr(&expr),
            Type::Map(Box::new(Type::String), Box::new(Type::Int)),
        );
    }

    #[test]
    fn type_from_expr_generic_result() {
        let expr = TypeExpr::Generic(
            "Result".into(),
            vec![
                TypeExpr::Named("Int".into()),
                TypeExpr::Named("String".into()),
            ],
        );
        assert_eq!(
            type_from_expr(&expr),
            Type::Result(Box::new(Type::Int), Box::new(Type::String)),
        );
    }

    #[test]
    fn type_from_expr_generic_option() {
        let expr = TypeExpr::Generic("Option".into(), vec![TypeExpr::Named("Bool".into())]);
        assert_eq!(type_from_expr(&expr), Type::Option(Box::new(Type::Bool)),);
    }

    #[test]
    fn type_from_expr_generic_vec_maps_to_list() {
        let expr = TypeExpr::Generic("Vec".into(), vec![TypeExpr::Named("Float".into())]);
        assert_eq!(type_from_expr(&expr), Type::List(Box::new(Type::Float)),);
    }

    #[test]
    fn type_from_expr_tuple() {
        let expr = TypeExpr::Tuple(vec![
            TypeExpr::Named("Int".into()),
            TypeExpr::Named("Bool".into()),
        ]);
        assert_eq!(
            type_from_expr(&expr),
            Type::Tuple(vec![Type::Int, Type::Bool]),
        );
    }

    #[test]
    fn type_from_expr_fn_type() {
        let expr = TypeExpr::Fn {
            params: vec![TypeExpr::Named("Int".into())],
            ret: Box::new(TypeExpr::Named("Bool".into())),
        };
        assert_eq!(
            type_from_expr(&expr),
            Type::Fn {
                params: vec![Type::Int],
                ret: Box::new(Type::Bool),
            },
        );
    }

    #[test]
    fn type_from_expr_refined() {
        let expr = TypeExpr::Refined {
            base: Box::new(TypeExpr::Named("Int".into())),
            predicate: "x > 0".into(),
        };
        let result = type_from_expr(&expr);
        assert_eq!(result, Type::refined_from_str(Type::Int, "x", "x > 0"),);
    }

    #[test]
    fn type_from_expr_generic_sequence() {
        let expr = TypeExpr::Generic("Sequence".into(), vec![TypeExpr::Named("Int".into())]);
        assert_eq!(type_from_expr(&expr), Type::Sequence(Box::new(Type::Int)),);
    }

    #[test]
    fn type_from_expr_generic_set() {
        let expr = TypeExpr::Generic("Set".into(), vec![TypeExpr::Named("Int".into())]);
        assert_eq!(type_from_expr(&expr), Type::Set(Box::new(Type::Int)),);
    }

    #[test]
    fn type_from_expr_generic_unknown_name() {
        let expr = TypeExpr::Generic("Stream".into(), vec![TypeExpr::Named("Int".into())]);
        assert_eq!(type_from_expr(&expr), Type::Named("Stream".into()),);
    }

    // -----------------------------------------------------------------------
    // resolve_type_opt
    // -----------------------------------------------------------------------

    #[test]
    fn resolve_type_opt_none_returns_unit() {
        assert_eq!(resolve_type_opt(None), Type::Unit);
    }

    #[test]
    fn resolve_type_opt_some_returns_converted() {
        let expr = TypeExpr::Named("Int".into());
        assert_eq!(resolve_type_opt(Some(&expr)), Type::Int);
    }

    // -----------------------------------------------------------------------
    // parse_type_tokens
    // -----------------------------------------------------------------------

    #[test]
    fn parse_empty_tokens_returns_unit() {
        assert_eq!(parse_type_tokens(&[]), Type::Unit);
    }

    #[test]
    fn parse_int() {
        assert_eq!(parse_type_tokens(&tokens(&["Int"])), Type::Int);
    }

    #[test]
    fn parse_bool() {
        assert_eq!(parse_type_tokens(&tokens(&["Bool"])), Type::Bool);
    }

    #[test]
    fn parse_string() {
        assert_eq!(parse_type_tokens(&tokens(&["String"])), Type::String);
    }

    #[test]
    fn parse_float() {
        assert_eq!(parse_type_tokens(&tokens(&["Float"])), Type::Float);
    }

    #[test]
    fn parse_nat() {
        assert_eq!(parse_type_tokens(&tokens(&["Nat"])), Type::Nat);
    }

    #[test]
    fn parse_bytes() {
        assert_eq!(parse_type_tokens(&tokens(&["Bytes"])), Type::Bytes);
    }

    #[test]
    fn parse_custom_named() {
        assert_eq!(
            parse_type_tokens(&tokens(&["CustomName"])),
            Type::Named("CustomName".into()),
        );
    }

    #[test]
    fn parse_generic_list() {
        assert_eq!(
            parse_type_tokens(&tokens(&["List", "<", "Int", ">"])),
            Type::List(Box::new(Type::Int)),
        );
    }

    #[test]
    fn parse_generic_map() {
        assert_eq!(
            parse_type_tokens(&tokens(&["Map", "<", "String", ",", "Int", ">"])),
            Type::Map(Box::new(Type::String), Box::new(Type::Int)),
        );
    }

    #[test]
    fn parse_generic_option() {
        assert_eq!(
            parse_type_tokens(&tokens(&["Option", "<", "Bool", ">"])),
            Type::Option(Box::new(Type::Bool)),
        );
    }

    #[test]
    fn parse_generic_result() {
        assert_eq!(
            parse_type_tokens(&tokens(&["Result", "<", "Int", ",", "String", ">"])),
            Type::Result(Box::new(Type::Int), Box::new(Type::String)),
        );
    }

    #[test]
    fn parse_generic_set() {
        assert_eq!(
            parse_type_tokens(&tokens(&["Set", "<", "Int", ">"])),
            Type::Set(Box::new(Type::Int)),
        );
    }

    #[test]
    fn parse_generic_sequence() {
        assert_eq!(
            parse_type_tokens(&tokens(&["Sequence", "<", "Int", ">"])),
            Type::Sequence(Box::new(Type::Int)),
        );
    }

    #[test]
    fn parse_generic_vec_maps_to_list() {
        assert_eq!(
            parse_type_tokens(&tokens(&["Vec", "<", "Float", ">"])),
            Type::List(Box::new(Type::Float)),
        );
    }

    #[test]
    fn parse_tuple() {
        assert_eq!(
            parse_type_tokens(&tokens(&["(", "Int", ",", "Bool", ")"])),
            Type::Tuple(vec![Type::Int, Type::Bool]),
        );
    }

    #[test]
    fn parse_empty_tuple_is_unit() {
        assert_eq!(parse_type_tokens(&tokens(&["(", ")"])), Type::Unit,);
    }

    #[test]
    fn parse_fn_type() {
        assert_eq!(
            parse_type_tokens(&tokens(&["fn", "(", "Int", ")", "->", "Bool"])),
            Type::Fn {
                params: vec![Type::Int],
                ret: Box::new(Type::Bool),
            },
        );
    }

    #[test]
    fn parse_fn_no_return_type() {
        assert_eq!(
            parse_type_tokens(&tokens(&["fn", "(", "Int", ")"])),
            Type::Fn {
                params: vec![Type::Int],
                ret: Box::new(Type::Unit),
            },
        );
    }

    #[test]
    fn parse_fn_multiple_params() {
        assert_eq!(
            parse_type_tokens(&tokens(&[
                "fn", "(", "Int", ",", "Bool", ")", "->", "String"
            ])),
            Type::Fn {
                params: vec![Type::Int, Type::Bool],
                ret: Box::new(Type::String),
            },
        );
    }

    #[test]
    fn parse_refinement_type() {
        let result = parse_type_tokens(&tokens(&["{", "x", ":", "Int", "|", "x > 0", "}"]));
        // The predicate "x > 0" comes as a single token from the token split,
        // which gets stored as Expr::Raw(["x > 0"]). Compare via predicate_str().
        if let Type::Refined {
            ref base,
            ref bound_var,
            ..
        } = result
        {
            assert_eq!(**base, Type::Int);
            assert_eq!(bound_var, "x");
            assert_eq!(result.predicate_str(), Some("x > 0".into()));
        } else {
            panic!("expected Refined, got {result:?}");
        }
    }

    #[test]
    fn parse_reference_stripped() {
        assert_eq!(parse_type_tokens(&tokens(&["&", "Int"])), Type::Int,);
    }

    #[test]
    fn parse_mut_reference_stripped() {
        assert_eq!(parse_type_tokens(&tokens(&["&", "mut", "Int"])), Type::Int,);
    }

    #[test]
    fn parse_taint_annotation_stripped() {
        assert_eq!(
            parse_type_tokens(&tokens(&["Int", "@", "tainted"])),
            Type::Int,
        );
    }

    #[test]
    fn parse_union_error_type() {
        assert_eq!(
            parse_type_tokens(&tokens(&["Int", "|", "String"])),
            Type::Result(Box::new(Type::Int), Box::new(Type::String)),
        );
    }

    #[test]
    fn parse_nested_generic() {
        assert_eq!(
            parse_type_tokens(&tokens(&["List", "<", "List", "<", "Int", ">", ">"])),
            Type::List(Box::new(Type::List(Box::new(Type::Int)))),
        );
    }

    #[test]
    fn parse_bare_ref_returns_unknown() {
        // "&" alone with no type after stripping
        assert_eq!(parse_type_tokens(&tokens(&["&"])), Type::Unknown,);
    }

    #[test]
    fn parse_only_taint_returns_unit() {
        // All tokens stripped by taint annotation
        assert_eq!(parse_type_tokens(&tokens(&["@", "tainted"])), Type::Unit,);
    }

    #[test]
    fn parse_brace_without_colon_returns_unknown() {
        // Malformed refinement: no colon
        assert_eq!(parse_type_tokens(&tokens(&["{", "x", "}"])), Type::Unknown,);
    }
}
