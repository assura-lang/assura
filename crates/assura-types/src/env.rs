//! Type environment construction.
//!
//! Builds TypeEnv from symbol tables and AST declarations.

use assura_parser::ast::{ClauseKind, Decl, ServiceItem};
use assura_resolve::{SymbolKind, SymbolTable};

use crate::clauses::{
    collect_input_param_types, extract_output_type_from_body, register_input_clause_params,
};
use crate::convert::{parse_type_tokens, resolve_type_opt, type_from_expr};
use crate::domain::StdlibTypes;
use crate::types::builtin_type;
use crate::{Type, TypeEnv};

// ---------------------------------------------------------------------------
// Type environment construction
// ---------------------------------------------------------------------------

/// Build a `TypeEnv` from a resolved symbol table and the source AST.
///
/// First walks the symbol table for top-level declarations, then walks the
/// AST to extract actual parameter types from `Param.ty` token sequences
/// and function return types from `FnDef.return_ty`.
pub(crate) fn build_type_env(
    symbols: &SymbolTable,
    source: &assura_parser::ast::SourceFile,
) -> TypeEnv {
    let mut env = TypeEnv::new();

    for sym in &symbols.symbols {
        let ty = match sym.kind {
            SymbolKind::BuiltinType => builtin_type(&sym.name).unwrap_or(Type::Unknown),
            SymbolKind::TypeDef
            | SymbolKind::ContractDef
            | SymbolKind::ServiceDef
            | SymbolKind::EnumDef => Type::Named(sym.name.clone()),

            // Placeholder; enriched below from AST
            SymbolKind::FnDef | SymbolKind::ExternFn | SymbolKind::BindFn => Type::Fn {
                params: Vec::new(),
                ret: Box::new(Type::Unknown),
            },

            SymbolKind::Operation | SymbolKind::Query => Type::Fn {
                params: Vec::new(),
                ret: Box::new(Type::Unknown),
            },

            SymbolKind::TypeParam => Type::TypeParam(sym.name.clone()),

            // Placeholder; enriched below from AST params
            SymbolKind::Parameter | SymbolKind::Field => Type::Unknown,

            SymbolKind::EnumVariant => Type::Named(sym.name.clone()),

            // Prophecy variables: placeholder; enriched below from AST
            SymbolKind::Prophecy => Type::Unknown,

            // Codec registries are not types; they define dispatch tables
            SymbolKind::CodecRegistry => Type::Named(sym.name.clone()),
        };

        env.insert(sym.name.clone(), ty);
    }

    // Enrich from AST: parse Param.ty token sequences into structured Types
    // and build proper function signatures with param types and return types.
    for decl in &source.decls {
        match &decl.node {
            Decl::FnDef(f) => {
                // Insert parameter types from structured TypeExpr
                for p in &f.params {
                    let ty = resolve_type_opt(p.ty.as_ref());
                    env.insert(p.name.clone(), ty);
                }
                // Build full function type
                let param_types: Vec<Type> = f
                    .params
                    .iter()
                    .map(|p| resolve_type_opt(p.ty.as_ref()))
                    .collect();
                let ret = resolve_type_opt(f.return_ty.as_ref());
                env.insert(
                    f.name.clone(),
                    Type::Fn {
                        params: param_types,
                        ret: Box::new(ret),
                    },
                );
            }
            Decl::Extern(e) => {
                for p in &e.params {
                    let ty = resolve_type_opt(p.ty.as_ref());
                    env.insert(p.name.clone(), ty);
                }
                let param_types: Vec<Type> = e
                    .params
                    .iter()
                    .map(|p| resolve_type_opt(p.ty.as_ref()))
                    .collect();
                let ret = resolve_type_opt(e.return_ty.as_ref());
                env.insert(
                    e.name.clone(),
                    Type::Fn {
                        params: param_types,
                        ret: Box::new(ret),
                    },
                );
            }
            Decl::Contract(c) => {
                // Extract input params from contract clauses and register them
                for clause in &c.clauses {
                    if clause.kind == ClauseKind::Input {
                        register_input_clause_params(&clause.body, &mut env);
                    }
                }
            }
            Decl::Service(s) => {
                // Enrich service operation/query types from their clauses.
                // Extract input clause params as parameter types and output
                // clause type as return type, mirroring FnDef/Extern handling.
                for item in &s.items {
                    let (name, clauses) = match item {
                        ServiceItem::Operation { name, clauses } => (name, clauses),
                        ServiceItem::Query { name, clauses } => (name, clauses),
                        _ => continue,
                    };
                    // Collect parameter types from input clauses
                    let mut param_types = Vec::new();
                    for clause in clauses {
                        if clause.kind == ClauseKind::Input {
                            collect_input_param_types(&clause.body, &mut param_types);
                        }
                    }
                    // Determine return type from output clauses
                    let mut ret = Type::Unit;
                    for clause in clauses {
                        if clause.kind == ClauseKind::Output {
                            let ty = extract_output_type_from_body(&clause.body);
                            if !ty.is_indeterminate() {
                                ret = ty;
                                break;
                            }
                        }
                    }
                    env.insert(
                        name.clone(),
                        Type::Fn {
                            params: param_types,
                            ret: Box::new(ret),
                        },
                    );
                }
            }
            Decl::TypeDef(td) => {
                // Register struct field types for field resolution
                if let assura_parser::ast::TypeBody::Struct(fields) = &td.body {
                    let field_types: Vec<(String, Type)> = fields
                        .iter()
                        .map(|f| (f.name.clone(), resolve_type_opt(f.ty.as_ref())))
                        .collect();
                    env.struct_fields.insert(td.name.clone(), field_types);
                }
            }
            Decl::EnumDef(e) => {
                // Register enum variant constructors as functions
                for variant in &e.variants {
                    if !variant.fields.is_empty() {
                        let field_types: Vec<Type> = variant
                            .fields
                            .iter()
                            .map(|f| parse_type_tokens(std::slice::from_ref(f)))
                            .collect();
                        env.insert(
                            variant.name.clone(),
                            Type::Fn {
                                params: field_types,
                                ret: Box::new(Type::Named(e.name.clone())),
                            },
                        );
                    }
                }
            }
            // Prophecy variables: register their type annotation in the env
            Decl::Prophecy(p) => {
                if let Some(te) = &p.ty {
                    env.insert(p.name.clone(), type_from_expr(te));
                }
            }
            Decl::Bind(b) => {
                // Register parameter types (same pattern as FnDef/Extern)
                for p in &b.params {
                    let ty = resolve_type_opt(p.ty.as_ref());
                    env.insert(p.name.clone(), ty);
                }
                let param_types: Vec<Type> = b
                    .params
                    .iter()
                    .map(|p| resolve_type_opt(p.ty.as_ref()))
                    .collect();
                let ret = resolve_type_opt(b.return_ty.as_ref());
                env.insert(
                    b.name.clone(),
                    Type::Fn {
                        params: param_types,
                        ret: Box::new(ret),
                    },
                );
            }
            // Block and other structural decls don't contribute to the type env.
            Decl::CodecRegistry(_) | Decl::Block { .. } => {}
        }
    }

    // T107: inject stdlib types (Pos, NonNeg, Email, Uuid, Port, Percentage)
    // so they are available for type resolution even without explicit imports
    let stdlib = StdlibTypes::new();
    for sdef in stdlib.all_types() {
        if env.lookup(&sdef.name).is_none() {
            env.insert(sdef.name.clone(), sdef.base_type.clone());
        }
    }
    env
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: parse source, resolve, and build type env via the full pipeline.
    fn env_from_source(src: &str) -> TypeEnv {
        let source = assura_parser::parse_unwrap(src);
        let resolved = assura_resolve::resolve(&source).unwrap();
        build_type_env(&resolved.symbols, &source)
    }

    #[test]
    fn empty_source_has_stdlib_types() {
        let env = env_from_source("");
        // Stdlib types like Pos, NonNeg, Email should be injected
        assert!(env.lookup("Pos").is_some());
        assert!(env.lookup("NonNeg").is_some());
    }

    #[test]
    fn fndef_params_enriched() {
        let env = env_from_source("fn add(a: Int, b: Int) -> Int { requires { a > 0 } }");
        assert_eq!(env.lookup("a"), Some(&Type::Int));
        assert_eq!(env.lookup("b"), Some(&Type::Int));
        match env.lookup("add") {
            Some(Type::Fn { params, ret }) => {
                assert_eq!(params.len(), 2);
                assert_eq!(params[0], Type::Int);
                assert_eq!(**ret, Type::Int);
            }
            other => panic!("expected Fn type for add, got {other:?}"),
        }
    }

    #[test]
    fn fndef_no_return_type_defaults_unit() {
        let env = env_from_source("fn noop() { ensures { true } }");
        match env.lookup("noop") {
            Some(Type::Fn { ret, .. }) => assert_eq!(**ret, Type::Unit),
            other => panic!("expected Fn, got {other:?}"),
        }
    }

    #[test]
    fn extern_params_enriched() {
        let env = env_from_source("extern fn ext(x: Bool) -> Nat");
        assert_eq!(env.lookup("x"), Some(&Type::Bool));
        match env.lookup("ext") {
            Some(Type::Fn { params, ret }) => {
                assert_eq!(params[0], Type::Bool);
                assert_eq!(**ret, Type::Nat);
            }
            other => panic!("expected Fn, got {other:?}"),
        }
    }

    #[test]
    fn bind_params_enriched() {
        let env = env_from_source("bind \"std::collections::HashMap\" as bd {\n  input(n: Int)\n}");
        assert_eq!(env.lookup("n"), Some(&Type::Int));
        assert!(env.lookup("bd").is_some());
    }

    #[test]
    fn typedef_struct_fields_registered() {
        let env = env_from_source("type Point { x: Float, y: Float }");
        let fields = env.struct_fields.get("Point").unwrap();
        assert_eq!(fields.len(), 2);
        assert_eq!(fields[0].0, "x");
        assert_eq!(fields[0].1, Type::Float);
    }

    #[test]
    fn enumdef_variant_constructors() {
        let env = env_from_source("enum Shape { Rect(Int, Int), Circle(Float) }");
        // Rect should have 2 Int params
        match env.lookup("Rect") {
            Some(Type::Fn { params, ret }) => {
                assert_eq!(params.len(), 2);
                assert_eq!(params[0], Type::Int);
                assert_eq!(params[1], Type::Int);
                assert_eq!(**ret, Type::Named("Shape".into()));
            }
            other => panic!("expected Fn constructor for Rect, got {other:?}"),
        }
        // Circle should have 1 Float param
        match env.lookup("Circle") {
            Some(Type::Fn { params, ret }) => {
                assert_eq!(params.len(), 1);
                assert_eq!(params[0], Type::Float);
                assert_eq!(**ret, Type::Named("Shape".into()));
            }
            other => panic!("expected Fn constructor for Circle, got {other:?}"),
        }
    }

    #[test]
    fn contract_input_params_registered() {
        let env = env_from_source("contract C { input(n: Nat) ensures { n > 0 } }");
        // The contract name should be registered
        assert!(env.lookup("C").is_some());
    }

    #[test]
    fn prophecy_type_registered() {
        let env = env_from_source("ghost prophecy p: Int");
        assert_eq!(env.lookup("p"), Some(&Type::Int));
    }

    #[test]
    fn prophecy_no_type_stays_unknown() {
        let env = env_from_source("ghost prophecy q");
        assert_eq!(env.lookup("q"), Some(&Type::Unknown));
    }

    #[test]
    fn multiple_decls_all_registered() {
        let env = env_from_source(
            "contract A { ensures { true } }\n\
             fn f(x: Int) -> Bool { ensures { true } }\n\
             type T { val: Nat }",
        );
        assert!(env.lookup("A").is_some());
        assert!(env.lookup("f").is_some());
        assert!(env.lookup("T").is_some());
        assert_eq!(env.lookup("x"), Some(&Type::Int));
    }
}
