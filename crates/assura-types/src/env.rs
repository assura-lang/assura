//! Type environment construction.
//!
//! Builds TypeEnv from symbol tables and AST/HIR declarations.

use assura_parser::ast::{ClauseKind, Decl, ServiceItem};
use assura_resolve::{SymbolKind, SymbolTable};

use crate::clauses::{
    collect_input_param_types, extract_output_type_from_body, register_input_clause_params,
};
use crate::convert::{parse_type_tokens, resolve_type, type_from_hir_type};
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
                // Insert parameter types (prefer parsed TypeExpr when available)
                for p in &f.params {
                    let ty = resolve_type(p.parsed_type.as_ref(), &p.ty);
                    env.insert(p.name.clone(), ty);
                }
                // Build full function type
                let param_types: Vec<Type> = f
                    .params
                    .iter()
                    .map(|p| resolve_type(p.parsed_type.as_ref(), &p.ty))
                    .collect();
                let ret = if f.return_ty.is_empty() {
                    Type::Unit
                } else {
                    parse_type_tokens(&f.return_ty)
                };
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
                    let ty = resolve_type(p.parsed_type.as_ref(), &p.ty);
                    env.insert(p.name.clone(), ty);
                }
                let param_types: Vec<Type> = e
                    .params
                    .iter()
                    .map(|p| resolve_type(p.parsed_type.as_ref(), &p.ty))
                    .collect();
                let ret = if e.return_ty.is_empty() {
                    Type::Unit
                } else {
                    parse_type_tokens(&e.return_ty)
                };
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
                        .map(|f| (f.name.clone(), parse_type_tokens(&f.ty)))
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
                if !p.ty_tokens.is_empty() {
                    env.insert(p.name.clone(), parse_type_tokens(&p.ty_tokens));
                }
            }
            // Bind params are registered above with Extern; Block and
            // other structural decls don't contribute to the type env.
            Decl::Bind(_) | Decl::CodecRegistry(_) | Decl::Block { .. } => {}
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

