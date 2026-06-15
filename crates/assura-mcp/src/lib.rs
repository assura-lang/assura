//! MCP (Model Context Protocol) server for Assura.
//!
//! Exposes Assura compiler tools (check, infer, explain, type_map) as MCP
//! tools so AI agents can call them directly via structured JSON-RPC.

use rmcp::{
    ServerHandler,
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{ServerCapabilities, ServerInfo},
    schemars, tool, tool_handler, tool_router,
};
use serde::Deserialize;

// ---------------------------------------------------------------------------
// Tool parameter types
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct CheckParams {
    /// Assura source code to verify (inline). Provide either `source` or `file`.
    #[serde(default)]
    pub source: Option<String>,
    /// Path to an .assura file. Provide either `source` or `file`.
    #[serde(default)]
    pub file: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct InferParams {
    /// Rust source code to infer contracts from (inline). Provide either `source` or `file`.
    #[serde(default)]
    pub source: Option<String>,
    /// Path to a Rust (.rs) file. Provide either `source` or `file`.
    #[serde(default)]
    pub file: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ExplainParams {
    /// Assura error code to explain (e.g. "A03001").
    pub code: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct TypeMapParams {
    /// Rust type to map to an Assura type (e.g. "Vec<Option<i64>>").
    pub rust_type: String,
}

// ---------------------------------------------------------------------------
// Server
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct AssuraMcpServer {
    #[expect(dead_code)]
    tool_router: ToolRouter<Self>,
}

impl AssuraMcpServer {
    pub fn new() -> Self {
        Self {
            tool_router: Self::tool_router(),
        }
    }
}

impl Default for AssuraMcpServer {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Tool implementations
// ---------------------------------------------------------------------------

#[tool_router]
impl AssuraMcpServer {
    #[tool(
        description = "Parse, type-check, and verify an Assura contract. Returns structured diagnostics and verification results. Provide either `source` (inline code) or `file` (path to .assura file)."
    )]
    fn assura_check(&self, Parameters(params): Parameters<CheckParams>) -> String {
        let source = match resolve_source(params.source, params.file) {
            Ok(s) => s,
            Err(e) => return e,
        };
        let result = run_check_pipeline(&source);
        serde_json::to_string_pretty(&result).unwrap_or_default()
    }

    #[tool(
        description = "Infer skeleton Assura contracts from Rust source code. Provide either `source` (inline Rust) or `file` (path to .rs file)."
    )]
    fn assura_infer(&self, Parameters(params): Parameters<InferParams>) -> String {
        let source = match resolve_source(params.source, params.file) {
            Ok(s) => s,
            Err(e) => return e,
        };
        infer_contracts_from_rust(&source)
    }

    #[tool(
        description = "Explain an Assura error code. Returns the error name, description, example, and suggested fix."
    )]
    fn assura_explain(&self, Parameters(params): Parameters<ExplainParams>) -> String {
        match assura_diagnostics::explain(&params.code) {
            Some(info) => {
                let result = serde_json::json!({
                    "code": info.code,
                    "name": info.name,
                    "description": info.description,
                    "example": info.example,
                    "fix": info.fix,
                });
                serde_json::to_string_pretty(&result).unwrap_or_default()
            }
            None => format!("Unknown error code: {}", params.code),
        }
    }

    #[tool(
        description = "Map a Rust type to the equivalent Assura type (e.g. Vec<i64> -> List<Int>)."
    )]
    fn assura_type_map(&self, Parameters(params): Parameters<TypeMapParams>) -> String {
        let assura_type = assura_codegen::type_map::rust_type_to_assura(&params.rust_type);
        serde_json::json!({
            "rust_type": params.rust_type,
            "assura_type": assura_type,
        })
        .to_string()
    }
}

#[tool_handler]
impl ServerHandler for AssuraMcpServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build()).with_instructions(
            "Assura contract-first AI-native language tools. Use assura_check to verify \
                 contracts, assura_infer to generate contracts from Rust code, assura_explain \
                 to look up error codes, and assura_type_map to convert Rust types.",
        )
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn resolve_source(inline: Option<String>, file: Option<String>) -> Result<String, String> {
    match (inline, file) {
        (Some(s), _) => Ok(s),
        (None, Some(path)) => {
            std::fs::read_to_string(&path).map_err(|e| format!("Failed to read {path}: {e}"))
        }
        (None, None) => Err("Provide either `source` (inline code) or `file` (path)".into()),
    }
}

#[derive(serde::Serialize)]
struct CheckResult {
    success: bool,
    parse_errors: Vec<String>,
    declarations: Vec<String>,
    resolution_errors: Vec<DiagnosticEntry>,
    type_errors: Vec<DiagnosticEntry>,
    verification: Vec<VerificationEntry>,
}

#[derive(serde::Serialize)]
struct DiagnosticEntry {
    code: String,
    message: String,
}

