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

/// Parameters for the `check` MCP tool (parse + type-check + verify).
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct CheckParams {
    /// Assura source code to verify (inline). Provide either `source` or `file`.
    #[serde(default)]
    pub source: Option<String>,
    /// Path to an .assura file. Provide either `source` or `file`.
    #[serde(default)]
    pub file: Option<String>,
}

/// Parameters for the `infer` MCP tool (infer contracts from Rust source).
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct InferParams {
    /// Rust source code to infer contracts from (inline). Provide either `source` or `file`.
    #[serde(default)]
    pub source: Option<String>,
    /// Path to a Rust (.rs) file. Provide either `source` or `file`.
    #[serde(default)]
    pub file: Option<String>,
}

/// Parameters for the `explain` MCP tool (explain an error code).
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ExplainParams {
    /// Assura error code to explain (e.g. "A03001").
    pub code: String,
}

/// Parameters for the `type_map` MCP tool (Rust type -> Assura type mapping).
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct TypeMapParams {
    /// Rust type to map to an Assura type (e.g. "Vec<Option<i64>>").
    pub rust_type: String,
}

/// Parameters for the `ir_prompt` MCP tool (AI Implementation IR generation prompt).
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct IrPromptParams {
    /// Assura source (inline). Provide either `source` or `file`.
    #[serde(default)]
    pub source: Option<String>,
    /// Path to an .assura file. Provide either `source` or `file`.
    #[serde(default)]
    pub file: Option<String>,
    /// Declaration name (required when the file has multiple verification jobs).
    #[serde(default)]
    pub decl: Option<String>,
    /// Pattern overlay: auto, identity, arithmetic, length-copy, call-chain, bounds-check, field-access
    #[serde(default = "default_ir_pattern")]
    pub pattern: String,
}

fn default_ir_pattern() -> String {
    "auto".into()
}

/// Parameters for the `ir_verify` MCP tool (AI verification loop).
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct IrVerifyParams {
    /// Assura contract source (inline). Provide either `source` or `file`.
    #[serde(default)]
    pub source: Option<String>,
    /// Path to an .assura contract file. Provide either `source` or `file`.
    #[serde(default)]
    pub file: Option<String>,
    /// Implementation IR source text (inline). Provide either `ir` or `ir_file`.
    #[serde(default)]
    pub ir: Option<String>,
    /// Path to an .ir file. Provide either `ir` or `ir_file`.
    #[serde(default)]
    pub ir_file: Option<String>,
}

// ---------------------------------------------------------------------------
// Server
// ---------------------------------------------------------------------------

/// MCP server exposing Assura compiler tools to AI agents.
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
        let (source, filename) = match resolve_source_with_path(params.source, params.file) {
            Ok(v) => v,
            Err(e) => return e,
        };
        let result = run_check_pipeline(&source, &filename);
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

    #[tool(
        description = "Render an AI prompt to generate Implementation IR (.ir sidecar) for an Assura contract. Provide `source` or `file`, optional `decl`, and `pattern` (auto by default)."
    )]
    fn assura_ir_prompt(&self, Parameters(params): Parameters<IrPromptParams>) -> String {
        match render_ir_prompt_tool(params) {
            Ok(json) => json,
            Err(e) => e,
        }
    }

    #[tool(
        description = "Verify an Implementation IR against an Assura contract using SMT solvers (Z3/CVC5). Returns per-clause verification results with counterexamples and progress tracking. The core tool for the AI verification loop: generate IR, submit for verification, read feedback, fix IR, resubmit until all clauses verify. Provide contract via `source`/`file` and IR via `ir`/`ir_file`."
    )]
    fn assura_ir_verify(&self, Parameters(params): Parameters<IrVerifyParams>) -> String {
        let contract = match resolve_source_with_path(params.source, params.file) {
            Ok((s, _)) => s,
            Err(e) => return format!("{{\"status\":\"error\",\"compile_errors\":[\"{e}\"]}}"),
        };
        let ir = match resolve_source(params.ir, params.ir_file) {
            Ok(s) => s,
            Err(e) => return format!("{{\"status\":\"error\",\"ir_errors\":[\"{e}\"]}}"),
        };
        let config = assura_config::CompilerConfig::default();
        let result = assura_pipeline::verify_ir(&contract, &ir, &config);
        serde_json::to_string_pretty(&result).unwrap_or_default()
    }
}