/// Build a type environment from an `HirFile`, using structured `HirType`
/// values instead of raw token parsing for function/extern/field types.
/// Contract and service clause handling still uses the AST via
/// `hir.resolved()` since clause body parsing is not yet migrated.
pub(crate) fn build_type_env_from_hir(hir: &assura_hir::HirFile) -> TypeEnv {
    let resolved = hir.resolved();
    let mut env = TypeEnv::new();

    // Phase 1: seed from symbol table (builtins, type names, etc.)
    for sym in &resolved.symbols.symbols {
        let ty = match sym.kind {
            SymbolKind::BuiltinType => builtin_type(&sym.name).unwrap_or(Type::Unknown),
            SymbolKind::TypeDef
            | SymbolKind::ContractDef
            | SymbolKind::ServiceDef
            | SymbolKind::EnumDef => Type::Named(sym.name.clone()),
            SymbolKind::FnDef | SymbolKind::ExternFn | SymbolKind::BindFn => Type::Fn {
                params: Vec::new(),
                ret: Box::new(Type::Unknown),
            },
            SymbolKind::Operation | SymbolKind::Query => Type::Fn {
                params: Vec::new(),
                ret: Box::new(Type::Unknown),
            },
            SymbolKind::TypeParam => Type::TypeParam(sym.name.clone()),
            SymbolKind::Parameter | SymbolKind::Field => Type::Unknown,
            SymbolKind::EnumVariant => Type::Named(sym.name.clone()),
            SymbolKind::Prophecy => Type::Unknown,
            SymbolKind::CodecRegistry => Type::Named(sym.name.clone()),
        };
        env.insert(sym.name.clone(), ty);
    }

    // Phase 2: enrich from HIR declarations
    use assura_hir::{HirDeclKind, HirServiceItem as HirSI};
    for decl in &hir.decls {
        match &decl.kind {
            HirDeclKind::FnDef(f) => {
                for p in &f.params {
                    env.insert(p.name.clone(), type_from_hir_type(&p.ty));
                }
                let param_types: Vec<Type> =
                    f.params.iter().map(|p| type_from_hir_type(&p.ty)).collect();
                let ret = type_from_hir_type(&f.return_ty);
                env.insert(
                    f.name.clone(),
                    Type::Fn {
                        params: param_types,
                        ret: Box::new(ret),
                    },
                );
            }
            HirDeclKind::Extern(e) => {
                for p in &e.params {
                    env.insert(p.name.clone(), type_from_hir_type(&p.ty));
                }
                let param_types: Vec<Type> =
                    e.params.iter().map(|p| type_from_hir_type(&p.ty)).collect();
                let ret = type_from_hir_type(&e.return_ty);
                env.insert(
                    e.name.clone(),
                    Type::Fn {
                        params: param_types,
                        ret: Box::new(ret),
                    },
                );
            }
            HirDeclKind::Bind(b) => {
                for p in &b.params {
                    env.insert(p.name.clone(), type_from_hir_type(&p.ty));
                }
                let param_types: Vec<Type> =
                    b.params.iter().map(|p| type_from_hir_type(&p.ty)).collect();
                let ret = type_from_hir_type(&b.return_ty);
                env.insert(
                    b.name.clone(),
                    Type::Fn {
                        params: param_types,
                        ret: Box::new(ret),
                    },
                );
            }
            HirDeclKind::Contract(c) => {
                // Input clause param registration still uses AST
                for clause in &c.clauses {
                    if clause.kind == assura_hir::HirClauseKind::Input {
                        let ast_clause = clause.to_ast_clause();
                        register_input_clause_params(&ast_clause.body, &mut env);
                    }
                }
            }
            HirDeclKind::Service(s) => {
                for item in &s.items {
                    let (name, clauses) = match item {
                        HirSI::Operation { name, clauses } => (name, clauses),
                        HirSI::Query { name, clauses } => (name, clauses),
                        _ => continue,
                    };
                    let mut param_types = Vec::new();
                    let mut ret = Type::Unit;
                    for clause in clauses {
                        if clause.kind == assura_hir::HirClauseKind::Input {
                            let ast_clause = clause.to_ast_clause();
                            collect_input_param_types(&ast_clause.body, &mut param_types);
                        }
                        if clause.kind == assura_hir::HirClauseKind::Output {
                            let ast_clause = clause.to_ast_clause();
                            let ty = extract_output_type_from_body(&ast_clause.body);
                            if !ty.is_indeterminate() {
                                ret = ty;
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
            HirDeclKind::TypeDef(td) => {
                if let assura_hir::HirTypeBody::Struct(fields) = &td.body {
                    let field_types: Vec<(String, Type)> = fields
                        .iter()
                        .map(|f| (f.name.clone(), type_from_hir_type(&f.ty)))
                        .collect();
                    env.struct_fields.insert(td.name.clone(), field_types);
                }
            }
            HirDeclKind::EnumDef(e) => {
                for variant in &e.variants {
                    if !variant.fields.is_empty() {
                        let field_types: Vec<Type> =
                            variant.fields.iter().map(type_from_hir_type).collect();
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
            // Prophecy variables are ghost; register their type in the env
            // so ensures clauses can reference the prophecy name.
            HirDeclKind::Prophecy(p) => {
                let ty = type_from_hir_type(&p.ty);
                env.insert(p.name.clone(), ty);
            }
            HirDeclKind::CodecRegistry(_) | HirDeclKind::Block(_) => {}
        }
    }

    // T107: inject stdlib types
    let stdlib = StdlibTypes::new();
    for sdef in stdlib.all_types() {
        if env.lookup(&sdef.name).is_none() {
            env.insert(sdef.name.clone(), sdef.base_type.clone());
        }
    }

    env
}