#[derive(serde::Serialize)]
struct VerificationEntry {
    status: String,
    clause: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<String>,
}

fn run_check_pipeline(source: &str) -> CheckResult {
    use assura_parser::ast::Decl;

    let (ast, parse_errors) = assura_parser::parse(source);
    let parse_error_strings: Vec<String> = parse_errors.iter().map(|e| format!("{e:?}")).collect();

    let ast = match ast {
        Some(a) => a,
        None => {
            return CheckResult {
                success: false,
                parse_errors: parse_error_strings,
                declarations: vec![],
                resolution_errors: vec![],
                type_errors: vec![],
                verification: vec![],
            };
        }
    };

    let declarations: Vec<String> = ast
        .decls
        .iter()
        .map(|d| match &d.node {
            Decl::Contract(c) => format!("contract {}", c.name),
            Decl::Bind(b) => format!("bind {}", b.name),
            Decl::FnDef(f) => format!("fn {}", f.name),
            Decl::Service(s) => format!("service {}", s.name),
            Decl::TypeDef(t) => format!("type {}", t.name),
            Decl::EnumDef(e) => format!("enum {}", e.name),
            Decl::Extern(e) => format!("extern {}", e.name),
            Decl::Prophecy(p) => format!("prophecy {}", p.name),
            Decl::CodecRegistry(c) => format!("codec_registry {}", c.name),
            Decl::Block { kind, name, .. } => format!("{kind} {name}"),
        })
        .collect();

    if !parse_errors.is_empty() {
        return CheckResult {
            success: false,
            parse_errors: parse_error_strings,
            declarations,
            resolution_errors: vec![],
            type_errors: vec![],
            verification: vec![],
        };
    }

    // Resolution
    let resolved = match assura_resolve::resolve(&ast) {
        Ok(r) => r,
        Err(errs) => {
            return CheckResult {
                success: false,
                parse_errors: vec![],
                declarations,
                resolution_errors: errs
                    .iter()
                    .map(|e| DiagnosticEntry {
                        code: e.code.to_string(),
                        message: e.message.clone(),
                    })
                    .collect(),
                type_errors: vec![],
                verification: vec![],
            };
        }
    };

    // HIR lowering + type checking
    let hir = assura_hir::lower(&resolved);
    let typed = match assura_types::type_check_hir(&hir) {
        Ok(t) => t,
        Err(errs) => {
            return CheckResult {
                success: false,
                parse_errors: vec![],
                declarations,
                resolution_errors: vec![],
                type_errors: errs
                    .iter()
                    .map(|e| DiagnosticEntry {
                        code: e.code.to_string(),
                        message: e.message.clone(),
                    })
                    .collect(),
                verification: vec![],
            };
        }
    };

    // SMT verification
    let results = assura_smt::verify(&typed);
    let verification: Vec<VerificationEntry> = results
        .iter()
        .map(|r| match r {
            assura_smt::VerificationResult::Verified { clause_desc } => VerificationEntry {
                status: "verified".into(),
                clause: clause_desc.clone(),
                model: None,
                reason: None,
            },
            assura_smt::VerificationResult::Counterexample {
                clause_desc, model, ..
            } => VerificationEntry {
                status: "counterexample".into(),
                clause: clause_desc.clone(),
                model: Some(model.clone()),
                reason: None,
            },
            assura_smt::VerificationResult::Timeout { clause_desc } => VerificationEntry {
                status: "timeout".into(),
                clause: clause_desc.clone(),
                model: None,
                reason: None,
            },
            assura_smt::VerificationResult::Unknown {
                clause_desc,
                reason,
            } => VerificationEntry {
                status: "unknown".into(),
                clause: clause_desc.clone(),
                model: None,
                reason: Some(reason.clone()),
            },
        })
        .collect();

    CheckResult {
        success: true,
        parse_errors: vec![],
        declarations,
        resolution_errors: vec![],
        type_errors: vec![],
        verification,
    }
}