#[tool_handler]
impl ServerHandler for AssuraMcpServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build()).with_instructions(
            "Assura contract-first AI-native language tools. Use assura_check to verify \
                 contracts, assura_infer to generate contracts from Rust code, assura_ir_prompt \
                 to generate Implementation IR prompts, assura_ir_verify to verify IR \
                 implementations against contracts (AI verification loop), assura_explain to \
                 look up error codes, and assura_type_map to convert Rust types.",
        )
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn resolve_source(inline: Option<String>, file: Option<String>) -> Result<String, String> {
    resolve_source_with_path(inline, file).map(|(s, _)| s)
}

fn resolve_source_with_path(
    inline: Option<String>,
    file: Option<String>,
) -> Result<(String, String), String> {
    match (inline, file) {
        (Some(s), _) => Ok((s, "<inline>".into())),
        (None, Some(path)) => {
            let content = std::fs::read_to_string(&path)
                .map_err(|e| format!("Failed to read {path}: {e}"))?;
            Ok((content, path))
        }
        (None, None) => Err("Provide either `source` (inline code) or `file` (path)".into()),
    }
}

fn run_check_pipeline(source: &str, filename: &str) -> assura_pipeline::PipelineResult {
    assura_pipeline::run_at(source, filename)
}

fn render_ir_prompt_tool(params: IrPromptParams) -> Result<String, String> {
    let (source, filename) = resolve_source_with_path(params.source, params.file)?;
    let pattern = params
        .pattern
        .parse::<assura_smt::IrPromptPattern>()
        .map_err(|()| {
            format!(
                "unknown pattern '{}': expected auto, identity, arithmetic, length-copy, \
             call-chain, bounds-check, or field-access",
                params.pattern
            )
        })?;

    let output = assura_pipeline::compile(
        &source,
        &filename,
        &assura_config::CompilerConfig::default(),
    );
    if output.has_errors {
        return Err(output
            .diagnostics
            .iter()
            .map(|d| d.to_string())
            .collect::<Vec<_>>()
            .join("\n"));
    }
    let typed = output
        .typed
        .ok_or_else(|| "type check produced no TypedFile".to_string())?;

    let path = std::path::Path::new(&filename);
    let contexts = assura_smt::ir_prompt_contexts_for_typed(&typed, Some(path));
    let jobs: Vec<_> = if let Some(ref name) = params.decl {
        let filtered: Vec<_> = contexts
            .into_iter()
            .filter(|c| &c.decl_name == name)
            .collect();
        if filtered.is_empty() {
            return Err(format!("no verification job named '{name}'"));
        }
        filtered
    } else if contexts.len() == 1 {
        contexts
    } else if contexts.is_empty() {
        Vec::new()
    } else {
        let names: Vec<_> = contexts.iter().map(|c| c.decl_name.as_str()).collect();
        return Err(format!(
            "file has {} verifiable declarations; pass `decl` to select one: {}",
            names.len(),
            names.join(", ")
        ));
    };

    if jobs.is_empty() {
        return Err("no verifiable declarations in file".into());
    }

    let suggested = jobs
        .first()
        .map(assura_smt::suggest_ir_pattern)
        .map(|p| p.as_str().to_string());
    let resolved_pattern = jobs.first().map(|ctx| {
        assura_smt::resolve_ir_pattern(ctx, pattern)
            .as_str()
            .to_string()
    });

    let prompts: Vec<serde_json::Value> = jobs
        .iter()
        .map(|ctx| {
            serde_json::json!({
                "decl": ctx.decl_name,
                "pattern": assura_smt::resolve_ir_pattern(ctx, pattern).as_str(),
                "prompt": assura_smt::render_ir_prompt(ctx, pattern),
            })
        })
        .collect();

    serde_json::to_string_pretty(&serde_json::json!({
        "file": filename,
        "suggested_pattern": suggested,
        "resolved_pattern": resolved_pattern,
        "prompts": prompts,
    }))
    .map_err(|e| e.to_string())
}

