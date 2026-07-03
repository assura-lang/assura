//! Structured contract metadata for generated Rust projects.
//!
//! Produces a machine-readable `assura-contracts.json` sidecar that AI agents
//! can parse to understand which contracts apply to which functions, what
//! pre/post-conditions exist, and what types are involved.

use assura_ast::{Clause, ClauseKind, ContractDecl, Decl, ExternDecl, FnDef, expr_to_string};
use assura_types::TypedFile;

use serde::Serialize;

use crate::types_gen::map_type_token;

/// Root metadata for a generated project.
#[derive(Debug, Clone, Serialize)]
pub struct ProjectMetadata {
    /// Original source file name.
    pub source: String,
    /// Contract metadata entries.
    pub contracts: Vec<ContractMeta>,
}

/// Metadata for a single contract, extern, or function.
#[derive(Debug, Clone, Serialize)]
pub struct ContractMeta {
    /// Contract/function name.
    pub name: String,
    /// Kind of declaration.
    pub kind: String,
    /// Generated Rust function name.
    pub function: String,
    /// Parameters.
    pub params: Vec<ParamMeta>,
    /// Return type info.
    pub return_type: ReturnTypeMeta,
    /// Requires clauses as source expressions.
    pub requires: Vec<String>,
    /// Ensures clauses as source expressions.
    pub ensures: Vec<String>,
    /// Effect annotations.
    pub effects: Vec<String>,
}

/// Parameter metadata.
#[derive(Debug, Clone, Serialize)]
pub struct ParamMeta {
    pub name: String,
    #[serde(rename = "type")]
    pub assura_type: String,
    pub rust_type: String,
}

/// Return type metadata.
#[derive(Debug, Clone, Serialize)]
pub struct ReturnTypeMeta {
    pub assura: String,
    pub rust: String,
}

/// Extract structured metadata from a typed file.
pub fn extract_metadata(typed: &TypedFile, source_name: &str) -> ProjectMetadata {
    let source = &typed.resolved.source;
    let mut contracts = Vec::new();

    for decl in &source.decls {
        match &decl.node {
            Decl::Contract(c) => {
                contracts.push(contract_meta(c));
            }
            Decl::Extern(ext) => {
                contracts.push(extern_meta(ext));
            }
            Decl::FnDef(f) => {
                contracts.push(fndef_meta(f));
            }
            _ => {}
        }
    }

    ProjectMetadata {
        source: source_name.to_string(),
        contracts,
    }
}

fn contract_meta(c: &ContractDecl) -> ContractMeta {
    let params: Vec<ParamMeta> = c
        .fn_params
        .iter()
        .map(|p| {
            let assura_type =
                p.ty.as_ref()
                    .map(|t| t.to_string())
                    .unwrap_or_else(|| "?".to_string());
            let rust_type =
                p.ty.as_ref()
                    .map(|t| map_type_token(&t.to_string()).to_string())
                    .unwrap_or_else(|| "?".to_string());
            ParamMeta {
                name: p.name.clone(),
                assura_type,
                rust_type,
            }
        })
        .collect();

    let requires = extract_clause_bodies(&c.clauses, ClauseKind::Requires);
    let ensures = extract_clause_bodies(&c.clauses, ClauseKind::Ensures);
    let effects = extract_clause_bodies(&c.clauses, ClauseKind::Effects);

    let return_type = infer_return_type(&c.clauses);

    ContractMeta {
        name: c.name.clone(),
        kind: "contract".to_string(),
        function: "check".to_string(),
        params,
        return_type,
        requires,
        ensures,
        effects,
    }
}

