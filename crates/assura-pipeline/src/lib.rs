//! Shared compiler pipeline for the Assura language.
//!
//! Provides a single `run()` function that executes the full
//! parse -> resolve -> HIR lower -> type check -> verify pipeline.
//! Used by the CLI, REPL, and MCP server to avoid duplicating
//! the pipeline chain.

use assura_parser::ast::Decl;

/// A diagnostic from any pipeline phase.
#[derive(Debug, Clone, serde::Serialize)]
pub struct PipelineDiagnostic {
    pub code: String,
    pub message: String,
}

/// A verification result entry.
#[derive(Debug, Clone, serde::Serialize)]
pub struct VerificationEntry {
    pub status: String,
    pub clause: String,
    pub model: Option<String>,
    pub reason: Option<String>,
}

/// The result of running the full compiler pipeline on a source string.
#[derive(Debug, Clone, serde::Serialize)]
pub struct PipelineResult {
    pub success: bool,
    pub declarations: Vec<String>,
    pub parse_errors: Vec<PipelineDiagnostic>,
    pub resolution_errors: Vec<PipelineDiagnostic>,
    pub type_errors: Vec<PipelineDiagnostic>,
    pub verification: Vec<VerificationEntry>,
}

impl PipelineResult {
    /// True if the pipeline produced any errors.
    pub fn has_errors(&self) -> bool {
        !self.parse_errors.is_empty()
            || !self.resolution_errors.is_empty()
            || !self.type_errors.is_empty()
    }
}

/// Extract a human-readable summary name from a declaration.
fn decl_summary(decl: &Decl) -> String {
    match decl {
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
    }
}

fn convert_verification(r: &assura_smt::VerificationResult) -> VerificationEntry {
    match r {
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
    }
}

/// Run the full compiler pipeline: parse -> resolve -> HIR -> typecheck -> verify.
///
/// Returns a structured result suitable for JSON serialization or
/// human-readable formatting.
pub fn run(source: &str) -> PipelineResult {
    let (ast, parse_errors) = assura_parser::parse(source);
    let parse_error_strs: Vec<PipelineDiagnostic> = parse_errors
        .iter()
        .map(|e| PipelineDiagnostic {
            code: String::new(),
            message: format!("{e:?}"),
        })
        .collect();

    let ast = match ast {
        Some(a) => a,
        None => {
            return PipelineResult {
                success: false,
                parse_errors: parse_error_strs,
                declarations: vec![],
                resolution_errors: vec![],
                type_errors: vec![],
                verification: vec![],
            };
        }
    };

    let declarations: Vec<String> = ast.decls.iter().map(|d| decl_summary(&d.node)).collect();

    if !parse_errors.is_empty() {
        return PipelineResult {
            success: false,
            parse_errors: parse_error_strs,
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
            return PipelineResult {
                success: false,
                parse_errors: vec![],
                declarations,
                resolution_errors: errs
                    .iter()
                    .map(|e| PipelineDiagnostic {
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
            return PipelineResult {
                success: false,
                parse_errors: vec![],
                declarations,
                resolution_errors: vec![],
                type_errors: errs
                    .iter()
                    .map(|e| PipelineDiagnostic {
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
    let verification: Vec<VerificationEntry> = results.iter().map(convert_verification).collect();

    let success = !results.iter().any(|r| {
        matches!(
            r,
            assura_smt::VerificationResult::Counterexample { .. }
                | assura_smt::VerificationResult::Timeout { .. }
        )
    });

    PipelineResult {
        success,
        parse_errors: vec![],
        declarations,
        resolution_errors: vec![],
        type_errors: vec![],
        verification,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_valid_contract() {
        let result = run(
            "contract SafeDiv {\n  input(x: Int, y: Int)\n  output(result: Int)\n  requires { y != 0 }\n  ensures { result > 0 }\n}",
        );
        assert!(result.parse_errors.is_empty());
        assert!(result.resolution_errors.is_empty());
        assert!(
            result.type_errors.is_empty(),
            "unexpected type errors: {:?}",
            result.type_errors
        );
        assert_eq!(result.declarations, vec!["contract SafeDiv"]);
        assert!(!result.verification.is_empty());
    }

    #[test]
    fn run_empty_source() {
        let result = run("");
        assert!(result.declarations.is_empty());
    }

    #[test]
    fn run_parse_error() {
        let result = run("contract Bad { @@@ }");
        assert!(!result.success);
    }

    #[test]
    fn run_multiple_declarations() {
        let result =
            run("contract A {\n  requires { true }\n}\ncontract B {\n  requires { true }\n}");
        assert_eq!(result.declarations.len(), 2);
    }

    #[test]
    fn run_has_errors_false_on_success() {
        let result = run("contract X {\n  requires { true }\n}");
        assert!(!result.has_errors());
    }

    #[test]
    fn run_has_errors_true_on_parse_error() {
        let result = run("contract { !!! }");
        assert!(result.has_errors());
    }

    #[test]
    fn run_serializes_to_json() {
        let result = run("contract T {\n  requires { true }\n}");
        let json = serde_json::to_string(&result);
        assert!(json.is_ok());
    }
}