/// Lightweight contract inference from Rust source text.
fn infer_contracts_from_rust(source: &str) -> String {
    // Use the full assura-rust-analyzer parser (syn-based) instead of naive
    // line-by-line scanning. This handles multi-line signatures, generics,
    // impl blocks, and doc comment annotations.
    match assura_rust_analyzer::parse_rust_source(source) {
        Ok(items) if !items.is_empty() => {
            let mut output = String::new();
            for item in &items {
                let label = match &item.kind {
                    assura_rust_analyzer::AnnotatedItemKind::Function { name, .. } => {
                        format!("fn {name}")
                    }
                    assura_rust_analyzer::AnnotatedItemKind::Struct { name, .. } => {
                        format!("struct {name}")
                    }
                    assura_rust_analyzer::AnnotatedItemKind::ImplBlock { self_type, .. } => {
                        format!("impl {self_type}")
                    }
                };
                output.push_str(&format!("// {label} (line {})\n", item.line));
                for r in &item.contract.requires {
                    output.push_str(&format!("//   @requires {}\n", r.body));
                }
                for e in &item.contract.ensures {
                    output.push_str(&format!("//   @ensures {}\n", e.body));
                }
                output.push('\n');
            }
            output
        }
        Ok(_) => {
            // No annotated items found; fall back to function signature extraction
            extract_function_signatures(source)
        }
        Err(_) => {
            // syn parse failed; fall back to function signature extraction
            extract_function_signatures(source)
        }
    }
}