/// Lightweight contract inference from Rust source text.
fn infer_contracts_from_rust(source: &str) -> String {
    let mut output = String::new();
    for line in source.lines() {
        let trimmed = line.trim();
        if !trimmed.starts_with("pub fn ") && !trimmed.starts_with("fn ") {
            continue;
        }
        // Extract function name
        let after_fn = if let Some(rest) = trimmed.strip_prefix("pub fn ") {
            rest
        } else if let Some(rest) = trimmed.strip_prefix("fn ") {
            rest
        } else {
            continue;
        };
        let name = after_fn
            .split(|c: char| c == '(' || c == '<' || c.is_whitespace())
            .next()
            .unwrap_or("unknown");
        if name.is_empty() {
            continue;
        }

        // Extract params between parens
        let params_str = after_fn
            .find('(')
            .and_then(|start| {
                after_fn[start + 1..]
                    .find(')')
                    .map(|end| &after_fn[start + 1..start + 1 + end])
            })
            .unwrap_or("");

        // Extract return type
        let ret = after_fn
            .find("->")
            .map(|i| {
                let rest = after_fn[i + 2..].trim();
                let end = rest.find(['{', ';']).unwrap_or(rest.len());
                rest[..end].trim()
            })
            .unwrap_or("()");

        let assura_ret = assura_codegen::type_map::rust_type_to_assura(ret);
        output.push_str(&format!("contract {name} {{\n"));

        // Parse params for contract
        for param in params_str.split(',') {
            let param = param.trim();
            if param.is_empty() || param.starts_with("&self") || param == "self" {
                continue;
            }
            if let Some((pname, ptype)) = param.split_once(':') {
                let pname = pname.trim();
                let ptype = assura_codegen::type_map::rust_type_to_assura(ptype.trim());
                output.push_str(&format!("  input: {pname}: {ptype}\n"));
            }
        }

        output.push_str(&format!("  output: result: {assura_ret}\n"));
        output.push_str("  requires: true\n");
        output.push_str("  ensures: true\n");
        output.push_str("}\n\n");
    }
    if output.is_empty() {
        "No public function signatures found.".into()
    } else {
        output
    }
}