fn extern_meta(ext: &ExternDecl) -> ContractMeta {
    let params: Vec<ParamMeta> = ext
        .params
        .iter()
        .map(|p| {
            let assura_type =
                p.ty.as_ref()
                    .map(|t| t.to_string())
                    .unwrap_or_else(|| "?".to_string());
            let rust_type =
                p.ty.as_ref()
                    .map(|t| map_type_token(&t.to_string()).to_string())
                    .unwrap_or_else(|| "?".to_string());
            ParamMeta {
                name: p.name.clone(),
                assura_type,
                rust_type,
            }
        })
        .collect();

    let requires = extract_clause_bodies(&ext.clauses, ClauseKind::Requires);
    let ensures = extract_clause_bodies(&ext.clauses, ClauseKind::Ensures);
    let effects = extract_clause_bodies(&ext.clauses, ClauseKind::Effects);

    let return_assura = ext
        .return_ty
        .as_ref()
        .map(|t| t.to_string())
        .unwrap_or_else(|| "Unit".to_string());
    let return_rust = map_type_token(&return_assura).to_string();

    ContractMeta {
        name: ext.name.clone(),
        kind: "extern".to_string(),
        function: ext.name.clone(),
        params,
        return_type: ReturnTypeMeta {
            assura: return_assura,
            rust: return_rust,
        },
        requires,
        ensures,
        effects,
    }
}

fn fndef_meta(f: &FnDef) -> ContractMeta {
    let params: Vec<ParamMeta> = f
        .params
        .iter()
        .map(|p| {
            let assura_type =
                p.ty.as_ref()
                    .map(|t| t.to_string())
                    .unwrap_or_else(|| "?".to_string());
            let rust_type =
                p.ty.as_ref()
                    .map(|t| map_type_token(&t.to_string()).to_string())
                    .unwrap_or_else(|| "?".to_string());
            ParamMeta {
                name: p.name.clone(),
                assura_type,
                rust_type,
            }
        })
        .collect();

    let requires = extract_clause_bodies(&f.clauses, ClauseKind::Requires);
    let ensures = extract_clause_bodies(&f.clauses, ClauseKind::Ensures);
    let effects = extract_clause_bodies(&f.clauses, ClauseKind::Effects);

    let return_assura = f
        .return_ty
        .as_ref()
        .map(|t| t.to_string())
        .unwrap_or_else(|| "Unit".to_string());
    let return_rust = map_type_token(&return_assura).to_string();

    ContractMeta {
        name: f.name.clone(),
        kind: if f.is_ghost {
            "ghost_fn"
        } else if f.is_lemma {
            "lemma"
        } else {
            "fn"
        }
        .to_string(),
        function: f.name.clone(),
        params,
        return_type: ReturnTypeMeta {
            assura: return_assura,
            rust: return_rust,
        },
        requires,
        ensures,
        effects,
    }
}

fn extract_clause_bodies(clauses: &[Clause], kind: ClauseKind) -> Vec<String> {
    clauses
        .iter()
        .filter(|c| c.kind == kind)
        .map(|c| expr_to_string(&c.body))
        .collect()
}

fn infer_return_type(clauses: &[Clause]) -> ReturnTypeMeta {
    // Look for output clause to determine return type
    for clause in clauses {
        if clause.kind == ClauseKind::Output {
            let body_str = expr_to_string(&clause.body);
            // Simple heuristic: if there are type annotations, extract them
            if body_str.contains(':') {
                let parts: Vec<&str> = body_str.split(':').collect();
                if parts.len() >= 2 {
                    let assura = parts[1].trim().to_string();
                    let rust = map_type_token(&assura).to_string();
                    return ReturnTypeMeta { assura, rust };
                }
            }
        }
    }
    ReturnTypeMeta {
        assura: "Unit".to_string(),
        rust: "()".to_string(),
    }
}