/// Fallback: extract function signatures from Rust source using simple parsing.
fn extract_function_signatures(source: &str) -> String {
    let mut output = String::new();
    for line in source.lines() {
        let trimmed = line.trim();
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

        let params_str = after_fn
            .find('(')
            .and_then(|start| {
                after_fn[start + 1..]
                    .find(')')
                    .map(|end| &after_fn[start + 1..start + 1 + end])
            })
            .unwrap_or("");

        let ret = after_fn
            .find("->")
            .map(|i| {
                let rest = after_fn[i + 2..].trim();
                let end = rest.find(['{', ';']).unwrap_or(rest.len());
                rest[..end].trim()
            })
            .unwrap_or("()");

        let assura_ret = assura_codegen::type_map::rust_type_to_assura(ret);
        // Emit real Assura clause syntax (not pseudo `input:` / `requires:` lines).
        output.push_str(&format!("contract {name} {{\n"));

        let mut inputs = Vec::new();
        for param in params_str.split(',') {
            let param = param.trim();
            if param.is_empty() || param.starts_with("&self") || param == "self" {
                continue;
            }
            if let Some((pname, ptype)) = param.split_once(':') {
                let pname = pname.trim();
                let ptype = assura_codegen::type_map::rust_type_to_assura(ptype.trim());
                inputs.push(format!("{pname}: {ptype}"));
            }
        }
        if inputs.is_empty() {
            output.push_str("    input()\n");
        } else {
            output.push_str(&format!("    input({})\n", inputs.join(", ")));
        }
        if assura_ret != "Unit" && assura_ret != "()" {
            output.push_str(&format!("    output(result: {assura_ret})\n"));
        }
        output.push_str("    requires { true }\n");
        output.push_str("    ensures { true }\n");
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
        let result = run_check_pipeline(source, "<inline>");
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
        let result = run_check_pipeline("", "<inline>");
        // Empty source should parse cleanly (no declarations, no errors)
        assert!(
            result.parse_errors.is_empty(),
            "empty source should produce no parse errors, got: {:?}",
            result.parse_errors
        );
        assert!(
            result.declarations.is_empty(),
            "empty source should produce no declarations, got: {:?}",
            result.declarations
        );
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
        let result = run_check_pipeline(source, "<inline>");
        assert!(result.parse_errors.is_empty());
        assert!(
            result.declarations.len() >= 2,
            "should have at least 2 declarations"
        );
        assert!(result.declarations.contains(&"contract Foo".to_string()));
        assert!(result.declarations.contains(&"contract Bar".to_string()));
    }

    #[test]
    fn ir_prompt_tool_renders_for_inline_contract() {
        let json = render_ir_prompt_tool(IrPromptParams {
            source: Some(
                "contract Echo {\n  input(x: Int)\n  output(result: Int)\n  ensures { result == x }\n}\n"
                    .into(),
            ),
            file: None,
            decl: None,
            pattern: "auto".into(),
        })
        .expect("prompt should render");
        assert!(json.contains("Instruction reference"));
        assert!(json.contains("Echo"));
        assert!(json.contains("pattern"));
    }

    #[test]
    fn check_pipeline_verification_results() {
        let source = "contract Simple {\n  input(x: Int)\n  output(result: Int)\n  ensures { result == x }\n}\n";
        let result = run_check_pipeline(source, "<inline>");
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
        let result = run_check_pipeline(source, "<inline>");
        let json_str = serde_json::to_string(&result).expect("result should serialize to JSON");
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
        assert!(
            result.contains("input(a: Int, b: Int)"),
            "should emit Assura input(...) syntax, got: {result}"
        );
        assert!(
            result.contains("output(result: Int)"),
            "should emit Assura output(...) syntax, got: {result}"
        );
        assert!(
            result.contains("requires { true }"),
            "should emit requires {{ }} clause, got: {result}"
        );
        // Must parse as real Assura (not pseudo-syntax).
        let pipe = run_check_pipeline(&result, "<infer>");
        assert!(
            pipe.parse_errors.is_empty(),
            "inferred contract must parse: {:?}",
            pipe.parse_errors
        );
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
        let server = AssuraMcpServer::new();
        let info = server.get_info();
        assert!(
            info.instructions.is_some(),
            "new() server should have instructions"
        );
    }

    #[test]
    fn server_default_creates_without_panic() {
        let server = AssuraMcpServer::default();
        let info = server.get_info();
        assert!(
            info.instructions.is_some(),
            "default() server should have instructions"
        );
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
            code: "A00000".into(),
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

    // -----------------------------------------------------------------------
    // assura_ir_verify tests (AI verification loop)
    // -----------------------------------------------------------------------

    #[test]
    fn tool_ir_verify_identity_verified() {
        let server = AssuraMcpServer::new();
        let params = IrVerifyParams {
            source: Some(
                "contract Echo {\n  input(x: Int)\n  output(result: Int)\n  ensures { result == x }\n}\n"
                    .into(),
            ),
            file: None,
            ir: Some(
                "module Echo {\n  fn #0 : ($0: Int) -> Int ! pure\n  {\n    $result = load $0 : Int\n  }\n}\n"
                    .into(),
            ),
            ir_file: None,
        };
        let result = server.assura_ir_verify(Parameters(params));
        assert!(
            result.contains("\"status\""),
            "should return JSON with status field"
        );
        assert!(
            result.contains("\"verified\""),
            "identity IR should verify: {result}"
        );
        assert!(
            result.contains("\"progress\""),
            "should include progress field"
        );
    }

    #[test]
    fn tool_ir_verify_bad_ir() {
        let server = AssuraMcpServer::new();
        let params = IrVerifyParams {
            source: Some("contract Echo {\n  input(x: Int)\n  output(result: Int)\n}\n".into()),
            file: None,
            ir: Some("not valid IR".into()),
            ir_file: None,
        };
        let result = server.assura_ir_verify(Parameters(params));
        assert!(
            result.contains("error"),
            "bad IR should produce error: {result}"
        );
    }

    #[test]
    fn tool_ir_verify_missing_params() {
        let server = AssuraMcpServer::new();
        let params = IrVerifyParams {
            source: None,
            file: None,
            ir: Some("module X { }".into()),
            ir_file: None,
        };
        let result = server.assura_ir_verify(Parameters(params));
        assert!(
            result.contains("error"),
            "missing contract should produce error: {result}"
        );
    }
}