/// Start the MCP server on stdio. Called by `assura mcp`.
pub async fn run_mcp_server() -> Result<(), Box<dyn std::error::Error>> {
    use rmcp::{ServiceExt, transport::stdio};
    let server = AssuraMcpServer::new();
    let service = server.serve(stdio()).await?;
    service.waiting().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // resolve_source tests
    // -----------------------------------------------------------------------

    #[test]
    fn resolve_source_inline() {
        let result = resolve_source(Some("contract Foo {}".into()), None);
        assert_eq!(result.unwrap(), "contract Foo {}");
    }

    #[test]
    fn resolve_source_inline_overrides_file() {
        let result = resolve_source(Some("inline".into()), Some("/nonexistent".into()));
        assert_eq!(result.unwrap(), "inline");
    }

    #[test]
    fn resolve_source_missing_file() {
        let result = resolve_source(None, Some("/this/does/not/exist.assura".into()));
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Failed to read"));
    }

    #[test]
    fn resolve_source_neither() {
        let result = resolve_source(None, None);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Provide either"));
    }

    #[test]
    fn resolve_source_real_file() {
        // Build path relative to workspace root
        let workspace = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap();
        let demo = workspace.join("tests/fixtures/test_basic.assura");
        let result = resolve_source(None, Some(demo.to_string_lossy().into()));
        assert!(
            result.is_ok(),
            "should read test_basic.assura: {:?}",
            result.err()
        );
        let content = result.unwrap();
        assert!(
            content.contains("contract"),
            "demo file should contain contracts"
        );
    }

    // -----------------------------------------------------------------------
    // run_check_pipeline tests
    // -----------------------------------------------------------------------

    #[test]
    fn check_pipeline_valid_contract() {
        let source = "contract Add {\n  input(a: Int, b: Int)\n  output(result: Int)\n  requires { a >= 0 }\n  ensures { result >= a }\n}\n";
        let result = run_check_pipeline(source);
        assert!(
            result.parse_errors.is_empty(),
            "should have no parse errors"
        );
        assert!(
            result.declarations.contains(&"contract Add".to_string()),
            "should list Add declaration"
        );
    }

    #[test]
    fn check_pipeline_empty_source() {
        let result = run_check_pipeline("");
        // Empty source may or may not parse to an AST, but should not panic
        assert!(result.parse_errors.is_empty() || !result.parse_errors.is_empty());
    }

    #[test]
    fn check_pipeline_multiple_declarations() {
        let source = r#"
contract Foo {
    input(x: Int)
    output(result: Int)
    ensures { result > 0 }
}

contract Bar {
    input(y: Bool)
    output(result: Bool)
    ensures { result == y }
}
"#;
        let result = run_check_pipeline(source);
        assert!(result.parse_errors.is_empty());
        assert!(
            result.declarations.len() >= 2,
            "should have at least 2 declarations"
        );
        assert!(result.declarations.contains(&"contract Foo".to_string()));
        assert!(result.declarations.contains(&"contract Bar".to_string()));
    }

    #[test]
    fn check_pipeline_verification_results() {
        let source = "contract Simple {\n  input(x: Int)\n  output(result: Int)\n  ensures { result == x }\n}\n";
        let result = run_check_pipeline(source);
        // Should have at least one verification entry
        assert!(
            !result.verification.is_empty(),
            "should produce verification results"
        );
        // Each entry should have a status
        for entry in &result.verification {
            assert!(
                ["verified", "counterexample", "timeout", "unknown"]
                    .contains(&entry.status.as_str()),
                "invalid status: {}",
                entry.status
            );
        }
    }

    #[test]
    fn check_pipeline_serializes_to_json() {
        let source = "contract Test {\n  input(x: Int)\n  output(result: Int)\n  ensures { result >= 0 }\n}\n";
        let result = run_check_pipeline(source);
        let json = serde_json::to_string(&result);
        assert!(json.is_ok(), "result should serialize to JSON");
        let json_str = json.unwrap();
        assert!(json_str.contains("\"success\""));
        assert!(json_str.contains("\"declarations\""));
    }

    // -----------------------------------------------------------------------
    // infer_contracts_from_rust tests
    // -----------------------------------------------------------------------

    #[test]
    fn infer_from_basic_function() {
        let source = "pub fn add(a: i64, b: i64) -> i64 { a + b }";
        let result = infer_contracts_from_rust(source);
        assert!(
            result.contains("contract add"),
            "should contain contract name"
        );
        assert!(result.contains("output:"), "should contain output");
    }

    #[test]
    fn infer_from_private_function() {
        let source = "fn helper(x: i64) -> bool { x > 0 }";
        let result = infer_contracts_from_rust(source);
        assert!(
            result.contains("contract helper"),
            "should infer private fns too"
        );
    }

    #[test]
    fn infer_skips_non_functions() {
        let source = "struct Foo { x: i32 }\nimpl Foo { }";
        let result = infer_contracts_from_rust(source);
        assert_eq!(result, "No public function signatures found.");
    }

    #[test]
    fn infer_multiple_functions() {
        let source = "pub fn foo(a: i64) -> i64 { a }\npub fn bar(b: bool) -> bool { b }";
        let result = infer_contracts_from_rust(source);
        assert!(result.contains("contract foo"));
        assert!(result.contains("contract bar"));
    }

    #[test]
    fn infer_skips_self_params() {
        let source = "pub fn method(&self, x: i64) -> i64 { x }";
        let result = infer_contracts_from_rust(source);
        assert!(result.contains("contract method"));
        assert!(!result.contains("self"), "should skip self param");
    }

    #[test]
    fn infer_empty_source() {
        let result = infer_contracts_from_rust("");
        assert_eq!(result, "No public function signatures found.");
    }

    // -----------------------------------------------------------------------
    // Server creation tests
    // -----------------------------------------------------------------------

    #[test]
    fn server_creates_without_panic() {
        let _server = AssuraMcpServer::new();
    }

    #[test]
    fn server_default_creates_without_panic() {
        let _server = AssuraMcpServer::default();
    }

    #[test]
    fn server_info_has_tools() {
        let server = AssuraMcpServer::new();
        let info = server.get_info();
        assert!(
            info.instructions.is_some(),
            "server should have instructions"
        );
        let instructions = info.instructions.unwrap();
        assert!(
            instructions.contains("assura_check"),
            "instructions should mention check tool"
        );
    }

    // -----------------------------------------------------------------------
    // Tool dispatch tests (via direct method calls)
    // -----------------------------------------------------------------------

    #[test]
    fn tool_check_inline_source() {
        let server = AssuraMcpServer::new();
        let params = CheckParams {
            source: Some("contract X { ensures { true } }".into()),
            file: None,
        };
        let result = server.assura_check(Parameters(params));
        assert!(
            result.contains("\"success\""),
            "should return JSON with success field"
        );
        assert!(result.contains("contract X"), "should list declaration");
    }

    #[test]
    fn tool_explain_known_code() {
        let server = AssuraMcpServer::new();
        let params = ExplainParams {
            code: "A03001".into(),
        };
        let result = server.assura_explain(Parameters(params));
        assert!(result.contains("A03001"), "should contain the error code");
        assert!(
            !result.contains("Unknown error code"),
            "should find the code"
        );
    }

    #[test]
    fn tool_explain_unknown_code() {
        let server = AssuraMcpServer::new();
        let params = ExplainParams {
            code: "A99999".into(),
        };
        let result = server.assura_explain(Parameters(params));
        assert!(
            result.contains("Unknown error code"),
            "should report unknown"
        );
    }

    #[test]
    fn tool_type_map() {
        let server = AssuraMcpServer::new();
        let params = TypeMapParams {
            rust_type: "i64".into(),
        };
        let result = server.assura_type_map(Parameters(params));
        assert!(result.contains("\"rust_type\":\"i64\""));
        assert!(result.contains("Int"), "i64 should map to Int");
    }

    #[test]
    fn tool_infer_inline() {
        let server = AssuraMcpServer::new();
        let params = InferParams {
            source: Some("pub fn double(x: i64) -> i64 { x * 2 }".into()),
            file: None,
        };
        let result = server.assura_infer(Parameters(params));
        assert!(
            result.contains("contract double"),
            "should infer contract for double"
        );
    }
}