/// Generate structured implementation guidance comment for a contract.
/// This appears above the `todo!()` placeholder in generated code.
pub fn implementation_guidance_comment(c: &ContractDecl) -> String {
    let mut lines = Vec::new();
    lines.push(format!("// ASSURA CONTRACT: {}", c.name));

    // Parameters
    let params: Vec<String> = c
        .fn_params
        .iter()
        .map(|p| {
            let ty =
                p.ty.as_ref()
                    .map(|t| map_type_token(&t.to_string()).to_string())
                    .unwrap_or_else(|| "?".to_string());
            format!("{}: {ty}", p.name)
        })
        .collect();
    if !params.is_empty() {
        lines.push(format!("// PARAMETERS: {}", params.join(", ")));
    }

    // Requires
    for clause in &c.clauses {
        if clause.kind == ClauseKind::Requires {
            lines.push(format!("// REQUIRES: {}", expr_to_string(&clause.body)));
        }
    }

    // Ensures
    for clause in &c.clauses {
        if clause.kind == ClauseKind::Ensures {
            lines.push(format!("// ENSURES: {}", expr_to_string(&clause.body)));
        }
    }

    // Effects
    let effect_strs: Vec<String> = c
        .clauses
        .iter()
        .filter(|cl| cl.kind == ClauseKind::Effects)
        .map(|cl| expr_to_string(&cl.body))
        .collect();
    if !effect_strs.is_empty() {
        lines.push(format!("// EFFECTS: {}", effect_strs.join(", ")));
    } else {
        lines.push("// EFFECTS: pure".to_string());
    }

    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn typecheck_ok(source: &str) -> assura_types::TypedFile {
        let file = assura_parser::parse_unwrap(source);
        let resolved = assura_resolve::resolve(&file).expect("resolve should succeed");
        assura_types::type_check(resolved).expect("type check should succeed")
    }


    #[test]
    fn metadata_extracts_contract_info() {
        let source = r#"
contract SafeDiv {
    input(a: Int, b: Int)
    requires { b != 0 }
    ensures { result == a / b }
}
"#;
        let typed = typecheck_ok(source);
        let meta = extract_metadata(&typed, "test.assura");

        assert_eq!(meta.source, "test.assura");
        assert!(!meta.contracts.is_empty());

        let c = &meta.contracts[0];
        assert_eq!(c.name, "SafeDiv");
        assert_eq!(c.kind, "contract");
        assert!(!c.requires.is_empty());
        assert!(!c.ensures.is_empty());
    }

    #[test]
    fn metadata_extracts_extern_info() {
        let source = r#"
extern fn compute(x: Int, y: Int) -> Int
    requires { y > 0 }
    effects { io }
"#;
        let typed = typecheck_ok(source);
        let meta = extract_metadata(&typed, "test.assura");

        assert!(!meta.contracts.is_empty());
        let c = &meta.contracts[0];
        assert_eq!(c.name, "compute");
        assert_eq!(c.kind, "extern");
        assert!(!c.requires.is_empty());
    }

    #[test]
    fn guidance_comment_includes_contract_info() {
        let source = r#"
contract Bounded {
    input(x: Int, max: Int)
    requires { x >= 0 }
    requires { x < max }
    ensures { result >= 0 }
}
"#;
        let (file, _) = assura_parser::parse(source);
        let file = file.unwrap();
        if let assura_ast::Decl::Contract(c) = &file.decls[0].node {
            let comment = implementation_guidance_comment(c);
            assert!(comment.contains("ASSURA CONTRACT: Bounded"));
            assert!(comment.contains("REQUIRES:"));
            assert!(comment.contains("ENSURES:"));
        } else {
            panic!("expected contract");
        }
    }

    #[test]
    fn metadata_serializes_to_json() {
        let source = r#"
contract Add {
    input(a: Int, b: Int)
    ensures { result == a + b }
}
"#;
        let typed = typecheck_ok(source);
        let meta = extract_metadata(&typed, "test.assura");
        let json = serde_json::to_string_pretty(&meta).unwrap();
        assert!(json.contains("\"name\": \"Add\""));
        assert!(json.contains("\"contracts\""));
        assert!(json.contains("\"ensures\""));
    }
}
